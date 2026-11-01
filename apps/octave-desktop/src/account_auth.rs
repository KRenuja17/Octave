//! Serialized native account lifecycle. Secrets never enter settings/SQLite/logs.
mod browser;
mod vault;
pub use browser::Browser;
use octave_provider::account::{Account, Channel, Identity};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};

fn outside_checkout(path: &std::path::Path) -> Result<(), Error> {
    // Resolve an existing ancestor before checking Git. Lexical ancestors of
    // checkout/../external otherwise include checkout and falsely reject CI's
    // external test directory. Recheck the created canonical root before use.
    let mut existing = path;
    while !existing.exists() {
        existing = existing.parent().ok_or(Error::UnsafeProfile)?;
    }
    let existing = existing.canonicalize().map_err(|_| Error::Storage)?;
    if existing.ancestors().any(|p| p.join(".git").exists()) {
        return Err(Error::UnsafeProfile);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    BrowserMissing,
    BrowserProtocol,
    CapturePages,
    CaptureAttach,
    CaptureCookies,
    CookieFormat,
    CookieAmbiguous,
    SignInTimeout,
    Cancelled,
    Cleanup,
    Storage,
    UnsafeProfile,
    Vault,
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    VaultUnavailable,
    CaptureIncomplete,
    CaptureEndpointMissing,
    CapturePageMissing,
    CaptureCookieMissing,
    Account(octave_provider::account::Error),
    ChannelDiscovery(octave_provider::account::Error),
    IdentityChanged,
    Busy,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Self::Account(reason) = self {
            return write!(f, "Account validation failed: {reason}");
        }
        if let Self::ChannelDiscovery(reason) = self {
            return write!(f, "Channel discovery failed: {reason}");
        }
        f.write_str(match self{
        Self::BrowserMissing=>"The selected Brave/Edge/Chrome browser was not found. Choose an installed browser.",
        Self::BrowserProtocol=>"The isolated sign-in browser connection failed. Close it and retry.",
        Self::CapturePages=>"The isolated browser could not list its capture pages.",
        Self::CaptureAttach=>"The YouTube Music capture page changed before it could be attached. Retry sign-in.",
        Self::CaptureCookies=>"The isolated browser refused the native YouTube cookie read.",
        Self::CookieFormat=>"The YouTube cookie response was malformed or exceeded the native limit.",
        Self::CookieAmbiguous=>"The isolated profile returned conflicting login cookies at the same scope. No account was connected.",
        Self::SignInTimeout=>"Sign-in timed out. The temporary browser was closed.",
        Self::Cancelled=>"Account operation cancelled.",
        Self::Cleanup=>"Temporary browser cleanup is incomplete. Close the Octave sign-in browser, then retry cleanup.",
        Self::Storage=>"Account storage could not be read, written or removed. No plaintext fallback was used.",
        Self::UnsafeProfile=>"Account data must be outside a Git checkout and use an app-owned directory.",
        Self::Vault=>"The OS credential store operation failed. Remember/Forget did not complete.",
        #[cfg(not(any(target_os="windows",target_os="macos")))]
        Self::VaultUnavailable=>"Protected account persistence is unavailable on this platform.",
        Self::CaptureIncomplete=>"The browser did not provide a completed YouTube session. Finish sign-in and wait for Music to load before closing the window.",
        Self::CaptureEndpointMissing=>"The isolated browser did not open its native capture endpoint.",
        Self::CapturePageMissing=>"The isolated capture browser did not expose a YouTube Music page with a supported account index.",
        Self::CaptureCookieMissing=>"The isolated profile did not provide a YouTube login cookie after reopening. No account was connected.",
        Self::Account(_) | Self::ChannelDiscovery(_)=>unreachable!(),
        Self::IdentityChanged=>"The account or channel changed. The refresh was rejected; sign in again.",
        Self::Busy=>"The account worker could not accept the operation. Wait for cleanup to finish, then retry.",
    })
    }
}
pub enum Event {
    CancelledCleanly,
    Ready(Account, Identity, Vec<Channel>),
    Saved(u64),
    Forgotten,
    Failed(Error),
}
enum Job {
    SignIn(Browser),
    Restore,
    Verify(Account),
    Select(Account, Channel),
    Save(Account, Identity),
    Forget,
}
struct Command {
    epoch: u64,
    cancelled: Arc<AtomicBool>,
    job: Job,
}
pub struct Auth {
    root: Option<PathBuf>,
    tx: Option<mpsc::SyncSender<Command>>,
    rx: Option<mpsc::Receiver<(u64, Event)>>,
    done: Option<mpsc::Receiver<()>>,
    cancelled: Arc<AtomicBool>,
    epoch: u64,
    pub pending: bool,
    submission_error: Option<Error>,
}
impl Default for Auth {
    fn default() -> Self {
        Self {
            root: None,
            tx: None,
            rx: None,
            done: None,
            cancelled: Arc::new(AtomicBool::new(false)),
            epoch: 0,
            pending: false,
            submission_error: None,
        }
    }
}
impl Auth {
    pub fn available(&self) -> bool {
        self.root.is_some()
    }
    pub fn at(root: PathBuf) -> Self {
        let mut auth = Self::default();
        auth.root = Some(root.join("account"));
        auth
    }
    pub fn saved(&self) -> bool {
        self.root
            .as_ref()
            .is_some_and(|r| r.join("session.sealed").is_file())
    }
    pub fn cancel(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        self.epoch += 1;
        self.pending = false;
        self.submission_error = None;
        self.cancelled = Arc::new(AtomicBool::new(false));
    }
    fn submit(&mut self, job: Job) {
        self.cancel();
        if self.tx.is_none() {
            let Some(root) = self.root.clone() else {
                return;
            };
            let (tx, commands) = mpsc::sync_channel::<Command>(4);
            let (results, rx) = mpsc::channel();
            let (done_tx, done) = mpsc::channel();
            self.tx = Some(tx);
            self.rx = Some(rx);
            self.done = Some(done);
            std::thread::spawn(move || {
                let store = vault::Store::new(root.clone());
                while let Ok(command) = commands.recv() {
                    if command.cancelled.load(Ordering::Acquire)
                        && !matches!(command.job, Job::Forget)
                    {
                        continue;
                    }
                    let result = execute(command.job, &root, &store, command.cancelled.clone());
                    if command.cancelled.load(Ordering::Acquire) {
                        let cleanup = if matches!(result, Ok(Event::Saved(_))) {
                            store.forget()
                        } else {
                            Ok(())
                        };
                        if let Ok(Event::Ready(account, _, _)) = &result {
                            account.disconnect();
                        }
                        let _ = results.send((
                            command.epoch,
                            if let Err(error) = cleanup {
                                Event::Failed(error)
                            } else if matches!(result, Err(Error::Cleanup)) {
                                Event::Failed(Error::Cleanup)
                            } else {
                                Event::CancelledCleanly
                            },
                        ));
                        continue;
                    }
                    let _ = results.send((command.epoch, result.unwrap_or_else(Event::Failed)));
                }
                let _ = done_tx.send(());
            });
        }
        if self.tx.as_ref().is_some_and(|tx| {
            tx.try_send(Command {
                epoch: self.epoch,
                cancelled: self.cancelled.clone(),
                job,
            })
            .is_ok()
        }) {
            self.pending = true;
        } else {
            self.submission_error = Some(Error::Busy);
        }
    }
    pub fn sign_in(&mut self, browser: Browser) {
        self.submit(Job::SignIn(browser));
    }
    pub fn restore(&mut self) {
        self.submit(Job::Restore);
    }
    pub fn verify(&mut self, a: Account) {
        self.submit(Job::Verify(a));
    }
    pub fn select(&mut self, a: Account, c: Channel) {
        self.submit(Job::Select(a, c));
    }
    pub fn save(&mut self, a: Account, i: Identity) {
        self.submit(Job::Save(a, i));
    }
    pub fn forget(&mut self) {
        self.submit(Job::Forget);
    }
    pub fn poll(&mut self) -> Option<Event> {
        if let Some(error) = self.submission_error.take() {
            return Some(Event::Failed(error));
        }
        while let Some(Ok((epoch, event))) = self.rx.as_ref().map(|rx| rx.try_recv()) {
            if epoch != self.epoch {
                if matches!(
                    event,
                    Event::Failed(Error::Cleanup | Error::Vault | Error::Storage)
                ) {
                    return Some(event);
                }
                if !self.pending && matches!(event, Event::CancelledCleanly) {
                    return Some(event);
                }
                if let Event::Ready(a, _, _) = event {
                    a.disconnect();
                }
                continue;
            }
            self.pending = false;
            // A completed account owns its revocation flag. Future vault jobs
            // must not revoke it by cancelling the preceding completed job.
            self.cancelled = Arc::new(AtomicBool::new(false));
            return Some(event);
        }
        None
    }
}
impl Drop for Auth {
    fn drop(&mut self) {
        self.cancel();
        self.tx.take();
        if let Some(done) = self.done.take() {
            let _ = done.recv_timeout(Duration::from_secs(8));
        }
    }
}
fn execute(
    job: Job,
    root: &std::path::Path,
    store: &vault::Store,
    cancelled: Arc<AtomicBool>,
) -> Result<Event, Error> {
    let ready = |account: Account, expected: Option<String>| -> Result<Event, Error> {
        let identity = account.verify().map_err(Error::Account)?;
        #[cfg(test)]
        eprintln!("account_identity_validation=passed");
        if expected.is_some_and(|id| id != identity.id) {
            account.disconnect();
            return Err(Error::IdentityChanged);
        }
        if cancelled.load(Ordering::Acquire) {
            account.disconnect();
            return Err(Error::Cancelled);
        }
        let mut channels = account.channels().map_err(Error::ChannelDiscovery)?;
        if channels.is_empty() {
            channels.push(Channel {
                id: identity.id.clone(),
                name: identity.name.clone(),
                selected: true,
            });
        }
        Ok(Event::Ready(account, identity, channels))
    };
    match job {
        Job::SignIn(browser) => {
            let session = browser::sign_in(&root.join("browser"), cancelled.clone(), browser)?;
            #[cfg(test)]
            eprintln!("browser_session_capture_and_profile_cleanup=passed");
            let account = Account::with_cancellation(
                session.cookie.to_string(),
                session.index,
                cancelled.clone(),
            )
            .map_err(Error::Account)?;
            ready(account, None)
        }
        Job::Restore => {
            let (account, expected) = store.restore()?;
            ready(account, Some(expected))
        }
        Job::Verify(account) => {
            let expected = account.selected_id();
            let event = ready(account.clone(), expected)?;
            if store.exists()
                && let Event::Ready(_, identity, _) = &event
            {
                store.save(&account, identity)?;
            }
            Ok(event)
        }
        Job::Select(account, channel) => {
            let (next, _) = account.select_channel(&channel).map_err(Error::Account)?;
            let event = ready(next.clone(), Some(channel.id))?;
            if store.exists()
                && let Event::Ready(_, identity, _) = &event
            {
                store.save(&next, identity)?;
            }
            Ok(event)
        }
        Job::Save(account, identity) => {
            let revision = account.session_revision();
            store.save(&account, &identity)?;
            Ok(Event::Saved(revision))
        }
        Job::Forget => {
            let browser = browser::cleanup_pending(&root.join("browser"));
            let vault = store.forget();
            browser?;
            vault?;
            Ok(Event::Forgotten)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_full_worker_queue_reports_failure_instead_of_silently_losing_cleanup() {
        let mut auth = Auth::default();
        let (tx, _commands) = mpsc::sync_channel(1);
        tx.try_send(Command {
            epoch: 0,
            cancelled: Arc::new(AtomicBool::new(false)),
            job: Job::Forget,
        })
        .unwrap_or_else(|_| panic!("Synthetic queue setup failed"));
        auth.tx = Some(tx);
        auth.forget();
        assert!(!auth.pending);
        assert!(matches!(auth.poll(), Some(Event::Failed(Error::Busy))));
    }
    #[test]
    fn cancelled_generation_revokes_late_credentials_instead_of_activating_them() {
        let mut auth = Auth::default();
        let (tx, rx) = mpsc::channel();
        auth.rx = Some(rx);
        let account = Account::new("SAPISID=synthetic-late-result".into(), 0).unwrap();
        let observer = account.clone();
        let identity = Identity {
            id: "fixture".into(),
            name: "Fixture".into(),
            channel_id: None,
            handle: None,
            premium: None,
        };
        tx.send((auth.epoch, Event::Ready(account, identity, Vec::new())))
            .unwrap();
        auth.cancel();
        assert!(auth.poll().is_none());
        assert!(!observer.is_active());
    }
    #[test]
    fn completing_auth_does_not_let_a_future_vault_job_cancel_the_account() {
        let mut auth = Auth::default();
        let (tx, rx) = mpsc::channel();
        auth.rx = Some(rx);
        let account =
            Account::with_cancellation("SAPISID=synthetic".into(), 0, auth.cancelled.clone())
                .unwrap();
        let observer = account.clone();
        let identity = Identity {
            id: "fixture".into(),
            name: "Fixture".into(),
            channel_id: None,
            handle: None,
            premium: None,
        };
        tx.send((auth.epoch, Event::Ready(account, identity, Vec::new())))
            .unwrap();
        assert!(matches!(auth.poll(), Some(Event::Ready(..))));
        auth.cancel();
        assert!(observer.is_active());
    }
    #[test]
    #[ignore = "Attended Google sign-in and read-only account/Premium playback at zero volume; explicit opt-in required"]
    fn live_attended_account_and_premium() {
        assert_eq!(
            std::env::var("OCTAVE_LIVE_ACCOUNT").as_deref(),
            Ok("1"),
            "Explicit live account opt-in is required"
        );
        let root = PathBuf::from(
            std::env::var_os("OCTAVE_TEST_DIR").expect("Set an external test directory"),
        )
        .join(format!(
            "live-account-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let remember = std::env::var("OCTAVE_LIVE_REMEMBER").as_deref() == Ok("1");
        let mut auth = Auth::at(root.clone());
        auth.sign_in(Browser::default());
        eprintln!(
            "attended_sign_in=waiting; sign in in the isolated browser, wait for Music, then CLOSE that window; no credentials are printed"
        );
        let start = std::time::Instant::now();
        let (account, identity, channels) = loop {
            match auth.poll() {
                Some(Event::Ready(a, i, c)) => break (a, i, c),
                Some(Event::Failed(error)) => panic!("Sign-in failed: {error}"),
                _ => (),
            }
            assert!(
                start.elapsed() < Duration::from_secs(930),
                "Sign-in timed out"
            );
            std::thread::sleep(Duration::from_millis(100));
        };
        eprintln!(
            "account_identity=verified; channel_discovery=passed; premium_reported={:?}",
            identity.premium
        );
        let store = vault::Store::new(root.join("account"));
        let player = octave_player::Player::new();
        player.set_volume(0.0);
        player.set_account(Some(account.clone()));
        let result = (|| -> Result<(), String> {
            for id in [
                "VLLM",
                "FEmusic_liked_videos",
                "FEmusic_liked_albums",
                "FEmusic_liked_playlists",
            ] {
                account
                    .browse(id, None)
                    .map_err(|e| format!("read-only library check failed: {e}"))?;
            }
            eprintln!("read_only_library_views=passed");
            let refreshed = account
                .verify()
                .map_err(|e| format!("same-account refresh failed: {e}"))?;
            if refreshed.id != identity.id {
                return Err("Refresh changed identity".into());
            }
            eprintln!("same_account_refresh=passed");
            if let Some(channel) = channels.iter().find(|c| c.id != identity.id) {
                let (candidate, selected) = account
                    .select_channel(channel)
                    .map_err(|e| format!("alternate channel validation failed: {e}"))?;
                if selected.id != channel.id {
                    return Err("Alternate channel identity did not match".into());
                }
                candidate.disconnect();
                eprintln!("alternate_channel_validation=passed");
            } else {
                eprintln!("alternate_channel_validation=not_applicable_single_channel");
            }
            if remember {
                store
                    .save(&account, &identity)
                    .map_err(|e| format!("remember test failed: {e}"))?;
                let (restored, expected) = store
                    .restore()
                    .map_err(|e| format!("restore test failed: {e}"))?;
                let verified = restored
                    .verify()
                    .map_err(|e| format!("restored account validation failed: {e}"))?;
                if verified.id != expected {
                    return Err("Restored account identity did not match".into());
                }
                restored.disconnect();
                eprintln!("protected_remember_restore=passed");
            }
            let track = octave_core::TrackId("dQw4w9WgXcQ".into());
            player.play(track);
            let started = std::time::Instant::now();
            let quality = loop {
                let snapshot = player.snapshot();
                if let octave_player::PlaybackState::Failed(_, failure) = snapshot.state {
                    return Err(format!("Native authenticated playback failed: {failure}"));
                }
                if snapshot.position >= Duration::from_secs(2)
                    && let Some(quality) = snapshot.quality
                {
                    break quality;
                }
                if started.elapsed() > Duration::from_secs(35) {
                    return Err("Native authenticated playback startup timed out".into());
                }
                std::thread::sleep(Duration::from_millis(50));
            };
            eprintln!(
                "native_playback=started; bitrate_bps={:?}; sample_rate={}; channels={}",
                quality.bitrate_bps, quality.sample_rate, quality.channels
            );
            if quality.bitrate_bps.is_none_or(|rate| rate < 192000) {
                return Err("The account did not receive a Premium-quality AAC stream; full-quality acceptance remains open".into());
            }
            let until = std::time::Instant::now() + Duration::from_secs(15);
            while std::time::Instant::now() < until {
                let snapshot = player.snapshot();
                if snapshot.buffering
                    || !matches!(snapshot.state, octave_player::PlaybackState::Playing(_))
                {
                    return Err("Authenticated playback was interrupted".into());
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            eprintln!("native_premium_playback=passed; muted=true");
            Ok(())
        })();
        player.stop();
        player.set_account(None);
        account.disconnect();
        let forgotten = if remember { store.forget() } else { Ok(()) };
        let browser = browser::cleanup_pending(&root.join("account/browser"));
        assert!(
            forgotten.is_ok(),
            "Live credential cleanup failed; retry Forget account"
        );
        assert!(
            browser.is_ok(),
            "Live browser cleanup failed; close the isolated browser and retry cleanup"
        );
        assert!(!store.exists());
        if remember {
            assert!(
                store.is_fully_forgotten().unwrap(),
                "Saved session or OS-vault key remains"
            );
        }
        eprintln!("live_credentials_and_browser_cleanup=passed; audio=stopped");
        if let Err(message) = result {
            panic!("{message}");
        }
    }
}
