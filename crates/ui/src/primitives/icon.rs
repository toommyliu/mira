use gpui::{Hsla, Styled as _, Svg, px, svg};

use crate::assets::{HEAVY, SIDEBAR, SIDEBAR_OPEN};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Icon {
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    Check,
    ChevronDown,
    ChevronRight,
    ChevronUp,
    Clock,
    LayoutGrid,
    Minus,
    Plus,
    RotateCcw,
    Square,
    X,
}

impl Icon {
    fn path(self) -> &'static str {
        match self {
            Icon::ArrowDown => "icons/arrow-down.svg",
            Icon::ArrowLeft => "icons/arrow-left.svg",
            Icon::ArrowRight => "icons/arrow-right.svg",
            Icon::ArrowUp => "icons/arrow-up.svg",
            Icon::Check => "icons/check.svg",
            Icon::ChevronDown => "icons/chevron-down.svg",
            Icon::ChevronRight => "icons/chevron-right.svg",
            Icon::ChevronUp => "icons/chevron-up.svg",
            Icon::Clock => "icons/clock.svg",
            Icon::LayoutGrid => "icons/layout-grid.svg",
            Icon::Minus => "icons/minus.svg",
            Icon::Plus => "icons/plus.svg",
            Icon::RotateCcw => "icons/rotate-ccw.svg",
            Icon::Square => "icons/square.svg",
            Icon::X => "icons/x.svg",
        }
    }

    pub fn for_key(key: &str) -> Option<Icon> {
        match key {
            "↑" => Some(Icon::ArrowUp),
            "↓" => Some(Icon::ArrowDown),
            "←" => Some(Icon::ArrowLeft),
            "→" => Some(Icon::ArrowRight),
            _ => None,
        }
    }
}

pub fn icon(icon: Icon, color: Hsla) -> Svg {
    svg()
        .path(icon.path())
        .flex_none()
        .size(px(14.0))
        .text_color(color)
}

pub fn sidebar_icon(open: bool, color: Hsla) -> Svg {
    svg()
        .path(if open { SIDEBAR_OPEN } else { SIDEBAR })
        .flex_none()
        .w(px(16.0))
        .h(px(13.0))
        .text_color(color)
}

pub(crate) fn small_icon(icon: Icon, color: Hsla) -> Svg {
    svg()
        .path(format!("{HEAVY}{}", icon.path()))
        .flex_none()
        .size(px(10.0))
        .text_color(color)
}
