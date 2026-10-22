//! Application-owned types shared by the desktop, provider and player.
pub mod diagnostics;
pub mod lyrics;
/// Stable content identity; never an expiring stream URL.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TrackId(pub String);
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Track {
    pub id: TrackId,
    pub title: String,
    pub artists: Vec<String>,
}

/// Selected AAC stream metadata and the format actually returned by the decoder.
/// Bitrate is the provider's report, not a measurement or a Premium entitlement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioQuality {
    pub bitrate_bps: Option<u32>,
    pub sample_rate: u32,
    pub channels: u16,
}
impl std::fmt::Display for AudioQuality {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AAC · ")?;
        match self.bitrate_bps {
            Some(bitrate) => write!(f, "{} kb/s reported", bitrate / 1000)?,
            None => f.write_str("bitrate not reported")?,
        }
        write!(f, " · {} Hz · {} ch", self.sample_rate, self.channels)
    }
}
