use eframe::egui;
use octave_core::{Track, TrackId};
use octave_provider::{Error, Provider};
use std::sync::mpsc::{self, Receiver};
#[derive(Default)]
pub struct RelatedUi {
    cancel: crate::request_cancel::RequestCancel,
    id: Option<TrackId>,
    response: Option<Receiver<Result<Vec<Track>, Error>>>,
    tracks: Vec<Track>,
    message: String,
    cache: std::collections::VecDeque<(TrackId, Vec<Track>)>,
}
impl RelatedUi {
    pub fn cancel_for(&mut self, id: Option<&TrackId>) {
        if self.id.as_ref() != id {
            self.cancel.cancel();
            self.response = None;
        }
    }

    pub fn render(&mut self, ui: &mut egui::Ui, id: Option<&TrackId>) -> Option<Track> {
        self.cancel_for(id);
        if self.id.as_ref() != id {
            self.id = id.cloned();
            self.response = None;
            self.tracks = self
                .cache
                .iter()
                .find(|(cached, _)| Some(cached) == id)
                .map(|(_, tracks)| tracks.clone())
                .unwrap_or_default();
            self.message.clear();
        }
        if let Some(result) = self.response.as_ref().and_then(|rx| match rx.try_recv() {
            Ok(value) => Some(value),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err(Error::Network)),
        }) {
            match result {
                Ok(tracks) => {
                    if let Some(id) = self.id.clone() {
                        self.cache.retain(|(cached, _)| cached != &id);
                        if self.cache.len() == 8 {
                            self.cache.pop_front();
                        }
                        self.cache.push_back((id, tracks.clone()));
                    }
                    self.tracks = tracks;
                    self.message = if self.tracks.is_empty() {
                        "No related songs available.".into()
                    } else {
                        String::new()
                    };
                }
                Err(_) => self.message = "Related songs unavailable. Retry when connected.".into(),
            }
            self.response = None;
        }
        ui.heading("Related songs");
        if ui
            .add_enabled(
                self.id.is_some() && self.response.is_none(),
                egui::Button::new("Load related songs"),
            )
            .clicked()
            && let Some(id) = self.id.clone()
        {
            let cancel = self.cancel.begin();
            let (tx, rx) = mpsc::sync_channel(1);
            self.response = Some(rx);
            std::thread::spawn(move || {
                let _ =
                    tx.send(Provider::new().and_then(|p| p.with_cancellation(cancel).related(&id)));
            });
        }
        if self.response.is_some() {
            ui.spinner();
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }
        ui.weak(&self.message);
        let mut selected = None;
        egui::ScrollArea::vertical()
            .id_salt("related-songs")
            .show_rows(ui, 64.0, self.tracks.len(), |ui, range| {
                for index in range {
                    let track = &self.tracks[index];
                    ui.push_id(index, |ui| {
                        ui.strong(&track.title);
                        ui.weak(track.artists.join(" · "));
                        if ui.small_button("Add to queue").clicked() {
                            selected = Some(track.clone());
                        }
                    });
                }
            });
        selected
    }
}
