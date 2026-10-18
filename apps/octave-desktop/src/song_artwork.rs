//! Visible-song artwork: one worker, eight pending requests, 96 cached textures.
use eframe::egui::{self, ColorImage, TextureHandle};
use octave_core::TrackId;
use std::{
    collections::{HashSet, VecDeque},
    sync::mpsc::{self, Receiver, SyncSender},
};

pub struct SongArtwork {
    requests: SyncSender<(String, egui::Context)>,
    results: Receiver<(String, Option<ColorImage>)>,
    pending: HashSet<String>,
    cache: VecDeque<(String, Option<TextureHandle>)>,
}
impl Default for SongArtwork {
    fn default() -> Self {
        let (requests, work) = mpsc::sync_channel::<(String, egui::Context)>(8);
        let (done, results) = mpsc::sync_channel(8);
        std::thread::spawn(move || {
            let provider = if cfg!(test) {
                None
            } else {
                octave_provider::Provider::new().ok()
            };
            while let Ok((id, ctx)) = work.recv() {
                let image = provider
                    .as_ref()
                    .and_then(|p| p.thumbnail(&TrackId(id.clone())).ok())
                    .and_then(|bytes| decode(&bytes));
                if done.send((id, image)).is_err() {
                    break;
                }
                ctx.request_repaint();
            }
        });
        Self {
            requests,
            results,
            pending: HashSet::new(),
            cache: VecDeque::new(),
        }
    }
}
fn decode(bytes: &[u8]) -> Option<ColorImage> {
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(2048);
    limits.max_image_height = Some(2048);
    limits.max_alloc = Some(32 * 1024 * 1024);
    reader.limits(limits);
    let rgba = reader
        .decode()
        .ok()?
        .resize_to_fill(160, 160, image::imageops::FilterType::Triangle)
        .into_rgba8();
    Some(ColorImage::from_rgba_unmultiplied([160, 160], &rgba))
}
impl SongArtwork {
    pub fn settled(&self) -> bool {
        self.pending.is_empty()
    }
    pub fn show(&mut self, ui: &mut egui::Ui, id: &str, title: &str, size: f32) {
        for _ in 0..2 {
            let Ok((key, image)) = self.results.try_recv() else {
                break;
            };
            self.pending.remove(&key);
            let texture = image.map(|image| {
                ui.ctx()
                    .load_texture(format!("song-{key}"), image, egui::TextureOptions::LINEAR)
            });
            if self.cache.len() == 96 {
                self.cache.pop_front();
            }
            self.cache.push_back((key, texture));
        }
        if let Some(index) = self.cache.iter().position(|(key, _)| key == id) {
            let entry = self.cache.remove(index).expect("located artwork");
            if let Some(texture) = &entry.1 {
                ui.add(
                    egui::Image::new(texture)
                        .fit_to_exact_size(egui::vec2(size, size))
                        .corner_radius(3.0)
                        .alt_text(format!("Artwork for {title}")),
                );
            } else {
                crate::theme::cover(ui, size);
            }
            self.cache.push_back(entry);
        } else {
            if self.pending.len() < 8
                && !self.pending.contains(id)
                && self
                    .requests
                    .try_send((id.to_owned(), ui.ctx().clone()))
                    .is_ok()
            {
                self.pending.insert(id.to_owned());
            }
            crate::theme::cover(ui, size);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn malformed_artwork_falls_back_and_fixture_is_square() {
        assert!(decode(b"not an image").is_none());
        let cover = decode(include_bytes!("../assets/cover-0.png")).unwrap();
        assert_eq!(cover.size, [160, 160]);
        let mut encoded = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(2049, 1)
            .write_to(&mut encoded, image::ImageFormat::Png)
            .unwrap();
        assert!(decode(encoded.get_ref()).is_none());
        let mut jpeg = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(480, 360)
            .write_to(&mut jpeg, image::ImageFormat::Jpeg)
            .unwrap();
        assert_eq!(decode(jpeg.get_ref()).unwrap().size, [160, 160]);
    }
}
