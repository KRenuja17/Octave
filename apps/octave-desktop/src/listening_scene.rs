//! Focus and companion surfaces share the same lyrics worker and audio clock.
use super::*;
use crate::shell::Destination;

impl Alpha {
    pub(super) fn destination(&self) -> Destination {
        if self.settings_page {
            Destination::Settings
        } else if self.playlists_page {
            Destination::Playlists
        } else if self.account_page {
            Destination::Account
        } else if self.queue_page {
            Destination::Listening(self.listening_tab)
        } else {
            Destination::Search
        }
    }
    pub(super) fn navigate(&mut self, target: Destination) {
        self.settings_page = target == Destination::Settings;
        self.playlists_page = target == Destination::Playlists;
        self.account_page = target == Destination::Account;
        self.queue_page = matches!(target, Destination::Listening(_));
        if let Destination::Listening(tab) = target {
            self.listening_tab = tab;
        }
    }
    pub(super) fn show_compact_panel(&mut self) {
        self.navigate(Destination::Listening(
            if self.session.view.panel_tab == 1 {
                2
            } else {
                1
            },
        ));
    }
    pub(super) fn render_lyrics(&mut self, ui: &mut egui::Ui) {
        let snapshot = self.player.snapshot();
        let can_seek = match &snapshot.state {
            PlaybackState::Playing(id) | PlaybackState::Paused(id) => {
                self.session.current().is_some_and(|track| &track.id == id)
            }
            _ => false,
        };
        self.lyrics.playhead(
            Duration::from_secs_f64(self.session.position.max(0.0)),
            can_seek,
        );
        if let Some(position) = self
            .lyrics
            .render(ui, self.session.current().map(|track| &track.id))
        {
            self.player.seek(position);
            self.session.position = position.as_secs_f64();
            self.seek = None;
        }
        if self.capture_load_lyrics && self.loaded && self.session.current().is_some() {
            self.capture_load_lyrics = false;
            self.lyrics.load_current();
        }
    }
    pub(super) fn focus_view(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            if ui.button("← Leave Focus").clicked()
                || ui.input(|i| i.key_pressed(egui::Key::Escape))
            {
                self.focus = false;
            }
            ui.checkbox(&mut self.session.view.lyrics_only, "Lyrics only");
            ui.checkbox(&mut self.session.view.reduced_motion, "Reduce motion");
            self.lyrics.controls(ui);
        });
        ui.add_space(20.0);
        let track = self.session.current().cloned();
        if !self.session.view.lyrics_only && ui.available_width() >= 840.0 {
            let width = (ui.available_width() * 0.40).clamp(280.0, 440.0);
            egui::Panel::left("focus-artwork")
                .exact_size(width)
                .frame(egui::Frame::new().inner_margin(18.0))
                .show(ui, |ui| {
                    if let Some(track) = track {
                        self.artwork.show(
                            ui,
                            &track.id.0,
                            &track.title,
                            (width - 36.0)
                                .min((ui.available_height() - 170.0).max(120.0))
                                .min(400.0),
                        );
                        ui.add_space(24.0);
                        ui.label(egui::RichText::new(&track.title).strong().size(30.0));
                        ui.label(track.artists.join(" · "));
                        ui.add_space(16.0);
                        ui.weak("Use the player below to pause, seek or change songs.");
                    } else {
                        ui.heading("Choose a song");
                    }
                });
        } else if let Some(track) = track {
            ui.horizontal(|ui| {
                self.artwork.show(ui, &track.id.0, &track.title, 44.0);
                ui.vertical(|ui| {
                    ui.strong(&track.title);
                    ui.weak(track.artists.join(" · "));
                });
            });
        }
        egui::ScrollArea::vertical()
            .id_salt(("focus-lyrics", self.session.current().map(|t| t.id.clone())))
            .auto_shrink([false, false])
            .show(ui, |ui| self.render_lyrics(ui));
    }
    pub(super) fn companion(&mut self, ui: &mut egui::Ui) {
        let mut resized = None;
        let panel = egui::Panel::right("listening-companion")
            .default_size(self.session.view.panel_width)
            .size_range(300.0..=440.0)
            .resizable(true)
            .frame(
                egui::Frame::new()
                    .fill(ui.visuals().window_fill)
                    .inner_margin(14.0),
            )
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.selectable_value(&mut self.session.view.panel_tab, 0, "Queue");
                    ui.selectable_value(&mut self.session.view.panel_tab, 1, "Lyrics");
                    ui.selectable_value(&mut self.session.view.panel_tab, 2, "Related");
                    if ui.small_button("Close panel").clicked() {
                        self.session.view.panel_open = false;
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    if ui.small_button("Narrower panel").clicked() {
                        resized = Some((self.session.view.panel_width - 20.0).max(300.0));
                    }
                    if ui.small_button("Wider panel").clicked() {
                        resized = Some((self.session.view.panel_width + 20.0).min(440.0));
                    }
                    if self.session.view.panel_tab == 1 {
                        self.lyrics.controls(ui);
                    }
                });
                ui.separator();
                match self.session.view.panel_tab {
                    1 => {
                        egui::ScrollArea::vertical()
                            .id_salt((
                                "companion-lyrics",
                                self.session.current().map(|t| t.id.clone()),
                            ))
                            .auto_shrink([false, false])
                            .show(ui, |ui| self.render_lyrics(ui));
                    }
                    2 => {
                        if let Some(track) = self
                            .related
                            .render(ui, self.session.current().map(|t| &t.id))
                        {
                            self.add(track, false);
                        }
                    }
                    _ => self.queue_preview(ui),
                }
            });
        self.session.view.panel_width = panel.response.rect.width().clamp(300.0, 440.0);
        if let Some(width) = resized {
            self.session.view.panel_width = width;
            let id = egui::Id::new("listening-companion");
            if let Some(mut state) = egui::containers::panel::PanelState::load(ui.ctx(), id) {
                state.outer_rect.min.x = state.outer_rect.max.x - width;
                ui.ctx().data_mut(|data| data.insert_persisted(id, state));
            }
        }
    }
    pub(super) fn queue_preview(&mut self, ui: &mut egui::Ui) {
        ui.weak(format!(
            "{} songs · order saved locally",
            self.session.queue.len()
        ));
        let mut select = None;
        egui::ScrollArea::vertical()
            .id_salt("companion-queue")
            .show_rows(ui, 64.0, self.session.queue.len(), |ui, range| {
                for index in range {
                    let track = &self.session.queue[index];
                    ui.push_id(index, |ui| {
                        ui.horizontal(|ui| {
                            self.artwork.show(ui, &track.id.0, &track.title, 40.0);
                            ui.vertical(|ui| {
                                let label = if index == self.session.cursor {
                                    format!("▶ {}", track.title)
                                } else {
                                    track.title.clone()
                                };
                                if ui
                                    .selectable_label(index == self.session.cursor, label)
                                    .clicked()
                                {
                                    select = Some(index);
                                }
                                ui.weak(track.artists.join(" · "));
                            });
                        });
                    });
                }
            });
        if let Some(index) = select {
            self.session.select(index);
            self.start(true);
        }
    }
}
