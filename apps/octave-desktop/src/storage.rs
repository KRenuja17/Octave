//! SQLite runs on one worker. Each queue/settings snapshot commits atomically.
use crate::listening::{MAX_QUEUE, Repeat, Session};
use octave_core::{Track, TrackId};
use rusqlite::{Connection, params};
use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver, SyncSender},
    thread::JoinHandle,
    time::Duration,
};

pub enum Event {
    Loaded(Session),
    Saved(Session),
    Failed,
}
pub struct Store {
    tx: Option<SyncSender<Session>>,
    pub events: Receiver<Event>,
    worker: Option<JoinHandle<()>>,
}
impl Store {
    pub fn open(path: PathBuf) -> Self {
        let (tx, rx) = mpsc::sync_channel(1);
        let (events_tx, events) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let opened = (|| {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent).map_err(|_| ())?;
                }
                let mut db = Connection::open(path).map_err(|_| ())?;
                db.busy_timeout(Duration::from_secs(2)).map_err(|_| ())?;
                migrate(&mut db).map_err(|_| ())?;
                let session = load(&db).map_err(|_| ())?;
                events_tx.send(Event::Loaded(session)).map_err(|_| ())?;
                Ok::<_, ()>(db)
            })();
            let Ok(mut db) = opened else {
                let _ = events_tx.send(Event::Failed);
                return;
            };
            while let Ok(session) = rx.recv() {
                let event = if save(&mut db, &session).is_ok() {
                    Event::Saved(session)
                } else {
                    Event::Failed
                };
                let _ = events_tx.send(event);
            }
        });
        Self {
            tx: Some(tx),
            events,
            worker: Some(worker),
        }
    }
    pub fn save(&self, session: Session) -> bool {
        self.tx
            .as_ref()
            .is_some_and(|tx| tx.try_send(session).is_ok())
    }
    pub fn finish(&mut self, session: Option<Session>) {
        if let Some(tx) = self.tx.take() {
            if let Some(session) = session {
                let _ = tx.send(session);
            }
            drop(tx);
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
impl Drop for Store {
    fn drop(&mut self) {
        self.finish(None);
    }
}
fn migrate(db: &mut Connection) -> rusqlite::Result<()> {
    let version: u32 = db.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version > 7 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    if version == 0 {
        let tx = db.transaction()?;
        tx.execute_batch("CREATE TABLE queue (slot INTEGER PRIMARY KEY, id TEXT NOT NULL UNIQUE, title TEXT NOT NULL, artists TEXT NOT NULL);
            CREATE TABLE session (singleton INTEGER PRIMARY KEY CHECK(singleton=1), cursor INTEGER NOT NULL, position REAL NOT NULL, volume REAL NOT NULL, repeat INTEGER NOT NULL);
            PRAGMA user_version=1;")?;
        tx.commit()?;
    }
    if version < 2 {
        let tx = db.transaction()?;
        tx.execute_batch("CREATE TABLE preferences (singleton INTEGER PRIMARY KEY CHECK(singleton=1), bass REAL NOT NULL, mid REAL NOT NULL, treble REAL NOT NULL, appearance INTEGER NOT NULL); PRAGMA user_version=2;")?;
        tx.commit()?;
    }
    if version < 3 {
        let tx = db.transaction()?;
        tx.execute_batch("ALTER TABLE preferences ADD COLUMN crossfade INTEGER NOT NULL DEFAULT 0; PRAGMA user_version=3;")?;
        tx.commit()?;
    }
    if version < 4 {
        db.execute_batch("CREATE TABLE IF NOT EXISTS view_preferences (singleton INTEGER PRIMARY KEY CHECK(singleton=1), panel_open INTEGER NOT NULL, panel_width REAL NOT NULL, panel_tab INTEGER NOT NULL, lyrics_only INTEGER NOT NULL, reduced_motion INTEGER NOT NULL); PRAGMA user_version=4;")?;
    }
    if version < 5 {
        let tx = db.transaction()?;
        tx.execute_batch("ALTER TABLE queue RENAME TO queue_v4; CREATE TABLE queue (slot INTEGER PRIMARY KEY, entry_id INTEGER NOT NULL UNIQUE, id TEXT NOT NULL, title TEXT NOT NULL, artists TEXT NOT NULL); INSERT INTO queue SELECT slot,slot+1,id,title,artists FROM queue_v4; DROP TABLE queue_v4; CREATE TABLE queue_shuffle_order (slot INTEGER PRIMARY KEY, entry_id INTEGER NOT NULL UNIQUE); PRAGMA user_version=5;")?;
        tx.commit()?;
    }
    if version < 6 {
        let tx = db.transaction()?;
        tx.execute_batch("ALTER TABLE preferences ADD COLUMN continuous INTEGER NOT NULL DEFAULT 1; ALTER TABLE preferences ADD COLUMN preload INTEGER NOT NULL DEFAULT 1; ALTER TABLE view_preferences ADD COLUMN auto_lyrics INTEGER NOT NULL DEFAULT 0; PRAGMA user_version=6;")?;
        tx.commit()?;
    }
    if version < 7 {
        let tx = db.transaction()?;
        tx.execute_batch("ALTER TABLE view_preferences ADD COLUMN translation_endpoint TEXT NOT NULL DEFAULT ''; ALTER TABLE view_preferences ADD COLUMN translation_target TEXT NOT NULL DEFAULT 'en'; ALTER TABLE view_preferences ADD COLUMN discord_id TEXT NOT NULL DEFAULT ''; ALTER TABLE view_preferences ADD COLUMN discord_enabled INTEGER NOT NULL DEFAULT 0; PRAGMA user_version=7;")?;
        tx.commit()?;
    }
    Ok(())
}
fn save(db: &mut Connection, s: &Session) -> rusqlite::Result<()> {
    if s.queue.len() > MAX_QUEUE {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let tx = db.transaction()?;
    tx.execute("DELETE FROM queue", [])?;
    for (i, track) in s.queue.iter().enumerate() {
        tx.execute(
            "INSERT INTO queue VALUES (?1,?2,?3,?4,?5)",
            params![
                i as i64,
                s.queue.id_at(i).ok_or(rusqlite::Error::InvalidQuery)? as i64,
                track.id.0,
                track.title,
                serde_json::to_string(&track.artists).unwrap()
            ],
        )?;
    }
    tx.execute("DELETE FROM queue_shuffle_order", [])?;
    if let Some(order) = &s.queue.original_order {
        for (slot, id) in order.iter().enumerate() {
            tx.execute(
                "INSERT INTO queue_shuffle_order VALUES (?1,?2)",
                params![slot as i64, *id as i64],
            )?;
        }
    }
    tx.execute(
        "INSERT OR REPLACE INTO session VALUES (1,?1,?2,?3,?4)",
        params![
            s.cursor as i64,
            s.position,
            s.volume,
            match s.repeat {
                Repeat::Off => 0,
                Repeat::All => 1,
                Repeat::One => 2,
            }
        ],
    )?;
    let gains = s.equalizer.sanitized().gains_db;
    tx.execute(
        "INSERT OR REPLACE INTO view_preferences VALUES (1,?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
        params![
            s.view.panel_open,
            s.view.panel_width.clamp(300.0, 440.0),
            s.view.panel_tab.min(2),
            s.view.lyrics_only,
            s.view.reduced_motion,
            s.view.auto_lyrics,
            s.view.translation_endpoint,
            s.view.translation_target,
            s.view.discord_id,
            s.view.discord_enabled
        ],
    )?;
    tx.execute(
        "INSERT OR REPLACE INTO preferences VALUES (1,?1,?2,?3,?4,?5,?6,?7)",
        params![
            gains[0],
            gains[1],
            gains[2],
            s.appearance.min(2),
            s.crossfade.min(octave_player::MAX_CROSSFADE_SECONDS),
            s.continuous,
            s.preload
        ],
    )?;
    tx.commit()
}
fn load(db: &Connection) -> rusqlite::Result<Session> {
    let mut s = Session::default();
    let mut stmt =
        db.prepare("SELECT entry_id,id,title,artists FROM queue ORDER BY slot LIMIT 10001")?;
    let entries = stmt
        .query_map([], |row| {
            let artists: String = row.get(3)?;
            Ok((
                row.get::<_, i64>(0)? as u64,
                Track {
                    id: TrackId(row.get(1)?),
                    title: row.get(2)?,
                    artists: serde_json::from_str(&artists)
                        .map_err(|_| rusqlite::Error::InvalidQuery)?,
                },
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    if entries.len() > MAX_QUEUE {
        return Err(rusqlite::Error::InvalidQuery);
    }
    let mut stmt =
        db.prepare("SELECT entry_id FROM queue_shuffle_order ORDER BY slot LIMIT 10001")?;
    let order = stmt
        .query_map([], |row| row.get::<_, i64>(0).map(|id| id as u64))?
        .collect::<Result<Vec<_>, _>>()?;
    if order.len() > MAX_QUEUE {
        return Err(rusqlite::Error::InvalidQuery);
    }
    s.queue =
        octave_core::queue::Queue::from_entries(entries, (!order.is_empty()).then_some(order))
            .ok_or(rusqlite::Error::InvalidQuery)?;
    let mut stmt =
        db.prepare("SELECT cursor,position,volume,repeat FROM session WHERE singleton=1")?;
    let mut rows = stmt.query([])?;
    if let Some(row) = rows.next()? {
        s.cursor = (row.get::<_, i64>(0)?.max(0) as usize).min(s.queue.len().saturating_sub(1));
        s.position = row.get::<_, f64>(1)?.clamp(0.0, 86400.0);
        s.volume = row.get::<_, f32>(2)?.clamp(0.0, 1.0);
        s.repeat = match row.get::<_, i64>(3)? {
            1 => Repeat::All,
            2 => Repeat::One,
            _ => Repeat::Off,
        };
    }
    let mut stmt = db.prepare(
        "SELECT bass,mid,treble,appearance,crossfade,continuous,preload FROM preferences WHERE singleton=1",
    )?;
    let mut rows = stmt.query([])?;
    if let Some(row) = rows.next()? {
        s.equalizer = octave_player::Equalizer {
            gains_db: [row.get(0)?, row.get(1)?, row.get(2)?],
        }
        .sanitized();
        s.appearance = row.get::<_, i64>(3)?.clamp(0, 2) as u8;
        s.crossfade = row
            .get::<_, i64>(4)?
            .clamp(0, i64::from(octave_player::MAX_CROSSFADE_SECONDS)) as u8;
        s.continuous = row.get(5)?;
        s.preload = row.get(6)?;
    }
    let mut stmt = db.prepare("SELECT panel_open,panel_width,panel_tab,lyrics_only,reduced_motion,auto_lyrics,translation_endpoint,translation_target,discord_id,discord_enabled FROM view_preferences WHERE singleton=1")?;
    let mut rows = stmt.query([])?;
    if let Some(row) = rows.next()? {
        s.view.panel_open = row.get(0)?;
        s.view.panel_width = row.get::<_, f32>(1)?.clamp(300.0, 440.0);
        s.view.panel_tab = row.get::<_, i64>(2)?.clamp(0, 2) as u8;
        s.view.lyrics_only = row.get(3)?;
        s.view.reduced_motion = row.get(4)?;
        s.view.auto_lyrics = row.get(5)?;
        s.view.translation_endpoint = row.get(6)?;
        s.view.translation_target = row.get(7)?;
        s.view.discord_id = row.get(8)?;
        s.view.discord_enabled = row.get(9)?;
    }
    Ok(s)
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::listening::track;
    #[test]
    fn large_duplicate_queue_preserves_occurrence_ids_and_restore_order() {
        let mut db = Connection::open_in_memory().unwrap();
        migrate(&mut db).unwrap();
        let mut state = Session {
            queue: (0..1753).map(|i| track(i % 100)).collect::<Vec<_>>().into(),
            cursor: 51,
            position: 42.0,
            ..Default::default()
        };
        let current = state.queue.id_at(state.cursor);
        state.shuffle_random().unwrap();
        save(&mut db, &state).unwrap();
        let mut restored = load(&db).unwrap();
        assert_eq!(restored, state);
        assert_eq!(restored.queue.id_at(restored.cursor), current);
        restored.queue.restore_upcoming(restored.cursor + 1);
        assert_eq!(restored.queue.len(), 1753);
        assert!(
            restored
                .queue
                .iter()
                .filter(|t| t.id == track(0).id)
                .count()
                > 1
        );
    }
    #[test]
    fn migration_and_roundtrip_do_not_store_play_intent() {
        let mut db = Connection::open_in_memory().unwrap();
        migrate(&mut db).unwrap();
        migrate(&mut db).unwrap();
        let mut s = Session::default();
        s.append(track(1), false);
        s.position = 42.5;
        s.volume = 0.25;
        s.repeat = Repeat::All;
        s.equalizer = octave_player::Equalizer {
            gains_db: [6.0, -3.0, 2.0],
        };
        s.appearance = 2;
        s.crossfade = 12;
        s.continuous = false;
        s.preload = false;
        s.view.auto_lyrics = true;
        s.view.panel_open = true;
        s.view.panel_width = 410.0;
        s.view.reduced_motion = true;
        save(&mut db, &s).unwrap();
        assert_eq!(load(&db).unwrap(), s);
        assert_eq!(
            db.pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))
                .unwrap(),
            7
        );
    }
    #[test]
    fn version_two_preserves_sound_preferences_when_adding_transition() {
        let mut db = Connection::open_in_memory().unwrap();
        migrate(&mut db).unwrap();
        let mut s = Session::default();
        s.append(track(1), false);
        s.equalizer.gains_db = [2.0, -3.0, 4.0];
        s.appearance = 2;
        save(&mut db, &s).unwrap();
        db.execute_batch("ALTER TABLE preferences RENAME TO preferences_v6; CREATE TABLE preferences (singleton INTEGER PRIMARY KEY,bass REAL NOT NULL,mid REAL NOT NULL,treble REAL NOT NULL,appearance INTEGER NOT NULL); INSERT INTO preferences SELECT singleton,bass,mid,treble,appearance FROM preferences_v6; DROP TABLE preferences_v6; DROP TABLE view_preferences; ALTER TABLE queue RENAME TO queue_v5; CREATE TABLE queue (slot INTEGER PRIMARY KEY,id TEXT NOT NULL UNIQUE,title TEXT NOT NULL,artists TEXT NOT NULL); INSERT INTO queue SELECT slot,id,title,artists FROM queue_v5; DROP TABLE queue_v5; DROP TABLE queue_shuffle_order; PRAGMA user_version=2;")
            .unwrap();
        migrate(&mut db).unwrap();
        assert_eq!(load(&db).unwrap(), s);
    }
    #[test]
    fn version_one_migration_preserves_old_listening_state() {
        let mut db = Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE queue (slot INTEGER PRIMARY KEY, id TEXT NOT NULL UNIQUE, title TEXT NOT NULL, artists TEXT NOT NULL); CREATE TABLE session (singleton INTEGER PRIMARY KEY CHECK(singleton=1), cursor INTEGER NOT NULL, position REAL NOT NULL, volume REAL NOT NULL, repeat INTEGER NOT NULL); PRAGMA user_version=1; INSERT INTO queue VALUES (0,'abcdefghijk','Old queue','[]'); INSERT INTO session VALUES (1,0,42.0,0.25,1);").unwrap();
        migrate(&mut db).unwrap();
        let state = load(&db).unwrap();
        assert_eq!(state.queue[0].title, "Old queue");
        assert_eq!(state.position, 42.0);
        assert_eq!(state.volume, 0.25);
        assert_eq!(state.equalizer, octave_player::Equalizer::default());
        assert_eq!(state.appearance, 0);
    }
    #[test]
    fn failed_snapshot_rolls_back_queue_and_settings() {
        let mut db = Connection::open_in_memory().unwrap();
        migrate(&mut db).unwrap();
        let mut s = Session::default();
        s.append(track(1), false);
        save(&mut db, &s).unwrap();
        let before = s.clone();
        db.execute_batch("CREATE TRIGGER fail_queue BEFORE INSERT ON queue WHEN NEW.slot=1 BEGIN SELECT RAISE(FAIL,'Injected write failure'); END;").unwrap();
        s.queue.push(track(1));
        s.volume = 0.1;
        assert!(save(&mut db, &s).is_err());
        assert_eq!(load(&db).unwrap(), before);
    }
    #[test]
    fn future_database_is_not_overwritten() {
        let mut db = Connection::open_in_memory().unwrap();
        db.pragma_update(None, "user_version", 99).unwrap();
        assert!(migrate(&mut db).is_err());
    }
}
