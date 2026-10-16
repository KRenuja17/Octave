//! Local-only playlist snapshots, persisted on a dedicated SQLite worker.
use octave_core::{Track, TrackId};
use rusqlite::{Connection, params};
use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver, SyncSender},
    thread::JoinHandle,
};

pub const MAX_PLAYLISTS: usize = 20;
#[derive(Clone, Debug, PartialEq)]
pub struct Playlist {
    pub name: String,
    pub tracks: Vec<Track>,
}
pub struct Store {
    tx: Option<SyncSender<Vec<Playlist>>>,
    pub events: Receiver<Result<Vec<Playlist>, ()>>,
    worker: Option<JoinHandle<()>>,
}
impl Store {
    pub fn open(path: PathBuf) -> Self {
        let (tx, rx) = mpsc::sync_channel::<Vec<Playlist>>(1);
        let (reply, events) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let opened = (|| {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent).map_err(|_| ())?;
                }
                let mut db = Connection::open(path).map_err(|_| ())?;
                db.busy_timeout(std::time::Duration::from_secs(2))
                    .map_err(|_| ())?;
                migrate(&mut db).map_err(|_| ())?;
                let data = load(&db).map_err(|_| ())?;
                reply.send(Ok(data)).map_err(|_| ())?;
                Ok::<_, ()>(db)
            })();
            let Ok(mut db) = opened else {
                let _ = reply.send(Err(()));
                return;
            };
            while let Ok(data) = rx.recv() {
                let result = save(&mut db, &data).map(|_| data).map_err(|_| ());
                if reply.send(result).is_err() {
                    break;
                }
            }
        });
        Self {
            tx: Some(tx),
            events,
            worker: Some(worker),
        }
    }
    pub fn save(&self, data: Vec<Playlist>) -> bool {
        self.tx.as_ref().is_some_and(|tx| tx.try_send(data).is_ok())
    }
}
impl Drop for Store {
    fn drop(&mut self) {
        self.tx.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
fn migrate(db: &mut Connection) -> rusqlite::Result<()> {
    let version: u32 = db.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if version > 1 {
        return Err(rusqlite::Error::InvalidQuery);
    }
    if version == 0 {
        let tx = db.transaction()?;
        tx.execute_batch("CREATE TABLE playlists (slot INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE COLLATE NOCASE);
            CREATE TABLE tracks (playlist INTEGER NOT NULL, slot INTEGER NOT NULL, id TEXT NOT NULL, title TEXT NOT NULL, artists TEXT NOT NULL, PRIMARY KEY(playlist,slot), UNIQUE(playlist,id)); PRAGMA user_version=1;")?;
        tx.commit()?;
    }
    Ok(())
}
fn validate(data: &[Playlist]) -> rusqlite::Result<()> {
    if data.len() > MAX_PLAYLISTS
        || data.iter().any(|p| {
            p.name.trim().is_empty()
                || p.name.chars().count() > 80
                || p.tracks.len() > crate::listening::MAX_QUEUE
        })
    {
        return Err(rusqlite::Error::InvalidQuery);
    }
    Ok(())
}
fn save(db: &mut Connection, data: &[Playlist]) -> rusqlite::Result<()> {
    validate(data)?;
    let tx = db.transaction()?;
    tx.execute_batch("DELETE FROM tracks; DELETE FROM playlists;")?;
    for (index, playlist) in data.iter().enumerate() {
        tx.execute(
            "INSERT INTO playlists VALUES (?1,?2)",
            params![index as i64, playlist.name],
        )?;
        for (slot, track) in playlist.tracks.iter().enumerate() {
            tx.execute(
                "INSERT INTO tracks VALUES (?1,?2,?3,?4,?5)",
                params![
                    index as i64,
                    slot as i64,
                    track.id.0,
                    track.title,
                    serde_json::to_string(&track.artists).unwrap()
                ],
            )?;
        }
    }
    tx.commit()
}
fn load(db: &Connection) -> rusqlite::Result<Vec<Playlist>> {
    let mut statement = db.prepare("SELECT slot,name FROM playlists ORDER BY slot LIMIT 21")?;
    let rows = statement.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
    let mut data = Vec::new();
    for row in rows {
        let (id, name) = row?;
        let mut tracks = db.prepare(
            "SELECT id,title,artists FROM tracks WHERE playlist=?1 ORDER BY slot LIMIT 501",
        )?;
        let tracks = tracks
            .query_map([id], |r| {
                let artists: String = r.get(2)?;
                Ok(Track {
                    id: TrackId(r.get(0)?),
                    title: r.get(1)?,
                    artists: serde_json::from_str(&artists)
                        .map_err(|_| rusqlite::Error::InvalidQuery)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        data.push(Playlist { name, tracks });
    }
    validate(&data)?;
    Ok(data)
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::listening::track;
    #[test]
    fn ordered_edits_roundtrip_and_failed_overwrites_roll_back() {
        let mut db = Connection::open_in_memory().unwrap();
        migrate(&mut db).unwrap();
        let mut data = vec![Playlist {
            name: "Evening 日本語".into(),
            tracks: vec![track(2), track(1)],
        }];
        save(&mut db, &data).unwrap();
        assert_eq!(load(&db).unwrap(), data);
        data[0].tracks.swap(0, 1);
        data[0].name = "Renamed".into();
        save(&mut db, &data).unwrap();
        let before = data.clone();
        data[0].tracks.push(track(1));
        assert!(save(&mut db, &data).is_err());
        assert_eq!(load(&db).unwrap(), before);
        data = before.clone();
        data.push(Playlist {
            name: "renamed".into(),
            tracks: vec![],
        });
        assert!(save(&mut db, &data).is_err());
        assert_eq!(load(&db).unwrap(), before);
        save(&mut db, &[]).unwrap();
        assert!(load(&db).unwrap().is_empty());
    }
    #[test]
    fn future_versions_and_oversized_lists_are_rejected() {
        let mut db = Connection::open_in_memory().unwrap();
        db.pragma_update(None, "user_version", 99).unwrap();
        assert!(migrate(&mut db).is_err());
        assert!(
            validate(&vec![
                Playlist {
                    name: "x".into(),
                    tracks: vec![]
                };
                MAX_PLAYLISTS + 1
            ])
            .is_err()
        );
        assert!(
            validate(&[Playlist {
                name: " ".into(),
                tracks: vec![]
            }])
            .is_err()
        );
    }
}
