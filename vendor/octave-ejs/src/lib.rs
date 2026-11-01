//! The MIT-licensed SWC preprocessor subset; no runtime, I/O or tracing.
pub mod preprocessor;
mod polyfill {
    pub const INTL_POLYFILL: &str = include_str!("polyfill/intl.js");
    pub const SETUP_CODE: &str = include_str!("polyfill/setup.js");
}
mod provider {
    #[derive(Debug)]
    pub enum JsChallengeError {
        Parse(String),
        Preprocess(String),
        Runtime(String),
    }
    impl std::fmt::Display for JsChallengeError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            match self {
                Self::Parse(s) | Self::Preprocess(s) | Self::Runtime(s) => f.write_str(s),
            }
        }
    }
}
