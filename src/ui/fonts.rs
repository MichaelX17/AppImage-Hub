//! System font loader providing real Regular, Medium, Bold, and Monospace weights.
//!
//! `egui` 0.31's `.strong()` only alters text color, not glyph weight. Loading real
//! static sans-serif weights from standard Linux font directories gives crisp modern
//! typographic hierarchy while falling back cleanly to `egui`'s bundled fonts.

use eframe::egui::{self, FontData, FontDefinitions, FontFamily, FontId};
use std::path::Path;
use std::sync::Arc;

pub const FAMILY_MEDIUM: &str = "app_medium";
pub const FAMILY_BOLD: &str = "app_bold";

const REGULAR_CANDIDATES: &[&str] = &[
    "/usr/share/fonts/noto/NotoSans-Regular.ttf",
    "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
    "/usr/share/fonts/google-noto/NotoSans-Regular.ttf",
    "/usr/share/fonts/Adwaita/AdwaitaSans-Regular.ttf",
    "/usr/share/fonts/cantarell/Cantarell-Regular.otf",
    "/usr/share/fonts/truetype/inter/Inter-Regular.ttf",
    "/usr/share/fonts/inter/Inter-Regular.ttf",
    "/usr/share/fonts/truetype/ubuntu/Ubuntu-R.ttf",
    "/usr/share/fonts/TTF/DejaVuSans.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/liberation/LiberationSans-Regular.ttf",
    "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
];

const MEDIUM_CANDIDATES: &[&str] = &[
    "/usr/share/fonts/noto/NotoSans-Medium.ttf",
    "/usr/share/fonts/truetype/noto/NotoSans-Medium.ttf",
    "/usr/share/fonts/google-noto/NotoSans-Medium.ttf",
    "/usr/share/fonts/truetype/inter/Inter-Medium.ttf",
    "/usr/share/fonts/inter/Inter-Medium.ttf",
    "/usr/share/fonts/cantarell/Cantarell-Bold.otf",
    "/usr/share/fonts/truetype/ubuntu/Ubuntu-M.ttf",
];

const BOLD_CANDIDATES: &[&str] = &[
    "/usr/share/fonts/noto/NotoSans-Bold.ttf",
    "/usr/share/fonts/truetype/noto/NotoSans-Bold.ttf",
    "/usr/share/fonts/google-noto/NotoSans-Bold.ttf",
    "/usr/share/fonts/truetype/inter/Inter-SemiBold.ttf",
    "/usr/share/fonts/truetype/inter/Inter-Bold.ttf",
    "/usr/share/fonts/inter/Inter-Bold.ttf",
    "/usr/share/fonts/cantarell/Cantarell-Bold.otf",
    "/usr/share/fonts/truetype/ubuntu/Ubuntu-B.ttf",
    "/usr/share/fonts/TTF/DejaVuSans-Bold.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
    "/usr/share/fonts/liberation/LiberationSans-Bold.ttf",
    "/usr/share/fonts/truetype/liberation/LiberationSans-Bold.ttf",
];

const MONO_CANDIDATES: &[&str] = &[
    "/usr/share/fonts/noto/NotoSansMono-Regular.ttf",
    "/usr/share/fonts/truetype/noto/NotoSansMono-Regular.ttf",
    "/usr/share/fonts/TTF/JetBrainsMono-Regular.ttf",
    "/usr/share/fonts/truetype/jetbrains-mono/JetBrainsMono-Regular.ttf",
    "/usr/share/fonts/TTF/DejaVuSansMono.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
    "/usr/share/fonts/Adwaita/AdwaitaMono-Regular.ttf",
];

fn load_first(candidates: &[&str]) -> Option<Arc<FontData>> {
    for path in candidates {
        if Path::new(path).is_file() {
            if let Ok(bytes) = std::fs::read(path) {
                return Some(Arc::new(FontData::from_owned(bytes)));
            }
        }
    }
    None
}

/// Installs crisp system sans-serif (Regular, Medium, Bold) and Monospace fonts
/// into `ctx`, keeping `egui`'s bundled fonts as fallbacks for symbols/glyphs.
pub fn install(ctx: &egui::Context) {
    let mut defs = FontDefinitions::default();

    let regular = load_first(REGULAR_CANDIDATES);
    let medium = load_first(MEDIUM_CANDIDATES);
    let bold = load_first(BOLD_CANDIDATES);
    let mono = load_first(MONO_CANDIDATES);

    if let Some(data) = regular {
        defs.font_data.insert("sys_regular".to_owned(), data);
        if let Some(list) = defs.families.get_mut(&FontFamily::Proportional) {
            list.insert(0, "sys_regular".to_owned());
        }
    }

    let base_proportional = defs
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();

    let mut medium_list = base_proportional.clone();
    if let Some(data) = medium {
        defs.font_data.insert("sys_medium".to_owned(), data);
        medium_list.insert(0, "sys_medium".to_owned());
    }
    defs.families
        .insert(FontFamily::Name(FAMILY_MEDIUM.into()), medium_list);

    let mut bold_list = base_proportional;
    if let Some(data) = bold {
        defs.font_data.insert("sys_bold".to_owned(), data);
        bold_list.insert(0, "sys_bold".to_owned());
    }
    defs.families
        .insert(FontFamily::Name(FAMILY_BOLD.into()), bold_list);

    if let Some(data) = mono {
        defs.font_data.insert("sys_mono".to_owned(), data);
        if let Some(list) = defs.families.get_mut(&FontFamily::Monospace) {
            list.insert(0, "sys_mono".to_owned());
        }
    }

    ctx.set_fonts(defs);
}

pub fn regular(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

pub fn medium(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(FAMILY_MEDIUM.into()))
}

pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(FAMILY_BOLD.into()))
}

pub fn mono(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}
