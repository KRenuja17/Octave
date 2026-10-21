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
