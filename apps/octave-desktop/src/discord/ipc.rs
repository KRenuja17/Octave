//! Bounded native IPC; never authenticates, subscribes to events, or logs peer data.
use serde_json::{Value, json};
use std::{
    io,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
const PACKET_LIMIT: usize = 65_536;
const DEADLINE: Duration = Duration::from_secs(1);
#[derive(Clone, Copy)]
pub(super) struct Timing {
    pub poll: Duration,
    pub publish: Duration,
    pub retry: Duration,
}
impl Default for Timing {
    fn default() -> Self {
        Self {
            poll: Duration::from_millis(100),
            publish: Duration::from_secs(5),
            retry: Duration::from_secs(10),
        }
    }
}
#[derive(Clone, Default, PartialEq)]
pub(super) struct Desired {
    pub id: String,
    pub activity: Option<Value>,
}
pub(super) struct Shared {
    pub desired: Mutex<Desired>,
    pub closed: AtomicBool,
    status: Mutex<&'static str>,
}
impl Shared {
    pub fn new(desired: Desired) -> Self {
        Self {
            desired: Mutex::new(desired),
            closed: AtomicBool::new(false),
            status: Mutex::new("Connecting…"),
        }
    }
    fn status(&self, status: &'static str) {
        *self.status.lock().unwrap() = status;
    }
    fn current(&self) -> Desired {
        self.desired.lock().unwrap().clone()
    }
    fn active(&self, id: &str) -> bool {
        !self.closed.load(Ordering::Acquire) && self.desired.lock().unwrap().id == id
    }
}
pub(super) struct Worker {
    shared: Arc<Shared>,
    done: mpsc::Receiver<()>,
}
impl Worker {
    pub fn start(desired: Desired) -> Self {
        Self::start_at(desired, endpoints(), Timing::default())
    }
    pub fn start_at(desired: Desired, endpoints: Vec<PathBuf>, timing: Timing) -> Self {
        let shared = Arc::new(Shared::new(desired));
        let (done_tx, done) = mpsc::channel();
        let state = shared.clone();
        if std::thread::Builder::new()
            .name("octave-discord".into())
            .spawn(move || {
                match tokio::runtime::Builder::new_current_thread()
                    .enable_io()
                    .enable_time()
                    .build()
                {
                    Ok(runtime) => runtime.block_on(run(&state, &endpoints, timing)),
                    Err(_) => state.status("Discord worker unavailable"),
                }
                let _ = done_tx.send(());
            })
            .is_err()
        {
            shared.status("Discord worker unavailable");
        }
        Self { shared, done }
    }
    pub fn update(&self, desired: Desired) {
        *self.shared.desired.lock().unwrap() = desired;
    }
    pub fn status(&self) -> String {
        self.shared.status.lock().unwrap().to_string()
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.shared.closed.store(true, Ordering::Release);
        // Exit only: let the worker clear presence before process termination.
        // One in-flight request + one clear, both bounded. Never waits in ui().
        let _ = self
            .done
            .recv_timeout(2 * DEADLINE + Duration::from_millis(500));
    }
}
fn endpoints() -> Vec<PathBuf> {
    #[cfg(target_os = "windows")]
    let prefix = PathBuf::from(r"\\?\pipe");
    #[cfg(not(target_os = "windows"))]
    let prefix = ["XDG_RUNTIME_DIR", "TMPDIR", "TMP", "TEMP"]
        .into_iter()
        .filter_map(std::env::var_os)
        .find(|p| !p.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    (0..10)
        .map(|n| prefix.join(format!("discord-ipc-{n}")))
        .collect()
}
#[cfg(target_os = "windows")]
async fn connect(
    path: &std::path::Path,
) -> io::Result<tokio::net::windows::named_pipe::NamedPipeClient> {
    tokio::net::windows::named_pipe::ClientOptions::new().open(path)
}
#[cfg(not(target_os = "windows"))]
async fn connect(path: &std::path::Path) -> io::Result<tokio::net::UnixStream> {
    tokio::net::UnixStream::connect(path).await
}
async fn run(shared: &Shared, endpoints: &[PathBuf], timing: Timing) {
    let mut retry = Instant::now();
    let mut previous_id = String::new();
    while !shared.closed.load(Ordering::Acquire) {
        let id = shared.current().id;
        if id != previous_id {
            retry = Instant::now();
            previous_id.clone_from(&id);
        }
        if id.is_empty() {
            shared.status("Disabled");
        } else if Instant::now() >= retry {
            let mut result = Err(io::Error::from(io::ErrorKind::NotConnected));
            shared.status("Connecting…");
            for endpoint in endpoints {
                if !shared.active(&id) {
                    break;
                }
                let Ok(Ok(stream)) = tokio::time::timeout(DEADLINE, connect(endpoint)).await else {
                    continue;
                };
                let mut ipc = Ipc::new(stream);
                result = bounded(ipc.handshake(&id)).await;
                if result.is_ok() && shared.active(&id) {
                    shared.status("Connected · no song playing");
                    result = serve(&mut ipc, shared, &id, timing).await;
                    // Clear even when an activity acknowledgement was lost.
                    let _ = bounded(ipc.publish(None)).await;
                    break;
                }
            }
            if shared.active(&id) {
                shared.status(
                    if result
                        .as_ref()
                        .is_err_and(|e| e.kind() == io::ErrorKind::PermissionDenied)
                    {
                        "Discord rejected presence · check the application ID"
                    } else {
                        "Discord unavailable · retrying automatically"
                    },
                );
            }
            retry = Instant::now() + timing.retry;
        }
        tokio::time::sleep(timing.poll).await;
    }
}
pub(super) async fn bounded<T>(
    future: impl std::future::Future<Output = io::Result<T>>,
) -> io::Result<T> {
    tokio::time::timeout(DEADLINE, future)
        .await
        .map_err(|_| io::Error::from(io::ErrorKind::TimedOut))?
}
pub(super) async fn serve<S: AsyncRead + AsyncWrite + Unpin>(
    ipc: &mut Ipc<S>,
    shared: &Shared,
    id: &str,
    timing: Timing,
) -> io::Result<()> {
    let mut sent: Option<Option<Value>> = None;
    let mut last = None::<Instant>;
    while shared.active(id) {
        let desired = shared.current();
        if sent.as_ref() != Some(&desired.activity)
            && (desired.activity.is_none()
                || last.is_none_or(|last| last.elapsed() >= timing.publish))
            && shared.active(id)
        {
            bounded(ipc.publish(desired.activity.as_ref())).await?;
            sent = Some(desired.activity);
            last = Some(Instant::now());
            shared.status(if sent.as_ref().is_some_and(Option::is_some) {
                "Sharing current song"
            } else {
                "Connected · presence cleared"
            });
        }
        // Partial input survives cancelled idle polls. Pings/CLOSE/EOF are
        // processed even when the song metadata is unchanged.
        match tokio::time::timeout(timing.poll, ipc.read_packet()).await {
            Ok(packet) => bounded(ipc.unsolicited(packet?)).await?,
            Err(_)
                if ipc
                    .partial_since
                    .is_some_and(|start| start.elapsed() >= DEADLINE) =>
            {
                return Err(io::ErrorKind::TimedOut.into());
            }
            Err(_) => (),
        }
    }
    Ok(())
}
pub(super) struct Ipc<S> {
    stream: S,
    incoming: Vec<u8>,
    partial_since: Option<Instant>,
    nonce: u64,
}
impl<S: AsyncRead + AsyncWrite + Unpin> Ipc<S> {
    pub fn new(stream: S) -> Self {
        Self {
            stream,
            incoming: Vec::new(),
            partial_since: None,
            nonce: 0,
        }
    }
    pub async fn write_packet(&mut self, opcode: u32, body: &[u8]) -> io::Result<()> {
        if body.len() > PACKET_LIMIT {
            return Err(io::ErrorKind::InvalidData.into());
        }
        // Send the complete header + payload as one frame.
        let mut frame = Vec::with_capacity(8 + body.len());
        frame.extend_from_slice(&opcode.to_le_bytes());
        frame.extend_from_slice(&(body.len() as u32).to_le_bytes());
        frame.extend_from_slice(body);
        self.stream.write_all(&frame).await?;
        self.stream.flush().await
    }
    pub async fn read_packet(&mut self) -> io::Result<(u32, Vec<u8>)> {
        loop {
            if self.incoming.len() >= 8 {
                let opcode = u32::from_le_bytes(self.incoming[..4].try_into().unwrap());
                let size = u32::from_le_bytes(self.incoming[4..8].try_into().unwrap()) as usize;
                if size > PACKET_LIMIT {
                    return Err(io::ErrorKind::InvalidData.into());
                }
                if self.incoming.len() >= size + 8 {
                    let body = self.incoming[8..size + 8].to_vec();
                    self.incoming.drain(..size + 8);
                    self.partial_since = (!self.incoming.is_empty()).then(Instant::now);
                    return Ok((opcode, body));
                }
            }
            let mut bytes = [0; 4096];
            let n = self.stream.read(&mut bytes).await?;
            if n == 0 {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
            self.partial_since.get_or_insert_with(Instant::now);
            self.incoming.extend_from_slice(&bytes[..n]);
        }
    }
    pub async fn unsolicited(&mut self, (opcode, body): (u32, Vec<u8>)) -> io::Result<()> {
        match opcode {
            3 => self.write_packet(4, &body).await,
            4 => Ok(()),
            1 => {
                let event: Value =
                    serde_json::from_slice(&body).map_err(|_| io::ErrorKind::InvalidData)?;
                if event["evt"] == "ERROR" {
                    Err(io::ErrorKind::PermissionDenied.into())
                } else {
                    Ok(())
                }
            }
            _ => Err(io::ErrorKind::ConnectionAborted.into()),
        }
    }
    async fn response(&mut self) -> io::Result<Value> {
        for _ in 0..8 {
            let packet = self.read_packet().await?;
            if packet.0 == 1 {
                return serde_json::from_slice(&packet.1)
                    .map_err(|_| io::ErrorKind::InvalidData.into());
            }
            self.unsolicited(packet).await?;
        }
        Err(io::ErrorKind::InvalidData.into())
    }
    pub async fn handshake(&mut self, id: &str) -> io::Result<()> {
        self.write_packet(0, &serde_json::to_vec(&json!({"v":1,"client_id":id}))?)
            .await?;
        let reply = self.response().await?;
        if reply["cmd"] != "DISPATCH" || reply["evt"] != "READY" || reply["data"]["v"] != 1 {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        Ok(())
    }
    pub async fn publish(&mut self, activity: Option<&Value>) -> io::Result<()> {
        self.nonce += 1;
        let nonce = self.nonce.to_string();
        self.write_packet(1, &serde_json::to_vec(&json!({
            "cmd":"SET_ACTIVITY", "args":{"pid":std::process::id(), "activity":activity}, "nonce":nonce
        }))?).await?;
        for _ in 0..8 {
            let reply = self.response().await?;
            if reply["nonce"] == nonce {
                if reply["evt"] == "ERROR" {
                    return Err(io::ErrorKind::PermissionDenied.into());
                }
                if reply["cmd"] != "SET_ACTIVITY" || !reply["evt"].is_null() {
                    return Err(io::ErrorKind::InvalidData.into());
                }
                #[cfg(test)]
                if std::env::var("OCTAVE_DISCORD_LIVE").as_deref() == Ok("1")
                    && let Some(activity) = activity
                {
                    // Only comparisons with our own outbound metadata. Never
                    // print the peer's user/profile data or free-form values.
                    let data = &reply["data"];
                    eprintln!(
                        "Discord ack: object={}, listening={}, title_matches={}, artist_matches={}, application_matches={}, artwork_returned={}, timing_returned={}",
                        data.is_object(),
                        data["type"] == 2,
                        data["details"] == activity["details"],
                        data["state"] == activity["state"],
                        data["application_id"] == super::APPLICATION_ID,
                        data["assets"]["large_image"].is_string(),
                        data["timestamps"].is_object()
                    );
                }
                return Ok(());
            }
        }
        Err(io::ErrorKind::InvalidData.into())
    }
}
