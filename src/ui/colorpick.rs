//! Custom HSV colour picker styled after ytm-rs:
//! a round swatch that opens a saturation/brightness square, a hue bar, and a `#RRGGBB` hex input.

use crate::ui::theme;
use eframe::egui::{
    self, Color32, ColorImage, CornerRadius, Id, Sense, Stroke, TextureHandle, TextureOptions, Ui,
    Vec2, pos2, vec2,
};

const PICKER_WIDTH: f32 = 216.0;

/// Converts `[r, g, b]` to `Color32`.
pub fn rgb_to_color(rgb: [u8; 3]) -> Color32 {
    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}

/// Converts `Color32` to `[r, g, b]`.
pub fn color_to_rgb(c: Color32) -> [u8; 3] {
    [c.r(), c.g(), c.b()]
}

/// Formats `Color32` as `#RRGGBB`.
pub fn to_hex(c: Color32) -> String {
    format!("#{:02X}{:02X}{:02X}", c.r(), c.g(), c.b())
}

/// Parses `#RRGGBB` or `RRGGBB` into `Color32`.
pub fn parse_hex(raw: &str) -> Option<Color32> {
    let hex = raw.trim().strip_prefix('#').unwrap_or(raw.trim());
    if hex.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some(Color32::from_rgb(r, g, b))
}

/// Round swatch button that opens the HSV picker popup. Returns `true` when `rgb` changed.
pub fn swatch_button(ui: &mut Ui, id_salt: impl std::hash::Hash, rgb: &mut [u8; 3]) -> bool {
    let mut color = rgb_to_color(*rgb);
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(30.0), Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    let popup_id = ui.make_persistent_id(id_salt);

    if resp.clicked() {
        ui.memory_mut(|m| m.toggle_popup(popup_id));
    }

    let is_open = ui.memory(|m| m.is_popup_open(popup_id));
    let ring_alpha = if resp.hovered() || is_open { 210 } else { 90 };
    let ring_color = if is_open {
        theme::accent()
    } else {
        theme::with_alpha(theme::text_primary(), ring_alpha)
    };

    ui.painter().circle_filled(rect.center(), 11.0, color);
    ui.painter()
        .circle_stroke(rect.center(), 13.5, Stroke::new(1.5_f32, ring_color));

    let mut changed = false;
    egui::popup_below_widget(
        ui,
        popup_id,
        &resp,
        egui::PopupCloseBehavior::CloseOnClickOutside,
        |ui| {
            changed = render_picker_body(ui, popup_id, &mut color);
        },
    );

    if changed {
        *rgb = color_to_rgb(color);
    }
    changed
}

/// Optional detail colour control: shows either a "Default" pill + swatch or the active custom
/// swatch + hex + "Reset" button. Returns `true` when modified.
pub fn optional_detail_picker(
    ui: &mut Ui,
    id_salt: impl std::hash::Hash + Copy,
    value: &mut Option<[u8; 3]>,
    fallback: Color32,
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let mut preview = value.unwrap_or_else(|| color_to_rgb(fallback));
        if swatch_button(ui, (id_salt, "swatch"), &mut preview) {
            *value = Some(preview);
            changed = true;
        }
        if let Some(rgb) = *value {
            ui.label(theme::secondary_label(to_hex(rgb_to_color(rgb))).size(12.0));
            if ui
                .add(theme::ghost_button("Reset").min_size(vec2(54.0, 26.0)))
                .clicked()
            {
                *value = None;
                changed = true;
            }
        } else {
            ui.label(theme::muted_label("Default").size(12.0));
        }
    });
    changed
}

fn render_picker_body(ui: &mut Ui, id: Id, c: &mut Color32) -> bool {
    ui.set_width(PICKER_WIDTH);
    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);

    let hsv_key = id.with("hsv");
    let mut hsv = ui
        .data(|d| d.get_temp::<(Color32, [f32; 3])>(hsv_key))
        .filter(|(prev, _)| prev == c)
        .map_or_else(|| rgb_to_hsv(*c), |(_, h)| h);

    let mut changed = false;

    // Saturation (x) / Value (y) 2D square
    let (sv_rect, sv_resp) = ui.allocate_exact_size(vec2(PICKER_WIDTH, 148.0), Sense::click_and_drag());
    if let Some(p) = sv_resp.interact_pointer_pos() {
        hsv[1] = ((p.x - sv_rect.left()) / sv_rect.width()).clamp(0.0, 1.0);
        hsv[2] = 1.0 - ((p.y - sv_rect.top()) / sv_rect.height()).clamp(0.0, 1.0);
        changed = true;
    }

    let sv_tex = sv_texture(ui.ctx(), id.with("sv_tex"), hsv[0]);
    ui.painter().image(
        sv_tex.id(),
        sv_rect,
        egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
        Color32::WHITE,
    );
    ui.painter().rect_stroke(
        sv_rect,
        CornerRadius::same(6),
        Stroke::new(1.0_f32, theme::border()),
        egui::StrokeKind::Inside,
    );

    let handle_pos = pos2(
        sv_rect.left() + hsv[1] * sv_rect.width(),
        sv_rect.top() + (1.0 - hsv[2]) * sv_rect.height(),
    );
    ui.painter()
        .circle_stroke(handle_pos, 7.0, Stroke::new(2.0_f32, Color32::WHITE));
    ui.painter()
        .circle_stroke(handle_pos, 8.2, Stroke::new(1.0_f32, Color32::from_black_alpha(150)));

    ui.add_space(10.0);

    // Hue bar
    let (hue_rect, hue_resp) = ui.allocate_exact_size(vec2(PICKER_WIDTH, 14.0), Sense::click_and_drag());
    if let Some(p) = hue_resp.interact_pointer_pos() {
        hsv[0] = ((p.x - hue_rect.left()) / hue_rect.width()).clamp(0.0, 0.9999);
        changed = true;
    }

    let hue_tex = hue_texture(ui.ctx(), id.with("hue_tex"));
    ui.painter().image(
        hue_tex.id(),
        hue_rect,
        egui::Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
        Color32::WHITE,
    );
    let hue_x = hue_rect.left() + hsv[0] * hue_rect.width();
    let hue_center = pos2(hue_x, hue_rect.center().y);
    ui.painter().circle_filled(hue_center, 7.0, hsv_to_rgb(hsv[0], 1.0, 1.0));
    ui.painter()
        .circle_stroke(hue_center, 7.0, Stroke::new(2.0_f32, Color32::WHITE));

    if changed {
        *c = hsv_to_rgb(hsv[0], hsv[1], hsv[2]);
    }

    ui.add_space(10.0);

    // Hex input row
    let hex_key = id.with("hex_buf");
    let mut hex_buf = ui
        .data(|d| d.get_temp::<String>(hex_key))
        .unwrap_or_else(|| to_hex(*c));
    if changed {
        hex_buf = to_hex(*c);
    }

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let (preview_rect, _) = ui.allocate_exact_size(vec2(26.0, 26.0), Sense::hover());
        ui.painter()
            .rect_filled(preview_rect, CornerRadius::same(6), *c);
        ui.painter().rect_stroke(
            preview_rect,
            CornerRadius::same(6),
            Stroke::new(1.0_f32, theme::border()),
            egui::StrokeKind::Inside,
        );

        let edit = ui.add_sized(
            [PICKER_WIDTH - 34.0, 28.0],
            egui::TextEdit::singleline(&mut hex_buf)
                .font(egui::TextStyle::Monospace)
                .hint_text("#FF3048"),
        );
        if edit.changed() {
            if let Some(parsed) = parse_hex(&hex_buf) {
                *c = parsed;
                hsv = rgb_to_hsv(parsed);
                changed = true;
            }
        }
        if !edit.has_focus() {
            hex_buf = to_hex(*c);
        }
    });

    ui.data_mut(|d| {
        d.insert_temp(hsv_key, (*c, hsv));
        d.insert_temp(hex_key, hex_buf);
    });

    changed
}

fn sv_texture(ctx: &egui::Context, id: Id, hue: f32) -> TextureHandle {
    let bucket = (hue * 360.0).round() as i32;
    let cached = ctx.data(|d| d.get_temp::<(i32, TextureHandle)>(id));
    if let Some((b, tex)) = cached {
        if b == bucket {
            return tex;
        }
    }
    const W: usize = 64;
    const H: usize = 64;
    let mut pixels = Vec::with_capacity(W * H);
    for y in 0..H {
        let v = 1.0 - (y as f32 / (H - 1) as f32);
        for x in 0..W {
            let s = x as f32 / (W - 1) as f32;
            pixels.push(hsv_to_rgb(hue, s, v));
        }
    }
    let image = ColorImage {
        size: [W, H],
        pixels,
    };
    let tex = ctx.load_texture(format!("colorpick_sv_{bucket}"), image, TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(id, (bucket, tex.clone())));
    tex
}

fn hue_texture(ctx: &egui::Context, id: Id) -> TextureHandle {
    if let Some(tex) = ctx.data(|d| d.get_temp::<TextureHandle>(id)) {
        return tex;
    }
    const W: usize = 128;
    let mut pixels = Vec::with_capacity(W);
    for x in 0..W {
        let h = x as f32 / (W - 1) as f32;
        pixels.push(hsv_to_rgb(h, 1.0, 1.0));
    }
    let image = ColorImage {
        size: [W, 1],
        pixels,
    };
    let tex = ctx.load_texture("colorpick_hue", image, TextureOptions::LINEAR);
    ctx.data_mut(|d| d.insert_temp(id, tex.clone()));
    tex
}

fn rgb_to_hsv(c: Color32) -> [f32; 3] {
    let r = c.r() as f32 / 255.0;
    let g = c.g() as f32 / 255.0;
    let b = c.b() as f32 / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let h = if delta < 1e-6 {
        0.0
    } else if (max - r).abs() < 1e-6 {
        ((g - b) / delta).rem_euclid(6.0) / 6.0
    } else if (max - g).abs() < 1e-6 {
        ((b - r) / delta + 2.0) / 6.0
    } else {
        ((r - g) / delta + 4.0) / 6.0
    };
    let s = if max < 1e-6 { 0.0 } else { delta / max };
    [h, s, max]
}

fn hsv_to_rgb(h: f32, s: f32, v: f32) -> Color32 {
    let h6 = (h.clamp(0.0, 1.0) * 6.0) % 6.0;
    let c = v * s;
    let x = c * (1.0 - ((h6 % 2.0) - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match h6 as i32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    Color32::from_rgb(
        ((r + m) * 255.0).round() as u8,
        ((g + m) * 255.0).round() as u8,
        ((b + m) * 255.0).round() as u8,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trip() {
        let c = Color32::from_rgb(0xFF, 0x30, 0x48);
        assert_eq!(to_hex(c), "#FF3048");
        assert_eq!(parse_hex("#ff3048"), Some(c));
        assert_eq!(parse_hex("FF3048"), Some(c));
        assert_eq!(parse_hex("invalid"), None);
    }
}
