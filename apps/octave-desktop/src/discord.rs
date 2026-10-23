//! Optional, bounded local Discord IPC. No bot token and no default publishing.
use octave_core::Track;
use octave_player::{PlaybackState, Snapshot};
use serde_json::{Value, json};
#[cfg(target_os = "windows")]
use std::time::Instant;
#[cfg(any(target_os = "windows", test))]
use std::{io, time::Duration};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};
#[cfg(any(target_os = "windows", test))]
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
#[cfg(any(target_os = "windows", test))]
const PACKET_LIMIT: usize = 65536;
pub fn valid_id(id: &str) -> bool {
    (17..=20).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_digit())
}
fn activity(track: Option<&Track>, snapshot: &Snapshot) -> Option<Value> {
    let track = track?;
    if snapshot.buffering || !matches!(&snapshot.state,PlaybackState::Playing(id) if id==&track.id)
    {
        return None;
    }
    let clip = |s: &str| {
        let mut text = String::new();
        for c in s.chars().filter(|c| !c.is_control()) {
            if text.len() + c.len_utf8() > 128 {
                break;
            }
            text.push(c);
        }
        text
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let mut value = json!({"details":clip(&track.title),"state":clip(&track.artists.join(" · ")),"timestamps":{"start":now.saturating_sub(snapshot.position.as_secs())},"instance":false});
    if track.id.0.len() == 11
        && track
            .id
            .0
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
    {
        value["assets"] = json!({"large_image":format!("https://i.ytimg.com/vi/{}/hqdefault.jpg",track.id.0),"large_text":"Octave"});
    }
    Some(value)
}
#[cfg(any(target_os = "windows", test))]
async fn write_packet<T: AsyncWrite + Unpin>(
    stream: &mut T,
    opcode: u32,
    body: &[u8],
) -> io::Result<()> {
    if body.len() > PACKET_LIMIT {
        return Err(io::ErrorKind::InvalidData.into());
    }
    stream.write_all(&opcode.to_le_bytes()).await?;
    stream.write_all(&(body.len() as u32).to_le_bytes()).await?;
    stream.write_all(body).await?;
    stream.flush().await
}
#[cfg(any(target_os = "windows", test))]
async fn read_packet<T: AsyncRead + Unpin>(stream: &mut T) -> io::Result<(u32, Vec<u8>)> {
    let opcode = stream.read_u32_le().await?;
    let len = stream.read_u32_le().await? as usize;
    if len > PACKET_LIMIT {
        return Err(io::ErrorKind::InvalidData.into());
    }
    let mut body = vec![0; len];
    stream.read_exact(&mut body).await?;
    Ok((opcode, body))
}
#[cfg(any(target_os = "windows", test))]
async fn response<T: AsyncRead + AsyncWrite + Unpin>(stream: &mut T) -> io::Result<Value> {
    for _ in 0..8 {
        let (op, body) = read_packet(stream).await?;
        match op {
            1 => {
                return serde_json::from_slice(&body)
                    .map_err(|_| io::ErrorKind::InvalidData.into());
            }
            3 => write_packet(stream, 4, &body).await?,
            _ => return Err(io::ErrorKind::ConnectionAborted.into()),
        }
    }
    Err(io::ErrorKind::InvalidData.into())
}
#[cfg(any(target_os = "windows", test))]
async fn handshake<T: AsyncRead + AsyncWrite + Unpin>(stream: &mut T, id: &str) -> io::Result<()> {
    write_packet(
        stream,
        0,
        &serde_json::to_vec(&json!({"v":1,"client_id":id}))?,
    )
    .await?;
    let value = response(stream).await?;
    if value["evt"] != "READY" {
        return Err(io::ErrorKind::PermissionDenied.into());
    }
    Ok(())
}
#[cfg(any(target_os = "windows", test))]
async fn publish<T: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut T,
    activity: Option<&Value>,
    nonce: u64,
) -> io::Result<()> {
    let nonce = nonce.to_string();
    write_packet(stream,1,&serde_json::to_vec(&json!({"cmd":"SET_ACTIVITY","args":{"pid":std::process::id(),"activity":activity},"nonce":nonce}))?).await?;
    for _ in 0..8 {
        let reply = response(stream).await?;
        if reply["nonce"] == nonce {
            if reply["evt"] == "ERROR" {
                return Err(io::ErrorKind::PermissionDenied.into());
            }
            return Ok(());
        }
    }
    Err(io::ErrorKind::InvalidData.into())
}
#[derive(Default)]
pub struct Presence {
    worker: Option<Worker>,
}
struct Worker {
    latest: Arc<Mutex<(String, Option<Value>)>>,
    closed: Arc<AtomicBool>,
    status: Arc<Mutex<String>>,
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.closed.store(true, Ordering::Release);
    }
}
impl Presence {
    pub fn settings(&self, ui: &mut eframe::egui::Ui, enabled: &mut bool, id: &mut String) {
        use eframe::egui;
        ui.separator();
        ui.strong("Discord Rich Presence");
        ui.add(
            egui::TextEdit::singleline(id)
                .hint_text("Octave application ID · no bot token")
                .char_limit(20),
        );
        ui.add_enabled(
            (cfg!(target_os = "windows") && valid_id(id)) || *enabled,
            egui::Checkbox::new(enabled, "Share current song on Discord"),
        );
        ui.weak("Off by default. Pause/stop clears the song; audio never depends on Discord.");
        if !cfg!(target_os = "windows") {
            ui.weak("This Discord adapter currently supports Windows.");
        }
        ui.weak(self.status());
    }
    pub fn update(&mut self, enabled: bool, id: &str, track: Option<&Track>, snapshot: &Snapshot) {
        let id = if enabled && valid_id(id) { id } else { "" };
        if self.worker.is_none() && !id.is_empty() {
            self.worker = Some(start_worker());
        }
        if let Some(worker) = &self.worker {
            *worker.latest.lock().unwrap() = (
                id.into(),
                if id.is_empty() {
                    None
                } else {
                    activity(track, snapshot)
                },
            );
        }
    }
    pub fn status(&self) -> String {
        self.worker
            .as_ref()
            .map(|w| w.status.lock().unwrap().clone())
            .unwrap_or_else(|| "Disabled".into())
    }
}
fn start_worker() -> Worker {
    let worker = Worker {
        latest: Default::default(),
        closed: Default::default(),
        status: Arc::new(Mutex::new("Connecting…".into())),
    };
    #[cfg(target_os = "windows")]
    {
        let latest = worker.latest.clone();
        let closed = worker.closed.clone();
        let status = worker.status.clone();
        std::thread::spawn(move || {
            let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                .enable_io()
                .enable_time()
                .build()
            else {
                return;
            };
            runtime.block_on(async move {
                let mut pipe: Option<tokio::net::windows::named_pipe::NamedPipeClient> = None;
                let mut connected = String::new();
                let mut sent = None;
                let mut nonce = 0;
                let mut retry = Instant::now();
                let mut last = Instant::now() - Duration::from_secs(10);
                while !closed.load(Ordering::Acquire) {
                    let (id, desired) = latest.lock().unwrap().clone();
                    if id != connected && pipe.is_some() {
                        if let Some(stream) = pipe.as_mut() {
                            nonce += 1;
                            let _ = tokio::time::timeout(
                                Duration::from_secs(1),
                                publish(stream, None, nonce),
                            )
                            .await;
                        }
                        pipe = None;
                        sent = None;
                        connected.clear();
                    }
                    if id.is_empty() {
                        *status.lock().unwrap() = "Disabled".into();
                        tokio::time::sleep(Duration::from_millis(100)).await;
                        continue;
                    }
                    if pipe.is_none() && Instant::now() >= retry {
                        for index in 0..10 {
                            if let Ok(mut stream) =
                                tokio::net::windows::named_pipe::ClientOptions::new()
                                    .open(format!(r"\\.\pipe\discord-ipc-{index}"))
                                && matches!(
                                    tokio::time::timeout(
                                        Duration::from_secs(1),
                                        handshake(&mut stream, &id)
                                    )
                                    .await,
                                    Ok(Ok(()))
                                )
                            {
                                pipe = Some(stream);
                                connected = id.clone();
                                *status.lock().unwrap() = "Connected · no song playing".into();
                                break;
                            }
                        }
                        if pipe.is_none() {
                            retry = Instant::now() + Duration::from_secs(10);
                            *status.lock().unwrap() = "Discord unavailable · will retry".into();
                        }
                    }
                    if *latest.lock().unwrap() != (id.clone(), desired.clone()) {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                        continue;
                    }
                    if let Some(stream) = pipe.as_mut()
                        && desired != sent
                        && (desired.is_none() || last.elapsed() >= Duration::from_secs(5))
                    {
                        nonce += 1;
                        let result = tokio::time::timeout(
                            Duration::from_secs(1),
                            publish(stream, desired.as_ref(), nonce),
                        )
                        .await;
                        if matches!(result, Ok(Ok(()))) {
                            sent = desired;
                            last = Instant::now();
                            *status.lock().unwrap() = if sent.is_some() {
                                "Sharing current song"
                            } else {
                                "Connected · presence cleared"
                            }
                            .into();
                        } else {
                            pipe = None;
                            sent = None;
                            connected.clear();
                            retry = Instant::now() + Duration::from_secs(10);
                            *status.lock().unwrap() = "Discord unavailable · will retry".into();
                        }
                    }

                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
                if let Some(stream) = pipe.as_mut() {
                    let _ = tokio::time::timeout(
                        Duration::from_secs(1),
                        publish(stream, None, nonce + 1),
                    )
                    .await;
                }
            });
        });
    }
    #[cfg(not(target_os = "windows"))]
    {
        *worker.status.lock().unwrap() = "Discord integration currently supports Windows".into();
    }
    worker
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn framed_ipc_handles_ping_ready_activity_and_clear() {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let (mut client, mut server) = tokio::io::duplex(4096);
                let server = tokio::spawn(async move {
                    assert_eq!(read_packet(&mut server).await.unwrap().0, 0);
                    write_packet(&mut server, 3, b"ping").await.unwrap();
                    assert_eq!(
                        read_packet(&mut server).await.unwrap(),
                        (4, b"ping".to_vec())
                    );
                    write_packet(&mut server, 1, br#"{"evt":"READY"}"#)
                        .await
                        .unwrap();
                    for empty in [false, true] {
                        let (_, body) = read_packet(&mut server).await.unwrap();
                        let request: Value = serde_json::from_slice(&body).unwrap();
                        assert_eq!(request["cmd"], "SET_ACTIVITY");
                        assert_eq!(request["args"]["activity"].is_null(), empty);
                        write_packet(
                            &mut server,
                            1,
                            &serde_json::to_vec(&json!({"nonce":request["nonce"]})).unwrap(),
                        )
                        .await
                        .unwrap();
                    }
                });
                handshake(&mut client, "123456789012345678").await.unwrap();
                publish(&mut client, Some(&json!({"details":"Original fixture"})), 1)
                    .await
                    .unwrap();
                publish(&mut client, None, 2).await.unwrap();
                server.await.unwrap();
            });
    }
    #[test]
    fn ipc_rejects_oversized_frames_and_missing_responses_time_out() {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let (mut client, mut server) = tokio::io::duplex(64);
                server.write_all(&1_u32.to_le_bytes()).await.unwrap();
                server.write_all(&65537_u32.to_le_bytes()).await.unwrap();
                assert_eq!(
                    read_packet(&mut client).await.unwrap_err().kind(),
                    io::ErrorKind::InvalidData
                );
                assert!(
                    tokio::time::timeout(Duration::from_millis(10), read_packet(&mut client))
                        .await
                        .is_err()
                );
            });
    }
    #[test]
    fn idle_pause_and_wrong_identity_never_publish_metadata() {
        let mut presence = Presence::default();
        presence.update(false, "", None, &Snapshot::default());
        presence.update(true, "", None, &Snapshot::default());
        assert!(presence.worker.is_none());
        let track = Track {
            id: octave_core::TrackId("abcdefghijk".into()),
            title: "Original fixture".into(),
            artists: vec!["Artist".into()],
        };
        let mut snapshot = Snapshot::default();
        assert_eq!(activity(Some(&track), &snapshot), None);
        snapshot.state = PlaybackState::Paused(track.id.clone());
        assert_eq!(activity(Some(&track), &snapshot), None);
        snapshot.state = PlaybackState::Playing(octave_core::TrackId("different".into()));
        assert_eq!(activity(Some(&track), &snapshot), None);
        snapshot.state = PlaybackState::Playing(track.id.clone());
        assert!(activity(Some(&track), &snapshot).is_some());
        snapshot.buffering = true;
        assert!(activity(Some(&track), &snapshot).is_none());
        assert!(!valid_id(""));
    }
}
