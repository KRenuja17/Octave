//! Account work is serialized; logout revokes all shared session handles.
use eframe::egui;
use octave_core::Track;
use octave_provider::account::{Account, Error, Page};
use std::{
    collections::{HashMap, HashSet},
    sync::mpsc::{self, Receiver},
};
use zeroize::Zeroize;
mod library;
pub use library::LibraryRoute;
enum ResultMessage {
    Page(Result<Page, Error>, bool),
    Rated(Track, bool, Result<(), Error>),
}
pub enum Action {
    Play(Vec<Track>, usize),
    Shuffle(Vec<Track>),
    Append(Track),
    AppendAll(Vec<Track>),
    OpenLibrary(LibraryRoute),
    OpenAccount,
    LocalPlaylists,
}
pub struct AccountUi {
    auth: crate::account_auth::Auth,
    browser: crate::account_auth::Browser,
    identity: Option<octave_provider::account::Identity>,
    channels: Vec<octave_provider::account::Channel>,
    remember: bool,
    playback_changed: bool,
    saved_revision: u64,
    verified_at: std::time::Instant,
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
    pending_browse: Option<String>,
    library: library::Library,
}
impl Default for AccountUi {
    fn default() -> Self {
        Self {
            auth: Default::default(),
            browser: Default::default(),
            identity: None,
            channels: Vec::new(),
            remember: false,
            playback_changed: false,
            saved_revision: 0,
            verified_at: std::time::Instant::now(),
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
            pending_browse: None,
            library: Default::default(),
        }
    }
}
impl Drop for AccountUi {
    fn drop(&mut self) {
        self.disconnect();
    }
}
impl AccountUi {
    pub fn at(root: std::path::PathBuf) -> Self {
        let mut account = Self::default();
        account.auth = crate::account_auth::Auth::at(root);
        account
    }
    pub fn take_playback_change(&mut self) -> Option<Option<Account>> {
        if !std::mem::take(&mut self.playback_changed) {
            return None;
        }
        Some(self.identity.as_ref().and(self.account.clone()))
    }
    fn disconnect(&mut self) {
        self.auth.cancel();
        if let Some(account) = self.account.take() {
            account.disconnect();
        }
        self.cookie.zeroize();
        self.page = Page::default();
        self.ratings.clear();
        self.seen.clear();
        self.library = Default::default();
        self.verified = false;
        self.identity = None;
        self.channels.clear();
        self.playback_changed = true;
        self.generation += 1;
        self.message =
            "Disconnected. The active Octave session was discarded. Use Forget account to remove remembered credentials too."
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
            self.library.message =
                "Pagination stopped because the server repeated a page token.".into();
            return;
        }
        self.browse_id = id.clone();
        self.pending_browse = Some(id.clone());
        self.library.loaded = true;
        self.pending_generation = self.generation;
        let (tx, rx) = mpsc::sync_channel(1);
        self.pending = Some(rx);
        self.library.message = "Loading your music…".into();
        std::thread::spawn(move || {
            let result = if cfg!(test) {
                Err(Error::Network)
            } else {
                account.browse(&id, token.as_deref())
            };
            let _ = tx.send(ResultMessage::Page(result, more));
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
        self.pending_browse = None;
        let (tx, rx) = mpsc::sync_channel(1);
        self.pending = Some(rx);
        self.message = "Waiting for YouTube to confirm the change…".into();
        self.library.message = self.message.clone();
        std::thread::spawn(move || {
            let result = if cfg!(test) {
                Err(Error::Network)
            } else {
                account.rate(&track.id, liked)
            };
            let _ = tx.send(ResultMessage::Rated(track, liked, result));
        });
    }
    pub fn poll(&mut self) {
        if let Some(event) = self.auth.poll() {
            use crate::account_auth::Event;
            match event {
                Event::Ready(account, identity, channels) => {
                    let changed = self
                        .account
                        .as_ref()
                        .is_none_or(|old| !old.same_session(&account));
                    if let Some(old) = self.account.replace(account.clone())
                        && !old.same_session(&account)
                    {
                        old.disconnect();
                    }
                    self.identity = Some(identity.clone());
                    self.channels = channels;
                    self.verified = true;
                    self.verified_at = std::time::Instant::now();
                    if changed {
                        self.playback_changed = true;
                        self.generation += 1;
                        self.pending = None;
                        self.page = Page::default();
                        self.ratings.clear();
                        self.seen.clear();
                        self.library = Default::default();
                        self.library.requested = true;
                        self.browse_id = LibraryRoute::Liked.id().into();
                    }
                    self.message = "Account connected. Your music is available in Library.".into();
                    if self.remember && changed {
                        self.auth.save(account, identity);
                    }
                }
                Event::Saved(revision) => {
                    self.saved_revision = revision;
                    self.message = "Account remembered using OS-protected encrypted storage.".into()
                }
                Event::Forgotten => {
                    self.remember = false;
                    self.message="Forgotten. Remembered credentials and temporary sign-in browser data were removed.".into();
                }
                Event::CancelledCleanly => {
                    self.message = "Cancelled. Temporary sign-in browser data was removed.".into()
                }
                Event::Failed(error) => {
                    if self
                        .account
                        .as_ref()
                        .is_some_and(|account| !account.is_active())
                    {
                        self.disconnect();
                    }
                    if matches!(
                        error,
                        crate::account_auth::Error::Vault | crate::account_auth::Error::Storage
                    ) {
                        self.remember = false;
                    }
                    self.message = error.to_string();
                }
            }
        }
        if self.remember
            && !self.auth.pending
            && let (Some(account), Some(identity)) = (self.account.as_ref(), self.identity.as_ref())
            && account.session_revision() > self.saved_revision
        {
            self.auth.save(account.clone(), identity.clone());
        } else if !self.auth.pending
            && self.pending.is_none()
            && self.verified_at.elapsed() >= std::time::Duration::from_secs(600)
            && self.identity.is_some()
            && let Some(account) = self.account.clone()
        {
            self.auth.verify(account);
            self.verified_at = std::time::Instant::now();
        }
        self.load_library_if_needed();
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
        let result_id = self
            .pending_browse
            .take()
            .unwrap_or_else(|| self.browse_id.clone());
        if self.generation != self.pending_generation {
            return;
        }
        let error = match result {
            ResultMessage::Page(Ok(page), more) => {
                self.accept_library_page(&result_id, page, more);
                self.verified = true;
                None
            }
            ResultMessage::Rated(track, liked, Ok(())) => {
                self.ratings.insert(track.id.0.clone(), liked);
                if !liked && self.browse_id == "VLLM" {
                    self.page.tracks.retain(|t| t.id != track.id);
                }
                self.library.remove_unliked(&track, liked);
                self.message = "YouTube confirmed the change.".into();
                self.library.message = self.message.clone();
                None
            }
            ResultMessage::Page(Err(error), more) => {
                self.library_error(&result_id, more, error);
                Some(error)
            }
            ResultMessage::Rated(_, _, Err(error)) => {
                self.library.message = error.to_string();
                Some(error)
            }
        };
        if let Some(error) = error {
            if error == Error::Expired {
                self.disconnect();
            }
            self.message = error.to_string();
        }
    }
    pub fn render(&mut self, ui: &mut egui::Ui) -> Option<Action> {
        let mut action = None;
        ui.heading("Account");
        ui.weak("Manage your YouTube Music connection and account storage.");
        ui.add_space(18.0);
        if self.account.is_none() {
            crate::theme::surface(ui, |ui| {
                ui.strong("Not connected");
                ui.add_space(8.0);
                ui.label("Search, listening and local playlists are available without connecting an account.");
                ui.add_space(8.0);
                ui.weak("Account access is experimental. Only streams actually offered by your account can be played.");
                ui.add_space(10.0);
                ui.add_enabled_ui(!self.auth.pending,|ui|{
                    ui.add_enabled(cfg!(any(target_os="windows",target_os="macos")),egui::Checkbox::new(&mut self.remember,"Remember account on this device"));
                    ui.weak("Off by default. Remembered credentials are encrypted with a key held by the OS credential store.");
                    ui.label("Sign-in browser");
                    egui::ComboBox::from_id_salt("account_signin_browser")
                        .selected_text(self.browser.label())
                        .show_ui(ui, |ui| {
                            for browser in crate::account_auth::Browser::ALL {
                                ui.add_enabled_ui(browser.installed(), |ui| {
                                    ui.selectable_value(&mut self.browser, browser, browser.label());
                                });
                            }
                        });
                    ui.small("Uses a separate Octave profile. Your personal browser sign-in is not imported.");
                    if ui.add_enabled(self.auth.available() && self.browser.installed(),egui::Button::new("Sign in with Google")).clicked(){
                        self.auth.sign_in(self.browser);self.message="Sign in in the separate browser, wait for YouTube Music to load, then CLOSE that window to connect Octave. Never paste credentials into chat.".into();
                    }
                    if self.auth.saved()&&ui.button("Restore remembered account").clicked(){self.remember=true;self.auth.restore();self.message="Verifying the remembered account…".into();}
                });
                if self.auth.pending && ui.button("Cancel sign-in").clicked() {
                    self.auth.cancel();
                    self.message = "Cancelling sign-in and cleaning the temporary browser…".into();
                }
            });
            ui.add_space(12.0);
            crate::theme::surface(ui, |ui| {
                ui.weak("Optional · advanced beta connection");
                egui::CollapsingHeader::new("Connection options").show(ui, |ui| {
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
                    .margin(egui::vec2(12.0, 10.0))
                    .desired_width(f32::INFINITY),
            );
            ui.add(
                egui::DragValue::new(&mut self.index)
                    .range(0..=99)
                    .prefix("Account index: "),
            );
            if ui
                .add_enabled(
                    self.pending.is_none() && !self.auth.pending && !self.cookie.is_empty(),
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
            });
            });
        } else {
            crate::theme::surface(ui, |ui| {
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
                if let Some(identity) = &self.identity {
                    ui.strong(&identity.name);
                    if let Some(handle) = &identity.handle {
                        ui.weak(handle);
                    }
                    ui.weak(match identity.premium{Some(true)=>"YouTube reports a Premium account.",Some(false)=>"YouTube reports a standard account.",None=>"Subscription status was not reported. Actual offered playback quality is shown in the player."});
                    let mut selected = None;
                    ui.add_enabled_ui(!self.auth.pending && self.pending.is_none(), |ui| {
                        egui::ComboBox::from_label("YouTube channel")
                            .selected_text(&identity.name)
                            .show_ui(ui, |ui| {
                                for channel in &self.channels {
                                    if ui
                                        .selectable_label(channel.id == identity.id, &channel.name)
                                        .clicked()
                                    {
                                        selected = Some(channel.clone());
                                    }
                                }
                            });
                        if ui.button("Refresh account session").clicked()
                            && let Some(account) = self.account.clone()
                        {
                            self.auth.verify(account);
                            self.message =
                                "Checking the same account and refreshing its session…".into();
                        }
                        if !self.remember
                            && ui
                                .add_enabled(
                                    cfg!(any(target_os = "windows", target_os = "macos")),
                                    egui::Button::new("Remember this account"),
                                )
                                .clicked()
                            && let Some(account) = self.account.clone()
                        {
                            self.remember = true;
                            self.auth.save(account, identity.clone());
                        }
                    });
                    if let Some(channel) = selected
                        && let Some(account) = self.account.clone()
                    {
                        self.auth.select(account, channel);
                        self.message = "Verifying the selected YouTube channel…".into();
                    }
                }
                ui.add_space(14.0);
                if crate::theme::primary(ui, "Open Library").clicked() {
                    action = Some(Action::OpenLibrary(LibraryRoute::Liked));
                }
            });
        }
        // Also allow removal of an orphan vault key when no sealed file remains.
        if self.auth.available()
            && ui
                .add_enabled(
                    !self.auth.pending,
                    egui::Button::new("Forget account and sign out"),
                )
                .clicked()
        {
            self.disconnect();
            self.auth.forget();
            self.message = "Removing remembered credentials and temporary browser data…".into();
        }
        if self.auth.pending {
            ui.spinner();
        }
        if self.pending.is_some() {
            ui.spinner();
        }
        if !self.message.is_empty() {
            ui.add_space(10.0);
            ui.label(&self.message);
        }
        action
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn signed_out_view_is_idle_and_connection_form_is_collapsed() {
        for width in [480.0, 1100.0] {
            let ctx = egui::Context::default();
            ctx.enable_accesskit();
            let mut account = AccountUi::default();
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, 760.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    assert!(account.render(ui).is_none());
                },
            );
            output.textures_delta.clear();
            let tree = output.platform_output.accesskit_update.unwrap();
            assert!(
                tree.nodes
                    .iter()
                    .any(|(_, n)| n.label() == Some("Connection options"))
            );
            assert!(
                !tree
                    .nodes
                    .iter()
                    .any(|(_, n)| n.label() == Some("Connect session"))
            );
            assert!(
                account.account.is_none() && account.pending.is_none() && account.cookie.is_empty()
            );
        }
    }
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
                account.render_library(ui, &LibraryRoute::Liked, None);
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
