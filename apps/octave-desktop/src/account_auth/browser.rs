//! Isolated installed browser, fixed CDP commands, no JS injection/page IPC.
use super::Error;
use ring::rand::{SecureRandom, SystemRandom};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    net::{SocketAddr, TcpStream},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
use tungstenite::{Message, WebSocket, protocol::WebSocketConfig};
use zeroize::Zeroizing;

pub(super) struct Session {
    pub cookie: Zeroizing<String>,
    pub index: u8,
}
pub(super) fn music_url(url: &str) -> Option<u8> {
    let url = tungstenite::http::Uri::try_from(url).ok()?;
    if url.scheme_str() != Some("https") || url.authority()?.as_str() != "music.youtube.com" {
        return None;
    }
    let query = url.query().unwrap_or_default();
    let index = query
        .split('&')
        .find_map(|p| p.strip_prefix("authuser="))
        .unwrap_or("0")
        .parse::<u8>()
        .ok()?;
    (index < 100).then_some(index)
}
fn cookies(value: &Value) -> Result<Zeroizing<String>, Error> {
    let mut jar = BTreeMap::new();
    let cookies = value["cookies"].as_array().ok_or(Error::BrowserProtocol)?;
    if cookies.len() > 256 {
        return Err(Error::BrowserProtocol);
    }
    for cookie in cookies {
        let domain = cookie["domain"].as_str().unwrap_or_default();
        if !matches!(
            domain,
            ".youtube.com" | "youtube.com" | "music.youtube.com" | ".music.youtube.com"
        ) {
            continue;
        }
        let name = cookie["name"].as_str().ok_or(Error::BrowserProtocol)?;
        let value = cookie["value"].as_str().ok_or(Error::BrowserProtocol)?;
        if name.is_empty()
            || name.len() > 256
            || value.len() > 8192
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
            || value
                .bytes()
                .any(|b| !(0x21..=0x7e).contains(&b) || b == b';')
        {
            return Err(Error::BrowserProtocol);
        }
        if let Some(previous) = jar.insert(name.to_owned(), Zeroizing::new(value.to_owned()))
            && previous.as_str() != value
        {
            return Err(Error::BrowserProtocol);
        }
    }
    let mut result = Zeroizing::new(String::new());
    for (name, value) in jar {
        if !result.is_empty() {
            result.push_str("; ");
        }
        result.push_str(&name);
        result.push('=');
        result.push_str(&value);
    }
    if result.len() > 16384 {
        return Err(Error::BrowserProtocol);
    }
    Ok(result)
}
fn login_fingerprint(header: &str) -> Option<[u8; 32]> {
    let mut authenticating = Zeroizing::new(String::new());
    let mut sapisid = false;
    for (name, value) in header.split(';').filter_map(|p| p.trim().split_once('=')) {
        if matches!(name, "SAPISID" | "__Secure-1PAPISID" | "__Secure-3PAPISID")
            && !value.is_empty()
        {
            sapisid = true;
        }
        // SIDTS and other rotating request cookies must not prevent a stable
        // login from completing. Identity is verified at the API before use.
        if matches!(
            name,
            "SAPISID"
                | "SID"
                | "HSID"
                | "SSID"
                | "APISID"
                | "__Secure-1PSID"
                | "__Secure-3PSID"
                | "__Secure-1PAPISID"
                | "__Secure-3PAPISID"
        ) {
            authenticating.push_str(name);
            authenticating.push('=');
            authenticating.push_str(value);
            authenticating.push(';');
        }
    }
    sapisid.then(|| {
        ring::digest::digest(&ring::digest::SHA256, authenticating.as_bytes())
            .as_ref()
            .try_into()
            .unwrap()
    })
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Browser {
    Brave,
    Edge,
    Chrome,
}
impl Browser {
    pub const ALL: [Self; 3] = [Self::Brave, Self::Edge, Self::Chrome];
    pub fn label(self) -> &'static str {
        match self {
            Self::Brave => "Brave",
            Self::Edge => "Microsoft Edge",
            Self::Chrome => "Google Chrome",
        }
    }
    pub fn installed(self) -> bool {
        self.executable().is_some()
    }
    fn executable(self) -> Option<PathBuf> {
        #[cfg(target_os = "windows")]
        let candidates: &[&str] = match self {
            Self::Edge => &[
                r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
                r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
            ],
            Self::Brave => &[
                r"C:\Program Files\BraveSoftware\Brave-Browser\Application\brave.exe",
                r"C:\Program Files (x86)\BraveSoftware\Brave-Browser\Application\brave.exe",
            ],
            Self::Chrome => &[
                r"C:\Program Files\Google\Chrome\Application\chrome.exe",
                r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
            ],
        };
        #[cfg(target_os = "macos")]
        let candidates: &[&str] = match self {
            Self::Brave => &["/Applications/Brave Browser.app/Contents/MacOS/Brave Browser"],
            Self::Edge => &["/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge"],
            Self::Chrome => &["/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"],
        };
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        let candidates: &[&str] = match self {
            Self::Brave => &["/usr/bin/brave-browser", "/usr/bin/brave"],
            Self::Edge => &["/usr/bin/microsoft-edge"],
            Self::Chrome => &[
                "/usr/bin/google-chrome",
                "/usr/bin/chromium",
                "/usr/bin/chromium-browser",
            ],
        };
        candidates.iter().map(PathBuf::from).find(|p| p.is_file())
    }
}
impl Default for Browser {
    fn default() -> Self {
        Self::ALL
            .into_iter()
            .find(|b| b.installed())
            .unwrap_or(Self::Edge)
    }
}
struct Rpc {
    socket: WebSocket<TcpStream>,
    sequence: u64,
}
impl Rpc {
    fn open(profile: &Path) -> Result<Self, Error> {
        let metadata =
            fs::metadata(profile.join("DevToolsActivePort")).map_err(|_| Error::BrowserProtocol)?;
        if metadata.len() > 256 {
            return Err(Error::BrowserProtocol);
        }
        let raw = Zeroizing::new(
            fs::read_to_string(profile.join("DevToolsActivePort"))
                .map_err(|_| Error::BrowserProtocol)?,
        );
        let mut lines = raw.lines();
        let port = lines
            .next()
            .and_then(|s| s.parse::<u16>().ok())
            .filter(|p| *p >= 1024)
            .ok_or(Error::BrowserProtocol)?;
        let path = lines
            .next()
            .filter(|s| {
                s.starts_with("/devtools/browser/")
                    && s.len() < 100
                    && s.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'-'))
            })
            .ok_or(Error::BrowserProtocol)?;
        let address = SocketAddr::from(([127, 0, 0, 1], port));
        let stream = TcpStream::connect_timeout(&address, Duration::from_secs(1))
            .map_err(|_| Error::BrowserProtocol)?;
        stream
            .set_read_timeout(Some(Duration::from_secs(1)))
            .map_err(|_| Error::BrowserProtocol)?;
        stream
            .set_write_timeout(Some(Duration::from_secs(1)))
            .map_err(|_| Error::BrowserProtocol)?;
        let config = WebSocketConfig::default()
            .max_message_size(Some(1024 * 1024))
            .max_frame_size(Some(1024 * 1024));
        let (socket, _) = tungstenite::client::client_with_config(
            format!("ws://127.0.0.1:{port}{path}"),
            stream,
            Some(config),
        )
        .map_err(|_| Error::BrowserProtocol)?;
        Ok(Self {
            socket,
            sequence: 0,
        })
    }
    fn call(&mut self, method: &str, params: Value, session: Option<&str>) -> Result<Value, Error> {
        // All callers are native and method names are compile-time constants.
        if !matches!(
            method,
            "Target.getTargets" | "Target.attachToTarget" | "Network.getCookies" | "Browser.close"
        ) {
            return Err(Error::BrowserProtocol);
        }
        self.sequence += 1;
        let mut command = json!({"id":self.sequence,"method":method,"params":params});
        if let Some(session) = session {
            command["sessionId"] = json!(session);
        }
        self.socket
            .send(Message::text(command.to_string()))
            .map_err(|_| Error::BrowserProtocol)?;
        let start = Instant::now();
        for _ in 0..128 {
            if start.elapsed() > Duration::from_secs(2) {
                break;
            }
            match self.socket.read().map_err(|_| Error::BrowserProtocol)? {
                Message::Text(text) => {
                    let text = Zeroizing::new(text.to_string());
                    let mut value: Value =
                        serde_json::from_str(&text).map_err(|_| Error::BrowserProtocol)?;
                    if value["id"] == self.sequence {
                        if value.get("error").is_some() {
                            return Err(Error::BrowserProtocol);
                        }
                        return Ok(value["result"].take());
                    }
                }
                Message::Ping(_) => self.socket.flush().map_err(|_| Error::BrowserProtocol)?,
                Message::Close(_) => return Err(Error::Cancelled),
                _ => (),
            }
        }
        Err(Error::BrowserProtocol)
    }
}
struct OwnedBrowser {
    child: Child,
    executable: PathBuf,
    profile: PathBuf,
    root: PathBuf,
    rpc: Option<Rpc>,
    nonce: String,
    cleaned: bool,
}
fn browser_command(executable: &Path, profile: &Path, capture: bool) -> Command {
    let mut command = Command::new(executable);
    command.arg(format!("--user-data-dir={}", profile.display()));
    // Authentication is a human-only browser phase. Debugging is enabled only
    // after the user closes that window, for capture in the same owned profile.
    if capture {
        command.args([
            "--remote-debugging-address=127.0.0.1",
            "--remote-debugging-port=0",
        ]);
    }
    command.args([
        "--no-first-run",
        "--no-default-browser-check",
        "--disable-sync",
        "--disable-background-mode",
        "--new-window",
        "https://music.youtube.com/",
    ]);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}
impl OwnedBrowser {
    fn launch(root: &Path, browser: Browser) -> Result<Self, Error> {
        if root.ancestors().any(|p| p.join(".git").exists()) {
            return Err(Error::UnsafeProfile);
        }
        let executable = browser.executable().ok_or(Error::BrowserMissing)?;
        fs::create_dir_all(root).map_err(|_| Error::Storage)?;
        let root = root.canonicalize().map_err(|_| Error::Storage)?;
        if root.ancestors().any(|p| p.join(".git").exists()) {
            return Err(Error::UnsafeProfile);
        }
        let mut bytes = [0; 16];
        SystemRandom::new()
            .fill(&mut bytes)
            .map_err(|_| Error::Storage)?;
        let nonce: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        let profile = root.join(format!("signin-{nonce}"));
        fs::create_dir(&profile).map_err(|_| Error::Storage)?;
        fs::write(profile.join("OCTAVE_OWNED_PROFILE"), &nonce).map_err(|_| Error::Storage)?;
        let child = browser_command(&executable, &profile, false)
            .spawn()
            .map_err(|_| Error::BrowserMissing)?;
        Ok(Self {
            child,
            executable,
            profile,
            root,
            rpc: None,
            nonce,
            cleaned: false,
        })
    }
    fn start_capture(&mut self) -> Result<(), Error> {
        if self
            .child
            .try_wait()
            .map_err(|_| Error::BrowserProtocol)?
            .is_none()
        {
            return Err(Error::BrowserProtocol);
        }
        let resolved = self
            .profile
            .canonicalize()
            .map_err(|_| Error::UnsafeProfile)?;
        if resolved.parent() != Some(self.root.as_path())
            || fs::read_to_string(resolved.join("OCTAVE_OWNED_PROFILE"))
                .map_err(|_| Error::UnsafeProfile)?
                != self.nonce
        {
            return Err(Error::UnsafeProfile);
        }
        self.child = browser_command(&self.executable, &resolved, true)
            .spawn()
            .map_err(|_| Error::BrowserMissing)?;
        Ok(())
    }
    fn cleanup(&mut self) -> Result<(), Error> {
        if self.cleaned {
            return Ok(());
        }
        if let Some(rpc) = &mut self.rpc {
            let _ = rpc.call("Browser.close", json!({}), None);
        }
        let stop = Instant::now();
        while self.child.try_wait().map_err(|_| Error::Cleanup)?.is_none()
            && stop.elapsed() < Duration::from_secs(3)
        {
            thread::sleep(Duration::from_millis(50));
        }
        if self.child.try_wait().map_err(|_| Error::Cleanup)?.is_none() {
            self.child.kill().map_err(|_| Error::Cleanup)?;
            self.child.wait().map_err(|_| Error::Cleanup)?;
        }
        // Resolve and verify the exact owned child before recursive removal.
        let resolved = self.profile.canonicalize().map_err(|_| Error::Cleanup)?;
        if resolved.parent() != Some(self.root.as_path())
            || fs::read_to_string(resolved.join("OCTAVE_OWNED_PROFILE"))
                .map_err(|_| Error::Cleanup)?
                != self.nonce
        {
            return Err(Error::Cleanup);
        }
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            match fs::remove_dir_all(&resolved) {
                Ok(()) => {
                    self.cleaned = true;
                    return Ok(());
                }
                Err(_) if Instant::now() < deadline => thread::sleep(Duration::from_millis(100)),
                Err(_) => return Err(Error::Cleanup),
            }
        }
    }
}
impl Drop for OwnedBrowser {
    fn drop(&mut self) {
        if !self.cleaned {
            let _ = self.cleanup();
        }
    }
}
pub(super) fn sign_in(
    root: &Path,
    cancelled: Arc<AtomicBool>,
    browser: Browser,
) -> Result<Session, Error> {
    cleanup_pending(root)?;
    let mut browser = OwnedBrowser::launch(root, browser)?;
    let start = Instant::now();
    let mut attached = None::<(String, String)>;
    let mut stable = None::<([u8; 32], u8)>;
    let result = (|| {
        // No CDP endpoint, cookie reads or browser automation during sign-in.
        // The user completes Google authentication and closes the owned window.
        while browser
            .child
            .try_wait()
            .map_err(|_| Error::BrowserProtocol)?
            .is_none()
        {
            if cancelled.load(Ordering::Acquire) {
                return Err(Error::Cancelled);
            }
            if start.elapsed() > Duration::from_secs(900) {
                return Err(Error::SignInTimeout);
            }
            thread::sleep(Duration::from_millis(100));
        }
        if cancelled.load(Ordering::Acquire) {
            return Err(Error::Cancelled);
        }
        browser.start_capture()?;
        let capture_start = Instant::now();
        loop {
            if cancelled.load(Ordering::Acquire) {
                return Err(Error::Cancelled);
            }
            if capture_start.elapsed() > Duration::from_secs(20) {
                return Err(Error::CaptureIncomplete);
            }
            if browser
                .child
                .try_wait()
                .map_err(|_| Error::BrowserProtocol)?
                .is_some()
            {
                return Err(Error::Cancelled);
            }
            if browser.rpc.is_none() {
                if browser.profile.join("DevToolsActivePort").is_file() {
                    match Rpc::open(&browser.profile) {
                        Ok(rpc) => browser.rpc = Some(rpc),
                        Err(_) if capture_start.elapsed() < Duration::from_secs(15) => {
                            thread::sleep(Duration::from_millis(100));
                            continue;
                        }
                        Err(error) => return Err(error),
                    }
                } else {
                    thread::sleep(Duration::from_millis(100));
                    continue;
                }
            }
            let rpc = browser.rpc.as_mut().unwrap();
            let targets = rpc.call("Target.getTargets", json!({}), None)?;
            let Some(target) = targets["targetInfos"].as_array().and_then(|a| {
                a.iter().find(|v| {
                    v["type"] == "page" && v["url"].as_str().and_then(music_url).is_some()
                })
            }) else {
                thread::sleep(Duration::from_millis(250));
                continue;
            };
            let index = target["url"]
                .as_str()
                .and_then(music_url)
                .ok_or(Error::BrowserProtocol)?;
            let id = target["targetId"]
                .as_str()
                .filter(|s| s.len() < 128)
                .ok_or(Error::BrowserProtocol)?;
            if attached.as_ref().is_none_or(|(previous, _)| previous != id) {
                let response = rpc.call(
                    "Target.attachToTarget",
                    json!({"targetId":id,"flatten":true}),
                    None,
                )?;
                attached = Some((
                    id.to_owned(),
                    response["sessionId"]
                        .as_str()
                        .filter(|s| s.len() < 128)
                        .ok_or(Error::BrowserProtocol)?
                        .to_owned(),
                ));
            }
            let mut response = rpc.call(
                "Network.getCookies",
                json!({"urls":["https://music.youtube.com/"]}),
                attached.as_ref().map(|(_, session)| session.as_str()),
            )?;
            let header = cookies(&response)?;
            // Zeroize owned cookie strings in the parsed protocol tree.
            if let Some(items) = response["cookies"].as_array_mut() {
                for item in items {
                    if let Value::String(text) = item["value"].take() {
                        drop(Zeroizing::new(text));
                    }
                }
            }
            if let Some(fingerprint) = login_fingerprint(&header) {
                if stable
                    .as_ref()
                    .is_some_and(|(prior, old_index)| *prior == fingerprint && *old_index == index)
                {
                    return Ok(Session {
                        cookie: header,
                        index,
                    });
                }
                stable = Some((fingerprint, index));
            } else {
                stable = None;
            }
            thread::sleep(Duration::from_millis(500));
        }
    })();
    let cleanup = browser.cleanup();
    if cleanup.is_err() {
        return Err(Error::Cleanup);
    }
    result
}

pub(super) fn cleanup_pending(root: &Path) -> Result<(), Error> {
    if !root.exists() {
        return Ok(());
    }
    let root = root.canonicalize().map_err(|_| Error::Cleanup)?;
    if root.ancestors().any(|p| p.join(".git").exists()) {
        return Err(Error::UnsafeProfile);
    }
    for entry in fs::read_dir(&root).map_err(|_| Error::Cleanup)? {
        let entry = entry.map_err(|_| Error::Cleanup)?;
        let name = entry.file_name();
        let name = name.to_str().ok_or(Error::Cleanup)?;
        let Some(nonce) = name.strip_prefix("signin-") else {
            continue;
        };
        if nonce.len() != 32
            || !nonce.bytes().all(|b| b.is_ascii_hexdigit())
            || !entry.file_type().map_err(|_| Error::Cleanup)?.is_dir()
        {
            return Err(Error::Cleanup);
        }
        let path = entry.path().canonicalize().map_err(|_| Error::Cleanup)?;
        if path.parent() != Some(root.as_path())
            || fs::read_to_string(path.join("OCTAVE_OWNED_PROFILE")).map_err(|_| Error::Cleanup)?
                != nonce
        {
            return Err(Error::Cleanup);
        }
        if path.join("DevToolsActivePort").is_file()
            && let Ok(mut rpc) = Rpc::open(&path)
        {
            let _ = rpc.call("Browser.close", json!({}), None);
        }
        let deadline = Instant::now() + Duration::from_secs(4);
        loop {
            match fs::remove_dir_all(&path) {
                Ok(()) => break,
                Err(_) if Instant::now() < deadline => thread::sleep(Duration::from_millis(100)),
                Err(_) => return Err(Error::Cleanup),
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rotating_request_cookies_do_not_hide_a_stable_login() {
        let a = login_fingerprint("SAPISID=synthetic; SID=fixed; __Secure-1PSIDTS=old");
        assert!(a.is_some());
        assert_eq!(
            a,
            login_fingerprint("SAPISID=synthetic; SID=fixed; __Secure-1PSIDTS=new")
        );
        assert_ne!(
            a,
            login_fingerprint("SAPISID=different; SID=fixed; __Secure-1PSIDTS=new")
        );
        assert_eq!(None, login_fingerprint("VISITOR_INFO1_LIVE=guest"));
    }
    #[test]
    fn human_signin_has_no_debugging_or_security_override_arguments() {
        let profile = Path::new("synthetic-owned-profile");
        let signin = browser_command(Path::new("browser"), profile, false);
        let args: Vec<_> = signin.get_args().map(|s| s.to_string_lossy()).collect();
        assert!(
            args.iter()
                .any(|s| s == "--user-data-dir=synthetic-owned-profile")
        );
        assert!(!args.iter().any(|s| s.contains("debugging")
            || s.contains("automation")
            || s.contains("disable-web-security")
            || s.contains("user-agent")));
        let capture = browser_command(Path::new("browser"), profile, true);
        assert!(
            capture
                .get_args()
                .any(|s| s == "--remote-debugging-address=127.0.0.1")
        );
    }
    #[test]
    fn capture_rejects_foreign_origins_and_cookie_injection() {
        assert_eq!(music_url("https://music.youtube.com/?authuser=2"), Some(2));
        for url in [
            "https://music.youtube.com.evil.test/",
            "http://music.youtube.com/",
            "https://user@music.youtube.com/",
            "https://music.youtube.com:444/",
            "file:///music.youtube.com",
            "https://music.youtube.com/?authuser=100",
        ] {
            assert_eq!(music_url(url), None);
        }
        let out=cookies(&json!({"cookies":[{"domain":".youtube.com","name":"SAPISID","value":"synthetic"},{"domain":"accounts.google.com","name":"SID","value":"foreign"}]})).unwrap();
        assert_eq!(out.as_str(), "SAPISID=synthetic");
        assert!(cookies(&json!({"cookies":[{"domain":".youtube.com","name":"SAPISID","value":"bad\r\nHeader: data"}]})).is_err());
    }
    #[test]
    #[ignore = "Explicit installed-browser isolation/CDP/cleanup test; never reads cookies or signs in"]
    fn isolated_browser_launches_connects_and_cleans_without_reading_credentials() {
        let root = PathBuf::from(
            std::env::var_os("OCTAVE_TEST_DIR").expect("Set external OCTAVE_TEST_DIR"),
        )
        .join("browser-smoke");
        let mut browser = OwnedBrowser::launch(&root, Browser::default()).unwrap();
        assert!(
            browser.start_capture().is_err(),
            "Capture must wait for the human window to exit"
        );
        // Only the disposable unsigned smoke process is stopped here. Production
        // waits for the user to close the normal sign-in window themselves.
        browser.child.kill().unwrap();
        browser.child.wait().unwrap();
        browser.start_capture().unwrap();
        let start = Instant::now();
        loop {
            if let Ok(rpc) = Rpc::open(&browser.profile) {
                browser.rpc = Some(rpc);
                break;
            }
            assert!(start.elapsed() < Duration::from_secs(30));
            thread::sleep(Duration::from_millis(100));
        }
        let targets = browser
            .rpc
            .as_mut()
            .unwrap()
            .call("Target.getTargets", json!({}), None)
            .unwrap();
        assert!(targets["targetInfos"].as_array().is_some());
        browser.cleanup().unwrap();
        assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
        fs::remove_dir(&root).unwrap();
    }
    #[test]
    fn cleanup_refuses_an_unowned_directory() {
        let root = std::env::var_os("OCTAVE_TEST_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join(format!("cleanup-{}", std::process::id()));
        let child = root.join(format!("signin-{}", "a".repeat(32)));
        fs::create_dir_all(&child).unwrap();
        fs::write(child.join("OCTAVE_OWNED_PROFILE"), "does-not-match").unwrap();
        fs::write(child.join("sentinel"), "keep").unwrap();
        assert_eq!(cleanup_pending(&root), Err(Error::Cleanup));
        assert!(child.join("sentinel").exists());
        fs::remove_file(child.join("sentinel")).unwrap();
        fs::remove_file(child.join("OCTAVE_OWNED_PROFILE")).unwrap();
        fs::remove_dir(child).unwrap();
        fs::remove_dir(root).unwrap();
    }
}
