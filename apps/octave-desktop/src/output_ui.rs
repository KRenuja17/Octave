use eframe::egui;
use octave_player::{Failure, OutputDevice};
use std::sync::mpsc::{self, Receiver};

pub struct OutputUi {
    devices: Vec<OutputDevice>,
    pending: Option<Receiver<Result<Vec<OutputDevice>, Failure>>>,
    selected: usize,
    applied: String,
    error: Option<String>,
}
impl Default for OutputUi {
    fn default() -> Self {
        Self {
            devices: Vec::new(),
            pending: None,
            selected: 0,
            applied: "System default".into(),
            error: None,
        }
    }
}
impl OutputUi {
    /// Outer Some means Apply was clicked; inner None means system default.
    pub fn render(&mut self, ui: &mut egui::Ui) -> Option<Option<OutputDevice>> {
        if let Some(rx) = &self.pending {
            match rx.try_recv() {
                Ok(result) => {
                    self.pending = None;
                    match result {
                        Ok(devices) => {
                            self.devices = devices;
                            self.selected = 0;
                            self.error = None;
                        }
                        Err(error) => self.error = Some(error.to_string()),
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.pending = None;
                    self.error = Some("Audio outputs could not be listed. Try again.".into());
                }
                Err(mpsc::TryRecvError::Empty) => {
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_millis(100));
                }
            }
        }
        ui.label(format!("Output preference: {}", self.applied));
        ui.weak("Applies until Quit. Changing output stops playback and keeps your position. Press Resume when ready.");
        if ui
            .add_enabled(self.pending.is_none(), egui::Button::new("Refresh outputs"))
            .clicked()
        {
            let (tx, rx) = mpsc::sync_channel(1);
            self.pending = Some(rx);
            let ctx = ui.ctx().clone();
            std::thread::spawn(move || {
                let _ = tx.send(octave_player::output_devices());
                ctx.request_repaint();
            });
        }
        if self.pending.is_some() {
            ui.spinner();
        }
        let selected_name = self
            .devices
            .get(self.selected.wrapping_sub(1))
            .map_or("System default", OutputDevice::name);
        egui::ComboBox::from_label("Audio output")
            .selected_text(selected_name)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut self.selected, 0, "System default");
                for (index, device) in self.devices.iter().enumerate() {
                    ui.selectable_value(
                        &mut self.selected,
                        index + 1,
                        format!("{} · {}", device.name(), index + 1),
                    );
                }
            });
        if let Some(error) = &self.error {
            ui.label(error);
        }
        if ui
            .add_enabled(
                self.pending.is_none(),
                egui::Button::new("Use selected output"),
            )
            .clicked()
        {
            let device = self.devices.get(self.selected.wrapping_sub(1)).cloned();
            self.applied = device
                .as_ref()
                .map_or("System default", OutputDevice::name)
                .to_owned();
            return Some(device);
        }
        None
    }
}
