use crate::{
    listening::{Repeat, Session},
    model::time,
    search::Search,
    storage::{self, Store},
    theme,
};
use eframe::egui;
use octave_core::{Track, TrackId};
use octave_player::{PlaybackState, Player};
use std::{
    path::PathBuf,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub fn run() -> eframe::Result {
    let path = crate::argument("--data-dir")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("LOCALAPPDATA")
                .map(PathBuf::from)
                .map(|p| p.join("Octave"))
        })
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .map(|p| p.join(".local/share/octave"))
        })
        .expect("Provide --data-dir when no user data directory is available")
        .join("listening.sqlite3");
    eframe::run_native(
        "Octave",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([1180.0, 760.0])
                .with_min_inner_size([480.0, 480.0]),
            ..Default::default()
        },
        Box::new(move |cc| {
            theme::install(&cc.egui_ctx);
            if let Some(scale) = crate::argument("--scale").and_then(|s| s.parse::<f32>().ok()) {
                cc.egui_ctx.set_pixels_per_point(scale.clamp(1.0, 2.0));
            }
            Ok(Box::new(Alpha::new(path)))
        }),
    )
}

struct Alpha {
    session: Session,
    player: Player,
    store: Store,
    loaded: bool,
    storage_failed: bool,
    saved: Session,
    save_time: Instant,
    search: Search,
    query: String,
    results: Vec<Track>,
    debounce: Option<Instant>,
    searching: bool,
    search_message: String,
    queue_page: bool,
    video_id: String,
    message: String,
    retry_at: Option<Instant>,
    retry_count: u32,
    failure_seen: bool,
    ended_seen: bool,
    seek: Option<f64>,
    frames: u32,
    capture_requested: bool,
}
impl Alpha {
    fn new(path: PathBuf) -> Self {
        Self {
            session: Session::default(),
            player: Player::new(),
            store: Store::open(path),
            loaded: false,
            storage_failed: false,
            saved: Session::default(),
            save_time: Instant::now(),
            search: Search::new(),
            query: String::new(),
            results: Vec::new(),
            debounce: None,
            searching: false,
            search_message: String::new(),
            queue_page: false,
            video_id: String::new(),
            message: String::new(),
            retry_at: None,
            retry_count: 0,
            failure_seen: false,
            ended_seen: false,
            seek: None,
            frames: 0,
            capture_requested: false,
        }
    }
    fn start(&mut self, reset_retry: bool) {
        if let Some(track) = self.session.current() {
            self.player.play_at(
                track.id.clone(),
                Duration::from_secs_f64(self.session.position.max(0.0)),
            );
            self.player.set_volume(self.session.volume);
            self.retry_at = None;
            self.failure_seen = false;
            self.ended_seen = false;
            self.seek = None;
            if reset_retry {
                self.retry_count = 0;
            }
        }
    }
    fn halt(&mut self) {
        self.player.stop();
        self.retry_at = None;
        self.failure_seen = false;
        self.seek = None;
    }
    fn toggle(&mut self) {
        match self.player.snapshot().state {
            PlaybackState::Playing(_) | PlaybackState::Paused(_) => self.player.toggle_pause(),
            PlaybackState::Loading(_) => self.halt(),
            _ => self.start(true),
        }
    }
    fn next(&mut self, automatic: bool) {
        self.halt();
        if self.session.advance(automatic) {
            self.start(true);
        }
    }
    fn previous(&mut self) {
        self.halt();
        self.session.previous();
        self.start(true);
    }
    fn add(&mut self, track: Track, next: bool) {
        self.message = if self.session.append(track, next) {
            "Added to queue."
        } else {
            "Already queued, or the 500-song queue is full."
        }
        .into();
    }
    fn poll(&mut self) {
        while let Ok(event) = self.store.events.try_recv() {
            match event {
                storage::Event::Loaded(session) => {
                    self.session = session;
                    self.saved = self.session.clone();
                    self.player.set_volume(self.session.volume);
                    self.loaded = true;
                }
                storage::Event::Saved(session) => {
                    self.storage_failed = false;
                    self.saved = session;
                }
                storage::Event::Failed => {
                    self.storage_failed = true;
                    self.loaded = true;
                }
            }
        }
        while let Ok((generation, result)) = self.search.results.try_recv() {
            if self.search.accepts(generation) {
                self.searching = false;
                match result {
                    Ok(tracks) => {
                        self.search_message = format!("{} songs found", tracks.len());
                        self.results = tracks;
                    }
                    Err(error) => {
                        self.search_message = error.to_string();
                        self.results.clear();
                    }
                }
            }
        }
        if self
            .debounce
            .is_some_and(|t| t.elapsed() >= Duration::from_millis(350))
        {
            self.debounce = None;
            if !self.query.trim().is_empty() {
                self.search.submit(self.query.clone());
                self.searching = true;
            }
        }
        let snapshot = self.player.snapshot();
        let same_track = match &snapshot.state {
            PlaybackState::Idle => false,
            PlaybackState::Loading(id)
            | PlaybackState::Playing(id)
            | PlaybackState::Paused(id)
            | PlaybackState::Ended(id)
            | PlaybackState::Failed(id, _) => self.session.current().is_some_and(|t| &t.id == id),
        };
        if same_track {
            self.session.position = snapshot.position.as_secs_f64();
            match &snapshot.state {
                PlaybackState::Ended(_) if !self.ended_seen => {
                    self.ended_seen = true;
                    self.next(true);
                }
                PlaybackState::Failed(_, failure) if !self.failure_seen => {
                    self.failure_seen = true;
                    if failure.retryable() && self.retry_count < 2 {
                        self.retry_count += 1;
                        self.retry_at = Some(
                            Instant::now() + Duration::from_secs(u64::from(self.retry_count) * 2),
                        );
                    }
                }
                _ => (),
            }
        }
        if self.retry_at.is_some_and(|t| Instant::now() >= t) {
            self.start(false);
        }
        if self.loaded
            && self.save_time.elapsed() >= Duration::from_secs(2)
            && self.session != self.saved
            && self.store.save(self.session.clone())
        {
            self.save_time = Instant::now();
        }
    }
    fn transport(&mut self, ui: &mut egui::Ui) {
        let snapshot = self.player.snapshot();
        ui.add_space(8.0);
        if let Some(track) = self.session.current() {
            ui.add(
                egui::Label::new(egui::RichText::new(&track.title).strong().size(18.0)).truncate(),
            )
            .on_hover_text(&track.title);
            ui.weak(track.artists.join(" · "));
        } else {
            ui.strong("Your next listen starts here");
        }
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    self.session.current().is_some(),
                    egui::Button::new("Previous"),
                )
                .clicked()
            {
                self.previous();
            }
            let label = match snapshot.state {
                PlaybackState::Playing(_) => "Pause",
                PlaybackState::Loading(_) => "Cancel",
                _ => "Resume",
            };
            if ui
                .add_enabled(
                    self.loaded && self.session.current().is_some(),
                    egui::Button::new(label).fill(theme::ACCENT),
                )
                .clicked()
            {
                self.toggle();
            }
            if ui
                .add_enabled(self.session.current().is_some(), egui::Button::new("Next"))
                .clicked()
            {
                self.next(false);
            }
            if ui.button("Stop").clicked() {
                self.halt();
            }
            if ui
                .button(format!(
                    "Repeat: {}",
                    match self.session.repeat {
                        Repeat::Off => "off",
                        Repeat::All => "queue",
                        Repeat::One => "song",
                    }
                ))
                .clicked()
            {
                self.session.repeat = self.session.repeat.next();
            }
            if ui.button("Shuffle upcoming").clicked() {
                self.session.shuffle(
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_nanos() as u64,
                );
            }
            ui.add(
                egui::Slider::new(&mut self.session.volume, 0.0..=1.0)
                    .text("Volume")
                    .show_value(false),
            );
        });
        self.player.set_volume(self.session.volume);
        let state = match &snapshot.state {
            PlaybackState::Idle => {
                if self.session.current().is_some() {
                    "Ready · press Resume to listen"
                } else {
                    "Nothing queued"
                }
            }
            PlaybackState::Loading(_) => "Connecting…",
            PlaybackState::Playing(_) => "Playing",
            PlaybackState::Paused(_) => "Paused",
            PlaybackState::Ended(_) => "Finished",
            PlaybackState::Failed(_, _) => "Playback interrupted",
        };
        ui.horizontal(|ui| {
            ui.weak(state);
            ui.weak(format!(
                "{} / {}",
                time(self.session.position),
                time(snapshot.duration.as_secs_f64())
            ));
        });
        let mut seconds = self.seek.unwrap_or(self.session.position);
        let can_seek = !snapshot.duration.is_zero()
            && matches!(
                snapshot.state,
                PlaybackState::Playing(_) | PlaybackState::Paused(_)
            );
        let slider = ui.add_enabled(
            can_seek,
            egui::Slider::new(&mut seconds, 0.0..=snapshot.duration.as_secs_f64().max(1.0))
                .text("Position")
                .show_value(false),
        );
        if slider.dragged() {
            self.seek = Some(seconds);
        }
        if slider.drag_stopped() || (slider.changed() && !slider.dragged()) {
            self.player.seek(Duration::from_secs_f64(seconds));
            self.seek = None;
        }
        if let PlaybackState::Failed(_, error) = &snapshot.state {
            ui.label(error.to_string());
            ui.horizontal_wrapped(|ui| {
                if self.retry_at.is_some() {
                    ui.weak(format!("Reconnecting · attempt {} of 2", self.retry_count));
                    if ui.button("Cancel retry").clicked() {
                        self.retry_at = None;
                    }
                } else if ui.button("Retry from saved position").clicked() {
                    self.start(true);
                }
                if ui.button("Skip song").clicked() {
                    self.next(false);
                }
            });
        }
        ui.add_space(6.0);
    }
    fn content(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.heading("Octave");
            ui.weak("LISTENING ALPHA");
            if ui.selectable_label(!self.queue_page, "Search").clicked() {
                self.queue_page = false;
            }
            if ui
                .selectable_label(
                    self.queue_page,
                    format!("Queue · {}", self.session.queue.len()),
                )
                .clicked()
            {
                self.queue_page = true;
            }
        });
        ui.weak("Unsigned access · some songs require sign-in and cannot play in this alpha.");
        if !self.loaded {
            ui.spinner();
            ui.label("Restoring your queue…");
            return;
        }
        if self.storage_failed {
            ui.colored_label(egui::Color32::LIGHT_RED, "Local storage is unavailable. This session may not survive restart; existing data has been left intact.");
        }
        ui.add_space(12.0);
        if self.queue_page {
            ui.horizontal_wrapped(|ui| {
                ui.heading("Up next");
                if ui.button("Clear queue").clicked() {
                    self.halt();
                    self.session.queue.clear();
                    self.session.cursor = 0;
                    self.session.position = 0.0;
                }
                ui.weak("Queue order and position are saved on this device.");
            });
        } else {
            ui.heading("Find something to get lost in.");
            let input = ui.add(
                egui::TextEdit::singleline(&mut self.query)
                    .id(egui::Id::new("alpha-search"))
                    .hint_text("Search YouTube Music")
                    .char_limit(512)
                    .desired_width(f32::INFINITY),
            );
            if ui.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::K)) {
                input.request_focus();
            }
            if input.changed() {
                self.search.invalidate();
                self.results.clear();
                self.searching = false;
                self.search_message.clear();
                self.debounce = Some(Instant::now());
            }
            ui.horizontal(|ui| {
                if self.searching {
                    ui.spinner();
                    ui.label("Searching…");
                } else {
                    ui.weak(&self.search_message);
                }
                if !self.query.trim().is_empty() && ui.button("Search again").clicked() {
                    self.search.invalidate();
                    self.search.submit(self.query.clone());
                    self.searching = true;
                    self.debounce = None;
                }
            });
            ui.collapsing("Open a video ID", |ui| {
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.video_id)
                            .hint_text("11-character video ID")
                            .char_limit(11),
                    );
                    if ui.button("Add to queue").clicked() {
                        let id = self.video_id.trim();
                        if id.len() == 11
                            && id
                                .bytes()
                                .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
                        {
                            self.add(
                                Track {
                                    id: TrackId(id.to_owned()),
                                    title: id.to_owned(),
                                    artists: Vec::new(),
                                },
                                false,
                            );
                        } else {
                            self.message = "Enter a valid 11-character video ID.".into();
                        }
                    }
                });
            });
        }
        if !self.message.is_empty() {
            ui.weak(&self.message);
        }
        ui.separator();
        let len = if self.queue_page {
            self.session.queue.len()
        } else {
            self.results.len()
        };
        if len == 0 {
            ui.add_space(24.0);
            ui.label(if self.queue_page {
                "Your queue is empty. Search for a song to begin."
            } else {
                "Search for songs, then play a result or build your queue."
            });
        }
        let mut action = None;
        egui::ScrollArea::vertical()
            .id_salt("alpha-tracks")
            .show_rows(ui, 64.0, len, |ui, range| {
                for index in range {
                    let track = if self.queue_page {
                        &self.session.queue[index]
                    } else {
                        &self.results[index]
                    };
                    ui.push_id(index, |ui| {
                        ui.horizontal(|ui| {
                            if ui
                                .button(if self.queue_page && self.session.cursor == index {
                                    "▶"
                                } else {
                                    "Play"
                                })
                                .on_hover_text(format!("Play {}", track.title))
                                .clicked()
                            {
                                action = Some((index, 0));
                            }
                            ui.vertical(|ui| {
                                ui.set_width((ui.available_width() - 60.0).max(120.0));
                                ui.add(
                                    egui::Label::new(egui::RichText::new(&track.title).strong())
                                        .truncate(),
                                )
                                .on_hover_text(&track.title);
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(track.artists.join(" · ")).weak(),
                                    )
                                    .truncate(),
                                );
                            });
                            ui.menu_button("•••", |ui| {
                                for (label, code) in if self.queue_page {
                                    vec![("Move up", 3), ("Move down", 4), ("Remove", 5)]
                                } else {
                                    vec![("Play next", 1), ("Add to queue", 2)]
                                } {
                                    if ui.button(label).clicked() {
                                        action = Some((index, code));
                                        ui.close();
                                    }
                                }
                            });
                        });
                        ui.separator();
                    });
                }
            });
        if let Some((index, code)) = action {
            if self.queue_page {
                match code {
                    0 => {
                        self.session.select(index);
                        self.start(true);
                    }
                    3 => self.session.move_item(index, index.saturating_sub(1)),
                    4 => self.session.move_item(index, index + 1),
                    5 if self.session.remove(index) => self.halt(),
                    _ => (),
                }
            } else {
                let track = self.results[index].clone();
                match code {
                    0 => {
                        self.session.queue = vec![track];
                        self.session.select(0);
                        self.start(true);
                    }
                    1 => self.add(track, true),
                    _ => self.add(track, false),
                }
            }
        }
    }
}
impl Drop for Alpha {
    fn drop(&mut self) {
        self.player.stop();
        self.store.finish(self.loaded.then(|| self.session.clone()));
    }
}
impl eframe::App for Alpha {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll();
        if !ui.ctx().egui_wants_keyboard_input() && self.loaded {
            if ui.input(|i| i.key_pressed(egui::Key::Space)) {
                self.toggle();
            }
            if ui.input(|i| i.modifiers.alt && i.key_pressed(egui::Key::ArrowRight)) {
                self.next(false);
            }
            if ui.input(|i| i.modifiers.alt && i.key_pressed(egui::Key::ArrowLeft)) {
                self.previous();
            }
        }
        if ui.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::K)) {
            self.queue_page = false;
        }
        egui::Panel::bottom("listening-player").show(ui, |ui| self.transport(ui));
        egui::CentralPanel::default().show(ui, |ui| self.content(ui));
        self.frames += 1;
        if std::env::args().any(|s| s == "--smoke-test") && self.loaded && self.frames >= 3 {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if let Some(path) = crate::argument("--capture") {
            let image = ui.input(|i| {
                i.events.iter().find_map(|e| {
                    if let egui::Event::Screenshot { image, .. } = e {
                        Some(image.clone())
                    } else {
                        None
                    }
                })
            });
            if let Some(image) = image {
                let bytes: Vec<u8> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
                if image::save_buffer(
                    path,
                    &bytes,
                    image.width() as u32,
                    image.height() as u32,
                    image::ColorType::Rgba8,
                )
                .is_ok()
                {
                    eprintln!("alpha_capture_saved");
                }
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
            if self.loaded && self.frames >= 30 && !self.capture_requested {
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
                self.capture_requested = true;
            }
        }
        ui.ctx().request_repaint_after(Duration::from_millis(50));
    }
}
