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
#[cfg(test)]
use std::time::{SystemTime, UNIX_EPOCH};
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
#[path = "listening_scene.rs"]
mod scene;

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
            if let Some(query) = crate::argument("--search") {
                app.query = query.chars().take(512).collect();
                app.debounce = Some(Instant::now());
            }
            app.desktop = Some(crate::desktop::Desktop::new(cc));
            Ok(Box::new(app))
        }),
    )
}

struct Alpha {
    focus: bool,
    catalog_page: bool,
    catalog: crate::catalog_ui::CatalogUi,
    last_history: Option<(TrackId, u64)>,
    history: crate::shell::History,
    playlist_import: crate::playlist_import::PlaylistImport,
    undo_queue: Option<(octave_core::queue::Queue, usize, f64, (u64, u64))>,
    listening_tab: u8,
    artwork: crate::song_artwork::SongArtwork,
    settings_page: bool,
    playlists_page: bool,
    transition_sequence: u64,
    crossfade_draft: u8,
    eq_draft: octave_player::Equalizer,
    applied_appearance: Option<u8>,
    playlists: Option<crate::playlist_ui::PlaylistsUi>,
    playlists_path: PathBuf,
    output_ui: crate::output_ui::OutputUi,
    lyrics: crate::lyrics_ui::LyricsUi,
    account: crate::account_ui::AccountUi,
    related: crate::related_ui::RelatedUi,
    account_page: bool,
    desktop: Option<crate::desktop::Desktop>,
    presence: crate::discord::Presence,
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
    capture_ready_at: Option<Instant>,
    capture_load_lyrics: bool,
}
impl Alpha {
    fn new(path: PathBuf) -> Self {
        Self {
            last_history: None,
            catalog_page: matches!(
                crate::argument("--page").as_deref(),
                Some("home" | "explore" | "history")
            ),
            catalog: crate::catalog_ui::CatalogUi::at(
                match crate::argument("--page").as_deref() {
                    Some("history") => "local:history",
                    Some("explore") => "FEmusic_explore",
                    _ => "FEmusic_home",
                }
                .into(),
            ),
            focus: crate::argument("--page").as_deref() == Some("focus"),
            history: crate::shell::History::default(),
            playlist_import: Default::default(),
            undo_queue: None,
            listening_tab: match crate::argument("--page").as_deref() {
                Some("now-playing") => 0,
                Some("lyrics") => 2,
                _ => 1,
            },
            artwork: crate::song_artwork::SongArtwork::default(),
            settings_page: matches!(
                crate::argument("--page").as_deref(),
                Some("settings" | "output" | "desktop")
            ),
            playlists_page: crate::argument("--page").as_deref() == Some("playlists"),
            transition_sequence: 0,
            crossfade_draft: 0,
            eq_draft: octave_player::Equalizer::default(),
            applied_appearance: None,
            playlists: None,
            playlists_path: path.with_extension("playlists.sqlite3"),
            output_ui: crate::output_ui::OutputUi::default(),
            lyrics: crate::lyrics_ui::LyricsUi::default(),
            account: crate::account_ui::AccountUi::default(),
            related: crate::related_ui::RelatedUi::default(),
            account_page: false,
            desktop: None,
            presence: Default::default(),
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
            queue_page: matches!(
                crate::argument("--page").as_deref(),
                Some("queue" | "now-playing" | "lyrics")
            ),
            video_id: String::new(),
            message: String::new(),
            retry_at: None,
            retry_count: 0,
            failure_seen: false,
            ended_seen: false,
            seek: None,
            frames: 0,
            capture_requested: false,
            capture_ready_at: None,
            capture_load_lyrics: crate::argument("--capture").is_some()
                && std::env::args().any(|s| s == "--capture-load-lyrics"),
        }
    }
    fn start(&mut self, reset_retry: bool) {
        self.transition_sequence = 0;
        if let Some(track) = self.session.current() {
            let id = track.id.clone();
            let position = Duration::from_secs_f64(self.session.position.max(0.0));
            if reset_retry {
                self.player.play_at(id, position);
            } else {
                self.player.retry_at(id, position);
            }
            self.player.set_volume(self.session.volume);
            self.retry_at = None;
            self.failure_seen = false;
            self.ended_seen = false;
            self.seek = None;
            if reset_retry {
                self.last_history = None;
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
            "Already queued, or the 10,000-entry queue is full."
        }
        .into();
    }
    fn poll(&mut self) {
        self.account.poll();
        while let Ok(event) = self.store.events.try_recv() {
            match event {
                storage::Event::Loaded(session) => {
                    self.session = session;
                    self.lyrics.translation.endpoint =
                        self.session.view.translation_endpoint.clone();
                    self.lyrics.translation.target = self.session.view.translation_target.clone();
                    self.eq_draft = self.session.equalizer;
                    self.player.set_equalizer(self.session.equalizer);
                    self.crossfade_draft = self.session.crossfade;
                    self.player.set_crossfade(self.session.crossfade);
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
        if self.loaded {
            if self.lyrics.translation.endpoint.is_empty()
                || octave_provider::translation::valid_endpoint(&self.lyrics.translation.endpoint)
            {
                self.session.view.translation_endpoint = self.lyrics.translation.endpoint.clone();
            }
            self.session.view.translation_target = self.lyrics.translation.target.clone();
        }
        let snapshot = self.player.snapshot();
        self.observe(snapshot);
        let current = self.session.current().map(|t| &t.id);
        self.lyrics.cancel_for(current);
        self.related.cancel_for(current);
        self.player
            .prepare_next(if self.session.continuous && self.session.preload {
                self.session.upcoming().map(|t| t.id.clone())
            } else {
                None
            });
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
        if snapshot.sequence > self.transition_sequence {
            let id = match &snapshot.state {
                PlaybackState::Playing(id)
                | PlaybackState::Paused(id)
                | PlaybackState::Ended(id)
                | PlaybackState::Failed(id, _) => Some(id),
                _ => None,
            };
            if let Some(cursor) = id.and_then(|id| {
                let expected = if self.session.repeat == Repeat::One {
                    self.session.cursor
                } else if self.session.cursor + 1 < self.session.queue.len() {
                    self.session.cursor + 1
                } else {
                    0
                };
                self.session
                    .queue
                    .get(expected)
                    .filter(|track| &track.id == id)
                    .map(|_| expected)
            }) {
                self.session.cursor = cursor;
                self.transition_sequence = snapshot.sequence;
                self.retry_count = 0;
                self.retry_at = None;
                self.failure_seen = false;
                self.ended_seen = false;
                self.seek = None;
            } else {
                self.halt();
                return;
            }
        } else if snapshot.sequence == 0 {
            // Explicit seek starts a fresh playback generation.
            self.transition_sequence = 0;
        }
        let same_track = match &snapshot.state {
            PlaybackState::Idle => false,
            PlaybackState::Loading(id)
            | PlaybackState::Playing(id)
            | PlaybackState::Paused(id)
            | PlaybackState::Ended(id)
            | PlaybackState::Failed(id, _) => self.session.current().is_some_and(|t| &t.id == id),
        };
        if same_track {
            if matches!(snapshot.state, PlaybackState::Playing(_))
                && !snapshot.buffering
                && !snapshot.position.is_zero()
                && let Some(track) = self.session.current().cloned()
                && self.last_history.as_ref() != Some(&(track.id.clone(), snapshot.sequence))
            {
                self.last_history = Some((track.id.clone(), snapshot.sequence));
                self.session.history.retain(|entry| entry.id != track.id);
                self.session.history.insert(0, track);
                self.session.history.truncate(200);
            }
            self.session.position = snapshot.position.as_secs_f64();
            match &snapshot.state {
                PlaybackState::Ended(_) if !self.ended_seen => {
                    self.ended_seen = true;
                    if self.session.continuous {
                        self.next(true);
                    } else {
                        self.halt();
                        self.message = "Finished. Continuous playback is off.".into();
                    }
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
        self.transport_snapshot(ui, snapshot);
    }
    fn transport_snapshot(&mut self, ui: &mut egui::Ui, snapshot: Snapshot) {
        self.mix_status(ui);
        if self.session.current().is_some() {
            let mut seconds = self
                .seek
                .unwrap_or(self.session.position)
                .min(snapshot.duration.as_secs_f64());
            let can_seek = !snapshot.duration.is_zero()
                && matches!(
                    snapshot.state,
                    PlaybackState::Playing(_) | PlaybackState::Paused(_)
                );
            ui.spacing_mut().slider_width = (ui.available_width() - 85.0).max(160.0);
            let slider = ui
                .scope(|ui| {
                    ui.spacing_mut().interact_size.y = 14.0;
                    ui.add_enabled(
                        can_seek,
                        egui::Slider::new(
                            &mut seconds,
                            0.0..=snapshot.duration.as_secs_f64().max(1.0),
                        )
                        .text("Position")
                        .show_value(false),
                    )
                })
                .inner;
            if slider.dragged() {
                self.seek = Some(seconds);
            }
            if slider.drag_stopped() || (slider.changed() && !slider.dragged()) {
                self.player.seek(Duration::from_secs_f64(seconds));
                self.seek = None;
            }
        }
        ui.add_space(8.0);
        if ui.available_width() >= 850.0 && self.session.current().is_some() {
            ui.horizontal(|ui| {
                ui.allocate_ui(egui::vec2(260.0, 54.0), |ui| self.track_identity(ui));
                ui.vertical(|ui| self.transport_controls(ui, &snapshot));
            });
        } else {
            self.track_identity(ui);
            if self.session.current().is_some() {
                self.transport_controls(ui, &snapshot);
            }
        }
        if self.session.current().is_none() {
            return;
        }
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
            PlaybackState::Playing(_) if snapshot.buffering => "Buffering…",
            PlaybackState::Playing(_) => "Playing",
            PlaybackState::Paused(_) => "Paused",
            PlaybackState::Ended(_) => "Finished",
            PlaybackState::Failed(_, _) => "Playback interrupted",
        };
        ui.horizontal_wrapped(|ui| {
            ui.small(state);
            ui.small(format!("{} / {}", time(self.session.position), time(snapshot.duration.as_secs_f64())));
        if let Some(quality) = snapshot.quality {
            ui.weak(quality.to_string()).on_hover_text("Selected unsigned AAC stream. Bitrate is reported by the service; sample rate and channels come from the decoder. Premium quality is not enabled.");
        }
        });
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
    fn track_identity(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if let Some(track) = self.session.current() {
                self.artwork.show(ui, &track.id.0, &track.title, 48.0);
            } else {
                theme::cover(
                    ui,
                    if ui.available_width() < 600.0 {
                        32.0
                    } else {
                        48.0
                    },
                );
            }
            ui.add_space(8.0);
            ui.vertical(|ui| {
                if let Some(track) = self.session.current() {
                    ui.add(
                        egui::Label::new(egui::RichText::new(&track.title).strong().size(18.0))
                            .truncate(),
                    )
                    .on_hover_text(&track.title);
                    ui.weak(track.artists.join(" · "));
                } else {
                    ui.strong("Nothing playing yet");
                    ui.weak("Choose a song to start listening");
                }
            });
        });
    }
    fn transport_controls(&mut self, ui: &mut egui::Ui, snapshot: &Snapshot) {
        ui.horizontal_wrapped(|ui| {
            if theme::transport_button(ui, "⏮", "Previous", true, false).clicked() {
                self.previous();
            }
            let (icon, label) = match snapshot.state {
                PlaybackState::Playing(_) => ("Ⅱ", "Pause"),
                PlaybackState::Loading(_) => ("×", "Cancel"),
                _ => ("▶", "Resume"),
            };
            if theme::transport_button(ui, icon, label, self.loaded, true).clicked() {
                self.toggle();
            }
            if theme::transport_button(ui, "⏭", "Next", true, false).clicked() {
                self.next(false);
            }
            ui.add_space(10.0);
            ui.scope(|ui| {
                ui.spacing_mut().slider_width = 85.0;
                ui.add(
                    egui::Slider::new(&mut self.session.volume, 0.0..=1.0)
                        .text("Volume")
                        .show_value(false),
                );
            });
            ui.menu_button("•••", |ui| {
                if ui.button("Stop").clicked() {
                    self.halt();
                    ui.close();
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
                if ui.button("Restore queue order").clicked() {
                    self.session.queue.restore_upcoming(self.session.cursor + 1);
                    ui.close();
                }
                if ui.button("Shuffle upcoming").clicked() {
                    if self.session.shuffle_random().is_err() {
                        self.message =
                            "Unable to obtain random shuffle. Queue order retained.".into();
                    }
                    ui.close();
                }
            })
            .response
            .on_hover_text("Playback options");
            if ui.button("Now playing").clicked() {
                self.catalog_page = false;
                self.listening_tab = 0;
                self.queue_page = true;
                self.settings_page = false;
                self.account_page = false;
                self.playlists_page = false;
            }
            if ui.button("Focus").clicked() {
                self.focus = true;
            }
            if ui.button("Side panel").clicked() {
                self.session.view.panel_open = !self.session.view.panel_open;
                if ui.available_width() < 850.0 {
                    self.show_compact_panel();
                }
            }
        });
    }
    fn now_playing(&mut self, ui: &mut egui::Ui, lyrics: bool) {
        let Some(track) = self.session.current().cloned() else {
            ui.heading("Nothing playing yet");
            ui.weak("Choose a song from Search or load a local playlist.");
            return;
        };
        let compact = ui.available_width() < 600.0;
        theme::surface(ui, |ui| {
            ui.horizontal_top(|ui| {
                let size = if lyrics && compact {
                    40.0
                } else if lyrics {
                    64.0
                } else if ui.available_width() < 600.0 {
                    112.0
                } else {
                    224.0
                };
                self.artwork.show(ui, &track.id.0, &track.title, size);
                ui.add_space(12.0);
                ui.vertical(|ui| {
                    ui.set_width(ui.available_width());
                    if !lyrics || !compact {
                        ui.weak(if lyrics { "Lyrics for" } else { "Now playing" });
                    }
                    ui.add(
                        egui::Label::new(egui::RichText::new(&track.title).strong().size(
                            if lyrics && compact {
                                17.0
                            } else if lyrics {
                                20.0
                            } else {
                                26.0
                            },
                        ))
                        .wrap(),
                    );
                    if !track.artists.is_empty() {
                        ui.label(track.artists.join(" · "));
                    }
                    if !lyrics {
                        ui.add_space(12.0);
                        let snapshot = self.player.snapshot();
                        ui.weak(match snapshot.state {
                            PlaybackState::Playing(_) if snapshot.buffering => "Buffering…",
                            PlaybackState::Playing(_) => "Playing",
                            PlaybackState::Paused(_) => "Paused",
                            PlaybackState::Loading(_) => "Connecting…",
                            PlaybackState::Failed(_, _) => "Playback interrupted",
                            PlaybackState::Ended(_) => "Finished",
                            PlaybackState::Idle => "Ready to listen",
                        });
                        ui.weak("Playback controls are always available below.");
                    }
                });
            });
        });
    }
    fn navigation(&mut self, ui: &mut egui::Ui) {
        for (label, route) in [
            ("Home", "FEmusic_home"),
            ("Explore", "FEmusic_explore"),
            ("History", "local:history"),
        ] {
            if theme::nav(ui, self.catalog_page && self.catalog.route == route, label).clicked() {
                self.navigate(crate::shell::Destination::Catalog(route.into()));
            }
        }

        if theme::nav(
            ui,
            !self.catalog_page
                && !self.queue_page
                && !self.account_page
                && !self.settings_page
                && !self.playlists_page,
            "Search",
        )
        .clicked()
        {
            self.catalog_page = false;
            self.settings_page = false;
            self.playlists_page = false;
            self.account_page = false;
            self.queue_page = false;
        }
        if theme::nav(
            ui,
            self.queue_page && !self.account_page,
            format!("Queue · {}", self.session.queue.len()),
        )
        .clicked()
        {
            self.catalog_page = false;
            self.settings_page = false;
            self.playlists_page = false;
            self.account_page = false;
            self.queue_page = true;
            self.listening_tab = 1;
        }
        if theme::nav(ui, self.account_page, "Account").clicked() {
            self.catalog_page = false;
            self.settings_page = false;
            self.playlists_page = false;
            self.account_page = true;
        }
        if theme::nav(ui, self.playlists_page, "Playlists").clicked() {
            self.playlists_page = true;
            self.catalog_page = false;
            self.settings_page = false;
            self.account_page = false;
            self.queue_page = false;
        }
        if theme::nav(ui, self.settings_page, "Settings").clicked() {
            self.catalog_page = false;
            self.settings_page = true;
            self.playlists_page = false;
            self.account_page = false;
            self.queue_page = false;
        }
        if ui.available_width() < 240.0 && !self.session.pinned.is_empty() {
            ui.separator();
            ui.weak("Pinned collections");
            let mut open = None;
            egui::ScrollArea::vertical()
                .id_salt("pinned-navigation")
                .max_height(ui.available_height().max(80.0))
                .show(ui, |ui| {
                    for collection in &self.session.pinned {
                        let title: String = collection.title.chars().take(36).collect();
                        if ui
                            .small_button(title)
                            .on_hover_text(&collection.title)
                            .clicked()
                        {
                            open = Some(collection.id.clone());
                        }
                    }
                });
            if let Some(id) = open {
                self.navigate(crate::shell::Destination::Catalog(id));
            }
        }
    }
    fn page_scroll(&mut self, ui: &mut egui::Ui) {
        let page = if self.settings_page {
            4
        } else if self.playlists_page {
            3
        } else if self.account_page {
            2
        } else if self.queue_page {
            10 + self.listening_tab
        } else {
            0
        };
        let listening = !self.catalog_page
            && self.queue_page
            && !self.settings_page
            && !self.playlists_page
            && !self.account_page;
        let mut top = false;
        if listening {
            ui.horizontal_wrapped(|ui| {
                ui.selectable_value(&mut self.listening_tab, 0, "Now playing");
                ui.selectable_value(&mut self.listening_tab, 1, "Up next");
                ui.selectable_value(&mut self.listening_tab, 2, "Lyrics");
                if self.listening_tab == 2 {
                    top = ui.small_button("Back to top").clicked();
                    if top {
                        self.lyrics.manual();
                    }
                    self.lyrics.controls(ui);
                }
            });
            ui.add_space(8.0);
            ui.separator();
        }
        // Each song starts at the beginning; returning to it preserves its reading position.
        let song = (listening && self.listening_tab == 2)
            .then(|| self.session.current().map(|t| t.id.0.clone()))
            .flatten();
        let page = if listening {
            10 + self.listening_tab
        } else {
            page
        };
        let mut scroll = egui::ScrollArea::vertical()
            .id_salt((
                "listening-page",
                page,
                song,
                self.catalog_page.then_some(&self.catalog.route),
            ))
            .auto_shrink([false, false]);
        if top {
            scroll = scroll.vertical_scroll_offset(0.0);
        } else if crate::argument("--capture").is_some()
            && std::env::args().any(|s| s == "--capture-scroll")
            && self.frames >= 40
            && self.lyrics.settled()
        {
            // Deterministic preview position even when the native window lacks pointer focus.
            scroll = scroll.vertical_scroll_offset(520.0);
        }
        scroll.show(ui, |ui| self.content_body(ui));
    }
    fn content(&mut self, ui: &mut egui::Ui) {
        if self.applied_appearance != Some(self.session.appearance) {
            theme::apply(ui.ctx(), self.session.appearance);
            self.applied_appearance = Some(self.session.appearance);
        }

        self.history.record(self.destination());
        if self.focus {
            self.focus_view(ui);
            return;
        }
        ui.horizontal(|ui| {
            if ui
                .add_enabled(self.history.can_back(), egui::Button::new("← Back"))
                .clicked()
                && let Some(target) = self.history.back()
            {
                self.navigate(target);
            }
            if ui
                .add_enabled(self.history.can_forward(), egui::Button::new("Forward →"))
                .clicked()
                && let Some(target) = self.history.forward()
            {
                self.navigate(target);
            }
            ui.checkbox(&mut self.session.view.panel_open, "Listening panel");
        });
        if self.session.view.panel_open && ui.available_width() >= 1100.0 {
            self.companion(ui);
        } else if self.session.view.panel_open && ui.available_width() < 1100.0 {
            ui.horizontal_wrapped(|ui| {
                if ui.small_button("Open queue").clicked() {
                    self.session.view.panel_tab = 0;
                    self.show_compact_panel();
                }
                if ui.small_button("Open lyrics").clicked() {
                    self.session.view.panel_tab = 1;
                    self.show_compact_panel();
                }
            });
        }
        if ui.available_width() >= 850.0 {
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(168.0, ui.available_height()),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.heading("Octave");
                        ui.add_space(24.0);
                        ui.spacing_mut().item_spacing.y = 12.0;
                        self.navigation(ui);
                        ui.add_space(24.0);
                        ui.separator();
                        ui.weak("Your music, on desktop");
                    },
                );
                ui.separator();
                ui.add_space(16.0);
                let width = ui.available_width();
                ui.vertical(|ui| {
                    ui.set_width(width);
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                    self.page_scroll(ui);
                });
            });
        } else {
            ui.label(egui::RichText::new("Octave").strong().size(20.0));
            ui.scope(|ui| {
                ui.spacing_mut().button_padding = egui::vec2(8.0, 6.0);
                ui.spacing_mut().item_spacing.x = 10.0;
                ui.horizontal_wrapped(|ui| self.navigation(ui));
            });
            ui.separator();
            ui.add_space(6.0);
            self.page_scroll(ui);
        }
    }
    fn content_body(&mut self, ui: &mut egui::Ui) {
        if self.applied_appearance != Some(self.session.appearance) {
            theme::apply(ui.ctx(), self.session.appearance);
            self.applied_appearance = Some(self.session.appearance);
        }
        if self.storage_failed {
            ui.colored_label(
                egui::Color32::LIGHT_RED,
                "Local storage is unavailable. Changes may not be saved.",
            );
        }
        if self.settings_page {
            ui.heading("Settings");
            ui.weak("Sound, appearance and desktop preferences.");
            ui.add_space(18.0);
            theme::surface(ui, |ui| {
                ui.collapsing("Optional integrations", |ui| {
                    self.lyrics.translation.settings(ui);
                    self.presence.settings(
                        ui,
                        &mut self.session.view.discord_enabled,
                        &mut self.session.view.discord_id,
                    );
                });
            });

            if let Some(desktop) = &mut self.desktop {
                theme::surface(ui, |ui| {
                    ui.weak("Window and tray");
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
                });
                ui.add_space(12.0);
            }
            if !self.loaded {
                ui.spinner();
                ui.label("Restoring your queue…");
                return;
            }
            let selected_output = theme::surface(ui, |ui| {
                ui.weak("Choose where your music plays");
                egui::CollapsingHeader::new("Audio output")
                    .default_open(crate::argument("--page").as_deref() == Some("output"))
                    .show(ui, |ui| self.output_ui.render(ui))
                    .body_returned
                    .flatten()
            })
            .inner;
            ui.add_space(12.0);
            if let Some(device) = selected_output {
                self.change_output(device);
            }
            theme::surface(ui, |ui| {
                ui.weak("Make Octave feel and sound right for you");
                egui::CollapsingHeader::new("Sound and appearance")
            .default_open(crate::argument("--page").as_deref()==Some("settings"))
            .show(ui, |ui| {
                ui.add_space(8.0);
                ui.checkbox(&mut self.session.view.reduced_motion, "Reduce motion");
                ui.strong("Appearance");
                egui::ComboBox::from_label("Appearance").selected_text(["Dark","Light","High contrast"][usize::from(self.session.appearance.min(2))]).show_ui(ui,|ui| {
                    for (index,label) in ["Dark","Light","High contrast"].into_iter().enumerate() { ui.selectable_value(&mut self.session.appearance,index as u8,label); }
                });
                ui.add_space(12.0);
                ui.separator();
                ui.add_space(12.0);
                ui.checkbox(&mut self.session.view.auto_lyrics,"Load lyrics when the song changes");
                ui.strong("Track transitions");
                ui.checkbox(&mut self.session.continuous,"Continuous playback");
                ui.checkbox(&mut self.session.preload,"Prepare the next song in advance");
                let mut standard=self.crossfade_draft>0;
                egui::ComboBox::from_label("Transition style").selected_text(if standard {"Standard crossfade"}else{"Gapless"}).show_ui(ui,|ui| {
                    if ui.selectable_value(&mut standard,false,"Gapless").clicked(){self.crossfade_draft=0;}
                    if ui.selectable_value(&mut standard,true,"Standard crossfade").clicked(){self.crossfade_draft=self.crossfade_draft.max(1);}
                });
                if standard { ui.add(egui::Slider::new(&mut self.crossfade_draft,1..=octave_player::MAX_CROSSFADE_SECONDS).suffix(" s").text("Blend length")); }
                if ui.button("Apply transition").clicked() {
                    self.halt_at_current_position();
                    self.session.crossfade = self.crossfade_draft.min(5);
                    self.player.set_crossfade(self.session.crossfade);
                    self.message = "Transition saved. Press Resume when ready.".into();
                }
                ui.weak("If the next song is not ready, it starts normally. Songs with different audio formats join without a fade.");
                ui.add_space(12.0);
                ui.separator();
                ui.add_space(12.0);
                ui.strong("Three-band equalizer");
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
                ui.weak("EQ and appearance are saved with your listening settings.");
            });
            });
            if self.applied_appearance != Some(self.session.appearance) {
                theme::apply(ui.ctx(), self.session.appearance);
                self.applied_appearance = Some(self.session.appearance);
            }
            if !self.message.is_empty() {
                ui.weak(&self.message);
            }
            return;
        }
        if !self.loaded {
            ui.spinner();
            ui.label("Restoring your queue…");
            return;
        }
        if self.catalog_page {
            self.catalog_body(ui);
            return;
        }
        if self.playlists_page {
            ui.heading("Your playlists");
            if let Some((tracks, random)) = self.playlist_import.render(ui) {
                self.halt();
                self.session.queue = tracks.into();
                self.session.cursor = 0;
                self.session.position = 0.0;
                if random && self.session.shuffle_all().is_err() {
                    self.message =
                        "Shuffle failed; complete playlist retained in source order.".into();
                }
                self.message = "Complete public playlist loaded. Press Resume to listen.".into();
            }
            ui.add_space(16.0);
            let loaded_playlist = self
                .playlists
                .get_or_insert_with(|| {
                    crate::playlist_ui::PlaylistsUi::new(self.playlists_path.clone())
                })
                .render(ui, &self.session.queue);
            if let Some(tracks) = loaded_playlist {
                self.halt();
                self.session.queue = tracks.into();
                self.session.cursor = 0;
                self.session.position = 0.0;
                self.queue_page = true;
                self.listening_tab = 1;
                self.account_page = false;
                self.message = "Playlist loaded. Press Resume to start listening.".into();
                if self.playlists.as_ref().is_some_and(|p| p.random_on_load)
                    && self.session.shuffle_all().is_err()
                {
                    self.message =
                        "Playlist loaded; random shuffle unavailable. Source order retained."
                            .into();
                }
                self.playlists_page = false;
            }
            return;
        }
        if self.account_page {
            ui.horizontal_wrapped(|ui| {
                if ui.button("Search music").clicked() {
                    self.account_page = false;
                    self.queue_page = false;
                }
                if ui.button("Local playlists").clicked() {
                    self.account_page = false;
                    self.playlists_page = true;
                    self.queue_page = false;
                }
            });
            ui.add_space(12.0);
            let action = egui::ScrollArea::vertical()
                .id_salt("account-page")
                .max_height(ui.clip_rect().height().max(80.0))
                .show(ui, |ui| self.account.render(ui, self.session.current()))
                .inner;
            if let Some(action) = action {
                match action {
                    crate::account_ui::Action::Play(track) => {
                        self.session.queue = vec![track].into();
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
        if self.queue_page && self.listening_tab != 1 {
            self.now_playing(ui, self.listening_tab == 2);
            if self.listening_tab == 2 {
                ui.add_space(if ui.available_width() < 600.0 {
                    8.0
                } else {
                    16.0
                });
                theme::surface(ui, |ui| {
                    self.render_lyrics(ui);
                });
            }
            return;
        }
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
                if ui
                    .add_enabled(
                        self.can_undo_removal(),
                        egui::Button::new("Undo queue removal"),
                    )
                    .clicked()
                {
                    self.undo_removal();
                }
            });
        } else {
            let input = ui
                .scope(|ui| {
                    ui.visuals_mut().widgets.inactive.corner_radius = egui::CornerRadius::same(24);
                    ui.visuals_mut().widgets.hovered.corner_radius = egui::CornerRadius::same(24);
                    ui.visuals_mut().widgets.active.corner_radius = egui::CornerRadius::same(24);
                    ui.add(
                        egui::TextEdit::singleline(&mut self.query)
                            .id(egui::Id::unique("alpha-search"))
                            .hint_text("Search songs and artists")
                            .char_limit(512)
                            .background_color(ui.visuals().widgets.inactive.bg_fill)
                            .margin(egui::vec2(18.0, 12.0))
                            .desired_width(f32::INFINITY),
                    )
                })
                .inner;
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
                if !self.query.is_empty() && ui.button("Clear search").clicked() {
                    self.query.clear();
                    self.results.clear();
                    self.search.invalidate();
                    self.searching = false;
                    self.search_message.clear();
                    self.debounce = None;
                }
                if !self.query.trim().is_empty() && ui.button("Search again").clicked() {
                    self.search.invalidate();
                    self.search.submit(self.query.clone());
                    self.searching = true;
                    self.debounce = None;
                }
                ui.menu_button("Open a video ID", |ui| {
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
            });
            if let Some(track) = self.results.first().cloned() {
                ui.add_space(12.0);
                ui.label(egui::RichText::new("Top result").strong().size(20.0));
                egui::Frame::new()
                    .fill(ui.visuals().window_fill)
                    .corner_radius(12.0)
                    .inner_margin(16.0)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let size = if ui.available_width() > 600.0 {
                                96.0
                            } else {
                                64.0
                            };
                            self.artwork.show(ui, &track.id.0, &track.title, size);
                            ui.add_space(8.0);
                            ui.vertical(|ui| {
                                ui.set_width(ui.available_width());
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(&track.title).strong().size(20.0),
                                    )
                                    .truncate(),
                                )
                                .on_hover_text(&track.title);
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(if track.artists.is_empty() {
                                            "Song".to_owned()
                                        } else {
                                            format!("Song · {}", track.artists.join(" · "))
                                        })
                                        .weak(),
                                    )
                                    .truncate(),
                                );
                                ui.horizontal_wrapped(|ui| {
                                    if theme::primary(ui, "Play top result").clicked() {
                                        self.session.queue = vec![track.clone()].into();
                                        self.session.select(0);
                                        self.start(true);
                                    }
                                    if ui.button("Queue top result").clicked() {
                                        self.add(track.clone(), false);
                                    }
                                });
                            });
                        });
                    });
                ui.add_space(16.0);
                ui.label(
                    egui::RichText::new(format!("Songs · {}", self.results.len()))
                        .strong()
                        .size(22.0),
                );
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Play results").clicked() {
                        self.session.queue = self.results.clone().into();
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
            ui.horizontal(|ui| {
                theme::cover(ui, 80.0);
                ui.add_space(18.0);
                ui.vertical(|ui| {
                    ui.add_space(12.0);
                    ui.label(
                        egui::RichText::new(if self.queue_page {
                            "Your queue is empty"
                        } else {
                            if self.searching {
                                "Searching YouTube Music"
                            } else if !self.query.trim().is_empty() {
                                "No songs to show"
                            } else {
                                "Search your music"
                            }
                        })
                        .size(23.0),
                    );
                    ui.weak(if self.queue_page {
                        "Search for a song and add it to your queue."
                    } else {
                        if self.searching {
                            "Results will appear here."
                        } else if !self.query.trim().is_empty() {
                            "Try another song or artist, or search again."
                        } else {
                            "Find songs and artists on YouTube Music."
                        }
                    });
                    ui.weak("Ctrl+K to search");
                });
            });
        }
        let mut action = None;
        egui::ScrollArea::vertical()
            .id_salt("alpha-tracks")
            .max_height(ui.clip_rect().height().max(80.0))
            .show_rows(ui, 68.0, len, |ui, range| {
                for index in range {
                    let track = if self.queue_page {
                        &self.session.queue[index]
                    } else {
                        &self.results[index]
                    };
                    ui.push_id(
                        (
                            self.session.queue.revision().0,
                            self.session.queue.id_at(index),
                        ),
                        |ui| {
                            let selected = self.queue_page && self.session.cursor == index;
                            let fill = if selected {
                                ui.visuals().selection.bg_fill
                            } else {
                                egui::Color32::TRANSPARENT
                            };
                            egui::Frame::new()
                                .fill(fill)
                                .corner_radius(4.0)
                                .inner_margin(4.0)
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        self.artwork.show(ui, &track.id.0, &track.title, 48.0);
                                        let play = ui
                                            .button("▶")
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
                                                egui::Label::new(
                                                    egui::RichText::new(&track.title).strong(),
                                                )
                                                .truncate(),
                                            )
                                            .on_hover_text(&track.title);
                                            ui.add(
                                                egui::Label::new(
                                                    egui::RichText::new(track.artists.join(" · "))
                                                        .weak(),
                                                )
                                                .truncate(),
                                            );
                                        });
                                        let menu = ui.menu_button("•••", |ui| {
                                            for (label, code) in if self.queue_page {
                                                vec![
                                                    ("Move up", 3),
                                                    ("Move down", 4),
                                                    ("Remove", 5),
                                                    ("Add duplicate", 6),
                                                ]
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
                                });
                            ui.add_space(3.0);
                        },
                    );
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
                    5 => self.remove_queue_entry(index),
                    6 if self.session.queue.len() < crate::listening::MAX_QUEUE => {
                        self.session.queue.push(self.session.queue[index].clone());
                    }
                    _ => (),
                }
            } else {
                let track = self.results[index].clone();
                match code {
                    0 => {
                        self.session.queue = vec![track].into();
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
        self.presence.update(
            self.session.view.discord_enabled,
            &self.session.view.discord_id,
            self.session.current(),
            &self.player.snapshot(),
        );
        let reduce_motion = self.session.view.reduced_motion || self.session.appearance == 2;
        ui.ctx().style_mut_of(
            if self.session.appearance == 1 {
                egui::Theme::Light
            } else {
                egui::Theme::Dark
            },
            |style| {
                style.animation_time = if reduce_motion { 0.0 } else { 0.18 };
                style.scroll_animation = if reduce_motion {
                    egui::style::ScrollAnimation::none()
                } else {
                    egui::style::ScrollAnimation::default()
                };
            },
        );
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
            self.catalog_page = false;
            self.settings_page = false;
            self.playlists_page = false;
            self.account_page = false;
            self.queue_page = false;
        }
        egui::Panel::bottom("listening-player")
            .frame(
                egui::Frame::new()
                    .fill(ui.visuals().window_fill)
                    .inner_margin(egui::Margin::symmetric(24, 8)),
            )
            .show(ui, |ui| self.transport(ui));
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(ui.visuals().panel_fill)
                    .inner_margin(if ui.available_width() < 850.0 {
                        egui::Margin::symmetric(14, 12)
                    } else {
                        egui::Margin::symmetric(28, 20)
                    }),
            )
            .show(ui, |ui| self.content(ui));
        self.frames += 1;
        if std::env::args().any(|s| s == "--smoke-test") && self.loaded && self.frames >= 3 {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if let Some(path) = crate::argument("--capture") {
            if self.loaded
                && self.artwork.settled()
                && self.lyrics.settled()
                && (!self.catalog_page || self.catalog.settled())
                && !self.capture_load_lyrics
            {
                self.capture_ready_at.get_or_insert_with(Instant::now);
            } else {
                self.capture_ready_at = None;
            }
            let image = ui.ctx().data_mut(|data| {
                data.remove_temp::<std::sync::Arc<egui::ColorImage>>(egui::Id::unique(
                    "octave.capture.image",
                ))
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
            if self.loaded
                && self.frames
                    >= if std::env::args().any(|s| s == "--capture-scroll") {
                        64
                    } else {
                        30
                    }
                && self.lyrics.settled()
                && ((self.artwork.settled()
                    && self.playlists.as_ref().is_none_or(|p| p.settled())
                    && !self.searching
                    && self.debounce.is_none())
                    || self.frames >= 240)
                && self
                    .capture_ready_at
                    .is_some_and(|ready| ready.elapsed() >= Duration::from_millis(600))
                && !self.capture_requested
            {
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::Screenshot(
                        egui::ScreenshotCallback::new({
                            let ctx = ui.ctx().clone();
                            move |image| {
                                ctx.data_mut(|data| {
                                    data.insert_temp(
                                        egui::Id::unique("octave.capture.image"),
                                        image,
                                    )
                                });
                                ctx.request_repaint();
                            }
                        }),
                    ));
                self.capture_requested = true;
            }
        }
        let repaint = if crate::argument("--capture").is_some() {
            16
        } else if matches!(self.player.snapshot().state, PlaybackState::Playing(_)) {
            50
        } else if !self.loaded
            || self.searching
            || self.debounce.is_some()
            || !self.lyrics.settled()
        {
            100
        } else {
            500
        };
        ui.ctx()
            .request_repaint_after(Duration::from_millis(repaint));
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
        let mut attempts = 0;
        let bounds = loop {
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
            if bounds.y0 >= 0.0 && bounds.y1 <= 740.0 {
                break bounds;
            }
            attempts += 1;
            assert!(attempts < 12, "Could not scroll to {name}");
            content_frame(
                ctx,
                app,
                vec![
                    egui::Event::PointerMoved(egui::pos2(900.0, 400.0)),
                    egui::Event::MouseWheel {
                        phase: egui::TouchPhase::Move,
                        unit: egui::MouseWheelUnit::Point,
                        delta: egui::vec2(0.0, if bounds.y0 < 0.0 { 180.0 } else { -180.0 }),
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
            for _ in 0..12 {
                content_frame(ctx, app, vec![]);
            }
        };
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
    fn navigation_stays_fixed_while_settings_scroll_and_can_switch_pages() {
        let path = path("fixed-navigation");
        let mut app = loaded(path.clone());
        let ctx = egui::Context::default();
        theme::install(&ctx);
        ctx.enable_accesskit();
        click(&ctx, &mut app, "Settings");
        click(&ctx, &mut app, "Sound and appearance");
        for _ in 0..20 {
            content_frame(&ctx, &mut app, vec![]);
        }
        let bounds = |output: egui::FullOutput, name: &str| {
            output
                .platform_output
                .accesskit_update
                .unwrap()
                .nodes
                .iter()
                .find_map(|(_, node)| {
                    (node.label() == Some(name))
                        .then(|| node.bounds())
                        .flatten()
                })
                .unwrap()
        };
        let nav_before = bounds(content_frame(&ctx, &mut app, vec![]), "Settings");
        let control_before = bounds(content_frame(&ctx, &mut app, vec![]), "Apply transition");
        content_frame(
            &ctx,
            &mut app,
            vec![
                egui::Event::PointerMoved(egui::pos2(900.0, 400.0)),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    phase: egui::TouchPhase::Move,
                    delta: egui::vec2(0.0, -240.0),
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        for _ in 0..20 {
            content_frame(&ctx, &mut app, vec![]);
        }
        let nav_after = bounds(content_frame(&ctx, &mut app, vec![]), "Settings");
        let control_after = bounds(content_frame(&ctx, &mut app, vec![]), "Apply transition");
        assert!((nav_before.y0 - nav_after.y0).abs() < 1.0);
        assert!(control_after.y0 < control_before.y0);
        click(&ctx, &mut app, "Account");
        assert!(app.account_page && !app.settings_page);
        assert_eq!(app.player.snapshot().state, PlaybackState::Idle);
        drop(app);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn history_requires_consumed_audio_and_stays_cleared_for_the_current_play() {
        let path = path("history");
        let mut app = loaded(path.clone());
        app.session.queue = vec![track(1), track(2)].into();
        let mut snapshot = Snapshot {
            state: PlaybackState::Playing(track(1).id),
            ..Default::default()
        };
        app.observe(snapshot.clone());
        assert!(app.session.history.is_empty());
        snapshot.position = Duration::from_secs(1);
        app.observe(snapshot.clone());
        assert_eq!(app.session.history, vec![track(1)]);
        app.session.history.clear();
        app.observe(snapshot);
        assert!(app.session.history.is_empty());
        app.observe(Snapshot {
            sequence: 1,
            state: PlaybackState::Playing(track(2).id),
            position: Duration::from_secs(1),
            ..Default::default()
        });
        assert_eq!(app.session.history, vec![track(2)]);
        app.navigate(crate::shell::Destination::Catalog("local:history".into()));
        let ctx = egui::Context::default();
        theme::install(&ctx);
        content_frame(&ctx, &mut app, vec![]);
        assert!(matches!(app.player.snapshot().state, PlaybackState::Idle));
        drop(app);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn continuous_off_finishes_without_advancing_or_autoplay() {
        let path = path("continuous-off");
        let mut app = loaded(path.clone());
        app.add(track(1), false);
        app.add(track(2), false);
        app.session.continuous = false;
        app.observe(Snapshot {
            state: PlaybackState::Ended(track(1).id),
            position: Duration::from_secs(60),
            duration: Duration::from_secs(60),
            ..Default::default()
        });
        assert_eq!(app.session.cursor, 0);
        assert_eq!(app.session.position, 60.0);
        assert!(matches!(app.player.snapshot().state, PlaybackState::Idle));
        drop(app);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn duplicate_transition_and_removal_undo_preserve_occurrence_and_position() {
        let path = path("queue-occurrences");
        let mut app = loaded(path.clone());
        app.session.queue = vec![track(1), track(1), track(2)].into();
        app.observe(Snapshot {
            sequence: 1,
            state: PlaybackState::Paused(track(1).id),
            position: Duration::from_secs(42),
            duration: Duration::from_secs(200),
            ..Default::default()
        });
        assert_eq!(app.session.cursor, 1);
        let id = app.session.queue.id_at(1);
        app.remove_queue_entry(0);
        assert_eq!(app.session.queue.id_at(0), id);
        app.undo_removal();
        assert_eq!(app.session.queue.id_at(1), id);
        assert_eq!(app.session.position, 42.0);
        assert!(matches!(app.player.snapshot().state, PlaybackState::Idle));
        drop(app);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn focus_and_companion_navigation_preserve_the_listening_session() {
        let path = path("focus-companion");
        let mut app = loaded(path.clone());
        app.add(track(1), false);
        app.session.position = 42.0;
        app.lyrics = crate::lyrics_ui::LyricsUi::reading_fixture(track(1).id);
        let original = app.session.queue.clone();
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        theme::install(&ctx);
        app.focus = true;
        let output = content_frame(&ctx, &mut app, vec![]);
        assert!(
            output
                .platform_output
                .accesskit_update
                .unwrap()
                .nodes
                .iter()
                .any(|(_, node)| node.label() == Some("← Leave Focus"))
        );
        content_frame(
            &ctx,
            &mut app,
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert!(!app.focus);
        app.session.view.panel_open = true;
        app.session.view.panel_tab = 1;
        content_frame(&ctx, &mut app, vec![]);
        assert_eq!(app.session.queue, original);
        assert_eq!(app.session.position, 42.0);
        assert!(matches!(app.player.snapshot().state, PlaybackState::Idle));
        app.session.view.panel_tab = 0;
        app.show_compact_panel();
        assert!(app.queue_page && app.listening_tab == 1);
        app.navigate(crate::shell::Destination::Search);
        assert!(!app.queue_page);
        drop(app);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn lyrics_scroll_keeps_tabs_fixed_and_top_and_song_switch_reset_reading() {
        let path = path("lyrics-scroll");
        let mut app = loaded(path.clone());
        app.add(track(1), false);
        app.session.position = 42.0;
        app.queue_page = true;
        app.listening_tab = 2;
        app.lyrics = crate::lyrics_ui::LyricsUi::reading_fixture(track(1).id);
        let ctx = egui::Context::default();
        theme::install(&ctx);
        ctx.enable_accesskit();
        let bounds = |output: egui::FullOutput, name: &str| {
            output
                .platform_output
                .accesskit_update
                .unwrap()
                .nodes
                .iter()
                .find_map(|(_, node)| {
                    (node.label() == Some(name) || node.value() == Some(name))
                        .then(|| node.bounds())
                        .flatten()
                })
                .unwrap_or_else(|| panic!("Missing {name}"))
        };
        content_frame(&ctx, &mut app, vec![]);
        let tab = bounds(content_frame(&ctx, &mut app, vec![]), "Up next");
        let line = bounds(
            content_frame(&ctx, &mut app, vec![]),
            "Original fixture line 0",
        );
        content_frame(
            &ctx,
            &mut app,
            vec![
                egui::Event::PointerMoved(egui::pos2(900.0, 400.0)),
                egui::Event::MouseWheel {
                    phase: egui::TouchPhase::Move,
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -150.0),
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        for _ in 0..20 {
            content_frame(&ctx, &mut app, vec![]);
        }
        let scrolled = bounds(
            content_frame(&ctx, &mut app, vec![]),
            "Original fixture line 0",
        );
        let fixed = bounds(content_frame(&ctx, &mut app, vec![]), "Up next");
        assert!(scrolled.y0 < line.y0 - 100.0);
        assert!((fixed.y0 - tab.y0).abs() < 1.0);
        click(&ctx, &mut app, "Back to top");
        let reset = bounds(
            content_frame(&ctx, &mut app, vec![]),
            "Original fixture line 0",
        );
        assert!((reset.y0 - line.y0).abs() < 1.0);
        content_frame(
            &ctx,
            &mut app,
            vec![
                egui::Event::PointerMoved(egui::pos2(900.0, 400.0)),
                egui::Event::MouseWheel {
                    phase: egui::TouchPhase::Move,
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -150.0),
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        for _ in 0..20 {
            content_frame(&ctx, &mut app, vec![]);
        }
        app.session.queue.push(track(2));
        app.session.cursor = 1;
        app.lyrics = crate::lyrics_ui::LyricsUi::reading_fixture(track(2).id);
        let next = bounds(
            content_frame(&ctx, &mut app, vec![]),
            "Original fixture line 0",
        );
        assert!((next.y0 - line.y0).abs() < 1.0);
        assert_eq!(app.session.position, 42.0);
        assert_eq!(app.player.snapshot().state, PlaybackState::Idle);
        drop(app);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn listening_tabs_preserve_queue_position_without_starting_playback() {
        let path = path("listening-tabs");
        let mut app = loaded(path.clone());
        app.add(track(1), false);
        app.session.position = 42.0;
        app.queue_page = true;
        let ctx = egui::Context::default();
        theme::install(&ctx);
        ctx.enable_accesskit();
        click(&ctx, &mut app, "Now playing");
        assert_eq!(app.listening_tab, 0);
        click(&ctx, &mut app, "Lyrics");
        assert_eq!(app.listening_tab, 2);
        click(&ctx, &mut app, "Queue · 1");
        assert_eq!(app.listening_tab, 1);
        assert_eq!(app.session.queue, vec![track(1)]);
        assert_eq!(app.session.position, 42.0);
        assert_eq!(app.player.snapshot().state, PlaybackState::Idle);
        drop(app);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn search_results_to_queue_and_clear_work_through_widgets() {
        let path = path("widgets");
        let mut app = loaded(path.clone());
        let ctx = egui::Context::default();
        theme::install(&ctx);
        ctx.enable_accesskit();
        app.results = vec![track(1), track(2)];
        app.query = "test query".into();
        content_frame(&ctx, &mut app, vec![]);
        click(&ctx, &mut app, "Queue top result");
        assert_eq!(app.session.queue, vec![track(1)]);
        assert_eq!(app.player.snapshot().state, PlaybackState::Idle);
        click(&ctx, &mut app, "Add results to queue");
        assert_eq!(app.session.queue.len(), 2);
        click(&ctx, &mut app, "Add results to queue");
        assert_eq!(app.session.queue.len(), 2);
        let generation = app.search.generation;
        app.debounce = Some(Instant::now());
        click(&ctx, &mut app, "Clear search");
        assert!(app.query.is_empty() && app.results.is_empty());
        assert!(app.debounce.is_none() && !app.searching);
        assert!(app.search.generation > generation);
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
        click(&ctx, &mut app, "Settings");
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
        click(&ctx, &mut app, "Settings");
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
    fn transition_updates_queue_once_and_repeated_track_resets_recovery() {
        let path = path("transitions");
        let mut app = loaded(path.clone());
        app.add(track(1), false);
        app.add(track(2), false);
        app.retry_count = 2;
        app.observe(Snapshot {
            buffering: false,
            quality: None,
            sequence: 1,
            state: PlaybackState::Playing(track(2).id),
            position: Duration::from_millis(250),
            duration: Duration::from_secs(180),
        });
        assert_eq!(app.session.cursor, 1);
        assert_eq!(app.retry_count, 0);
        assert_eq!(app.session.position, 0.25);
        app.session.repeat = Repeat::One;
        app.observe(Snapshot {
            buffering: false,
            quality: None,
            sequence: 2,
            state: PlaybackState::Paused(track(2).id),
            position: Duration::from_millis(100),
            duration: Duration::from_secs(180),
        });
        assert_eq!(app.session.cursor, 1);
        assert_eq!(app.session.position, 0.1);
        app.observe(Snapshot {
            buffering: false,
            quality: None,
            sequence: 0,
            state: PlaybackState::Loading(track(2).id),
            position: Duration::from_secs(15),
            duration: Duration::from_secs(180),
        });
        assert_eq!(app.transition_sequence, 0);
        app.crossfade_draft = 3;
        let ctx = egui::Context::default();
        theme::install(&ctx);
        ctx.enable_accesskit();
        content_frame(&ctx, &mut app, vec![]);
        click(&ctx, &mut app, "Settings");
        click(&ctx, &mut app, "Sound and appearance");
        for _ in 0..20 {
            content_frame(&ctx, &mut app, vec![]);
        }
        click(&ctx, &mut app, "Apply transition");
        assert_eq!(app.player.snapshot().state, PlaybackState::Idle);
        assert_eq!(app.session.crossfade, 3);
        drop(app);
        let restored = loaded(path.clone());
        assert_eq!(restored.session.crossfade, 3);
        assert_eq!(restored.player.snapshot().state, PlaybackState::Idle);
        drop(restored);
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
            buffering: false,
            quality: None,
            sequence: 0,
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
            buffering: false,
            quality: None,
            sequence: 0,
            state: PlaybackState::Failed(track(2).id, Failure::Provider(Error::Timeout)),
            position: Duration::ZERO,
            duration: Duration::ZERO,
        });
        assert_eq!(
            app.session.position, 42.0,
            "Stale track events must be ignored"
        );
        for error in [Error::SignInRequired, Error::BotCheckRequired] {
            app.retry_count = 0;
            app.failure_seen = false;
            app.observe(Snapshot {
                buffering: false,
                quality: None,
                sequence: 0,
                state: PlaybackState::Failed(track(1).id, Failure::Provider(error)),
                position: Duration::from_secs(42),
                duration: Duration::ZERO,
            });
            assert!(app.retry_at.is_none());
            assert_eq!(app.retry_count, 0);
            assert_eq!(app.session.position, 42.0);
            assert_eq!(app.session.queue, queue);
        }
        drop(app);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn listening_layout_and_text_input_work_at_narrow_sizes() {
        let path = path("layout");
        let mut app = loaded(path.clone());
        app.add(track(1), false);
        let ctx = egui::Context::default();
        theme::install(&ctx);
        ctx.enable_accesskit();
        for size in [[1180.0, 760.0], [560.0, 720.0], [480.0, 480.0]] {
            // Auto-sized panels settle after a viewport resize.
            for pass in 0..3 {
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(size[0], size[1]),
                        )),
                        ..Default::default()
                    },
                    |ui| {
                        egui::Panel::bottom("test-player").show(ui, |ui| {
                            app.transport_snapshot(
                                ui,
                                Snapshot {
                                    quality: Some(octave_core::AudioQuality {
                                        bitrate_bps: Some(129502),
                                        sample_rate: 44100,
                                        channels: 2,
                                    }),
                                    state: PlaybackState::Playing(track(1).id),
                                    duration: Duration::from_secs(213),
                                    ..Default::default()
                                },
                            )
                        });
                        egui::CentralPanel::default().show(ui, |ui| app.content(ui));
                    },
                );
                output.textures_delta.clear();
                if pass < 2 {
                    continue;
                }
                assert!(!output.shapes.is_empty());
                let tree = output.platform_output.accesskit_update.unwrap();
                for label in ["Previous", "Pause", "Next"] {
                    let node = tree
                        .nodes
                        .iter()
                        .find_map(|(_, node)| (node.label() == Some(label)).then_some(node))
                        .unwrap_or_else(|| panic!("Missing accessible transport: {label}"));
                    let bounds = node.bounds().unwrap();
                    assert!(
                        bounds.x0 >= 0.0
                            && bounds.y0 >= 0.0
                            && bounds.x1 <= f64::from(size[0])
                            && bounds.y1 <= f64::from(size[1]),
                        "Transport {label} is outside {size:?}: {bounds:?}"
                    );
                }
                assert!(tree.nodes.iter().any(|(_, node)| {
                    node.value()
                        .is_some_and(|label| label.contains("129 kb/s reported"))
                }));
            }
        }
        ctx.memory_mut(|m| m.request_focus(egui::Id::unique("alpha-search")));
        let old = app.search.generation;
        let mut output = ctx.run_ui(
            egui::RawInput {
                events: vec![egui::Event::Text("東京 مرحبا שלום 123 ".into())],
                ..Default::default()
            },
            |ui| app.content(ui),
        );
        output.textures_delta.clear();
        assert_eq!(app.query, "東京 مرحبا שלום 123 ");
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
