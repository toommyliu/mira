use gpui::{App, Global, Menu, MenuItem, Window, actions};
use mira_ui::Theme;

actions!(
    appearance,
    [UseSystemAppearance, UseLightAppearance, UseDarkAppearance]
);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Appearance {
    System,
    Light,
    Dark,
}

impl Global for Appearance {}

pub(crate) fn init(cx: &mut App, on_change: fn(&mut App)) {
    cx.set_global(Appearance::System);
    cx.on_action(move |_: &UseSystemAppearance, cx| choose(Appearance::System, on_change, cx));
    cx.on_action(move |_: &UseLightAppearance, cx| choose(Appearance::Light, on_change, cx));
    cx.on_action(move |_: &UseDarkAppearance, cx| choose(Appearance::Dark, on_change, cx));
}

pub(crate) fn follow_system(window: &mut Window) {
    window
        .observe_window_appearance(|_, cx| {
            if *cx.global::<Appearance>() == Appearance::System {
                apply(cx);
            }
        })
        .detach();
}

pub(crate) fn menu(cx: &App) -> MenuItem {
    let chosen = *cx.global::<Appearance>();
    MenuItem::submenu(Menu {
        name: "Appearance".into(),
        disabled: false,
        items: vec![
            MenuItem::action("System", UseSystemAppearance).checked(chosen == Appearance::System),
            MenuItem::action("Light", UseLightAppearance).checked(chosen == Appearance::Light),
            MenuItem::action("Dark", UseDarkAppearance).checked(chosen == Appearance::Dark),
        ],
    })
}

fn choose(appearance: Appearance, on_change: fn(&mut App), cx: &mut App) {
    cx.set_global(appearance);
    apply(cx);
    on_change(cx);
}

fn apply(cx: &mut App) {
    let theme = match *cx.global::<Appearance>() {
        Appearance::System => Theme::for_window(cx.window_appearance()),
        Appearance::Light => Theme::light(),
        Appearance::Dark => Theme::dark(),
    };
    cx.set_global(theme);
    cx.refresh_windows();
}
