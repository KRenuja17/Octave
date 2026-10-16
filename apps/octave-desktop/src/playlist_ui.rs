use crate::playlists::{MAX_PLAYLISTS, Playlist, Store};
use eframe::egui;
use octave_core::Track;
use std::path::PathBuf;

pub struct PlaylistsUi {
    store: Store,
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
            data: Vec::new(),
            busy: true,
            selected: 0,
            selection_after_save: None,
            name: String::new(),
            message: String::new(),
            confirm_delete: false,
        }
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
                    self.message = "Saved playlists are up to date.".into();
                }
                Err(()) => self.message="Could not load or save playlists. Check storage access and use a unique name; the previous saved data is unchanged.".into(),
            }
        }
        ui.weak("Stored on this computer. Edit songs in Queue, then replace the selected playlist. Loading replaces the queue without starting playback.");
        let mut loaded = None;
        ui.add_enabled_ui(!self.busy, |ui| {
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
            if !self.data.is_empty() {
                egui::ComboBox::from_label("Saved playlist")
                    .selected_text(&self.data[self.selected].name)
                    .show_ui(ui, |ui| {
                        for (index, playlist) in self.data.iter().enumerate() {
                            if ui
                                .selectable_value(&mut self.selected, index, &playlist.name)
                                .changed()
                            {
                                self.confirm_delete = false;
                            }
                        }
                    });
                ui.label(format!(
                    "{} songs · {} of {MAX_PLAYLISTS} playlists",
                    self.data[self.selected].tracks.len(),
                    self.data.len()
                ));
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Load into queue").clicked() {
                        loaded = Some(self.data[self.selected].tracks.clone());
                    }
                    if ui.button("Replace with queue").clicked() {
                        let mut data = self.data.clone();
                        data[self.selected].tracks = queue.to_vec();
                        self.submit(data);
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
                    }
                    if ui.button("Delete playlist…").clicked() {
                        self.confirm_delete = true;
                    }
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
        click(&ctx, &mut app, &queue, "Rename playlist");
        ready(&ctx, &mut app, &queue);
        let edited = vec![track(1)];
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
        click(&ctx, &mut app, &[], "Delete playlist…");
        click(&ctx, &mut app, &[], "Cancel deletion");
        assert_eq!(app.data.len(), 1);
        click(&ctx, &mut app, &[], "Delete playlist…");
        click(&ctx, &mut app, &[], "Confirm delete playlist");
        ready(&ctx, &mut app, &[]);
        assert!(app.data.is_empty());
        drop(app);
        std::fs::remove_file(path).unwrap();
    }
}
