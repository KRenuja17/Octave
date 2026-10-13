use eframe::egui::{self, Color32, FontFamily, FontId, TextStyle};
pub const BG: Color32 = Color32::from_rgb(16, 21, 25);
pub const PANEL: Color32 = Color32::from_rgb(23, 29, 33);
pub const CARD: Color32 = Color32::from_rgb(30, 38, 42);
pub const TEXT: Color32 = Color32::from_rgb(235, 239, 229);
pub const MUTED: Color32 = Color32::from_rgb(159, 174, 174);
pub const ACCENT: Color32 = Color32::from_rgb(198, 236, 163);
pub fn install(ctx: &egui::Context) {
    let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
    style.visuals = egui::Visuals::dark();
    style.visuals.panel_fill = BG;
    style.visuals.window_fill = PANEL;
    style.visuals.extreme_bg_color = BG;
    style.visuals.override_text_color = Some(TEXT);
    style.visuals.selection.bg_fill = Color32::from_rgb(60, 83, 64);
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
        FontId::new(30.0, FontFamily::Proportional),
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
    ctx.set_theme(egui::Theme::Dark);
    ctx.set_style_of(egui::Theme::Dark, style);
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
