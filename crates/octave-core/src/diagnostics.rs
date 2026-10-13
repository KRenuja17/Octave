//! Typed diagnostics prevent accidental logging of raw service responses.
use std::fmt;
/// Cookies, tokens and authenticated URLs must redact their debug representation.
pub struct Sensitive<T>(T);
impl<T> Sensitive<T> {
    pub fn new(value: T) -> Self {
        Self(value)
    }
    /// Explicit transport/storage access; never pass this value to a logger.
    pub fn expose(&self) -> &T {
        &self.0
    }
}
impl<T> fmt::Debug for Sensitive<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[REDACTED]")
    }
}
#[derive(Clone, Copy, Debug)]
pub enum Event {
    ApplicationStarted,
    WindowReady,
    ApplicationStopped,
}
impl Event {
    pub fn as_json(self) -> &'static str {
        match self {
            Self::ApplicationStarted => r#"{"event":"application_started"}"#,
            Self::WindowReady => r#"{"event":"window_ready"}"#,
            Self::ApplicationStopped => r#"{"event":"application_stopped"}"#,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::Sensitive;
    #[test]
    fn nested_debug_output_redacts_credentials() {
        let secret = Sensitive::new("session-cookie-do-not-log".to_owned());
        assert_eq!(secret.expose(), "session-cookie-do-not-log");
        let output = format!("{:#?}", Some(&secret));
        assert!(output.contains("[REDACTED]"));
        assert!(!output.contains("session-cookie"));
    }
}
