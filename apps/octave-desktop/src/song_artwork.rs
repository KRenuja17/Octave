//! One worker, eight pending requests, 96 small and eight Focus textures.
use eframe::egui::{self, ColorImage, TextureHandle};
use octave_core::TrackId;
use std::{
    collections::{HashSet, VecDeque},
    sync::mpsc::{self, Receiver, SyncSender},
};

pub struct SongArtwork {
    requests: SyncSender<(String, bool, egui::Context)>,
    results: Receiver<((String, bool), Option<ColorImage>)>,
    pending: HashSet<(String, bool)>,
    cache: VecDeque<((String, bool), Option<TextureHandle>)>,
}
impl Default for SongArtwork {
    fn default() -> Self {
        let (requests, work) = mpsc::sync_channel::<(String, bool, egui::Context)>(8);
        let (done, results) = mpsc::sync_channel(8);
        std::thread::spawn(move || {
            let provider = if cfg!(test) {
                None
            } else {
                octave_provider::Provider::new().ok()
            };
            while let Ok((id, large, ctx)) = work.recv() {
                let image = provider
                    .as_ref()
                    .and_then(|p| {
                        if large {
                            p.thumbnail_large(&TrackId(id.clone())).ok()
                        } else {
                            p.thumbnail(&TrackId(id.clone())).ok()
                        }
                    })
                    .and_then(|bytes| decode_tier(&bytes, if large { 512 } else { 160 }));
                if done.send(((id, large), image)).is_err() {
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
fn decode_tier(bytes: &[u8], edge: u32) -> Option<ColorImage> {
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
        .resize_to_fill(edge, edge, image::imageops::FilterType::Triangle)
        .into_rgba8();
    Some(ColorImage::from_rgba_unmultiplied(
        [edge as usize, edge as usize],
        &rgba,
    ))
}
impl SongArtwork {
    pub fn settled(&self) -> bool {
        self.pending.is_empty()
    }
    pub fn show(&mut self, ui: &mut egui::Ui, id: &str, title: &str, size: f32) {
        let large = size > 180.0;
        let requested = (id.to_owned(), large);
        for _ in 0..2 {
            let Ok((key, image)) = self.results.try_recv() else {
                break;
            };
            self.pending.remove(&key);
            let texture = image.map(|image| {
                ui.ctx().load_texture(
                    format!("song-{}-{}", key.0, key.1),
                    image,
                    egui::TextureOptions::LINEAR,
                )
            });
            let limit = if key.1 { 8 } else { 96 };
            if self.cache.iter().filter(|entry| entry.0.1 == key.1).count() >= limit
                && let Some(old) = self.cache.iter().position(|entry| entry.0.1 == key.1)
            {
                self.cache.remove(old);
            }
            self.cache.push_back((key, texture));
        }
        if let Some(index) = self.cache.iter().position(|(key, _)| key == &requested) {
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
                && !self.pending.contains(&requested)
                && self
                    .requests
                    .try_send((id.to_owned(), large, ui.ctx().clone()))
                    .is_ok()
            {
                self.pending.insert(requested);
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
        assert!(decode_tier(b"not an image", 160).is_none());
        let cover = decode_tier(include_bytes!("../assets/cover-0.png"), 160).unwrap();
        assert_eq!(cover.size, [160, 160]);
        assert_eq!(
            decode_tier(include_bytes!("../assets/cover-0.png"), 512)
                .unwrap()
                .size,
            [512, 512]
        );
        let mut encoded = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(2049, 1)
            .write_to(&mut encoded, image::ImageFormat::Png)
            .unwrap();
        assert!(decode_tier(encoded.get_ref(), 160).is_none());
        let mut jpeg = std::io::Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(480, 360)
            .write_to(&mut jpeg, image::ImageFormat::Jpeg)
            .unwrap();
        assert_eq!(decode_tier(jpeg.get_ref(), 160).unwrap().size, [160, 160]);
    }
}
