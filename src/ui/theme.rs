//! Modern spatial visual design system ("Obsidian / Alabaster Studio").
//!
//! Inspired by Linear, Raycast, Setapp, and macOS Sequoia:
//! - Tonal depth & hairline borders instead of heavy box outlines
//! - Real Regular / Medium / Bold grotesque typography + Monospace tabular figures
//! - Smooth micro-interaction hover transitions (`animate_bool_responsive`)
//! - Dark & Light base themes with full custom detail-color personalization

use crate::config::{ThemeMode, ThemeSettings};
use crate::ui::fonts;
use crate::ui::icons::{self, Icon};
use eframe::egui::{
    self, Align2, Color32, CornerRadius, Margin, Rect, RichText, Sense, Stroke, StrokeKind, Style,
    Visuals, pos2, vec2,
};
use std::cell::Cell;

// ─── Layout spacing & sizing tokens ────────────────────────────────────────
pub const RADIUS_PILL: u8 = 255;

pub const SPACE_XS: f32 = 4.0;
pub const SPACE_SM: f32 = 8.0;
pub const SPACE_MD: f32 = 12.0;
pub const SPACE_LG: f32 = 16.0;
pub const SPACE_XL: f32 = 24.0;

pub const MENU_WIDTH: f32 = 204.0;
pub const MENU_ITEM_HEIGHT: f32 = 34.0;
pub const MENU_HEIGHT: f32 = 136.0;

/// Curated accent swatches for quick personalization.
pub const ACCENT_PRESETS: &[(&str, [u8; 3])] = &[
    ("Linear Indigo", [0x63, 0x66, 0xF1]),
    ("Raycast Crimson", [0xFF, 0x3B, 0x5C]),
    ("Emerald Mint", [0x10, 0xB9, 0x81]),
    ("Sky Blue", [0x38, 0xBD, 0xF8]),
    ("Violet Pulse", [0xA8, 0x55, 0xF7]),
    ("Amber Gold", [0xF5, 0x9E, 0x0B]),
    ("Coral Sunset", [0xFB, 0x71, 0x85]),
    ("Teal Frost", [0x14, 0xB8, 0xA6]),
];

/// Curated complete looks (Base mode + custom accent/detail harmonies).
pub struct ThemePreset {
    pub name: &'static str,
    pub subtitle: &'static str,
    pub settings: ThemeSettings,
}

pub fn curated_presets() -> Vec<ThemePreset> {
    vec![
        ThemePreset {
            name: "Obsidian Indigo",
            subtitle: "Linear-inspired dark",
            settings: ThemeSettings {
                mode: ThemeMode::Dark,
                accent_rgb: [0x63, 0x66, 0xF1],
                custom_surface_rgb: None,
                custom_border_rgb: None,
                custom_detail_rgb: Some([0x81, 0x8C, 0xF8]),
                corner_radius: 14,
            },
        },
        ThemePreset {
            name: "Crimson Studio",
            subtitle: "Raycast-style dark",
            settings: ThemeSettings {
                mode: ThemeMode::Dark,
                accent_rgb: [0xFF, 0x3B, 0x5C],
                custom_surface_rgb: Some([0x1A, 0x15, 0x1C]),
                custom_border_rgb: None,
                custom_detail_rgb: Some([0xFF, 0x6B, 0x81]),
                corner_radius: 14,
            },
        },
        ThemePreset {
            name: "Midnight Emerald",
            subtitle: "Deep slate + mint",
            settings: ThemeSettings {
                mode: ThemeMode::Dark,
                accent_rgb: [0x10, 0xB9, 0x81],
                custom_surface_rgb: Some([0x11, 0x19, 0x1B]),
                custom_border_rgb: None,
                custom_detail_rgb: Some([0x34, 0xD3, 0x99]),
                corner_radius: 14,
            },
        },
        ThemePreset {
            name: "Alabaster Light",
            subtitle: "Crisp studio light",
            settings: ThemeSettings {
                mode: ThemeMode::Light,
                accent_rgb: [0x4F, 0x46, 0xE5],
                custom_surface_rgb: None,
                custom_border_rgb: None,
                custom_detail_rgb: Some([0x63, 0x66, 0xF1]),
                corner_radius: 14,
            },
        },
        ThemePreset {
            name: "Nordic Daylight",
            subtitle: "Cool mist + ocean",
            settings: ThemeSettings {
                mode: ThemeMode::Light,
                accent_rgb: [0x02, 0x84, 0xC7],
                custom_surface_rgb: Some([0xF0, 0xF7, 0xFF]),
                custom_border_rgb: Some([0xBA, 0xD4, 0xEA]),
                custom_detail_rgb: Some([0x0E, 0xA5, 0xE9]),
                corner_radius: 14,
            },
        },
        ThemePreset {
            name: "Warm Porcelain",
            subtitle: "Soft sand + coral",
            settings: ThemeSettings {
                mode: ThemeMode::Light,
                accent_rgb: [0xE1, 0x1D, 0x48],
                custom_surface_rgb: Some([0xFF, 0xF8, 0xF6]),
                custom_border_rgb: None,
                custom_detail_rgb: Some([0xF4, 0x3F, 0x5E]),
                corner_radius: 16,
            },
        },
    ]
}

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
    pub border_hover: Color32,
    pub divider: Color32,
    pub specular: Color32,
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
            darken(accent, 0.10)
        } else {
            lighten(accent, 0.14)
        };
        let accent_soft = with_alpha(accent, if light { 26 } else { 36 });
        let on_accent = contrast_on(accent);

        let (base_bg, base_sidebar, base_surface, base_raised, base_hover) = if light {
            (
                Color32::from_rgb(0xF5, 0xF5, 0xF8), // Soft alabaster canvas
                Color32::from_rgb(0xED, 0xED, 0xF2), // Gentle sidebar
                Color32::from_rgb(0xFF, 0xFF, 0xFF), // Clean elevated cards
                Color32::from_rgb(0xF1, 0xF1, 0xF6), // Soft inputs & pills
                Color32::from_rgb(0xE8, 0xE8, 0xF0), // Gentle hover
            )
        } else {
            (
                Color32::from_rgb(0x11, 0x11, 0x16), // Soft deep slate canvas
                Color32::from_rgb(0x15, 0x15, 0x1C), // Soft inset sidebar
                Color32::from_rgb(0x1B, 0x1B, 0x24), // Soft elevated Bento card
                Color32::from_rgb(0x23, 0x23, 0x2E), // Soft interactive pill/input
                Color32::from_rgb(0x2B, 0x2B, 0x38), // Gentle hover surface
            )
        };

        let (surface, surface_raised, surface_hover) = match settings.custom_surface_rgb {
            Some([r, g, b]) => {
                let tint = Color32::from_rgb(r, g, b);
                let s = lerp_color(base_surface, tint, 0.42);
                let sr = if light {
                    darken(s, 0.04)
                } else {
                    lighten(s, 0.07)
                };
                let sh = if light {
                    darken(s, 0.08)
                } else {
                    lighten(s, 0.13)
                };
                (s, sr, sh)
            }
            None => (base_surface, base_raised, base_hover),
        };

        // Always pre-blend borders, dividers, and specular highlights into 100% opaque
        // RGB colors (alpha = 255). Translucent strokes on Linux/Wayland compositors can
        // punch holes in the framebuffer alpha channel and show windows behind the app.
        let (border, border_hover, divider) = match settings.custom_border_rgb {
            Some([r, g, b]) => {
                let b_col = Color32::from_rgb(r, g, b);
                (
                    lerp_color(surface, b_col, 0.58),
                    lerp_color(surface, b_col, 0.88),
                    lerp_color(base_bg, b_col, 0.35),
                )
            }
            None => {
                if light {
                    (
                        lerp_color(surface, Color32::from_rgb(15, 15, 26), 0.08),
                        lerp_color(surface, Color32::from_rgb(15, 15, 26), 0.16),
                        lerp_color(base_bg, Color32::from_rgb(15, 15, 26), 0.07),
                    )
                } else {
                    (
                        lerp_color(surface, Color32::WHITE, 0.065),
                        lerp_color(surface, Color32::WHITE, 0.14),
                        lerp_color(base_bg, Color32::WHITE, 0.055),
                    )
                }
            }
        };

        let specular = if light {
            lerp_color(surface, Color32::WHITE, 0.75)
        } else {
            lerp_color(surface, Color32::WHITE, 0.05)
        };

        let (text_primary, text_secondary, text_muted) = if light {
            (
                Color32::from_rgb(0x18, 0x18, 0x22),
                Color32::from_rgb(0x56, 0x56, 0x68),
                Color32::from_rgb(0x84, 0x84, 0x96),
            )
        } else {
            (
                Color32::from_rgb(0xEE, 0xEE, 0xF5),
                Color32::from_rgb(0xA2, 0xA2, 0xB6),
                Color32::from_rgb(0x6E, 0x6E, 0x84),
            )
        };

        let detail = match settings.custom_detail_rgb {
            Some([r, g, b]) => Color32::from_rgb(r, g, b),
            None => accent_hover,
        };
        let detail_soft = with_alpha(detail, if light { 26 } else { 36 });

        let success = if light {
            Color32::from_rgb(0x15, 0x80, 0x3D)
        } else {
            Color32::from_rgb(0x34, 0xD3, 0x99)
        };
        let warning = if light {
            Color32::from_rgb(0xB4, 0x53, 0x09)
        } else {
            Color32::from_rgb(0xFB, 0xBF, 0x24)
        };
        let danger = if light {
            Color32::from_rgb(0xDC, 0x26, 0x26)
        } else {
            Color32::from_rgb(0xF8, 0x71, 0x71)
        };
        let neutral = if light {
            Color32::from_rgb(0xE8, 0xE8, 0xF0)
        } else {
            Color32::from_rgb(0x21, 0x21, 0x2D)
        };

        let radius_lg = settings.corner_radius.clamp(6, 18);
        let radius_md = radius_lg.saturating_sub(4).max(4);
        let radius_sm = radius_lg.saturating_sub(7).max(3);

        Self {
            light,
            bg: base_bg,
            sidebar_bg: base_sidebar,
            surface,
            surface_raised,
            surface_hover,
            border,
            border_hover,
            divider,
            specular,
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
            success_soft: with_alpha(success, if light { 24 } else { 32 }),
            warning,
            warning_soft: with_alpha(warning, if light { 26 } else { 36 }),
            danger,
            danger_soft: with_alpha(danger, if light { 24 } else { 36 }),
            neutral,
            neutral_soft: with_alpha(text_secondary, if light { 20 } else { 26 }),
            radius_sm,
            radius_md,
            radius_lg,
        }
    }
}

thread_local! {
    static PALETTE: Cell<Palette> = Cell::new(Palette::from_settings(&ThemeSettings::default()));
}

#[inline]
pub fn current() -> Palette {
    PALETTE.with(Cell::get)
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
pub fn border_hover() -> Color32 {
    current().border_hover
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
    PALETTE.with(|cell| cell.set(p));

    let mut visuals = if p.light {
        Visuals::light()
    } else {
        Visuals::dark()
    };
    visuals.override_text_color = Some(p.text_primary);
    visuals.interact_cursor = Some(egui::CursorIcon::PointingHand);
    visuals.panel_fill = p.bg;
    visuals.window_fill = p.surface;
    visuals.window_stroke = Stroke::new(1.0_f32, p.border);
    visuals.window_corner_radius = CornerRadius::same(p.radius_lg);
    visuals.menu_corner_radius = CornerRadius::same(p.radius_lg);

    let shadow_color = if p.light {
        Color32::from_black_alpha(22)
    } else {
        Color32::from_black_alpha(140)
    };
    visuals.window_shadow = egui::epaint::Shadow {
        offset: [0, 10],
        blur: 32,
        spread: 0,
        color: shadow_color,
    };
    visuals.popup_shadow = egui::epaint::Shadow {
        offset: [0, 8],
        blur: 24,
        spread: 0,
        color: shadow_color,
    };
    visuals.extreme_bg_color = p.surface_raised;
    visuals.faint_bg_color = p.surface;

    visuals.widgets.noninteractive.bg_fill = p.surface;
    visuals.widgets.noninteractive.weak_bg_fill = p.surface;
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, p.text_secondary);
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, p.border);

    visuals.widgets.inactive.bg_fill = p.surface_raised;
    visuals.widgets.inactive.weak_bg_fill = p.surface_raised;
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, p.text_primary);
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, p.border);

    visuals.widgets.hovered.bg_fill = p.surface_hover;
    visuals.widgets.hovered.weak_bg_fill = p.surface_hover;
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, p.text_primary);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, p.border_hover);
    visuals.widgets.hovered.expansion = 0.0;

    visuals.widgets.active.bg_fill = p.accent;
    visuals.widgets.active.weak_bg_fill = p.accent;
    visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, p.on_accent);
    visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, p.accent_hover);

    visuals.widgets.open.bg_fill = p.surface_raised;
    visuals.widgets.open.weak_bg_fill = p.surface_raised;
    visuals.widgets.open.fg_stroke = Stroke::new(1.0_f32, p.text_primary);
    visuals.widgets.open.bg_stroke = Stroke::new(1.0_f32, with_alpha(p.accent, 150));

    visuals.selection.bg_fill = with_alpha(p.accent, if p.light { 50 } else { 85 });
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
    style.spacing.icon_width = 18.0;
    style.spacing.icon_width_inner = 10.0;
    style.spacing.icon_spacing = 8.0;
    style.spacing.window_margin = Margin::same(SPACE_MD as i8);
    style.spacing.menu_margin = Margin::same(SPACE_SM as i8);
    style.spacing.scroll = egui::style::ScrollStyle::solid();
    ctx.set_style(style);
}

pub fn apply(ctx: &egui::Context) {
    apply_theme(ctx, &ThemeSettings::default());
}

// ─── Ambient Canvas Lighting & Elevated Surfaces ───────────────────────────

/// Paints a hardware-interpolated, butter-smooth whisper-soft ambient tint at the
/// top edge of the canvas (zero banding).
pub fn paint_canvas_ambient(ui: &egui::Ui, rect: Rect) {
    let p = current();
    let band_h = 180.0_f32.min(rect.height());
    if band_h <= 1.0 || rect.width() <= 1.0 {
        return;
    }
    let top_alpha = if p.light { 10 } else { 15 };
    let top_col = with_alpha(p.accent, top_alpha);
    let bottom_col = Color32::TRANSPARENT;

    let mut mesh = egui::Mesh::default();
    let y0 = rect.top();
    let y1 = rect.top() + band_h;
    mesh.colored_vertex(pos2(rect.left(), y0), top_col);
    mesh.colored_vertex(pos2(rect.right(), y0), top_col);
    mesh.colored_vertex(pos2(rect.right(), y1), bottom_col);
    mesh.colored_vertex(pos2(rect.left(), y1), bottom_col);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    ui.painter().add(egui::Shape::mesh(mesh));
}

/// Paints a distinct tonal surface for top-level dashboard Info / Stat cards
/// (Library, Updates, AppImage Hub / Sync Status) so they visually separate from
/// regular AppImage cards in both Dark and Light themes.
pub fn paint_info_card(painter: &egui::Painter, rect: Rect, hover_t: f32, soft_accent: bool) {
    let p = current();
    let r = CornerRadius::same(p.radius_lg);

    // Distinct tonal base: soft cool slate tint in Light mode (vs pure white app cards),
    // and deeper recessed slate in Dark mode (vs elevated app cards).
    let base_info = if p.light {
        lerp_color(p.sidebar_bg, p.surface_raised, 0.45)
    } else {
        lerp_color(p.bg, p.surface, 0.48)
    };
    let tinted = if soft_accent {
        lerp_color(base_info, p.accent, if p.light { 0.075 } else { 0.095 })
    } else {
        base_info
    };
    let hover_target = if p.light {
        darken(tinted, 0.03)
    } else {
        lighten(tinted, 0.04)
    };
    let fill = lerp_color(tinted, hover_target, hover_t * 0.6);
    painter.rect_filled(rect, r, fill);

    let rest_stroke = if soft_accent {
        lerp_color(tinted, p.accent, if p.light { 0.22 } else { 0.26 })
    } else {
        p.border
    };
    let hover_stroke = if soft_accent {
        lerp_color(tinted, p.accent, if p.light { 0.38 } else { 0.42 })
    } else {
        p.border_hover
    };
    painter.rect_stroke(
        rect,
        r,
        Stroke::new(1.0_f32, lerp_color(rest_stroke, hover_stroke, hover_t)),
        StrokeKind::Inside,
    );
}

/// Paints a soft, calm Bento card surface with gentle hover elevation, subtle
/// light-mode shadow, and whisper-thin hairline border (never harsh neon strokes).
pub fn paint_bento_card(painter: &egui::Painter, rect: Rect, hover_t: f32, soft_accent: bool) {
    let p = current();
    let r = CornerRadius::same(p.radius_lg);

    // Whisper-soft ambient shadow on Light mode
    if p.light {
        let shadow_rect = rect.translate(vec2(0.0, 2.0 + hover_t * 1.5));
        painter.rect_filled(
            shadow_rect,
            r,
            Color32::from_black_alpha((8.0 + hover_t * 6.0) as u8),
        );
    }

    let base_fill = if soft_accent {
        lerp_color(p.surface, p.accent, if p.light { 0.025 } else { 0.04 })
    } else {
        p.surface
    };
    let fill = lerp_color(base_fill, p.surface_hover, hover_t * 0.45);
    painter.rect_filled(rect, r, fill);

    // Delicate top specular highlight
    if rect.width() > 36.0 {
        let inset = p.radius_lg as f32;
        painter.line_segment(
            [
                pos2(rect.left() + inset, rect.top() + 0.75),
                pos2(rect.right() - inset, rect.top() + 0.75),
            ],
            Stroke::new(1.0_f32, p.specular),
        );
    }

    let rest_stroke = if soft_accent {
        lerp_color(base_fill, p.accent, if p.light { 0.18 } else { 0.22 })
    } else {
        p.border
    };
    let hover_stroke = if soft_accent {
        lerp_color(base_fill, p.accent, if p.light { 0.34 } else { 0.38 })
    } else {
        p.border_hover
    };
    let stroke_color = lerp_color(rest_stroke, hover_stroke, hover_t);
    painter.rect_stroke(
        rect,
        r,
        Stroke::new(1.0_f32, stroke_color),
        StrokeKind::Inside,
    );
}

/// Card surface frame for standard non-interactive containers.
pub fn card_frame() -> egui::Frame {
    let p = current();
    egui::Frame::new()
        .fill(p.surface)
        .stroke(Stroke::new(1.0_f32, p.border))
        .corner_radius(CornerRadius::same(p.radius_lg))
        .inner_margin(Margin::same(SPACE_LG as i8))
}

/// Floating drawer / sheet container.
pub fn panel_frame() -> egui::Frame {
    let p = current();
    let shadow_alpha = if p.light { 34 } else { 150 };
    egui::Frame::new()
        .fill(p.surface)
        .stroke(Stroke::new(1.0_f32, p.border))
        .corner_radius(CornerRadius::same(p.radius_lg.saturating_add(2)))
        .inner_margin(Margin::same(SPACE_XL as i8))
        .shadow(egui::epaint::Shadow {
            offset: [-6, 8],
            blur: 32,
            spread: 0,
            color: Color32::from_black_alpha(shadow_alpha),
        })
}

/// Grouped settings card container.
pub fn settings_card_frame() -> egui::Frame {
    let p = current();
    let mut frame = egui::Frame::new()
        .fill(p.surface)
        .stroke(Stroke::new(1.0_f32, p.border))
        .corner_radius(CornerRadius::same(p.radius_lg))
        .inner_margin(Margin::symmetric(SPACE_LG as i8, SPACE_MD as i8));
    if p.light {
        frame = frame.shadow(egui::epaint::Shadow {
            offset: [0, 2],
            blur: 10,
            spread: 0,
            color: Color32::from_black_alpha(10),
        });
    }
    frame
}

/// Popup / overflow menu frame.
pub fn menu_frame() -> egui::Frame {
    let p = current();
    let shadow_alpha = if p.light { 32 } else { 130 };
    egui::Frame::new()
        .fill(if p.light { p.surface } else { p.surface_raised })
        .stroke(Stroke::new(1.0_f32, p.border_hover))
        .corner_radius(CornerRadius::same(p.radius_lg))
        .inner_margin(Margin::same(6))
        .shadow(egui::epaint::Shadow {
            offset: [0, 8],
            blur: 24,
            spread: 0,
            color: Color32::from_black_alpha(shadow_alpha),
        })
}

/// Modal dialog / Command Palette frame.
pub fn modal_frame() -> egui::Frame {
    let p = current();
    let shadow_alpha = if p.light { 42 } else { 170 };
    egui::Frame::new()
        .fill(p.surface)
        .stroke(Stroke::new(1.0_f32, p.border_hover))
        .corner_radius(CornerRadius::same(p.radius_lg.saturating_add(2)))
        .inner_margin(Margin::same(SPACE_XL as i8))
        .shadow(egui::epaint::Shadow {
            offset: [0, 14],
            blur: 40,
            spread: 0,
            color: Color32::from_black_alpha(shadow_alpha),
        })
}

// ─── Modern Custom-Painted Controls ────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ButtonTone {
    Primary,
    Secondary,
    Ghost,
    Danger,
    SubtleAccent,
}

/// Modern pill button with optional vector icon, real Medium/Bold typography,
/// and smooth animated hover transitions.
pub fn pill_action(
    ui: &mut egui::Ui,
    icon: Option<Icon>,
    label: &str,
    tone: ButtonTone,
    height: f32,
) -> egui::Response {
    let p = current();
    let font = if matches!(tone, ButtonTone::Primary | ButtonTone::Danger) {
        fonts::bold(13.0)
    } else {
        fonts::medium(13.0)
    };
    // Use PLACEHOLDER so `painter.galley(..., fg)` applies the dynamic theme text color
    // in both Dark and Light modes (never baking white text onto light buttons).
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font, Color32::PLACEHOLDER);
    let icon_w = if icon.is_some() { 15.0 + 6.0 } else { 0.0 };
    let pad_x = if label.is_empty() { 10.0 } else { 14.0 };
    let desired = vec2(galley.size().x + icon_w + pad_x * 2.0, height);

    let (rect, resp) = ui.allocate_exact_size(desired, Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    let hover = ui.ctx().animate_bool_responsive(resp.id, resp.hovered() && ui.is_enabled());
    let enabled_alpha = if ui.is_enabled() { 1.0_f32 } else { 0.42_f32 };

    let (base_fill, hover_fill, fg, border_col) = match tone {
        ButtonTone::Primary => (
            lerp_color(p.accent, p.surface, if p.light { 0.06 } else { 0.10 }),
            p.accent,
            p.on_accent,
            Color32::TRANSPARENT,
        ),
        ButtonTone::Secondary => (p.surface_raised, p.surface_hover, p.text_primary, p.border),
        ButtonTone::Ghost => (
            Color32::TRANSPARENT,
            p.surface_hover,
            p.text_secondary,
            Color32::TRANSPARENT,
        ),
        ButtonTone::Danger => (
            p.danger,
            lighten(p.danger, 0.10),
            Color32::WHITE,
            Color32::TRANSPARENT,
        ),
        ButtonTone::SubtleAccent => (
            with_alpha(p.accent, if p.light { 24 } else { 32 }),
            with_alpha(p.accent, if p.light { 42 } else { 54 }),
            if p.light { p.accent } else { lighten(p.accent, 0.22) },
            with_alpha(p.accent, if p.light { 48 } else { 58 }),
        ),
    };

    let fill = scale_alpha(lerp_color(base_fill, hover_fill, hover), enabled_alpha);
    let fg = scale_alpha(
        if matches!(tone, ButtonTone::Ghost) {
            lerp_color(fg, p.text_primary, hover)
        } else {
            fg
        },
        enabled_alpha,
    );

    let cr = CornerRadius::same(RADIUS_PILL);
    if fill.a() > 0 {
        ui.painter().rect_filled(rect, cr, fill);
    }
    if border_col.a() > 0 {
        let hover_border = if matches!(tone, ButtonTone::Secondary) {
            lerp_color(border_col, p.border_hover, hover)
        } else {
            border_col
        };
        ui.painter().rect_stroke(
            rect,
            cr,
            Stroke::new(1.0_f32, scale_alpha(hover_border, enabled_alpha)),
            StrokeKind::Inside,
        );
    }

    let content_w = galley.size().x + icon_w;
    let mut x = rect.center().x - content_w / 2.0;
    if let Some(ic) = icon {
        icons::paint(ui.painter(), ic, pos2(x + 7.5, rect.center().y), 14.0, fg);
        x += 15.0 + 6.0;
    }
    if !label.is_empty() {
        ui.painter().galley(
            pos2(x, rect.center().y - galley.size().y / 2.0 - 0.5),
            galley,
            fg,
        );
    }

    resp
}

/// Crisp icon-only rounded button (32×32 default) with smooth hover fill.
pub fn icon_button(
    ui: &mut egui::Ui,
    icon: Icon,
    size: f32,
    active: bool,
    tooltip: &str,
) -> egui::Response {
    let p = current();
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    let resp = resp
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(tooltip);
    let hover = ui.ctx().animate_bool_responsive(resp.id, resp.hovered());

    let fill = if active {
        p.accent_soft
    } else {
        lerp_color(Color32::TRANSPARENT, p.surface_hover, hover)
    };
    let fg = if active {
        p.accent
    } else {
        lerp_color(p.text_secondary, p.text_primary, hover)
    };
    let cr = CornerRadius::same(RADIUS_PILL);
    if fill.a() > 0 {
        ui.painter().rect_filled(rect, cr, fill);
    }
    if active {
        ui.painter().rect_stroke(
            rect,
            cr,
            Stroke::new(1.0_f32, with_alpha(p.accent, 65)),
            StrokeKind::Inside,
        );
    }
    icons::paint(ui.painter(), icon, rect.center(), size * 0.48, fg);
    resp
}

/// Modern menu item inside a floating popover (icon + label, full-width hover pill).
pub fn popover_item(
    ui: &mut egui::Ui,
    icon: Icon,
    label: &str,
    danger: bool,
) -> egui::Response {
    let p = current();
    let w = ui.available_width().max(160.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(w, MENU_ITEM_HEIGHT), Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    let hover = ui.ctx().animate_bool_responsive(resp.id, resp.hovered());

    let hover_bg = if danger { p.danger_soft } else { p.surface_hover };
    let fill = lerp_color(Color32::TRANSPARENT, hover_bg, hover);
    let fg = if danger {
        p.danger
    } else {
        lerp_color(p.text_primary, p.text_primary, hover)
    };
    let icon_fg = if danger {
        p.danger
    } else {
        lerp_color(p.text_secondary, p.text_primary, hover)
    };

    if fill.a() > 0 {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(p.radius_md), fill);
    }
    icons::paint(
        ui.painter(),
        icon,
        pos2(rect.left() + 18.0, rect.center().y),
        15.0,
        icon_fg,
    );
    ui.painter().text(
        pos2(rect.left() + 36.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        fonts::medium(13.0),
        fg,
    );
    resp
}

/// Sidebar / settings navigation row with vector icon, animated hover, accent
/// indicator, and optional tabular count badge.
pub fn nav_item(
    ui: &mut egui::Ui,
    icon: Icon,
    label: &str,
    badge: Option<(usize, bool)>,
    active: bool,
) -> egui::Response {
    let p = current();
    let width = ui.available_width().max(40.0);
    let height = 36.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    let hover = ui.ctx().animate_bool_responsive(resp.id, resp.hovered());

    let fill = if active {
        p.accent_soft
    } else {
        lerp_color(Color32::TRANSPARENT, p.surface_hover, hover * 0.75)
    };
    let fg = if active {
        p.accent
    } else {
        lerp_color(p.text_secondary, p.text_primary, hover)
    };

    let cr = CornerRadius::same(p.radius_md.max(8));
    if fill.a() > 0 {
        ui.painter().rect_filled(rect, cr, fill);
    }
    if active {
        let bar = Rect::from_min_size(
            pos2(rect.left() + 3.0, rect.center().y - 8.0),
            vec2(3.0, 16.0),
        );
        ui.painter()
            .rect_filled(bar, CornerRadius::same(2), p.accent);
    }

    icons::paint(
        ui.painter(),
        icon,
        pos2(rect.left() + 19.0, rect.center().y),
        15.5,
        fg,
    );

    let font = if active {
        fonts::bold(13.0)
    } else {
        fonts::medium(13.0)
    };
    ui.painter().text(
        pos2(rect.left() + 36.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        font,
        fg,
    );

    if let Some((count, highlight)) = badge.filter(|&(n, _)| n > 0) {
        let count_str = count.to_string();
        let pill_w = (count_str.len() as f32 * 7.0 + 14.0).max(22.0);
        let pill_rect = Rect::from_center_size(
            pos2(rect.right() - 10.0 - pill_w / 2.0, rect.center().y),
            vec2(pill_w, 19.0),
        );
        let (pill_bg, pill_fg) = if highlight {
            (p.accent, p.on_accent)
        } else {
            (p.neutral_soft, p.text_muted)
        };
        ui.painter()
            .rect_filled(pill_rect, CornerRadius::same(RADIUS_PILL), pill_bg);
        ui.painter().text(
            pill_rect.center(),
            Align2::CENTER_CENTER,
            count_str,
            fonts::mono(10.5),
            pill_fg,
        );
    }

    resp
}

/// Subtle keyboard shortcut cap (e.g. `Ctrl+K`).
pub fn keycap(ui: &mut egui::Ui, text: &str) {
    let p = current();
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), fonts::mono(10.0), p.text_muted);
    let desired = vec2(galley.size().x + 10.0, 20.0);
    let (rect, _) = ui.allocate_exact_size(desired, Sense::hover());
    let cr = CornerRadius::same(5);
    ui.painter().rect_filled(rect, cr, p.neutral_soft);
    ui.painter()
        .rect_stroke(rect, cr, Stroke::new(1.0_f32, p.border), StrokeKind::Inside);
    ui.painter().galley(
        pos2(
            rect.center().x - galley.size().x / 2.0,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        p.text_muted,
    );
}

/// Compact monospace version pill (`v2.4.0`).
pub fn version_pill(ui: &mut egui::Ui, text: &str, fg: Color32, bg: Color32) {
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), fonts::mono(10.5), fg);
    let desired = vec2(galley.size().x + 14.0, 22.0);
    let (rect, _) = ui.allocate_exact_size(desired, Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::same(RADIUS_PILL), bg);
    ui.painter().galley(
        pos2(
            rect.center().x - galley.size().x / 2.0,
            rect.center().y - galley.size().y / 2.0 - 0.5,
        ),
        galley,
        fg,
    );
}

/// Refined status pill with a soft status dot + Medium label.
pub fn status_pill(ui: &mut egui::Ui, text: &str, kind: BadgeKind) -> egui::Response {
    let (fg, bg) = badge_colors(kind);
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), fonts::medium(11.0), fg);
    let desired = vec2(galley.size().x + 24.0, 22.0);
    let (rect, resp) = ui.allocate_exact_size(desired, Sense::hover());
    let cr = CornerRadius::same(RADIUS_PILL);
    ui.painter().rect_filled(rect, cr, bg);
    ui.painter()
        .rect_stroke(rect, cr, Stroke::new(1.0_f32, with_alpha(fg, 36)), StrokeKind::Inside);
    ui.painter()
        .circle_filled(pos2(rect.left() + 10.0, rect.center().y), 2.8, fg);
    ui.painter().galley(
        pos2(
            rect.left() + 17.0,
            rect.center().y - galley.size().y / 2.0 - 0.5,
        ),
        galley,
        fg,
    );
    resp
}

// ─── Legacy-compatible Button Helpers (modernized styling) ─────────────────

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
        egui::Button::new(RichText::new(text.into()).font(fonts::bold(13.0)).color(p.on_accent)),
        p.accent,
        84.0,
    )
    .stroke(Stroke::NONE)
}

pub fn secondary_button(text: impl Into<String>) -> egui::Button<'static> {
    let p = current();
    pill(
        egui::Button::new(RichText::new(text.into()).font(fonts::medium(13.0)).color(p.text_primary)),
        p.neutral,
        84.0,
    )
}

pub fn danger_button(text: impl Into<String>) -> egui::Button<'static> {
    let p = current();
    pill(
        egui::Button::new(RichText::new(text.into()).font(fonts::bold(13.0)).color(Color32::WHITE)),
        p.danger,
        84.0,
    )
    .stroke(Stroke::NONE)
}

pub fn ghost_button(text: impl Into<String>) -> egui::Button<'static> {
    let p = current();
    pill(
        egui::Button::new(RichText::new(text.into()).font(fonts::medium(13.0)).color(p.text_secondary)),
        p.surface_raised,
        64.0,
    )
}

/// Smooth animated toggle switch tinted with the active accent color.
pub fn toggle(ui: &mut egui::Ui, on: &mut bool) -> egui::Response {
    let p = current();
    let desired = vec2(42.0, 23.0);
    let (rect, mut resp) = ui.allocate_exact_size(desired, Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    let t = ui.ctx().animate_bool_responsive(resp.id, *on);

    let off_track = if p.light {
        Color32::from_rgb(0xD4, 0xD4, 0xDE)
    } else {
        Color32::from_rgb(0x32, 0x32, 0x42)
    };
    let track_fill = lerp_color(off_track, p.accent, t);
    let radius = rect.height() / 2.0;

    ui.painter().rect_filled(rect, radius, track_fill);

    let knob_r = 8.5;
    let knob_x = egui::lerp((rect.left() + radius)..=(rect.right() - radius), t);
    let knob_center = pos2(knob_x, rect.center().y);
    let knob_color = if *on { p.on_accent } else { Color32::WHITE };
    ui.painter()
        .circle_filled(knob_center + vec2(0.0, 1.0), knob_r, Color32::from_black_alpha(38));
    ui.painter().circle_filled(knob_center, knob_r, knob_color);

    resp
}

pub fn divider(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, divider_color());
}

pub fn settings_card(ui: &mut egui::Ui, body: impl FnOnce(&mut egui::Ui)) {
    settings_card_frame().show(ui, |ui| {
        ui.set_width(ui.available_width());
        body(ui);
    });
}

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
                    .font(fonts::medium(14.0))
                    .color(text_primary()),
            );
            if let Some(d) = desc {
                ui.add(
                    egui::Label::new(
                        RichText::new(d)
                            .font(fonts::regular(12.0))
                            .color(text_secondary()),
                    )
                    .wrap(),
                );
            }
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                control(ui);
            });
        });
    } else {
        ui.horizontal(|ui| {
            let text_w = (avail * 0.58).clamp(180.0, 440.0);
            ui.vertical(|ui| {
                ui.set_width(text_w);
                ui.label(
                    RichText::new(title)
                        .font(fonts::medium(14.0))
                        .color(text_primary()),
                );
                if let Some(d) = desc {
                    ui.add(
                        egui::Label::new(
                            RichText::new(d)
                                .font(fonts::regular(12.0))
                                .color(text_secondary()),
                        )
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

pub fn menu_popup_pos(button: Rect, bounds: Rect, menu_height: f32) -> egui::Pos2 {
    let width = MENU_WIDTH + 12.0;
    let x = (button.right() - width).max(bounds.left() + SPACE_SM);

    let below = button.bottom() + 6.0;
    let y = if below + menu_height + SPACE_SM <= bounds.bottom() {
        below
    } else {
        (button.top() - menu_height - 6.0).max(bounds.top() + SPACE_SM)
    };

    pos2(x, y)
}

// ─── Color Utilities ───────────────────────────────────────────────────────

use eframe::egui::Vec2;

pub fn with_alpha(color: Color32, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha)
}

pub fn scale_alpha(color: Color32, factor: f32) -> Color32 {
    let a = ((color.a() as f32) * factor.clamp(0.0, 1.0)).round() as u8;
    with_alpha(color, a)
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

pub fn contrast_on(bg: Color32) -> Color32 {
    let luma = 0.299 * bg.r() as f32 + 0.587 * bg.g() as f32 + 0.114 * bg.b() as f32;
    if luma > 155.0 {
        Color32::from_rgb(16, 16, 22)
    } else {
        Color32::WHITE
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

pub fn muted_label(text: impl Into<String>) -> RichText {
    RichText::new(text.into())
        .font(fonts::regular(12.5))
        .color(text_muted())
}

pub fn secondary_label(text: impl Into<String>) -> RichText {
    RichText::new(text.into())
        .font(fonts::regular(13.0))
        .color(text_secondary())
}

pub fn section_heading(text: impl Into<String>) -> RichText {
    RichText::new(text.into().to_uppercase())
        .font(fonts::bold(10.5))
        .color(text_muted())
}

pub fn field_label(text: impl Into<String>) -> RichText {
    RichText::new(text.into())
        .font(fonts::medium(12.5))
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
        assert_eq!(p.border.a(), 255);
        assert_eq!(p.border_hover.a(), 255);
        assert_eq!(p.divider.a(), 255);
        assert_eq!(p.specular.a(), 255);
    }

    #[test]
    fn contrast_picks_dark_text_on_bright_accent() {
        assert_eq!(
            contrast_on(Color32::from_rgb(255, 240, 120)),
            Color32::from_rgb(16, 16, 22)
        );
        assert_eq!(contrast_on(Color32::from_rgb(40, 20, 80)), Color32::WHITE);
    }
}
