mod artwork;
mod live;
mod model;
mod prototype;
mod theme;
use eframe::egui;
use model::{Page, ViewState};
use prototype::Prototype;

fn argument(name: &str) -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    args.windows(2).find(|w| w[0] == name).map(|w| w[1].clone())
}
fn main() -> eframe::Result {
    if std::env::args().any(|s| s == "--live-preview") {
        return live::run();
    }
    let compact = std::env::args().any(|s| s == "--compact");
    let capture = argument("--capture");
    let scale = argument("--scale")
        .and_then(|v| v.parse::<f32>().ok())
        .unwrap_or(1.0)
        .clamp(1.0, 2.0);
    let smoke = std::env::args().any(|s| s == "--smoke-test");
    eframe::run_native(
        "Octave",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size(if compact {
                    [560.0, 720.0]
                } else {
                    [1280.0, 900.0]
                })
                .with_min_inner_size([480.0, 480.0]),
            ..Default::default()
        },
        Box::new(move |cc| {
            let mut prototype = Prototype::new(&cc.egui_ctx);
            cc.egui_ctx.set_pixels_per_point(scale);
            prototype.page = match argument("--page").as_deref() {
                Some("search") => Page::Search,
                Some("library") => Page::Library,
                Some("album") => Page::Collection(0),
                Some("playlist") => Page::Collection(1),
                Some("queue") => Page::Queue,
                Some("lyrics") => Page::Lyrics,
                _ => Page::Home,
            };
            prototype.state = match argument("--state").as_deref() {
                Some("loading") => ViewState::Loading,
                Some("empty") => ViewState::Empty,
                Some("error") => ViewState::Error,
                Some("offline") => ViewState::Offline,
                Some("signed-out") => ViewState::SignedOut,
                _ => ViewState::Ready,
            };
            if let Some(query) = argument("--query") {
                prototype.query = query;
                prototype.filter();
            }
            Ok(Box::new(Application {
                prototype,
                frames: 0,
                capture,
                screenshot_requested: false,
                smoke,
            }))
        }),
    )
}
struct Application {
    prototype: Prototype,
    frames: u32,
    capture: Option<String>,
    screenshot_requested: bool,
    smoke: bool,
}
impl eframe::App for Application {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.prototype.render(ui);
        self.frames += 1;
        if self.frames == 1 {
            eprintln!("native_preview_ready");
        }
        if let Some(path) = &self.capture {
            let screenshot = ui.input(|i| {
                i.events.iter().find_map(|e| {
                    if let egui::Event::Screenshot { image, .. } = e {
                        Some(image.clone())
                    } else {
                        None
                    }
                })
            });
            if let Some(image) = screenshot {
                let bytes: Vec<u8> = image.pixels.iter().flat_map(|p| p.to_array()).collect();

                match image::save_buffer(
                    path,
                    &bytes,
                    image.width() as u32,
                    image.height() as u32,
                    image::ColorType::Rgba8,
                ) {
                    Ok(()) => eprintln!(
                        "capture_saved width={} height={} rendered_rows={} frame_ms={:.2}",
                        image.width(),
                        image.height(),
                        self.prototype.rendered_rows,
                        self.prototype.last_frame_ms
                    ),
                    Err(_) => eprintln!("capture_failed"),
                }
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
            if self.frames >= 20 && !self.screenshot_requested {
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
                self.screenshot_requested = true;
            }
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(30));
        } else if self.smoke {
            if self.frames >= 3 {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            } else {
                ui.ctx().request_repaint();
            }
        }
    }
}
