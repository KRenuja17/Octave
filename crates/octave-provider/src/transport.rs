//! One process-wide async HTTP runtime behind the synchronous provider API.
//! Request deadlines include the complete body; cancellation drops the future.
//! Separate finite lanes keep artwork/catalog traffic out of playback capacity.
use crate::{Error, WEB_UA, network};
use reqwest::{Client, Request, Response};
use std::{
    sync::{Arc, OnceLock, mpsc},
    time::{Duration, Instant},
};
use tokio::{runtime::Runtime, sync::Semaphore};

pub(crate) const REQUEST_TIMEOUT: Duration = Duration::from_secs(12);
const CANCEL_POLL: Duration = Duration::from_millis(20);
#[derive(Clone, Copy)]
pub(crate) enum Lane {
    Media,
    Metadata,
}
#[derive(Clone, Copy)]
pub(crate) enum Expected {
    Bytes(usize),
    Range { start: u64, end: u64, total: u64 },
}
impl Expected {
    fn limit(self) -> usize {
        match self {
            Self::Bytes(limit) => limit,
            Self::Range { start, end, .. } => (end - start + 1) as usize,
        }
    }
    fn oversized(self) -> Error {
        match self {
            Self::Bytes(_) => Error::InvalidResponse,
            Self::Range { .. } => Error::InvalidRange,
        }
    }
    fn validate(self, response: &Response) -> Result<(), Error> {
        let status = response.status();
        if !status.is_success() {
            return Err(Error::Http(status.as_u16()));
        }
        if let Self::Range { start, end, total } = self {
            if status.as_u16() != 206
                || response
                    .headers()
                    .get("content-range")
                    .and_then(|v| v.to_str().ok())
                    != Some(format!("bytes {start}-{end}/{total}").as_str())
            {
                return Err(Error::InvalidRange);
            }
            if response
                .headers()
                .get("content-encoding")
                .is_some_and(|v| v != "identity")
            {
                return Err(Error::InvalidRange);
            }
            if response
                .content_length()
                .is_some_and(|length| length != end - start + 1)
            {
                return Err(Error::InvalidRange);
            }
        }
        if response
            .content_length()
            .is_some_and(|length| length > self.limit() as u64)
        {
            return Err(self.oversized());
        }
        Ok(())
    }
}
pub(crate) struct Transport {
    pub client: Client,
    runtime: Runtime,
    media: Arc<Semaphore>,
    metadata: Arc<Semaphore>,
}
// Header access is needed only by the opt-in anonymous bootstrap investigation.
// Production byte callers do not retain response metadata or gain a cookie jar.
pub(crate) struct Reply {
    pub bytes: Vec<u8>,
    #[cfg(test)]
    pub headers: reqwest::header::HeaderMap,
}
impl std::fmt::Debug for Reply {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Reply { body: [REDACTED], headers: [REDACTED] }")
    }
}
pub(crate) fn shared() -> Result<Arc<Transport>, Error> {
    static TRANSPORT: OnceLock<Result<Arc<Transport>, Error>> = OnceLock::new();
    TRANSPORT
        .get_or_init(|| Transport::new().map(Arc::new))
        .clone()
}
impl Transport {
    pub(crate) fn new() -> Result<Self, Error> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .max_blocking_threads(2)
            .enable_all()
            .thread_name("octave-http")
            .build()
            .map_err(|_| Error::Network)?;
        let client = {
            let _entered = runtime.enter();
            Client::builder()
                .connect_timeout(Duration::from_secs(5))
                .timeout(REQUEST_TIMEOUT)
                .pool_max_idle_per_host(2)
                .redirect(reqwest::redirect::Policy::none())
                .user_agent(WEB_UA)
                .build()
                .map_err(network)?
        };
        Ok(Self {
            client,
            runtime,
            media: Arc::new(Semaphore::new(4)),
            metadata: Arc::new(Semaphore::new(8)),
        })
    }
    pub fn bytes(
        &self,
        request: Request,
        expected: Expected,
        lane: Lane,
        timeout: Duration,
        cancelled: impl Fn() -> bool,
    ) -> Result<Vec<u8>, Error> {
        self.reply(request, expected, lane, timeout, cancelled)
            .map(|reply| reply.bytes)
    }
    pub fn reply(
        &self,
        request: Request,
        expected: Expected,
        lane: Lane,
        timeout: Duration,
        cancelled: impl Fn() -> bool,
    ) -> Result<Reply, Error> {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        let permit = match lane {
            Lane::Media => &self.media,
            Lane::Metadata => &self.metadata,
        }
        .clone()
        .try_acquire_owned()
        .map_err(|_| Error::Busy)?;
        let deadline = Instant::now() + timeout;
        let client = self.client.clone();
        let (tx, rx) = mpsc::sync_channel(1);
        let task = self.runtime.spawn(async move {
            let _permit = permit;
            let result = tokio::time::timeout_at(tokio::time::Instant::from_std(deadline), async {
                let mut response = client.execute(request).await.map_err(network)?;
                expected.validate(&response)?;
                #[cfg(test)]
                let headers = {
                    let mut headers = response.headers().clone();
                    for value in headers.values_mut() {
                        value.set_sensitive(true);
                    }
                    headers
                };
                let mut bytes = Vec::with_capacity(
                    response
                        .content_length()
                        .unwrap_or(4096)
                        .min(expected.limit() as u64) as usize,
                );
                while let Some(chunk) = response.chunk().await.map_err(network)? {
                    if chunk.len() > expected.limit().saturating_sub(bytes.len()) {
                        return Err(expected.oversized());
                    }
                    if bytes.len() + chunk.len() > bytes.capacity() {
                        let capacity = (bytes.capacity().saturating_mul(2))
                            .max(bytes.len() + chunk.len())
                            .min(expected.limit());
                        bytes.reserve_exact(capacity - bytes.len());
                    }
                    bytes.extend_from_slice(&chunk);
                }
                if matches!(expected, Expected::Range { .. }) && bytes.len() != expected.limit() {
                    return Err(Error::InvalidRange);
                }
                Ok(Reply {
                    bytes,
                    #[cfg(test)]
                    headers,
                })
            })
            .await
            .unwrap_or(Err(Error::Timeout));
            let _ = tx.send(result);
        });
        // Abort on every early return, including unwinding a caller cancellation check.
        struct Abort(tokio::task::JoinHandle<()>);
        impl Drop for Abort {
            fn drop(&mut self) {
                self.0.abort();
            }
        }
        let _abort = Abort(task);
        loop {
            if cancelled() {
                return Err(Error::Cancelled);
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(Error::Timeout);
            }
            match rx.recv_timeout(remaining.min(CANCEL_POLL)) {
                Ok(result) => {
                    return if cancelled() {
                        Err(Error::Cancelled)
                    } else {
                        result
                    };
                }
                Err(mpsc::RecvTimeoutError::Timeout) => (),
                Err(mpsc::RecvTimeoutError::Disconnected) => return Err(Error::Network),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{BufRead, BufReader, Read, Write},
        net::TcpListener,
        sync::atomic::{AtomicBool, Ordering},
        thread,
    };
    fn server(
        handler: impl FnOnce(std::net::TcpStream) + Send + 'static,
    ) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/fixture", listener.local_addr().unwrap());
        let task = thread::spawn(move || {
            let (socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut reader = BufReader::new(socket.try_clone().unwrap());
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap() == 0 || line == "\r\n" {
                    break;
                }
            }
            handler(socket);
        });
        (url, task)
    }
    #[test]
    fn absolute_deadline_includes_a_body_that_keeps_making_progress() {
        let transport = Transport::new().unwrap();
        let (url, server) = server(|mut socket| {
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 64\r\nConnection: close\r\n\r\n")
                .unwrap();
            for _ in 0..64 {
                if socket.write_all(b"x").is_err() {
                    break;
                }
                thread::sleep(Duration::from_millis(30));
            }
        });
        let start = Instant::now();
        assert_eq!(
            transport.bytes(
                transport.client.get(url).build().unwrap(),
                Expected::Bytes(64),
                Lane::Metadata,
                Duration::from_millis(150),
                || false
            ),
            Err(Error::Timeout)
        );
        assert!(start.elapsed() < Duration::from_secs(1));
        server.join().unwrap();
    }
    #[test]
    fn cancellation_aborts_stalled_headers_and_body_and_releases_capacity() {
        let transport = Transport::new().unwrap();
        for body in [false, true] {
            let (ready, wait) = mpsc::channel();
            let (url, server) = server(move |mut socket| {
                if body {
                    socket
                        .write_all(
                            b"HTTP/1.1 200 OK\r\nContent-Length: 64\r\nConnection: close\r\n\r\nx",
                        )
                        .unwrap();
                }
                ready.send(()).unwrap();
                // The client must close the canceled connection, not wait for a timeout.
                assert_eq!(socket.read(&mut [0; 1]).unwrap(), 0);
            });
            let cancel = Arc::new(AtomicBool::new(false));
            let flag = cancel.clone();
            let trigger = thread::spawn(move || {
                wait.recv_timeout(Duration::from_secs(2)).unwrap();
                flag.store(true, Ordering::Release);
            });
            let start = Instant::now();
            assert_eq!(
                transport.bytes(
                    transport.client.get(url).build().unwrap(),
                    Expected::Bytes(64),
                    Lane::Media,
                    Duration::from_secs(5),
                    || cancel.load(Ordering::Acquire)
                ),
                Err(Error::Cancelled)
            );
            assert!(start.elapsed() < Duration::from_secs(1));
            trigger.join().unwrap();
            server.join().unwrap();
        }
        let deadline = Instant::now() + Duration::from_secs(1);
        while transport.media.available_permits() != 4 {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(5));
        }
    }
    #[test]
    fn response_limits_status_and_ranges_fail_before_accepting_payload() {
        let transport = Transport::new().unwrap();
        for (headers, expected, error) in [
            (
                "200 OK\r\nContent-Length: 999",
                Expected::Bytes(16),
                Error::InvalidResponse,
            ),
            (
                "302 Found\r\nLocation: http://127.0.0.1:1",
                Expected::Bytes(16),
                Error::Http(302),
            ),
            (
                "206 Partial Content\r\nContent-Range: bytes 0-3/4\r\nContent-Length: 4\r\nContent-Encoding: gzip",
                Expected::Range {
                    start: 0,
                    end: 3,
                    total: 4,
                },
                Error::InvalidRange,
            ),
            (
                "206 Partial Content\r\nContent-Range: bytes 0-3/4\r\nContent-Length: 3",
                Expected::Range {
                    start: 0,
                    end: 3,
                    total: 4,
                },
                Error::InvalidRange,
            ),
        ] {
            let headers = headers.to_owned();
            let (url, server) = server(move |mut socket| {
                let _ = write!(socket, "HTTP/1.1 {headers}\r\nConnection: close\r\n\r\n");
            });
            assert_eq!(
                transport.bytes(
                    transport.client.get(url).build().unwrap(),
                    expected,
                    Lane::Media,
                    Duration::from_secs(1),
                    || false
                ),
                Err(error)
            );
            server.join().unwrap();
        }
        let (url, server) = server(|mut socket| {
            socket.write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n10\r\n0123456789abcdef\r\n10\r\n0123456789abcdef\r\n0\r\n\r\n").unwrap();
        });
        assert_eq!(
            transport.bytes(
                transport.client.get(url).build().unwrap(),
                Expected::Bytes(20),
                Lane::Metadata,
                Duration::from_secs(1),
                || false
            ),
            Err(Error::InvalidResponse)
        );
        server.join().unwrap();
    }
    #[test]
    fn metadata_saturation_does_not_consume_playback_capacity() {
        let transport = Transport::new().unwrap();
        let _metadata = transport
            .metadata
            .clone()
            .try_acquire_many_owned(8)
            .unwrap();
        let request = transport.client.get("http://127.0.0.1:1").build().unwrap();
        assert_eq!(
            transport.bytes(
                request,
                Expected::Bytes(1),
                Lane::Metadata,
                Duration::from_secs(1),
                || false
            ),
            Err(Error::Busy)
        );
        let (url, server) = server(|mut socket| {
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nOK")
                .unwrap();
        });
        assert_eq!(
            transport
                .bytes(
                    transport.client.get(url).build().unwrap(),
                    Expected::Bytes(2),
                    Lane::Media,
                    Duration::from_secs(1),
                    || false
                )
                .unwrap(),
            b"OK"
        );
        server.join().unwrap();
    }
}
