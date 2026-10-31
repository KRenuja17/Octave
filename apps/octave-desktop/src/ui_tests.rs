//! Headless egui input/layout checks. Real GPU captures complement these tests.
use crate::{
    model::{Page, ViewState},
    prototype::Prototype,
};
use eframe::egui::{self, Event, Key, Modifiers};
fn frame(
    ctx: &egui::Context,
    app: &mut Prototype,
    size: [f32; 2],
    events: Vec<Event>,
) -> egui::FullOutput {
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(size[0], size[1]),
            )),
            events,
            ..Default::default()
        },
        |ui| app.render(ui),
    );
    output.textures_delta.clear(); // No GPU in these input/layout tests.
    output
}
fn key(key: Key, modifiers: Modifiers) -> Event {
    Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}

#[test]
fn ime_output_tracks_focused_field_and_clears_after_blur() {
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    let ids = [
        egui::Id::unique("ime-first"),
        egui::Id::unique("ime-second"),
    ];
    let mut texts = [String::new(), String::new()];
    let render = |texts: &mut [String; 2], events| {
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(480.0, 640.0),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                for (index, text) in texts.iter_mut().enumerate() {
                    egui::TextEdit::singleline(text)
                        .id(ids[index])
                        .desired_width(250.0)
                        .show(ui);
                    ui.add_space(40.0);
                }
            },
        );
        output.textures_delta.clear();
        output.platform_output.ime
    };
    render(&mut texts, vec![]);
    ctx.memory_mut(|m| m.request_focus(ids[0]));
    let first = render(
        &mut texts,
        vec![Event::Ime(egui::ImeEvent::Preedit {
            text: "日本".into(),
            active_range_chars: Some(0..2),
        })],
    )
    .unwrap();
    assert!(first.rect.is_finite() && first.cursor_rect.is_finite());
    assert!(first.rect.expand(2.0).contains_rect(first.cursor_rect));
    ctx.memory_mut(|m| m.request_focus(ids[1]));
    let second = render(
        &mut texts,
        vec![Event::Ime(egui::ImeEvent::Commit("한국어".into()))],
    )
    .unwrap();
    assert!(second.rect.top() > first.rect.bottom());
    assert!(second.rect.expand(2.0).contains_rect(second.cursor_rect));
    assert_eq!(texts, ["日本", "한국어"]);
    ctx.memory_mut(|m| m.surrender_focus(ids[1]));
    assert!(render(&mut texts, vec![]).is_none());
}

#[test]
fn accessibility_selection_crosses_long_text_run_boundaries() {
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    ctx.enable_accesskit();
    let id = egui::Id::unique("long-accessible-edit");
    let source = "日本語".repeat(200);
    let mut text = source.clone();
    let render = |text: &mut String, events| {
        let mut output = ctx.run_ui(
            egui::RawInput {
                events,
                ..Default::default()
            },
            |ui| {
                egui::TextEdit::singleline(text).id(id).show(ui);
            },
        );
        output.textures_delta.clear();
        output
    };
    render(&mut text, vec![]);
    ctx.memory_mut(|m| m.request_focus(id));
    let tree = render(&mut text, vec![])
        .platform_output
        .accesskit_update
        .unwrap();
    let (_, field) = tree
        .nodes
        .iter()
        .find(|(node_id, _)| *node_id == id.accesskit_id())
        .unwrap();
    assert_eq!(field.children().len(), 3);
    let selection = egui::accesskit::TextSelection {
        anchor: egui::accesskit::TextPosition {
            node: field.children()[0],
            character_index: 254,
        },
        focus: egui::accesskit::TextPosition {
            node: field.children()[2],
            character_index: 2,
        },
    };
    let tree = render(
        &mut text,
        vec![Event::AccessKitActionRequest(
            egui::accesskit::ActionRequest {
                action: egui::accesskit::Action::SetTextSelection,
                target_tree: egui::accesskit::TreeId::ROOT,
                target_node: id.accesskit_id(),
                data: Some(egui::accesskit::ActionData::SetTextSelection(selection)),
            },
        )],
    )
    .platform_output
    .accesskit_update
    .unwrap();
    let (_, field) = tree
        .nodes
        .iter()
        .find(|(node_id, _)| *node_id == id.accesskit_id())
        .unwrap();
    assert_eq!(field.text_selection(), Some(&selection));
    let range = egui::TextEdit::load_state(&ctx, id)
        .unwrap()
        .cursor
        .char_range()
        .unwrap()
        .as_sorted_char_range();
    assert_eq!((range.start.0, range.end.0), (254, 512));
    assert_eq!(text, source);
}

#[test]
fn accessibility_text_runs_preserve_logical_unicode() {
    // This checks the tree consumed by adapters, not spoken screen-reader output.
    for source in [
        "עברית العربية 日本 👨‍👩‍👧‍👦 🇱🇰\nnext".to_owned(),
        "א日本🇱🇰 ".repeat(80),
    ] {
        let ctx = egui::Context::default();
        crate::theme::install(&ctx);
        ctx.enable_accesskit();
        let mut text = source.clone();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            egui::TextEdit::multiline(&mut text)
                .desired_width(180.0)
                .show(ui);
        });
        output.textures_delta.clear();
        let tree = output.platform_output.accesskit_update.unwrap();
        let parent = tree
            .nodes
            .iter()
            .find(|(_, n)| n.role() == egui::accesskit::Role::MultilineTextInput)
            .unwrap();
        // Updates are unordered; the parent's children define reading order.
        let runs: Vec<_> = parent
            .1
            .children()
            .iter()
            .map(|id| {
                tree.nodes
                    .iter()
                    .find(|(node_id, _)| node_id == id)
                    .unwrap()
            })
            .collect();
        let reconstructed: String = runs
            .iter()
            .map(|(_, n)| n.value().unwrap_or_default())
            .collect();
        assert_eq!(reconstructed, source);
        for (_, node) in runs {
            let value = node.value().unwrap_or_default();
            assert_eq!(
                node.character_lengths()
                    .iter()
                    .map(|&v| usize::from(v))
                    .sum::<usize>(),
                value.len()
            );
            assert_eq!(node.character_lengths().len(), value.chars().count());
        }
        assert_eq!(text, source);
    }
}

#[test]
fn accessibility_bidi_bounds_follow_glyphs_at_text_sizes_and_scales() {
    use egui::epaint::text::CharIndex;
    for scale in [1.0, 1.25, 1.5, 2.0] {
        for font_size in [15.0, 30.0, 42.0] {
            for source in [
                "אבגדה",
                "ABC אבג 123 العربية XYZ\nnext",
                "A 👩‍💻 بِ 🇱🇰 e\u{301}",
            ] {
                let ctx = egui::Context::default();
                crate::theme::install(&ctx);
                ctx.enable_accesskit();
                let id = egui::Id::unique("bidi-bounds");
                let transform = egui::emath::TSTransform::new(egui::vec2(31.0, 47.0), scale);
                let mut layout = None;
                let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
                    let galley = ui.fonts_mut(|f| {
                        f.layout(
                            source.into(),
                            egui::FontId::proportional(font_size),
                            egui::Color32::WHITE,
                            180.0,
                        )
                    });
                    egui::text_selection::accesskit_text::update_accesskit_for_text_widget(
                        &ctx,
                        id,
                        None,
                        egui::accesskit::Role::MultilineTextInput,
                        transform,
                        &galley,
                    );
                    layout = Some(galley);
                });
                output.textures_delta.clear();
                let galley = layout.unwrap();
                let tree = output.platform_output.accesskit_update.unwrap();
                let parent = &tree
                    .nodes
                    .iter()
                    .find(|(node_id, _)| *node_id == id.accesskit_id())
                    .unwrap()
                    .1;
                let runs: Vec<_> = parent
                    .children()
                    .iter()
                    .map(|child| {
                        &tree
                            .nodes
                            .iter()
                            .find(|(node_id, _)| node_id == child)
                            .unwrap()
                            .1
                    })
                    .collect();
                assert_eq!(
                    runs.iter()
                        .map(|n| n.value().unwrap_or_default())
                        .collect::<String>(),
                    source
                );
                let expected: Vec<_> = galley
                    .rows
                    .iter()
                    .flat_map(|row| {
                        let mut chars: Vec<_> = row
                            .glyphs
                            .iter()
                            .map(|g| {
                                (
                                    g.chr,
                                    g.is_rtl(),
                                    (row.pos.x + g.pos.x) * scale + 31.0,
                                    g.advance_width * scale,
                                )
                            })
                            .collect();
                        if row.ends_with_newline {
                            chars.push((
                                '\n',
                                row.glyphs.last().is_some_and(|g| g.is_rtl()),
                                (row.pos.x + row.x_offset(CharIndex(row.glyphs.len()))) * scale
                                    + 31.0,
                                0.0,
                            ));
                        }
                        chars
                    })
                    .collect();
                let mut offset = 0;
                for run in runs {
                    let bounds = run.bounds().unwrap();
                    assert!(bounds.x0 <= bounds.x1 && bounds.y0 < bounds.y1);
                    let rtl =
                        run.text_direction() == Some(egui::accesskit::TextDirection::RightToLeft);
                    let positions = run.character_positions().unwrap();
                    let widths = run.character_widths().unwrap();
                    for (i, chr) in run.value().unwrap_or_default().chars().enumerate() {
                        let (expected_chr, expected_rtl, x, width) = expected[offset];
                        assert_eq!(chr, expected_chr);
                        assert_eq!(rtl, expected_rtl, "direction: {source}");
                        let actual_x = if rtl {
                            bounds.x1 as f32 - positions[i] - widths[i]
                        } else {
                            bounds.x0 as f32 + positions[i]
                        };
                        assert!((actual_x - x).abs() < 0.02, "character bounds: {source}");
                        assert!((widths[i] - width).abs() < 0.02);
                        assert!(
                            positions[i] >= -0.02
                                && f64::from(positions[i] + widths[i]) <= bounds.width() + 0.02
                        );
                        offset += 1;
                    }
                }
                for row in &galley.rows {
                    let rect = row.rect_without_leading_space().expand(0.02);
                    for glyph in &row.glyphs {
                        let glyph_rect = glyph.logical_rect().translate(row.pos.to_vec2());
                        assert!(
                            rect.left() <= glyph_rect.left() && rect.right() >= glyph_rect.right(),
                            "row horizontal bounds {rect:?}, glyph {glyph_rect:?}: {source}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn accessibility_selection_crosses_bidi_runs_and_deletes_logical_text() {
    let ctx = egui::Context::default();
    crate::theme::install(&ctx);
    ctx.enable_accesskit();
    let id = egui::Id::unique("bidi-selection");
    let mut text = "ABC אבג 123 العربية XYZ".to_owned();
    let original = text.clone();
    let render = |text: &mut String, events| {
        let mut output = ctx.run_ui(
            egui::RawInput {
                events,
                ..Default::default()
            },
            |ui| {
                egui::TextEdit::singleline(text).id(id).show(ui);
            },
        );
        output.textures_delta.clear();
        output
    };
    render(&mut text, vec![]);
    ctx.memory_mut(|m| m.request_focus(id));
    let tree = render(&mut text, vec![])
        .platform_output
        .accesskit_update
        .unwrap();
    let parent = &tree
        .nodes
        .iter()
        .find(|(node_id, _)| *node_id == id.accesskit_id())
        .unwrap()
        .1;
    assert!(parent.children().len() >= 4);
    let end_node = parent.children()[parent.children().len() - 1];
    let last_length = tree
        .nodes
        .iter()
        .find(|(node_id, _)| *node_id == end_node)
        .unwrap()
        .1
        .character_lengths()
        .len();
    let selection = egui::accesskit::TextSelection {
        anchor: egui::accesskit::TextPosition {
            node: parent.children()[0],
            character_index: 1,
        },
        focus: egui::accesskit::TextPosition {
            node: end_node,
            character_index: last_length - 1,
        },
    };
    render(
        &mut text,
        vec![Event::AccessKitActionRequest(
            egui::accesskit::ActionRequest {
                action: egui::accesskit::Action::SetTextSelection,
                target_tree: egui::accesskit::TreeId::ROOT,
                target_node: id.accesskit_id(),
                data: Some(egui::accesskit::ActionData::SetTextSelection(selection)),
            },
        )],
    );
    let range = egui::TextEdit::load_state(&ctx, id)
        .unwrap()
        .cursor
        .char_range()
        .unwrap()
        .as_sorted_char_range();
    assert_eq!(
        (range.start.0, range.end.0),
        (1, original.chars().count() - 1)
    );
    render(&mut text, vec![key(Key::Backspace, Modifiers::NONE)]);
    assert_eq!(text, "AZ");
}
#[test]
fn keyboard_search_text_and_tab_focus_work() {
    let ctx = egui::Context::default();
    let mut app = Prototype::new(&ctx);
    frame(&ctx, &mut app, [1200.0, 800.0], vec![]);
    frame(
        &ctx,
        &mut app,
        [1200.0, 800.0],
        vec![key(Key::K, Modifiers::CTRL)],
    );
    assert_eq!(app.page, Page::Search);
    assert!(ctx.memory(|m| m.has_focus(egui::Id::unique("catalog-search"))));
    frame(
        &ctx,
        &mut app,
        [1200.0, 800.0],
        vec![Event::Text("東京".into())],
    );
    assert_eq!(app.query, "東京");
    assert!(!app.filtered.is_empty());
    let before = app.session.playing;
    frame(
        &ctx,
        &mut app,
        [1200.0, 800.0],
        vec![key(Key::Space, Modifiers::NONE), Event::Text(" ".into())],
    );
    assert_eq!(
        app.session.playing, before,
        "Typing spaces must not toggle playback"
    );
    frame(
        &ctx,
        &mut app,
        [1200.0, 800.0],
        vec![key(Key::Tab, Modifiers::NONE)],
    );
    assert!(ctx.memory(|m| m.focused().is_some()));
    assert!(!ctx.memory(|m| m.has_focus(egui::Id::unique("catalog-search"))));
}
#[test]
fn large_library_virtualizes_and_scrolls_without_building_all_rows() {
    let ctx = egui::Context::default();
    let mut app = Prototype::new(&ctx);
    app.page = Page::Library;
    let mut maximum = 0;
    for _ in 0..8 {
        frame(
            &ctx,
            &mut app,
            [1200.0, 800.0],
            vec![
                Event::PointerMoved(egui::pos2(650.0, 480.0)),
                Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -1000.0),
                    phase: egui::TouchPhase::Move,
                    modifiers: Modifiers::NONE,
                },
            ],
        );
        maximum = maximum.max(app.rendered_rows);
    }
    assert!(
        maximum > 0 && maximum < 24,
        "Rendered {maximum} rows of 10000"
    );
}
#[test]
fn accessibility_names_and_small_layout_scenarios_are_present() {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut app = Prototype::new(&ctx);
    app.page = Page::Library;
    frame(&ctx, &mut app, [1200.0, 800.0], vec![]);
    let output = frame(&ctx, &mut app, [1200.0, 800.0], vec![]);
    let tree = output.platform_output.accesskit_update.unwrap();
    let labels: Vec<String> = tree
        .nodes
        .iter()
        .filter_map(|(_, node)| node.label().map(str::to_owned))
        .collect();
    assert!(
        labels.iter().any(|s| s == "Search sample library"),
        "{labels:?}"
    );
    assert!(
        labels.iter().any(|s| s == "Playback position"),
        "{labels:?}"
    );
    assert!(
        labels
            .iter()
            .any(|s| s.starts_with("Play In the waking light")),
        "{labels:?}"
    );
    for size in [
        [1200.0, 800.0],
        [800.0, 540.0],
        [480.0, 640.0],
        [350.0, 450.0],
    ] {
        for page in [
            Page::Home,
            Page::Search,
            Page::Library,
            Page::Collection(0),
            Page::Collection(1),
            Page::Queue,
            Page::Lyrics,
        ] {
            app.page = page;
            for state in ViewState::ALL {
                app.state = state;
                let output = frame(&ctx, &mut app, size, vec![]);
                assert!(!output.shapes.is_empty());
            }
        }
    }
}

fn click_named(ctx: &egui::Context, app: &mut Prototype, name: &str) {
    let output = frame(ctx, app, [1200.0, 800.0], vec![]);
    let tree = output.platform_output.accesskit_update.unwrap();
    let rect = tree
        .nodes
        .iter()
        .find_map(|(_, node)| {
            if node.label() == Some(name) {
                node.bounds()
            } else {
                None
            }
        })
        .unwrap_or_else(|| panic!("Missing accessible action: {name}"));
    let pos = egui::pos2(
        ((rect.x0 + rect.x1) * 0.5) as f32,
        ((rect.y0 + rect.y1) * 0.5) as f32,
    );
    for pressed in [true, false] {
        frame(
            ctx,
            app,
            [1200.0, 800.0],
            vec![
                Event::PointerMoved(pos),
                Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Modifiers::NONE,
                },
            ],
        );
    }
}
#[test]
fn collection_to_queue_and_recovery_work_through_widgets() {
    let ctx = egui::Context::default();
    ctx.enable_accesskit();
    let mut app = Prototype::new(&ctx);
    frame(&ctx, &mut app, [1200.0, 800.0], vec![]);
    click_named(&ctx, &mut app, "Explore Golden hour  →");
    assert_eq!(app.page, Page::Collection(1));
    click_named(&ctx, &mut app, "Play collection");
    assert!(app.session.playing);
    assert_eq!(app.session.current(), Some(6));
    click_named(&ctx, &mut app, "View queue");
    assert_eq!(app.page, Page::Queue);
    click_named(&ctx, &mut app, "Clear queue");
    assert!(app.session.queue.is_empty());
    assert!(!app.session.playing);
    click_named(&ctx, &mut app, "Add Golden hour");
    assert_eq!(app.session.queue.len(), 6);
    app.state = ViewState::Error;
    click_named(&ctx, &mut app, "Try again");
    assert_eq!(app.state, ViewState::Ready);
    assert_eq!(app.session.queue.len(), 6);
}
#[test]
fn scrolling_benchmark_keeps_keyboard_responsive() {
    let ctx = egui::Context::default();
    let mut app = Prototype::new(&ctx);
    app.page = Page::Library;
    for _ in 0..10 {
        frame(&ctx, &mut app, [1200.0, 800.0], vec![]);
    }
    let mut times = Vec::new();
    let mut max_rows = 0;
    for _ in 0..120 {
        let started = std::time::Instant::now();
        frame(
            &ctx,
            &mut app,
            [1200.0, 800.0],
            vec![
                Event::PointerMoved(egui::pos2(650.0, 480.0)),
                Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, -150.0),
                    phase: egui::TouchPhase::Move,
                    modifiers: Modifiers::NONE,
                },
            ],
        );
        times.push(started.elapsed().as_secs_f64() * 1000.0);
        max_rows = max_rows.max(app.rendered_rows);
    }
    frame(
        &ctx,
        &mut app,
        [1200.0, 800.0],
        vec![key(Key::K, Modifiers::CTRL)],
    );
    frame(
        &ctx,
        &mut app,
        [1200.0, 800.0],
        vec![Event::Text("Tokyo".into())],
    );
    assert_eq!(app.page, Page::Search);
    assert_eq!(app.query, "Tokyo");
    assert!(!app.filtered.is_empty());
    times.sort_by(f64::total_cmp);
    println!(
        "native_scroll_benchmark tracks=10000 frames=120 max_rows={max_rows} p50_ms={:.3} p95_ms={:.3} max_ms={:.3}",
        times[60], times[114], times[119]
    );
    assert!(max_rows < 24);
}
