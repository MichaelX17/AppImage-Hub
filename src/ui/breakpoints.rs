//! Single source of truth for width breakpoints and size classes.
//! Every UI layout decision that depends on available width MUST go
//! through here.

#[allow(dead_code)]
pub const BP_LARGE: f32 = 1200.0; // full layout
#[allow(dead_code)]
pub const BP_MEDIUM: f32 = 900.0; // cards start stacking
#[allow(dead_code)]
pub const BP_NARROW: f32 = 640.0; // toolbar collapses, side panel becomes drawer
#[allow(dead_code)]
pub const BP_TIGHT: f32 = 480.0; // icon-only toolbar, single-column everything

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum SizeClass {
    Large,
    Medium,
    Narrow,
    Tight,
}

impl SizeClass {
    pub fn from_width(w: f32) -> Self {
        if w >= BP_LARGE {
            Self::Large
        } else if w >= BP_MEDIUM {
            Self::Medium
        } else if w >= BP_NARROW {
            Self::Narrow
        } else {
            Self::Tight
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thresholds() {
        assert_eq!(SizeClass::from_width(1200.0), SizeClass::Large);
        assert_eq!(SizeClass::from_width(1199.9), SizeClass::Medium);
        assert_eq!(SizeClass::from_width(900.0), SizeClass::Medium);
        assert_eq!(SizeClass::from_width(899.9), SizeClass::Narrow);
        assert_eq!(SizeClass::from_width(640.0), SizeClass::Narrow);
        assert_eq!(SizeClass::from_width(639.9), SizeClass::Tight);
        assert_eq!(SizeClass::from_width(480.0), SizeClass::Tight);
        assert_eq!(SizeClass::from_width(479.9), SizeClass::Tight);
    }
}
