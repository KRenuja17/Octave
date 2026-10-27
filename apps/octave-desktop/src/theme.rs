use eframe::egui::{self, Color32, FontFamily, FontId, TextStyle};
pub const BG: Color32 = Color32::from_rgb(16, 21, 25);
pub const PANEL: Color32 = Color32::from_rgb(23, 29, 33);
pub const CARD: Color32 = Color32::from_rgb(30, 38, 42);
pub const TEXT: Color32 = Color32::from_rgb(235, 239, 229);
pub const MUTED: Color32 = Color32::from_rgb(159, 174, 174);
pub const ACCENT: Color32 = Color32::from_rgb(198, 236, 163);
pub fn install(ctx: &egui::Context) {
    apply(ctx, 0);
    install_fonts(ctx);
}
pub fn apply(ctx: &egui::Context, appearance: u8) {
    let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
    style.visuals = egui::Visuals::dark();
    style.visuals.panel_fill = BG;
    style.visuals.window_fill = PANEL;
    style.visuals.extreme_bg_color = CARD;
    style.visuals.override_text_color = Some(TEXT);
    style.visuals.selection.bg_fill = Color32::from_rgb(60, 83, 64);
    style.visuals.weak_text_color = Some(MUTED);
    style.visuals.selection.stroke = egui::Stroke::new(1.0, ACCENT);
    style.visuals.handle_shape = egui::style::HandleShape::Circle;
    style.visuals.widgets.inactive.bg_fill = CARD;
    style.visuals.widgets.inactive.weak_bg_fill = CARD;
    style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(50, 63, 59);
    style.visuals.widgets.active.bg_fill = Color32::from_rgb(62, 83, 63);
    style.spacing.item_spacing = egui::vec2(10.0, 8.0);
    style.spacing.button_padding = egui::vec2(13.0, 8.0);
    style.spacing.interact_size.y = 34.0;
    style.spacing.slider_width = 160.0;
    style.text_styles.insert(
        TextStyle::Heading,
        FontId::new(28.0, FontFamily::Proportional),
    );
    style
        .text_styles
        .insert(TextStyle::Body, FontId::new(15.0, FontFamily::Proportional));
    style.text_styles.insert(
        TextStyle::Button,
        FontId::new(14.0, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Small,
        FontId::new(12.0, FontFamily::Proportional),
    );
    let theme = if appearance == 1 {
        egui::Theme::Light
    } else {
        egui::Theme::Dark
    };
    if appearance == 1 {
        style.visuals = egui::Visuals::light();
        style.visuals.panel_fill = Color32::from_rgb(244, 246, 240);
        style.visuals.override_text_color = Some(Color32::from_rgb(24, 31, 26));
        style.visuals.selection.bg_fill = Color32::from_rgb(178, 207, 159);
    } else if appearance == 2 {
        style.visuals.panel_fill = Color32::BLACK;
        style.visuals.window_fill = Color32::BLACK;
        style.visuals.extreme_bg_color = Color32::BLACK;
        style.visuals.override_text_color = Some(Color32::WHITE);
        style.visuals.weak_text_color = Some(Color32::from_gray(220));
        style.visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, Color32::WHITE);
        style.visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, Color32::WHITE);
    }
    ctx.set_theme(theme);
    ctx.set_style_of(theme, style);
}
fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    // Read installed Windows fonts; never redistribute operating-system font files.
    for (name, path, primary) in [
        ("Segoe", "C:/Windows/Fonts/segoeui.ttf", true),
        ("Japanese", "C:/Windows/Fonts/msgothic.ttc", false),
        ("Korean", "C:/Windows/Fonts/malgun.ttf", false),
        ("Thai and Lao", "C:/Windows/Fonts/LeelawUI.ttf", false),
        ("Indic", "C:/Windows/Fonts/Nirmala.ttf", false),
    ] {
        if let Ok(bytes) = std::fs::read(path) {
            fonts
                .font_data
                .insert(name.into(), egui::FontData::from_owned(bytes).into());
            let family = fonts.families.entry(FontFamily::Proportional).or_default();
            if primary {
                family.insert(0, name.into());
            } else {
                family.push(name.into());
            }
        }
    }
    ctx.set_fonts(fonts);
}
pub fn eyebrow(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).size(11.0).strong().color(ACCENT));
}
pub fn muted(ui: &mut egui::Ui, text: impl Into<String>) {
    ui.label(egui::RichText::new(text.into()).color(MUTED));
}
pub fn primary(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(
        egui::Button::new(egui::RichText::new(text).color(BG).strong())
            .fill(ACCENT)
            .corner_radius(18.0),
    )
}

/// Neutral cover placeholder for tracks without artwork metadata.
pub fn cover(ui: &mut egui::Ui, size: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, 4.0, ui.visuals().widgets.inactive.bg_fill);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "\u{266b}",
        FontId::proportional(size * 0.5),
        ui.visuals().text_color(),
    );
}

pub fn nav(ui: &mut egui::Ui, selected: bool, text: impl Into<egui::WidgetText>) -> egui::Response {
    let button = egui::Button::new(text)
        .selected(selected)
        .frame(selected)
        .corner_radius(8.0);
    if ui.layout().main_dir() == egui::Direction::TopDown {
        ui.add_sized([168.0, 42.0], button)
    } else {
        ui.add(button)
    }
}

pub fn transport_button(
    ui: &mut egui::Ui,
    icon: &str,
    label: &str,
    enabled: bool,
    primary: bool,
) -> egui::Response {
    let mut button = egui::Button::new(egui::RichText::new(icon).size(20.0).color(if primary {
        BG
    } else {
        ui.visuals().text_color()
    }))
    .min_size(egui::vec2(40.0, 40.0))
    .corner_radius(20.0)
    .frame(primary);
    if primary {
        button = button.fill(ACCENT);
    }
    let response = ui.add_enabled(enabled, button).on_hover_text(label);
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    response
}

/// Shared settings/account surface, using the active appearance rather than fixed colors.
pub fn surface<R>(
    ui: &mut egui::Ui,
    content: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    egui::Frame::new()
        .fill(ui.visuals().window_fill)
        .corner_radius(12.0)
        .inner_margin(if ui.available_width() < 600.0 {
            12.0
        } else {
            16.0
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
            content(ui)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn renderer_context() -> egui::Context {
        let ctx = egui::Context::default();
        install(&ctx);
        let mut output = ctx.run_ui(egui::RawInput::default(), |_| {});
        output.textures_delta.clear();
        ctx
    }

    #[test]
    #[ignore = "Explicit renderer qualification: paragraphs, isolates and fallback scripts"]
    fn rtl_renderer_complex_paragraphs_and_fallback() {
        let ctx = renderer_context();
        for text in [
            "אבג abc\nمرحبا 123\n\nEnglish שלום",
            "שלום \u{2066}abc 123 def ghi\u{2069} עולם",
            "abc \u{2067}مرحبا 123 بالعالم\u{2069} xyz",
            "بِسْمِ 日本語 한국어 ไทย हिन्दी שלום",
        ] {
            for width in [80.0, 180.0, 600.0] {
                let galley = ctx.fonts_mut(|fonts| {
                    fonts.layout(text.into(), FontId::proportional(26.0), TEXT, width)
                });
                let mut logical = String::new();
                for placed in &galley.rows {
                    logical.push_str(&placed.row.text());
                    if placed.ends_with_newline {
                        logical.push('\n');
                    }
                    assert!(placed.row.glyphs.iter().all(|g| {
                        g.pos.x.is_finite()
                            && g.pos.y.is_finite()
                            && g.advance_width.is_finite()
                            && g.advance_width >= 0.0
                    }));
                }
                assert_eq!(logical, text, "width {width}");
                assert_eq!(galley.end().index.0, text.chars().count());
            }
        }
    }

    #[test]
    #[ignore = "Explicit renderer qualification: missing-glyph emoji joiner fallback"]
    fn rtl_renderer_emoji_fallback_preserves_logical_text() {
        let ctx = renderer_context();
        let text = "שלום 👩‍💻 🎵 e\u{301} بِسْمِ";
        for width in [80.0, 180.0, 600.0] {
            let galley = ctx.fonts_mut(|fonts| {
                fonts.layout(text.into(), FontId::proportional(26.0), TEXT, width)
            });
            let logical: String = galley.rows.iter().map(|r| r.row.text()).collect();
            assert_eq!(
                logical, text,
                "Fallback must preserve the ZWJ at width {width}"
            );
        }
    }

    #[test]
    #[ignore = "Explicit Windows font coverage qualification; requires installed script fonts"]
    fn rtl_renderer_installed_font_fallback_coverage() {
        let installed = renderer_context();
        let defaults = egui::Context::default();
        let mut output = defaults.run_ui(egui::RawInput::default(), |_| {});
        output.textures_delta.clear();
        let id = FontId::proportional(26.0);
        let mut fallback_cases = 0;
        for text in ["日本語", "한국어", "ไทย", "हिन्दी", "مرحبا", "שלום"]
        {
            let before = defaults.fonts_mut(|f| f.has_glyphs(&id, text));
            let after = installed.fonts_mut(|f| f.has_glyphs(&id, text));
            eprintln!("RTL_COVERAGE {text:?} bundled={before} installed={after}");
            assert!(after, "Installed font coverage missing for {text:?}");
            fallback_cases += usize::from(!before && after);
        }
        assert!(
            fallback_cases > 0,
            "Fixture must actually exercise added fallback coverage"
        );
    }

    #[test]
    #[ignore = "Explicit renderer measurement: uncached large mixed-direction text"]
    fn rtl_renderer_large_text_measurement() {
        // Fresh contexts and unique paragraphs avoid galley/paragraph cache hits.
        // Timings are diagnostic, not a machine-dependent pass threshold.
        for paragraphs in [false, true] {
            for repeats in [100, 500, 2000] {
                let ctx = renderer_context();
                let unit = if paragraphs {
                    "שלום abc 123 مرحبا بالعالم\n"
                } else {
                    "שלום abc 123 مرحبا بالعالم "
                };
                let text: String = (0..repeats).map(|i| format!("{i} {unit}")).collect();
                let chars = text.chars().count();
                let started = std::time::Instant::now();
                let galley = ctx
                    .fonts_mut(|fonts| fonts.layout(text, FontId::proportional(26.0), TEXT, 180.0));
                let elapsed = started.elapsed();
                assert_eq!(galley.end().index.0, chars);
                assert!(galley.rows.len() >= repeats);
                eprintln!(
                    "RTL_PERF paragraphs={paragraphs} chars={chars} rows={} elapsed_us={}",
                    galley.rows.len(),
                    elapsed.as_micros()
                );
            }
        }
    }

    #[test]
    #[ignore = "Explicit renderer qualification: Arabic marks and wrapped logical text"]
    fn rtl_renderer_arabic_marks_preserve_logical_text_when_wrapped() {
        let ctx = renderer_context();
        for text in ["بِسْمِ", "مرحبا 123 بالعالم", "שלום abc 123 עולם", "لا ﷺ"]
        {
            for width in [80.0, 300.0] {
                let galley = ctx.fonts_mut(|fonts| {
                    fonts.layout(text.into(), FontId::proportional(26.0), TEXT, width)
                });
                let logical: String = galley.rows.iter().map(|r| r.row.text()).collect();
                assert_eq!(logical, text, "Logical text at width {width}");
                assert_eq!(galley.end().index.0, text.chars().count());
                assert!(
                    galley
                        .rows
                        .iter()
                        .flat_map(|r| &r.row.glyphs)
                        .all(|g| g.pos.x.is_finite() && g.pos.y.is_finite())
                );
            }
        }
    }

    #[test]
    #[ignore = "Explicit renderer qualification: Hebrew cursor round trips"]
    fn rtl_renderer_cursor_round_trips() {
        use egui::epaint::text::{CharIndex, cursor::CCursor};
        let ctx = renderer_context();
        let text = "אבג";
        let galley = ctx
            .fonts_mut(|fonts| fonts.layout_no_wrap(text.into(), FontId::proportional(26.0), TEXT));
        for index in 0..=text.chars().count() {
            let cursor = CCursor {
                index: CharIndex(index),
                prefer_next_row: false,
            };
            let rect = galley.pos_from_cursor(cursor);
            assert_eq!(
                galley.cursor_from_pos(rect.center().to_vec2()).index.0,
                index
            );
        }
    }

    #[test]
    #[ignore = "Explicit renderer qualification: wrapped RTL trailing whitespace"]
    fn rtl_renderer_wrapped_trailing_whitespace_uses_paragraph_direction() {
        let ctx = renderer_context();
        let text = "אב abc def ghi jkl mno pqr stu";
        let mut checked = 0;
        for width in [100.0, 140.0, 180.0] {
            let galley = ctx.fonts_mut(|fonts| {
                fonts.layout(text.into(), FontId::proportional(26.0), TEXT, width)
            });
            for placed in &galley.rows {
                let glyphs = &placed.row.glyphs;
                if let Some(last) = glyphs.last().filter(|g| g.chr == ' ')
                    && glyphs.iter().any(|g| g.chr.is_ascii_alphabetic())
                {
                    checked += 1;
                    let first_content = glyphs
                        .iter()
                        .filter(|g| !g.chr.is_whitespace())
                        .map(|g| g.pos.x)
                        .fold(f32::INFINITY, f32::min);
                    assert!(
                        last.pos.x <= first_content,
                        "L1: trailing whitespace belongs at the RTL paragraph edge: {:?}",
                        placed.row.text()
                    );
                }
            }
        }
        assert!(
            checked > 0,
            "Fixture must exercise wrapped trailing whitespace"
        );
    }

    #[test]
    #[ignore = "Explicit renderer qualification: discontiguous mixed-direction selection"]
    fn rtl_renderer_selection_excludes_unselected_visual_neighbors() {
        use egui::epaint::text::{CharIndex, cursor::CCursor};
        use egui::text_selection::{CCursorRange, visuals::paint_text_selection};
        let ctx = renderer_context();
        let text = "abc אבג xyz";
        let mut galley = ctx
            .fonts_mut(|fonts| fonts.layout_no_wrap(text.into(), FontId::proportional(26.0), TEXT));
        assert_eq!(galley.rows[0].row.text(), text);
        let selection = CCursorRange::two(
            CCursor {
                index: CharIndex(0),
                prefer_next_row: false,
            },
            CCursor {
                index: CharIndex(5),
                prefer_next_row: false,
            },
        );
        assert_eq!(selection.slice_str(text), "abc א");
        let mut regions = Vec::new();
        paint_text_selection(
            &mut galley,
            &ctx.style_of(egui::Theme::Dark).visuals,
            &selection,
            Some(&mut regions),
        );
        let row = &galley.rows[0].row;
        let rects: Vec<_> = regions
            .iter()
            .map(|region| {
                let mut bounds = egui::Rect::NOTHING;
                for index in region.vertex_indices {
                    bounds.extend_with(row.visuals.mesh.vertices[index as usize].pos);
                }
                bounds
            })
            .collect();
        for (index, glyph) in row
            .glyphs
            .iter()
            .enumerate()
            .filter(|(_, g)| !g.chr.is_whitespace())
        {
            let selected = rects
                .iter()
                .any(|rect| rect.contains(glyph.logical_rect().center()));
            assert_eq!(
                selected,
                index < 5,
                "Selection background for logical glyph {index} ({})",
                glyph.chr
            );
        }
    }

    /// A renderer adoption gate, not part of ordinary offline acceptance.
    /// Keep this failing result visible until bidi layout is implemented.
    #[test]
    #[ignore = "Known RTL renderer limitation; run explicitly when evaluating a bidi fix"]
    fn rtl_renderer_preserves_logical_text_and_visual_order() {
        let ctx = egui::Context::default();
        install(&ctx);
        let mut output = ctx.run_ui(egui::RawInput::default(), |_| {});
        output.textures_delta.clear();
        for (text, visual) in [("אב", "בא"), ("ab אב", "ab בא"), ("אב 12", "12 בא")] {
            let galley = ctx.fonts_mut(|fonts| {
                fonts.layout_no_wrap(text.into(), FontId::proportional(26.0), TEXT)
            });
            let row = &galley.rows[0].row;
            assert_eq!(
                row.text(),
                text,
                "Logical glyph order must support selection and accessibility"
            );
            let mut glyphs: Vec<_> = row.glyphs.iter().collect();
            glyphs.sort_by(|a, b| a.pos.x.total_cmp(&b.pos.x));
            let actual: String = glyphs.iter().map(|g| g.chr).collect();
            assert_eq!(actual, visual, "Visual ordering of {text:?}");
        }
    }

    #[test]
    #[ignore = "Explicit renderer qualification: every plain and bidi selection range"]
    fn rtl_renderer_selection_ranges_match_logical_glyphs() {
        use egui::epaint::text::{CharIndex, cursor::CCursor};
        use egui::text_selection::{CCursorRange, visuals::paint_text_selection};
        let ctx = renderer_context();
        for text in ["abc", "אבג", "abc אבג xyz"] {
            let original = ctx.fonts_mut(|fonts| {
                fonts.layout_no_wrap(text.into(), FontId::proportional(26.0), TEXT)
            });
            assert_eq!(original.rows[0].row.text(), text);
            for start in 0..text.chars().count() {
                for end in start + 1..=text.chars().count() {
                    let mut galley = original.clone();
                    let selection = CCursorRange::two(
                        CCursor {
                            index: CharIndex(start),
                            prefer_next_row: false,
                        },
                        CCursor {
                            index: CharIndex(end),
                            prefer_next_row: false,
                        },
                    );
                    let mut regions = Vec::new();
                    paint_text_selection(
                        &mut galley,
                        &ctx.style_of(egui::Theme::Dark).visuals,
                        &selection,
                        Some(&mut regions),
                    );
                    let row = &galley.rows[0].row;
                    let rects: Vec<_> = regions
                        .iter()
                        .map(|region| {
                            let mut rect = egui::Rect::NOTHING;
                            for index in region.vertex_indices {
                                rect.extend_with(row.visuals.mesh.vertices[index as usize].pos);
                            }
                            rect
                        })
                        .collect();
                    for (index, glyph) in row.glyphs.iter().enumerate() {
                        if glyph.advance_width > 0.0 {
                            let selected = rects
                                .iter()
                                .any(|r| r.contains(glyph.logical_rect().center()));
                            assert_eq!(
                                selected,
                                (start..end).contains(&index),
                                "{text:?} selection {start}..{end}, glyph {index}"
                            );
                        }
                    }
                    assert_eq!(
                        selection.slice_str(text),
                        text.chars()
                            .skip(start)
                            .take(end - start)
                            .collect::<String>()
                    );
                }
            }
        }
    }
}
