//! Durable listening state. Queue order is explicit, including after shuffle.
use octave_core::Track;

pub const MAX_QUEUE: usize = 500;
#[derive(Clone, Debug, PartialEq)]
pub struct ViewPreferences {
    pub panel_open: bool,
    pub panel_width: f32,
    pub panel_tab: u8,
    pub lyrics_only: bool,
    pub reduced_motion: bool,
}
impl Default for ViewPreferences {
    fn default() -> Self {
        Self {
            panel_open: false,
            panel_width: 360.0,
            panel_tab: 0,
            lyrics_only: false,
            reduced_motion: false,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Repeat {
    #[default]
    Off,
    All,
    One,
}
impl Repeat {
    pub fn next(self) -> Self {
        match self {
            Self::Off => Self::All,
            Self::All => Self::One,
            Self::One => Self::Off,
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct Session {
    pub view: ViewPreferences,
    pub crossfade: u8,
    pub equalizer: octave_player::Equalizer,
    pub appearance: u8,
    pub queue: Vec<Track>,
    pub cursor: usize,
    pub position: f64,
    pub volume: f32,
    pub repeat: Repeat,
}
impl Default for Session {
    fn default() -> Self {
        Self {
            view: ViewPreferences::default(),
            crossfade: 0,
            equalizer: octave_player::Equalizer::default(),
            appearance: 0,
            queue: Vec::new(),
            cursor: 0,
            position: 0.0,
            volume: 0.7,
            repeat: Repeat::Off,
        }
    }
}
impl Session {
    /// The entry that automatic completion would select, without changing the cursor.
    pub fn upcoming(&self) -> Option<&Track> {
        if self.repeat == Repeat::One {
            self.current()
        } else {
            self.queue.get(self.cursor + 1).or_else(|| {
                (self.repeat == Repeat::All)
                    .then(|| self.queue.first())
                    .flatten()
            })
        }
    }
    pub fn current(&self) -> Option<&Track> {
        self.queue.get(self.cursor)
    }
    pub fn append(&mut self, track: Track, next: bool) -> bool {
        if self.queue.len() >= MAX_QUEUE || self.queue.iter().any(|t| t.id == track.id) {
            return false;
        }
        let index = if next && self.current().is_some() {
            self.cursor + 1
        } else {
            self.queue.len()
        };
        self.queue.insert(index, track);
        true
    }
    pub fn select(&mut self, index: usize) {
        if index < self.queue.len() {
            self.cursor = index;
            self.position = 0.0;
        }
    }
    /// Automatic completion honors repeat-one; a manual next always advances.
    pub fn advance(&mut self, automatic: bool) -> bool {
        if self.queue.is_empty() {
            return false;
        }
        if automatic && self.repeat == Repeat::One {
            self.position = 0.0;
            return true;
        }
        if self.cursor + 1 < self.queue.len() {
            self.cursor += 1;
        } else if self.repeat == Repeat::All {
            self.cursor = 0;
        } else {
            self.position = 0.0;
            return false;
        }
        self.position = 0.0;
        true
    }
    pub fn previous(&mut self) {
        if self.position <= 3.0 {
            self.cursor = self.cursor.saturating_sub(1);
        }
        self.position = 0.0;
    }
    pub fn remove(&mut self, index: usize) -> bool {
        if index >= self.queue.len() {
            return false;
        }
        let current_removed = index == self.cursor;
        self.queue.remove(index);
        if index < self.cursor || self.cursor >= self.queue.len() {
            self.cursor = self.cursor.saturating_sub(1);
        }
        if current_removed {
            self.position = 0.0;
        }
        current_removed
    }
    pub fn move_item(&mut self, from: usize, to: usize) {
        if from >= self.queue.len() || to >= self.queue.len() {
            return;
        }
        let selected = self.current().map(|t| t.id.clone());
        let track = self.queue.remove(from);
        self.queue.insert(to, track);
        self.cursor = self
            .queue
            .iter()
            .position(|t| Some(&t.id) == selected.as_ref())
            .unwrap_or(0);
    }
    /// Fisher–Yates on upcoming tracks; seed permits reproducible tests.
    pub fn shuffle(&mut self, mut seed: u64) {
        seed = seed.max(1);
        let start = (self.cursor + 1).min(self.queue.len());
        for i in ((start + 1)..self.queue.len()).rev() {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let j = start + seed as usize % (i - start + 1);
            self.queue.swap(i, j);
        }
    }
}
#[cfg(test)]
pub fn track(id: usize) -> Track {
    Track {
        id: octave_core::TrackId(format!("{id:011}")),
        title: format!("Song {id}"),
        artists: vec!["Artist".into()],
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn edits_preserve_current_and_deduplicate() {
        let mut s = Session::default();
        for i in 0..8 {
            assert!(s.append(track(i), false));
        }
        s.select(3);
        s.position = 12.0;
        assert!(!s.append(track(3), true));
        s.move_item(3, 6);
        assert_eq!(s.current(), Some(&track(3)));
        assert!(!s.remove(0));
        assert_eq!(s.current(), Some(&track(3)));
        s.shuffle(55);
        assert_eq!(s.current(), Some(&track(3)));
        assert_eq!(s.position, 12.0);
        assert!(s.remove(s.cursor));
        assert_eq!(s.position, 0.0);
        while !s.queue.is_empty() {
            s.remove(0);
        }
        assert!(s.current().is_none());
        assert_eq!(s.cursor, 0);
    }
    #[test]
    fn repeat_and_end_have_explicit_semantics() {
        let mut s = Session::default();
        s.append(track(1), false);
        s.append(track(2), false);
        s.repeat = Repeat::One;
        assert_eq!(s.upcoming(), Some(&track(1)));
        assert!(s.advance(true));
        assert_eq!(s.cursor, 0);
        assert!(s.advance(false));
        assert_eq!(s.cursor, 1);
        s.repeat = Repeat::Off;
        assert_eq!(s.upcoming(), None);
        assert!(!s.advance(true));
        s.repeat = Repeat::All;
        assert_eq!(s.upcoming(), Some(&track(1)));
        assert!(s.advance(true));
        assert_eq!(s.cursor, 0);
    }
    #[test]
    fn shuffle_is_reproducible_and_keeps_all_tracks() {
        let mut s = Session::default();
        for i in 0..20 {
            s.append(track(i), false);
        }
        let mut other = s.clone();
        s.shuffle(100);
        other.shuffle(100);
        assert_eq!(s, other);
        assert_eq!(s.current(), Some(&track(0)));
        assert_ne!(s.queue, (0..20).map(track).collect::<Vec<_>>());
        for i in 0..20 {
            assert!(s.queue.contains(&track(i)));
        }
    }
}
