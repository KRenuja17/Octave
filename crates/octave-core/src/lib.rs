//! Application-owned types shared by the desktop, provider and player.
pub mod diagnostics;
/// Stable content identity; never an expiring stream URL.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TrackId(pub String);
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Track {
    pub id: TrackId,
    pub title: String,
    pub artists: Vec<String>,
}
