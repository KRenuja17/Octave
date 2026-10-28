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
