use eframe::egui::{self, Color32, FontFamily, FontId, TextStyle};
pub const BG: Color32 = Color32::from_rgb(18, 29, 49);
pub const PANEL: Color32 = Color32::from_rgb(27, 41, 64);
pub const CARD: Color32 = Color32::from_rgb(38, 54, 78);
pub const TEXT: Color32 = Color32::from_rgb(232, 237, 245);
pub const MUTED: Color32 = Color32::from_rgb(157, 171, 190);
pub const ACCENT: Color32 = Color32::from_rgb(223, 183, 124);
pub fn install(ctx: &egui::Context) {
    apply(ctx, 0);
    install_fonts(ctx);
}
pub fn apply(ctx: &egui::Context, appearance: u8) {
    let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
    style.visuals = egui::Visuals::dark();
    style.visuals.panel_fill = BG;
    style.visuals.window_fill = PANEL;
    style.visuals.extreme_bg_color = BG;
    style.visuals.override_text_color = Some(TEXT);
    style.visuals.selection.bg_fill = Color32::from_rgb(68, 60, 49);
    style.visuals.weak_text_color = Some(MUTED);
    style.visuals.selection.stroke = egui::Stroke::new(1.0, ACCENT);
    style.visuals.handle_shape = egui::style::HandleShape::Circle;
    style.visuals.widgets.inactive.bg_fill = CARD;
    style.visuals.widgets.inactive.weak_bg_fill = CARD;
    style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(48, 66, 92);
    style.visuals.widgets.active.bg_fill = Color32::from_rgb(74, 64, 48);
    style.spacing.item_spacing = egui::vec2(10.0, 8.0);
    style.spacing.button_padding = egui::vec2(13.0, 8.0);
    style.spacing.interact_size.y = 34.0;
    style.spacing.slider_width = 160.0;
    style.text_styles.insert(
        TextStyle::Heading,
        FontId::new(32.0, FontFamily::Name("Display".into())),
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
        style.visuals.panel_fill = Color32::from_rgb(237, 241, 246);
        style.visuals.window_fill = Color32::from_rgb(222, 229, 239);
        style.visuals.override_text_color = Some(Color32::from_rgb(34, 51, 74));
        style.visuals.selection.bg_fill = Color32::from_rgb(222, 199, 163);
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
    let mut display = fonts.families[&FontFamily::Proportional].clone();
    if let Ok(bytes) = std::fs::read("C:/Windows/Fonts/georgia.ttf") {
        fonts.font_data.insert(
            "Octave display".into(),
            egui::FontData::from_owned(bytes).into(),
        );
        display.insert(0, "Octave display".into());
    }
    fonts
        .families
        .insert(FontFamily::Name("Display".into()), display);
    ctx.set_fonts(fonts);
}
/// Original vector record mark; no artwork or remote assets needed.
pub fn record(ui: &mut egui::Ui, size: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let center = rect.center();
    let ink = ui.visuals().weak_text_color();
    for fraction in [0.48, 0.43, 0.38, 0.33] {
        ui.painter().circle_stroke(
            center,
            size * fraction,
            egui::Stroke::new(1.0, ink.gamma_multiply(0.45)),
        );
    }
    ui.painter().circle_filled(center, size * 0.19, ACCENT);
    ui.painter().circle_filled(center, size * 0.035, BG);
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
