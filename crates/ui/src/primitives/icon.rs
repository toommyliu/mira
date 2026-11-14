use gpui::{App, Hsla, IntoElement, RenderOnce, Styled as _, Svg, Window, px, svg};

use crate::assets::{HEAVY, SIDEBAR, SIDEBAR_OPEN};

#[derive(Clone, Copy, PartialEq, Eq, Debug, IntoElement)]
pub enum Icon {
    ArrowDown,
    ArrowDownToLine,
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    Bell,
    Check,
    ChevronDown,
    ChevronRight,
    ChevronUp,
    Clock,
    Ellipsis,
    Eye,
    EyeOff,
    LayoutGrid,
    Minus,
    Pin,
    Plus,
    RotateCcw,
    Search,
    Sidebar,
    SidebarOpen,
    Square,
    X,
}

impl Icon {
    fn path(self) -> &'static str {
        match self {
            Icon::ArrowDown => "icons/arrow-down.svg",
            Icon::ArrowDownToLine => "icons/arrow-down-to-line.svg",
            Icon::ArrowLeft => "icons/arrow-left.svg",
            Icon::ArrowRight => "icons/arrow-right.svg",
            Icon::ArrowUp => "icons/arrow-up.svg",
            Icon::Bell => "icons/bell.svg",
            Icon::Check => "icons/check.svg",
            Icon::ChevronDown => "icons/chevron-down.svg",
            Icon::ChevronRight => "icons/chevron-right.svg",
            Icon::ChevronUp => "icons/chevron-up.svg",
            Icon::Clock => "icons/clock.svg",
            Icon::Ellipsis => "icons/ellipsis.svg",
            Icon::Eye => "icons/eye.svg",
            Icon::EyeOff => "icons/eye-off.svg",
            Icon::LayoutGrid => "icons/layout-grid.svg",
            Icon::Minus => "icons/minus.svg",
            Icon::Pin => "icons/pin.svg",
            Icon::Plus => "icons/plus.svg",
            Icon::RotateCcw => "icons/rotate-ccw.svg",
            Icon::Search => "icons/search.svg",
            Icon::Sidebar => SIDEBAR,
            Icon::SidebarOpen => SIDEBAR_OPEN,
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
    let svg = svg().path(icon.path()).flex_none().text_color(color);
    match icon {
        Icon::Sidebar | Icon::SidebarOpen => svg.w(px(18.0)).h(px(14.0)),
        _ => svg.size(px(14.0)),
    }
}

pub fn small_icon(icon: Icon, color: Hsla) -> Svg {
    svg()
        .path(format!("{HEAVY}{}", icon.path()))
        .flex_none()
        .size(px(10.0))
        .text_color(color)
}

impl RenderOnce for Icon {
    fn render(self, window: &mut Window, _: &mut App) -> impl IntoElement {
        icon(self, window.text_style().color)
    }
}
