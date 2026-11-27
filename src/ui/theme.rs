//! Dynamic visual design system inspired by `ytm-rs`.
//!
//! Supports Dark and Light base themes with full detail-color personalization
//! (accent, card surface tint, border/divider stroke, highlight/badge detail,
//! and corner roundness). Every color getter reads the active [`Palette`] so
//! changing any setting updates the entire UI live on the next frame.

use crate::config::{ThemeMode, ThemeSettings};
use eframe::egui::{
    self, Color32, CornerRadius, Margin, Rect, RichText, Sense, Stroke, Style, Visuals, pos2, vec2,
};
use std::sync::{OnceLock, RwLock};

// ─── Layout spacing & sizing tokens ────────────────────────────────────────
pub const RADIUS_PILL: u8 = 255;

pub const SPACE_XS: f32 = 4.0;
pub const SPACE_SM: f32 = 8.0;
pub const SPACE_MD: f32 = 12.0;
pub const SPACE_LG: f32 = 16.0;
pub const SPACE_XL: f32 = 24.0;

pub const MENU_WIDTH: f32 = 200.0;
pub const MENU_ITEM_HEIGHT: f32 = 36.0;
pub const MENU_HEIGHT: f32 = 140.0;

/// Curated accent swatches shown in Settings › Appearance.
pub const ACCENT_PRESETS: &[(&str, [u8; 3])] = &[
    ("Crimson", [0xFF, 0x30, 0x48]),
    ("Violet", [0xA8, 0x55, 0xF7]),
    ("Ocean", [0x3B, 0x82, 0xF6]),
    ("Emerald", [0x10, 0xB9, 0x81]),
    ("Amber", [0xF5, 0x9E, 0x0B]),
    ("Cyan", [0x06, 0xB6, 0xD4]),
    ("Rose", [0xF4, 0x3F, 0x5E]),
    ("Indigo", [0x63, 0x66, 0xF1]),
];

// ─── Active Palette ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    pub light: bool,
    pub bg: Color32,
    pub sidebar_bg: Color32,
    pub surface: Color32,
    pub surface_raised: Color32,
    pub surface_hover: Color32,
    pub border: Color32,
    pub divider: Color32,
    pub text_primary: Color32,
    pub text_secondary: Color32,
    pub text_muted: Color32,
    pub accent: Color32,
    pub accent_hover: Color32,
    pub accent_soft: Color32,
    pub on_accent: Color32,
    pub detail: Color32,
    pub detail_soft: Color32,
    pub success: Color32,
    pub success_soft: Color32,
    pub warning: Color32,
    pub warning_soft: Color32,
    pub danger: Color32,
    pub danger_soft: Color32,
    pub neutral: Color32,
    pub neutral_soft: Color32,
    pub radius_sm: u8,
    pub radius_md: u8,
    pub radius_lg: u8,
}

impl Palette {
    pub fn from_settings(settings: &ThemeSettings) -> Self {
        let light = settings.mode == ThemeMode::Light;
        let accent = Color32::from_rgb(
            settings.accent_rgb[0],
            settings.accent_rgb[1],
            settings.accent_rgb[2],
        );
        let accent_hover = if light {
            darken(accent, 0.12)
        } else {
            lighten(accent, 0.16)
        };
        let accent_soft = with_alpha(accent, if light { 30 } else { 42 });
        let on_accent = contrast_on(accent);

        // Base Dark / Light tokens directly modeled after ytm-rs
        let (base_bg, base_sidebar, base_surface, base_raised, base_hover) = if light {
            (
                Color32::from_rgb(0xF6, 0xF4, 0xF8), // ytm-rs BG_LIGHT
                Color32::from_rgb(0xEF, 0xEC, 0xF3), // subtle sidebar contrast
                Color32::from_rgb(0xFC, 0xFB, 0xFD), // ytm-rs surface_fill light
                Color32::from_rgb(0xFF, 0xFF, 0xFF), // raised inputs / popups
                Color32::from_rgb(0xEB, 0xE7, 0xF0), // hover fill
            )
        } else {
            (
                Color32::from_rgb(0x0C, 0x0A, 0x0E), // ytm-rs BG
                Color32::from_rgb(0x13, 0x10, 0x17), // subtle sidebar panel
                Color32::from_rgb(0x1C, 0x18, 0x20), // ytm-rs surface_fill dark
                Color32::from_rgb(0x26, 0x22, 0x2A), // ytm-rs menu_color dark
                Color32::from_rgb(0x30, 0x2B, 0x36), // hover fill
            )
        };

        // Optional custom surface tint (blended smoothly so contrast stays intact)
        let (surface, surface_raised, surface_hover) = match settings.custom_surface_rgb {
            Some([r, g, b]) => {
                let tint = Color32::from_rgb(r, g, b);
                let s = lerp_color(base_surface, tint, 0.45);
                let sr = if light {
                    lighten(s, 0.06)
                } else {
                    lighten(s, 0.08)
                };
                let sh = if light {
                    darken(s, 0.07)
                } else {
                    lighten(s, 0.14)
                };
                (s, sr, sh)
            }
            None => (base_surface, base_raised, base_hover),
        };

        // Optional custom border / divider color
        let (border, divider) = match settings.custom_border_rgb {
            Some([r, g, b]) => {
                let b_col = Color32::from_rgb(r, g, b);
                (with_alpha(b_col, 175), with_alpha(b_col, 95))
            }
            None => {
                if light {
                    (
                        Color32::from_rgba_unmultiplied(22, 20, 26, 26),
                        Color32::from_rgba_unmultiplied(22, 20, 26, 18),
                    )
                } else {
                    (
                        Color32::from_rgba_unmultiplied(255, 255, 255, 26),
                        Color32::from_rgba_unmultiplied(255, 255, 255, 18),
                    )
                }
            }
        };

        let (text_primary, text_secondary, text_muted) = if light {
            (
                Color32::from_rgb(0x16, 0x14, 0x1A), // ytm-rs TEXT_LIGHT
                Color32::from_rgb(0x56, 0x52, 0x5E), // ytm-rs TEXT_DIM_LIGHT
                Color32::from_rgb(0x7C, 0x76, 0x84), // ytm-rs TEXT_FAINT_LIGHT
            )
        } else {
            (
                Color32::from_rgb(0xF2, 0xF0, 0xF5), // ytm-rs TEXT
                Color32::from_rgb(0xB2, 0xAC, 0xBA), // ytm-rs TEXT_DIM
                Color32::from_rgb(0x82, 0x7C, 0x8C), // ytm-rs TEXT_FAINT
            )
        };

        let detail = match settings.custom_detail_rgb {
            Some([r, g, b]) => Color32::from_rgb(r, g, b),
            None => accent_hover,
        };
        let detail_soft = with_alpha(detail, if light { 28 } else { 42 });

        let success = if light {
            Color32::from_rgb(0x15, 0x80, 0x3D)
        } else {
            Color32::from_rgb(0x22, 0xC5, 0x5E)
        };
        let warning = if light {
            Color32::from_rgb(0xB4, 0x53, 0x09)
        } else {
            Color32::from_rgb(0xF5, 0x9E, 0x0B)
        };
        let danger = if light {
            Color32::from_rgb(0xDC, 0x26, 0x26)
        } else {
            Color32::from_rgb(0xEF, 0x44, 0x44)
        };
        let neutral = if light {
            Color32::from_rgb(0xE6, 0xE2, 0xEC)
        } else {
            Color32::from_rgb(0x2A, 0x25, 0x32)
        };

        let radius_lg = settings.corner_radius.clamp(6, 18);
        let radius_md = radius_lg.saturating_sub(3).max(4);
        let radius_sm = radius_lg.saturating_sub(6).max(3);

        Self {
            light,
            bg: base_bg,
            sidebar_bg: base_sidebar,
            surface,
            surface_raised,
            surface_hover,
            border,
            divider,
            text_primary,
            text_secondary,
            text_muted,
            accent,
            accent_hover,
            accent_soft,
            on_accent,
            detail,
            detail_soft,
            success,
            success_soft: with_alpha(success, if light { 26 } else { 38 }),
            warning,
            warning_soft: with_alpha(warning, if light { 28 } else { 40 }),
            danger,
            danger_soft: with_alpha(danger, if light { 26 } else { 40 }),
            neutral,
            neutral_soft: with_alpha(text_secondary, if light { 22 } else { 30 }),
            radius_sm,
            radius_md,
            radius_lg,
        }
    }
}

static PALETTE: OnceLock<RwLock<Palette>> = OnceLock::new();

fn palette_lock() -> &'static RwLock<Palette> {
    PALETTE.get_or_init(|| RwLock::new(Palette::from_settings(&ThemeSettings::default())))
}

pub fn current() -> Palette {
    palette_lock()
        .read()
        .map(|g| *g)
        .unwrap_or_else(|_| Palette::from_settings(&ThemeSettings::default()))
}

// ─── Dynamic color accessors ───────────────────────────────────────────────

pub fn is_light() -> bool {
    current().light
}
pub fn bg() -> Color32 {
    current().bg
}
pub fn sidebar_bg() -> Color32 {
    current().sidebar_bg
}
pub fn surface() -> Color32 {
    current().surface
}
pub fn surface_raised() -> Color32 {
    current().surface_raised
}
pub fn surface_hover() -> Color32 {
    current().surface_hover
}
pub fn border() -> Color32 {
    current().border
}
pub fn divider_color() -> Color32 {
    current().divider
}
pub fn text_primary() -> Color32 {
    current().text_primary
}
pub fn text_secondary() -> Color32 {
    current().text_secondary
}
pub fn text_muted() -> Color32 {
    current().text_muted
}
pub fn accent() -> Color32 {
    current().accent
}
pub fn accent_hover() -> Color32 {
    current().accent_hover
}
pub fn accent_soft() -> Color32 {
    current().accent_soft
}
pub fn on_accent() -> Color32 {
    current().on_accent
}
pub fn detail() -> Color32 {
    current().detail
}
pub fn detail_soft() -> Color32 {
    current().detail_soft
}
pub fn success() -> Color32 {
    current().success
}
pub fn warning() -> Color32 {
    current().warning
}
pub fn danger() -> Color32 {
    current().danger
}
pub fn neutral() -> Color32 {
    current().neutral
}
pub fn radius_sm() -> u8 {
    current().radius_sm
}
pub fn radius_md() -> u8 {
    current().radius_md
}
pub fn radius_lg() -> u8 {
    current().radius_lg
}

// ─── Applying theme to egui::Context ───────────────────────────────────────

pub fn apply_theme(ctx: &egui::Context, settings: &ThemeSettings) {
    let p = Palette::from_settings(settings);
    if let Ok(mut guard) = palette_lock().write() {
        *guard = p;
    }

    let mut visuals = if p.light {
        Visuals::light()
    } else {
        Visuals::dark()
    };
    visuals.override_text_color = Some(p.text_primary);
    visuals.interact_cursor = Some(egui::CursorIcon::PointingHand);
    visuals.panel_fill = p.bg;
    visuals.window_fill = p.surface_raised;
    visuals.window_stroke = Stroke::new(1.0_f32, p.border);
    visuals.window_corner_radius = CornerRadius::same(p.radius_lg);
    visuals.menu_corner_radius = CornerRadius::same(p.radius_lg);

    let shadow_color = if p.light {
        Color32::from_black_alpha(28)
    } else {
        Color32::from_black_alpha(120)
    };
    visuals.window_shadow = egui::epaint::Shadow {
        offset: [0, 8],
        blur: 24,
        spread: 0,
        color: shadow_color,
    };
    visuals.popup_shadow = egui::epaint::Shadow {
        offset: [0, 6],
        blur: 18,
        spread: 0,
        color: shadow_color,
    };
    visuals.extreme_bg_color = p.surface_raised;
    visuals.faint_bg_color = p.surface;

    // Non-interactive
    visuals.widgets.noninteractive.bg_fill = p.surface;
    visuals.widgets.noninteractive.weak_bg_fill = p.surface;
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, p.text_secondary);
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, p.border);

    // Idle
    visuals.widgets.inactive.bg_fill = p.surface_raised;
    visuals.widgets.inactive.weak_bg_fill = p.surface_raised;
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, p.text_primary);
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, p.border);

    // Hover
    visuals.widgets.hovered.bg_fill = p.surface_hover;
    visuals.widgets.hovered.weak_bg_fill = p.surface_hover;
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, p.text_primary);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, with_alpha(p.accent, 180));
    visuals.widgets.hovered.expansion = 0.0;

    // Active / pressed
    visuals.widgets.active.bg_fill = p.accent;
    visuals.widgets.active.weak_bg_fill = p.accent;
    visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, p.on_accent);
    visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, p.accent_hover);

    visuals.widgets.open.bg_fill = p.surface_raised;
    visuals.widgets.open.weak_bg_fill = p.surface_raised;
    visuals.widgets.open.fg_stroke = Stroke::new(1.0_f32, p.text_primary);
    visuals.widgets.open.bg_stroke = Stroke::new(1.0_f32, with_alpha(p.accent, 160));

    visuals.selection.bg_fill = with_alpha(p.accent, if p.light { 60 } else { 100 });
    visuals.selection.stroke = Stroke::new(1.0_f32, p.accent);
    visuals.hyperlink_color = p.accent_hover;

    let rounding = CornerRadius::same(p.radius_md);
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
    style.spacing.item_spacing = vec2(SPACE_SM, SPACE_SM);
    style.spacing.button_padding = vec2(14.0, 7.0);
    style.spacing.interact_size.y = 32.0;
    style.spacing.icon_width = 20.0;
    style.spacing.icon_width_inner = 12.0;
    style.spacing.icon_spacing = 8.0;
    style.spacing.window_margin = Margin::same(SPACE_MD as i8);
    style.spacing.menu_margin = Margin::same(SPACE_SM as i8);
    style.spacing.scroll = egui::style::ScrollStyle::solid();
    ctx.set_style(style);
}

pub fn apply(ctx: &egui::Context) {
    apply_theme(ctx, &ThemeSettings::default());
}

// ─── Frames & Containers (ytm-rs surface style) ────────────────────────────

/// Elevated card surface for AppImage rows.
pub fn card_frame() -> egui::Frame {
    let p = current();
    egui::Frame::new()
        .fill(p.surface)
        .stroke(Stroke::new(1.0_f32, p.border))
        .corner_radius(CornerRadius::same(p.radius_lg))
        .inner_margin(Margin::same(SPACE_LG as i8))
}

/// Side panel / sheet container.
pub fn panel_frame() -> egui::Frame {
    let p = current();
    egui::Frame::new()
        .fill(p.surface)
        .stroke(Stroke::new(1.0_f32, p.border))
        .corner_radius(CornerRadius::same(p.radius_lg))
        .inner_margin(Margin::same(SPACE_LG as i8))
}

/// Grouped settings card (ytm-rs style: elevated rounded container with crisp border).
pub fn settings_card_frame() -> egui::Frame {
    let p = current();
    egui::Frame::new()
        .fill(p.surface)
        .stroke(Stroke::new(1.0_f32, p.border))
        .corner_radius(CornerRadius::same(p.radius_lg))
        .inner_margin(Margin::symmetric(SPACE_LG as i8, SPACE_MD as i8))
}

/// Popup / overflow menu frame.
pub fn menu_frame() -> egui::Frame {
    let p = current();
    let shadow_alpha = if p.light { 28 } else { 110 };
    egui::Frame::new()
        .fill(p.surface_raised)
        .stroke(Stroke::new(1.0_f32, p.border))
        .corner_radius(CornerRadius::same(p.radius_lg))
        .inner_margin(Margin::same(SPACE_SM as i8))
        .shadow(egui::epaint::Shadow {
            offset: [0, 6],
            blur: 18,
            spread: 0,
            color: Color32::from_black_alpha(shadow_alpha),
        })
}

/// Modal dialog frame.
pub fn modal_frame() -> egui::Frame {
    let p = current();
    let shadow_alpha = if p.light { 36 } else { 140 };
    egui::Frame::new()
        .fill(p.surface_raised)
        .stroke(Stroke::new(1.0_f32, p.border))
        .corner_radius(CornerRadius::same(p.radius_lg.saturating_add(2)))
        .inner_margin(Margin::same(SPACE_XL as i8))
        .shadow(egui::epaint::Shadow {
            offset: [0, 10],
            blur: 28,
            spread: 0,
            color: Color32::from_black_alpha(shadow_alpha),
        })
}

// ─── Buttons & Controls ────────────────────────────────────────────────────

fn pill(button: egui::Button<'static>, fill: Color32, min_width: f32) -> egui::Button<'static> {
    let p = current();
    button
        .fill(fill)
        .stroke(Stroke::new(1.0_f32, p.border))
        .corner_radius(CornerRadius::same(RADIUS_PILL))
        .min_size(vec2(min_width, 32.0))
}

pub fn primary_button(text: impl Into<String>) -> egui::Button<'static> {
    let p = current();
    pill(
        egui::Button::new(RichText::new(text.into()).color(p.on_accent).strong()),
        p.accent,
        84.0,
    )
    .stroke(Stroke::NONE)
}

pub fn secondary_button(text: impl Into<String>) -> egui::Button<'static> {
    let p = current();
    pill(
        egui::Button::new(RichText::new(text.into()).color(p.text_primary)),
        p.neutral,
        84.0,
    )
}

pub fn danger_button(text: impl Into<String>) -> egui::Button<'static> {
    let p = current();
    pill(
        egui::Button::new(RichText::new(text.into()).color(Color32::WHITE).strong()),
        p.danger,
        84.0,
    )
    .stroke(Stroke::NONE)
}

pub fn ghost_button(text: impl Into<String>) -> egui::Button<'static> {
    let p = current();
    pill(
        egui::Button::new(RichText::new(text.into()).color(p.text_secondary)),
        p.surface_raised,
        64.0,
    )
}

pub fn add_button(text: impl Into<String>) -> egui::Button<'static> {
    let p = current();
    pill(
        egui::Button::new(RichText::new(text.into()).color(p.on_accent).strong()),
        p.accent,
        96.0,
    )
    .stroke(Stroke::NONE)
}

/// Sidebar / tab navigation item styled after ytm-rs's pill nav rows.
pub fn nav_item_button(
    ui: &mut egui::Ui,
    label: &str,
    badge: Option<usize>,
    active: bool,
) -> egui::Response {
    let p = current();
    let width = ui.available_width().max(40.0);
    let height = 38.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);

    let bg = if active {
        p.accent_soft
    } else if resp.hovered() {
        p.surface_hover
    } else {
        Color32::TRANSPARENT
    };
    let fg = if active {
        p.accent
    } else if resp.hovered() {
        p.text_primary
    } else {
        p.text_secondary
    };

    let r = CornerRadius::same(p.radius_md.max(8));
    if bg != Color32::TRANSPARENT {
        ui.painter().rect_filled(rect, r, bg);
    }
    if active {
        // Subtle left accent indicator bar (ytm-rs style)
        let bar = Rect::from_min_size(
            pos2(rect.left() + 3.0, rect.center().y - 9.0),
            vec2(3.0, 18.0),
        );
        ui.painter()
            .rect_filled(bar, CornerRadius::same(2), p.accent);
    }

    ui.painter().text(
        pos2(rect.left() + 14.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(13.5),
        fg,
    );

    if let Some(count) = badge.filter(|&n| n > 0) {
        let count_str = count.to_string();
        let pill_w = (count_str.len() as f32 * 7.5 + 14.0).max(24.0);
        let pill_rect = Rect::from_center_size(
            pos2(rect.right() - 12.0 - pill_w / 2.0, rect.center().y),
            vec2(pill_w, 20.0),
        );
        ui.painter()
            .rect_filled(pill_rect, CornerRadius::same(RADIUS_PILL), p.accent);
        ui.painter().text(
            pill_rect.center(),
            egui::Align2::CENTER_CENTER,
            count_str,
            egui::FontId::proportional(11.0),
            p.on_accent,
        );
    }

    resp
}

/// Smooth animated ytm-rs-style toggle switch tinted with the active accent color.
pub fn toggle(ui: &mut egui::Ui, on: &mut bool) -> egui::Response {
    let p = current();
    let desired = vec2(44.0, 24.0);
    let (rect, mut resp) = ui.allocate_exact_size(desired, Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    let t = ui.ctx().animate_bool_responsive(resp.id, *on);

    let off_track = if p.light {
        Color32::from_rgb(0xD2, 0xCE, 0xDA)
    } else {
        Color32::from_rgb(0x3A, 0x35, 0x44)
    };
    let track_fill = lerp_color(off_track, p.accent, t);
    let radius = rect.height() / 2.0;

    ui.painter().rect_filled(rect, radius, track_fill);

    let knob_r = 9.0;
    let knob_x = egui::lerp((rect.left() + radius)..=(rect.right() - radius), t);
    let knob_center = pos2(knob_x, rect.center().y);
    let knob_color = if *on { p.on_accent } else { Color32::WHITE };
    ui.painter()
        .circle_filled(knob_center + vec2(0.0, 1.0), knob_r, Color32::from_black_alpha(40));
    ui.painter().circle_filled(knob_center, knob_r, knob_color);

    resp
}

/// Subtle horizontal divider inside settings cards.
pub fn divider(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, divider_color());
}

/// Grouped settings card wrapper.
pub fn settings_card(ui: &mut egui::Ui, body: impl FnOnce(&mut egui::Ui)) {
    settings_card_frame().show(ui, |ui| {
        ui.set_width(ui.available_width());
        body(ui);
    });
}

/// Responsive ytm-rs-style setting row: title + description on the left, control on the right.
/// Automatically stacks vertically when available width is narrow (< 460 px).
pub fn setting_row(
    ui: &mut egui::Ui,
    title: &str,
    desc: Option<&str>,
    control: impl FnOnce(&mut egui::Ui),
) {
    let avail = ui.available_width();
    let stack = avail < 460.0;

    ui.add_space(6.0);
    if stack {
        ui.vertical(|ui| {
            ui.label(
                RichText::new(title)
                    .size(14.0)
                    .strong()
                    .color(text_primary()),
            );
            if let Some(d) = desc {
                ui.add(
                    egui::Label::new(RichText::new(d).size(12.0).color(text_secondary())).wrap(),
                );
            }
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                control(ui);
            });
        });
    } else {
        ui.horizontal(|ui| {
            let text_w = (avail * 0.56).clamp(180.0, 440.0);
            ui.vertical(|ui| {
                ui.set_width(text_w);
                ui.label(
                    RichText::new(title)
                        .size(14.0)
                        .strong()
                        .color(text_primary()),
                );
                if let Some(d) = desc {
                    ui.add(
                        egui::Label::new(RichText::new(d).size(12.0).color(text_secondary()))
                            .wrap(),
                    );
                }
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                control(ui);
            });
        });
    }
    ui.add_space(6.0);
}

pub fn menu_secondary_button(text: impl Into<String>) -> egui::Button<'static> {
    let p = current();
    egui::Button::new(RichText::new(text.into()).size(13.5).color(p.text_primary))
        .fill(p.surface_hover)
        .stroke(Stroke::NONE)
        .corner_radius(CornerRadius::same(p.radius_md))
}

pub fn menu_danger_button(text: impl Into<String>) -> egui::Button<'static> {
    let p = current();
    egui::Button::new(
        RichText::new(text.into())
            .size(13.5)
            .strong()
            .color(Color32::WHITE),
    )
    .fill(p.danger)
    .stroke(Stroke::NONE)
    .corner_radius(CornerRadius::same(p.radius_md))
}

pub fn menu_popup_pos(button: Rect, card: Rect, menu_height: f32) -> egui::Pos2 {
    let width = MENU_WIDTH + 2.0 * SPACE_SM;
    let x = (button.right() - width).max(card.left() + SPACE_XS);

    let below = button.bottom() + SPACE_SM;
    let y = if below + menu_height + SPACE_XS <= card.bottom() {
        below
    } else {
        (button.top() - menu_height - SPACE_XS).max(card.top() + SPACE_XS)
    };

    pos2(x, y)
}

pub fn menu_toggle_button(open: bool) -> egui::Button<'static> {
    let p = current();
    let (fill, color, stroke) = if open {
        (
            p.accent_soft,
            p.accent,
            Stroke::new(1.0_f32, with_alpha(p.accent, 180)),
        )
    } else {
        (p.surface_raised, p.text_secondary, Stroke::new(1.0_f32, p.border))
    };
    egui::Button::new(RichText::new("•••").size(15.0).color(color))
        .fill(fill)
        .stroke(stroke)
        .corner_radius(CornerRadius::same(p.radius_md))
        .min_size(vec2(34.0, 28.0))
}

// ─── Color Utilities ───────────────────────────────────────────────────────

pub fn with_alpha(color: Color32, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha)
}

pub fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let mix = |x: u8, y: u8| ((x as f32) + ((y as f32) - (x as f32)) * t).round() as u8;
    Color32::from_rgba_unmultiplied(
        mix(a.r(), b.r()),
        mix(a.g(), b.g()),
        mix(a.b(), b.b()),
        mix(a.a(), b.a()),
    )
}

pub fn lighten(c: Color32, amount: f32) -> Color32 {
    lerp_color(c, Color32::WHITE, amount)
}

pub fn darken(c: Color32, amount: f32) -> Color32 {
    lerp_color(c, Color32::BLACK, amount)
}

/// High-contrast foreground (white or near-black) for arbitrary background colors.
pub fn contrast_on(bg: Color32) -> Color32 {
    let luma = 0.299 * bg.r() as f32 + 0.587 * bg.g() as f32 + 0.114 * bg.b() as f32;
    if luma > 155.0 {
        Color32::from_rgb(18, 16, 22)
    } else {
        Color32::WHITE
    }
}

/// Subtle focus/hover highlight ring (refined ytm-rs style, gentler than heavy neon).
pub fn draw_glow(ui: &egui::Ui, rect: Rect, corner_radius: u8, color: Color32) {
    if !rect.is_finite() || rect.width() <= 1.0 || rect.height() <= 1.0 {
        return;
    }
    let painter = ui.painter();
    for (expand, alpha, width) in [(2.5_f32, 24_u8, 2.5_f32), (1.0_f32, 70_u8, 1.5_f32)] {
        let radius = CornerRadius::same(corner_radius.saturating_add(expand as u8));
        painter.rect_stroke(
            rect.expand(expand),
            radius,
            Stroke::new(width, with_alpha(color, alpha)),
            egui::StrokeKind::Outside,
        );
    }
}

// ─── Badges & Typography ───────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BadgeKind {
    Success,
    Accent,
    Warning,
    Danger,
    Neutral,
}

pub fn badge_sized(
    ui: &mut egui::Ui,
    text: impl Into<String>,
    kind: BadgeKind,
    size: egui::Vec2,
) -> egui::Response {
    let (fg, bg) = badge_colors(kind);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    if rect.is_finite() && rect.width() > 0.0 && rect.height() > 0.0 {
        let painter = ui.painter();
        painter.rect_filled(rect, CornerRadius::same(RADIUS_PILL), bg);
        painter.rect_stroke(
            rect,
            CornerRadius::same(RADIUS_PILL),
            Stroke::new(1.0_f32, with_alpha(fg, 85)),
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
    let p = current();
    match kind {
        BadgeKind::Success => (p.success, p.success_soft),
        BadgeKind::Accent => (p.detail, p.detail_soft),
        BadgeKind::Warning => (p.warning, p.warning_soft),
        BadgeKind::Danger => (p.danger, p.danger_soft),
        BadgeKind::Neutral => (p.text_secondary, p.neutral_soft),
    }
}

pub fn status_dot(ui: &mut egui::Ui, color: Color32) {
    let avail = ui.available_size();
    if !avail.x.is_finite() || !avail.y.is_finite() || avail.x < 8.0 || avail.y < 8.0 {
        return;
    }
    let (rect, _) = ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
    if rect.is_finite() && rect.width() > 0.0 && rect.height() > 0.0 {
        ui.painter().circle_filled(rect.center(), 4.0, color);
    }
}

pub fn muted_label(text: impl Into<String>) -> RichText {
    RichText::new(text.into()).color(text_muted())
}

pub fn secondary_label(text: impl Into<String>) -> RichText {
    RichText::new(text.into()).color(text_secondary())
}

pub fn section_heading(text: impl Into<String>) -> RichText {
    RichText::new(text.into().to_uppercase())
        .strong()
        .size(11.0)
        .color(text_muted())
}

pub fn field_label(text: impl Into<String>) -> RichText {
    RichText::new(text.into())
        .strong()
        .size(12.5)
        .color(text_secondary())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_and_light_palettes_have_distinct_backgrounds() {
        let dark = Palette::from_settings(&ThemeSettings {
            mode: ThemeMode::Dark,
            ..Default::default()
        });
        let light = Palette::from_settings(&ThemeSettings {
            mode: ThemeMode::Light,
            ..Default::default()
        });
        assert!(!dark.light);
        assert!(light.light);
        assert_ne!(dark.bg, light.bg);
        assert_ne!(dark.text_primary, light.text_primary);
    }

    #[test]
    fn custom_detail_colors_override_defaults() {
        let settings = ThemeSettings {
            mode: ThemeMode::Dark,
            accent_rgb: [0x10, 0xB9, 0x81],
            custom_surface_rgb: Some([0x20, 0x30, 0x40]),
            custom_border_rgb: Some([0xFF, 0xAA, 0x00]),
            custom_detail_rgb: Some([0x38, 0xBD, 0xF8]),
            corner_radius: 16,
        };
        let p = Palette::from_settings(&settings);
        assert_eq!(p.accent, Color32::from_rgb(0x10, 0xB9, 0x81));
        assert_eq!(p.detail, Color32::from_rgb(0x38, 0xBD, 0xF8));
        assert_eq!(p.radius_lg, 16);
    }

    #[test]
    fn contrast_picks_dark_text_on_bright_accent() {
        assert_eq!(contrast_on(Color32::from_rgb(255, 240, 120)), Color32::from_rgb(18, 16, 22));
        assert_eq!(contrast_on(Color32::from_rgb(40, 20, 80)), Color32::WHITE);
    }
}
