//! Private library pages stay in a bounded session-only cache, separate from Account settings.
use super::*;
use crate::{song_artwork::SongArtwork, theme};
use std::collections::VecDeque;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum LibraryRoute {
    Liked,
    Songs,
    Albums,
    Playlists,
    Collection { id: String, title: String },
}
impl LibraryRoute {
    pub const ROOTS: [Self; 4] = [Self::Liked, Self::Songs, Self::Albums, Self::Playlists];
    pub fn id(&self) -> &str {
        match self {
            Self::Liked => "VLLM",
            Self::Songs => "FEmusic_liked_videos",
            Self::Albums => "FEmusic_liked_albums",
            Self::Playlists => "FEmusic_liked_playlists",
            Self::Collection { id, .. } => id,
        }
    }
    pub fn title(&self) -> &str {
        match self {
            Self::Liked => "Liked Music",
            Self::Songs => "Songs",
            Self::Albums => "Albums",
            Self::Playlists => "Playlists",
            Self::Collection { title, .. } => title,
        }
    }
}

struct CachedPage {
    id: String,
    page: Page,
    seen: HashSet<String>,
    loaded: bool,
    message: String,
}
#[derive(Default)]
pub(super) struct Library {
    pub(super) loaded: bool,
    pub(super) requested: bool,
    pub(super) message: String,
    filter: String,
    cache: VecDeque<CachedPage>,
    artwork: SongArtwork,
    #[cfg(any(test, debug_assertions))]
    preview: bool,
}
impl Library {
    pub(super) fn remove_unliked(&mut self, track: &Track, liked: bool) {
        if liked {
            // A like made elsewhere invalidates a cached liked list; don't invent its ordering.
            self.cache.retain(|cached| cached.id != "VLLM");
        } else {
            for cached in &mut self.cache {
                if cached.id == "VLLM" {
                    cached.page.tracks.retain(|t| t.id != track.id);
                }
            }
        }
    }
}
impl AccountUi {
    /// Authored visual fixtures only; no account handle or cookie is constructed.
    /// Excluded from production packages and reachable only in explicit debug captures/tests.
    #[cfg(any(test, debug_assertions))]
    pub(crate) fn preview_library(&mut self, ctx: &egui::Context) -> Vec<Track> {
        let titles = [
            "Glasshouse",
            "Night Transit",
            "Open Water",
            "Soft Focus",
            "Northern Lights",
            "After Hours",
            "Slow Motion",
            "Quiet Places",
            "Moon Garden",
            "Green Room",
            "Daybreak",
            "Still Here",
        ];
        let tracks: Vec<_> = titles
            .iter()
            .enumerate()
            .map(|(i, title)| Track {
                id: octave_core::TrackId(format!("preview-{i:03}")),
                title: (*title).into(),
                artists: vec![["Octave Studio", "Sage Ensemble", "The Night Room"][i % 3].into()],
            })
            .collect();
        self.library = Library {
            loaded: true,
            requested: true,
            preview: true,
            ..Default::default()
        };
        self.browse_id = LibraryRoute::Liked.id().into();
        self.page = Page {
            tracks: tracks.clone(),
            ..Default::default()
        };
        self.library.artwork.preview_covers(ctx, &tracks);
        for route in [
            LibraryRoute::Songs,
            LibraryRoute::Albums,
            LibraryRoute::Playlists,
        ] {
            self.library.cache.push_back(CachedPage {
                id: route.id().into(),
                page: if route == LibraryRoute::Songs {
                    self.page.clone()
                } else {
                    Page {
                        collections: (0..6)
                            .map(|i| octave_core::Collection {
                                id: format!("preview-collection-{i}"),
                                title: format!("{} collection {}", route.title(), i + 1),
                            })
                            .collect(),
                        ..Default::default()
                    }
                },
                seen: Default::default(),
                loaded: true,
                message: String::new(),
            });
        }
        tracks
    }
    fn library_connected(&self) -> bool {
        let connected = self.account.is_some();
        #[cfg(any(test, debug_assertions))]
        let connected = connected || self.library.preview;
        connected
    }
    pub fn select_library(&mut self, route: &LibraryRoute) {
        self.library.requested = true;
        if self.browse_id != route.id() {
            self.library.cache.push_back(CachedPage {
                id: std::mem::replace(&mut self.browse_id, route.id().into()),
                page: std::mem::take(&mut self.page),
                seen: std::mem::take(&mut self.seen),
                loaded: std::mem::take(&mut self.library.loaded),
                message: std::mem::take(&mut self.library.message),
            });
            if let Some(index) = self.library.cache.iter().position(|p| p.id == route.id()) {
                let cached = self.library.cache.remove(index).unwrap();
                self.page = cached.page;
                self.seen = cached.seen;
                self.library.loaded = cached.loaded;
                self.library.message = cached.message;
            }
            while self.library.cache.len() > 8 {
                self.library.cache.pop_front();
            }
            self.library.filter.clear();
        }
        self.load_library_if_needed();
    }
    pub(super) fn load_library_if_needed(&mut self) {
        if self.library.requested && !self.library.loaded && self.pending.is_none() {
            self.browse(self.browse_id.clone(), false);
        }
    }
    pub(super) fn accept_library_page(&mut self, id: &str, page: Page, more: bool) {
        if id == "VLLM" {
            for track in &page.tracks {
                self.ratings.insert(track.id.0.clone(), true);
            }
        }
        let target = if id == self.browse_id {
            self.library.loaded = true;
            self.library.message.clear();
            &mut self.page
        } else if let Some(cached) = self.library.cache.iter_mut().find(|p| p.id == id) {
            cached.loaded = true;
            cached.message.clear();
            &mut cached.page
        } else {
            // The destination was evicted while this bounded request was in flight.
            return;
        };
        if !more {
            *target = Page::default();
        }
        let mut ids: HashSet<_> = target.tracks.iter().map(|t| t.id.clone()).collect();
        target
            .tracks
            .extend(page.tracks.into_iter().filter(|t| ids.insert(t.id.clone())));
        let mut ids: HashSet<_> = target.collections.iter().map(|c| c.id.clone()).collect();
        target.collections.extend(
            page.collections
                .into_iter()
                .filter(|c| ids.insert(c.id.clone())),
        );
        target.continuation = page.continuation;
    }
    pub(super) fn library_error(&mut self, id: &str, more: bool, error: Error) {
        let (page, seen, message) = if id == self.browse_id {
            (&self.page, &mut self.seen, &mut self.library.message)
        } else if let Some(cached) = self.library.cache.iter_mut().find(|p| p.id == id) {
            (&cached.page, &mut cached.seen, &mut cached.message)
        } else {
            return;
        };
        if more && let Some(token) = &page.continuation {
            seen.remove(token);
        }
        *message = error.to_string();
    }
    pub fn library_settled(&self) -> bool {
        self.pending.is_none() && self.library.artwork.settled()
    }
    pub fn home_library(&mut self, ui: &mut egui::Ui) -> Option<Action> {
        let mut action = None;
        theme::eyebrow(ui, "YOUR MUSIC");
        ui.add_space(8.0);
        egui::Frame::new()
            .fill(ui.visuals().faint_bg_color)
            .corner_radius(12.0)
            .inner_margin(16.0)
            .show(ui, |ui| {
                ui.set_width((ui.available_width() - 1.0).max(0.0));
                ui.horizontal(|ui| {
                    liked_cover(
                        ui,
                        if ui.available_width() < 440.0 {
                            64.0
                        } else {
                            88.0
                        },
                    );
                    ui.add_space(12.0);
                    ui.vertical(|ui| {
                        if ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new("Liked Music").strong().size(22.0),
                                )
                                .frame(false),
                            )
                            .clicked()
                        {
                            action = Some(Action::OpenLibrary(LibraryRoute::Liked));
                        }
                        ui.weak(if self.library_connected() {
                            "Your favourites, together in your library"
                        } else {
                            "Connect your account to listen to your favourites"
                        });
                        if ui.small_button("Open Library →").clicked() {
                            action = Some(Action::OpenLibrary(LibraryRoute::Liked));
                        }
                    });
                });
            });
        ui.add_space(24.0);
        action
    }
    pub fn render_library(
        &mut self,
        ui: &mut egui::Ui,
        route: &LibraryRoute,
        current: Option<&Track>,
    ) -> Option<Action> {
        self.select_library(route);
        let mut action = None;
        let busy = self.pending.is_some();
        let loading = self.pending_browse.as_deref() == Some(route.id()) && busy;
        egui::ScrollArea::vertical()
            .id_salt(("music-library", route))
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(8.0);
                ui.heading("Library");
                ui.add_space(14.0);
                ui.horizontal_wrapped(|ui| {
                    for root in LibraryRoute::ROOTS {
                        if ui.add(egui::Button::new(root.title()).selected(root == *route).corner_radius(16.0)).clicked() {
                            action = Some(Action::OpenLibrary(root));
                        }
                    }
                    if ui.button("Local playlists").clicked() { action = Some(Action::LocalPlaylists); }
                });
                ui.add_space(24.0);
                if !self.library_connected() {
                    liked_cover(ui, 100.0);
                    ui.add_space(16.0);
                    ui.label(egui::RichText::new("Your music belongs here").strong().size(26.0));
                    ui.label("Connect YouTube Music to find your liked songs, saved albums and playlists in one place.");
                    ui.add_space(14.0);
                    if theme::primary(ui, "Connect account").clicked() { action = Some(Action::OpenAccount); }
                    ui.add_space(8.0);
                    ui.weak("Search, listening and local playlists are available without an account.");
                    return;
                }
                let compact = ui.available_width() < 560.0;
                ui.horizontal(|ui| {
                    if *route == LibraryRoute::Liked {
                        liked_cover(ui, if compact { 88.0 } else { 136.0 });
                        ui.add_space(if compact { 8.0 } else { 20.0 });
                    }
                    ui.vertical(|ui| {
                        theme::eyebrow(ui, if *route == LibraryRoute::Liked { "AUTO PLAYLIST · YOUTUBE MUSIC" } else { "YOUR YOUTUBE MUSIC" });
                        ui.add_space(6.0);
                        ui.add(egui::Label::new(egui::RichText::new(route.title()).strong().size(if compact { 26.0 } else { 38.0 })).wrap());
                        if *route == LibraryRoute::Liked && !compact { ui.weak("All the music you give a thumbs up."); }
                        ui.add_space(6.0);
                        let count = self.page.tracks.len();
                        let collections = self.page.collections.len();
                        let summary = if collections == 0 { format!("{count} songs") } else if count == 0 { format!("{collections} collections") } else { format!("{count} songs · {collections} collections") };
                        ui.weak(if loading { "Loading your music…".into() } else if self.page.continuation.is_some() { format!("{summary} loaded · more available") } else { summary });
                    });
                });
                ui.add_space(20.0);
                let filter = self.library.filter.trim().to_lowercase();
                let songs: Vec<_> = self.page.tracks.iter().enumerate().filter(|(_, t)| matches_filter(t, &filter)).map(|(i, _)| i).collect();
                let collections: Vec<_> = self.page.collections.iter().enumerate().filter(|(_, c)| c.title.to_lowercase().contains(&filter)).map(|(i, _)| i).collect();
                let partial = self.page.continuation.is_some();
                ui.horizontal_wrapped(|ui| {
                    if !self.page.tracks.is_empty() { ui.add_enabled_ui(!songs.is_empty(), |ui| {
                        if theme::primary(ui, if partial { "Play loaded" } else { "Play all" }).clicked() {
                            action = Some(Action::Play(songs.iter().map(|&i| self.page.tracks[i].clone()).collect(), 0));
                        }
                        if ui.button(if partial { "Shuffle loaded" } else { "Shuffle" }).clicked() {
                            action = Some(Action::Shuffle(songs.iter().map(|&i| self.page.tracks[i].clone()).collect()));
                        }
                        if ui.button("Add to queue").clicked() {
                            action = Some(Action::AppendAll(songs.iter().map(|&i| self.page.tracks[i].clone()).collect()));
                        }
                    }); }
                    if ui.add_enabled(!busy, egui::Button::new("Refresh")).clicked() { self.browse(route.id().into(), false); }
                    if loading { ui.spinner(); }
                });
                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    let response = ui.add(egui::TextEdit::singleline(&mut self.library.filter).id_salt("library-filter").hint_text("Filter loaded music").desired_width((ui.available_width() - 70.0).max(100.0)).margin(egui::vec2(12.0, 9.0)).char_limit(256));
                    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Filter loaded music"));
                    if ui.button("Clear").clicked() { self.library.filter.clear(); }
                });
                ui.add_space(12.0);
                if !self.library.message.is_empty() && !loading {
                    ui.label(&self.library.message);
                }
                if !loading && self.library.loaded && songs.is_empty() && collections.is_empty() {
                    ui.add_space(20.0);
                    ui.strong(if filter.is_empty() { "No music to show yet" } else { "No matches in loaded music" });
                    ui.weak(if filter.is_empty() { "Try another library tab or refresh this view." } else { "Try another title or artist, or clear the filter." });
                }
                if !collections.is_empty() {
                    let columns = ((ui.available_width() / 180.0).floor() as usize).clamp(1, 5);
                    let width = (ui.available_width() - (columns - 1) as f32 * 16.0) / columns as f32;
                    let rows = collections.len().div_ceil(columns);
                    let (rect, range) = visible_rows(ui, rows, width.min(176.0) + 62.0);
                    for row in range {
                        ui.scope_builder(egui::UiBuilder::new().max_rect(row_rect(rect, row, width.min(176.0) + 62.0)), |ui| {
                            ui.horizontal_top(|ui| {
                                ui.spacing_mut().item_spacing.x = 16.0;
                                for &index in collections.iter().skip(row * columns).take(columns) {
                                    let collection = &self.page.collections[index];
                                    ui.allocate_ui_with_layout(egui::vec2(width, width.min(176.0) + 52.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                                        ui.set_min_width(width);
                                        let cover = collection_cover(ui, width.min(176.0), matches!(route, LibraryRoute::Albums));
                                        cover.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Open {}", collection.title)));
                                        if ui.add(egui::Button::new(egui::RichText::new(&collection.title).strong()).frame(false).truncate()).on_hover_text(&collection.title).clicked() || cover.clicked() {
                                            action = Some(Action::OpenLibrary(LibraryRoute::Collection { id: collection.id.clone(), title: collection.title.clone() }));
                                        }
                                    });
                                }
                            });
                        });
                    }
                    ui.add_space(16.0);
                }
                if !songs.is_empty() {
                    ui.separator();
                    let (rect, range) = visible_rows(ui, songs.len(), 70.0);
                    let mut rating = None;
                    for row in range {
                        let index = songs[row];
                        let track = &self.page.tracks[index];
                        let selected = current.is_some_and(|t| t.id == track.id);
                        ui.push_id(&track.id.0, |ui| {
                            ui.scope_builder(egui::UiBuilder::new().max_rect(row_rect(rect, row, 70.0)), |ui| {
                                let hover = ui.rect_contains_pointer(ui.max_rect());
                                egui::Frame::new().fill(if selected || hover { ui.visuals().faint_bg_color } else { egui::Color32::TRANSPARENT }).corner_radius(6.0).inner_margin(8.0).show(ui, |ui| {
                                    ui.set_width((rect.width() - 16.0).max(0.0));
                                    ui.horizontal(|ui| {
                                        if song_play(ui, &track.title, selected).clicked() {
                                            action = Some(Action::Play(self.page.tracks.clone(), index));
                                        }
                                        self.library.artwork.show(ui, &track.id.0, &track.title, 44.0);
                                        ui.add_space(6.0);
                                        let width = (ui.available_width() - 60.0).max(40.0);
                                        ui.allocate_ui_with_layout(egui::vec2(width, 48.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                                            ui.set_min_size(egui::vec2(width, 48.0));
                                            ui.spacing_mut().item_spacing.y = 2.0;
                                            ui.spacing_mut().button_padding = egui::vec2(0.0, 0.0);
                                            ui.spacing_mut().interact_size.y = 22.0;
                                            if ui.add(egui::Button::new(egui::RichText::new(&track.title).strong().color(if selected { theme::ACCENT } else { ui.visuals().text_color() })).frame(false).truncate()).on_hover_text(&track.title).clicked() {
                                                action = Some(Action::Play(self.page.tracks.clone(), index));
                                            }
                                            ui.add(egui::Label::new(egui::RichText::new(track.artists.join(" · ")).weak()).truncate());
                                        });
                                        ui.menu_button("···", |ui| {
                                            if ui.button("Add to queue").clicked() { action = Some(Action::Append(track.clone())); ui.close(); }
                                            let liked = self.ratings.get(&track.id.0).copied().unwrap_or(false);
                                            if ui.add_enabled(!busy && self.verified, egui::Button::new(if liked { "Remove from liked music" } else { "Like song" })).clicked() { rating = Some((track.clone(), !liked)); ui.close(); }
                                        }).response.on_hover_text(format!("More options for {}", track.title)).widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("More options for {}", track.title)));
                                    });
                                });
                            });
                        });
                    }
                    if let Some((track, liked)) = rating { self.rate(track, liked); }
                }
                if partial {
                    ui.add_space(16.0);
                    if ui.add_enabled(!busy, egui::Button::new("Load more music")).clicked() { self.browse(route.id().into(), true); }
                }
                ui.add_space(20.0);
            });
        action
    }
}
fn matches_filter(track: &Track, filter: &str) -> bool {
    track.title.to_lowercase().contains(filter)
        || track
            .artists
            .iter()
            .any(|artist| artist.to_lowercase().contains(filter))
}
fn song_play(ui: &mut egui::Ui, title: &str, selected: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(36.0, 44.0), egui::Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Play {title}"))
    });
    if response.hovered() || response.has_focus() {
        ui.painter()
            .circle_stroke(rect.center(), 16.0, ui.visuals().widgets.hovered.fg_stroke);
    }
    let color = if selected {
        theme::ACCENT
    } else {
        ui.visuals().text_color()
    };
    let center = rect.center();
    if selected {
        for (index, height) in [10.0, 18.0, 14.0].into_iter().enumerate() {
            ui.painter().rect_filled(
                egui::Rect::from_center_size(
                    center + egui::vec2((index as f32 - 1.0) * 5.0, 0.0),
                    egui::vec2(3.0, height),
                ),
                1.0,
                color,
            );
        }
    } else {
        ui.painter().add(egui::Shape::convex_polygon(
            vec![
                center + egui::vec2(-5.0, -8.0),
                center + egui::vec2(8.0, 0.0),
                center + egui::vec2(-5.0, 8.0),
            ],
            color,
            egui::Stroke::NONE,
        ));
    }
    response.on_hover_text(format!("Play {title}"))
}
// One page scroller owns the header and virtualized rows, including compact/high-scale layouts.
fn visible_rows(
    ui: &mut egui::Ui,
    count: usize,
    stride: f32,
) -> (egui::Rect, std::ops::Range<usize>) {
    let (_, rect) = ui.allocate_space(egui::vec2(ui.available_width(), count as f32 * stride));
    let first = (((ui.clip_rect().top() - rect.top()) / stride)
        .floor()
        .max(0.0) as usize)
        .min(count);
    let last = (((ui.clip_rect().bottom() - rect.top()) / stride)
        .ceil()
        .max(0.0) as usize
        + 1)
    .min(count);
    (rect, first..last.max(first))
}
fn row_rect(rect: egui::Rect, row: usize, stride: f32) -> egui::Rect {
    egui::Rect::from_min_size(
        rect.min + egui::vec2(0.0, row as f32 * stride),
        egui::vec2(rect.width(), stride),
    )
}
pub(crate) fn liked_cover(ui: &mut egui::Ui, size: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(rect, 10.0, egui::Color32::from_rgb(57, 81, 65));
    painter.circle_filled(
        rect.center() + egui::vec2(size * 0.17, -size * 0.17),
        size * 0.30,
        egui::Color32::from_rgb(77, 107, 72),
    );
    painter.circle_filled(
        rect.center() + egui::vec2(-size * 0.22, size * 0.22),
        size * 0.26,
        egui::Color32::from_rgb(49, 70, 63),
    );
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "♥",
        egui::FontId::proportional(size * 0.5),
        theme::ACCENT,
    );
}
fn collection_cover(ui: &mut egui::Ui, size: f32, album: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::click());
    ui.painter()
        .rect_filled(rect, 8.0, ui.visuals().faint_bg_color);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        if album { "◎" } else { "♫" },
        egui::FontId::proportional(size * 0.48),
        theme::ACCENT,
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    fn deliver(account: &mut AccountUi, id: &str, page: Page, more: bool) {
        let (tx, rx) = mpsc::sync_channel(1);
        account.pending = Some(rx);
        account.pending_browse = Some(id.into());
        account.pending_generation = account.generation;
        tx.send(ResultMessage::Page(Ok(page), more)).unwrap();
        account.poll();
    }
    #[test]
    fn library_keeps_inflight_results_with_their_destination_and_clears_on_logout() {
        let mut account = AccountUi::default();
        let track = crate::listening::track(7);
        account.select_library(&LibraryRoute::Liked);
        account.select_library(&LibraryRoute::Albums);
        deliver(
            &mut account,
            "VLLM",
            Page {
                tracks: vec![track.clone()],
                continuation: Some("liked-next".into()),
                ..Default::default()
            },
            false,
        );
        assert!(
            account.page.tracks.is_empty(),
            "A late liked page must not appear in Albums"
        );
        account.select_library(&LibraryRoute::Liked);
        assert_eq!(account.page.tracks, vec![track.clone()]);
        assert_eq!(account.page.continuation.as_deref(), Some("liked-next"));
        assert!(
            account.pending.is_none(),
            "Revisiting cached music doesn't fetch or play"
        );
        account.select_library(&LibraryRoute::Songs);
        assert!(!account.library.cache.is_empty());
        let (tx, rx) = mpsc::sync_channel(1);
        account.pending = Some(rx);
        account.disconnect();
        tx.send(ResultMessage::Page(
            Ok(Page {
                tracks: vec![track],
                ..Default::default()
            }),
            false,
        ))
        .unwrap();
        account.poll();
        assert!(account.page.tracks.is_empty() && account.library.cache.is_empty());
        assert!(account.ratings.is_empty() && !account.library_connected());
    }
    #[test]
    fn library_cache_is_bounded_and_failed_pagination_can_retry_its_cursor() {
        let mut account = AccountUi::default();
        for index in 0..24 {
            account.select_library(&LibraryRoute::Collection {
                id: format!("fixture-{index}"),
                title: format!("Collection {index}"),
            });
            deliver(
                &mut account,
                &format!("fixture-{index}"),
                Page {
                    tracks: vec![crate::listening::track(index)],
                    continuation: Some("cursor".into()),
                    ..Default::default()
                },
                false,
            );
        }
        assert_eq!(account.library.cache.len(), 8);
        account.seen.insert("cursor".into());
        account.library_error("fixture-23", true, Error::Network);
        assert!(!account.seen.contains("cursor"));
        assert_eq!(account.page.continuation.as_deref(), Some("cursor"));
        assert_eq!(account.page.tracks.len(), 1);
    }
    #[test]
    fn signed_out_library_never_starts_signin_and_its_controls_fit_compact_widths() {
        for width in [280.0, 480.0, 1000.0] {
            let ctx = egui::Context::default();
            crate::theme::install(&ctx);
            ctx.enable_accesskit();
            let mut account = AccountUi::default();
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, 600.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    account.render_library(ui, &LibraryRoute::Liked, None);
                },
            );
            output.textures_delta.clear();
            let tree = output.platform_output.accesskit_update.unwrap();
            assert!(
                tree.nodes
                    .iter()
                    .any(|(_, n)| n.label() == Some("Connect account"))
            );
            assert!(
                account.account.is_none() && account.pending.is_none() && !account.auth.pending
            );
        }
    }
    #[test]
    fn filter_and_partial_queue_controls_do_not_claim_to_play_an_entire_library() {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        ctx.enable_accesskit();
        let mut account = AccountUi::default();
        account.preview_library(&ctx);
        account.page.continuation = Some("next".into());
        account.library.filter = "Sage".into();
        assert_eq!(
            account
                .page
                .tracks
                .iter()
                .filter(|t| matches_filter(t, "sage"))
                .count(),
            4
        );
        for _ in 0..2 {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(480.0, 800.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    account.render_library(ui, &LibraryRoute::Liked, None);
                },
            );
            output.textures_delta.clear();
            let tree = output.platform_output.accesskit_update.unwrap();
            assert!(
                tree.nodes
                    .iter()
                    .any(|(_, n)| n.label() == Some("Play loaded"))
            );
            assert!(
                !tree
                    .nodes
                    .iter()
                    .any(|(_, n)| n.label() == Some("Play all"))
            );
        }
        assert!(account.pending.is_none() && account.account.is_none());
    }
}
