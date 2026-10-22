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
    pub fn settled(&self) -> bool {
        self.response.is_none()
    }

    /// Explicit user action shared by the button and opt-in capture verification.
    pub fn load_current(&mut self) {
        let Some(id) = self.id.clone().filter(|_| self.response.is_none()) else {
            return;
        };
        self.message.clear();
        let (tx, rx) = mpsc::sync_channel(1);
        self.response = Some(rx);
        std::thread::spawn(move || {
            let result = Provider::new().and_then(|p| p.lyrics(&id));
            let _ = tx.send((id, result));
        });
    }

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
                    Err(error) => self.message = lyrics_error(error).into(),
                }
            }
            self.response = None;
        }
        ui.horizontal_wrapped(|ui| {
            ui.heading("Lyrics");
            let label = if !self.message.is_empty() {
                "Retry lyrics"
            } else if self.lyrics.is_some() {
                "Reload lyrics"
            } else {
                "Load lyrics"
            };
            if ui
                .add_enabled(
                    self.id.is_some() && self.settled(),
                    egui::Button::new(label),
                )
                .clicked()
            {
                self.load_current();
            }
        });
        ui.weak("Text from YouTube Music · not time-synced");
        if let Some(lyrics) = &self.lyrics
            && !lyrics.source.trim().is_empty()
        {
            ui.weak(&lyrics.source);
        }
        ui.add_space(16.0);
        if self.id.is_none() {
            ui.label("Choose a song to view its lyrics.");
        } else if self.lyrics.is_none() && self.settled() && self.message.is_empty() {
            ui.label("Load lyrics for this song. Your music keeps its place.");
        }
        if !self.settled() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.weak(if self.lyrics.is_some() {
                    "Refreshing lyrics…"
                } else {
                    "Loading lyrics…"
                });
            });
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }
        if !self.message.is_empty() {
            ui.label(&self.message);
        }
        if let Some(lyrics) = &self.lyrics {
            // The page owns scrolling, keeping artwork, attribution and lyrics in one flow.
            ui.scope(|ui| {
                ui.set_max_width(ui.available_width().min(720.0));
                let size = if ui.available_width() < 600.0 {
                    22.0
                } else {
                    26.0
                };
                for line in lyrics.text.lines() {
                    if line.trim().is_empty() {
                        ui.add_space(18.0);
                    } else {
                        ui.add(egui::Label::new(egui::RichText::new(line).size(size)).wrap());
                        ui.add_space(5.0);
                    }
                }
                ui.add_space(16.0);
            });
        }
    }
}

fn lyrics_error(error: Error) -> &'static str {
    match error {
        Error::Timeout => "Lyrics unavailable: the request timed out. Try again.",
        Error::Network => "Lyrics unavailable: check your connection and try again.",
        Error::Http(401 | 403) | Error::SignInRequired => {
            "Lyrics unavailable for this unsigned request. Try another song."
        }
        _ => "Lyrics unavailable right now. Try again or choose another song.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "Opt-in unsigned network request; no audio or account access"]
    fn live_unsigned_lyrics_reach_the_panel() {
        let id = TrackId("dQw4w9WgXcQ".into());
        let mut app = LyricsUi::default();
        frame(&mut app, &id);
        app.load_current();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while !app.settled() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(50));
            frame(&mut app, &id);
        }
        assert!(app.settled(), "Lyrics worker did not finish");
        let lyrics = app.lyrics.as_ref().expect("Live song should have lyrics");
        assert!(!lyrics.text.trim().is_empty());
        assert!(!lyrics.source.trim().is_empty());
        assert!(app.message.is_empty());
        println!(
            "live_lyrics_rendered=true characters={} source_present=true",
            lyrics.text.chars().count()
        );
    }

    #[test]
    fn failed_refresh_preserves_readable_lyrics_and_uses_lyrics_guidance() {
        let id = TrackId("aaaaaaaaaaa".into());
        let mut app = LyricsUi::default();
        frame(&mut app, &id);
        let original = Lyrics {
            text: "Original test verse\n\nSecond stanza".into(),
            source: "Test source".into(),
        };
        app.lyrics = Some(original.clone());
        let (tx, rx) = mpsc::sync_channel(1);
        app.response = Some(rx);
        tx.send((id.clone(), Err(Error::Timeout))).unwrap();
        frame(&mut app, &id);
        assert_eq!(app.lyrics, Some(original));
        assert!(app.settled() && app.message.contains("timed out"));
        assert!(!app.message.contains("playback"));
        assert!(!lyrics_error(Error::Http(403)).contains("stream"));
    }

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
