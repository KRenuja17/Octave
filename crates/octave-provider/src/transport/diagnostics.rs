//! Opt-in, bounded diagnostics. Never retain requests, URLs, headers or bodies.
use super::{Error, Expected, Lane};
use std::{
    sync::{
        Arc,
        atomic::{AtomicU16, AtomicU64, AtomicUsize, Ordering},
    },
    time::Instant,
};

pub(super) struct Trace {
    started: Instant,
    kind: &'static str,
    lane: &'static str,
    headers_ms: AtomicU64,
    status: AtomicU16,
    bytes: AtomicUsize,
}
impl Trace {
    pub fn begin(request: &reqwest::Request, expected: Expected, lane: Lane) -> Option<Arc<Self>> {
        // Exact opt-in only. Disabled operation adds no allocation or event output.
        if std::env::var_os("OCTAVE_HTTP_DIAGNOSTICS").as_deref() != Some(std::ffi::OsStr::new("1"))
        {
            return None;
        }
        Some(Arc::new(Self::new(request, expected, lane)))
    }
    pub(super) fn new(request: &reqwest::Request, expected: Expected, lane: Lane) -> Self {
        Self {
            started: Instant::now(),
            kind: if matches!(expected, Expected::Range { .. }) {
                "range"
            } else {
                match request.url().path() {
                    "/youtubei/v1/visitor_id" => "visitor",
                    "/youtubei/v1/player" => "player",
                    _ => "other",
                }
            },
            lane: match lane {
                Lane::Media => "media",
                Lane::Metadata => "metadata",
                Lane::Optional => "optional",
            },
            headers_ms: AtomicU64::new(u64::MAX),
            status: AtomicU16::new(0),
            bytes: AtomicUsize::new(0),
        }
    }
    pub fn headers(&self, status: u16) {
        self.status.store(status, Ordering::Relaxed);
        self.headers_ms
            .store(self.started.elapsed().as_millis() as u64, Ordering::Release);
    }
    pub fn body(&self, bytes: usize) {
        self.bytes.store(bytes, Ordering::Relaxed);
    }
    pub(super) fn line(&self, error: Option<Error>) -> String {
        let headers = self.headers_ms.load(Ordering::Acquire);
        let phase = if headers == u64::MAX {
            "before_headers"
        } else {
            "body"
        };
        let outcome = match error {
            None => "ok",
            Some(Error::Timeout) => "timeout",
            Some(Error::Cancelled) => "cancelled",
            Some(Error::Busy) => "busy",
            Some(Error::Http(_)) => "http_error",
            Some(Error::InvalidRange | Error::InvalidResponse) => "invalid_response",
            Some(_) => "network_error",
        };
        // Only fixed tokens and numeric measurements can reach this line.
        format!(
            "octave_http kind={} lane={} outcome={outcome} phase={phase} elapsed_ms={} headers_ms={} status={} body_bytes={}",
            self.kind,
            self.lane,
            self.started.elapsed().as_millis(),
            if headers == u64::MAX {
                -1
            } else {
                headers as i128
            },
            self.status.load(Ordering::Relaxed),
            self.bytes.load(Ordering::Relaxed),
        )
    }
    pub fn finish(&self, error: Option<Error>) {
        static EVENTS: AtomicUsize = AtomicUsize::new(0);
        // Bounded even when a public desktop session explicitly enables tracing.
        if EVENTS.fetch_add(1, Ordering::Relaxed) < 2048 {
            eprintln!("{}", self.line(error));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn diagnostics_distinguish_header_and_body_timeouts_without_request_secrets() {
        let client = reqwest::Client::new();
        let request = client
            .get("https://user:password@example.invalid/youtubei/v1/player?secret=signed")
            .header("Authorization", "Bearer credential")
            .build()
            .unwrap();
        let trace = Trace::new(&request, Expected::Bytes(100), Lane::Media);
        let before = trace.line(Some(Error::Timeout));
        assert!(before.contains("kind=player lane=media outcome=timeout phase=before_headers"));
        assert!(before.contains("headers_ms=-1 status=0 body_bytes=0"));
        trace.headers(200);
        trace.body(42);
        let during = trace.line(Some(Error::Timeout));
        assert!(during.contains("phase=body"));
        assert!(during.contains("status=200 body_bytes=42"));
        for line in [before, during] {
            for secret in [
                "user",
                "password",
                "example",
                "secret",
                "signed",
                "credential",
                "Bearer",
                "https",
                "youtubei",
            ] {
                assert!(!line.contains(secret));
            }
        }
    }
}
