//! Bounded, seekable transport. Only call from a decoding worker, never the audio callback.
use crate::{Error, PLAYER_UA, Provider, ResolvedStream, network, transport};
use std::{
    io::{self, Read, Seek, SeekFrom},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};
const BLOCK: u64 = 256 * 1024;
/// Generation changes abort in-flight HTTP; each whole range transaction has
/// a 12s wall-clock deadline, including its response body.
pub struct RangeReader {
    provider: Provider,
    stream: ResolvedStream,
    generation: Arc<AtomicU64>,
    expected: u64,
    position: u64,
    start: u64,
    bytes: Vec<u8>,
    fault: Arc<Mutex<Option<Error>>>,
}
impl RangeReader {
    pub fn new(
        provider: Provider,
        stream: ResolvedStream,
        generation: Arc<AtomicU64>,
        expected: u64,
    ) -> Self {
        Self {
            provider,
            stream,
            generation,
            expected,
            position: 0,
            start: 0,
            bytes: Vec::new(),
            fault: Arc::new(Mutex::new(None)),
        }
    }
    pub fn fault(&self) -> Arc<Mutex<Option<Error>>> {
        self.fault.clone()
    }
    pub fn length(&self) -> u64 {
        self.stream.content_length
    }
    fn check(&self) -> Result<(), Error> {
        if self.generation.load(Ordering::Acquire) == self.expected {
            Ok(())
        } else {
            Err(Error::Cancelled)
        }
    }
    fn fetch(&mut self) -> Result<(), Error> {
        self.check()?;
        let end = self
            .position
            .saturating_add(BLOCK - 1)
            .min(self.length() - 1);
        let request = self
            .provider
            .transport
            .client
            .get(self.stream.url.expose())
            .header("User-Agent", PLAYER_UA)
            .header("Accept-Encoding", "identity")
            .header("Range", format!("bytes={}-{}", self.position, end))
            .build()
            .map_err(network)?;
        let bytes = self.provider.transport.bytes(
            request,
            transport::Expected::Range {
                start: self.position,
                end,
                total: self.length(),
            },
            transport::Lane::Media,
            transport::REQUEST_TIMEOUT,
            || {
                self.generation.load(Ordering::Acquire) != self.expected
                    || self.provider.is_cancelled()
            },
        )?;
        self.check()?;
        self.bytes = bytes;
        self.start = self.position;
        Ok(())
    }
}
impl Read for RangeReader {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        self.check().map_err(io::Error::other)?;
        if out.is_empty() || self.position >= self.length() {
            return Ok(0);
        }
        if (self.position < self.start || self.position >= self.start + self.bytes.len() as u64)
            && let Err(error) = self.fetch()
        {
            *self.fault.lock().unwrap() = Some(error);
            return Err(io::Error::other(error));
        }
        let offset = (self.position - self.start) as usize;
        let count = out.len().min(self.bytes.len() - offset);
        out[..count].copy_from_slice(&self.bytes[offset..offset + count]);
        self.position += count as u64;
        Ok(count)
    }
}
impl Seek for RangeReader {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        self.check().map_err(io::Error::other)?;
        let position = match to {
            SeekFrom::Start(p) => i128::from(p),
            SeekFrom::Current(n) => i128::from(self.position) + i128::from(n),
            SeekFrom::End(n) => i128::from(self.length()) + i128::from(n),
        };
        if position < 0 || position > i128::from(u64::MAX) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Invalid audio seek",
            ));
        }
        self.position = position as u64;
        Ok(self.position)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use octave_core::{TrackId, diagnostics::Sensitive};
    use std::{
        io::{BufRead, BufReader, Write},
        net::TcpListener,
        thread,
    };

    fn server_reader(
        status: &str,
        content_range: &str,
        body: Vec<u8>,
        length: u64,
    ) -> (RangeReader, thread::JoinHandle<String>) {
        let server = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = server.local_addr().unwrap();
        let headers = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nContent-Range: {content_range}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let handle = thread::spawn(move || {
            let (mut socket, _) = server.accept().unwrap();
            socket
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut input = BufReader::new(socket.try_clone().unwrap());
            let mut request = String::new();
            loop {
                let mut line = String::new();
                if input.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                    break;
                }
                request.push_str(&line);
            }
            socket.write_all(headers.as_bytes()).unwrap();
            let _ = socket.write_all(&body);
            request
        });
        let mut r = reader();
        r.stream.url = Sensitive::new(format!("http://{address}/audio"));
        r.stream.content_length = length;
        (r, handle)
    }

    #[test]
    fn ranges_are_bounded_and_backward_seek_reuses_block() {
        let body: Vec<u8> = (0..BLOCK).map(|n| (n % 251) as u8).collect();
        let (mut r, server) = server_reader(
            "206 Partial Content",
            &format!("bytes 0-{}/{}", BLOCK - 1, BLOCK * 8),
            body,
            BLOCK * 8,
        );
        let mut out = [0; 32];
        r.read_exact(&mut out).unwrap();
        assert_eq!(out[31], 31);
        r.seek(SeekFrom::Start(7)).unwrap();
        r.read_exact(&mut out).unwrap();
        assert_eq!(out[0], 7);
        assert_eq!(r.bytes.len() as u64, BLOCK);
        assert!(
            server
                .join()
                .unwrap()
                .to_lowercase()
                .contains(&format!("range: bytes=0-{}", BLOCK - 1))
        );
    }

    #[test]
    fn large_virtual_media_reads_only_one_block_near_end() {
        let length = 1024 * 1024 * 1024_u64;
        let offset = length - BLOCK;
        let header = format!("bytes {offset}-{}/{length}", length - 1);
        let (mut reader, server) = server_reader(
            "206 Partial Content",
            &header,
            vec![7; BLOCK as usize],
            length,
        );
        reader.seek(SeekFrom::Start(offset)).unwrap();
        let mut sample = [0; 4096];
        reader.read_exact(&mut sample).unwrap();
        assert_eq!(sample, [7; 4096]);
        assert_eq!(reader.bytes.len(), BLOCK as usize);
        assert!(
            server
                .join()
                .unwrap()
                .to_lowercase()
                .contains(&format!("range: bytes={offset}-{}", length - 1))
        );
    }
    #[test]
    fn rejects_full_download_wrong_ranges_and_truncation() {
        for (status, header, body, expected) in [
            (
                "200 OK",
                "bytes 0-99/100",
                vec![0; 100],
                Error::InvalidRange,
            ),
            (
                "206 Partial Content",
                "bytes 1-100/100",
                vec![0; 100],
                Error::InvalidRange,
            ),
            (
                "206 Partial Content",
                "bytes 0-99/100",
                vec![0; 90],
                Error::InvalidRange,
            ),
            ("403 Forbidden", "", vec![], Error::Http(403)),
        ] {
            let (mut r, server) = server_reader(status, header, body, 100);
            assert!(r.read(&mut [0; 1]).is_err());
            assert_eq!(*r.fault().lock().unwrap(), Some(expected));
            server.join().unwrap();
        }
    }
    fn reader() -> RangeReader {
        let stream = ResolvedStream {
            bitrate_bps: None,
            track_id: TrackId("abcdefghijk".into()),
            url: Sensitive::new("https://unused.googlevideo.com".into()),
            content_length: 100,
            duration: std::time::Duration::from_secs(1),
        };
        RangeReader::new(
            Provider {
                transport: Arc::new(transport::Transport::new().unwrap()),
                cancelled: None,
            },
            stream,
            Arc::new(AtomicU64::new(1)),
            1,
        )
    }
    #[test]
    fn reconnect_with_a_fresh_reader_preserves_requested_range() {
        let (mut failed, first) = server_reader("503 Service Unavailable", "", vec![], 100);
        failed.seek(SeekFrom::Start(42)).unwrap();
        assert!(failed.read(&mut [0; 1]).is_err());
        assert_eq!(*failed.fault().lock().unwrap(), Some(Error::Http(503)));
        first.join().unwrap();
        let (mut resumed, second) =
            server_reader("206 Partial Content", "bytes 42-99/100", vec![7; 58], 100);
        resumed.seek(SeekFrom::Start(42)).unwrap();
        let mut bytes = [0; 16];
        resumed.read_exact(&mut bytes).unwrap();
        assert_eq!(bytes, [7; 16]);
        assert!(resumed.fault().lock().unwrap().is_none());
        assert!(
            second
                .join()
                .unwrap()
                .to_lowercase()
                .contains("range: bytes=42-99")
        );
    }
    #[test]
    fn a_new_generation_aborts_an_inflight_range_without_returning_stale_bytes() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut reader = reader();
        reader.stream.url =
            Sensitive::new(format!("http://{}/audio", listener.local_addr().unwrap()));
        let generation = reader.generation.clone();
        let (ready, wait) = std::sync::mpsc::channel();
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                .unwrap();
            let mut request = BufReader::new(socket.try_clone().unwrap());
            loop {
                let mut line = String::new();
                request.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
            }
            socket.write_all(b"HTTP/1.1 206 Partial Content\r\nContent-Range: bytes 0-99/100\r\nContent-Length: 100\r\n\r\n").unwrap();
            ready.send(()).unwrap();
            assert_eq!(socket.read(&mut [0; 1]).unwrap(), 0);
        });
        let cancel = thread::spawn(move || {
            wait.recv_timeout(std::time::Duration::from_secs(2))
                .unwrap();
            generation.store(2, Ordering::Release);
        });
        let started = std::time::Instant::now();
        assert!(reader.read(&mut [0; 8]).is_err());
        assert_eq!(*reader.fault.lock().unwrap(), Some(Error::Cancelled));
        assert_eq!(reader.position, 0);
        assert!(reader.bytes.is_empty());
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
        cancel.join().unwrap();
        server.join().unwrap();
    }
    #[test]
    fn seek_bounds_eof_and_cancellation() {
        let mut r = reader();
        assert!(r.seek(SeekFrom::Current(-1)).is_err());
        assert_eq!(r.seek(SeekFrom::End(-10)).unwrap(), 90);
        assert_eq!(r.seek(SeekFrom::Start(101)).unwrap(), 101);
        assert_eq!(r.read(&mut [0; 1]).unwrap(), 0);
        r.generation.store(2, Ordering::Release);
        assert!(r.seek(SeekFrom::Start(0)).is_err());
        assert!(r.read(&mut [0; 1]).is_err());
    }
}
