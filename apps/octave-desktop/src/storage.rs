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
    if version > 1 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    if version == 0 {
        let tx = db.transaction()?;
        tx.execute_batch("CREATE TABLE queue (slot INTEGER PRIMARY KEY, id TEXT NOT NULL UNIQUE, title TEXT NOT NULL, artists TEXT NOT NULL);
            CREATE TABLE session (singleton INTEGER PRIMARY KEY CHECK(singleton=1), cursor INTEGER NOT NULL, position REAL NOT NULL, volume REAL NOT NULL, repeat INTEGER NOT NULL);
            PRAGMA user_version=1;")?;
        tx.commit()?;
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
        save(&mut db, &s).unwrap();
        assert_eq!(load(&db).unwrap(), s);
        assert_eq!(
            db.pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))
                .unwrap(),
            1
        );
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
