use crate::playlists::{MAX_PLAYLISTS, Playlist, Store};
use eframe::egui;
use octave_core::Track;
use std::path::PathBuf;

pub struct PlaylistsUi {
    store: Store,
    artwork: crate::song_artwork::SongArtwork,
    data: Vec<Playlist>,
    busy: bool,
    selected: usize,
    selection_after_save: Option<usize>,
    name: String,
    message: String,
    confirm_delete: bool,
}
impl PlaylistsUi {
    pub fn new(path: PathBuf) -> Self {
        Self {
            store: Store::open(path),
            artwork: crate::song_artwork::SongArtwork::default(),
            data: Vec::new(),
            busy: true,
            selected: 0,
            selection_after_save: None,
            name: String::new(),
            message: String::new(),
            confirm_delete: false,
        }
    }
    pub fn settled(&self) -> bool {
        !self.busy && self.artwork.settled()
    }
    fn submit(&mut self, data: Vec<Playlist>) {
        self.selection_after_save = None;
        if self.store.save(data) {
            self.busy = true;
            self.message = "Saving…".into();
            self.confirm_delete = false;
        } else {
            self.message =
                "Playlist storage is unavailable. Your last saved playlists are unchanged.".into();
        }
    }
    pub fn render(&mut self, ui: &mut egui::Ui, queue: &[Track]) -> Option<Vec<Track>> {
        while let Ok(event) = self.store.events.try_recv() {
            self.busy = false;
            let selected = self.selection_after_save.take().unwrap_or(self.selected);
            match event {
                Ok(data) => {
                    self.data = data;
                    self.selected = selected.min(self.data.len().saturating_sub(1));
                    self.message.clear();
                }
                Err(()) => self.message="Could not load or save playlists. Check storage access and use a unique name; the previous saved data is unchanged.".into(),
            }
        }
        ui.weak("Local library · saved on this computer");
        ui.add_space(6.0);
        let mut loaded = None;
        ui.add_enabled_ui(!self.busy, |ui| {
            ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.name)
                    .hint_text("Playlist name")
                    .char_limit(80),
            );
            if ui
                .add_enabled(
                    !self.name.trim().is_empty() && self.data.len() < MAX_PLAYLISTS,
                    egui::Button::new("Save queue as new playlist"),
                )
                .clicked()
            {
                let mut data = self.data.clone();
                data.push(Playlist {
                    name: self.name.trim().into(),
                    tracks: queue.to_vec(),
                });
                self.submit(data);
                if self.busy {
                    self.selection_after_save = Some(self.data.len());
                }
            }
            });
            ui.add_space(10.0);
            if self.data.is_empty() {
                ui.heading("Create your first playlist");
                ui.weak("Add songs to your queue, give the playlist a name, then save it here.");
            } else {
                let compact = ui.available_width() < 600.0;
                let cover_size = if compact { 72.0 } else { 112.0 };
                let card_width = if compact { 112.0 } else { 148.0 };
                egui::ScrollArea::horizontal().id_salt("playlist-covers").show(ui, |ui| {
                    ui.horizontal_top(|ui| {
                        for (index, playlist) in self.data.iter().enumerate() {
                            ui.push_id(index, |ui| {
                                ui.vertical(|ui| {
                                    ui.set_width(card_width);
                                    let cover = ui.scope(|ui| {
                                    if let Some(track) = playlist.tracks.first() {
                                        self.artwork.show(ui, &track.id.0, &playlist.name, cover_size);
                                    } else { crate::theme::cover(ui, cover_size); }
                                    });
                                    let open = ui.interact(cover.response.rect, ui.id().with("open-cover"), egui::Sense::click()).on_hover_text(&playlist.name);
                                    open.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Open playlist {}", playlist.name)));
                                    if open.clicked() { self.selected = index; self.confirm_delete = false; }
                                    if ui.add_sized([card_width, 34.0], egui::Button::new(
                                        egui::RichText::new(&playlist.name).strong()).selected(self.selected == index).truncate())
                                        .on_hover_text(&playlist.name).clicked() {
                                        self.selected = index;
                                        self.confirm_delete = false;
                                    }
                                    ui.weak(format!("{} songs", playlist.tracks.len()));
                                });
                            });
                        }
                    });
                });
                ui.add_space(10.0);
                ui.separator();
                ui.add_space(12.0);
                ui.label(egui::RichText::new(&self.data[self.selected].name).strong().size(24.0));
                ui.weak(format!("Playlist · {} songs", self.data[self.selected].tracks.len()));
                ui.horizontal_wrapped(|ui| {
                    if crate::theme::primary(ui, "Load into queue").on_hover_text("Replace the queue without starting playback").clicked() {
                        loaded = Some(self.data[self.selected].tracks.clone());
                    }
                    ui.menu_button("Edit playlist", |ui| {
                    if ui.button("Replace with queue").clicked() {
                        let mut data = self.data.clone();
                        data[self.selected].tracks = queue.to_vec();
                        self.submit(data);
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            !self.name.trim().is_empty(),
                            egui::Button::new("Rename playlist"),
                        )
                        .clicked()
                    {
                        let mut data = self.data.clone();
                        data[self.selected].name = self.name.trim().into();
                        self.submit(data);
                        ui.close();
                    }
                    if ui.button("Delete playlist…").clicked() {
                        self.confirm_delete = true;
                        ui.close();
                    }
                    });
                });
                if self.confirm_delete {
                    ui.label(format!(
                        "Delete '{}' from this computer?",
                        self.data[self.selected].name
                    ));
                    ui.horizontal(|ui| {
                        if ui.button("Confirm delete playlist").clicked() {
                            let mut data = self.data.clone();
                            data.remove(self.selected);
                            self.submit(data);
                        }
                        if ui.button("Cancel deletion").clicked() {
                            self.confirm_delete = false;
                        }
                    });
                }
                ui.add_space(16.0);
                let tracks = &self.data[self.selected].tracks;
                if tracks.is_empty() { ui.weak("This playlist has no songs yet. Add songs to Queue, then use Replace with queue."); }
                egui::ScrollArea::vertical().id_salt(("playlist-songs", self.selected))
                    .max_height(320.0).show_rows(ui, 56.0, tracks.len(), |ui, range| {
                    for index in range {
                        let track = &tracks[index];
                        ui.horizontal(|ui| {
                            ui.weak(format!("{:02}", index + 1));
                            self.artwork.show(ui, &track.id.0, &track.title, 40.0);
                            ui.vertical(|ui| {
                                ui.set_width(ui.available_width());
                                ui.add(egui::Label::new(egui::RichText::new(&track.title).strong()).truncate()).on_hover_text(&track.title);
                                ui.add(egui::Label::new(egui::RichText::new(track.artists.join(" · ")).weak()).truncate());
                            });
                        });
                    }
                });
            }
        });
        if self.busy {
            ui.spinner();
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }
        ui.label(&self.message);
        loaded
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::listening::track;
    fn frame(
        ctx: &egui::Context,
        app: &mut PlaylistsUi,
        queue: &[Track],
        events: Vec<egui::Event>,
    ) -> (egui::FullOutput, Option<Vec<Track>>) {
        let mut result = None;
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(900.0, 700.0),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                result = app.render(ui, queue);
            },
        );
        output.textures_delta.clear();
        (output, result)
    }
    fn ready(ctx: &egui::Context, app: &mut PlaylistsUi, queue: &[Track]) {
        let end = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while app.busy {
            frame(ctx, app, queue, vec![]);
            assert!(std::time::Instant::now() < end, "Playlist worker timed out");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
    fn click(
        ctx: &egui::Context,
        app: &mut PlaylistsUi,
        queue: &[Track],
        name: &str,
    ) -> Option<Vec<Track>> {
        let tree = frame(ctx, app, queue, vec![])
            .0
            .platform_output
            .accesskit_update
            .unwrap();
        let rect = tree
            .nodes
            .iter()
            .find_map(|(_, node)| {
                (node.label() == Some(name))
                    .then(|| node.bounds())
                    .flatten()
            })
            .unwrap_or_else(|| panic!("Missing {name}"));
        let pos = egui::pos2(
            ((rect.x0 + rect.x1) / 2.0) as f32,
            ((rect.y0 + rect.y1) / 2.0) as f32,
        );
        let mut result = None;
        for pressed in [true, false] {
            result = frame(
                ctx,
                app,
                queue,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            )
            .1
            .or(result);
        }
        result
    }
    #[test]
    fn cover_selection_previews_without_loading_and_resets_delete_confirmation() {
        let path = std::env::var_os("OCTAVE_TEST_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join(format!("playlist-cover-ui-{}.sqlite3", std::process::id()));
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let queue = vec![track(9)];
        let mut app = PlaylistsUi::new(path.clone());
        ready(&ctx, &mut app, &queue);
        app.data = vec![
            Playlist {
                name: "First".into(),
                tracks: vec![track(1)],
            },
            Playlist {
                name: "Second".into(),
                tracks: vec![track(2), track(3)],
            },
        ];
        app.confirm_delete = true;
        assert!(click(&ctx, &mut app, &queue, "Open playlist Second").is_none());
        assert_eq!(app.selected, 1);
        assert!(!app.confirm_delete);
        assert_eq!(queue, vec![track(9)]);
        assert_eq!(
            click(&ctx, &mut app, &queue, "Load into queue"),
            Some(vec![track(2), track(3)])
        );
        drop(app);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn buttons_save_edit_reload_and_delete_durable_playlists() {
        let path = std::env::var_os("OCTAVE_TEST_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir)
            .join(format!("playlist-ui-{}.sqlite3", std::process::id()));
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let queue = vec![track(2), track(1)];
        let mut app = PlaylistsUi::new(path.clone());
        ready(&ctx, &mut app, &queue);
        app.name = "Evening 日本語".into();
        click(&ctx, &mut app, &queue, "Save queue as new playlist");
        ready(&ctx, &mut app, &queue);
        assert_eq!(app.data[0].tracks, queue);
        app.name = "Renamed".into();
        click(&ctx, &mut app, &queue, "Edit playlist");
        click(&ctx, &mut app, &queue, "Rename playlist");
        ready(&ctx, &mut app, &queue);
        let edited = vec![track(1)];
        click(&ctx, &mut app, &edited, "Edit playlist");
        click(&ctx, &mut app, &edited, "Replace with queue");
        ready(&ctx, &mut app, &edited);
        drop(app);
        let mut app = PlaylistsUi::new(path.clone());
        ready(&ctx, &mut app, &[]);
        assert_eq!(app.data[0].name, "Renamed");
        assert_eq!(
            click(&ctx, &mut app, &[], "Load into queue").unwrap(),
            edited
        );
        click(&ctx, &mut app, &[], "Edit playlist");
        click(&ctx, &mut app, &[], "Delete playlist…");
        click(&ctx, &mut app, &[], "Cancel deletion");
        assert_eq!(app.data.len(), 1);
        click(&ctx, &mut app, &[], "Edit playlist");
        click(&ctx, &mut app, &[], "Delete playlist…");
        click(&ctx, &mut app, &[], "Confirm delete playlist");
        ready(&ctx, &mut app, &[]);
        assert!(app.data.is_empty());
        drop(app);
        std::fs::remove_file(path).unwrap();
    }
}
