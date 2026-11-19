use gpui::{
    App, AppContext as _, Context, Entity, IntoElement, Keystroke, ParentElement as _, Render,
    Styled as _, Window, WindowAppearance, div, prelude::FluentBuilder as _, px,
};
use mira_ui::{
    ActiveTheme as _, Theme,
    components::{FieldRow, Group, ShortcutField},
    h_flex,
    primitives::{Input, Select, Switch, Tab, Tabs, button::Variant},
};

use super::super::{Screen, view::*};

const SHORTCUTS: [(&str, &str); 4] = [
    ("Load script", "cmd-o"),
    ("Start or stop script", "cmd-r"),
    ("Open Accounts", "cmd-shift-a"),
    ("Open Scripts", "cmd-shift-s"),
];

pub(in crate::storyboard) struct Settings {
    page: usize,
    start: usize,
    update_checked: bool,
    system: bool,
    pointer: bool,
    token: Entity<Input>,
    shortcuts: [Option<Keystroke>; SHORTCUTS.len()],
    conflict: Option<usize>,
}

fn system_dark(cx: &App) -> bool {
    matches!(
        cx.window_appearance(),
        WindowAppearance::Dark | WindowAppearance::VibrantDark
    )
}

impl Settings {
    pub(in crate::storyboard) fn new(cx: &mut Context<Self>) -> Self {
        let token = cx.new(|cx| {
            Input::new("GitHub token", cx)
                .placeholder("No token saved")
                .password()
        });
        Self {
            page: 0,
            start: 0,
            update_checked: false,
            system: false,
            pointer: true,
            token,
            shortcuts: SHORTCUTS.map(|(_, binding)| Keystroke::parse(binding).ok()),
            conflict: None,
        }
    }

    #[cfg(test)]
    pub(in crate::storyboard) fn preview_shortcuts(mut self) -> Self {
        self.page = 3;
        self
    }

    #[cfg(test)]
    pub(in crate::storyboard) fn preview_updates(mut self) -> Self {
        self.page = 2;
        self
    }

    fn general(&self, cx: &Context<Self>) -> Group {
        Group::new().title("Startup").row(
            "Open at launch",
            None,
            Select::new(
                "start",
                "Open at launch",
                ["Game", "Accounts"],
                Some(self.start),
            )
            .width(px(148.0))
            .on_select(choose(cx, |view, start, _| view.start = start)),
        )
    }

    fn updates(&self, cx: &Context<Self>) -> gpui::Div {
        let preview = StatusPreview::current(cx);
        let message = status_preview(
            "Checking for updates…",
            "Couldn't check for updates",
            "GitHub is unreachable. Try again in a moment.",
            cx,
        )
        .or_else(|| {
            self.update_checked
                .then(|| status("Mira is up to date", true, cx))
        });
        mira_ui::v_flex()
            .gap(px(16.0))
            .child(
                Group::new()
                    .title("App updates")
                    .row(
                        concat!("Installed version ", env!("CARGO_PKG_VERSION")),
                        None,
                        command(
                            "check-updates",
                            "Check for updates",
                            Variant::Outline,
                            change(cx, |view, _| view.update_checked = true),
                        )
                        .disabled(preview == StatusPreview::Working),
                    )
                    .children(message.map(|row| row.px(px(12.0)).py(px(8.0)))),
            )
            .child(Group::new().title("Script packages").row(
                "GitHub access token",
                Some("Used for private package repositories.".into()),
                div().w(px(216.0)).child(self.token.clone()),
            ))
    }
}

impl Render for Settings {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let dark = cx.theme().base.l < 0.5;
        let theme_choice = if self.system && dark == system_dark(cx) {
            0
        } else if dark {
            2
        } else {
            1
        };
        let theme = |choice: usize, window: &mut Window, cx: &mut App| {
            let _ = window;
            let mut next = Theme::mira(if choice == 0 {
                system_dark(cx)
            } else {
                choice == 2
            });
            next.pointer_cursor = cx.theme().pointer_cursor;
            cx.set_global(next);
            cx.refresh_windows();
        };
        let owner = cx.weak_entity();
        let appearance = Group::new()
            .row(
                "Theme",
                None,
                Select::new(
                    "appearance",
                    "Theme",
                    ["System", "Light", "Dark"],
                    Some(theme_choice),
                )
                .width(px(148.0))
                .on_select(move |choice, window, cx| {
                    owner
                        .update(cx, |view, cx| {
                            view.system = choice == 0;
                            cx.notify();
                        })
                        .ok();
                    theme(choice, window, cx);
                }),
            )
            .row(
                "Pointer cursors",
                Some("Show a hand over clickable controls.".into()),
                Switch::new("pointer", "Pointer cursors", self.pointer)
                    .hide_label()
                    .on_toggle(choose(cx, |view, on, cx| {
                        view.pointer = on;
                        let mut theme = cx.theme().clone();
                        theme.pointer_cursor = on;
                        cx.set_global(theme);
                        cx.refresh_windows();
                    })),
            );
        let shortcuts = Group::new().children(SHORTCUTS.into_iter().enumerate().map(
            |(index, (label, binding))| {
                let value = &self.shortcuts[index];
                let default = Keystroke::parse(binding).ok();
                let changed = value != &default;
                FieldRow::new(label)
                    .when(self.conflict == Some(index), |row| {
                        row.description("That shortcut is already used.")
                    })
                    .child(
                        h_flex()
                            .gap(px(8.0))
                            .when(changed, |group| {
                                group.child(command(
                                    ("reset", index),
                                    "Reset",
                                    Variant::Ghost,
                                    change(cx, move |view, _| {
                                        view.shortcuts[index] = Keystroke::parse(binding).ok();
                                        view.conflict = None;
                                    }),
                                ))
                            })
                            .child(
                                ShortcutField::new(("shortcut", index), label, value.clone())
                                    .on_change(choose(
                                        cx,
                                        move |view, value: Option<Keystroke>, _| {
                                            if value.is_some()
                                                && view.shortcuts.iter().enumerate().any(
                                                    |(other, existing)| {
                                                        other != index && existing == &value
                                                    },
                                                )
                                            {
                                                view.conflict = Some(index);
                                            } else {
                                                view.shortcuts[index] = value;
                                                view.conflict = None;
                                            }
                                        },
                                    )),
                            ),
                    )
            },
        ));
        let pages = ["General", "Appearance", "Updates", "Shortcuts"];
        let panel = match self.page {
            1 => appearance.into_any_element(),
            2 => self.updates(cx).into_any_element(),
            3 => shortcuts.into_any_element(),
            _ => self.general(cx).into_any_element(),
        };
        frame(tool_chrome(Screen::Settings, None, cx), cx)
            .child(
                compact_toolbar().child(
                    Tabs::new("settings-pages", "Settings", pages.len(), move |index| {
                        Tab::new(pages[index])
                    })
                    .selected(self.page)
                    .on_select(choose(cx, |view, page, _| view.page = page)),
                ),
            )
            .child(scroll("settings-body").p(px(16.0)).child(panel))
    }
}
