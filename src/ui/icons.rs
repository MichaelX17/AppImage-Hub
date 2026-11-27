//! Minimalist procedural vector icons modeled after Lucide (1.6 px strokes on a 24×24 grid).
//!
//! Zero external icon-font dependencies and crisp rendering at any DPI scale.

use eframe::egui::{self, Color32, CornerRadius, Pos2, Rect, Shape, Stroke, StrokeKind, pos2, vec2};
use std::f32::consts::TAU;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    AppsGrid,
    Updates,
    GitBranch,
    HardDrive,
    Settings,
    Search,
    Plus,
    Play,
    Edit,
    Folder,
    MoreHorizontal,
    Trash,
    Refresh,
    Check,
    AlertTriangle,
    Close,
    ChevronLeft,
    ChevronDown,
    LayoutGrid,
    LayoutList,
    Sliders,
    Sun,
    Moon,
    Palette,
    Link,
    Key,
    Info,
    Download,
    Command,
    ArrowUpRight,
}

/// Paints `icon` centered inside `center` with edge length `size` and `color`.
pub fn paint(painter: &egui::Painter, icon: Icon, center: Pos2, size: f32, color: Color32) {
    paint_rotated(painter, icon, center, size, color, 0.0);
}

/// Paints `icon` with optional rotation in radians (used for spinning sync/refresh icon).
pub fn paint_rotated(
    painter: &egui::Painter,
    icon: Icon,
    center: Pos2,
    size: f32,
    color: Color32,
    angle: f32,
) {
    let s = size / 24.0;
    let sw = (1.65 * s).clamp(1.2, 2.4);
    let stroke = Stroke::new(sw, color);
    let (sin, cos) = angle.sin_cos();

    let pt = |x: f32, y: f32| -> Pos2 {
        let dx = (x - 12.0) * s;
        let dy = (y - 12.0) * s;
        if angle == 0.0 {
            pos2(center.x + dx, center.y + dy)
        } else {
            pos2(center.x + dx * cos - dy * sin, center.y + dx * sin + dy * cos)
        }
    };

    let line = |a: (f32, f32), b: (f32, f32)| {
        painter.line_segment([pt(a.0, a.1), pt(b.0, b.1)], stroke);
    };

    let poly = |pts: &[(f32, f32)]| {
        let mapped: Vec<Pos2> = pts.iter().map(|&(x, y)| pt(x, y)).collect();
        painter.add(Shape::line(mapped, stroke));
    };

    let arc = |cx: f32, cy: f32, r: f32, a0: f32, a1: f32, steps: usize| {
        let mut pts = Vec::with_capacity(steps + 1);
        for i in 0..=steps {
            let t = i as f32 / steps as f32;
            let a = a0 + (a1 - a0) * t;
            pts.push(pt(cx + r * a.cos(), cy + r * a.sin()));
        }
        painter.add(Shape::line(pts, stroke));
    };

    match icon {
        Icon::AppsGrid | Icon::LayoutGrid => {
            let cr = CornerRadius::same((2.0 * s).round().max(1.0) as u8);
            for (x, y) in [(3.5, 3.5), (14.0, 3.5), (3.5, 14.0), (14.0, 14.0)] {
                let r = Rect::from_min_max(pt(x, y), pt(x + 6.5, y + 6.5));
                painter.rect_stroke(r, cr, stroke, StrokeKind::Middle);
            }
        }
        Icon::LayoutList => {
            for y in [6.0, 12.0, 18.0] {
                painter.circle_filled(pt(4.5, y), (1.3 * s).max(1.0), color);
                line((8.5, y), (20.5, y));
            }
        }
        Icon::Updates | Icon::Download => {
            line((12.0, 3.5), (12.0, 15.0));
            poly(&[(7.5, 11.0), (12.0, 15.5), (16.5, 11.0)]);
            poly(&[(4.0, 16.5), (4.0, 19.5), (20.0, 19.5), (20.0, 16.5)]);
        }
        Icon::GitBranch => {
            painter.circle_stroke(pt(6.5, 6.0), 2.5 * s, stroke);
            painter.circle_stroke(pt(6.5, 18.0), 2.5 * s, stroke);
            painter.circle_stroke(pt(17.5, 8.5), 2.5 * s, stroke);
            line((6.5, 8.5), (6.5, 15.5));
            poly(&[(17.5, 11.0), (17.5, 14.0), (14.0, 17.0), (9.0, 18.0)]);
        }
        Icon::HardDrive => {
            let r = Rect::from_min_max(pt(3.5, 5.0), pt(20.5, 19.0));
            painter.rect_stroke(
                r,
                CornerRadius::same((3.0 * s).round().max(2.0) as u8),
                stroke,
                StrokeKind::Middle,
            );
            line((3.5, 13.0), (20.5, 13.0));
            painter.circle_filled(pt(7.5, 16.0), (1.1 * s).max(1.0), color);
            painter.circle_filled(pt(11.0, 16.0), (1.1 * s).max(1.0), color);
        }
        Icon::Settings => {
            painter.circle_stroke(pt(12.0, 12.0), 3.3 * s, stroke);
            painter.circle_stroke(pt(12.0, 12.0), 7.2 * s, stroke);
            for i in 0..8 {
                let a = (i as f32) * (TAU / 8.0);
                let (sn, cs) = a.sin_cos();
                line(
                    (12.0 + 7.2 * cs, 12.0 + 7.2 * sn),
                    (12.0 + 9.6 * cs, 12.0 + 9.6 * sn),
                );
            }
        }
        Icon::Search => {
            painter.circle_stroke(pt(10.5, 10.5), 6.2 * s, stroke);
            line((15.2, 15.2), (20.2, 20.2));
        }
        Icon::Plus => {
            line((12.0, 5.0), (12.0, 19.0));
            line((5.0, 12.0), (19.0, 12.0));
        }
        Icon::Play => {
            let tri = vec![pt(7.5, 5.2), pt(19.0, 12.0), pt(7.5, 18.8)];
            painter.add(Shape::convex_polygon(tri, color, Stroke::NONE));
        }
        Icon::Edit => {
            poly(&[
                (16.5, 4.0),
                (20.0, 7.5),
                (8.5, 19.0),
                (4.0, 20.0),
                (5.0, 15.5),
                (16.5, 4.0),
            ]);
            line((14.0, 6.5), (17.5, 10.0));
        }
        Icon::Folder => {
            poly(&[
                (3.5, 6.5),
                (9.2, 6.5),
                (11.2, 9.0),
                (20.5, 9.0),
                (20.5, 18.5),
                (3.5, 18.5),
                (3.5, 6.5),
            ]);
        }
        Icon::MoreHorizontal => {
            let r = (1.6 * s).max(1.3);
            painter.circle_filled(pt(6.0, 12.0), r, color);
            painter.circle_filled(pt(12.0, 12.0), r, color);
            painter.circle_filled(pt(18.0, 12.0), r, color);
        }
        Icon::Trash => {
            line((4.5, 6.5), (19.5, 6.5));
            poly(&[(9.0, 6.5), (9.0, 4.2), (15.0, 4.2), (15.0, 6.5)]);
            poly(&[(6.5, 6.5), (7.3, 19.5), (16.7, 19.5), (17.5, 6.5)]);
            line((10.2, 10.2), (10.2, 16.0));
            line((13.8, 10.2), (13.8, 16.0));
        }
        Icon::Refresh => {
            arc(12.0, 12.0, 7.2, -2.5, 0.3, 12);
            poly(&[(16.8, 7.2), (19.4, 9.4), (19.8, 6.0)]);
            arc(12.0, 12.0, 7.2, 0.65, 3.45, 12);
            poly(&[(7.2, 16.8), (4.6, 14.6), (4.2, 18.0)]);
        }
        Icon::Check => {
            poly(&[(5.0, 12.5), (9.8, 17.2), (19.2, 7.2)]);
        }
        Icon::AlertTriangle => {
            poly(&[(12.0, 4.0), (20.5, 19.0), (3.5, 19.0), (12.0, 4.0)]);
            line((12.0, 9.5), (12.0, 13.8));
            painter.circle_filled(pt(12.0, 16.5), (1.1 * s).max(1.0), color);
        }
        Icon::Close => {
            line((6.0, 6.0), (18.0, 18.0));
            line((18.0, 6.0), (6.0, 18.0));
        }
        Icon::ChevronLeft => {
            poly(&[(15.0, 6.0), (9.0, 12.0), (15.0, 18.0)]);
        }
        Icon::ChevronDown => {
            poly(&[(6.5, 9.5), (12.0, 15.0), (17.5, 9.5)]);
        }
        Icon::Sliders => {
            for (y, kx) in [(6.5, 9.0), (12.0, 15.5), (17.5, 10.5)] {
                line((4.0, y), (kx - 2.2, y));
                line((kx + 2.2, y), (20.0, y));
                painter.circle_stroke(pt(kx, y), 2.1 * s, stroke);
            }
        }
        Icon::Sun => {
            painter.circle_stroke(pt(12.0, 12.0), 4.0 * s, stroke);
            for i in 0..8 {
                let a = (i as f32) * (TAU / 8.0);
                let (sn, cs) = a.sin_cos();
                line(
                    (12.0 + 6.8 * cs, 12.0 + 6.8 * sn),
                    (12.0 + 9.2 * cs, 12.0 + 9.2 * sn),
                );
            }
        }
        Icon::Moon => {
            arc(12.5, 12.0, 7.0, 0.45, 4.45, 16);
            arc(15.5, 9.2, 5.2, 1.35, 3.65, 12);
        }
        Icon::Palette => {
            painter.circle_stroke(pt(12.0, 12.0), 8.2 * s, stroke);
            for (x, y) in [(8.5, 10.0), (12.0, 7.8), (15.5, 10.0), (15.2, 14.2)] {
                painter.circle_filled(pt(x, y), (1.3 * s).max(1.1), color);
            }
        }
        Icon::Link => {
            arc(9.0, 12.0, 4.0, 1.57, 4.71, 10);
            arc(15.0, 12.0, 4.0, -1.57, 1.57, 10);
            line((9.0, 8.0), (11.5, 8.0));
            line((9.0, 16.0), (11.5, 16.0));
            line((12.5, 8.0), (15.0, 8.0));
            line((12.5, 16.0), (15.0, 16.0));
            line((9.5, 12.0), (14.5, 12.0));
        }
        Icon::Key => {
            painter.circle_stroke(pt(8.0, 12.0), 3.8 * s, stroke);
            line((11.8, 12.0), (20.5, 12.0));
            line((17.0, 12.0), (17.0, 15.5));
            line((20.0, 12.0), (20.0, 14.8));
        }
        Icon::Info => {
            painter.circle_stroke(pt(12.0, 12.0), 8.2 * s, stroke);
            line((12.0, 11.2), (12.0, 16.2));
            painter.circle_filled(pt(12.0, 8.2), (1.15 * s).max(1.0), color);
        }
        Icon::Command => {
            let r = Rect::from_min_max(pt(9.0, 9.0), pt(15.0, 15.0));
            painter.rect_stroke(r, CornerRadius::ZERO, stroke, StrokeKind::Middle);
            for (cx, cy) in [(6.5, 6.5), (17.5, 6.5), (6.5, 17.5), (17.5, 17.5)] {
                painter.circle_stroke(pt(cx, cy), 2.5 * s, stroke);
            }
        }
        Icon::ArrowUpRight => {
            line((7.0, 17.0), (17.0, 7.0));
            poly(&[(9.0, 7.0), (17.0, 7.0), (17.0, 15.0)]);
        }
    }
}

/// Convenience helper: paints an icon inline as an egui widget.
pub fn widget(ui: &mut egui::Ui, icon: Icon, size: f32, color: Color32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(size, size), egui::Sense::hover());
    if ui.is_rect_visible(rect) {
        paint(ui.painter(), icon, rect.center(), size, color);
    }
    resp
}
