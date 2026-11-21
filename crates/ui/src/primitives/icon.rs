use gpui::{App, Hsla, IntoElement, RenderOnce, Styled as _, Svg, Window, px, svg};

use crate::Tone;

#[derive(Clone, Copy, PartialEq, Eq, Debug, IntoElement)]
pub enum Icon {
    ArrowDown,
    ArrowDownToLine,
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    Bell,
    BellRing,
    Check,
    ChevronDown,
    ChevronRight,
    ChevronUp,
    CircleAlert,
    CircleDot,
    FileCode,
    FilePlus2,
    Pencil,
    LoaderCircle,
    Circle,
    CircleStop,
    Download,

    CircleCheck,
    Clock,
    Ellipsis,
    Eye,
    EyeOff,
    GripVertical,
    Info,
    LayoutGrid,
    MapPin,
    Minus,
    Pin,
    Plus,
    RefreshCw,
    RotateCcw,
    RotateCw,
    Search,
    Server,
    Sidebar,
    SidebarOpen,
    Square,
    TriangleAlert,
    User,
    X,
}

impl Icon {
    pub(super) fn of(tone: Tone) -> Self {
        match tone {
            Tone::Neutral | Tone::Accent => Icon::Info,
            Tone::Success => Icon::CircleCheck,
            Tone::Warning | Tone::Attention => Icon::TriangleAlert,
            Tone::Danger => Icon::CircleAlert,
        }
    }

    pub(super) fn path(self) -> &'static str {
        match self {
            Icon::ArrowDown => "icons/arrow-down.svg",
            Icon::ArrowDownToLine => "icons/arrow-down-to-line.svg",
            Icon::ArrowLeft => "icons/arrow-left.svg",
            Icon::ArrowRight => "icons/arrow-right.svg",
            Icon::ArrowUp => "icons/arrow-up.svg",
            Icon::Bell => "icons/bell.svg",
            Icon::BellRing => "icons/bell-ring.svg",
            Icon::Check => "icons/check.svg",
            Icon::ChevronDown => "icons/chevron-down.svg",
            Icon::ChevronRight => "icons/chevron-right.svg",
            Icon::ChevronUp => "icons/chevron-up.svg",
            Icon::CircleDot => "icons/circle-dot.svg",
            Icon::FileCode => "icons/file-code.svg",
            Icon::FilePlus2 => "icons/file-plus-2.svg",
            Icon::Pencil => "icons/pencil.svg",
            Icon::LoaderCircle => "icons/loader-circle.svg",
            Icon::Circle => "icons/circle.svg",
            Icon::CircleStop => "icons/circle-stop.svg",
            Icon::CircleAlert => "icons/circle-alert.svg",
            Icon::CircleCheck => "icons/circle-check.svg",
            Icon::Clock => "icons/clock.svg",
            Icon::Ellipsis => "icons/ellipsis.svg",
            Icon::Download => "icons/download.svg",
            Icon::Eye => "icons/eye.svg",
            Icon::EyeOff => "icons/eye-off.svg",
            Icon::GripVertical => "icons/grip-vertical.svg",
            Icon::Info => "icons/info.svg",
            Icon::LayoutGrid => "icons/layout-grid.svg",
            Icon::MapPin => "icons/map-pin.svg",
            Icon::Minus => "icons/minus.svg",
            Icon::Pin => "icons/pin.svg",
            Icon::Plus => "icons/plus.svg",
            Icon::RefreshCw => "icons/refresh-cw.svg",
            Icon::RotateCcw => "icons/rotate-ccw.svg",
            Icon::RotateCw => "icons/rotate-cw.svg",
            Icon::Search => "icons/search.svg",
            Icon::Server => "icons/server.svg",
            Icon::Sidebar => "icons/sidebar.svg",
            Icon::SidebarOpen => "icons/sidebar-open.svg",
            Icon::Square => "icons/square.svg",
            Icon::TriangleAlert => "icons/triangle-alert.svg",
            Icon::User => "icons/user.svg",
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

pub fn small_icon(icon: Icon, color: Hsla) -> Svg {
    self::icon(icon, color).size(px(10.0))
}

impl RenderOnce for Icon {
    fn render(self, window: &mut Window, _: &mut App) -> impl IntoElement {
        icon(self, window.text_style().color)
    }
}
