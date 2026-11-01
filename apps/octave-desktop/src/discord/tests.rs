use super::{ipc::*, *};
use std::{
    io,
    sync::{Arc, atomic::Ordering},
};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};

const ID: &str = "123456789012345678";
fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}
fn fixture() -> (Track, Snapshot) {
    let track = Track {
        id: TrackId("abcdefghijk".into()),
        title: "Original fixture".into(),
        artists: vec!["Artist".into()],
    };
    let snapshot = Snapshot {
        state: PlaybackState::Playing(track.id.clone()),
        duration: Duration::from_secs(240),
        ..Default::default()
    };
    (track, snapshot)
}
async fn ready<S: AsyncRead + AsyncWrite + Unpin>(server: &mut Ipc<S>, id: &str) {
    let (op, body) = bounded(server.read_packet()).await.unwrap();
    assert_eq!(op, 0);
    assert_eq!(
        serde_json::from_slice::<Value>(&body).unwrap(),
        json!({"v":1,"client_id":id})
    );
    server.write_packet(1, br#"{"cmd":"DISPATCH","evt":"READY","data":{"v":1,"user":{"username":"must-not-be-logged"}}}"#).await.unwrap();
}
async fn request<S: AsyncRead + AsyncWrite + Unpin>(server: &mut Ipc<S>) -> Value {
    let (op, body) = bounded(server.read_packet()).await.unwrap();
    assert_eq!(op, 1);
    let request: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(request["cmd"], "SET_ACTIVITY");
    assert_eq!(request["args"]["pid"], std::process::id());
    server
        .write_packet(
            1,
            &serde_json::to_vec(
                &json!({"cmd":"SET_ACTIVITY","evt":null,"nonce":request["nonce"],"data":null}),
            )
            .unwrap(),
        )
        .await
        .unwrap();
    request["args"]["activity"].clone()
}

#[test]
fn clock_stays_stable_and_reanchors_for_seek_repeat_pause_and_buffering() {
    let (track, mut snapshot) = fixture();
    let mut clock = Clock::default();
    let now = Instant::now();
    let wall = Duration::from_millis(1_700_000_000_990);
    let first = clock.activity(Some(&track), &snapshot, wall, now).unwrap();
    for millis in (20..2000).step_by(20) {
        let delta = Duration::from_millis(millis);
        snapshot.position = delta;
        assert_eq!(
            clock.activity(Some(&track), &snapshot, wall + delta, now + delta),
            Some(first.clone())
        );
    }
    snapshot.position = Duration::from_secs(180);
    let sought = clock
        .activity(
            Some(&track),
            &snapshot,
            wall + Duration::from_secs(2),
            now + Duration::from_secs(2),
        )
        .unwrap();
    assert_eq!(sought["timestamps"]["start"], 1_699_999_822_u64);
    assert_eq!(sought["timestamps"]["end"], 1_700_000_062_u64);
    snapshot.sequence += 1;
    snapshot.position = Duration::ZERO;
    assert_ne!(
        clock
            .activity(
                Some(&track),
                &snapshot,
                wall + Duration::from_secs(3),
                now + Duration::from_secs(3)
            )
            .unwrap()["timestamps"],
        sought["timestamps"]
    );
    snapshot.state = PlaybackState::Paused(track.id.clone());
    assert!(clock.activity(Some(&track), &snapshot, wall, now).is_none());
    assert!(clock.0.is_none());
    snapshot.state = PlaybackState::Playing(track.id.clone());
    let resumed = clock
        .activity(
            Some(&track),
            &snapshot,
            wall + Duration::from_secs(10),
            now + Duration::from_secs(10),
        )
        .unwrap();
    assert_eq!(resumed["timestamps"]["start"], 1_700_000_010_u64);
    snapshot.buffering = true;
    assert!(clock.activity(Some(&track), &snapshot, wall, now).is_none());
    snapshot.buffering = false;
    snapshot.state = PlaybackState::Playing(TrackId("different".into()));
    assert!(clock.activity(Some(&track), &snapshot, wall, now).is_none());
    assert!(clock.activity(None, &snapshot, wall, now).is_none());
}

#[test]
fn metadata_uses_listening_and_public_urls_and_preserves_graphemes() {
    let (mut track, snapshot) = fixture();
    track.title = format!("{}👩🏽‍💻", "a".repeat(125));
    track.artists = vec!["\nArtist\t".into()];
    let mut clock = Clock::default();
    let value = clock
        .activity(
            Some(&track),
            &snapshot,
            Duration::from_secs(1000),
            Instant::now(),
        )
        .unwrap();
    assert_eq!(value["type"], 2);
    assert_eq!(value["details"], "a".repeat(125));
    assert_eq!(value["state"], "Artist");
    assert_eq!(
        value["buttons"][0]["url"],
        "https://music.youtube.com/watch?v=abcdefghijk"
    );
    assert_eq!(
        value["assets"]["large_image"],
        "https://i.ytimg.com/vi/abcdefghijk/hqdefault.jpg"
    );
    assert_eq!(
        clip(&format!("{}e\u{301}", "a".repeat(127)), "fallback"),
        "a".repeat(127)
    );
    track.id = TrackId("https://private.example/secret".into());
    track.title.clear();
    let mut snapshot = snapshot;
    snapshot.duration = Duration::ZERO;
    snapshot.state = PlaybackState::Playing(track.id.clone());
    let value = clock
        .activity(
            Some(&track),
            &snapshot,
            Duration::from_secs(1000),
            Instant::now(),
        )
        .unwrap();
    assert_eq!(value["details"], "Unknown title");
    assert!(value.get("assets").is_none());
    assert!(value.get("buttons").is_none());
    assert!(value["timestamps"].get("end").is_none());
    assert!(!value.to_string().contains("private.example"));
}

#[test]
fn disabled_and_invalid_configuration_never_starts_ipc() {
    let mut presence = Presence::default();
    for id in [
        "00000000000000000",
        "01234567890123456",
        "18446744073709551616",
        "12345678901234567x",
        " 123456789012345678",
        "１２３４５６７８９０１２３４５６７８",
    ] {
        assert!(!valid_id(id));
        presence.update(true, id, None, &Snapshot::default());
    }
    assert!(valid_id(ID));
    assert!(valid_id(APPLICATION_ID));
    assert!(valid_id("18446744073709551615"));
    assert!(!crate::listening::ViewPreferences::default().discord_enabled);
    presence.update(false, "", None, &Snapshot::default());
    presence.update(false, ID, None, &Snapshot::default());
    assert!(presence.worker.is_none());
    assert_eq!(presence.status(), "Disabled");
}

#[test]
fn settings_expose_a_named_edit_and_opt_in_checkbox() {
    let ctx = eframe::egui::Context::default();
    ctx.enable_accesskit();
    let presence = Presence::default();
    let mut enabled = false;
    let mut id = ID.to_owned();
    let mut output = ctx.run_ui(eframe::egui::RawInput::default(), |ui| {
        presence.settings(ui, &mut enabled, &mut id)
    });
    output.textures_delta.clear();
    let tree = output.platform_output.accesskit_update.unwrap();
    assert!(tree.nodes.iter().any(|(_, node)| node.role()
        == eframe::egui::accesskit::Role::TextInput
        && !node.labelled_by().is_empty()));
    assert!(tree.nodes.iter().any(|(_, node)| node.role()
        == eframe::egui::accesskit::Role::CheckBox
        && node.label() == Some("Share current song on Discord")));
    assert!(!enabled);
}

#[test]
fn framed_ipc_handles_ping_unrelated_nonce_activity_and_clear() {
    runtime().block_on(async {
        let (client, server) = tokio::io::duplex(4096);
        let mut client = Ipc::new(client);
        let peer = tokio::spawn(async move {
            let mut server = Ipc::new(server);
            let (op, _) = server.read_packet().await.unwrap();
            assert_eq!(op, 0);
            server.write_packet(3, b"ping").await.unwrap();
            assert_eq!(server.read_packet().await.unwrap(), (4, b"ping".to_vec()));
            server
                .write_packet(1, br#"{"cmd":"DISPATCH","evt":"READY","data":{"v":1}}"#)
                .await
                .unwrap();
            // An unrelated event must never be mistaken for the command ack.
            server
                .write_packet(1, br#"{"cmd":"DISPATCH","evt":"CURRENT_USER_UPDATE"}"#)
                .await
                .unwrap();
            assert_eq!(request(&mut server).await["details"], "Fixture");
            assert!(request(&mut server).await.is_null());
        });
        bounded(client.handshake(ID)).await.unwrap();
        bounded(client.publish(Some(&json!({"details":"Fixture"}))))
            .await
            .unwrap();
        bounded(client.publish(None)).await.unwrap();
        peer.await.unwrap();
    });
}

#[test]
fn ipc_rejects_malformed_ready_errors_wrong_commands_and_oversized_frames() {
    runtime().block_on(async {
        for reply in [
            json!({"evt":"READY"}),
            json!({"cmd":"DISPATCH","evt":"READY","data":{"v":2}}),
            json!({"cmd":"DISPATCH","evt":"ERROR","data":{"message":"must-not-leak"}}),
        ] {
            let (client, server) = tokio::io::duplex(4096);
            let mut client = Ipc::new(client);
            let peer = tokio::spawn(async move {
                let mut server = Ipc::new(server);
                server.read_packet().await.unwrap();
                server
                    .write_packet(1, &serde_json::to_vec(&reply).unwrap())
                    .await
                    .unwrap();
            });
            let error = bounded(client.handshake(ID)).await.unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
            assert!(!error.to_string().contains("must-not-leak"));
            peer.await.unwrap();
        }
        for (cmd, event, kind) in [
            (
                "SET_ACTIVITY",
                json!("ERROR"),
                io::ErrorKind::PermissionDenied,
            ),
            ("AUTHORIZE", Value::Null, io::ErrorKind::InvalidData),
        ] {
            let (client, server) = tokio::io::duplex(4096);
            let mut client = Ipc::new(client);
            let peer = tokio::spawn(async move {
                let mut server = Ipc::new(server);
                let (_, body) = server.read_packet().await.unwrap();
                let request: Value = serde_json::from_slice(&body).unwrap();
                server
                    .write_packet(
                        1,
                        &serde_json::to_vec(
                            &json!({"cmd":cmd,"evt":event,"nonce":request["nonce"]}),
                        )
                        .unwrap(),
                    )
                    .await
                    .unwrap();
            });
            assert_eq!(
                bounded(client.publish(None)).await.unwrap_err().kind(),
                kind
            );
            peer.await.unwrap();
        }
        let (client, mut server) = tokio::io::duplex(64);
        let mut client = Ipc::new(client);
        server.write_all(&[1, 0, 0, 0, 1, 0, 1, 0]).await.unwrap();
        assert_eq!(
            client.read_packet().await.unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        assert_eq!(
            client
                .write_packet(1, &vec![0; 65537])
                .await
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
    });
}

#[test]
fn fragmented_frames_survive_cancelled_idle_reads_and_eof_is_detected() {
    runtime().block_on(async {
        let (client, mut server) = tokio::io::duplex(128);
        let mut client = Ipc::new(client);
        server.write_all(&[3, 0, 0]).await.unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(5), client.read_packet())
                .await
                .is_err()
        );
        server
            .write_all(&[0, 4, 0, 0, 0, b'p', b'i'])
            .await
            .unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(5), client.read_packet())
                .await
                .is_err()
        );
        server.write_all(b"ng").await.unwrap();
        assert_eq!(client.read_packet().await.unwrap(), (3, b"ping".to_vec()));
        drop(server);
        assert_eq!(
            client.read_packet().await.unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );
    });
}

#[test]
fn active_session_coalesces_changes_clears_immediately_and_answers_idle_ping() {
    runtime().block_on(async {
        let (client, server) = tokio::io::duplex(4096);
        let shared = Arc::new(Shared::new(Desired {
            id: ID.into(),
            activity: Some(json!({"details":"First"})),
        }));
        let peer_shared = shared.clone();
        let peer = tokio::spawn(async move {
            let mut server = Ipc::new(server);
            assert_eq!(request(&mut server).await["details"], "First");
            server.write_packet(3, b"idle").await.unwrap();
            assert_eq!(
                bounded(server.read_packet()).await.unwrap(),
                (4, b"idle".to_vec())
            );
            for name in ["Obsolete", "Newest"] {
                peer_shared.desired.lock().unwrap().activity = Some(json!({"details":name}));
            }
            assert_eq!(request(&mut server).await["details"], "Newest");
            peer_shared.desired.lock().unwrap().activity = None;
            let cleared = Instant::now();
            assert!(request(&mut server).await.is_null());
            assert!(cleared.elapsed() < Duration::from_millis(250));
            peer_shared.closed.store(true, Ordering::Release);
            // Keep the transport open until the loop observes shutdown.
            tokio::time::sleep(Duration::from_millis(30)).await;
        });
        let mut client = Ipc::new(client);
        serve(
            &mut client,
            &shared,
            ID,
            Timing {
                poll: Duration::from_millis(5),
                publish: Duration::from_millis(300),
                retry: Duration::from_millis(5),
            },
        )
        .await
        .unwrap();
        peer.await.unwrap();
    });
}

// A real OS transport exercises the production worker, rather than just duplex.
#[test]
fn native_worker_reconnects_clears_on_disable_id_change_and_drop() {
    use std::sync::mpsc;
    let rt = runtime();
    #[cfg(target_os = "windows")]
    let path = std::path::PathBuf::from(format!(
        r"\\?\pipe\octave-discord-test-{}",
        std::process::id()
    ));
    #[cfg(not(target_os = "windows"))]
    let path =
        std::env::temp_dir().join(format!("octave-discord-test-{}.sock", std::process::id()));
    let (events_tx, events_rx) = mpsc::channel();
    let (bound_tx, bound_rx) = mpsc::channel();
    let server_path = path.clone();
    let peer = std::thread::spawn(move || {
        rt.block_on(async move {
            // First connection drops to model Discord restarting. Second clears on
            // disable; third clears before switching app IDs; fourth clears on quit.
            for session in 0..4 {
                #[cfg(target_os = "windows")]
                let server = {
                    let server = tokio::net::windows::named_pipe::ServerOptions::new()
                        .first_pipe_instance(session == 0)
                        .create(&server_path)
                        .unwrap();
                    if session == 0 {
                        bound_tx.send(()).unwrap();
                    }
                    bounded(server.connect()).await.unwrap();
                    server
                };
                #[cfg(not(target_os = "windows"))]
                let server = {
                    let listener = tokio::net::UnixListener::bind(&server_path).unwrap();
                    if session == 0 {
                        bound_tx.send(()).unwrap();
                    }
                    let (server, _) = bounded(listener.accept()).await.unwrap();
                    drop(listener);
                    std::fs::remove_file(&server_path).unwrap();
                    server
                };
                let mut server = Ipc::new(server);
                ready(
                    &mut server,
                    if session == 3 {
                        "223456789012345678"
                    } else {
                        ID
                    },
                )
                .await;
                assert_eq!(request(&mut server).await["details"], "Native fixture");
                events_tx.send(session).unwrap();
                if session > 0 {
                    assert!(request(&mut server).await.is_null());
                    events_tx.send(session + 10).unwrap();
                }
            }
        })
    });
    bound_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    let desired = Desired {
        id: ID.into(),
        activity: Some(json!({"details":"Native fixture"})),
    };
    let worker = Worker::start_at(
        desired.clone(),
        vec![path],
        Timing {
            poll: Duration::from_millis(5),
            publish: Duration::from_millis(5),
            retry: Duration::from_millis(20),
        },
    );
    let event = |expected| {
        assert_eq!(
            events_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            expected
        )
    };
    event(0);
    event(1);
    worker.update(Desired::default());
    event(11);
    worker.update(desired.clone());
    event(2);
    worker.update(Desired {
        id: "223456789012345678".into(),
        ..desired
    });
    event(12);
    event(3);
    let shutdown = Instant::now();
    drop(worker);
    event(13);
    assert!(shutdown.elapsed() < Duration::from_secs(3));
    peer.join().unwrap();
}

#[test]
fn unresponsive_peer_times_out_and_close_aborts_the_session() {
    runtime().block_on(async {
        let (client, server) = tokio::io::duplex(4096);
        let mut client = Ipc::new(client);
        let start = Instant::now();
        assert_eq!(
            bounded(client.publish(None)).await.unwrap_err().kind(),
            io::ErrorKind::TimedOut
        );
        assert!(start.elapsed() < Duration::from_secs(3));
        drop(server);
        assert_eq!(
            client
                .unsolicited((2, b"{\"message\":\"private peer text\"}".to_vec()))
                .await
                .unwrap_err()
                .kind(),
            io::ErrorKind::ConnectionAborted
        );
    });
}

/// Explicit opt-in: publishes actual unsigned playback to the running Discord
/// desktop. Never reads a token/profile or changes system/app-profile volume.
#[test]
#[ignore = "Attended live Discord publishing and unsigned default-device playback at zero volume"]
fn live_discord_with_muted_unsigned_playback() {
    assert_eq!(
        std::env::var("OCTAVE_DISCORD_LIVE").as_deref(),
        Ok("1"),
        "Set OCTAVE_DISCORD_LIVE=1 to authorize live presence publishing"
    );
    let player = octave_player::Player::new();
    player.set_volume(0.0);
    let mut presence = Presence::default();
    let track = Track {
        id: TrackId("dQw4w9WgXcQ".into()),
        title: "Never Gonna Give You Up".into(),
        artists: vec!["Rick Astley".into()],
    };
    fn wait(
        presence: &mut Presence,
        player: &octave_player::Player,
        track: &Track,
        enabled: bool,
        stage: &str,
        predicate: impl Fn(&Snapshot, &str) -> bool,
    ) {
        let start = Instant::now();
        loop {
            let snapshot = player.snapshot();
            presence.update(enabled, "", Some(track), &snapshot);
            let status = presence.status();
            assert!(
                !matches!(snapshot.state, PlaybackState::Failed(..)),
                "{stage}: {:?}",
                snapshot.state
            );
            if predicate(&snapshot, &status) {
                eprintln!(
                    "{stage}: {status}; consumed {:.3}s",
                    snapshot.position.as_secs_f64()
                );
                return;
            }
            assert!(
                start.elapsed() < Duration::from_secs(35),
                "{stage}: {status}; {:?}",
                snapshot.state
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    let sharing = |s: &Snapshot, status: &str| {
        matches!(s.state, PlaybackState::Playing(_))
            && !s.buffering
            && s.position > Duration::from_millis(200)
            && status == "Sharing current song"
    };
    player.play(track.id.clone());
    wait(
        &mut presence,
        &player,
        &track,
        true,
        "initial publish",
        sharing,
    );
    // Leave the profile visible long enough for an attended appearance check.
    let seconds = std::env::var("OCTAVE_DISCORD_HOLD_SECONDS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(20)
        .clamp(20, 120);
    let hold = Instant::now();
    while hold.elapsed() < Duration::from_secs(seconds) {
        presence.update(true, "", Some(&track), &player.snapshot());
        std::thread::sleep(Duration::from_millis(50));
    }
    player.toggle_pause();
    wait(
        &mut presence,
        &player,
        &track,
        true,
        "pause clear",
        |s, status| {
            matches!(s.state, PlaybackState::Paused(_)) && status == "Connected · presence cleared"
        },
    );
    player.toggle_pause();
    wait(
        &mut presence,
        &player,
        &track,
        true,
        "resume publish",
        sharing,
    );
    player.seek(Duration::from_secs(90));
    wait(
        &mut presence,
        &player,
        &track,
        true,
        "seek publish",
        |s, status| sharing(s, status) && s.position >= Duration::from_secs(90),
    );
    player.stop();
    wait(
        &mut presence,
        &player,
        &track,
        true,
        "stop clear",
        |s, status| {
            matches!(s.state, PlaybackState::Idle) && status == "Connected · presence cleared"
        },
    );
    let next = Track {
        id: TrackId("hpSrLjc5SMs".into()),
        title: "Wonderwall".into(),
        artists: vec!["Oasis".into()],
    };
    player.play(next.id.clone());
    wait(
        &mut presence,
        &player,
        &next,
        true,
        "next recording publish",
        sharing,
    );
    wait(
        &mut presence,
        &player,
        &next,
        false,
        "disable clear",
        |_, status| status == "Disabled",
    );
    wait(
        &mut presence,
        &player,
        &next,
        true,
        "re-enable publish",
        sharing,
    );
    drop(presence);
    player.stop();
    eprintln!("quit: bounded clear and worker shutdown completed; audio stopped");
}
