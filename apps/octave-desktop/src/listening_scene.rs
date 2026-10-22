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
    pub(super) fn remove_queue_entry(&mut self, index: usize) {
        if index >= self.session.queue.len() {
            return;
        }
        let before = (
            self.session.queue.clone(),
            self.session.cursor,
            self.session.position,
        );
        if self.session.remove(index) {
            self.halt();
        }
        self.undo_queue = Some((before.0, before.1, before.2, self.session.queue.revision()));
    }
    pub(super) fn can_undo_removal(&self) -> bool {
        self.undo_queue
            .as_ref()
            .is_some_and(|(_, _, _, revision)| *revision == self.session.queue.revision())
    }
    pub(super) fn undo_removal(&mut self) {
        if !self.can_undo_removal() {
            self.undo_queue = None;
            return;
        }
        if let Some((queue, cursor, position, _)) = self.undo_queue.take() {
            let active = self.session.queue.id_at(self.session.cursor);
            let preserve = matches!(
                self.player.snapshot().state,
                PlaybackState::Playing(_) | PlaybackState::Paused(_)
            );
            let current = preserve
                .then(|| {
                    queue
                        .iter()
                        .enumerate()
                        .position(|(i, _)| queue.id_at(i) == active)
                })
                .flatten();
            self.session.queue = queue;
            self.session.cursor = current
                .unwrap_or(cursor)
                .min(self.session.queue.len().saturating_sub(1));
            if current.is_none() {
                self.session.position = position;
            }
        }
    }

    pub(super) fn queue_preview(&mut self, ui: &mut egui::Ui) {
        ui.weak(format!(
            "{} entries · complete loaded queue",
            self.session.queue.len()
        ));
        if ui
            .add_enabled(
                self.can_undo_removal(),
                egui::Button::new("Undo queue removal"),
            )
            .clicked()
        {
            self.undo_removal();
        }
        let mut action = None;
        let mut drop = None;
        egui::ScrollArea::vertical()
            .id_salt("companion-queue")
            .show_rows(ui, 72.0, self.session.queue.len(), |ui, range| {
                for index in range {
                    let track = self.session.queue[index].clone();
                    let entry = self.session.queue.id_at(index).unwrap();
                    let row = ui
                        .push_id(entry, |ui| {
                            ui.horizontal(|ui| {
                                ui.dnd_drag_source(
                                    egui::Id::new(("queue-drag", entry)),
                                    entry,
                                    |ui| {
                                        ui.label("⠿");
                                    },
                                );
                                self.artwork.show(ui, &track.id.0, &track.title, 36.0);
                                ui.vertical(|ui| {
                                    ui.set_width((ui.available_width() - 46.0).max(90.0));
                                    let label = if index == self.session.cursor {
                                        format!("▶ {}", track.title)
                                    } else {
                                        track.title.clone()
                                    };
                                    if ui
                                        .add(egui::Button::new(label).frame(false).wrap())
                                        .clicked()
                                    {
                                        action = Some((index, 0));
                                    }
                                    ui.add(
                                        egui::Label::new(
                                            egui::RichText::new(track.artists.join(" · ")).weak(),
                                        )
                                        .truncate(),
                                    );
                                });
                                ui.menu_button("•••", |ui| {
                                    for (label, code) in [
                                        ("Move up", 1),
                                        ("Move down", 2),
                                        ("Remove", 3),
                                        ("Add duplicate", 4),
                                        ("Play next", 5),
                                    ] {
                                        if ui.button(label).clicked() {
                                            action = Some((index, code));
                                            ui.close();
                                        }
                                    }
                                });
                            })
                        })
                        .inner
                        .response;
                    if row.dnd_hover_payload::<u64>().is_some() {
                        ui.painter().hline(
                            row.rect.x_range(),
                            row.rect.top(),
                            egui::Stroke::new(2.0, ui.visuals().selection.stroke.color),
                        );
                    }
                    if let Some(source) = row.dnd_release_payload::<u64>() {
                        drop = Some((*source, index));
                    }
                }
            });
        if let Some((source, to)) = drop {
            if let Some(from) = self
                .session
                .queue
                .iter()
                .enumerate()
                .position(|(i, _)| self.session.queue.id_at(i) == Some(source))
            {
                self.session.move_item(from, to);
            }
        } else if let Some((index, code)) = action {
            match code {
                0 => {
                    self.session.select(index);
                    self.start(true);
                }
                1 => self.session.move_item(index, index.saturating_sub(1)),
                2 => self.session.move_item(index, index + 1),
                3 => self.remove_queue_entry(index),
                4 => {
                    if self.session.queue.len() < crate::listening::MAX_QUEUE {
                        self.session.queue.push(self.session.queue[index].clone());
                    }
                }
                5 => self.session.move_item(
                    index,
                    (self.session.cursor + 1).min(self.session.queue.len() - 1),
                ),
                _ => (),
            }
        }
    }
}
