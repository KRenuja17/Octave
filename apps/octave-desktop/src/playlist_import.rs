//! A complete import is atomic; cancellation/failure never exposes a partial queue.
use eframe::egui;
use octave_core::Track;
use octave_provider::{Error, Provider, playlist::CompletePlaylist};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver},
};
enum Event {
    Progress(usize, usize),
    Complete(Result<CompletePlaylist, Error>),
}
#[derive(Default)]
pub struct PlaylistImport {
    input: String,
    random: bool,
    rx: Option<Receiver<Event>>,
    cancel: Arc<AtomicBool>,
    message: String,
}
impl Drop for PlaylistImport {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
    }
}
impl PlaylistImport {
    pub fn render(&mut self, ui: &mut egui::Ui) -> Option<(Vec<Track>, bool)> {
        let mut complete = None;
        while let Some(event) = self.rx.as_ref().and_then(|rx| rx.try_recv().ok()) {
            match event {
                Event::Progress(count, skipped) => {
                    self.message = format!(
                        "Loaded {count} playable · {skipped} unavailable · fetching remaining pages…"
                    )
                }
                Event::Complete(result) => {
                    self.rx = None;
                    match result {
                        Ok(data)=>{ self.message=format!("Complete: {} playable · {} unavailable · {} pages",data.tracks.len(),data.skipped,data.pages); complete=Some((data.tracks,self.random)); },
                        Err(_)=>self.message="Could not load the complete public playlist (limit 10,000 entries). Queue unchanged; retry or choose another playlist.".into(),
                    }
                }
            }
        }
        egui::CollapsingHeader::new("Import a complete public playlist").show(ui, |ui| {
            ui.weak(
                "All pages load before replacing your queue. Private playlists require an account.",
            );
            ui.add(
                egui::TextEdit::singleline(&mut self.input)
                    .hint_text("Playlist URL or PL… / OLAK… ID")
                    .char_limit(512),
            );
            ui.add_enabled_ui(self.rx.is_none(), |ui| {
                ui.checkbox(&mut self.random, "True random · entire playlist");
            });
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(
                        self.rx.is_none()
                            && octave_provider::playlist::playlist_id(&self.input).is_some(),
                        egui::Button::new("Load complete playlist into queue"),
                    )
                    .clicked()
                {
                    let input = self.input.clone();
                    let cancel = Arc::new(AtomicBool::new(false));
                    self.cancel = cancel.clone();
                    let (tx, rx) = mpsc::channel();
                    self.rx = Some(rx);
                    self.message = "Loading public playlist…".into();
                    std::thread::spawn(move || {
                        let result = Provider::new().and_then(|p| {
                            p.complete_playlist(
                                &input,
                                || cancel.load(Ordering::Acquire),
                                |count, skipped| {
                                    let _ = tx.send(Event::Progress(count, skipped));
                                },
                            )
                        });
                        let _ = tx.send(Event::Complete(result));
                    });
                }
                if self.rx.is_some() && ui.button("Cancel import").clicked() {
                    self.cancel.store(true, Ordering::Release);
                    self.rx = None;
                    self.message = "Import cancelled. Queue unchanged.".into();
                }
            });
            ui.weak(&self.message);
        });
        if self.rx.is_some() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }
        complete
    }
}
