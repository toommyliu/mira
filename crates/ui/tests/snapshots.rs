use gpui::{
    AppContext as _, Context, InteractiveElement as _, IntoElement, ParentElement as _, Render,
    Styled as _, Window, div, point, px, size,
};
use mira_ui::{
    ActiveTheme as _, FOCUS_SCOPE, Tone,
    components::StatusLabel,
    primitives::{
        Activity, Alert, Badge, Button, Checkbox, CheckedState, Dialog, Dot, DropdownMenu, Icon,
        List, ListItem, MenuItem, Menubar, Popover, Select, SelectOption, Switch, Tab, Tabs,
        ToggleGroup, ToggleItem,
        button::{Joined, Size, Variant},
    },
    snapshot,
    theme::UI_FONT,
};

type PaintScene = Box<dyn Fn(&mut Window, &mut Context<Scene>) -> gpui::AnyElement>;

struct Scene(PaintScene, gpui::FocusHandle);

impl Scene {
    fn open(
        render: impl Fn(&mut Window, &mut Context<Scene>) -> gpui::AnyElement + 'static,
        window: &mut Window,
        cx: &mut gpui::App,
    ) -> gpui::Entity<Scene> {
        let scene = cx.new(|cx| Scene(Box::new(render), cx.focus_handle()));
        let focus = scene.read(cx).1.clone();
        window.focus(&focus, cx);
        scene
    }
}

impl Render for Scene {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let layout = theme.layout;
        let typography = theme.typography;
        div()
            .size_full()
            .key_context(FOCUS_SCOPE)
            .track_focus(&self.1)
            .p(layout.space(4.0))
            .bg(theme.background)
            .text_color(theme.foreground)
            .font_family(UI_FONT)
            .text_size(typography.body)
            .child((self.0)(window, cx))
    }
}

fn scene(
    name: &str,
    width: f32,
    height: f32,
    render: fn(&mut Window, &mut Context<Scene>) -> gpui::AnyElement,
) {
    for path in snapshot(name, size(px(width), px(height)), move |window, cx| {
        Scene::open(render, window, cx)
    }) {
        assert!(path.exists(), "{} was not written", path.display());
    }
}

fn scene_after_keys(
    name: &str,
    width: f32,
    height: f32,
    keys: &[&'static str],
    render: fn(&mut Window, &mut Context<Scene>) -> gpui::AnyElement,
) {
    for path in mira_ui::snapshot_after(
        name,
        size(px(width), px(height)),
        keys,
        move |window, cx| Scene::open(render, window, cx),
    ) {
        assert!(path.exists(), "{} was not written", path.display());
    }
}

fn hover(cx: &mut gpui::HeadlessAppContext, owner: gpui::AnyWindowHandle, x: f32, y: f32) {
    use gpui::{MouseMoveEvent, PlatformInput, point};

    for nudge in [0.0, 1.0] {
        cx.update_window(owner, |_, window, cx| {
            window.dispatch_event(
                PlatformInput::MouseMove(MouseMoveEvent {
                    position: point(px(x + nudge), px(y)),
                    ..Default::default()
                }),
                cx,
            );
            window.refresh();
        })
        .unwrap();
        cx.run_until_parked();
        cx.capture_screenshot(owner).unwrap();
    }
}

fn assert_centered_checkbox_marks(
    cx: &mut gpui::HeadlessAppContext,
    owner: gpui::AnyWindowHandle,
    scale: u32,
) {
    cx.update_window(owner, |_, window, cx| {
        window.set_scale_factor(scale as f32);
        window.draw(cx).clear(cx);
    })
    .unwrap();
    let image = cx.capture_screenshot(owner).unwrap();
    let last = point(16 * scale - 1, 16 * scale - 1);
    for (name, top) in [("check", 22), ("minus", 50)] {
        let fill = image.get_pixel(18 * scale, (top + 8) * scale)[0];
        let mut first_ink = last;
        let mut last_ink = point(0, 0);
        for y in 2 * scale..14 * scale {
            for x in 2 * scale..14 * scale {
                let pixel = image.get_pixel(16 * scale + x, top * scale + y)[0];
                if pixel.abs_diff(fill) > 64 {
                    first_ink = first_ink.min(&point(x, y));
                    last_ink = last_ink.max(&point(x, y));
                }
            }
        }
        assert!(first_ink.x <= last_ink.x, "missing {name} at {scale}×");
        assert_eq!(first_ink, last - last_ink, "{name} margins at {scale}×");
    }
}

#[test]
fn checkbox_centers_its_marks_and_keeps_its_solid_rounded_edge_on_hover() {
    mira_ui::snapshot_with(
        "checkbox-hover",
        size(px(160.0), px(104.0)),
        |window, cx| {
            Scene::open(
                |_, _| {
                    div()
                        .child(
                            Checkbox::new("rewards", "Rewards", CheckedState::Checked)
                                .on_toggle(|_, _, _| {}),
                        )
                        .child(Checkbox::new("mixed", "Mixed", CheckedState::Indeterminate))
                        .child(Checkbox::new("off", "Unchecked", CheckedState::Unchecked))
                        .into_any_element()
                },
                window,
                cx,
            )
        },
        |cx, owner| {
            hover(cx, owner, 24.0, 30.0);
            for scale in [1, 2] {
                assert_centered_checkbox_marks(cx, owner, scale);
            }
            let image = cx.capture_screenshot(owner).unwrap();
            let scale = image.width() / 160;
            let pixel = |x, y| image.get_pixel(x * scale, y * scale);
            let fill = pixel(19, 27);
            assert_ne!(fill, pixel(12, 27));
            assert_eq!(pixel(16, 27), fill, "checked edge and fill");
            let background = f32::from(pixel(12, 27)[0]);
            let contrast = f32::from(fill[0]) - background;
            let radius = 4;
            let mut cutout = 0.0;
            for y in 0..radius * scale {
                for x in 0..radius * scale {
                    let ink = image.get_pixel(16 * scale + x, 22 * scale + y)[0];
                    cutout += 1.0 - (f32::from(ink) - background) / contrast;
                }
            }
            let expected = (radius * radius) as f32 * (1.0 - std::f32::consts::FRAC_PI_4);
            let area = cutout / (scale * scale) as f32;
            assert!(
                (area - expected).abs() < 0.3,
                "4 px rounded corner, got {area} px²"
            );
        },
    );
}

#[test]
fn hovered_switch_has_no_edge_line() {
    for (name, on, edge, inside) in [
        ("switch-hover-on", true, 16, 22),
        ("switch-hover-off", false, 43, 40),
    ] {
        mira_ui::snapshot_with(
            name,
            size(px(160.0), px(52.0)),
            move |window, cx| {
                Scene::open(
                    move |_, _| {
                        Switch::new("relogin", "Auto relogin", on)
                            .on_toggle(|_, _, _| {})
                            .into_any_element()
                    },
                    window,
                    cx,
                )
            },
            |cx, owner| {
                hover(cx, owner, 30.0, 30.0);
                let image = cx.capture_screenshot(owner).unwrap();
                let scale = image.width() / 160;
                let border = image.get_pixel(edge * scale + scale / 2 * u32::from(on), 30 * scale);
                let fill = image.get_pixel(inside * scale, 30 * scale);
                assert_eq!(border, fill, "{name} edge and fill");
            },
        );
    }
}

#[test]
fn snapshot_controls() {
    scene("controls", 620.0, 340.0, |_, _| {
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
                    .gap(px(8.0))
                    .child(
                        div()
                            .flex()
                            .child(
                                Button::new("load", "Load")
                                    .variant(Variant::Primary)
                                    .joined(Joined::Next),
                            )
                            .child(
                                Button::icon("load-more", Icon::ChevronDown, "Other loads")
                                    .variant(Variant::Primary)
                                    .joined(Joined::Previous),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .child(
                                Button::new("load-off", "Load")
                                    .variant(Variant::Primary)
                                    .disabled(true)
                                    .joined(Joined::Next),
                            )
                            .child(
                                Button::icon("load-off-more", Icon::ChevronDown, "Other loads")
                                    .variant(Variant::Primary)
                                    .joined(Joined::Previous),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .child(Button::new("export", "Export").joined(Joined::Next))
                            .child(
                                Button::icon("export-more", Icon::ChevronDown, "Export options")
                                    .variant(Variant::Secondary)
                                    .joined(Joined::Previous),
                            ),
                    ),
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
fn snapshot_button_pointer_states() {
    use gpui::{MouseButton, MouseDownEvent, MouseMoveEvent, PlatformInput, point};

    for (name, held) in [("button-hover", false), ("button-held", true)] {
        let paths = mira_ui::snapshot_with(
            name,
            size(px(400.0), px(100.0)),
            |window, cx| {
                Scene::open(
                    |_, cx| {
                        mira_ui::h_flex()
                            .gap(cx.theme().space(2.0))
                            .child(Button::new("edit", "Edit").tooltip("Edit profile"))
                            .child(Button::new("selected", "Selected").pressed(true))
                            .child(Button::new("save", "Save").variant(Variant::Primary))
                            .child(Button::new("disabled", "Disabled").disabled(true))
                            .into_any_element()
                    },
                    window,
                    cx,
                )
            },
            |cx, owner| {
                let position = point(px(32.0), px(29.0));
                cx.update_window(owner, |_, window, cx| {
                    window.dispatch_event(
                        PlatformInput::MouseMove(MouseMoveEvent {
                            position,
                            pressed_button: None,
                            modifiers: Default::default(),
                        }),
                        cx,
                    );
                })
                .expect("hover the first button");
                cx.run_until_parked();
                if held {
                    cx.update_window(owner, |_, window, cx| {
                        window.dispatch_event(
                            PlatformInput::MouseDown(MouseDownEvent {
                                position,
                                button: MouseButton::Left,
                                click_count: 1,
                                first_mouse: false,
                                modifiers: Default::default(),
                            }),
                            cx,
                        );
                    })
                    .expect("press the first button");
                }
            },
        );
        assert!(paths.iter().all(|path| path.exists()));
    }
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
fn snapshot_dialog_small_window() {
    scene("dialog-small-window", 340.0, 280.0, |_, _| {
        Dialog::new("boosts", true, "Add boosts you own")
            .alert()
            .description("Adding one from your bank moves it to your inventory.")
            .body(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .children((0..12usize).map(|index| {
                        Checkbox::new(
                            ("boost", index),
                            format!("Class Experience Boost {}", index + 1),
                            if index < 3 {
                                CheckedState::Checked
                            } else {
                                CheckedState::Unchecked
                            },
                        )
                    })),
            )
            .action(Button::new("cancel", "Cancel"))
            .action(Button::new("add", "Add 3 boosts").variant(Variant::Primary))
            .into_any_element()
    });
}

#[test]
fn snapshot_dialog_initial_focus() {
    scene_after_keys(
        "dialog-initial-focus",
        520.0,
        300.0,
        &["f19"],
        |window, cx| {
            let field = |label: &'static str, window: &mut Window, cx: &mut Context<Scene>| {
                let input = window.use_keyed_state(label, cx, |_, cx| {
                    mira_ui::primitives::Input::new(label, cx)
                });
                mira_ui::primitives::Field::new(label, input)
            };
            Dialog::new("add-account", true, "Add account")
                .body(field("Username", window, cx))
                .body(field("Password", window, cx))
                .action(Button::new("cancel", "Cancel"))
                .action(Button::new("save", "Save").variant(Variant::Primary))
                .on_close(|_, _| {})
                .into_any_element()
        },
    );
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
fn snapshot_select_short_window() {
    scene_after_keys(
        "select-short-window",
        340.0,
        240.0,
        &["tab", "enter"],
        |_, _| {
            Select::new(
                "profile",
                "Profile",
                (1..=20)
                    .map(|index| SelectOption::new(format!("Ultra boss {index}")))
                    .collect::<Vec<_>>(),
                Some(16),
            )
            .width(px(200.0))
            .into_any_element()
        },
    );
}

#[test]
fn snapshot_long_notices() {
    for path in snapshot("long-notices", size(px(420.0), px(300.0)), |window, cx| {
        let stack = mira_ui::primitives::Toaster::new(window, cx);
        mira_ui::primitives::Toaster::push(
            &stack,
            mira_ui::primitives::Toast::new("Couldn't load profile")
                .body(
                    "Couldn't read ~/Documents/Mira/profiles/void-highlord.yaml: permission denied",
                )
                .tone(Tone::Danger),
            cx,
        );
        Scene::open(
            move |window, cx| {
                div()
                    .size_full()
                    .child(
                        Alert::new("save", Tone::Danger, "Couldn't save the setups")
                            .body("Couldn't write ~/Library/Application Support/Mira/setups.json: the disk is full")
                            .action(Button::new("retry", "Retry").size(Size::Small)),
                    )
                    .child(mira_ui::primitives::Toaster::layer(&stack,window, cx))
                    .into_any_element()
            },
            window,
            cx,
        )
    }) {
        assert!(path.exists(), "{} was not written", path.display());
    }
}

#[test]
fn snapshot_popover_tab() {
    scene_after_keys(
        "popover-tab",
        420.0,
        260.0,
        &["tab", "enter", "tab", "tab"],
        |_, _| {
            div()
                .flex()
                .gap(px(8.0))
                .child(
                    Popover::new("options")
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
                .child(Button::new("run", "Run").variant(Variant::Primary))
                .into_any_element()
        },
    );
}

#[test]
fn snapshot_sources_short_window() {
    scene_after_keys(
        "sources-short-window",
        420.0,
        300.0,
        &["tab", "enter", "end", "up", "right"],
        |_, _| {
            DropdownMenu::new("sources")
                .trigger(Button::new("sources", "Sources").end_slot(Icon::ChevronDown))
                .items(|_, _| {
                    let folder = |path: &str| {
                        MenuItem::submenu(
                            path.to_owned(),
                            vec![
                                MenuItem::action("Show in Finder", |_, _| {}),
                                MenuItem::action("Remove from library", |_, _| {}),
                            ],
                        )
                    };
                    let mut items = vec![MenuItem::header("Folders")];
                    items.extend(
                        [
                            "scripts",
                            "aqw-community",
                            "legion",
                            "dailies",
                            "bosses",
                            "merge",
                        ]
                        .map(|name| folder(&format!("~/Documents/Mira/{name}"))),
                    );
                    items.push(MenuItem::action("Add folder…", |_, _| {}));
                    items.push(MenuItem::separator());
                    items.push(MenuItem::header("Packages"));
                    items.extend(
                        [("@lucent/dailies", "0.0.3"), ("@lucent/ultras", "1.2.0")]
                            .map(|(name, version)| folder(name).end_slot(Badge::new(version))),
                    );
                    items.push(MenuItem::action("Install package…", |_, _| {}));
                    items
                })
                .into_any_element()
        },
    );
}

#[test]
fn snapshot_alert_tones() {
    scene("alert-tones", 460.0, 380.0, |_, _| {
        div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(
                Alert::new("saved", Tone::Success, "Setup saved")
                    .body("Every account in Dailies logs in with its own server."),
            )
            .child(Alert::new(
                "tip",
                Tone::Neutral,
                "Scripts reload when you save them",
            ))
            .child(
                Alert::new("slow", Tone::Warning, "Scripts folder can't be read")
                    .body("Locate it or remove it from the library.")
                    .action(Button::new("locate", "Locate…").size(Size::Small)),
            )
            .child(
                Alert::new("failed", Tone::Danger, "Couldn't save the setups")
                    .body("The disk is full."),
            )
            .child(
                Alert::new("bell", Tone::Accent, "Drop alert on")
                    .icon(Icon::Bell)
                    .body("Beeps when a listed drop appears."),
            )
            .into_any_element()
    });
}

#[test]
fn snapshot_menubar() {
    scene("menubar", 560.0, 300.0, |_, cx| {
        let theme = cx.theme().clone();
        let trigger = |id: &'static str, title: &'static str| {
            Button::new(id, title)
                .variant(Variant::Ghost)
                .size(Size::Small)
        };
        Menubar::new("bar", "Game")
            .h(px(38.0))
            .gap(px(2.0))
            .px(px(6.0))
            .border_b_1()
            .border_color(theme.border)
            .menu(trigger("windows", "Windows"), |_, _| {
                vec![MenuItem::action("Packets", |_, _| {})]
            })
            .menu(trigger("scripts", "Scripts"), |_, _| {
                vec![
                    MenuItem::header("Running daily.ts"),
                    MenuItem::action("Stop", |_, _| {}),
                    MenuItem::action("Run script…", |_, _| {}),
                ]
            })
            .child(div().flex_1())
            .menu(
                trigger("attack", "Auto attack")
                    .child("Auto attack")
                    .child(div().text_color(theme.muted_foreground).child("Generic")),
                |_, _| {
                    vec![
                        MenuItem::switch("Auto attack", true, |_, _| {}),
                        MenuItem::separator(),
                        MenuItem::header("Profile"),
                        MenuItem::radio("Generic", true, |_, _| {}),
                        MenuItem::radio("Ultra speaker", false, |_, _| {}),
                        MenuItem::separator(),
                        MenuItem::action("Edit profiles…", |_, _| {}),
                    ]
                },
            )
            .child(trigger("bank", "Bank"))
            .default_open(2)
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
fn right_clicking_a_tab_opens_its_menu_without_pressing_it() {
    mira_ui::snapshot_with(
        "tab-right-click",
        size(px(240.0), px(160.0)),
        |window, cx| {
            Scene::open(
                |_, _| {
                    div()
                        .w(px(180.0))
                        .child(
                            Tabs::new("setups", "Setups", 2, |index| {
                                Tab::new(["Daily", "Warlord farm"][index])
                                    .menu(|_, _| vec![MenuItem::action("Edit", |_, _| {})])
                            })
                            .vertical()
                            .selected(0)
                            .on_select(|_, _, _| {}),
                        )
                        .into_any_element()
                },
                window,
                cx,
            )
        },
        |cx, owner| {
            use gpui::{MouseButton, MouseDownEvent, PlatformInput, point};

            hover(cx, owner, 60.0, 70.0);
            let image = cx.capture_screenshot(owner).unwrap();
            let scale = image.width() / 240;
            let hovered = *image.get_pixel(30 * scale, 62 * scale);
            cx.update_window(owner, |_, window, cx| {
                window.dispatch_event(
                    PlatformInput::MouseDown(MouseDownEvent {
                        button: MouseButton::Right,
                        position: point(px(60.0), px(70.0)),
                        click_count: 1,
                        ..Default::default()
                    }),
                    cx,
                );
                window.refresh();
            })
            .unwrap();
            cx.run_until_parked();
            let image = cx.capture_screenshot(owner).unwrap();
            assert_eq!(
                *image.get_pixel(30 * scale, 62 * scale),
                hovered,
                "a right-click leaves the row's hover fill"
            );
        },
    );
}

#[test]
fn snapshot_server_signal() {
    scene("server-signal", 380.0, 220.0, |_, cx| {
        let theme = cx.theme().clone();
        let row = |name: &'static str, players: &'static str, ping: &'static str, level| {
            MenuItem::action(name, |_, _| {}).end_slot(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .text_color(theme.muted_foreground)
                    .child(players)
                    .child(ping)
                    .child(mira_ui::primitives::Signal::new(
                        format!("{name} connection"),
                        level,
                    )),
            )
        };
        div()
            .w(px(340.0))
            .p(px(4.0))
            .rounded(px(10.0))
            .bg(theme.popover)
            .child(mira_ui::primitives::MenuList::new(
                "servers",
                "Servers",
                vec![
                    row("Twilly", "862/1000", "66 ms", 3).checked(true),
                    row("Swordhaven (EU)", "166/1000", "176 ms", 2),
                    row("Far away", "40/1000", "320 ms", 1),
                    row("Offline", "Offline", "", 0),
                ],
            ))
            .into_any_element()
    });
}

#[test]
fn snapshot_progress_arcs() {
    scene("progress-arcs", 260.0, 90.0, |_, _| {
        div()
            .flex()
            .items_center()
            .gap(px(16.0))
            .child(mira_ui::primitives::Spinner::new("arc-24").size(px(24.0)))
            .child(mira_ui::primitives::Spinner::new("arc-16"))
            .child(Button::icon("refresh", Icon::RotateCcw, "Refresh").size(Size::XSmall))
            .child(
                Button::icon("refreshing", Icon::RotateCcw, "Refresh")
                    .size(Size::XSmall)
                    .busy(true),
            )
            .child(Button::new("save", "Save").busy(true))
            .into_any_element()
    });
}

#[test]
fn snapshot_focused_sidebar_rows() {
    scene_after_keys("sidebar-focus", 240.0, 140.0, &["tab"], |_, _| {
        div()
            .w(px(180.0))
            .child(
                Tabs::new("setups", "Setups", 2, |index| {
                    Tab::new(["Daily", "Daily 2"][index])
                        .end_slot(div().child("8"))
                        .menu(|_, _| vec![MenuItem::action("Edit", |_, _| {})])
                })
                .vertical()
                .selected(0)
                .on_select(|_, _, _| {}),
            )
            .into_any_element()
    });
}

#[test]
fn snapshot_attention_and_run_parts() {
    mira_ui::snapshot_with(
        "attention-and-run-parts",
        size(px(520.0), px(250.0)),
        |window, cx| {
            Scene::open(
                |_, cx| {
                    let theme = cx.theme().clone();
                    let row = |glyph, text: &'static str| {
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.0))
                            .text_color(theme.muted_foreground)
                            .child(mira_ui::primitives::icon(glyph, theme.muted_foreground))
                            .child(text)
                    };
                    div()
                        .flex()
                        .gap(px(24.0))
                        .child(
                            div().w(px(180.0)).child(
                                Tabs::new("setups", "Setups", 2, |index| {
                                    let (name, count) =
                                        [("Daily", "8"), ("Warlord farm", "4")][index];
                                    Tab::new(name)
                                        .end_slot(div().child(count))
                                        .menu(|_, _| vec![MenuItem::action("Edit", |_, _| {})])
                                })
                                .vertical()
                                .selected(0),
                            ),
                        )
                        .child(
                            div()
                                .flex_1()
                                .flex()
                                .flex_col()
                                .gap(px(12.0))
                                .child(
                                    div()
                                        .flex()
                                        .gap(px(6.0))
                                        .child(Badge::new("Running"))
                                        .child(Badge::new("Needs you").tone(Tone::Attention))
                                        .child(Badge::new("Failed").tone(Tone::Danger))
                                        .child(Dot::new(Tone::Attention))
                                        .child(Dot::new(Tone::Danger)),
                                )
                                .child(
                                    mira_ui::primitives::SegmentedProgress::new(
                                        "run",
                                        "1 done, 2 running, 1 needs you, 4 not started",
                                        8,
                                    )
                                    .part(1, Tone::Success)
                                    .part(2, Tone::Neutral)
                                    .part(1, Tone::Attention),
                                )
                                .child(row(Icon::FileCode, "daily.ts"))
                                .child(row(Icon::MapPin, "Twig · yulgar · Enter"))
                                .child(row(Icon::User, "Lv 100 · ArchPaladin"))
                                .child(row(Icon::Server, "Twig"))
                                .child(
                                    theme
                                        .floating_bar()
                                        .child("3 selected")
                                        .child(
                                            Button::new("stop", "Stop")
                                                .variant(Variant::Ghost)
                                                .size(Size::Small),
                                        )
                                        .child(
                                            Button::new("close", "Close")
                                                .variant(Variant::Ghost)
                                                .size(Size::Small),
                                        ),
                                ),
                        )
                        .into_any_element()
                },
                window,
                cx,
            )
        },
        |cx, owner| hover(cx, owner, 120.0, 72.0),
    );
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
        let layout = cx.theme().layout;
        let typography = cx.theme().typography;
        use mira_ui::{
            components::{Empty, FieldRow, Group},
            primitives::Field,
        };
        let theme = cx.theme().clone();
        div()
            .size_full()
            .p(layout.space(4.0))
            .bg(theme.background)
            .text_color(theme.foreground)
            .font_family(UI_FONT)
            .text_size(typography.body)
            .flex()
            .gap(layout.space(4.0))
            .child(
                div()
                    .w(px(300.0))
                    .flex()
                    .flex_col()
                    .gap(layout.space(3.0))
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

#[test]
fn snapshot_chip_editing() {
    fn render(_: &mut Window, _: &mut Context<Scene>) -> gpui::AnyElement {
        mira_ui::components::Chips::new(
            "drops",
            "Drops",
            ["Tainted Gem", "Dark Crystal Shard", "Void Aura"].map(Into::into),
        )
        .marked(Icon::Bell, Icon::BellRing, "Beep", [true, false, false])
        .on_mark(|_, _, _, _| {})
        .on_remove(|_, _, _| {})
        .on_edit(|_, _, _, _| {})
        .into_any_element()
    }
    scene_after_keys(
        "chip-editing-selected",
        420.0,
        80.0,
        &["tab", "right", "enter"],
        render,
    );
    scene_after_keys(
        "chip-editing-caret",
        420.0,
        80.0,
        &["tab", "right", "enter", "right"],
        render,
    );
}

#[test]
fn snapshot_color_picker_open() {
    scene_after_keys("color-picker", 320.0, 300.0, &["tab", "enter"], |_, _| {
        div()
            .flex()
            .justify_end()
            .child(mira_ui::primitives::ColorPicker::new(
                "primary",
                "Light primary",
                mira_ui::theme::parse_hex("#2563EB").unwrap(),
            ))
            .into_any_element()
    });
}

#[test]
fn snapshot_shortcut_field_recording() {
    scene_after_keys("shortcut-recording", 320.0, 120.0, &["tab"], |_, _| {
        div()
            .flex()
            .flex_col()
            .items_start()
            .gap(px(12.0))
            .child(mira_ui::components::ShortcutField::new(
                "bank",
                "Bank",
                Some(gpui::Keystroke::parse("cmd-b").unwrap()),
            ))
            .child(mira_ui::components::ShortcutField::new(
                "jump",
                "Jump",
                Some(gpui::Keystroke::parse("cmd-j").unwrap()),
            ))
            .child(mira_ui::components::ShortcutField::new("ads", "Ads", None))
            .into_any_element()
    });
}

#[test]
fn snapshot_radio_previews_focused() {
    scene_after_keys("radio-previews", 420.0, 150.0, &["tab"], |_, cx| {
        let theme = cx.theme().clone();
        let art = move || {
            div()
                .w(px(96.0))
                .h(px(56.0))
                .rounded(px(4.0))
                .bg(theme.muted)
        };
        mira_ui::primitives::RadioGroup::new(
            "mode",
            "Mode",
            ["System", "Light", "Dark"]
                .map(|label| mira_ui::primitives::Radio::new(label).preview(art()))
                .into_iter()
                .collect(),
            Some(1),
        )
        .into_any_element()
    });
}

#[test]
fn snapshot_disclosure_hover() {
    let paths = mira_ui::snapshot_with(
        "disclosure-hover",
        size(px(240.0), px(90.0)),
        |window, cx| {
            Scene::open(
                |_, _| {
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.0))
                        .child(mira_ui::components::Disclosure::new(
                            "hovered", "Advanced", false,
                        ))
                        .child(mira_ui::components::Disclosure::new(
                            "rest", "Advanced", true,
                        ))
                        .into_any_element()
                },
                window,
                cx,
            )
        },
        |cx, owner| hover(cx, owner, 30.0, 28.0),
    );
    assert!(paths.iter().all(|path| path.exists()));
}

#[test]
fn snapshot_long_select_scrolls() {
    use gpui::{Keystroke, PlatformInput, ScrollDelta, ScrollEvent, point};

    let paths = mira_ui::snapshot_with(
        "select-long",
        size(px(320.0), px(360.0)),
        |window, cx| {
            Scene::open(
                |_, _| {
                    Select::new(
                        "font",
                        "Font",
                        (0..40)
                            .map(|index| {
                                gpui::SharedString::from(if index % 3 == 1 {
                                    format!("Hiragino Kaku Gothic ProN W{index}")
                                } else {
                                    format!("Font family {index}")
                                })
                            })
                            .collect::<Vec<_>>(),
                        Some(0),
                    )
                    .width(px(176.0))
                    .into_any_element()
                },
                window,
                cx,
            )
        },
        |cx, owner| {
            for key in ["tab", "enter"] {
                cx.update_window(owner, |_, window, cx| {
                    window.dispatch_keystroke(Keystroke::parse(key).unwrap(), cx);
                })
                .unwrap();
                cx.run_until_parked();
            }
            hover(cx, owner, 90.0, 200.0);
            cx.update_window(owner, |_, window, cx| {
                window.dispatch_event(
                    PlatformInput::Scroll(ScrollEvent {
                        position: point(px(90.0), px(200.0)),
                        delta: ScrollDelta::Pixels(point(px(0.0), px(-200.0))),
                        ..Default::default()
                    }),
                    cx,
                );
                window.refresh();
            })
            .unwrap();
            cx.run_until_parked();
        },
    );
    assert!(paths.iter().all(|path| path.exists()));
}

#[test]
fn snapshot_shortcut_field_hints() {
    for (name, keys) in [
        ("shortcut-plain-key", &["tab", "a"][..]),
        ("shortcut-rejected", &["tab", "cmd-q"][..]),
        ("shortcut-return", &["tab", "enter"][..]),
    ] {
        scene_after_keys(name, 320.0, 80.0, keys, |_, _| {
            mira_ui::components::ShortcutField::new(
                "bank",
                "Bank",
                Some(gpui::Keystroke::parse("cmd-b").unwrap()),
            )
            .on_change(|_, _, _| Err("Used by Quit".into()))
            .into_any_element()
        });
    }
}
