use gpui::{
    AppContext as _, Context, IntoElement, ParentElement as _, Render, Styled as _, Window, div,
    px, size,
};
use mira_ui::{
    ActiveTheme as _, Tone,
    components::StatusLabel,
    primitives::{
        Activity, Alert, Badge, Button, Dialog, Dot, DropdownMenu, Icon, List, ListItem, MenuItem,
        Popover, Select, SelectOption, Switch, Tab, Tabs, ToggleGroup, ToggleItem,
        button::{Size, Variant},
    },
    snapshot,
    theme::{SYSTEM_SANS, TEXT_BODY},
};

struct Scene(fn(&mut Window, &mut Context<Scene>) -> gpui::AnyElement);

impl Render for Scene {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        div()
            .size_full()
            .p(px(16.0))
            .bg(theme.base)
            .text_color(theme.text)
            .font_family(SYSTEM_SANS)
            .text_size(TEXT_BODY)
            .child((self.0)(window, cx))
    }
}

fn scene(
    name: &str,
    width: f32,
    height: f32,
    render: fn(&mut Window, &mut Context<Scene>) -> gpui::AnyElement,
) {
    for path in snapshot(name, size(px(width), px(height)), move |_, cx| {
        cx.new(|_| Scene(render))
    }) {
        assert!(path.exists(), "{} was not written", path.display());
    }
}

#[test]
fn snapshot_controls() {
    scene("controls", 620.0, 300.0, |_, _| {
        div()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(Tabs::new("tabs", "Sections", 3, |index| {
                let tab = Tab::new(["General", "Combat", "Scripts"][index]);
                if index == 2 {
                    tab.end_slot(Badge::new("3"))
                } else {
                    tab
                }
            }))
            .child(
                div()
                    .flex()
                    .gap(px(8.0))
                    .child(Button::new("run", "Run").variant(Variant::Primary))
                    .child(Button::new("queue", "Add to queue"))
                    .child(Button::new("options", "Options").variant(Variant::Ghost))
                    .child(Button::new("trash", "Move to Trash").variant(Variant::Danger)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.0))
                    .child(ToggleGroup::new(
                        "view",
                        "View",
                        [
                            ToggleItem::new("Library"),
                            ToggleItem::new("Queue").end_slot(Badge::new("2")),
                        ],
                        0,
                    ))
                    .child(Select::new(
                        "room",
                        "Room",
                        vec!["Ledgermayne", "Moreskulls"],
                        Some(0),
                    ))
                    .child(Badge::new("Running").tone(Tone::Accent))
                    .child(StatusLabel::new(
                        "status",
                        Activity::Running,
                        "Running daily.ts",
                    )),
            )
            .child(
                Switch::new("relogin", "Auto relogin", true)
                    .description("Log back in after a disconnect"),
            )
            .child(
                Alert::new("warn", Tone::Warning, "Scripts folder can't be read")
                    .body("Locate it or remove it from the library."),
            )
            .into_any_element()
    });
}

#[test]
fn snapshot_dialog() {
    scene("dialog", 560.0, 420.0, |_, _| {
        Dialog::new("edit", true, "Edit account")
            .description("Changes apply the next time this account logs in.")
            .body(
                List::new("scripts", "Scripts", 5, |index| match index {
                    0 => ListItem::header("Mine"),
                    1 => ListItem::new("daily.ts").end_slot("2m ago"),
                    2 => ListItem::new("army.ts")
                        .description("~/Documents/Mira/scripts")
                        .end_slot("1h ago"),
                    3 => ListItem::header("aqw-community"),
                    _ => ListItem::new("hunt.ts")
                        .start_slot(Dot::new(Tone::Warning))
                        .end_slot(Button::icon("remove", Icon::X, "Remove").size(Size::XSmall)),
                })
                .selected(Some(1)),
            )
            .action(Button::new("cancel", "Cancel"))
            .action(Button::new("save", "Save").variant(Variant::Primary))
            .into_any_element()
    });
}

#[test]
fn snapshot_menu() {
    scene("menu", 520.0, 360.0, |_, _| {
        div()
            .flex()
            .gap(px(12.0))
            .child(
                DropdownMenu::new("sources")
                    .default_open(true)
                    .trigger(
                        Button::new("sources", "Sources")
                            .start_slot(Dot::new(Tone::Danger))
                            .end_slot(Icon::ChevronDown),
                    )
                    .items(|_, _| {
                        vec![
                            MenuItem::switch("Auto zone", true, |_, _| {}),
                            MenuItem::separator(),
                            MenuItem::header("Map"),
                            MenuItem::radio("Ledgermayne", true, |_, _| {}).shortcut("⌘1"),
                            MenuItem::radio("Moreskulls", false, |_, _| {}).shortcut("⌘2"),
                            MenuItem::radio("Darkcarnax", false, |_, _| {}).end_slot("Event"),
                            MenuItem::separator(),
                            MenuItem::action("aqw-community", |_, _| {})
                                .start_slot(Dot::new(Tone::Danger))
                                .description("Can't read this folder"),
                            MenuItem::submenu(
                                "Packages",
                                vec![MenuItem::action("Install package…", |_, _| {})],
                            ),
                            MenuItem::action("Move to Trash", |_, _| {}).danger(),
                        ]
                    }),
            )
            .child(
                Select::new(
                    "room",
                    "Room",
                    vec![
                        SelectOption::new("Ledgermayne").description("Default room"),
                        SelectOption::new("Moreskulls"),
                    ],
                    Some(0),
                )
                .width(px(180.0)),
            )
            .into_any_element()
    });
}

#[test]
fn snapshot_popover() {
    scene("popover", 420.0, 260.0, |_, _| {
        div()
            .flex()
            .child(
                Popover::new("options")
                    .default_open(true)
                    .trigger(Button::new("options", "Options").end_slot(Icon::ChevronDown))
                    .content(|_, _, _| {
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(8.0))
                            .child(Switch::new("safe", "Safe start", true))
                            .child(Switch::new("restart", "Restart on failure", false))
                            .into_any_element()
                    }),
            )
            .into_any_element()
    });
}

#[test]
fn snapshot_tabs() {
    scene("tabs", 460.0, 260.0, |_, _| {
        div()
            .flex()
            .gap(px(24.0))
            .child(
                div().w(px(200.0)).child(
                    Tabs::new("sessions", "Tabs", 4, |index| {
                        let (name, status) = [
                            ("Main", "Running daily.ts"),
                            ("Alt 1", "Relogin in 12s"),
                            ("Alt 2", "Idle"),
                            ("Alt 3", "Disconnected"),
                        ][index];
                        let tab = Tab::new(name).description(status);
                        if index == 2 {
                            tab.end_slot(mira_ui::primitives::Kbd::new("⌘3"))
                        } else {
                            tab.on_close(|_, _| {})
                        }
                    })
                    .vertical()
                    .selected(0),
                ),
            )
            .child(
                div()
                    .flex_1()
                    .child(Tabs::new("pages", "Pages", 2, |index| {
                        let tab = Tab::new(["Log", "Spammer"][index]);
                        if index == 1 {
                            tab.end_slot(Badge::new("3"))
                        } else {
                            tab
                        }
                    })),
            )
            .into_any_element()
    });
}

struct Form {
    walk: gpui::Entity<mira_ui::primitives::Input>,
}

impl Render for Form {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use mira_ui::{
            components::{Empty, FieldRow, Group},
            primitives::Field,
        };
        let theme = cx.theme().clone();
        div()
            .size_full()
            .p(px(16.0))
            .bg(theme.base)
            .text_color(theme.text)
            .font_family(SYSTEM_SANS)
            .text_size(TEXT_BODY)
            .flex()
            .gap(px(16.0))
            .child(
                div()
                    .w(px(300.0))
                    .flex()
                    .flex_col()
                    .gap(px(12.0))
                    .child(
                        Group::new()
                            .title("Gameplay")
                            .child(
                                FieldRow::new("Auto relogin")
                                    .description("Log back in after a disconnect")
                                    .child(
                                        Switch::new("relogin", "Auto relogin", true).hide_label(),
                                    ),
                            )
                            .child(FieldRow::new("Collisions").child(
                                Switch::new("collisions", "Collisions", false).hide_label(),
                            )),
                    )
                    .child(
                        Field::new("Walk speed", self.walk.clone())
                            .description("Default is 8")
                            .end_slot(
                                Button::icon("reset", Icon::RotateCcw, "Reset walk speed")
                                    .size(Size::XSmall),
                            ),
                    ),
            )
            .child(
                div().flex_1().child(
                    Empty::new("No scripts yet", "Add a folder or install a package.")
                        .compact()
                        .action(Button::new("show", "Show in Finder"))
                        .action(Button::new("add", "Add folder…").variant(Variant::Primary)),
                ),
            )
    }
}

#[test]
fn snapshot_form() {
    use mira_ui::primitives::{Input, Numeric};
    for path in snapshot("form", size(px(620.0), px(260.0)), |_, cx| {
        let walk = cx.new(|cx| {
            let mut input = Input::new("Walk speed", cx)
                .numeric(Numeric {
                    min: 0,
                    max: 99,
                    step: 1,
                })
                .suffix("px/s")
                .small();
            input.set_text("12", cx);
            input
        });
        cx.new(|_| Form { walk })
    }) {
        assert!(path.exists());
    }
}
