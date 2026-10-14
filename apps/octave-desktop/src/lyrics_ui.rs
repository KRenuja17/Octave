use eframe::egui;
use octave_core::TrackId;
use octave_provider::{Error, Provider, lyrics::Lyrics};
use std::sync::mpsc::{self, Receiver};
type Response = (TrackId, Result<Option<Lyrics>, Error>);
#[derive(Default)]
pub struct LyricsUi {
    id: Option<TrackId>,
    response: Option<Receiver<Response>>,
    lyrics: Option<Lyrics>,
    message: String,
}
impl LyricsUi {
    pub fn render(&mut self, ui: &mut egui::Ui, id: Option<&TrackId>) {
        if self.id.as_ref() != id {
            self.id = id.cloned();
            self.lyrics = None;
            self.message.clear();
        }
        if let Some(response) = &self.response
            && let Ok((id, result)) = response.try_recv()
        {
            if self.id.as_ref() == Some(&id) {
                match result {
                    Ok(lyrics) => {
                        self.lyrics = lyrics;
                        self.message = if self.lyrics.is_none() {
                            "No lyrics are available for this song.".into()
                        } else {
                            String::new()
                        };
                    }
                    Err(error) => self.message = format!("Lyrics unavailable: {error}"),
                }
            }
            self.response = None;
        }
        ui.collapsing("Lyrics", |ui| {
            ui.weak("Optional text from YouTube Music. Lyrics requests never control playback.");
            if ui
                .add_enabled(
                    self.id.is_some() && self.response.is_none(),
                    egui::Button::new("Load lyrics"),
                )
                .clicked()
            {
                let id = self.id.clone().unwrap();
                let (tx, rx) = mpsc::sync_channel(1);
                self.response = Some(rx);
                std::thread::spawn(move || {
                    let result = Provider::new().and_then(|p| p.lyrics(&id));
                    let _ = tx.send((id, result));
                });
            }
            if self.response.is_some() {
                ui.spinner();
            }
            ui.label(&self.message);
            if let Some(lyrics) = &self.lyrics {
                egui::ScrollArea::vertical()
                    .id_salt("live-lyrics")
                    .max_height(180.0)
                    .show(ui, |ui| {
                        ui.label(&lyrics.text);
                        ui.weak(&lyrics.source);
                    });
            }
        });
    }
}
