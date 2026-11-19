use eframe::egui::{self, Color32, CornerRadius, Margin, Rect, RichText, Stroke, Style, Visuals};

// ─── Base surfaces ─────────────────────────────────────────────────────────
/// Application background: very dark, rich purple/grey (`#121016`).
pub const BG: Color32 = Color32::from_rgb(0x12, 0x10, 0x16);
/// Cards and panels sit just above the background (`#1A1625`).
pub const SURFACE: Color32 = Color32::from_rgb(0x1A, 0x16, 0x25);
/// Inputs, thumbnails and other raised fills.
pub const SURFACE_RAISED: Color32 = Color32::from_rgb(0x22, 0x1C, 0x30);
pub const SURFACE_HOVER: Color32 = Color32::from_rgb(0x2B, 0x24, 0x3D);
pub const BORDER: Color32 = Color32::from_rgb(0x33, 0x2B, 0x45);

// ─── Text ──────────────────────────────────────────────────────────────────
pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(0xF5, 0xF2, 0xFF);
pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(0xB6, 0xAF, 0xC9);
pub const TEXT_MUTED: Color32 = Color32::from_rgb(0x7E, 0x76, 0x93);

// ─── Accents ───────────────────────────────────────────────────────────────
/// Neon purple used for primary buttons and active states (`#A855F7`).
pub const ACCENT: Color32 = Color32::from_rgb(0xA8, 0x55, 0xF7);
pub const ACCENT_HOVER: Color32 = Color32::from_rgb(0xC0, 0x84, 0xFC);
/// Bright green used for "Up to date" badges (`#22C55E`).
pub const SUCCESS: Color32 = Color32::from_rgb(0x22, 0xC5, 0x5E);
#[allow(dead_code)]
pub const WARNING: Color32 = Color32::from_rgb(0xF5, 0x9E, 0x0B);
pub const DANGER: Color32 = Color32::from_rgb(0xEF, 0x44, 0x44);
pub const NEUTRAL: Color32 = Color32::from_rgb(0x35, 0x2E, 0x47);

/// Translucent accent variants, used as badge backgrounds and glow layers.
/// Stored premultiplied (as egui expects) so they can remain `const`.
pub const ACCENT_SOFT: Color32 = Color32::from_rgba_premultiplied(30, 15, 45, 46);
pub const SUCCESS_SOFT: Color32 = Color32::from_rgba_premultiplied(5, 31, 15, 40);
pub const WARNING_SOFT: Color32 = Color32::from_rgba_premultiplied(38, 25, 2, 40);
pub const DANGER_SOFT: Color32 = Color32::from_rgba_premultiplied(38, 11, 11, 40);
pub const NEUTRAL_SOFT: Color32 = Color32::from_rgba_premultiplied(19, 18, 22, 34);

// ─── Radii ─────────────────────────────────────────────────────────────────
pub const RADIUS_SM: u8 = 6;
pub const RADIUS_MD: u8 = 8;
pub const RADIUS_LG: u8 = 12;
/// Pill radius. egui clamps it to half the widget size, so 255 always
/// produces fully rounded ("pill") ends.
pub const RADIUS_PILL: u8 = 255;

pub const SPACE_XS: f32 = 4.0;
pub const SPACE_SM: f32 = 8.0;
pub const SPACE_MD: f32 = 12.0;
pub const SPACE_LG: f32 = 16.0;
#[allow(dead_code)]
pub const SPACE_XL: f32 = 24.0;

/// Content width of popup menu items (the frame adds its own margin).
pub const MENU_WIDTH: f32 = 200.0;
/// Height of a full-width popup menu action.
pub const MENU_ITEM_HEIGHT: f32 = 36.0;
/// Total height of the overflow menu (frame padding + 3 items + gaps).
pub const MENU_HEIGHT: f32 = 140.0;

pub fn apply_style(ctx: &egui::Context) {
    let mut visuals = Visuals::dark();
    visuals.override_text_color = Some(TEXT_PRIMARY);
    // Show the "pointer clicker" (hand) cursor over all clickable buttons.
    visuals.interact_cursor = Some(egui::CursorIcon::PointingHand);
    visuals.panel_fill = BG;
    // Popups / windows (menus, combo boxes, dialogs): neon frame + glow.
    visuals.window_fill = SURFACE_RAISED;
    visuals.window_stroke = Stroke::new(1.5_f32, with_alpha(ACCENT, 210));
    visuals.window_corner_radius = CornerRadius::same(RADIUS_LG);
    visuals.menu_corner_radius = CornerRadius::same(RADIUS_LG);
    visuals.window_shadow = egui::epaint::Shadow {
        offset: [0, 6],
        blur: 24,
        spread: 1,
        color: with_alpha(ACCENT, 80),
    };
    visuals.popup_shadow = egui::epaint::Shadow {
        offset: [0, 4],
        blur: 20,
        spread: 0,
        color: with_alpha(ACCENT, 70),
    };
    visuals.extreme_bg_color = SURFACE_RAISED;
    visuals.faint_bg_color = SURFACE;

    // Non-interactive widgets (separators, group frames, …)
    visuals.widgets.noninteractive.bg_fill = SURFACE;
    visuals.widgets.noninteractive.weak_bg_fill = SURFACE;
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, TEXT_SECONDARY);
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, BORDER);

    // Idle widgets (default buttons, text edits, checkboxes, …)
    visuals.widgets.inactive.bg_fill = SURFACE_RAISED;
    visuals.widgets.inactive.weak_bg_fill = SURFACE_RAISED;
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, TEXT_PRIMARY);
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, BORDER);

    // Hover
    visuals.widgets.hovered.bg_fill = SURFACE_HOVER;
    visuals.widgets.hovered.weak_bg_fill = SURFACE_HOVER;
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, TEXT_PRIMARY);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, ACCENT_HOVER);
    visuals.widgets.hovered.expansion = 0.0;

    // Active / pressed: neon accent
    visuals.widgets.active.bg_fill = ACCENT;
    visuals.widgets.active.weak_bg_fill = ACCENT;
    visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, Color32::WHITE);
    visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, ACCENT_HOVER);

    visuals.widgets.open.bg_fill = SURFACE_RAISED;
    visuals.widgets.open.weak_bg_fill = SURFACE_RAISED;
    visuals.widgets.open.fg_stroke = Stroke::new(1.0_f32, TEXT_PRIMARY);
    visuals.widgets.open.bg_stroke = Stroke::new(1.0_f32, BORDER);

    visuals.selection.bg_fill = with_alpha(ACCENT, 110);
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
    style.spacing.item_spacing = egui::vec2(SPACE_SM, SPACE_SM);
    style.spacing.button_padding = egui::vec2(14.0, 7.0);
    style.spacing.interact_size.y = 32.0;
    // Larger rounded-square checkbox/radio indicators.
    style.spacing.icon_width = 20.0;
    style.spacing.icon_width_inner = 12.0;
    style.spacing.icon_spacing = 8.0;
    style.spacing.window_margin = Margin::same(SPACE_MD as i8);
    style.spacing.menu_margin = Margin::same(SPACE_SM as i8);
    // Solid scrollbars allocate space beside the content instead of floating
    // on top of cards and form fields.
    style.spacing.scroll = egui::style::ScrollStyle::solid();
    ctx.set_style(style);
}

pub fn apply(ctx: &egui::Context) {
    apply_style(ctx);
}

/// Card surface used for every AppImage row: soft 12 px corners.
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
        // Square top-left corner so the side panel sits flush against the
        // window edge without the dark rounded-corner notch.
        .corner_radius(CornerRadius {
            nw: 0,
            ne: 0,
            sw: RADIUS_LG,
            se: RADIUS_LG,
        })
        .inner_margin(Margin::same(SPACE_LG as i8))
}

/// Common shape for every action button: pill-shaped, fixed height.
fn pill(button: egui::Button<'static>, fill: Color32, min_width: f32) -> egui::Button<'static> {
    button
        .fill(fill)
        .stroke(Stroke::new(1.0_f32, with_alpha(BORDER, 180)))
        .corner_radius(CornerRadius::same(RADIUS_PILL))
        .min_size(egui::vec2(min_width, 32.0))
}

pub fn primary_button(text: impl Into<String>) -> egui::Button<'static> {
    pill(
        egui::Button::new(RichText::new(text.into()).color(Color32::WHITE).strong()),
        ACCENT,
        84.0,
    )
    .stroke(Stroke::new(1.0_f32, ACCENT_HOVER))
}

pub fn secondary_button(text: impl Into<String>) -> egui::Button<'static> {
    pill(
        egui::Button::new(RichText::new(text.into()).color(TEXT_PRIMARY)),
        NEUTRAL,
        84.0,
    )
}

pub fn danger_button(text: impl Into<String>) -> egui::Button<'static> {
    pill(
        egui::Button::new(RichText::new(text.into()).color(Color32::WHITE)),
        DANGER,
        84.0,
    )
}

pub fn ghost_button(text: impl Into<String>) -> egui::Button<'static> {
    pill(
        egui::Button::new(RichText::new(text.into()).color(TEXT_SECONDARY)),
        SURFACE_RAISED,
        64.0,
    )
}

/// The toolbar "Add" action: high-visibility neon pill.
#[allow(dead_code)]
pub fn add_button(text: impl Into<String>) -> egui::Button<'static> {
    pill(
        egui::Button::new(RichText::new(text.into()).color(Color32::WHITE).strong()),
        ACCENT,
        96.0,
    )
    .stroke(Stroke::new(1.0_f32, ACCENT_HOVER))
}

/// Full-width secondary action inside a popup menu ("Add Shortcut…").
/// Soft-rounded (not pill) and slightly lighter than the popup surface.
pub fn menu_secondary_button(text: impl Into<String>) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text.into()).size(14.0).color(TEXT_PRIMARY))
        .fill(SURFACE_HOVER)
        .stroke(Stroke::NONE)
        .corner_radius(CornerRadius::same(RADIUS_MD + 2))
}

/// Full-width destructive action inside a popup menu ("Delete AppImage").
pub fn menu_danger_button(text: impl Into<String>) -> egui::Button<'static> {
    egui::Button::new(
        RichText::new(text.into())
            .size(14.0)
            .strong()
            .color(Color32::WHITE),
    )
    .fill(DANGER)
    .stroke(Stroke::NONE)
    .corner_radius(CornerRadius::same(RADIUS_MD + 2))
}

/// Frame for the card overflow menu: neon border + soft glow.
pub fn menu_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(SURFACE_RAISED)
        .stroke(Stroke::new(1.5_f32, with_alpha(ACCENT, 210)))
        .corner_radius(CornerRadius::same(RADIUS_LG))
        .inner_margin(Margin::same(SPACE_SM as i8))
        .shadow(egui::epaint::Shadow {
            offset: [0, 6],
            blur: 18,
            spread: 0,
            color: with_alpha(ACCENT, 80),
        })
}

/// Where to place the card overflow menu so it opens right under the `•••`
/// button, right-aligned to it, and stays inside the card. If there is not
/// enough room below the button, it flips above it (still inside the card).
pub fn menu_popup_pos(button: Rect, card: Rect, menu_height: f32) -> egui::Pos2 {
    let width = MENU_WIDTH + 2.0 * SPACE_SM;
    let x = (button.right() - width).max(card.left() + SPACE_XS);

    let below = button.bottom() + SPACE_SM;
    let y = if below + menu_height + SPACE_XS <= card.bottom() {
        below
    } else {
        (button.top() - menu_height - SPACE_XS).max(card.top() + SPACE_XS)
    };

    egui::pos2(x, y)
}

/// The card's `•••` overflow toggle. Shows an "active" (accent) state while open.
pub fn menu_toggle_button(open: bool) -> egui::Button<'static> {
    let (fill, color, stroke) = if open {
        (
            ACCENT_SOFT,
            ACCENT_HOVER,
            Stroke::new(1.0_f32, with_alpha(ACCENT, 200)),
        )
    } else {
        (SURFACE_RAISED, TEXT_SECONDARY, Stroke::new(1.0_f32, BORDER))
    };
    egui::Button::new(RichText::new("•••").size(16.0).color(color))
        .fill(fill)
        .stroke(stroke)
        .corner_radius(CornerRadius::same(RADIUS_MD))
        .min_size(egui::vec2(34.0, 28.0))
}

/// Replace the alpha channel of `color`, keeping its RGB.
pub fn with_alpha(color: Color32, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha)
}

/// egui has no CSS `box-shadow`, so a "neon glow" is faked by painting a few
/// rounded-rect strokes of decreasing alpha just outside `rect`.
pub fn draw_glow(ui: &egui::Ui, rect: Rect, corner_radius: u8, color: Color32) {
    if !rect.is_finite() || rect.width() <= 1.0 || rect.height() <= 1.0 {
        return;
    }
    let painter = ui.painter();
    for (expand, alpha, width) in [
        (4.0_f32, 18_u8, 4.0_f32),
        (2.0_f32, 48_u8, 2.5_f32),
        (0.5_f32, 110_u8, 1.5_f32),
    ] {
        let radius = CornerRadius::same(corner_radius.saturating_add(expand as u8));
        painter.rect_stroke(
            rect.expand(expand),
            radius,
            Stroke::new(width, with_alpha(color, alpha)),
            egui::StrokeKind::Outside,
        );
    }
    // Inner accent ring so the card edge itself reads as "active".
    painter.rect_stroke(
        rect,
        CornerRadius::same(corner_radius),
        Stroke::new(1.5_f32, with_alpha(color, 220)),
        egui::StrokeKind::Inside,
    );
}

/// Pill-shaped status badge, e.g. "Up to date" / "Update available".
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadgeKind {
    Success,
    Accent,
    #[allow(dead_code)]
    Warning,
    Danger,
    Neutral,
}

/// Fixed-size status badge so the card layout stays stable regardless of text.
pub fn badge_sized(
    ui: &mut egui::Ui,
    text: impl Into<String>,
    kind: BadgeKind,
    size: egui::Vec2,
) -> egui::Response {
    let (fg, bg) = badge_colors(kind);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());
    if rect.is_finite() && rect.width() > 0.0 && rect.height() > 0.0 {
        let painter = ui.painter();
        painter.rect_filled(rect, CornerRadius::same(RADIUS_PILL), bg);
        painter.rect_stroke(
            rect,
            CornerRadius::same(RADIUS_PILL),
            Stroke::new(1.0_f32, with_alpha(fg, 90)),
            egui::StrokeKind::Inside,
        );
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            text.into(),
            egui::FontId::proportional(11.0),
            fg,
        );
    }
    response
}

fn badge_colors(kind: BadgeKind) -> (Color32, Color32) {
    match kind {
        BadgeKind::Success => (SUCCESS, SUCCESS_SOFT),
        BadgeKind::Accent => (ACCENT_HOVER, ACCENT_SOFT),
        BadgeKind::Warning => (WARNING, WARNING_SOFT),
        BadgeKind::Danger => (DANGER, DANGER_SOFT),
        BadgeKind::Neutral => (TEXT_SECONDARY, NEUTRAL_SOFT),
    }
}
/// A small inline badge indicating an update is available.
/// Uses both color and text so it is not color-alone.
#[allow(dead_code)]
pub fn update_available_label(tag: &str) -> RichText {
    RichText::new(format!("• {} - Update available", tag))
        .strong()
        .color(ACCENT_HOVER)
}

/// Label for the "up to date" version status.
#[allow(dead_code)]
pub fn up_to_date_label(tag: &str) -> RichText {
    RichText::new(format!("{} - Up to date", tag)).color(SUCCESS)
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
    RichText::new(text.into().to_uppercase())
        .strong()
        .size(11.0)
        .color(TEXT_MUTED)
}

pub fn field_label(text: impl Into<String>) -> RichText {
    RichText::new(text.into())
        .strong()
        .size(12.0)
        .color(TEXT_SECONDARY)
}

/// A labeled field row helper: renders `label` above `content` function with
/// consistent spacing. Returns the result of `content`.
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
