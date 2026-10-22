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
    if version > 4 {
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
    Ok(())
}
fn save(db: &mut Connection, s: &Session) -> rusqlite::Result<()> {
    let tx = db.transaction()?;
    tx.execute("DELETE FROM queue", [])?;
    for (i, track) in s.queue.iter().take(MAX_QUEUE).enumerate() {
        tx.execute(
            "INSERT INTO queue VALUES (?1,?2,?3,?4)",
            params![
                i as i64,
                track.id.0,
                track.title,
                serde_json::to_string(&track.artists).unwrap()
            ],
        )?;
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
        "INSERT OR REPLACE INTO view_preferences VALUES (1,?1,?2,?3,?4,?5)",
        params![
            s.view.panel_open,
            s.view.panel_width.clamp(300.0, 440.0),
            s.view.panel_tab.min(2),
            s.view.lyrics_only,
            s.view.reduced_motion
        ],
    )?;
    tx.execute(
        "INSERT OR REPLACE INTO preferences VALUES (1,?1,?2,?3,?4,?5)",
        params![
            gains[0],
            gains[1],
            gains[2],
            s.appearance.min(2),
            s.crossfade.min(5)
        ],
    )?;
    tx.commit()
}
fn load(db: &Connection) -> rusqlite::Result<Session> {
    let mut s = Session::default();
    let mut stmt = db.prepare("SELECT id,title,artists FROM queue ORDER BY slot LIMIT 500")?;
    s.queue = stmt
        .query_map([], |row| {
            let artists: String = row.get(2)?;
            Ok(Track {
                id: TrackId(row.get(0)?),
                title: row.get(1)?,
                artists: serde_json::from_str(&artists)
                    .map_err(|_| rusqlite::Error::InvalidQuery)?,
            })
        })?
        .collect::<Result<_, _>>()?;
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
        "SELECT bass,mid,treble,appearance,crossfade FROM preferences WHERE singleton=1",
    )?;
    let mut rows = stmt.query([])?;
    if let Some(row) = rows.next()? {
        s.equalizer = octave_player::Equalizer {
            gains_db: [row.get(0)?, row.get(1)?, row.get(2)?],
        }
        .sanitized();
        s.appearance = row.get::<_, i64>(3)?.clamp(0, 2) as u8;
        s.crossfade = row.get::<_, i64>(4)?.clamp(0, 5) as u8;
    }
    let mut stmt = db.prepare("SELECT panel_open,panel_width,panel_tab,lyrics_only,reduced_motion FROM view_preferences WHERE singleton=1")?;
    let mut rows = stmt.query([])?;
    if let Some(row) = rows.next()? {
        s.view.panel_open = row.get(0)?;
        s.view.panel_width = row.get::<_, f32>(1)?.clamp(300.0, 440.0);
        s.view.panel_tab = row.get::<_, i64>(2)?.clamp(0, 2) as u8;
        s.view.lyrics_only = row.get(3)?;
        s.view.reduced_motion = row.get(4)?;
    }
    Ok(s)
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::listening::track;
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
        s.crossfade = 4;
        s.view.panel_open = true;
        s.view.panel_width = 410.0;
        s.view.reduced_motion = true;
        save(&mut db, &s).unwrap();
        assert_eq!(load(&db).unwrap(), s);
        assert_eq!(
            db.pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))
                .unwrap(),
            4
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
        db.execute_batch("ALTER TABLE preferences DROP COLUMN crossfade; PRAGMA user_version=2;")
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
