use eframe::egui;
use octave_core::diagnostics::Event;
fn main() -> eframe::Result {
    let smoke_test = std::env::args().any(|arg| arg == "--smoke-test");
    eprintln!("{}", Event::ApplicationStarted.as_json());
    let result = eframe::run_native(
        "Octave",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([960.0, 640.0])
                .with_min_inner_size([480.0, 320.0]),
            ..Default::default()
        },
        Box::new(move |_| {
            Ok(Box::new(Octave {
                smoke_test,
                frames: 0,
            }))
        }),
    );
    eprintln!("{}", Event::ApplicationStopped.as_json());
    result
}
struct Octave {
    smoke_test: bool,
    frames: u32,
}
impl eframe::App for Octave {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.add_space(32.0);
            ui.heading("Octave");
            ui.label("Your music, at home on your desktop.");
            ui.add_space(24.0);
            ui.label("Playback is coming soon.");
            ui.label("This early build establishes the desktop application.");
        });
        self.frames = self.frames.saturating_add(1);
        if self.frames == 1 {
            eprintln!("{}", Event::WindowReady.as_json());
        }
        if self.smoke_test {
            if self.frames >= 3 {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            } else {
                ui.ctx().request_repaint();
            }
        }
    }
}
