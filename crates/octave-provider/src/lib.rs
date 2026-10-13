//! Provider boundary. Network access and resolution follow in M1.
use octave_core::{TrackId, diagnostics::Sensitive};
/// Transient transport result; never persist as a track's identity.
#[derive(Debug)]
pub struct ResolvedStream {
    pub track_id: TrackId,
    pub url: Sensitive<String>,
}
