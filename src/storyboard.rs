mod canvas;
mod game;
mod library;
mod tools;
mod view;

use gpui::{AnyView, App, AppContext as _, Pixels, Size, px, size};

use view::{Navigation, Sessions};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Screen {
    Game,
    Accounts,
    Scripts,
    Profiles,
    Environment,
    Follower,
    Loader,
    Packets,
    Settings,
}

#[derive(Clone, Copy)]
struct Frame {
    screen: Screen,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

const FRAMES: [Frame; 9] = [
    Frame {
        screen: Screen::Game,
        x: 0.0,
        y: 0.0,
        width: 960.0,
        height: 578.0,
    },
    Frame {
        screen: Screen::Accounts,
        x: 1040.0,
        y: 0.0,
        width: 920.0,
        height: 464.0,
    },
    Frame {
        screen: Screen::Loader,
        x: 2120.0,
        y: 0.0,
        width: 880.0,
        height: 504.0,
    },
    Frame {
        screen: Screen::Scripts,
        x: 0.0,
        y: 670.0,
        width: 860.0,
        height: 508.0,
    },
    Frame {
        screen: Screen::Profiles,
        x: 1040.0,
        y: 670.0,
        width: 1000.0,
        height: 608.0,
    },
    Frame {
        screen: Screen::Packets,
        x: 2120.0,
        y: 670.0,
        width: 900.0,
        height: 512.0,
    },
    Frame {
        screen: Screen::Environment,
        x: 0.0,
        y: 1370.0,
        width: 760.0,
        height: 600.0,
    },
    Frame {
        screen: Screen::Follower,
        x: 1040.0,
        y: 1370.0,
        width: 640.0,
        height: 340.0,
    },
    Frame {
        screen: Screen::Settings,
        x: 2120.0,
        y: 1370.0,
        width: 660.0,
        height: 384.0,
    },
];

impl Frame {
    fn size(self) -> Size<Pixels> {
        size(px(self.width), px(self.height))
    }

    fn build(self, nav: Navigation, sessions: gpui::Entity<Sessions>, cx: &mut App) -> AnyView {
        match self.screen {
            Screen::Game => cx.new(|cx| game::Game::new(nav, sessions, cx)).into(),
            Screen::Accounts => cx.new(|cx| library::Accounts::new(sessions, cx)).into(),
            Screen::Scripts => cx.new(|cx| library::Scripts::new(sessions, cx)).into(),
            Screen::Profiles => cx.new(library::Profiles::new).into(),
            Screen::Environment => cx.new(|cx| tools::Environment::new(sessions, cx)).into(),
            Screen::Follower => cx.new(|cx| tools::Follower::new(sessions, cx)).into(),
            Screen::Loader => cx.new(|cx| tools::Loader::new(sessions, cx)).into(),
            Screen::Packets => cx.new(|cx| tools::Packets::new(sessions, cx)).into(),
            Screen::Settings => cx.new(tools::Settings::new).into(),
        }
    }
}

impl Screen {
    fn name(self) -> &'static str {
        match self {
            Self::Game => "Mira",
            Self::Accounts => "Accounts",
            Self::Scripts => "Scripts",
            Self::Profiles => "Combat profiles",
            Self::Environment => "Environment",
            Self::Follower => "Follower",
            Self::Loader => "Loader/grabber",
            Self::Packets => "Packets",
            Self::Settings => "Settings",
        }
    }

    fn frame(self) -> Frame {
        *FRAMES
            .iter()
            .find(|frame| frame.screen == self)
            .expect("screen has a frame")
    }
}

#[cfg(feature = "storyboard")]
pub(crate) use canvas::run;
