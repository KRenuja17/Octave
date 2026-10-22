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
            self.response = None;
            self.message.clear();
        }
        let result = self
            .response
            .as_ref()
            .and_then(|response| match response.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.id.clone().map(|id| (id, Err(Error::Network)))
                }
                Err(mpsc::TryRecvError::Empty) => None,
            });
        if let Some((id, result)) = result {
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
        ui.heading("Lyrics");
        {
            ui.weak("Text from YouTube Music · not time-synced");
            ui.add_space(12.0);
            if self.id.is_none() {
                ui.label("Choose a song to view its lyrics.");
            } else if self.lyrics.is_none() && self.response.is_none() && self.message.is_empty() {
                ui.label("Load lyrics for this song. Your music keeps its place.");
            }
            if ui
                .add_enabled(
                    self.id.is_some() && self.response.is_none(),
                    egui::Button::new(if self.message.is_empty() {
                        "Load lyrics"
                    } else {
                        "Retry lyrics"
                    }),
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
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.weak("Loading lyrics…");
                });
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(100));
            }
            if !self.message.is_empty() {
                ui.label(&self.message);
            }
            if let Some(lyrics) = &self.lyrics {
                egui::ScrollArea::vertical()
                    .id_salt(("live-lyrics", self.id.as_ref().map(|id| &id.0)))
                    .max_height(420.0)
                    .show(ui, |ui| {
                        ui.add_space(12.0);
                        ui.label(egui::RichText::new(&lyrics.text).size(22.0));
                        ui.add_space(20.0);
                        ui.weak(&lyrics.source);
                    });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn frame(app: &mut LyricsUi, id: &TrackId) {
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| app.render(ui, Some(id)));
        output.textures_delta.clear();
    }
    #[test]
    fn track_changes_discard_old_lyrics_and_pending_responses() {
        let old = TrackId("aaaaaaaaaaa".into());
        let next = TrackId("bbbbbbbbbbb".into());
        let mut app = LyricsUi::default();
        frame(&mut app, &old);
        let (tx, rx) = mpsc::sync_channel(1);
        app.response = Some(rx);
        tx.send((
            old,
            Ok(Some(Lyrics {
                text: "Original test verse".into(),
                source: "Test fixture".into(),
            })),
        ))
        .unwrap();
        frame(&mut app, &next);
        assert!(app.lyrics.is_none() && app.response.is_none() && app.message.is_empty());
        assert_eq!(app.id, Some(next));
    }
    #[test]
    fn missing_lyrics_and_disconnected_worker_leave_retryable_state() {
        let id = TrackId("aaaaaaaaaaa".into());
        let mut app = LyricsUi::default();
        frame(&mut app, &id);
        let (tx, rx) = mpsc::sync_channel(1);
        app.response = Some(rx);
        tx.send((id.clone(), Ok(None))).unwrap();
        frame(&mut app, &id);
        assert!(app.message.contains("No lyrics") && app.response.is_none());
        let (tx, rx) = mpsc::sync_channel(1);
        app.response = Some(rx);
        drop(tx);
        frame(&mut app, &id);
        assert!(app.message.contains("Lyrics unavailable") && app.response.is_none());
    }
}
