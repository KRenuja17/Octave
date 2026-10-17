use crate::{
    listening::{Repeat, Session},
    model::time,
    search::Search,
    storage::{self, Store},
    theme,
};
use eframe::egui;
use octave_core::{Track, TrackId};
use octave_player::{PlaybackState, Player, Snapshot};
use std::{
    path::PathBuf,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub fn run() -> eframe::Result {
    let path = crate::argument("--data-dir")
        .map(PathBuf::from)
        .or_else(crate::paths::data_directory)
        .expect("Provide --data-dir when no user data directory is available")
        .join("listening.sqlite3");
    eframe::run_native(
        "Octave",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size(if std::env::args().any(|s| s == "--compact") {
                    [560.0, 720.0]
                } else {
                    [1180.0, 760.0]
                })
                .with_min_inner_size([480.0, 480.0]),
            ..Default::default()
        },
        Box::new(move |cc| {
            theme::install(&cc.egui_ctx);
            if let Some(scale) = crate::argument("--scale").and_then(|s| s.parse::<f32>().ok()) {
                cc.egui_ctx.set_pixels_per_point(scale.clamp(1.0, 2.0));
            }
            let mut app = Alpha::new(path);
            app.account_page = crate::argument("--page").as_deref() == Some("account");
            app.desktop = Some(crate::desktop::Desktop::new(cc));
            Ok(Box::new(app))
        }),
    )
}

struct Alpha {
    eq_draft: octave_player::Equalizer,
    applied_appearance: Option<u8>,
    playlists: Option<crate::playlist_ui::PlaylistsUi>,
    playlists_path: PathBuf,
    output_ui: crate::output_ui::OutputUi,
    lyrics: crate::lyrics_ui::LyricsUi,
    account: crate::account_ui::AccountUi,
    account_page: bool,
    desktop: Option<crate::desktop::Desktop>,
    quitting: bool,
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
            eq_draft: octave_player::Equalizer::default(),
            applied_appearance: None,
            playlists: None,
            playlists_path: path.with_extension("playlists.sqlite3"),
            output_ui: crate::output_ui::OutputUi::default(),
            lyrics: crate::lyrics_ui::LyricsUi::default(),
            account: crate::account_ui::AccountUi::default(),
            account_page: false,
            desktop: None,
            quitting: false,
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
    fn change_output(&mut self, device: Option<octave_player::OutputDevice>) {
        self.halt_at_current_position();
        self.player.set_output_device(device);
        self.message = "Output selected. Press Resume to continue from your saved position.".into();
    }
    fn halt_at_current_position(&mut self) {
        let snapshot = self.player.snapshot();
        if matches!(
            snapshot.state,
            PlaybackState::Playing(_) | PlaybackState::Paused(_)
        ) {
            self.session.position = snapshot.position.as_secs_f64();
        }
        self.halt();
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
        self.account.poll();
        while let Ok(event) = self.store.events.try_recv() {
            match event {
                storage::Event::Loaded(session) => {
                    self.session = session;
                    self.eq_draft = self.session.equalizer;
                    self.player.set_equalizer(self.session.equalizer);
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
        self.observe(snapshot);
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
    fn observe(&mut self, snapshot: Snapshot) {
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
                    egui::Button::new(egui::RichText::new(label).color(theme::BG))
                        .fill(theme::ACCENT),
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
        ui.horizontal_wrapped(|ui| {
            ui.heading("Octave");
            ui.weak("DESKTOP BETA PREVIEW");
            if ui
                .selectable_label(!self.queue_page && !self.account_page, "Search")
                .clicked()
            {
                self.account_page = false;
                self.queue_page = false;
            }
            if ui
                .selectable_label(
                    self.queue_page && !self.account_page,
                    format!("Queue · {}", self.session.queue.len()),
                )
                .clicked()
            {
                self.account_page = false;
                self.queue_page = true;
            }
            if ui.selectable_label(self.account_page, "Account").clicked() {
                self.account_page = true;
            }
        });
        ui.weak("Unsigned access · some songs require sign-in and cannot play in this alpha.");
        if let Some(desktop) = &mut self.desktop {
            egui::CollapsingHeader::new("Desktop settings")
                .default_open(crate::argument("--page").as_deref() == Some("desktop"))
                .show(ui, |ui| {
                    ui.weak(&desktop.status);
                    ui.add_enabled(
                        desktop.can_hide(),
                        egui::Checkbox::new(
                            &mut desktop.close_to_tray,
                            "Keep playing in tray when closing this window",
                        ),
                    );
                    ui.weak(
                    "This setting lasts until Quit. Use the tray menu to show the window again.",
                );
                    if ui.button("Quit Octave").clicked() {
                        self.quitting = true;
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
        }
        if !self.loaded {
            ui.spinner();
            ui.label("Restoring your queue…");
            return;
        }
        let selected_output = egui::CollapsingHeader::new("Audio output")
            .default_open(crate::argument("--page").as_deref() == Some("output"))
            .show(ui, |ui| self.output_ui.render(ui))
            .body_returned
            .flatten();
        if let Some(device) = selected_output {
            self.change_output(device);
        }
        egui::CollapsingHeader::new("Sound and appearance")
            .default_open(crate::argument("--page").as_deref()==Some("settings"))
            .show(ui, |ui| {
                ui.label("Three-band equalizer");
                for (index,label) in ["Bass · 100 Hz", "Mid · 1 kHz", "Treble · 10 kHz"].into_iter().enumerate() {
                    ui.add(egui::Slider::new(&mut self.eq_draft.gains_db[index],-12.0..=12.0).suffix(" dB").text(label));
                }
                ui.weak(format!("Automatic headroom: {:.0} dB. Apply stops playback; Resume uses the new sound.",self.eq_draft.headroom_db()));
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Flat EQ").clicked() { self.eq_draft=octave_player::Equalizer::default(); }
                    if ui.button("Apply equalizer").clicked() {
                        self.halt_at_current_position();
                        self.session.equalizer=self.eq_draft.sanitized();
                        self.player.set_equalizer(self.session.equalizer);
                        self.message="Equalizer applied. Press Resume when ready.".into();
                    }
                });
                egui::ComboBox::from_label("Appearance").selected_text(["Dark","Light","High contrast"][usize::from(self.session.appearance.min(2))]).show_ui(ui,|ui| {
                    for (index,label) in ["Dark","Light","High contrast"].into_iter().enumerate() { ui.selectable_value(&mut self.session.appearance,index as u8,label); }
                });
                ui.weak("EQ and appearance are saved with your listening settings.");
            });
        if self.applied_appearance != Some(self.session.appearance) {
            theme::apply(ui.ctx(), self.session.appearance);
            self.applied_appearance = Some(self.session.appearance);
        }
        let loaded_playlist = egui::CollapsingHeader::new("Local playlists")
            .default_open(crate::argument("--page").as_deref() == Some("playlists"))
            .show(ui, |ui| {
                self.playlists
                    .get_or_insert_with(|| {
                        crate::playlist_ui::PlaylistsUi::new(self.playlists_path.clone())
                    })
                    .render(ui, &self.session.queue)
            })
            .body_returned
            .flatten();
        if let Some(tracks) = loaded_playlist {
            self.halt();
            self.session.queue = tracks;
            self.session.cursor = 0;
            self.session.position = 0.0;
            self.queue_page = true;
            self.account_page = false;
            self.message = "Playlist loaded. Press Resume to start listening.".into();
        }
        if self.account_page {
            let action = egui::ScrollArea::vertical()
                .id_salt("account-page")
                .max_height(ui.clip_rect().height().max(80.0))
                .show(ui, |ui| self.account.render(ui, self.session.current()))
                .inner;
            if let Some(action) = action {
                match action {
                    crate::account_ui::Action::Play(track) => {
                        self.session.queue = vec![track];
                        self.session.select(0);
                        self.start(true);
                    }
                    crate::account_ui::Action::Append(track) => self.add(track, false),
                }
            }
            return;
        }
        if self.storage_failed {
            ui.colored_label(egui::Color32::LIGHT_RED, "Local storage is unavailable. This session may not survive restart; existing data has been left intact.");
        }
        ui.add_space(12.0);
        self.lyrics
            .render(ui, self.session.current().map(|t| &t.id));
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
            input.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Search YouTube Music")
            });
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
            if !self.results.is_empty() {
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Play results").clicked() {
                        self.session.queue = self.results.clone();
                        self.session.select(0);
                        self.start(true);
                    }
                    if ui.button("Add results to queue").clicked() {
                        let mut added = 0;
                        for track in &self.results {
                            added += usize::from(self.session.append(track.clone(), false));
                        }
                        self.message =
                            format!("Added {added} songs. Already queued songs were skipped.");
                    }
                });
            }
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
            .max_height(ui.clip_rect().height().max(80.0))
            .show_rows(ui, 64.0, len, |ui, range| {
                for index in range {
                    let track = if self.queue_page {
                        &self.session.queue[index]
                    } else {
                        &self.results[index]
                    };
                    ui.push_id(index, |ui| {
                        ui.horizontal(|ui| {
                            let play = ui
                                .button(if self.queue_page && self.session.cursor == index {
                                    "▶"
                                } else {
                                    "Play"
                                })
                                .on_hover_text(format!("Play {}", track.title));
                            play.widget_info(|| {
                                egui::WidgetInfo::labeled(
                                    egui::WidgetType::Button,
                                    true,
                                    format!("Play {}", track.title),
                                )
                            });
                            if play.clicked() {
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
                            let menu = ui.menu_button("•••", |ui| {
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
                            menu.response.widget_info(|| {
                                egui::WidgetInfo::labeled(
                                    egui::WidgetType::Button,
                                    true,
                                    format!("Actions for {}", track.title),
                                )
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
    fn raw_input_hook(&mut self, _ctx: &egui::Context, input: &mut egui::RawInput) {
        if crate::argument("--capture").is_some()
            && std::env::args().any(|s| s == "--capture-scroll")
            && (25..32).contains(&self.frames)
        {
            let rect = input.screen_rect.unwrap_or(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(560.0, 720.0),
            ));
            input.events.push(egui::Event::PointerMoved(rect.center()));
            input.events.push(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, -100.0),
                phase: egui::TouchPhase::Move,
                modifiers: egui::Modifiers::NONE,
            });
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll();
        let commands = self.desktop.as_ref().map(|d| d.drain()).unwrap_or_default();
        for command in commands {
            self.desktop_command(command, ui.ctx());
        }
        if let Some(desktop) = &mut self.desktop {
            desktop.update(self.session.current(), &self.player.snapshot());
            if !self.quitting
                && desktop.close_to_tray
                && desktop.can_hide()
                && ui.input(|i| i.viewport().close_requested())
            {
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::CancelClose);
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::Visible(false));
            }
        }
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
            self.account_page = false;
            self.queue_page = false;
        }
        egui::Panel::bottom("listening-player").show(ui, |ui| self.transport(ui));
        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("listening-content")
                .show(ui, |ui| self.content(ui));
        });
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

impl Alpha {
    fn desktop_command(&mut self, command: crate::desktop::Command, ctx: &egui::Context) {
        use crate::desktop::Command;
        match command {
            Command::Show => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
            Command::Quit => {
                self.quitting = true;
                self.halt();
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            _ if !self.loaded => (),
            Command::Play => match self.player.snapshot().state {
                PlaybackState::Playing(_) | PlaybackState::Loading(_) => (),
                PlaybackState::Paused(_) => self.player.toggle_pause(),
                _ => self.start(true),
            },
            Command::Pause => {
                if matches!(self.player.snapshot().state, PlaybackState::Playing(_)) {
                    self.player.toggle_pause();
                }
            }
            Command::Toggle => self.toggle(),
            Command::Next => self.next(false),
            Command::Previous => self.previous(),
            Command::Stop => self.halt(),
            Command::Seek(position) => self.player.seek(position),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::listening::track;
    use octave_player::Failure;
    use octave_provider::Error;

    fn path(name: &str) -> PathBuf {
        std::env::var_os("OCTAVE_TEST_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join(format!(
                "octave-{name}-{}-{}.sqlite3",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ))
    }
    fn loaded(path: PathBuf) -> Alpha {
        let mut app = Alpha::new(path);
        let start = Instant::now();
        while !app.loaded {
            app.poll();
            assert!(start.elapsed() < Duration::from_secs(5));
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(!app.storage_failed);
        app
    }
    fn content_frame(
        ctx: &egui::Context,
        app: &mut Alpha,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1180.0, 760.0),
                )),
                events,
                ..Default::default()
            },
            |ui| app.content(ui),
        );
        output.textures_delta.clear();
        output
    }
    fn click(ctx: &egui::Context, app: &mut Alpha, name: &str) {
        let output = content_frame(ctx, app, vec![]);
        let tree = output.platform_output.accesskit_update.unwrap();
        let bounds = tree
            .nodes
            .iter()
            .find_map(|(_, node)| {
                (node.label() == Some(name))
                    .then(|| node.bounds())
                    .flatten()
            })
            .unwrap_or_else(|| panic!("Missing action: {name}"));
        let pos = egui::pos2(
            ((bounds.x0 + bounds.x1) / 2.0) as f32,
            ((bounds.y0 + bounds.y1) / 2.0) as f32,
        );
        for pressed in [true, false] {
            content_frame(
                ctx,
                app,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
    }
    #[test]
    fn search_results_to_queue_and_clear_work_through_widgets() {
        let path = path("widgets");
        let mut app = loaded(path.clone());
        let ctx = egui::Context::default();
        theme::install(&ctx);
        ctx.enable_accesskit();
        app.results = vec![track(1), track(2)];
        content_frame(&ctx, &mut app, vec![]);
        click(&ctx, &mut app, "Add results to queue");
        assert_eq!(app.session.queue.len(), 2);
        click(&ctx, &mut app, "Add results to queue");
        assert_eq!(app.session.queue.len(), 2);
        click(&ctx, &mut app, "Queue · 2");
        assert!(app.queue_page);
        click(&ctx, &mut app, "Clear queue");
        assert!(app.session.queue.is_empty());
        assert_eq!(app.player.snapshot().state, PlaybackState::Idle);
        drop(app);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn equalizer_apply_preserves_queue_and_preferences_survive_restart() {
        let path = path("equalizer");
        let mut app = loaded(path.clone());
        app.add(track(1), false);
        app.session.position = 42.0;
        app.eq_draft = octave_player::Equalizer {
            gains_db: [6.0, -4.0, 2.0],
        };
        app.session.appearance = 1;
        let ctx = egui::Context::default();
        theme::install(&ctx);
        ctx.enable_accesskit();
        content_frame(&ctx, &mut app, vec![]);
        click(&ctx, &mut app, "Sound and appearance");
        for _ in 0..20 {
            content_frame(&ctx, &mut app, vec![]);
        }
        click(&ctx, &mut app, "Apply equalizer");
        assert_eq!(app.session.equalizer, app.eq_draft);
        assert_eq!(app.session.position, 42.0);
        assert_eq!(app.player.snapshot().state, PlaybackState::Idle);
        drop(app);
        let restored = loaded(path.clone());
        assert_eq!(restored.session.equalizer.gains_db, [6.0, -4.0, 2.0]);
        assert_eq!(restored.session.appearance, 1);
        assert_eq!(restored.player.snapshot().state, PlaybackState::Idle);
        drop(restored);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn output_selection_keeps_queue_position_and_cancels_retry() {
        let path = path("output-selection");
        let mut app = loaded(path.clone());
        app.add(track(1), false);
        app.session.position = 42.0;
        app.retry_at = Some(Instant::now() + Duration::from_secs(10));
        let ctx = egui::Context::default();
        theme::install(&ctx);
        ctx.enable_accesskit();
        content_frame(&ctx, &mut app, vec![]);
        click(&ctx, &mut app, "Audio output");
        // Settle the collapsing-header animation before locating the button.
        for _ in 0..20 {
            content_frame(&ctx, &mut app, vec![]);
        }
        click(&ctx, &mut app, "Use selected output");
        assert_eq!(app.session.queue.len(), 1);
        assert_eq!(app.session.position, 42.0);
        assert!(app.retry_at.is_none());
        assert_eq!(app.player.snapshot().state, PlaybackState::Idle);
        drop(app);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn restart_restores_queue_position_and_volume_without_audio() {
        let path = path("restart");
        let mut app = loaded(path.clone());
        app.add(track(1), false);
        app.add(track(2), false);
        app.session.select(1);
        app.session.position = 37.5;
        app.session.volume = 0.2;
        app.session.repeat = Repeat::All;
        let expected = app.session.clone();
        drop(app);
        let restored = loaded(path.clone());
        assert_eq!(restored.session, expected);
        assert_eq!(restored.player.snapshot().state, PlaybackState::Idle);
        drop(restored);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn interruption_retains_queue_and_position_and_retry_is_bounded() {
        let path = path("recovery");
        let mut app = loaded(path.clone());
        app.add(track(1), false);
        app.add(track(2), false);
        let queue = app.session.queue.clone();
        let failed = Snapshot {
            state: PlaybackState::Failed(track(1).id, Failure::Provider(Error::Timeout)),
            position: Duration::from_secs(42),
            duration: Duration::from_secs(180),
        };
        for attempt in 1..=2 {
            app.failure_seen = false;
            app.retry_at = None;
            app.observe(failed.clone());
            assert_eq!(app.retry_count, attempt);
            assert!(app.retry_at.is_some());
            app.observe(failed.clone());
            assert_eq!(
                app.retry_count, attempt,
                "Repeated UI frames must not spend retries"
            );
        }
        app.failure_seen = false;
        app.retry_at = None;
        app.observe(failed);
        assert!(app.retry_at.is_none());
        assert_eq!(app.session.position, 42.0);
        assert_eq!(app.session.queue, queue);
        app.halt();
        assert!(app.retry_at.is_none());
        assert_eq!(app.session.position, 42.0);
        app.failure_seen = false;
        app.observe(Snapshot {
            state: PlaybackState::Failed(track(2).id, Failure::Provider(Error::Timeout)),
            position: Duration::ZERO,
            duration: Duration::ZERO,
        });
        assert_eq!(
            app.session.position, 42.0,
            "Stale track events must be ignored"
        );
        app.retry_count = 0;
        app.failure_seen = false;
        app.observe(Snapshot {
            state: PlaybackState::Failed(track(1).id, Failure::Provider(Error::SignInRequired)),
            position: Duration::from_secs(42),
            duration: Duration::ZERO,
        });
        assert!(app.retry_at.is_none());
        assert_eq!(app.retry_count, 0);
        drop(app);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn listening_layout_and_text_input_work_at_narrow_sizes() {
        let path = path("layout");
        let mut app = loaded(path.clone());
        let ctx = egui::Context::default();
        theme::install(&ctx);
        ctx.enable_accesskit();
        for size in [[1180.0, 760.0], [560.0, 720.0], [480.0, 480.0]] {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(size[0], size[1]),
                    )),
                    ..Default::default()
                },
                |ui| {
                    egui::Panel::bottom("test-player").show(ui, |ui| app.transport(ui));
                    egui::CentralPanel::default().show(ui, |ui| app.content(ui));
                },
            );
            output.textures_delta.clear();
            assert!(!output.shapes.is_empty());
        }
        ctx.memory_mut(|m| m.request_focus(egui::Id::new("alpha-search")));
        let old = app.search.generation;
        let mut output = ctx.run_ui(
            egui::RawInput {
                events: vec![egui::Event::Text("東京 ".into())],
                ..Default::default()
            },
            |ui| app.content(ui),
        );
        output.textures_delta.clear();
        assert_eq!(app.query, "東京 ");
        assert!(app.search.generation > old);
        assert!(app.debounce.is_some());
        assert_eq!(app.player.snapshot().state, PlaybackState::Idle);
        drop(app);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    #[ignore = "Opt-in live service/audio test; takes about four minutes and uses an isolated database"]
    fn live_full_track_restart_and_intentional_resume() {
        let path = path("live");
        let mut app = loaded(path.clone());
        let tracks = octave_provider::Provider::new()
            .unwrap()
            .search("Rick Astley Never Gonna Give You Up")
            .unwrap();
        let track = tracks
            .into_iter()
            .find(|t| t.id.0 == "dQw4w9WgXcQ")
            .expect("Reference result missing");
        app.add(track, false);
        app.session.volume = 0.0;
        app.start(true);
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            app.poll();
            if matches!(app.player.snapshot().state, PlaybackState::Playing(_))
                && app.session.position >= 3.0
            {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "Track did not start: {:?}",
                app.player.snapshot().state
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        let position = app.session.position;
        drop(app);
        let mut app = loaded(path.clone());
        assert_eq!(app.session.position, position);
        assert_eq!(app.player.snapshot().state, PlaybackState::Idle);
        app.start(true);
        let deadline = Instant::now() + Duration::from_secs(270);
        let mut played = false;
        loop {
            app.poll();
            let snapshot = app.player.snapshot();
            if matches!(snapshot.state, PlaybackState::Playing(_)) {
                played = true;
            }
            if played && snapshot.state == PlaybackState::Idle {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "Full track did not finish: {:?}",
                snapshot.state
            );
            if matches!(snapshot.state, PlaybackState::Failed(_, _)) && app.retry_at.is_none() {
                panic!("Playback failed: {:?}", snapshot.state);
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        assert_eq!(app.session.queue.len(), 1);
        assert_eq!(app.session.position, 0.0);
        println!("full_track_restart_intentional_resume=passed restored_seconds={position:.2}");
        drop(app);
        std::fs::remove_file(path).unwrap();
    }
}
