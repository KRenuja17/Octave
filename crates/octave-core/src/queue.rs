//! Explicit occurrence identity; video identity alone cannot identify duplicates.
use crate::Track;
static INSTANCES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
#[derive(Clone, Debug)]
pub struct Queue {
    instance: u64,
    revision: u64,
    tracks: Vec<Track>,
    ids: Vec<u64>,
    next_id: u64,
    pub original_order: Option<Vec<u64>>,
}
impl Default for Queue {
    fn default() -> Self {
        Self {
            instance: INSTANCES.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            revision: 0,
            tracks: Vec::new(),
            ids: Vec::new(),
            next_id: 1,
            original_order: None,
        }
    }
}
impl PartialEq for Queue {
    fn eq(&self, other: &Self) -> bool {
        self.tracks == other.tracks
            && self.ids == other.ids
            && self.original_order == other.original_order
    }
}
impl PartialEq<Vec<Track>> for Queue {
    fn eq(&self, other: &Vec<Track>) -> bool {
        self.tracks == *other
    }
}
impl From<Vec<Track>> for Queue {
    fn from(tracks: Vec<Track>) -> Self {
        let mut queue = Self::default();
        for track in tracks {
            queue.push(track);
        }
        queue
    }
}
impl std::ops::Deref for Queue {
    type Target = [Track];
    fn deref(&self) -> &[Track] {
        &self.tracks
    }
}
impl Queue {
    pub fn from_entries(entries: Vec<(u64, Track)>, order: Option<Vec<u64>>) -> Option<Self> {
        let mut queue = Self::default();
        let mut seen = std::collections::HashSet::new();
        for (id, track) in entries {
            if id == 0 || id >= i64::MAX as u64 || !seen.insert(id) {
                return None;
            }
            queue.tracks.push(track);
            queue.ids.push(id);
            queue.next_id = queue.next_id.max(id + 1);
        }
        queue.original_order = order;
        Some(queue)
    }
    pub fn revision(&self) -> (u64, u64) {
        (self.instance, self.revision)
    }
    pub fn id_at(&self, index: usize) -> Option<u64> {
        self.ids.get(index).copied()
    }
    pub fn push(&mut self, track: Track) {
        self.insert(self.len(), track);
    }
    pub fn insert(&mut self, index: usize, track: Track) {
        self.revision = self.revision.wrapping_add(1);
        let id = self.next_id;
        self.next_id += 1;
        self.tracks.insert(index, track);
        self.ids.insert(index, id);
    }
    pub fn remove(&mut self, index: usize) -> Track {
        self.revision = self.revision.wrapping_add(1);
        self.ids.remove(index);
        self.tracks.remove(index)
    }
    pub fn clear(&mut self) {
        self.revision = self.revision.wrapping_add(1);
        self.tracks.clear();
        self.ids.clear();
        self.original_order = None;
    }
    pub fn truncate(&mut self, len: usize) {
        self.revision = self.revision.wrapping_add(1);
        self.tracks.truncate(len);
        self.ids.truncate(len);
    }
    pub fn swap(&mut self, a: usize, b: usize) {
        self.revision = self.revision.wrapping_add(1);
        self.tracks.swap(a, b);
        self.ids.swap(a, b);
    }
    pub fn move_entry(&mut self, from: usize, to: usize) {
        self.revision = self.revision.wrapping_add(1);
        let id = self.ids.remove(from);
        let track = self.tracks.remove(from);
        self.ids.insert(to, id);
        self.tracks.insert(to, track);
    }
    pub fn remember_order(&mut self) {
        if !self.is_empty() && self.original_order.is_none() {
            self.original_order = Some(self.ids.clone());
        }
    }
    /// Preserve played/current prefix. Removed entries stay removed; additions
    /// follow surviving original entries and retain their relative order.
    pub fn restore_upcoming(&mut self, start: usize) {
        self.revision = self.revision.wrapping_add(1);
        if let Some(order) = self.original_order.take() {
            let ranks: std::collections::HashMap<_, _> = order
                .into_iter()
                .enumerate()
                .map(|(i, id)| (id, i))
                .collect();
            let mut indices: Vec<usize> = (start.min(self.len())..self.len()).collect();
            indices.sort_by_key(|&i| ranks.get(&self.ids[i]).copied().unwrap_or(usize::MAX));
            let old = self.clone();
            for (to, from) in (start..self.len()).zip(indices) {
                self.tracks[to] = old.tracks[from].clone();
                self.ids[to] = old.ids[from];
            }
        }
    }
}
/// Rejection sampling avoids modulo bias. Entropy failure is propagated.
pub fn random_below<E>(
    bound: u64,
    mut entropy: impl FnMut() -> Result<u64, E>,
) -> Result<usize, E> {
    assert!(bound > 0);
    let threshold = bound.wrapping_neg() % bound;
    loop {
        let sample = entropy()?;
        if sample >= threshold {
            return Ok((sample % bound) as usize);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn track() -> Track {
        Track {
            id: crate::TrackId("same-recording".into()),
            title: "Original fixture".into(),
            artists: Vec::new(),
        }
    }
    #[test]
    fn duplicate_occurrences_keep_identity_through_moves_and_restore() {
        let mut q = Queue::from(vec![track(), track(), track()]);
        let current = q.id_at(0);
        q.remember_order();
        q.swap(1, 2);
        q.move_entry(0, 1);
        assert_eq!(q.id_at(1), current);
        q.restore_upcoming(0);
        assert_eq!(q.id_at(0), current);
        let entries = q
            .iter()
            .enumerate()
            .map(|(i, t)| (q.id_at(i).unwrap(), t.clone()))
            .collect();
        assert_eq!(Queue::from_entries(entries, None).unwrap(), q);
    }
    #[test]
    fn bounded_random_rejects_biased_tail_and_propagates_failure() {
        let mut values = [0, 9].into_iter();
        assert_eq!(
            random_below(10, || Ok::<_, ()>(values.next().unwrap())),
            Ok(9)
        );
        assert_eq!(
            random_below(10, || Err::<u64, _>("entropy unavailable")),
            Err("entropy unavailable")
        );
    }
}
