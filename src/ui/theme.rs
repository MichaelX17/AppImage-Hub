use eframe::egui::{self, Color32, CornerRadius, Margin, RichText, Stroke, Style, Visuals};

pub const BG: Color32 = Color32::from_rgb(23, 24, 33);
pub const SURFACE: Color32 = Color32::from_rgb(34, 36, 49);
pub const SURFACE_RAISED: Color32 = Color32::from_rgb(41, 43, 58);
pub const SURFACE_HOVER: Color32 = Color32::from_rgb(52, 54, 74);
pub const BORDER: Color32 = Color32::from_rgb(65, 67, 87);

pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(241, 240, 247);
pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(181, 181, 197);
pub const TEXT_MUTED: Color32 = Color32::from_rgb(133, 134, 151);

pub const ACCENT: Color32 = Color32::from_rgb(124, 58, 237);
pub const ACCENT_HOVER: Color32 = Color32::from_rgb(145, 92, 244);
pub const SUCCESS: Color32 = Color32::from_rgb(74, 222, 128);
#[allow(dead_code)]
pub const WARNING: Color32 = Color32::from_rgb(245, 158, 11);
pub const DANGER: Color32 = Color32::from_rgb(217, 65, 65);
pub const NEUTRAL: Color32 = Color32::from_rgb(72, 74, 92);

#[allow(dead_code)]
pub const RADIUS_SM: u8 = 6;
pub const RADIUS_MD: u8 = 10;
pub const RADIUS_LG: u8 = 12;

pub const SPACE_XS: f32 = 4.0;
pub const SPACE_SM: f32 = 8.0;
pub const SPACE_MD: f32 = 12.0;
pub const SPACE_LG: f32 = 16.0;
#[allow(dead_code)]
pub const SPACE_XL: f32 = 24.0;

pub fn apply_style(ctx: &egui::Context, compact: bool) {
    let mut visuals = Visuals::dark();
    visuals.override_text_color = Some(TEXT_PRIMARY);
    visuals.panel_fill = BG;
    visuals.window_fill = BG;
    visuals.extreme_bg_color = SURFACE_RAISED;
    visuals.faint_bg_color = SURFACE;
    visuals.widgets.noninteractive.bg_fill = BG;
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, TEXT_SECONDARY);
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, BORDER);
    visuals.widgets.inactive.bg_fill = SURFACE_RAISED;
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, TEXT_PRIMARY);
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, BORDER);
    visuals.widgets.hovered.bg_fill = SURFACE_HOVER;
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, TEXT_PRIMARY);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, ACCENT_HOVER);
    visuals.widgets.active.bg_fill = ACCENT;
    visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, Color32::WHITE);
    visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, ACCENT_HOVER);
    visuals.widgets.open.bg_fill = SURFACE_RAISED;
    visuals.widgets.open.fg_stroke = Stroke::new(1.0_f32, TEXT_PRIMARY);
    visuals.widgets.open.bg_stroke = Stroke::new(1.0_f32, BORDER);
    visuals.selection.bg_fill = ACCENT;
    visuals.selection.stroke = Stroke::new(1.0_f32, Color32::WHITE);
    visuals.hyperlink_color = ACCENT_HOVER;

    let rounding = CornerRadius::same(RADIUS_MD);
    for w in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        w.corner_radius = rounding;
    }

    let mut style = Style {
        visuals,
        ..Default::default()
    };
    let s = if compact { 0.82 } else { 1.0 };
    style.spacing.item_spacing = egui::vec2(SPACE_SM * s, SPACE_SM * s);
    style.spacing.button_padding = egui::vec2(12.0 * s, 6.0 * s);
    style.spacing.interact_size.y = 30.0 * s;
    style.spacing.window_margin = Margin::same(SPACE_MD as i8);
    ctx.set_style(style);
}

pub fn apply(ctx: &egui::Context) {
    apply_style(ctx, false);
}

pub fn card_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(SURFACE)
        .stroke(Stroke::new(1.0_f32, BORDER))
        .corner_radius(CornerRadius::same(RADIUS_LG))
        .inner_margin(Margin::same(SPACE_LG as i8))
}

pub fn panel_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(SURFACE)
        .stroke(Stroke::new(1.0_f32, BORDER))
        .inner_margin(Margin::same(SPACE_LG as i8))
}

pub fn primary_button(text: impl Into<String>) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text.into()).color(Color32::WHITE).strong()).fill(ACCENT)
}

pub fn secondary_button(text: impl Into<String>) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text.into()).color(TEXT_PRIMARY)).fill(NEUTRAL)
}

pub fn danger_button(text: impl Into<String>) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text.into()).color(Color32::WHITE)).fill(DANGER)
}

pub fn ghost_button(text: impl Into<String>) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text.into()).color(TEXT_SECONDARY)).fill(SURFACE_RAISED)
}

#[allow(dead_code)]
pub fn status_dot(ui: &mut egui::Ui, color: Color32) {
    let avail = ui.available_size();
    if !avail.x.is_finite() || !avail.y.is_finite() || avail.x < 8.0 || avail.y < 8.0 {
        return;
    }
    let (rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
    if rect.is_finite() && rect.width() > 0.0 && rect.height() > 0.0 {
        ui.painter().circle_filled(rect.center(), 4.0, color);
    }
}

pub fn muted_label(text: impl Into<String>) -> RichText {
    RichText::new(text.into()).color(TEXT_MUTED)
}

pub fn secondary_label(text: impl Into<String>) -> RichText {
    RichText::new(text.into()).color(TEXT_SECONDARY)
}

pub fn section_heading(text: impl Into<String>) -> RichText {
    RichText::new(text.into())
        .strong()
        .size(15.0)
        .color(TEXT_PRIMARY)
}

pub fn field_label(text: impl Into<String>) -> RichText {
    RichText::new(text.into()).strong().color(TEXT_PRIMARY)
}

/// A small inline badge indicating an update is available.
/// Uses both color and text so it is not color-alone.
pub fn update_available_label(tag: &str) -> RichText {
    RichText::new(format!("↑ {} — Update available", tag))
        .strong()
        .color(ACCENT_HOVER)
}

/// Label for the "up to date" version status.
pub fn up_to_date_label(tag: &str) -> RichText {
    RichText::new(format!("✓ {} — Up to date", tag)).color(SUCCESS)
}

/// A compact button styled for a primary update action when an update is available.
/// Visually distinct from the normal ghost Update button.
pub fn update_button() -> egui::Button<'static> {
    egui::Button::new(RichText::new("Update").color(Color32::WHITE).strong()).fill(ACCENT)
}

/// A labeled field row helper: renders `label` above `content` function with consistent spacing.
/// Returns the result of `content`.
#[allow(dead_code)]
pub fn labeled_field<R>(
    ui: &mut egui::Ui,
    label: impl Into<String>,
    content: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    ui.label(field_label(label.into()));
    let r = content(ui);
    ui.add_space(SPACE_MD);
    r
}
