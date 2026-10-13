//! Playback boundary. Foundation startup opens no audio device.
use octave_core::TrackId;
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum PlaybackState {
    #[default]
    Idle,
    Loading(TrackId),
    Playing(TrackId),
    Paused(TrackId),
}
