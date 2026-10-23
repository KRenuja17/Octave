//! One coalescing unsigned worker and a bounded memory cache for public browsing.
use eframe::egui;
use octave_core::{Collection, Track, TrackId};
use octave_provider::{Error, Provider, catalog::CatalogPage};
use std::sync::{
    Arc, Condvar, Mutex,
    mpsc::{self, Receiver},
};
enum Request {
    Page(String, Option<String>),
    Suggest(String),
    Order(String, bool),
}
enum Reply {
    Page(Box<CatalogPage>),
    Suggest(Vec<String>),
    Order(Option<TrackId>, Vec<Track>),
}
type Pending = Arc<(Mutex<(bool, Option<(u64, Request)>)>, Condvar)>;
struct Worker {
    pending: Pending,
    results: Receiver<(u64, Result<Reply, Error>)>,
}
impl Worker {
    fn new() -> Self {
        let pending: Pending = Arc::new((Mutex::new((false, None)), Condvar::new()));
        let work = pending.clone();
        let (tx, results) = mpsc::channel();
        std::thread::spawn(move || {
            loop {
                let (lock, signal) = &*work;
                let mut state = lock.lock().unwrap();
                while !state.0 && state.1.is_none() {
                    state = signal.wait(state).unwrap();
                }
                if state.0 {
                    break;
                }
                let (generation, request) = state.1.take().unwrap();
                drop(state);
                let result = if cfg!(test) {
                    Err(Error::Unavailable)
                } else {
                    Provider::new().and_then(|p| match request {
                        Request::Page(id, token) => p
                            .catalog(&id, token.as_deref())
                            .map(|page| Reply::Page(Box::new(page))),
                        Request::Suggest(query) => p.suggestions(&query).map(Reply::Suggest),
                        Request::Order(id, radio) => p
                            .service_order(&id, radio)
                            .map(|tracks| Reply::Order(radio.then_some(TrackId(id)), tracks)),
                    })
                };
                if tx.send((generation, result)).is_err() {
                    break;
                }
            }
        });
        Self { pending, results }
    }
    fn send(&self, generation: u64, request: Request) {
        self.pending.0.lock().unwrap().1 = Some((generation, request));
        self.pending.1.notify_one();
    }
    fn cancel_pending(&self, generation: u64) {
        let mut pending = self.pending.0.lock().unwrap();
        if pending.1.as_ref().is_some_and(|(id, _)| *id == generation) {
            pending.1 = None;
        }
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.pending.0.lock().unwrap().0 = true;
        self.pending.1.notify_one();
    }
}
pub enum Action {
    Append(Track, bool),
    Play(Track),
    Pin(Collection),
    Import(String),
    Replace(Vec<Track>),
    Retune(TrackId, Vec<Track>),
    ClearHistory,
}
pub struct CatalogUi {
    pub route: String,
    active: String,
    worker: Worker,
    generation: u64,
    busy: bool,
    started: Option<std::time::Instant>,
    page: Option<CatalogPage>,
    cache: std::collections::VecDeque<(String, CatalogPage)>,
    filter: String,
    kind: u8,
    query: String,
    suggestions: Vec<String>,
    message: String,
    artwork: crate::song_artwork::SongArtwork,
    filter_signature: Option<(String, String, u8, usize, usize)>,
    song_indices: Vec<usize>,
    collection_indices: Vec<usize>,
    cursors: std::collections::HashSet<String>,
}
impl Default for CatalogUi {
    fn default() -> Self {
        Self {
            route: "FEmusic_home".into(),
            active: String::new(),
            worker: Worker::new(),
            generation: 0,
            busy: false,
            started: None,
            page: None,
            cache: Default::default(),
            filter: String::new(),
            kind: 0,
            query: String::new(),
            suggestions: Vec::new(),
            message: String::new(),
            artwork: Default::default(),
            filter_signature: None,
            song_indices: Vec::new(),
            collection_indices: Vec::new(),
            cursors: Default::default(),
        }
    }
}
impl CatalogUi {
    pub fn settled(&self) -> bool {
        !self.busy && self.artwork.settled() && self.active == self.route
    }
    pub fn at(route: String) -> Self {
        Self {
            route,
            ..Self::default()
        }
    }
    fn submit(&mut self, request: Request) {
        self.generation = self.generation.wrapping_add(1);
        self.busy = true;
        self.started = Some(std::time::Instant::now());
        self.message.clear();
        self.worker.send(self.generation, request);
    }
    fn expire_request(&mut self) {
        if self.busy
            && self
                .started
                .is_some_and(|started| started.elapsed() >= std::time::Duration::from_secs(15))
        {
            // A platform resolver/body read can outlive the transport's timeout.
            // Keep one worker, retract queued work, and reject any late result.
            self.worker.cancel_pending(self.generation);
            self.generation = self.generation.wrapping_add(1);
            self.busy = false;
            self.started = None;
            self.message = "Public catalog request timed out. Retry when the connection is ready; the queue is unchanged.".into();
        }
    }
    pub fn render(
        &mut self,
        ui: &mut egui::Ui,
        history: &[Track],
        pins: &[Collection],
        current: Option<&Track>,
    ) -> Option<Action> {
        let mut action = None;
        if self.active != self.route {
            self.generation = self.generation.wrapping_add(1);
            self.busy = false;
            self.started = None;
            self.page = self
                .cache
                .iter()
                .find(|(id, _)| id == &self.route)
                .map(|(_, page)| page.clone());
            self.active = self.route.clone();
            self.filter.clear();
            self.filter_signature = None;
            self.cursors.clear();
            self.message.clear();
            if !self.route.starts_with("local:") && self.page.is_none() {
                self.submit(Request::Page(self.route.clone(), None));
            }
        }
        self.expire_request();
        while let Ok((generation, result)) = self.worker.results.try_recv() {
            if generation != self.generation {
                continue;
            }
            self.busy = false;
            self.started = None;
            match result {
                Ok(Reply::Page(page)) => {
                    self.filter_signature = None;
                    let mut page = *page;
                    if page
                        .page
                        .continuation
                        .as_ref()
                        .is_some_and(|token| self.cursors.contains(token))
                    {
                        page.page.continuation = None;
                        self.message = "The service repeated a page cursor. Refresh or open a specific collection.".into();
                    }
                    if let Some(existing) = &self.page {
                        let mut tracks = existing.page.tracks.clone();
                        let mut ids: std::collections::HashSet<_> =
                            tracks.iter().map(|t| t.id.clone()).collect();
                        tracks.extend(
                            page.page
                                .tracks
                                .into_iter()
                                .filter(|t| ids.insert(t.id.clone())),
                        );
                        page.page.tracks = tracks;
                        let mut collections = existing.page.collections.clone();
                        let mut ids: std::collections::HashSet<_> =
                            collections.iter().map(|c| c.id.clone()).collect();
                        collections.extend(
                            page.page
                                .collections
                                .into_iter()
                                .filter(|c| ids.insert(c.id.clone())),
                        );
                        page.page.collections = collections;
                        page.title = existing.title.clone();
                        if page.description.is_empty() {
                            page.description = existing.description.clone();
                        }
                        if page.artwork.is_none() {
                            page.artwork = existing.artwork.clone();
                        }
                        let mut details = existing.details.clone();
                        details.extend(page.details);
                        page.details = details;
                        let mut covers = existing.covers.clone();
                        covers.extend(page.covers);
                        page.covers = covers;
                    }
                    if page.page.tracks.len() > 5000 || page.page.collections.len() > 1000 {
                        self.message = "Catalog limit reached; use a specific collection or complete playlist import.".into();
                    } else {
                        self.page = Some(page.clone());
                        self.cache.retain(|(id, _)| id != &self.route);
                        if self.cache.len() == 8 {
                            self.cache.pop_front();
                        }
                        self.cache.push_back((self.route.clone(), page));
                    }
                }
                Ok(Reply::Suggest(values)) => self.suggestions = values,
                Ok(Reply::Order(source, tracks)) => {
                    action = Some(match source {
                        Some(id) => Action::Retune(id, tracks),
                        None => Action::Replace(tracks),
                    })
                }
                Err(error) => {
                    if crate::argument("--capture").is_some() {
                        eprintln!("catalog_request_failed={error:?}");
                    }
                    self.message = match error {
                        Error::Timeout => "Public catalog request timed out. Retry when the connection is ready.",
                        Error::InvalidResponse => "This public page format is unavailable. Try Explore or search.",
                        _ => "Public catalog unavailable. Check the connection and retry; your queue is unchanged.",
                    }.into();
                }
            }
        }
        let compact = ui.available_width() < 650.0;
        let destinations = [
            ("Home", "FEmusic_home"),
            ("Explore", "FEmusic_explore"),
            ("Charts", "FEmusic_charts"),
            ("Moods & genres", "FEmusic_moods_and_genres"),
            ("History", "local:history"),
            ("Pinned", "local:pinned"),
        ];
        if compact {
            egui::ComboBox::from_label("Browse")
                .selected_text(
                    destinations
                        .iter()
                        .find(|(_, id)| *id == self.route)
                        .map_or("Collection", |(name, _)| *name),
                )
                .show_ui(ui, |ui| {
                    for (name, id) in destinations {
                        ui.selectable_value(&mut self.route, id.into(), name);
                    }
                });
        } else {
            ui.horizontal_wrapped(|ui| {
                for (name, id) in destinations {
                    if ui.selectable_label(self.route == id, name).clicked() {
                        self.route = id.into();
                    }
                }
            });
        }
        ui.horizontal_wrapped(|ui| {
            let input = ui.add(
                egui::TextEdit::singleline(&mut self.query)
                    .hint_text("Search songs, albums, artists and playlists")
                    .char_limit(512)
                    .desired_width(if compact {
                        (ui.available_width() - 200.0).clamp(120.0, 320.0)
                    } else {
                        320.0
                    }),
            );
            if (ui
                .button(if compact { "Search" } else { "Search catalog" })
                .clicked()
                || (input.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))))
                && !self.query.trim().is_empty()
            {
                self.route = format!("search:{}", self.query.trim());
                self.suggestions.clear();
            }
            if ui
                .add_enabled(
                    !self.busy && !self.query.trim().is_empty(),
                    egui::Button::new("Suggestions"),
                )
                .clicked()
            {
                self.submit(Request::Suggest(self.query.clone()));
            }
        });
        if !self.suggestions.is_empty() {
            ui.horizontal_wrapped(|ui| {
                for query in &self.suggestions {
                    if ui.small_button(query).clicked() {
                        self.route = format!("search:{query}");
                        self.query = query.clone();
                    }
                }
            });
        }
        if self.route != self.active {
            ui.ctx().request_repaint();
            return action;
        }
        if self.route == "local:history" {
            ui.heading("Listening history");
            ui.weak("Songs actually consumed by Octave, stored only on this computer.");
            if ui.button("Clear listening history").clicked() {
                action = Some(Action::ClearHistory);
            }
            self.song_rows(
                ui,
                history,
                &(0..history.len()).collect::<Vec<_>>(),
                None,
                &mut action,
            );
            return action;
        }
        if self.route == "local:pinned" {
            ui.heading("Pinned collections");
            self.collection_rows(ui, &pins.iter().collect::<Vec<_>>(), None);
            return action;
        }
        if self.route == "FEmusic_home" && !history.is_empty() {
            ui.heading("Listen again");
            ui.horizontal_wrapped(|ui| {
                for track in history.iter().take(6) {
                    ui.vertical(|ui| {
                        self.artwork.show(ui, &track.id.0, &track.title, 80.0);
                        if ui.small_button(&track.title).clicked() {
                            action = Some(Action::Play(track.clone()));
                        }
                    });
                }
            });
        }
        if let Some(page) = &self.page {
            let title = page.title.clone();
            let description = page.description.clone();
            let artwork = page.artwork.clone();
            ui.add_space(12.0);
            ui.horizontal_top(|ui| {
                if let Some(url) = artwork {
                    self.artwork.show(ui, &url, &title, 96.0);
                }
                ui.vertical(|ui| {
                    ui.heading(&title);
                    if !description.is_empty() {
                        ui.weak(description);
                    }
                    ui.weak(format!(
                        "{} loaded songs · {} collections",
                        page.page.tracks.len(),
                        page.page.collections.len()
                    ));
                });
            });
            if !self.route.starts_with("FE") && !self.route.starts_with("search:") {
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .button(if pins.iter().any(|c| c.id == self.route) {
                            "Unpin collection"
                        } else {
                            "Pin collection"
                        })
                        .clicked()
                    {
                        action = Some(Action::Pin(Collection {
                            id: self.route.clone(),
                            title,
                        }));
                    }
                    if self.route.starts_with("VL") {
                        if ui.button("Load complete playlist").clicked() {
                            action = Some(Action::Import(self.route.clone()));
                        }
                        if ui
                            .add_enabled(
                                !self.busy,
                                egui::Button::new("Load service shuffle batch"),
                            )
                            .clicked()
                        {
                            self.submit(Request::Order(self.route.clone(), false));
                        }
                    }
                });
            }
        }
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.filter)
                    .hint_text("Filter loaded results")
                    .char_limit(256)
                    .desired_width(if compact {
                        (ui.available_width() - 160.0).clamp(120.0, 320.0)
                    } else {
                        280.0
                    }),
            );
            let kinds = ["All", "Songs", "Albums", "Artists", "Playlists"];
            if compact {
                egui::ComboBox::from_id_salt("catalog-kind")
                    .selected_text(kinds[self.kind as usize])
                    .show_ui(ui, |ui| {
                        for (index, name) in kinds.iter().enumerate() {
                            ui.selectable_value(&mut self.kind, index as u8, *name);
                        }
                    });
            } else {
                for (index, name) in kinds.iter().enumerate() {
                    ui.selectable_value(&mut self.kind, index as u8, *name);
                }
            }
        });
        if let Some(current) = current
            && ui
                .add_enabled(
                    !self.busy,
                    egui::Button::new("Start mix · replace upcoming recommendations"),
                )
                .clicked()
        {
            self.submit(Request::Order(current.id.0.clone(), true));
        }
        if self.busy {
            ui.spinner();
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }
        if !self.message.is_empty() {
            ui.weak(&self.message);
        }
        if ui
            .add_enabled(
                !self.busy,
                egui::Button::new(if compact {
                    "Refresh"
                } else {
                    "Refresh public page"
                }),
            )
            .clicked()
        {
            self.page = None;
            self.cursors.clear();
            self.submit(Request::Page(self.route.clone(), None));
        }
        if let Some(page) = self.page.take() {
            let signature = (
                self.route.clone(),
                self.filter.clone(),
                self.kind,
                page.page.tracks.len(),
                page.page.collections.len(),
            );
            if self.filter_signature.as_ref() != Some(&signature) {
                let query = self.filter.to_lowercase();
                self.song_indices = page
                    .page
                    .tracks
                    .iter()
                    .enumerate()
                    .filter_map(|(i, t)| {
                        (query.is_empty()
                            || t.title.to_lowercase().contains(&query)
                            || t.artists.iter().any(|a| a.to_lowercase().contains(&query)))
                        .then_some(i)
                    })
                    .collect();
                self.collection_indices = page
                    .page
                    .collections
                    .iter()
                    .enumerate()
                    .filter_map(|(i, c)| {
                        ((query.is_empty() || c.title.to_lowercase().contains(&query))
                            && match self.kind {
                                2 => c.id.starts_with("MPRE"),
                                3 => c.id.starts_with("UC"),
                                4 => c.id.starts_with("VL"),
                                _ => true,
                            })
                        .then_some(i)
                    })
                    .collect();
                self.filter_signature = Some(signature);
            }
            if self.kind != 1 {
                let collections: Vec<_> = self
                    .collection_indices
                    .iter()
                    .map(|i| &page.page.collections[*i])
                    .collect();
                self.collection_rows(ui, &collections, Some(&page));
            }
            if self.kind <= 1 {
                let indices = std::mem::take(&mut self.song_indices);
                self.song_rows(ui, &page.page.tracks, &indices, Some(&page), &mut action);
                self.song_indices = indices;
            }
            if let Some(token) = &page.page.continuation
                && ui
                    .add_enabled(!self.busy, egui::Button::new("Load more catalog results"))
                    .clicked()
            {
                self.cursors.insert(token.clone());
                self.submit(Request::Page(self.route.clone(), Some(token.clone())));
            }
            self.page = Some(page);
        }
        action
    }
    fn collection_rows(
        &mut self,
        ui: &mut egui::Ui,
        collections: &[&Collection],
        page: Option<&CatalogPage>,
    ) {
        egui::ScrollArea::horizontal()
            .id_salt(("catalog-collections", &self.route))
            .max_height(220.0)
            .show_viewport(ui, |ui, viewport| {
                const STRIDE: f32 = 158.0;
                ui.set_min_width(collections.len() as f32 * STRIDE);
                let start = (viewport.min.x / STRIDE).floor().max(0.0) as usize;
                let end = ((viewport.max.x / STRIDE).ceil() as usize + 1).min(collections.len());
                ui.spacing_mut().item_spacing.x = 10.0;
                ui.horizontal_top(|ui| {
                    ui.add_space(start as f32 * STRIDE);
                    for collection in collections.iter().take(end).skip(start) {
                        ui.allocate_ui_with_layout(
                            egui::vec2(148.0, 200.0),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                if let Some(url) = page.and_then(|p| p.covers.get(&collection.id)) {
                                    self.artwork.show(ui, url, &collection.title, 148.0);
                                } else {
                                    crate::theme::cover(ui, 148.0);
                                }
                                if ui
                                    .add_sized(
                                        [148.0, 36.0],
                                        egui::Button::new(&collection.title)
                                            .truncate()
                                            .frame(false),
                                    )
                                    .on_hover_text(&collection.title)
                                    .clicked()
                                {
                                    self.route = collection.id.clone();
                                }
                            },
                        );
                    }
                });
            });
    }
    fn song_rows(
        &mut self,
        ui: &mut egui::Ui,
        tracks: &[Track],
        indices: &[usize],
        page: Option<&CatalogPage>,
        action: &mut Option<Action>,
    ) {
        egui::ScrollArea::vertical()
            .id_salt(("catalog-songs", &self.route, self.kind))
            .max_height(ui.available_height().max(100.0))
            .show_rows(ui, 68.0, indices.len(), |ui, range| {
                for index in range {
                    let track = &tracks[indices[index]];
                    ui.push_id(index, |ui| {
                        ui.horizontal(|ui| {
                            self.artwork.show(ui, &track.id.0, &track.title, 44.0);
                            let play = ui
                                .button("▶")
                                .on_hover_text(format!("Play {}", track.title));
                            play.widget_info(|| {
                                egui::WidgetInfo::labeled(
                                    egui::WidgetType::Button,
                                    true,
                                    format!("Play {}", track.title),
                                )
                            });
                            if play.clicked() {
                                *action = Some(Action::Play(track.clone()));
                            }
                            let wide = ui.available_width() > 600.0;
                            ui.vertical(|ui| {
                                ui.set_width(
                                    (ui.available_width() - if wide { 280.0 } else { 95.0 })
                                        .max(70.0),
                                );
                                ui.add(
                                    egui::Label::new(egui::RichText::new(&track.title).strong())
                                        .truncate(),
                                );
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(track.artists.join(" · ")).weak(),
                                    )
                                    .truncate(),
                                );
                            });
                            if let Some(details) = page.and_then(|p| p.details.get(&track.id)) {
                                if wide {
                                    ui.add_sized(
                                        [170.0, 22.0],
                                        egui::Label::new(
                                            egui::RichText::new(&details.album).weak(),
                                        )
                                        .truncate(),
                                    );
                                }
                                ui.weak(&details.duration);
                            }
                            let menu = ui.menu_button("•••", |ui| {
                                if ui.button("Play next").clicked() {
                                    *action = Some(Action::Append(track.clone(), true));
                                    ui.close();
                                }
                                if ui.button("Add to queue").clicked() {
                                    *action = Some(Action::Append(track.clone(), false));
                                    ui.close();
                                }
                            });
                            menu.response.widget_info(|| {
                                egui::WidgetInfo::labeled(
                                    egui::WidgetType::Button,
                                    true,
                                    format!("Actions for {}", track.title),
                                )
                            });
                        });
                    });
                }
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> CatalogUi {
        let mut catalog = CatalogUi::at("FEmusic_explore".into());
        catalog.active = catalog.route.clone();
        let mut page = CatalogPage {
            title: "Public fixture".into(),
            ..Default::default()
        };
        page.page.tracks = (0..1753)
            .map(|i| Track {
                id: TrackId(format!("{i:011}")),
                title: format!("Song {i}"),
                artists: vec!["Fixture artist".into()],
            })
            .collect();
        page.page.collections = (0..1000)
            .map(|i| Collection {
                id: format!("VLPL{i}"),
                title: format!("Playlist {i}"),
            })
            .collect();
        catalog.page = Some(page);
        catalog
    }
    #[test]
    fn wall_clock_deadline_preserves_page_and_rejects_late_queue_changes() {
        let mut catalog = fixture();
        catalog.busy = true;
        catalog.started = Some(std::time::Instant::now() - std::time::Duration::from_secs(16));
        let generation = catalog.generation;
        let (tx, rx) = mpsc::channel();
        catalog.worker.results = rx;
        tx.send((
            generation,
            Ok(Reply::Order(
                None,
                vec![Track {
                    id: TrackId("late".into()),
                    title: "Late queue".into(),
                    artists: Vec::new(),
                }],
            )),
        ))
        .unwrap();
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(Default::default(), |ui| {
            assert!(catalog.render(ui, &[], &[], None).is_none());
        });
        output.textures_delta.clear();
        assert!(!catalog.busy && catalog.started.is_none());
        assert_eq!(catalog.page.as_ref().unwrap().page.tracks.len(), 1753);
        assert!(catalog.message.contains("timed out"));
        assert_ne!(catalog.generation, generation);
    }
    #[test]
    fn large_catalog_virtualizes_rows_filters_without_replacing_queue_and_routes_safely() {
        let mut catalog = fixture();
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        crate::theme::install(&ctx);
        for _ in 0..3 {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1000.0, 760.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    assert!(catalog.render(ui, &[], &[], None).is_none());
                },
            );
            let nodes = &output
                .platform_output
                .accesskit_update
                .as_ref()
                .unwrap()
                .nodes;
            assert!(
                nodes.len() < 500,
                "Only visible rows/cards should produce widgets"
            );
            output.textures_delta.clear();
        }
        assert_eq!(catalog.song_indices.len(), 1753);
        catalog.filter = "Song 1752".into();
        let mut output = ctx.run_ui(Default::default(), |ui| {
            catalog.render(ui, &[], &[], None);
        });
        output.textures_delta.clear();
        assert_eq!(catalog.song_indices, vec![1752]);
        catalog.route = "local:history".into();
        let mut output = ctx.run_ui(Default::default(), |ui| {
            catalog.render(ui, &[], &[], None);
        });
        output.textures_delta.clear();
        assert!(catalog.page.is_none());
        assert_eq!(catalog.active, "local:history");
    }
    #[test]
    #[ignore = "opt-in headless frame measurement; no GPU or physical input acceptance"]
    fn catalog_scrolling_frame_measurement() {
        let mut catalog = fixture();
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        let mut times = Vec::new();
        for frame in 0..140 {
            let start = std::time::Instant::now();
            let mut output = ctx.run_ui(
                egui::RawInput {
                    time: Some(frame as f64 / 60.0),
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1180.0, 760.0),
                    )),
                    events: vec![
                        egui::Event::PointerMoved(egui::pos2(800.0, 600.0)),
                        egui::Event::MouseWheel {
                            phase: egui::TouchPhase::Move,
                            unit: egui::MouseWheelUnit::Point,
                            delta: egui::vec2(0.0, -100.0),
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                    ..Default::default()
                },
                |ui| {
                    assert!(catalog.render(ui, &[], &[], None).is_none());
                },
            );
            output.textures_delta.clear();
            if frame >= 20 {
                times.push(start.elapsed().as_secs_f64() * 1000.0);
            }
        }
        times.sort_by(f64::total_cmp);
        println!(
            "catalog_headless tracks=1753 collections=1000 frames=120 p50_ms={:.3} p95_ms={:.3} max_ms={:.3}",
            times[60], times[114], times[119]
        );
        assert_eq!(catalog.song_indices.len(), 1753);
    }
}
