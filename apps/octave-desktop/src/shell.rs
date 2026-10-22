//! Navigation history stores destinations, never play intent or queue copies.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Destination {
    #[default]
    Search,
    Catalog(String),
    Listening(u8),
    Playlists,
    Account,
    Settings,
}
#[derive(Default)]
pub struct History {
    back: Vec<Destination>,
    forward: Vec<Destination>,
    current: Option<Destination>,
}
impl History {
    pub fn record(&mut self, destination: Destination) {
        if self.current.as_ref() == Some(&destination) {
            return;
        }
        if let Some(current) = self.current.replace(destination) {
            self.back.push(current);
        }
        if self.back.len() > 64 {
            self.back.remove(0);
        }
        self.forward.clear();
    }
    pub fn back(&mut self) -> Option<Destination> {
        let target = self.back.pop()?;
        if let Some(current) = self.current.replace(target.clone()) {
            self.forward.push(current);
        }
        Some(target)
    }
    pub fn forward(&mut self) -> Option<Destination> {
        let target = self.forward.pop()?;
        if let Some(current) = self.current.replace(target.clone()) {
            self.back.push(current);
        }
        Some(target)
    }
    pub fn can_back(&self) -> bool {
        !self.back.is_empty()
    }
    pub fn can_forward(&self) -> bool {
        !self.forward.is_empty()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn branching_history_keeps_destinations_without_replaying_actions() {
        let mut h = History::default();
        h.record(Destination::Search);
        h.record(Destination::Listening(2));
        h.record(Destination::Playlists);
        assert_eq!(h.back(), Some(Destination::Listening(2)));
        assert_eq!(h.forward(), Some(Destination::Playlists));
        h.back();
        h.record(Destination::Settings);
        assert_eq!(h.forward(), None);
        assert_eq!(h.back(), Some(Destination::Listening(2)));
    }
}
