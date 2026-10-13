//! Deterministic, offline-only interaction model for the native experience prototype.
use std::collections::HashSet;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Home,
    Search,
    Library,
    Collection(usize),
    Queue,
    Lyrics,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ViewState {
    #[default]
    Ready,
    Loading,
    Empty,
    Error,
    Offline,
    SignedOut,
}
impl ViewState {
    pub const ALL: [Self; 6] = [
        Self::Ready,
        Self::Loading,
        Self::Empty,
        Self::Error,
        Self::Offline,
        Self::SignedOut,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Ready => "Ready",
            Self::Loading => "Loading",
            Self::Empty => "Empty",
            Self::Error => "Error",
            Self::Offline => "Offline",
            Self::SignedOut => "Signed out",
        }
    }
}
#[derive(Clone)]
pub struct Song {
    pub title: String,
    pub artist: String,
    pub album: usize,
    pub seconds: u32,
    searchable: String,
}
pub struct Collection {
    pub title: &'static str,
    pub artist: &'static str,
    pub kind: &'static str,
    pub description: &'static str,
}
pub const COLLECTIONS: [Collection; 6] = [
    Collection {
        title: "After the rain",
        artist: "Mira Sol",
        kind: "Album",
        description: "Soft edges. Open windows. A new beginning.",
    },
    Collection {
        title: "Golden hour",
        artist: "Octave editorial",
        kind: "Playlist",
        description: "A slower soundtrack for the last light of the day.",
    },
    Collection {
        title: "Quiet geometry",
        artist: "Nolan & the North",
        kind: "Album",
        description: "Small patterns, spacious melodies, room to breathe.",
    },
    Collection {
        title: "Night drive",
        artist: "Octave editorial",
        kind: "Playlist",
        description: "The city slips by. Stay a little longer.",
    },
    Collection {
        title: "Blue in bloom",
        artist: "Ember Coast",
        kind: "Album",
        description: "Warm voices and a little ocean air.",
    },
    Collection {
        title: "Far from here",
        artist: "Octave editorial",
        kind: "Playlist",
        description: "New places, familiar feelings. Follow your curiosity.",
    },
];
pub struct Library {
    pub songs: Vec<Song>,
}
impl Default for Library {
    fn default() -> Self {
        let titles = [
            "In the waking light",
            "Windows open",
            "Somewhere, slowly",
            "The things we leave behind",
            "A softer kind of blue",
            "After the rain",
            "Sun on the kitchen floor",
            "All the way home",
            "Before the evening fades",
            "Little satellites",
            "Nothing to hurry",
            "Golden hour",
            "Parallel lines",
            "Still moving",
            "Quiet geometry",
            "Between the waves",
            "Paper moons",
            "Almost a memory",
            "City after midnight",
            "Signals from the coast",
            "Last train",
            "Rooms with a view",
            "Night drive",
            "Under the overpass",
            "Blue in bloom",
            "Tidepool",
            "Letters to the sea",
            "Small hours",
            "First light",
            "Low tide",
            "東京の朝 · Morning in Tokyo",
            "서울의 밤 · Seoul at night",
            "Απαλό φως · Soft light",
            "Тихий берег · Quiet shore",
            "A very long song title that keeps going, across the city and all the way back home again",
            "Far from here",
        ];
        let songs = (0..10_000)
            .map(|i| {
                let album = (i % 36) / 6;
                let title = if i < 36 {
                    titles[i].to_owned()
                } else {
                    format!("{} — take {}", titles[i % 36], i / 36 + 1)
                };
                let artist = if COLLECTIONS[album].kind == "Album" {
                    COLLECTIONS[album].artist
                } else {
                    [
                        "Juniper Lane",
                        "Noor & Atlas",
                        "Sora",
                        "The Glass Hours",
                        "Mira Sol",
                        "Ember Coast",
                    ][i % 6]
                }
                .to_owned();
                let searchable =
                    format!("{title} {artist} {}", COLLECTIONS[album].title).to_lowercase();
                Song {
                    title,
                    artist,
                    album,
                    seconds: 165 + (i % 9) as u32 * 17,
                    searchable,
                }
            })
            .collect();
        Self { songs }
    }
}
impl Library {
    pub fn search(&self, query: &str, liked: Option<&HashSet<usize>>) -> Vec<usize> {
        let query = query.trim().to_lowercase();
        self.songs
            .iter()
            .enumerate()
            .filter(|(i, s)| {
                s.searchable.contains(&query) && liked.is_none_or(|set| set.contains(i))
            })
            .map(|(i, _)| i)
            .collect()
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Repeat {
    #[default]
    Off,
    All,
    One,
}
pub struct Session {
    pub queue: Vec<usize>,
    pub cursor: usize,
    pub playing: bool,
    pub position: f64,
    pub volume: f32,
    pub repeat: Repeat,
    pub shuffled: bool,
    pub liked: HashSet<usize>,
}
impl Default for Session {
    fn default() -> Self {
        Self {
            queue: (0..6).collect(),
            cursor: 0,
            playing: false,
            position: 42.0,
            volume: 0.7,
            repeat: Repeat::Off,
            shuffled: false,
            liked: [0, 7, 12, 24].into_iter().collect(),
        }
    }
}
impl Session {
    pub fn current(&self) -> Option<usize> {
        self.queue.get(self.cursor).copied()
    }
    pub fn play(&mut self, songs: Vec<usize>, start: usize) {
        self.queue = songs;
        self.cursor = start.min(self.queue.len().saturating_sub(1));
        self.position = 0.0;
        self.playing = !self.queue.is_empty();
    }
    pub fn next(&mut self) {
        if self.cursor + 1 < self.queue.len() {
            self.cursor += 1;
        } else if self.repeat == Repeat::All && !self.queue.is_empty() {
            self.cursor = 0;
        } else {
            self.playing = false;
        }
        self.position = 0.0;
    }
    pub fn previous(&mut self) {
        if self.position <= 3.0 {
            self.cursor = self.cursor.saturating_sub(1);
        }
        self.position = 0.0;
    }
    pub fn tick(&mut self, elapsed: f64, library: &Library) {
        if !self.playing {
            return;
        }
        if let Some(i) = self.current() {
            self.position += elapsed.min(0.25);
            if self.position >= f64::from(library.songs[i].seconds) {
                if self.repeat == Repeat::One {
                    self.position = 0.0;
                } else {
                    self.next();
                }
            }
        }
    }
    pub fn remove(&mut self, index: usize) {
        if index >= self.queue.len() {
            return;
        }
        self.queue.remove(index);
        if index < self.cursor {
            self.cursor -= 1;
        } else if index == self.cursor {
            self.position = 0.0;
        }
        if self.cursor >= self.queue.len() {
            self.cursor = self.queue.len().saturating_sub(1);
        }
        if self.queue.is_empty() {
            self.playing = false;
            self.position = 0.0;
        }
    }
    pub fn move_item(&mut self, from: usize, to: usize) {
        if from >= self.queue.len() || to >= self.queue.len() {
            return;
        }
        self.queue.swap(from, to);
        if self.cursor == from {
            self.cursor = to;
        } else if self.cursor == to {
            self.cursor = from;
        }
    }
    pub fn toggle_like(&mut self, id: usize) {
        if !self.liked.remove(&id) {
            self.liked.insert(id);
        }
    }
    pub fn shuffle(&mut self) {
        // A repeatable rearrangement makes prototype interactions and tests reproducible.
        if self.cursor + 1 < self.queue.len() {
            self.queue[self.cursor + 1..].reverse();
        }
        self.shuffled = !self.shuffled;
    }
}
pub fn time(seconds: f64) -> String {
    let s = seconds.max(0.0) as u32;
    format!("{}:{:02}", s / 60, s % 60)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn removal_and_reordering_keep_current_track() {
        let mut s = Session {
            cursor: 2,
            ..Default::default()
        };
        let current = s.current();
        s.remove(0);
        assert_eq!(s.current(), current);
        s.move_item(1, 3);
        assert_eq!(s.current(), current);
        while !s.queue.is_empty() {
            s.remove(0);
        }
        assert_eq!(s.current(), None);
        assert!(!s.playing);
    }
    #[test]
    fn transport_repeat_and_end_are_predictable() {
        let library = Library::default();
        let mut s = Session::default();
        s.play(vec![0, 1], 0);
        s.next();
        assert_eq!(s.current(), Some(1));
        s.next();
        assert!(!s.playing);
        s.repeat = Repeat::All;
        s.playing = true;
        s.next();
        assert_eq!(s.current(), Some(0));
        s.repeat = Repeat::One;
        s.position = library.songs[0].seconds as f64;
        s.tick(0.1, &library);
        assert_eq!(s.current(), Some(0));
        assert_eq!(s.position, 0.0);
    }
    #[test]
    fn large_library_search_and_unicode() {
        let l = Library::default();
        assert_eq!(l.songs.len(), 10_000);
        assert!(l.search("TOKYO", None).contains(&30));
        assert!(l.search("서울", None).contains(&31));
        assert!(l.search("no such fixture", None).is_empty());
        assert_eq!(
            l.search("", Some(&[3, 4].into_iter().collect())),
            vec![3, 4]
        );
    }
}
