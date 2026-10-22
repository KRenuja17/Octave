use eframe::egui;
use octave_core::TrackId;
use octave_provider::{Error, Provider, lyrics::Lyrics};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;
use std::{collections::VecDeque, time::Instant};
type Response = (TrackId, Result<Option<Lyrics>, Error>);
#[derive(Default)]
pub struct LyricsUi {
    id: Option<TrackId>,
    response: Option<Receiver<Response>>,
    lyrics: Option<Lyrics>,
    message: String,
    reading: octave_core::lyrics::Reading,
    position: Duration,
    can_seek: bool,
    cache: VecDeque<(Lyrics, Instant)>,
    pub auto_load: bool,
}
impl LyricsUi {
    #[cfg(test)]
    pub(crate) fn reading_fixture(id: TrackId) -> Self {
        Self {
            id: Some(id.clone()),
            lyrics: Some(Lyrics {
                track_id: id,
                text: (0..80)
                    .map(|i| format!("Original fixture line {i}\n"))
                    .collect(),
                source: "Original test fixture".into(),
                cues: Vec::new(),
            }),
            ..Self::default()
        }
    }

    pub fn settled(&self) -> bool {
        self.response.is_none()
    }

    /// Explicit user action shared by the button and opt-in capture verification.
    pub fn load_current(&mut self) {
        let Some(id) = self.id.clone().filter(|_| self.response.is_none()) else {
            return;
        };
        self.message.clear();
        let (tx, rx) = mpsc::sync_channel(1);
        self.response = Some(rx);
        std::thread::spawn(move || {
            let result = Provider::new().and_then(|p| p.lyrics(&id));
            let _ = tx.send((id, result));
        });
    }

    pub fn playhead(&mut self, position: Duration, can_seek: bool) {
        self.position = position;
        self.can_seek = can_seek;
    }

    pub fn manual(&mut self) {
        self.reading.manual();
    }

    pub fn controls(&mut self, ui: &mut egui::Ui) {
        if self
            .lyrics
            .as_ref()
            .is_some_and(|lyrics| !lyrics.cues.is_empty())
        {
            if self.reading.following {
                if ui
                    .small_button("Following lyrics")
                    .on_hover_text("Pause automatic scrolling")
                    .clicked()
                {
                    self.reading.manual();
                }
            } else if ui.small_button("Resume follow").clicked() {
                self.reading.resume();
            }
        }
    }

    pub fn render(&mut self, ui: &mut egui::Ui, id: Option<&TrackId>) -> Option<Duration> {
        let mut seek = None;
        if self.id.as_ref() != id {
            if let Some(document) = self.lyrics.take() {
                self.cache
                    .retain(|(cached, _)| cached.track_id != document.track_id);
                if self.cache.len() == 8 {
                    self.cache.pop_front();
                }
                self.cache.push_back((document, Instant::now()));
            }
            self.id = id.cloned();
            self.lyrics = self
                .cache
                .iter()
                .find(|(document, time)| {
                    Some(&document.track_id) == id && time.elapsed() < Duration::from_secs(900)
                })
                .map(|(document, _)| document.clone());
            self.response = None;
            self.message.clear();
            self.reading = Default::default();
        }
        if self.auto_load
            && self.lyrics.is_none()
            && self.response.is_none()
            && self.message.is_empty()
        {
            self.load_current();
        }
        let result = self
            .response
            .as_ref()
            .and_then(|response| match response.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.id.clone().map(|id| (id, Err(Error::Network)))
                }
                Err(mpsc::TryRecvError::Empty) => None,
            });
        if let Some((id, result)) = result {
            if self.id.as_ref() == Some(&id) {
                match result {
                    Ok(lyrics) if lyrics.as_ref().is_none_or(|lyrics| lyrics.track_id == id) => {
                        self.lyrics = lyrics;
                        self.message = if self.lyrics.is_none() {
                            "No lyrics are available for this song.".into()
                        } else {
                            String::new()
                        };
                    }
                    Err(error) => self.message = lyrics_error(error).into(),
                    _ => self.message = lyrics_error(Error::InvalidResponse).into(),
                }
            }
            self.response = None;
        }
        ui.horizontal_wrapped(|ui| {
            ui.heading("Lyrics");
            let label = if !self.message.is_empty() {
                "Retry lyrics"
            } else if self.lyrics.is_some() {
                "Reload lyrics"
            } else {
                "Load lyrics"
            };
            if ui
                .add_enabled(
                    self.id.is_some() && self.settled(),
                    egui::Button::new(label),
                )
                .clicked()
            {
                self.load_current();
            }
        });
        ui.weak(
            if self
                .lyrics
                .as_ref()
                .is_some_and(|lyrics| !lyrics.cues.is_empty())
            {
                "Text from YouTube Music · line timings loaded"
            } else {
                "Text from YouTube Music · not time-synced"
            },
        );
        if let Some(lyrics) = &self.lyrics
            && !lyrics.source.trim().is_empty()
        {
            ui.weak(&lyrics.source);
        }
        ui.add_space(16.0);
        if self.id.is_none() {
            ui.label("Choose a song to view its lyrics.");
        } else if self.lyrics.is_none() && self.settled() && self.message.is_empty() {
            ui.label("Load lyrics for this song. Your music keeps its place.");
        }
        if !self.settled() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.weak(if self.lyrics.is_some() {
                    "Refreshing lyrics…"
                } else {
                    "Loading lyrics…"
                });
            });
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }
        if !self.message.is_empty() {
            ui.label(&self.message);
        }
        if let Some(lyrics) = &self.lyrics {
            let manual = ui.input(|input| {
                let over = input
                    .pointer
                    .hover_pos()
                    .is_some_and(|p| ui.clip_rect().contains(p));
                (over
                    && (input
                        .raw
                        .events
                        .iter()
                        .any(|event| matches!(event, egui::Event::MouseWheel { .. }))
                        || input.pointer.is_decidedly_dragging()))
                    || [
                        egui::Key::PageUp,
                        egui::Key::PageDown,
                        egui::Key::Home,
                        egui::Key::End,
                    ]
                    .iter()
                    .any(|&key| input.key_pressed(key))
            });
            if manual {
                self.reading.manual();
            }
            let active = lyrics.active_at(self.position);
            if egui::Popup::is_any_open(ui.ctx()) {
                self.reading.manual();
            }
            let anchor = lyrics.anchor_at(self.position);
            let recenter = self.reading.update(anchor);
            // The page owns scrolling, keeping artwork, attribution and lyrics in one flow.
            ui.scope(|ui| {
                ui.set_max_width(ui.available_width().min(720.0));
                let size = if ui.available_width() < 600.0 {
                    22.0
                } else {
                    26.0
                };
                if !lyrics.cues.is_empty() {
                    ui.add_space(ui.clip_rect().height() * 0.25);
                    for (index, cue) in lyrics.cues.iter().enumerate() {
                        let current = active == Some(index);
                        let text = if cue.text.trim().is_empty() {
                            "♪"
                        } else {
                            &cue.text
                        };
                        let rich =
                            egui::RichText::new(text)
                                .size(size)
                                .strong()
                                .color(if current {
                                    ui.visuals().text_color()
                                } else {
                                    ui.visuals().weak_text_color()
                                });
                        let response = ui.add(
                            egui::Button::new(rich)
                                .wrap()
                                .frame(current)
                                .min_size(egui::vec2(ui.available_width(), 42.0)),
                        );
                        if current {
                            let rect = response.rect;
                            ui.painter().vline(
                                rect.left() + 2.0,
                                rect.y_range(),
                                egui::Stroke::new(3.0, ui.visuals().selection.stroke.color),
                            );
                        }
                        if recenter && anchor == Some(index) {
                            response.scroll_to_me(Some(egui::Align::Center));
                        }
                        if response.clicked() && self.can_seek && !manual {
                            seek = Some(Duration::from_millis(cue.start_ms));
                            self.reading.resume();
                        }
                        response.on_hover_text(if self.can_seek {
                            "Seek to this line"
                        } else {
                            "Start playback to seek to a line"
                        });
                        ui.add_space(9.0);
                    }
                    ui.add_space(ui.clip_rect().height() * 0.4);
                } else {
                    for line in lyrics.text.lines() {
                        if line.trim().is_empty() {
                            ui.add_space(18.0);
                        } else {
                            ui.add(egui::Label::new(egui::RichText::new(line).size(size)).wrap());
                            ui.add_space(5.0);
                        }
                    }
                }
                ui.add_space(16.0);
            });
        }
        seek
    }
}

fn lyrics_error(error: Error) -> &'static str {
    match error {
        Error::Timeout => "Lyrics unavailable: the request timed out. Try again.",
        Error::Network => "Lyrics unavailable: check your connection and try again.",
        Error::Http(401 | 403) | Error::SignInRequired => {
            "Lyrics unavailable for this unsigned request. Try another song."
        }
        _ => "Lyrics unavailable right now. Try again or choose another song.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn timed_app() -> LyricsUi {
        let id = TrackId("aaaaaaaaaaa".into());
        LyricsUi {
            id: Some(id.clone()),
            lyrics: Some(Lyrics {
                track_id: id,
                text: "First phrase\nSecond phrase".into(),
                source: "Original fixture".into(),
                cues: vec![
                    octave_core::lyrics::LyricCue {
                        id: 0,
                        text: "First phrase".into(),
                        start_ms: 1000,
                        end_ms: 2000,
                    },
                    octave_core::lyrics::LyricCue {
                        id: 1,
                        text: "Second phrase".into(),
                        start_ms: 3000,
                        end_ms: 4000,
                    },
                ],
            }),
            position: Duration::from_millis(1500),
            can_seek: true,
            ..Default::default()
        }
    }
    #[test]
    fn timed_click_seeks_but_manual_scroll_and_paging_keep_reading_ownership() {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut app = timed_app();
        let frame = |app: &mut LyricsUi, events: Vec<egui::Event>| {
            let mut seek = None;
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(700.0, 600.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    app.controls(ui);
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        seek = app.render(ui, Some(&TrackId("aaaaaaaaaaa".into())));
                    });
                },
            );
            output.textures_delta.clear();
            (seek, output)
        };
        let (_, output) = frame(&mut app, vec![]);
        let bounds = output
            .platform_output
            .accesskit_update
            .unwrap()
            .nodes
            .iter()
            .find_map(|(_, node)| {
                (node.label() == Some("First phrase"))
                    .then(|| node.bounds())
                    .flatten()
            })
            .unwrap();
        let pos = egui::pos2(
            (bounds.x0 + bounds.x1) as f32 * 0.5,
            (bounds.y0 + bounds.y1) as f32 * 0.5,
        );
        frame(
            &mut app,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        let (seek, _) = frame(
            &mut app,
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert_eq!(seek, Some(Duration::from_millis(1000)));
        frame(
            &mut app,
            vec![
                egui::Event::PointerMoved(egui::pos2(400.0, 350.0)),
                egui::Event::MouseWheel {
                    phase: egui::TouchPhase::Move,
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -80.0),
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        assert!(!app.reading.following);
        app.position = Duration::from_millis(3500);
        frame(&mut app, vec![]);
        assert!(!app.reading.following);
        app.reading.resume();
        frame(
            &mut app,
            vec![egui::Event::Key {
                key: egui::Key::Home,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert!(!app.reading.following);
        app.can_seek = false;
        assert_eq!(frame(&mut app, vec![]).0, None);
    }
    #[test]
    #[ignore = "Opt-in unsigned network request; no audio or account access"]
    fn live_unsigned_lyrics_reach_the_panel() {
        let id = TrackId("dQw4w9WgXcQ".into());
        let mut app = LyricsUi::default();
        load_live(&mut app, &id);
    }

    #[test]
    #[ignore = "Opt-in unsigned timed-lyrics requests; no audio or account access"]
    fn live_unsigned_timed_lyrics_reach_the_panel() {
        let mut app = LyricsUi::default();
        for raw in ["hpSrLjc5SMs", "6Zt4P1scOwE"] {
            let id = TrackId(raw.into());
            load_live(&mut app, &id);
            let lyrics = app.lyrics.as_ref().unwrap();
            assert!(
                !lyrics.cues.is_empty(),
                "Expected genuine line timings for {raw}"
            );
            let count = lyrics.cues.len();
            let output = frame(&mut app, &id);
            let tree = output.platform_output.accesskit_update.unwrap();
            assert!(tree.nodes.iter().any(|(_, node)| {
                node.label() == Some("Text from YouTube Music · line timings loaded")
                    || node.value() == Some("Text from YouTube Music · line timings loaded")
            }));
            println!("live_timed_panel=true track={raw} cues={count}");
        }
        // Switching back to a plain recording must discard the prior cue map.
        let id = TrackId("dQw4w9WgXcQ".into());
        frame(&mut app, &id);
        assert!(app.lyrics.is_none());
        load_live(&mut app, &id);
    }

    fn load_live(app: &mut LyricsUi, id: &TrackId) {
        frame(app, id);
        app.load_current();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(45);
        while !app.settled() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(50));
            frame(app, id);
        }
        assert!(app.settled(), "Lyrics worker did not finish");
        let lyrics = app.lyrics.as_ref().expect("Live song should have lyrics");
        assert_eq!(&lyrics.track_id, id);
        assert!(!lyrics.text.trim().is_empty());
        assert!(!lyrics.source.trim().is_empty());
        assert!(app.message.is_empty());
        println!(
            "live_lyrics_rendered=true characters={} source_present=true",
            lyrics.text.chars().count()
        );
    }

    #[test]
    fn failed_refresh_preserves_readable_lyrics_and_uses_lyrics_guidance() {
        let id = TrackId("aaaaaaaaaaa".into());
        let mut app = LyricsUi::default();
        frame(&mut app, &id);
        let original = Lyrics {
            track_id: id.clone(),
            text: "Original test verse\n\nSecond stanza".into(),
            source: "Test source".into(),
            cues: Vec::new(),
        };
        app.lyrics = Some(original.clone());
        let (tx, rx) = mpsc::sync_channel(1);
        app.response = Some(rx);
        tx.send((id.clone(), Err(Error::Timeout))).unwrap();
        frame(&mut app, &id);
        assert_eq!(app.lyrics, Some(original));
        assert!(app.settled() && app.message.contains("timed out"));
        assert!(!app.message.contains("playback"));
        assert!(!lyrics_error(Error::Http(403)).contains("stream"));
    }

    fn frame(app: &mut LyricsUi, id: &TrackId) -> egui::FullOutput {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            app.render(ui, Some(id));
        });
        output.textures_delta.clear();
        output
    }
    #[test]
    fn track_changes_discard_old_lyrics_and_pending_responses() {
        let old = TrackId("aaaaaaaaaaa".into());
        let next = TrackId("bbbbbbbbbbb".into());
        let mut app = LyricsUi::default();
        frame(&mut app, &old);
        let (tx, rx) = mpsc::sync_channel(1);
        app.response = Some(rx);
        tx.send((
            old.clone(),
            Ok(Some(Lyrics {
                track_id: old,
                text: "Original test verse".into(),
                source: "Test fixture".into(),
                cues: Vec::new(),
            })),
        ))
        .unwrap();
        frame(&mut app, &next);
        assert!(app.lyrics.is_none() && app.response.is_none() && app.message.is_empty());
        assert_eq!(app.id, Some(next));
    }
    #[test]
    fn missing_lyrics_and_disconnected_worker_leave_retryable_state() {
        let id = TrackId("aaaaaaaaaaa".into());
        let mut app = LyricsUi::default();
        frame(&mut app, &id);
        let (tx, rx) = mpsc::sync_channel(1);
        app.response = Some(rx);
        tx.send((id.clone(), Ok(None))).unwrap();
        frame(&mut app, &id);
        assert!(app.message.contains("No lyrics") && app.response.is_none());
        let (tx, rx) = mpsc::sync_channel(1);
        app.response = Some(rx);
        drop(tx);
        frame(&mut app, &id);
        assert!(app.message.contains("Lyrics unavailable") && app.response.is_none());
    }
}
