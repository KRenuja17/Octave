//! Fail-closed update verification. No key discovery, execution or installation.
use ring::{
    digest::{Context, SHA256},
    signature::{ED25519, UnparsedPublicKey},
};
use serde_json::Value;
use std::{
    fmt,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};
const MAX_METADATA: usize = 16384;
const MAX_PAYLOAD: u64 = 256 * 1024 * 1024;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Signature,
    Metadata,
    Expired,
    Rollback,
    Network,
    Integrity,
    Io,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Signature => "Update signature is invalid for the trusted key.",
            Self::Metadata => "Update metadata is invalid or unsupported.",
            Self::Expired => "Update metadata has expired or exceeds the allowed lifetime.",
            Self::Rollback => "Update sequence is not newer than the installed release.",
            Self::Network => "Update download failed. Retry to resume the partial download.",
            Self::Integrity => "Update payload failed verification. It will not be activated.",
            Self::Io => "Update files could not be accessed safely.",
        })
    }
}
impl std::error::Error for Error {}
pub struct VerifiedRelease {
    version: String,
    sequence: u64,
    url: String,
    length: u64,
    sha256: [u8; 32],
}
impl fmt::Debug for VerifiedRelease {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VerifiedRelease")
            .field("version", &self.version)
            .field("sequence", &self.sequence)
            .finish_non_exhaustive()
    }
}
pub fn decode_hex<const N: usize>(text: &str) -> Result<[u8; N], Error> {
    if text.len() != N * 2 {
        return Err(Error::Metadata);
    }
    let mut bytes = [0; N];
    for (i, pair) in text.as_bytes().as_chunks::<2>().0.iter().enumerate() {
        let digit = |c: u8| match c {
            b'0'..=b'9' => Ok(c - b'0'),
            b'a'..=b'f' => Ok(c - b'a' + 10),
            b'A'..=b'F' => Ok(c - b'A' + 10),
            _ => Err(Error::Metadata),
        };
        bytes[i] = digit(pair[0])? * 16 + digit(pair[1])?;
    }
    Ok(bytes)
}
/// Trust must come from a separately pinned key, never from the metadata itself.
pub fn verify(
    metadata: &[u8],
    signature: &[u8],
    trusted_key: &[u8; 32],
    installed_sequence: u64,
    now: u64,
) -> Result<VerifiedRelease, Error> {
    if metadata.len() > MAX_METADATA {
        return Err(Error::Metadata);
    }
    UnparsedPublicKey::new(&ED25519, trusted_key)
        .verify(metadata, signature)
        .map_err(|_| Error::Signature)?;
    let value: Value = serde_json::from_slice(metadata).map_err(|_| Error::Metadata)?;
    let number = |key: &str| value[key].as_u64().ok_or(Error::Metadata);
    let text = |key: &str| value[key].as_str().ok_or(Error::Metadata);
    if number("schema")? != 1
        || text("product")? != "Octave"
        || text("target")? != "x86_64-pc-windows-gnu"
    {
        return Err(Error::Metadata);
    }
    let sequence = number("sequence")?;
    if sequence <= installed_sequence {
        return Err(Error::Rollback);
    }
    let expires = number("expires_unix")?;
    if expires <= now || expires > now.saturating_add(30 * 86400) {
        return Err(Error::Expired);
    }
    let version = text("version")?;
    if version.is_empty()
        || version.len() > 64
        || !version
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b".-+".contains(&c))
    {
        return Err(Error::Metadata);
    }
    let url = reqwest::Url::parse(text("url")?).map_err(|_| Error::Metadata)?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(Error::Metadata);
    }
    let length = number("length")?;
    if length == 0 || length > MAX_PAYLOAD {
        return Err(Error::Metadata);
    }
    Ok(VerifiedRelease {
        version: version.into(),
        sequence,
        url: url.into(),
        length,
        sha256: decode_hex(text("sha256")?)?,
    })
}
fn regular(path: &Path) -> Result<Option<u64>, Error> {
    match fs::symlink_metadata(path) {
        Ok(m) if m.is_file() && !m.file_type().is_symlink() => Ok(Some(m.len())),
        Ok(_) => Err(Error::Io),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(Error::Io),
    }
}
fn hash(path: &Path) -> Result<[u8; 32], Error> {
    let mut file = File::open(path).map_err(|_| Error::Io)?;
    let mut context = Context::new(&SHA256);
    let mut bytes = [0; 65536];
    loop {
        let read = file.read(&mut bytes).map_err(|_| Error::Io)?;
        if read == 0 {
            break;
        }
        context.update(&bytes[..read]);
    }
    Ok(context.finish().as_ref().try_into().unwrap())
}
impl VerifiedRelease {
    pub fn version(&self) -> &str {
        &self.version
    }
    pub fn sequence(&self) -> u64 {
        self.sequence
    }
    /// A verified archive is promoted only after exact length and SHA-256 checks.
    /// Caller owns a trusted cache directory; never run concurrent downloads there.
    pub fn download(&self, directory: &Path) -> Result<PathBuf, Error> {
        fs::create_dir_all(directory).map_err(|_| Error::Io)?;
        let digest: String = self.sha256.iter().map(|b| format!("{b:02x}")).collect();
        let target = directory.join(format!("Octave-{}-{digest}.zip", self.sequence));
        let partial = target.with_extension("zip.part");
        if let Some(length) = regular(&target)? {
            return if length == self.length && hash(&target)? == self.sha256 {
                Ok(target)
            } else {
                Err(Error::Integrity)
            };
        }
        let mut offset = regular(&partial)?.unwrap_or(0);
        if offset > self.length {
            return Err(Error::Integrity);
        }
        if offset < self.length {
            let client = reqwest::blocking::Client::builder()
                .connect_timeout(Duration::from_secs(5))
                .timeout(Duration::from_secs(60))
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(|_| Error::Network)?;
            let mut request = client.get(&self.url).header("Accept-Encoding", "identity");
            if offset > 0 {
                request = request.header("Range", format!("bytes={offset}-"));
            }
            let response = request.send().map_err(|_| Error::Network)?;
            match response.status().as_u16() {
                200 => offset = 0,
                206 => {
                    let expected = format!("bytes {}-{}/{}", offset, self.length - 1, self.length);
                    if response
                        .headers()
                        .get("content-range")
                        .and_then(|h| h.to_str().ok())
                        != Some(expected.as_str())
                    {
                        return Err(Error::Integrity);
                    }
                }
                _ => return Err(Error::Network),
            }
            if response
                .content_length()
                .is_some_and(|n| n != self.length - offset)
            {
                return Err(Error::Integrity);
            }
            let mut file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(offset == 0)
                .append(offset > 0)
                .open(&partial)
                .map_err(|_| Error::Io)?;
            let mut reader = response.take(self.length - offset + 1);
            let mut buffer = [0; 65536];
            let mut total = offset;
            loop {
                let read = match reader.read(&mut buffer) {
                    Ok(read) => read,
                    Err(_) => {
                        let _ = file.sync_all();
                        return Err(Error::Network);
                    }
                };
                if read == 0 {
                    break;
                }
                total += read as u64;
                if total > self.length {
                    drop(file);
                    let _ = fs::remove_file(&partial);
                    return Err(Error::Integrity);
                }
                file.write_all(&buffer[..read]).map_err(|_| Error::Io)?;
            }
            file.sync_all().map_err(|_| Error::Io)?;
            if total != self.length {
                return Err(Error::Network);
            }
        }
        if hash(&partial)? != self.sha256 {
            let _ = fs::remove_file(&partial);
            return Err(Error::Integrity);
        }
        fs::rename(&partial, &target).map_err(|_| Error::Io)?;
        Ok(target)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use ring::{
        rand::SystemRandom,
        signature::{Ed25519KeyPair, KeyPair},
    };
    use serde_json::json;
    fn key() -> Ed25519KeyPair {
        let bytes = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
        Ed25519KeyPair::from_pkcs8(bytes.as_ref()).unwrap()
    }
    fn metadata() -> Value {
        json!({"schema":1,"product":"Octave","target":"x86_64-pc-windows-gnu","version":"0.1.0-rc.1","sequence":2,"expires_unix":1100,"url":"https://updates.example.test/release.zip","length":100,"sha256":"00".repeat(32)})
    }
    fn signed(value: Value, key: &Ed25519KeyPair) -> Result<VerifiedRelease, Error> {
        let bytes = serde_json::to_vec(&value).unwrap();
        verify(
            &bytes,
            key.sign(&bytes).as_ref(),
            key.public_key().as_ref().try_into().unwrap(),
            1,
            1000,
        )
    }
    #[test]
    fn tampering_wrong_keys_expiry_and_downgrade_fail_closed() {
        let signer = key();
        assert!(signed(metadata(), &signer).is_ok());
        let bytes = serde_json::to_vec(&metadata()).unwrap();
        let sig = signer.sign(&bytes);
        let mut changed = bytes.clone();
        changed.push(b' ');
        assert_eq!(
            verify(
                &changed,
                sig.as_ref(),
                signer.public_key().as_ref().try_into().unwrap(),
                1,
                1000
            )
            .unwrap_err(),
            Error::Signature
        );
        assert_eq!(
            verify(
                &bytes,
                sig.as_ref(),
                key().public_key().as_ref().try_into().unwrap(),
                1,
                1000
            )
            .unwrap_err(),
            Error::Signature
        );
        for (field, value, error) in [
            ("sequence", json!(1), Error::Rollback),
            ("expires_unix", json!(999), Error::Expired),
            ("target", json!("other"), Error::Metadata),
            ("url", json!("http://insecure.test"), Error::Metadata),
            ("length", json!(MAX_PAYLOAD + 1), Error::Metadata),
        ] {
            let mut v = metadata();
            v[field] = value;
            assert_eq!(signed(v, &signer).unwrap_err(), error);
        }
    }
    #[test]
    fn resume_handles_range_ignored_wrong_range_redirect_and_oversize() {
        use std::net::TcpListener;
        let cases = [
            (
                "ignored",
                "HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\ngood",
                None,
            ),
            (
                "wrong-range",
                "HTTP/1.1 206 Partial Content\r\nContent-Length: 2\r\nContent-Range: bytes 1-2/4\r\nConnection: close\r\n\r\nod",
                Some(Error::Integrity),
            ),
            (
                "redirect",
                "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                Some(Error::Network),
            ),
            (
                "oversize",
                "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n5\r\ngood!\r\n0\r\n\r\n",
                Some(Error::Integrity),
            ),
        ];
        for (name, response, expected) in cases {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let address = listener.local_addr().unwrap();
            listener.set_nonblocking(true).unwrap();
            let server = std::thread::spawn(move || {
                let deadline = std::time::Instant::now() + Duration::from_secs(5);
                let mut socket = loop {
                    match listener.accept() {
                        Ok((socket, _)) => break socket,
                        Err(error)
                            if error.kind() == std::io::ErrorKind::WouldBlock
                                && std::time::Instant::now() < deadline =>
                        {
                            std::thread::sleep(Duration::from_millis(10))
                        }
                        Err(error) => panic!("Fixture request did not arrive: {error}"),
                    }
                };
                // Accepted sockets can inherit the listener's nonblocking mode on Windows.
                socket.set_nonblocking(false).unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut request = Vec::new();
                let mut byte = [0];
                while !request.ends_with(b"\r\n\r\n") {
                    socket.read_exact(&mut byte).unwrap();
                    request.push(byte[0]);
                }
                assert!(
                    String::from_utf8(request)
                        .unwrap()
                        .to_lowercase()
                        .contains("range: bytes=2-")
                );
                socket.write_all(response.as_bytes()).unwrap();
            });
            let root = std::env::var_os("OCTAVE_TEST_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(std::env::temp_dir)
                .join(format!("octave-update-{name}-{}", std::process::id()));
            fs::create_dir_all(&root).unwrap();
            let sha256: [u8; 32] = ring::digest::digest(&SHA256, b"good")
                .as_ref()
                .try_into()
                .unwrap();
            let digest: String = sha256.iter().map(|b| format!("{b:02x}")).collect();
            let archive = root.join(format!("Octave-2-{digest}.zip"));
            let partial = archive.with_extension("zip.part");
            fs::write(&partial, b"go").unwrap();
            let release = VerifiedRelease {
                version: "test".into(),
                sequence: 2,
                url: format!("http://{address}/test"),
                length: 4,
                sha256,
            };
            let result = release.download(&root);
            server.join().unwrap();
            if let Some(error) = expected {
                assert_eq!(result.unwrap_err(), error);
                assert!(!archive.exists());
            } else {
                assert_eq!(fs::read(result.unwrap()).unwrap(), b"good");
                assert!(!partial.exists());
                // A verified cached archive needs no network connection.
                assert_eq!(release.download(&root).unwrap(), archive);
            }
            if partial.exists() {
                fs::remove_file(partial).unwrap();
            }
            if archive.exists() {
                fs::remove_file(archive).unwrap();
            }
            fs::remove_dir(root).unwrap();
        }
    }
    #[test]
    fn tampered_download_is_deleted_without_promotion() {
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut request = Vec::new();
            let mut byte = [0];
            while !request.ends_with(b"\r\n\r\n") {
                socket.read_exact(&mut byte).unwrap();
                request.push(byte[0]);
            }
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\nevil")
                .unwrap();
        });
        let root = std::env::var_os("OCTAVE_TEST_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join(format!("octave-update-tamper-{}", std::process::id()));
        let release = VerifiedRelease {
            version: "test".into(),
            sequence: 2,
            url: format!("http://{address}/test"),
            length: 4,
            sha256: ring::digest::digest(&SHA256, b"good")
                .as_ref()
                .try_into()
                .unwrap(),
        };
        assert_eq!(release.download(&root).unwrap_err(), Error::Integrity);
        server.join().unwrap();
        assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
        fs::remove_dir(root).unwrap();
    }
    #[test]
    fn interrupted_download_resumes_and_corruption_is_not_promoted() {
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let payload = vec![42; 1024];
        let server_payload = payload.clone();
        let server = std::thread::spawn(move || {
            for attempt in 0..2 {
                let (mut socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut request = Vec::new();
                let mut byte = [0];
                while !request.ends_with(b"\r\n\r\n") {
                    socket.read_exact(&mut byte).unwrap();
                    request.push(byte[0]);
                }
                if attempt == 0 {
                    write!(
                        socket,
                        "HTTP/1.1 200 OK\r\nContent-Length: 1024\r\nConnection: close\r\n\r\n"
                    )
                    .unwrap();
                    socket.write_all(&server_payload[..256]).unwrap();
                } else {
                    assert!(
                        String::from_utf8(request)
                            .unwrap()
                            .to_lowercase()
                            .contains("range: bytes=256-")
                    );
                    write!(socket,"HTTP/1.1 206 Partial Content\r\nContent-Length: 768\r\nContent-Range: bytes 256-1023/1024\r\nConnection: close\r\n\r\n").unwrap();
                    socket.write_all(&server_payload[256..]).unwrap();
                }
            }
        });
        let root = std::env::var_os("OCTAVE_TEST_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join(format!("octave-update-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let release = VerifiedRelease {
            version: "test".into(),
            sequence: 2,
            url: format!("http://{address}/test"),
            length: 1024,
            sha256: ring::digest::digest(&SHA256, &payload)
                .as_ref()
                .try_into()
                .unwrap(),
        };
        assert_eq!(release.download(&root).unwrap_err(), Error::Network);
        let archive = release.download(&root).unwrap();
        assert_eq!(fs::read(&archive).unwrap(), payload);
        server.join().unwrap();
        fs::write(&archive, b"corrupt").unwrap();
        assert_eq!(release.download(&root).unwrap_err(), Error::Integrity);
        fs::remove_file(archive).unwrap();
        fs::remove_dir(root).unwrap();
    }
}
