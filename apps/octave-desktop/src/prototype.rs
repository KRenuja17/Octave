use crate::{
    artwork::Artwork,
    model::*,
    theme::{self, *},
};
use eframe::egui::{self, RichText, Vec2};

pub struct Prototype {
    pub page: Page,
    pub state: ViewState,
    pub library: Library,
    pub session: Session,
    pub query: String,
    pub filtered: Vec<usize>,
    pub only_liked: bool,
    artwork: Artwork,
    pub rendered_rows: usize,
    pub status: String,
    focus_search: bool,
    pub last_frame_ms: f64,
}
impl Prototype {
    pub fn artwork_ready(&self) -> bool {
        self.artwork.ready()
    }
    pub fn new(ctx: &egui::Context) -> Self {
        theme::install(ctx);
        let library = Library::default();
        let filtered = library.search("", None);
        Self {
            page: Page::Home,
            state: ViewState::Ready,
            library,
            session: Session::default(),
            query: String::new(),
            filtered,
            only_liked: false,
            artwork: Artwork::new(ctx.clone()),
            rendered_rows: 0,
            status: String::new(),
            focus_search: false,
            last_frame_ms: 0.0,
        }
    }
    pub fn filter(&mut self) {
        self.filtered = self
            .library
            .search(&self.query, self.only_liked.then_some(&self.session.liked));
    }
    fn navigate(&mut self, page: Page) {
        self.page = page;
        self.state = ViewState::Ready;
        self.status.clear();
        if page == Page::Search {
            self.focus_search = true;
        }
    }
    pub fn render(&mut self, ui: &mut egui::Ui) {
        let started = std::time::Instant::now();
        self.rendered_rows = 0;
        self.artwork.poll(ui.ctx());
        self.session
            .tick(ui.input(|i| f64::from(i.stable_dt)), &self.library);
        let compact = ui.available_width() < 900.0 || ui.available_height() < 660.0;
        let search_shortcut = ui.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::K));
        if search_shortcut {
            self.navigate(Page::Search);
        }
        if !ui.memory(|m| m.has_focus(egui::Id::new("catalog-search"))) {
            if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Space)) {
                self.session.playing = !self.session.playing && self.session.current().is_some();
            }
            if ui.input_mut(|i| i.consume_key(egui::Modifiers::ALT, egui::Key::ArrowRight)) {
                self.session.next();
            }
            if ui.input_mut(|i| i.consume_key(egui::Modifiers::ALT, egui::Key::ArrowLeft)) {
                self.session.previous();
            }
        }
        egui::Panel::bottom("player")
            .exact_size(if compact { 164.0 } else { 132.0 })
            .frame(egui::Frame::new().fill(PANEL).inner_margin(16))
            .show(ui, |ui| self.player_bar(ui, compact));
        if compact {
            egui::Panel::top("compact-nav")
                .frame(egui::Frame::new().fill(PANEL).inner_margin(8))
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        let narrow = ui.available_width() < 480.0;
                        for (page, label) in [
                            (Page::Home, "Home"),
                            (Page::Search, "Search"),
                            (Page::Library, "Library"),
                            (Page::Queue, "Queue"),
                            (Page::Lyrics, "Lyrics"),
                        ] {
                            if narrow && matches!(page, Page::Queue | Page::Lyrics) {
                                continue;
                            }
                            if ui.selectable_label(self.page == page, label).clicked() {
                                self.navigate(page);
                            }
                        }
                        if narrow {
                            ui.menu_button("More", |ui| {
                                if ui.button("Queue").clicked() {
                                    self.navigate(Page::Queue);
                                    ui.close();
                                }
                                if ui.button("Lyrics").clicked() {
                                    self.navigate(Page::Lyrics);
                                    ui.close();
                                }
                            });
                        }
                    });
                });
        } else {
            egui::Panel::left("navigation")
                .exact_size(206.0)
                .resizable(false)
                .frame(egui::Frame::new().fill(PANEL).inner_margin(22))
                .show(ui, |ui| self.sidebar(ui));
        }
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(BG)
                    .inner_margin(if compact { 16 } else { 28 }),
            )
            .show(ui, |ui| {
                self.toolbar(ui, compact);
                ui.add_space(12.0);
                if ui.available_height() < 360.0
                    && (!matches!(self.page, Page::Home | Page::Lyrics)
                        || self.state != ViewState::Ready)
                {
                    egui::ScrollArea::vertical()
                        .id_salt("short-page")
                        .show(ui, |ui| self.page_content(ui));
                } else {
                    self.page_content(ui);
                }
            });
        self.last_frame_ms = started.elapsed().as_secs_f64() * 1000.0;
        if self.session.playing || self.state == ViewState::Loading {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(33));
        }
    }
    fn page_content(&mut self, ui: &mut egui::Ui) {
        if self.state != ViewState::Ready {
            self.state_view(ui);
            return;
        }
        match self.page {
            Page::Home => self.home(ui),
            Page::Search | Page::Library => self.search_library(ui),
            Page::Collection(index) => self.collection(ui, index),
            Page::Queue => self.queue(ui),
            Page::Lyrics => self.lyrics(ui),
        }
    }
    fn sidebar(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .id_salt("sidebar-scroll")
            .show(ui, |ui| self.sidebar_contents(ui));
    }
    fn sidebar_contents(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        ui.label(RichText::new("◒  octave").size(27.0).strong().color(ACCENT));
        ui.add_space(36.0);
        eyebrow(ui, "YOUR SPACE");
        ui.add_space(8.0);
        for (page, label) in [
            (Page::Home, "Home"),
            (Page::Search, "Search"),
            (Page::Library, "Your library"),
        ] {
            if ui
                .add_sized(
                    [160.0, 40.0],
                    egui::Button::new(label).selected(self.page == page),
                )
                .clicked()
            {
                self.navigate(page);
            }
        }
        ui.add_space(28.0);
        eyebrow(ui, "IN ROTATION");
        ui.add_space(8.0);
        for i in [1, 3, 5] {
            if ui
                .add(egui::Button::new(COLLECTIONS[i].title).frame(false))
                .clicked()
            {
                self.navigate(Page::Collection(i));
            }
        }
        ui.add_space(22.0);
        if ui.button("Up next").clicked() {
            self.navigate(Page::Queue);
        }
        if ui.button("Now playing / lyrics").clicked() {
            self.navigate(Page::Lyrics);
        }
        ui.add_space(24.0);
        ui.vertical(|ui| {
            ui.label(
                RichText::new("Sample library · no audio")
                    .size(11.0)
                    .color(MUTED),
            );
            ui.label(
                RichText::new("NATIVE EXPERIENCE PREVIEW")
                    .size(10.0)
                    .color(ACCENT),
            );
            ui.add_space(12.0);
            if ui.button("Account").clicked() {
                self.state = ViewState::SignedOut;
            }
        });
    }
    fn toolbar(&mut self, ui: &mut egui::Ui, compact: bool) {
        ui.horizontal(|ui| {
            let back = ui.button("←").on_hover_text("Back to Home");
            back.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Back to Home")
            });
            if back.clicked() {
                self.navigate(Page::Home);
            }
            if !compact {
                muted(ui, "A space for your soundtrack");
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.menu_button("Preview states", |ui| {
                    ui.label("Sample data only · no audio is emitted");
                    for state in ViewState::ALL {
                        if ui
                            .selectable_label(self.state == state, state.label())
                            .clicked()
                        {
                            self.state = state;
                            ui.close();
                        }
                    }
                    ui.separator();
                    ui.label("Ctrl+K  Search");
                    ui.label("Space  Play / pause");
                    ui.label("Alt+← / →  Previous / next");
                });
            });
        });
    }
    fn home(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .id_salt("home-scroll")
            .show(ui, |ui| {
                egui::Frame::new()
                    .fill(egui::Color32::from_rgb(36, 49, 46))
                    .corner_radius(16.0)
                    .inner_margin(24)
                    .show(ui, |ui| {
                        let width = ui.available_width();
                        ui.set_min_width(width);
                        let top = ui.cursor().min;
                        eyebrow(ui, "A MOMENT FOR YOURSELF");
                        ui.add_space(10.0);
                        ui.label(
                            RichText::new("Find your\nkind of quiet.")
                                .size(38.0)
                                .strong(),
                        );
                        muted(ui, "Fresh sounds for a softer landing.");
                        ui.add_space(8.0);
                        if primary(ui, "Explore Golden hour  →").clicked() {
                            self.navigate(Page::Collection(1));
                        }
                        if width > 630.0 {
                            let rect = egui::Rect::from_min_size(
                                top + egui::vec2(width - 190.0, 10.0),
                                egui::vec2(180.0, 180.0),
                            );
                            ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                                self.artwork.show(ui, 1, 180.0)
                            });
                        }
                    });
                ui.add_space(26.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Made for this moment").size(23.0).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("View library").clicked() {
                            self.navigate(Page::Library);
                        }
                    });
                });
                muted(ui, "A few places to start. Stay as long as you like.");
                ui.add_space(12.0);
                let columns = ((ui.available_width() / 180.0).floor() as usize).clamp(2, 6);
                let card_width =
                    (ui.available_width() - 12.0 * (columns as f32 - 1.0)) / columns as f32;
                egui::Grid::new("home-cards")
                    .num_columns(columns)
                    .spacing([12.0, 18.0])
                    .show(ui, |ui| {
                        for (i, collection) in COLLECTIONS.iter().enumerate() {
                            ui.allocate_ui_with_layout(
                                Vec2::new(card_width, card_width + 58.0),
                                egui::Layout::top_down(egui::Align::LEFT),
                                |ui| {
                                    self.artwork.show(ui, i, card_width.min(220.0));
                                    if ui
                                        .add(
                                            egui::Button::new(
                                                RichText::new(collection.title).strong(),
                                            )
                                            .frame(false),
                                        )
                                        .clicked()
                                    {
                                        self.navigate(Page::Collection(i));
                                    }
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(format!(
                                                "{} · {}",
                                                collection.kind, collection.artist
                                            ))
                                            .size(12.0)
                                            .color(MUTED),
                                        )
                                        .truncate(),
                                    );
                                },
                            );
                            if (i + 1) % columns == 0 {
                                ui.end_row();
                            }
                        }
                    });
                ui.add_space(20.0);
                eyebrow(ui, "A LITTLE DISCOVERY");
                ui.label("Every title and cover here is part of Octave's original sample library.");
            });
    }
    fn search_library(&mut self, ui: &mut egui::Ui) {
        ui.heading(if self.page == Page::Search {
            "Find your next favorite"
        } else {
            "Your library"
        });
        muted(
            ui,
            if self.page == Page::Search {
                "Songs, artists, and a little serendipity."
            } else {
                "The sounds you come back to. 10,000 sample tracks."
            },
        );
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            let response = ui.add_sized(
                [ui.available_width().min(460.0) - 85.0, 38.0],
                egui::TextEdit::singleline(&mut self.query)
                    .id(egui::Id::new("catalog-search"))
                    .hint_text("Search songs, artists, albums…")
                    .char_limit(256),
            );
            response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Search sample library")
            });
            if self.focus_search {
                response.request_focus();
                self.focus_search = false;
            }
            if response.changed() {
                self.filter();
            }
            if ui.button("Clear").clicked() {
                self.query.clear();
                self.filter();
            }
        });
        ui.horizontal(|ui| {
            if ui.selectable_label(!self.only_liked, "All songs").clicked() {
                self.only_liked = false;
                self.filter();
            }
            if ui
                .selectable_label(self.only_liked, "Liked songs")
                .clicked()
            {
                self.only_liked = true;
                self.filter();
            }
            muted(ui, format!("{} tracks", self.filtered.len()));
        });
        ui.add_space(10.0);
        if self.filtered.is_empty() {
            self.empty(
                ui,
                "No matches, yet.",
                "Try another title or artist, or clear your filters.",
            );
        } else {
            self.track_list(ui, false);
        }
    }
    fn collection(&mut self, ui: &mut egui::Ui, index: usize) {
        let c = &COLLECTIONS[index];
        let small = ui.available_width() < 550.0 || ui.input(|i| i.content_rect().height() < 700.0);
        ui.horizontal(|ui| {
            self.artwork
                .show(ui, index, if small { 64.0 } else { 164.0 });
            ui.vertical(|ui| {
                eyebrow(ui, &c.kind.to_uppercase());
                ui.add(
                    egui::Label::new(
                        RichText::new(c.title)
                            .size(if small { 23.0 } else { 38.0 })
                            .strong(),
                    )
                    .wrap(),
                );
                muted(ui, c.artist);
                if !small {
                    muted(ui, c.description);
                }
                if !small {
                    muted(ui, "6 songs · original sample collection");
                }
            });
        });
        ui.add_space(16.0);
        ui.horizontal_wrapped(|ui| {
            if primary(ui, "Play collection").clicked() {
                self.session.play((index * 6..index * 6 + 6).collect(), 0);
            }
            if ui.button("Add to queue").clicked() {
                self.session.queue.extend(index * 6..index * 6 + 6);
                self.status = "Collection added to Up next".into();
            }
            if ui.button("View queue").clicked() {
                self.navigate(Page::Queue);
            }
        });
        if !self.status.is_empty() {
            muted(ui, self.status.clone());
        }
        ui.add_space(14.0);
        self.track_list(ui, false);
    }
    fn queue(&mut self, ui: &mut egui::Ui) {
        ui.heading("Up next");
        muted(ui, "A little intention. A little room for surprise.");
        ui.add_space(14.0);
        ui.horizontal_wrapped(|ui| {
            muted(
                ui,
                format!("{} songs in this session", self.session.queue.len()),
            );
            if ui.button("Clear queue").clicked() {
                self.session.play(Vec::new(), 0);
            }
            if ui.button("Add Golden hour").clicked() {
                self.session.queue.extend(6..12);
            }
        });
        ui.add_space(12.0);
        if self.session.queue.is_empty() {
            self.empty(
                ui,
                "Room for something good.",
                "Add a song or collection to start your queue.",
            );
        } else {
            self.track_list(ui, true);
        }
    }
    fn track_list(&mut self, ui: &mut egui::Ui, queue: bool) {
        let album = if let Page::Collection(i) = self.page {
            Some(i)
        } else {
            None
        };
        let count = if queue {
            self.session.queue.len()
        } else if album.is_some() {
            6
        } else {
            self.filtered.len()
        };
        let wide = ui.available_width() > 650.0;
        ui.horizontal(|ui| {
            eyebrow(
                ui,
                if queue {
                    "YOUR LISTENING ORDER"
                } else {
                    "TRACK / ARTIST"
                },
            );
            if wide {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    muted(ui, "ALBUM                         TIME       ");
                });
            }
        });
        ui.separator();
        let mut play = None;
        let mut remove = None;
        let mut movement = None;
        let mut likes_changed = false;
        egui::ScrollArea::vertical()
            .max_height(600.0)
            .id_salt(("tracks", format!("{:?}", self.page)))
            .auto_shrink([false, false])
            .show_rows(ui, 56.0, count, |ui, range| {
                self.rendered_rows += range.len();
                for row in range {
                    let id = if queue {
                        self.session.queue[row]
                    } else if let Some(a) = album {
                        a * 6 + row
                    } else {
                        self.filtered[row]
                    };
                    let song = self.library.songs[id].clone();
                    let active = self.session.current() == Some(id);
                    ui.push_id((row, id), |ui| {
                        let width = ui.available_width();
                        egui::Frame::new()
                            .fill(if active {
                                egui::Color32::from_rgb(34, 48, 40)
                            } else {
                                BG
                            })
                            .inner_margin(4)
                            .corner_radius(6.0)
                            .show(ui, |ui| {
                                ui.set_min_height(48.0);
                                ui.set_max_width(width - 8.0);
                                ui.horizontal(|ui| {
                                    let play_button = ui.add_sized(
                                        [30.0, 32.0],
                                        egui::Button::new(if active && self.session.playing {
                                            "Ⅱ"
                                        } else {
                                            "▷"
                                        })
                                        .frame(false),
                                    );
                                    play_button.widget_info(|| {
                                        egui::WidgetInfo::labeled(
                                            egui::WidgetType::Button,
                                            true,
                                            format!("Play {}", song.title),
                                        )
                                    });
                                    if play_button
                                        .on_hover_text(format!("Play {}", song.title))
                                        .clicked()
                                    {
                                        if active {
                                            self.session.playing = !self.session.playing;
                                        } else {
                                            play = Some(row);
                                        }
                                    }
                                    self.artwork.show(ui, song.album, 42.0);
                                    let reserved = if wide { 358.0 } else { 236.0 };
                                    ui.allocate_ui_with_layout(
                                        Vec2::new((width - reserved).max(36.0), 44.0),
                                        egui::Layout::top_down(egui::Align::LEFT),
                                        |ui| {
                                            ui.set_min_width((width - reserved).max(36.0));
                                            ui.spacing_mut().item_spacing.y = 3.0;
                                            ui.add(
                                                egui::Label::new(
                                                    RichText::new(&song.title).color(if active {
                                                        ACCENT
                                                    } else {
                                                        TEXT
                                                    }),
                                                )
                                                .truncate(),
                                            )
                                            .on_hover_text(&song.title);
                                            ui.add(
                                                egui::Label::new(
                                                    RichText::new(&song.artist)
                                                        .size(12.0)
                                                        .color(MUTED),
                                                )
                                                .truncate(),
                                            )
                                            .on_hover_text(&song.artist);
                                        },
                                    );
                                    if wide {
                                        ui.add_sized(
                                            [112.0, 28.0],
                                            egui::Label::new(
                                                RichText::new(COLLECTIONS[song.album].title)
                                                    .size(12.0)
                                                    .color(MUTED),
                                            )
                                            .truncate(),
                                        );
                                    }
                                    let liked = self.session.liked.contains(&id);
                                    let like = ui.add_sized(
                                        [28.0, 30.0],
                                        egui::Button::new(if liked { "♥" } else { "♡" })
                                            .frame(false),
                                    );
                                    like.widget_info(|| {
                                        egui::WidgetInfo::labeled(
                                            egui::WidgetType::Button,
                                            true,
                                            format!(
                                                "{} {}",
                                                if liked { "Unlike" } else { "Like" },
                                                song.title
                                            ),
                                        )
                                    });
                                    if like
                                        .on_hover_text(if liked {
                                            "Unlike song"
                                        } else {
                                            "Like song"
                                        })
                                        .clicked()
                                    {
                                        self.session.toggle_like(id);
                                        likes_changed = true;
                                    }
                                    ui.label(
                                        RichText::new(time(song.seconds as f64))
                                            .size(12.0)
                                            .color(MUTED),
                                    );
                                    let actions = ui
                                        .menu_button("…", |ui| {
                                            if queue {
                                                if ui
                                                    .add_enabled(
                                                        row > 0,
                                                        egui::Button::new("Move up"),
                                                    )
                                                    .clicked()
                                                {
                                                    movement = Some((row, row - 1));
                                                    ui.close();
                                                }
                                                if ui
                                                    .add_enabled(
                                                        row + 1 < count,
                                                        egui::Button::new("Move down"),
                                                    )
                                                    .clicked()
                                                {
                                                    movement = Some((row, row + 1));
                                                    ui.close();
                                                }
                                                if ui.button("Remove from queue").clicked() {
                                                    remove = Some(row);
                                                    ui.close();
                                                }
                                            } else {
                                                if ui.button("Play next").clicked() {
                                                    let at = (self.session.cursor + 1)
                                                        .min(self.session.queue.len());
                                                    self.session.queue.insert(at, id);
                                                    self.status =
                                                        "Added after the current song".into();
                                                    ui.close();
                                                }
                                                if ui.button("Add to queue").clicked() {
                                                    self.session.queue.push(id);
                                                    self.status = "Song added to Up next".into();
                                                    ui.close();
                                                }
                                                if ui.button("Open collection").clicked() {
                                                    self.navigate(Page::Collection(song.album));
                                                    ui.close();
                                                }
                                            }
                                        })
                                        .response
                                        .on_hover_text(format!("More actions for {}", song.title));
                                    actions.widget_info(|| {
                                        egui::WidgetInfo::labeled(
                                            egui::WidgetType::Button,
                                            true,
                                            format!("More actions for {}", song.title),
                                        )
                                    });
                                });
                            });
                    });
                }
            });
        if let Some(row) = play {
            let ids = if queue {
                self.session.queue.clone()
            } else if let Some(a) = album {
                (a * 6..a * 6 + 6).collect()
            } else {
                self.filtered.clone()
            };
            self.session.play(ids, row);
        }
        if let Some(row) = remove {
            self.session.remove(row);
        }
        if let Some((from, to)) = movement {
            self.session.move_item(from, to);
        }
        if likes_changed && self.only_liked {
            self.filter();
        }
    }
    fn player_bar(&mut self, ui: &mut egui::Ui, compact: bool) {
        let song = self
            .session
            .current()
            .map(|id| self.library.songs[id].clone());
        if compact {
            ui.horizontal(|ui| {
                if let Some(s) = &song {
                    self.artwork.show(ui, s.album, 38.0);
                    ui.add(egui::Label::new(RichText::new(&s.title).strong()).truncate());
                } else {
                    muted(ui, "Your soundtrack starts here");
                }
            });
        }
        ui.horizontal(|ui| {
            if !compact && let Some(s) = &song {
                self.artwork.show(ui, s.album, 52.0);
                ui.allocate_ui_with_layout(
                    Vec2::new((ui.available_width() * 0.25).min(245.0), 46.0),
                    egui::Layout::top_down(egui::Align::LEFT),
                    |ui| {
                        ui.add(egui::Label::new(RichText::new(&s.title).strong()).truncate())
                            .on_hover_text(&s.title);
                        muted(ui, &s.artist);
                    },
                );
            }
            ui.add_enabled_ui(song.is_some(), |ui| {
                if ui.button("Previous").clicked() {
                    self.session.previous();
                }
                if primary(
                    ui,
                    if self.session.playing {
                        "Pause"
                    } else {
                        "Play"
                    },
                )
                .clicked()
                {
                    self.session.playing = !self.session.playing;
                }
                if ui.button("Next").clicked() {
                    self.session.next();
                }
            });
            if !compact {
                if ui
                    .selectable_label(self.session.shuffled, "Shuffle")
                    .clicked()
                {
                    self.session.shuffle();
                }
                let label = match self.session.repeat {
                    Repeat::Off => "Repeat off",
                    Repeat::All => "Repeat all",
                    Repeat::One => "Repeat one",
                };
                if ui.button(label).clicked() {
                    self.session.repeat = match self.session.repeat {
                        Repeat::Off => Repeat::All,
                        Repeat::All => Repeat::One,
                        Repeat::One => Repeat::Off,
                    };
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Queue").clicked() {
                    self.navigate(Page::Queue);
                }
                if !compact && ui.button("Lyrics").clicked() {
                    self.navigate(Page::Lyrics);
                }
            });
        });
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(time(self.session.position))
                    .size(12.0)
                    .color(MUTED),
            );
            let duration = song.as_ref().map_or(0.0, |s| s.seconds as f64);
            let reserve = if compact { 130.0 } else { 330.0 };
            ui.spacing_mut().slider_width = (ui.available_width() - reserve).max(80.0);
            ui.add_enabled(
                duration > 0.0,
                egui::Slider::new(&mut self.session.position, 0.0..=duration.max(1.0))
                    .show_value(false),
            )
            .widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Slider, true, "Playback position")
            });
            ui.label(RichText::new(time(duration)).size(12.0).color(MUTED));
            if !compact {
                ui.spacing_mut().slider_width = 75.0;
                ui.add(
                    egui::Slider::new(&mut self.session.volume, 0.0..=1.0)
                        .show_value(false)
                        .text("Volume"),
                );
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if compact {
                    ui.menu_button("Demo", |ui| {
                        ui.label("Silent prototype controls");
                        ui.add(
                            egui::Slider::new(&mut self.session.volume, 0.0..=1.0).text("Volume"),
                        );
                        if ui
                            .selectable_label(self.session.shuffled, "Shuffle")
                            .clicked()
                        {
                            self.session.shuffle();
                        }
                        for repeat in [Repeat::Off, Repeat::All, Repeat::One] {
                            if ui
                                .selectable_label(
                                    self.session.repeat == repeat,
                                    format!("Repeat {repeat:?}"),
                                )
                                .clicked()
                            {
                                self.session.repeat = repeat;
                            }
                        }
                    });
                } else {
                    ui.label(RichText::new("DEMO · SILENT").size(10.0).color(MUTED));
                }
            });
        });
    }
    fn lyrics(&mut self, ui: &mut egui::Ui) {
        let Some(id) = self.session.current() else {
            self.empty(
                ui,
                "Nothing playing yet.",
                "Choose a song to open its listening view.",
            );
            return;
        };
        let song = self.library.songs[id].clone();
        egui::ScrollArea::vertical()
            .id_salt("lyrics-scroll")
            .show(ui, |ui| {
                let wide = ui.available_width() > 640.0;
                let content = |ui: &mut egui::Ui| {
                    eyebrow(ui, "LYRICS · ORIGINAL SAMPLE");
                    ui.add_space(20.0);
                    for (i, line) in [
                        "Let the morning find us here",
                        "With the windows open wide",
                        "All the noise is somewhere else",
                        "There is nothing left to hide",
                        "Take the long way through the light",
                        "We have time, we have time",
                    ]
                    .iter()
                    .enumerate()
                    {
                        let current = (self.session.position as usize / 12) % 6 == i;
                        ui.label(
                            RichText::new(*line)
                                .size(if wide { 27.0 } else { 21.0 })
                                .strong()
                                .color(if current { ACCENT } else { MUTED }),
                        );
                        ui.add_space(14.0);
                    }
                };
                if wide {
                    ui.horizontal_top(|ui| {
                        ui.vertical(|ui| {
                            ui.set_width(220.0);
                            self.artwork.show(ui, song.album, 210.0);
                            ui.add_space(12.0);
                            ui.label(RichText::new(&song.title).size(23.0).strong());
                            muted(ui, &song.artist);
                            muted(ui, "Sample lyrics, not provider lyrics.");
                        });
                        ui.add_space(22.0);
                        ui.vertical(content);
                    });
                } else {
                    ui.horizontal(|ui| {
                        self.artwork.show(ui, song.album, 84.0);
                        ui.vertical(|ui| {
                            ui.label(RichText::new(&song.title).strong());
                            muted(ui, &song.artist);
                        });
                    });
                    ui.add_space(24.0);
                    content(ui);
                }
            });
    }
    fn empty(&mut self, ui: &mut egui::Ui, title: &str, body: &str) {
        ui.add_space(36.0);
        ui.heading(title);
        muted(ui, body);
        ui.add_space(16.0);
        if primary(ui, "Explore collections").clicked() {
            self.navigate(Page::Home);
        }
    }
    fn state_view(&mut self, ui: &mut egui::Ui) {
        let (title, body, action) = match self.state {
            ViewState::Loading => (
                "Finding the right notes…",
                "Your music is on its way.",
                "Show sample content",
            ),
            ViewState::Empty => (
                "A fresh start.",
                "Your saved music will have a home here.",
                "Explore sample library",
            ),
            ViewState::Error => (
                "That didn't quite land.",
                "We couldn't load this page. Your current queue is still here.",
                "Try again",
            ),
            ViewState::Offline => (
                "A little out of reach.",
                "You're offline. Reconnect to load music; your current queue stays in place.",
                "Check connection",
            ),
            ViewState::SignedOut => (
                "Make yourself at home.",
                "Sign in will bring your YouTube Music library here. Account connection is coming later.",
                "Explore sample library",
            ),
            ViewState::Ready => return,
        };
        ui.add_space(40.0);
        egui::Frame::new()
            .fill(CARD)
            .corner_radius(16.0)
            .inner_margin(28)
            .show(ui, |ui| {
                eyebrow(ui, self.state.label());
                ui.add_space(12.0);
                if self.state == ViewState::Loading {
                    ui.spinner();
                }
                ui.heading(title);
                ui.add_space(10.0);
                ui.label(body);
                ui.add_space(20.0);
                if primary(ui, action).clicked() {
                    self.state = ViewState::Ready;
                }
                ui.add_space(12.0);
                muted(ui, "Preview scenario · this does not contact a service.");
            });
    }
}
