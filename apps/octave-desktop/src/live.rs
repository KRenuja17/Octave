use eframe::egui;
use octave_core::{Track, TrackId, diagnostics::Event};
use octave_player::{PlaybackState, Player};
use octave_provider::Provider;
use std::{
    sync::mpsc::{self, Receiver},
    time::Duration,
};
pub fn run() -> eframe::Result {
    let smoke_test = std::env::args().any(|arg| arg == "--smoke-test");
    eprintln!("{}", Event::ApplicationStarted.as_json());
    let result = eframe::run_native(
        "Octave",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([960.0, 640.0])
                .with_min_inner_size([480.0, 320.0]),
            ..Default::default()
        },
        Box::new(move |_| {
            Ok(Box::new(Octave {
                smoke_test,
                frames: 0,
                player: Player::new(),
                query: String::new(),
                video_id: String::new(),
                results: Vec::new(),
                search: None,
                message: String::new(),
                selected: None,
                seek_seconds: None,
            }))
        }),
    );
    eprintln!("{}", Event::ApplicationStopped.as_json());
    result
}
struct Octave {
    smoke_test: bool,
    frames: u32,
    player: Player,
    query: String,
    video_id: String,
    results: Vec<Track>,
    search: Option<Receiver<Result<Vec<Track>, octave_provider::Error>>>,
    message: String,
    selected: Option<Track>,
    seek_seconds: Option<f64>,
}
impl Octave {
    fn search(&mut self) {
        let query = self.query.clone();
        let (tx, rx) = mpsc::sync_channel(1);
        self.search = Some(rx);
        self.message = "Searching…".into();
        std::thread::spawn(move || {
            let result = Provider::new().and_then(|p| p.search(&query));
            let _ = tx.send(result);
        });
    }
    fn play(&mut self, track: Track) {
        self.player.play(track.id.clone());
        self.selected = Some(track);
        self.seek_seconds = None;
    }
}
impl eframe::App for Octave {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if let Some(rx) = &self.search {
            match rx.try_recv() {
                Ok(result) => {
                    match result {
                        Ok(tracks) => {
                            self.message = format!("{} results", tracks.len());
                            self.results = tracks;
                        }
                        Err(e) => self.message = e.to_string(),
                    };
                    self.search = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.message = "Search worker stopped. Try again.".into();
                    self.search = None;
                }
                Err(mpsc::TryRecvError::Empty) => (),
            }
        }
        let snapshot = self.player.snapshot();
        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("Octave");
            ui.label("Playback feasibility preview · unsigned access · direct AAC only");
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                let input = ui.add(
                    egui::TextEdit::singleline(&mut self.query)
                        .hint_text("Search YouTube Music")
                        .char_limit(512),
                );
                let enter = input.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if ui
                    .add_enabled(
                        self.search.is_none() && !self.query.trim().is_empty(),
                        egui::Button::new("Search"),
                    )
                    .clicked()
                    || (enter && self.search.is_none() && !self.query.trim().is_empty())
                {
                    self.search();
                }
            });
            ui.label(&self.message);
            ui.collapsing("Play a video ID", |ui| {
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.video_id)
                            .hint_text("11-character video ID")
                            .char_limit(11),
                    );
                    if ui.button("Play ID").clicked() {
                        self.play(Track {
                            id: TrackId(self.video_id.trim().to_owned()),
                            title: self.video_id.clone(),
                            artists: Vec::new(),
                        });
                    }
                });
            });
            ui.separator();
            if let Some(track) = &self.selected {
                ui.strong(&track.title);
            }
            let text = match &snapshot.state {
                PlaybackState::Idle => "Choose a track to test playback.".to_owned(),
                PlaybackState::Loading(_) => "Resolving and buffering…".into(),
                PlaybackState::Playing(_) => "Playing".into(),
                PlaybackState::Paused(_) => "Paused".into(),
                PlaybackState::Ended(_) => "Finished".into(),
                PlaybackState::Failed(_, error) => error.to_string(),
            };
            ui.label(text);
            ui.horizontal(|ui| {
                let active = matches!(
                    snapshot.state,
                    PlaybackState::Playing(_) | PlaybackState::Paused(_)
                );
                let label = if matches!(snapshot.state, PlaybackState::Paused(_)) {
                    "Resume"
                } else {
                    "Pause"
                };
                if ui.add_enabled(active, egui::Button::new(label)).clicked() {
                    self.player.toggle_pause();
                }
                if ui.button("Stop").clicked() {
                    self.player.stop();
                }
                if ui
                    .add_enabled(self.selected.is_some(), egui::Button::new("Retry"))
                    .clicked()
                {
                    self.play(self.selected.clone().unwrap());
                }
                ui.label(format!(
                    "{}:{:02} / {}:{:02}",
                    snapshot.position.as_secs() / 60,
                    snapshot.position.as_secs() % 60,
                    snapshot.duration.as_secs() / 60,
                    snapshot.duration.as_secs() % 60
                ));
            });
            if !snapshot.duration.is_zero() {
                let mut seconds = self.seek_seconds.unwrap_or(snapshot.position.as_secs_f64());
                let slider = ui.add(
                    egui::Slider::new(&mut seconds, 0.0..=snapshot.duration.as_secs_f64())
                        .show_value(false),
                );
                if slider.dragged() {
                    self.seek_seconds = Some(seconds);
                }
                if slider.drag_stopped() || (slider.changed() && !slider.dragged()) {
                    self.player.seek(Duration::from_secs_f64(seconds));
                    self.seek_seconds = None;
                }
            }
            ui.separator();
            let mut chosen = None;
            egui::ScrollArea::vertical().show(ui, |ui| {
                for track in &self.results {
                    ui.horizontal(|ui| {
                        if ui.button("Play").clicked() {
                            chosen = Some(track.clone());
                        }
                        ui.vertical(|ui| {
                            ui.label(&track.title);
                            if !track.artists.is_empty() {
                                ui.weak(track.artists.join(", "));
                            }
                        });
                    });
                    ui.add_space(4.0);
                }
            });
            if let Some(track) = chosen {
                self.play(track);
            }
        });
        self.frames = self.frames.saturating_add(1);
        if self.frames == 1 {
            eprintln!("{}", Event::WindowReady.as_json());
        }
        if self.smoke_test && self.frames >= 3 {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ui.ctx().request_repaint_after(Duration::from_millis(50));
    }
}
