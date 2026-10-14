//! Account work is serialized; logout revokes all shared session handles.
use eframe::egui;
use octave_core::Track;
use octave_provider::account::{Account, Error, Page};
use std::{
    collections::{HashMap, HashSet},
    sync::mpsc::{self, Receiver},
};
use zeroize::Zeroize;
enum ResultMessage {
    Page(Result<Page, Error>, bool),
    Rated(Track, bool, Result<(), Error>),
}
pub enum Action {
    Play(Track),
    Append(Track),
}
pub struct AccountUi {
    account: Option<Account>,
    cookie: String,
    index: u8,
    pending: Option<Receiver<ResultMessage>>,
    page: Page,
    browse_id: String,
    pub message: String,
    verified: bool,
    seen: HashSet<String>,
    ratings: HashMap<String, bool>,
    generation: u64,
    pending_generation: u64,
}
impl Default for AccountUi {
    fn default() -> Self {
        Self {
            account: None,
            cookie: String::new(),
            index: 0,
            pending: None,
            page: Page::default(),
            browse_id: "VLLM".into(),
            message: String::new(),
            verified: false,
            seen: HashSet::new(),
            ratings: HashMap::new(),
            generation: 0,
            pending_generation: 0,
        }
    }
}
impl Drop for AccountUi {
    fn drop(&mut self) {
        self.disconnect();
    }
}
impl AccountUi {
    fn disconnect(&mut self) {
        if let Some(account) = self.account.take() {
            account.disconnect();
        }
        self.cookie.zeroize();
        self.page = Page::default();
        self.ratings.clear();
        self.seen.clear();
        self.verified = false;
        self.generation += 1;
        self.message =
            "Disconnected. Octave has discarded the session. Your browser remains signed in."
                .into();
    }
    fn browse(&mut self, id: String, more: bool) {
        if self.pending.is_some() {
            return;
        }
        let Some(account) = self.account.clone() else {
            return;
        };
        let token = if more {
            self.page.continuation.clone()
        } else {
            None
        };
        if more && token.is_none() {
            return;
        }
        if !more {
            self.page = Page::default();
            self.seen.clear();
        }
        if let Some(token) = &token
            && !self.seen.insert(token.clone())
        {
            self.page.continuation = None;
            self.message = "Pagination stopped because the server repeated a page token.".into();
            return;
        }
        self.browse_id = id.clone();
        self.pending_generation = self.generation;
        let (tx, rx) = mpsc::sync_channel(1);
        self.pending = Some(rx);
        self.message = "Loading account library…".into();
        std::thread::spawn(move || {
            let _ = tx.send(ResultMessage::Page(
                account.browse(&id, token.as_deref()),
                more,
            ));
        });
    }
    fn rate(&mut self, track: Track, liked: bool) {
        if self.pending.is_some() {
            return;
        }
        let Some(account) = self.account.clone() else {
            return;
        };
        self.pending_generation = self.generation;
        let (tx, rx) = mpsc::sync_channel(1);
        self.pending = Some(rx);
        self.message = "Waiting for YouTube to confirm the change…".into();
        std::thread::spawn(move || {
            let result = account.rate(&track.id, liked);
            let _ = tx.send(ResultMessage::Rated(track, liked, result));
        });
    }
    pub fn poll(&mut self) {
        let result = self.pending.as_ref().and_then(|rx| match rx.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Disconnected) => {
                Some(ResultMessage::Page(Err(Error::Network), false))
            }
            Err(_) => None,
        });
        let Some(result) = result else {
            return;
        };
        self.pending = None;
        if self.generation != self.pending_generation {
            return;
        }
        let error = match result {
            ResultMessage::Page(Ok(page), more) => {
                if !more {
                    self.page = Page::default();
                }
                for track in page.tracks {
                    if !self.page.tracks.iter().any(|t| t.id == track.id) {
                        if self.browse_id == "VLLM" {
                            self.ratings.insert(track.id.0.clone(), true);
                        }
                        self.page.tracks.push(track);
                    }
                }
                for collection in page.collections {
                    if !self.page.collections.iter().any(|c| c.id == collection.id) {
                        self.page.collections.push(collection);
                    }
                }
                self.page.continuation = page.continuation;
                self.verified = true;
                self.message = format!(
                    "{} songs · {} collections",
                    self.page.tracks.len(),
                    self.page.collections.len()
                );
                None
            }
            ResultMessage::Rated(track, liked, Ok(())) => {
                self.ratings.insert(track.id.0.clone(), liked);
                if !liked && self.browse_id == "VLLM" {
                    self.page.tracks.retain(|t| t.id != track.id);
                }
                self.message = "YouTube confirmed the change.".into();
                None
            }
            ResultMessage::Page(Err(error), more) => {
                if more && let Some(token) = &self.page.continuation {
                    self.seen.remove(token);
                }
                Some(error)
            }
            ResultMessage::Rated(_, _, Err(error)) => Some(error),
        };
        if let Some(error) = error {
            if error == Error::Expired {
                self.disconnect();
            }
            self.message = error.to_string();
        }
    }
    pub fn render(&mut self, ui: &mut egui::Ui, current: Option<&Track>) -> Option<Action> {
        let mut action = None;
        ui.heading("Your music");
        ui.weak("Account library preview · playback still uses the unsigned resolver.");
        if self.account.is_none() {
            ui.label("Connect an existing YouTube Music browser session. Octave never asks for your Google password and keeps the session in memory only.");
            ui.hyperlink_to("Open YouTube Music to sign in", "https://music.youtube.com");
            ui.collapsing("How to connect", |ui| {
                ui.label("In your browser's developer tools, open Network and visit your Music library. Select a POST request to music.youtube.com/youtubei/v1/browse. Copy only the Cookie request-header value into the masked field below. Set the account index to the request's X-Goog-AuthUser value (usually 0). Do not paste these values into chat or save them in a file.");
                ui.label("This is an advanced beta connection method. Close/Quit or Disconnect forgets it; connecting again requires a fresh paste. Clear your clipboard after connecting.");
            });
            ui.add(
                egui::TextEdit::singleline(&mut self.cookie)
                    .password(true)
                    .hint_text("Cookie header value")
                    .char_limit(16384)
                    .desired_width(f32::INFINITY),
            );
            ui.add(
                egui::DragValue::new(&mut self.index)
                    .range(0..=99)
                    .prefix("Account index: "),
            );
            if ui
                .add_enabled(
                    self.pending.is_none() && !self.cookie.is_empty(),
                    egui::Button::new("Connect session"),
                )
                .clicked()
            {
                match Account::new(std::mem::take(&mut self.cookie), self.index) {
                    Ok(account) => {
                        self.account = Some(account);
                        self.browse("VLLM".into(), false);
                    }
                    Err(error) => self.message = error.to_string(),
                }
            }
        } else {
            ui.horizontal_wrapped(|ui| {
                ui.label(if self.verified {
                    "Session connected"
                } else {
                    "Session not yet verified"
                });
                if ui.button("Disconnect").clicked() {
                    self.disconnect();
                }
            });
            ui.horizontal_wrapped(|ui| {
                for (label, id) in [
                    ("Liked songs", "VLLM"),
                    ("Library songs", "FEmusic_liked_videos"),
                    ("Albums", "FEmusic_liked_albums"),
                    ("Playlists", "FEmusic_liked_playlists"),
                ] {
                    if ui
                        .add_enabled(self.pending.is_none(), egui::Button::new(label))
                        .clicked()
                    {
                        self.browse(id.into(), false);
                    }
                }
                if ui
                    .add_enabled(self.pending.is_none(), egui::Button::new("Refresh"))
                    .clicked()
                {
                    self.browse(self.browse_id.clone(), false);
                }
            });
            if let Some(track) = current {
                ui.horizontal_wrapped(|ui| {
                    ui.label(format!("Current song: {}", track.title));
                    for (label, liked) in [("Like song", true), ("Remove like", false)] {
                        if ui
                            .add_enabled(
                                self.pending.is_none() && self.verified,
                                egui::Button::new(label),
                            )
                            .clicked()
                        {
                            self.rate(track.clone(), liked);
                        }
                    }
                    if let Some(liked) = self.ratings.get(&track.id.0) {
                        ui.weak(if *liked {
                            "Confirmed liked"
                        } else {
                            "Confirmed not liked"
                        });
                    }
                });
            }
        }
        if self.pending.is_some() {
            ui.spinner();
        }
        ui.label(&self.message);
        let mut chosen = None;
        let mut unlike = None;
        egui::ScrollArea::vertical()
            .id_salt("account-library")
            .max_height(360.0)
            .show_rows(
                ui,
                64.0,
                self.page.collections.len() + self.page.tracks.len(),
                |ui, range| {
                    for index in range {
                        if let Some(collection) = self.page.collections.get(index) {
                            if ui
                                .add_enabled(
                                    self.pending.is_none(),
                                    egui::Button::new(&collection.title),
                                )
                                .clicked()
                            {
                                chosen = Some(collection.id.clone());
                            }
                            continue;
                        }
                        let track = &self.page.tracks[index - self.page.collections.len()];
                        ui.push_id(&track.id.0, |ui| {
                            ui.horizontal(|ui| {
                                if ui.button("Play").clicked() {
                                    action = Some(Action::Play(track.clone()));
                                }
                                if ui.button("Queue").clicked() {
                                    action = Some(Action::Append(track.clone()));
                                }
                                if self.browse_id == "VLLM"
                                    && ui
                                        .add_enabled(
                                            self.pending.is_none(),
                                            egui::Button::new("Unlike"),
                                        )
                                        .clicked()
                                {
                                    unlike = Some(track.clone());
                                }
                                ui.add(egui::Label::new(&track.title).truncate())
                                    .on_hover_text(&track.title);
                            });
                            if !track.artists.is_empty() {
                                ui.weak(track.artists.join(" · "));
                            }
                            ui.separator();
                        });
                    }
                },
            );
        if self.page.continuation.is_some()
            && ui
                .add_enabled(self.pending.is_none(), egui::Button::new("Load more"))
                .clicked()
        {
            chosen = Some(String::new());
        }
        if let Some(id) = chosen {
            if id.is_empty() {
                self.browse(self.browse_id.clone(), true);
            } else {
                self.browse(id, false);
            }
        }
        if let Some(track) = unlike {
            self.rate(track, false);
        }
        action
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paged_results_deduplicate_and_large_library_is_virtualized() {
        let mut account = AccountUi::default();
        for more in [false, true] {
            let (tx, rx) = mpsc::sync_channel(1);
            account.pending = Some(rx);
            tx.send(ResultMessage::Page(
                Ok(Page {
                    tracks: vec![crate::listening::track(1)],
                    continuation: Some("next".into()),
                    ..Default::default()
                }),
                more,
            ))
            .unwrap();
            account.poll();
        }
        assert_eq!(account.page.tracks.len(), 1);
        account.account = Some(Account::new("SAPISID=synthetic".into(), 0).unwrap());
        account.page.tracks = (0..10000).map(crate::listening::track).collect();
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 720.0),
                )),
                ..Default::default()
            },
            |ui| {
                account.render(ui, None);
            },
        );
        output.textures_delta.clear();
        assert!(
            output.shapes.len() < 500,
            "Offscreen account rows should not render"
        );
        account.seen.insert("next".into());
        account.browse("VLLM".into(), true);
        assert!(account.pending.is_none());
        assert!(account.page.continuation.is_none());
    }
    #[test]
    fn rejected_like_keeps_confirmed_state_and_logout_rejects_late_results() {
        let mut ui = AccountUi::default();
        let track = crate::listening::track(1);
        ui.ratings.insert(track.id.0.clone(), true);
        let (tx, rx) = mpsc::sync_channel(1);
        ui.pending = Some(rx);
        tx.send(ResultMessage::Rated(
            track.clone(),
            false,
            Err(Error::Rejected),
        ))
        .unwrap();
        ui.poll();
        assert_eq!(ui.ratings.get(&track.id.0), Some(&true));
        let (tx, rx) = mpsc::sync_channel(1);
        ui.pending = Some(rx);
        ui.disconnect();
        tx.send(ResultMessage::Page(
            Ok(Page {
                tracks: vec![track],
                ..Default::default()
            }),
            false,
        ))
        .unwrap();
        ui.poll();
        assert!(ui.page.tracks.is_empty());
        assert!(!ui.verified);
    }
}
