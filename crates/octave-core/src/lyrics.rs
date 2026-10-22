//! Service-authored lyric timing and user-owned reading state.
use crate::TrackId;
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lyrics {
    pub track_id: TrackId,
    pub text: String,
    pub source: String,
    /// Empty for plain lyrics. Timings are never estimated from text.
    pub cues: Vec<LyricCue>,
}

/// A line occurrence. IDs are stable within one document, including repeated text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LyricCue {
    pub id: usize,
    pub text: String,
    pub start_ms: u64,
    pub end_ms: u64,
}

impl Lyrics {
    /// Ranges are half-open. Gaps and the end of the document have no active line.
    /// Provider validation guarantees sorted, nonoverlapping cue ranges.
    pub fn active_at(&self, position: Duration) -> Option<usize> {
        let ms = position.as_millis();
        self.anchor_at(position).filter(|&i| {
            u128::from(self.cues[i].start_ms) <= ms && ms < u128::from(self.cues[i].end_ms)
        })
    }
    /// Read near the preceding line in gaps, without highlighting it.
    pub fn anchor_at(&self, position: Duration) -> Option<usize> {
        if self.cues.is_empty() {
            return None;
        }
        Some(
            self.cues
                .partition_point(|cue| u128::from(cue.start_ms) <= position.as_millis())
                .saturating_sub(1),
        )
    }
}

#[derive(Clone, Debug)]
pub struct Reading {
    pub following: bool,
    active: Option<usize>,
    recenter: bool,
}
impl Default for Reading {
    fn default() -> Self {
        Self {
            following: true,
            active: None,
            recenter: true,
        }
    }
}
impl Reading {
    pub fn manual(&mut self) {
        self.following = false;
        self.recenter = false;
    }
    pub fn resume(&mut self) {
        self.following = true;
        self.recenter = true;
    }
    pub fn reflow(&mut self) {
        if self.following {
            self.recenter = true;
        }
    }
    /// Call with the consumed-audio position. Repeated paused/buffering frames
    /// never advance this controller; manual reading survives cue changes/seeks.
    pub fn update(&mut self, active: Option<usize>) -> bool {
        let changed = self.active != active;
        self.active = active;
        let center = self.following && active.is_some() && (changed || self.recenter);
        if center {
            self.recenter = false;
        }
        center
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn consumed_clock_handles_pause_seek_gaps_and_repeat() {
        let doc = Lyrics {
            track_id: TrackId("fixture".into()),
            text: "A\nA".into(),
            source: "Test".into(),
            cues: vec![
                LyricCue {
                    id: 0,
                    text: "A".into(),
                    start_ms: 1000,
                    end_ms: 2000,
                },
                LyricCue {
                    id: 1,
                    text: "A".into(),
                    start_ms: 3000,
                    end_ms: 4000,
                },
            ],
        };
        let at = |ms| doc.active_at(Duration::from_millis(ms));
        assert_eq!(doc.anchor_at(Duration::from_millis(2500)), Some(0));
        assert_eq!(
            [at(0), at(1000), at(1999), at(2000), at(3000), at(4000)],
            [None, Some(0), Some(0), None, Some(1), None]
        );
        let mut reading = Reading::default();
        assert!(reading.update(at(1000)));
        assert!(!reading.update(at(1000))); // paused/buffering
        reading.manual();
        assert!(!reading.update(at(3000)));
        assert!(!reading.update(at(1000))); // backward seek/repeat cannot steal reading
        reading.resume();
        assert!(reading.update(at(1000)));
        assert!(!reading.update(at(1000)));
        assert!(!reading.update(at(2500)));
        assert!(reading.update(at(3000)));
    }
}
