#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

mod account_ui;
mod alpha;
mod artwork;
mod catalog_ui;
mod desktop;
mod discord;
mod listening;
mod live;
mod lyrics_ui;
mod model;
mod output_ui;
mod paths;
mod playlist_import;
mod playlist_ui;
mod playlists;
mod prototype;
mod related_ui;
mod request_cancel;
mod search;
mod shell;
mod song_artwork;
mod storage;
mod theme;
mod translation_ui;
#[cfg(test)]
mod ui_tests;
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
    if !std::env::args().any(|s| s == "--prototype") {
        return alpha::run();
    }
    let compact = std::env::args().any(|s| s == "--compact");
    let capture = argument("--capture");
    let scale = argument("--scale")
        .and_then(|v| v.parse::<f32>().ok())
        .map(|scale| scale.clamp(1.0, 2.0));
    let smoke = std::env::args().any(|s| s == "--smoke-test");
    eframe::run_native(
        "Octave",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size(if compact {
                    [560.0, 720.0]
                } else {
                    [1180.0, 760.0]
                })
                .with_min_inner_size([480.0, 480.0]),
            ..Default::default()
        },
        Box::new(move |cc| {
            let mut prototype = Prototype::new(&cc.egui_ctx);
            if let Some(scale) = scale {
                cc.egui_ctx.set_pixels_per_point(scale);
            }
            prototype.page = match argument("--page").as_deref() {
                Some("search") => Page::Search,
                Some("library") => Page::Library,
                Some("album") => Page::Collection(
                    argument("--collection")
                        .and_then(|v| v.parse::<usize>().ok())
                        .filter(|i| *i < 6)
                        .unwrap_or(0),
                ),
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
                capture_scroll: std::env::args().any(|s| s == "--capture-scroll"),
                smoke,
                stress_seconds: argument("--stress-seconds")
                    .and_then(|s| s.parse::<u64>().ok())
                    .map(|s| s.clamp(1, 3600)),
                started: std::time::Instant::now(),
            }))
        }),
    )
}
struct Application {
    stress_seconds: Option<u64>,
    started: std::time::Instant,
    prototype: Prototype,
    frames: u32,
    capture: Option<String>,
    screenshot_requested: bool,
    capture_scroll: bool,
    smoke: bool,
}
impl eframe::App for Application {
    fn raw_input_hook(&mut self, _ctx: &egui::Context, input: &mut egui::RawInput) {
        if self.stress_seconds.is_some()
            || (self.capture.is_some() && self.capture_scroll && (25..32).contains(&self.frames))
        {
            let rect = input.screen_rect.unwrap_or(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(350.0, 450.0),
            ));
            input.events.push(egui::Event::PointerMoved(rect.center()));
            input.events.push(egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, -100.0),
                phase: egui::TouchPhase::Move,
                modifiers: egui::Modifiers::NONE,
            });
        }
    }
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if let Some(seconds) = self.stress_seconds {
            self.prototype.page = match (self.started.elapsed().as_secs() / 5) % 9 {
                0 => Page::Home,
                1 => Page::Library,
                2 => Page::Search,
                value => Page::Collection((value - 3) as usize),
            };
            if self.started.elapsed().as_secs() >= seconds {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(16));
        }
        self.prototype.render(ui);
        self.frames += 1;
        if self.frames == 1 {
            eprintln!("native_preview_ready");
        }
        if let Some(path) = &self.capture {
            let screenshot = ui.ctx().data_mut(|data| {
                data.remove_temp::<std::sync::Arc<egui::ColorImage>>(egui::Id::unique(
                    "octave.capture.image",
                ))
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
            if self.frames >= 60 && self.prototype.artwork_ready() && !self.screenshot_requested {
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::Screenshot(
                        egui::ScreenshotCallback::new({
                            let ctx = ui.ctx().clone();
                            move |image| {
                                ctx.data_mut(|data| {
                                    data.insert_temp(
                                        egui::Id::unique("octave.capture.image"),
                                        image,
                                    )
                                });
                                ctx.request_repaint();
                            }
                        }),
                    ));
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
