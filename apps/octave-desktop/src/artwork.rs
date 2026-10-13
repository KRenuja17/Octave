//! Original artwork is decoded and resized on one bounded background worker.
use eframe::egui::{self, ColorImage, TextureHandle};
use std::sync::mpsc::{self, Receiver};
const COVERS: [&[u8]; 6] = [
    include_bytes!("../assets/cover-0.png"),
    include_bytes!("../assets/cover-1.png"),
    include_bytes!("../assets/cover-2.png"),
    include_bytes!("../assets/cover-3.png"),
    include_bytes!("../assets/cover-4.png"),
    include_bytes!("../assets/cover-5.png"),
];
pub struct Artwork {
    textures: [Option<TextureHandle>; 6],
    results: Receiver<(usize, ColorImage)>,
}
impl Artwork {
    pub fn ready(&self) -> bool {
        self.textures.iter().all(Option::is_some)
    }
    pub fn new(ctx: egui::Context) -> Self {
        let (tx, results) = mpsc::sync_channel(2);
        std::thread::spawn(move || {
            for (index, bytes) in COVERS.iter().enumerate() {
                if let Ok(image) =
                    image::load_from_memory_with_format(bytes, image::ImageFormat::Png)
                {
                    let rgba = image
                        .resize_exact(320, 320, image::imageops::FilterType::Triangle)
                        .into_rgba8();
                    let image = ColorImage::from_rgba_unmultiplied([320, 320], &rgba);
                    if tx.send((index, image)).is_err() {
                        break;
                    }
                    ctx.request_repaint();
                }
            }
        });
        Self {
            textures: std::array::from_fn(|_| None),
            results,
        }
    }
    pub fn poll(&mut self, ctx: &egui::Context) {
        // At most two uploads per frame; six textures total (~2.4 MiB RGBA).
        for _ in 0..2 {
            let Ok((index, image)) = self.results.try_recv() else {
                break;
            };
            self.textures[index] = Some(ctx.load_texture(
                format!("cover-{index}"),
                image,
                egui::TextureOptions::LINEAR,
            ));
        }
    }
    pub fn show(&self, ui: &mut egui::Ui, index: usize, size: f32) {
        if let Some(texture) = &self.textures[index % 6] {
            ui.add(
                egui::Image::new(texture)
                    .fit_to_exact_size(egui::vec2(size, size))
                    .corner_radius(8.0)
                    .alt_text(format!(
                        "Cover for {}",
                        crate::model::COLLECTIONS[index % 6].title
                    )),
            );
        } else {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
            ui.painter()
                .rect_filled(rect, 8.0, egui::Color32::from_rgb(54, 68, 71));
        }
    }
}
