use std::sync::{Arc, OnceLock};

use gpui::{
    App, Context, Entity, FontWeight, Image, ImageFormat, InteractiveElement as _, IntoElement,
    ObjectFit, ParentElement as _, Render, Role, SharedString, StatefulInteractiveElement as _,
    Styled as _, StyledImage as _, Window, div, img, prelude::FluentBuilder as _, px,
};
use mira_ui::{
    h_flex,
    primitives::{
        Align, Button, DropdownMenu, Icon, MenuItem, Popover, Switch, Toast, Toaster,
        button::{Size, Variant},
    },
    v_flex,
};

use super::{FRAMES, Screen, view::*};

pub(super) struct Game {
    nav: Navigation,
    sessions: Entity<Sessions>,
    location: &'static str,
    running: bool,
    grid: bool,
    rail: bool,
    attack: bool,
    relogin: bool,
    hide_players: bool,
}

impl Game {
    pub fn new(nav: Navigation, sessions: Entity<Sessions>, cx: &mut Context<Self>) -> Self {
        cx.observe(&sessions, |_, _, cx| cx.notify()).detach();
        Self {
            nav,
            sessions,
            location: "Stone causeway",
            running: false,
            grid: false,
            rail: false,
            attack: false,
            relogin: true,
            hide_players: false,
        }
    }

    fn sessions_menu(&self, cx: &Context<Self>) -> DropdownMenu {
        let sessions = self.sessions.read(cx);
        let selected = sessions.active;
        let names = sessions.names.clone();
        let label = sessions
            .current()
            .cloned()
            .unwrap_or_else(|| "Sessions".into());
        let entity = self.sessions.clone();
        let nav = self.nav.clone();
        let rail = self.rail;
        let toggle_rail = std::rc::Rc::new(change(cx, |view, _| view.rail = !view.rail));
        DropdownMenu::new("sessions")
            .width(px(200.0))
            .trigger(menu_button("sessions-trigger", label, cx).max_w(px(140.0)))
            .items(move |_, _| {
                let mut items = names
                    .iter()
                    .enumerate()
                    .map(|(index, name)| {
                        let entity = entity.clone();
                        MenuItem::radio(name.clone(), index == selected, move |_, cx| {
                            entity.update(cx, |session, cx| {
                                session.active = index;
                                cx.notify();
                            });
                        })
                    })
                    .collect::<Vec<_>>();
                if !items.is_empty() {
                    items.push(MenuItem::separator());
                }
                let toggle_rail = toggle_rail.clone();
                items.push(MenuItem::action(
                    if rail {
                        "Hide session rail"
                    } else {
                        "Show session rail"
                    },
                    move |window, cx| toggle_rail(window, cx),
                ));
                let close = entity.clone();
                items.push(
                    MenuItem::action("Close selected tab", move |_, cx| {
                        close.update(cx, |sessions, cx| {
                            sessions.close();
                            cx.notify();
                        });
                    })
                    .disabled(names.is_empty()),
                );
                let nav = nav.clone();
                items.push(MenuItem::action("Accounts", move |window, cx| {
                    if let Some(open) = &nav {
                        open(Screen::Accounts, window, cx);
                    }
                }));
                items
            })
    }

    fn windows_menu(&self, cx: &App) -> DropdownMenu {
        let nav = self.nav.clone();
        DropdownMenu::new("tools")
            .width(px(192.0))
            .trigger(menu_button("tools-trigger", "Windows", cx))
            .items(move |_, _| {
                FRAMES
                    .iter()
                    .filter(|frame| frame.screen != Screen::Game)
                    .map(|frame| {
                        let screen = frame.screen;
                        let nav = nav.clone();
                        MenuItem::action(screen.name(), move |window, cx| {
                            if let Some(open) = &nav {
                                open(screen, window, cx);
                            }
                        })
                    })
                    .collect()
            })
    }

    fn options(&self, cx: &Context<Self>) -> Popover {
        let owner = cx.weak_entity();
        Popover::new("options")
            .align(Align::Start)
            .width(px(240.0))
            .trigger(menu_button("options-trigger", "Options", cx))
            .content(move |_, _, cx| {
                let Ok((relogin, hide_players)) =
                    owner.read_with(cx, |view, _| (view.relogin, view.hide_players))
                else {
                    return div().into_any_element();
                };
                let auto = owner.clone();
                let hide = owner.clone();
                v_flex()
                    .gap(px(12.0))
                    .p(px(8.0))
                    .child(title("Game options"))
                    .child(
                        Switch::new("auto-relogin", "Auto relogin", relogin).on_toggle(
                            move |on, _, cx| {
                                auto.update(cx, |view, cx| {
                                    view.relogin = on;
                                    cx.notify();
                                })
                                .ok();
                            },
                        ),
                    )
                    .child(
                        Switch::new("hide-players", "Hide other players", hide_players).on_toggle(
                            move |on, _, cx| {
                                hide.update(cx, |view, cx| {
                                    view.hide_players = on;
                                    cx.notify();
                                })
                                .ok();
                            },
                        ),
                    )
                    .into_any_element()
            })
    }

    fn jump(&self, disabled: bool, cx: &Context<Self>) -> DropdownMenu {
        let selected = self.location;
        let owner = cx.weak_entity();
        DropdownMenu::new("jump")
            .align(Align::End)
            .width(px(176.0))
            .trigger(menu_button("jump-trigger", "Jump", cx).disabled(disabled))
            .items(move |_, _| Self::locations(selected, &owner))
    }

    fn locations(selected: &'static str, owner: &gpui::WeakEntity<Self>) -> Vec<MenuItem> {
        ["Stone causeway", "West gate", "East arch"]
            .into_iter()
            .map(|location| {
                let owner = owner.clone();
                MenuItem::radio(location, selected == location, move |_, cx| {
                    owner
                        .update(cx, |view, cx| {
                            view.location = location;
                            cx.notify();
                        })
                        .ok();
                })
            })
            .collect()
    }

    fn more(&self, disabled: bool, cx: &Context<Self>) -> DropdownMenu {
        let nav = self.nav.clone();
        let attack = self.attack;
        let location = self.location;
        let owner = cx.weak_entity();
        DropdownMenu::new("more-game-actions")
            .align(Align::End)
            .width(px(176.0))
            .trigger(menu_button("more-game", "More", cx).disabled(disabled))
            .items(move |_, _| {
                let menu_owner = owner.clone();
                let nav = nav.clone();
                vec![
                    MenuItem::switch("Auto attack", attack, move |_, cx| {
                        menu_owner
                            .update(cx, |view, cx| {
                                view.attack = !view.attack;
                                cx.notify();
                            })
                            .ok();
                    }),
                    MenuItem::submenu("Jump", Self::locations(location, &owner)),
                    MenuItem::action("Bank", move |window, cx| {
                        if let Some(open) = &nav {
                            open(Screen::Loader, window, cx);
                        }
                    }),
                ]
            })
    }

    fn viewport(&self, name: SharedString, active: bool, cx: &App) -> gpui::Div {
        static SCENE: OnceLock<Arc<Image>> = OnceLock::new();
        let art = SCENE.get_or_init(|| {
            Arc::new(Image::from_bytes(
                ImageFormat::Jpeg,
                include_bytes!("../../assets/storyboard/stone-causeway.jpg").to_vec(),
            ))
        });
        let running = self.running && active;
        let colors = palette(cx);
        let preview = active
            .then(|| {
                status_preview(
                    "Reconnecting…",
                    "Connection lost",
                    "Waiting for the game session to reconnect.",
                    cx,
                )
            })
            .flatten();
        let session_info = if let Some(preview) = preview {
            preview.into_any_element()
        } else {
            meta(
                if running {
                    "Running · Relic circuit"
                } else {
                    self.location
                },
                cx,
            )
            .into_any_element()
        };
        v_flex()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .relative()
            .overflow_hidden()
            .bg(gpui::rgb(0x222720))
            .child(img(art.clone()).size_full().object_fit(ObjectFit::Cover))
            .child(
                h_flex()
                    .absolute()
                    .left(px(12.0))
                    .bottom(px(12.0))
                    .gap(px(8.0))
                    .px(px(10.0))
                    .py(px(6.0))
                    .rounded(px(8.0))
                    .bg(colors.popover)
                    .border_1()
                    .border_color(colors.border)
                    .child(div().font_weight(FontWeight::MEDIUM).child(name))
                    .child(vertical_edge(cx))
                    .child(session_info),
            )
            .when(self.grid && active, |viewport| {
                viewport.child(
                    div()
                        .absolute()
                        .inset_0()
                        .border_2()
                        .border_color(colors.ring),
                )
            })
    }
}

impl Render for Game {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let sessions = self.sessions.read(cx);
        let names = sessions.names.clone();
        let active = sessions.active;
        let tile_focuses: Vec<_> = if self.grid {
            (0..names.len())
                .map(|index| mira_ui::nav::focus_handle(("tile", index), window, cx))
                .collect()
        } else {
            Vec::new()
        };
        let disabled = names.is_empty();
        let add = self.sessions.clone();
        let scripts = self.nav.clone();
        let bank = self.nav.clone();
        let rail_actions = self.sessions.clone();
        let wide = window_width(Screen::Game, window) >= px(760.0);
        let colors = palette(cx);
        let bar = h_flex()
            .flex_1()
            .min_w_0()
            .gap(px(4.0))
            .child(self.sessions_menu(cx))
            .child(
                Button::icon("new-session", Icon::Plus, "New tab")
                    .size(Size::Small)
                    .on_click(move |_, _, cx| {
                        add.update(cx, |sessions, cx| {
                            sessions.add();
                            cx.notify();
                        });
                    }),
            )
            .child(
                Button::icon("grid", Icon::LayoutGrid, "Grid view")
                    .size(Size::Small)
                    .pressed(self.grid)
                    .disabled(names.len() < 2)
                    .on_click(cx.listener(|view, _, _, cx| {
                        view.grid = !view.grid;
                        cx.notify();
                    })),
            )
            .child(vertical_edge(cx).mx(px(4.0)))
            .child(self.windows_menu(cx))
            .child(
                command("scripts", "Scripts", Variant::Ghost, move |window, cx| {
                    if let Some(open) = &scripts {
                        open(Screen::Scripts, window, cx);
                    }
                })
                .size(Size::Small),
            )
            .child(self.options(cx))
            .child(spring())
            .when(wide, |bar| {
                bar.child(
                    Button::new(
                        "attack",
                        if self.attack {
                            "Auto attack on"
                        } else {
                            "Auto attack"
                        },
                    )
                    .variant(Variant::Ghost)
                    .size(Size::Small)
                    .pressed(self.attack)
                    .disabled(disabled)
                    .on_click(cx.listener(|view, _, _, cx| {
                        view.attack = !view.attack;
                        cx.notify();
                    })),
                )
                .child(self.jump(disabled, cx))
                .child(
                    command("bank", "Bank", Variant::Ghost, move |window, cx| {
                        if let Some(open) = &bank {
                            open(Screen::Loader, window, cx);
                        }
                    })
                    .size(Size::Small)
                    .disabled(disabled),
                )
                .child(vertical_edge(cx).mx(px(4.0)))
            })
            .when(!wide, |bar| bar.child(self.more(disabled, cx)))
            .child(
                command(
                    "script",
                    if self.running { "Stop" } else { "Start" },
                    if self.running {
                        Variant::Outline
                    } else {
                        Variant::Primary
                    },
                    change(cx, |view, cx| {
                        view.running = !view.running;
                        Toaster::push(
                            Toast::new(if view.running {
                                "Relic circuit started"
                            } else {
                                "Relic circuit stopped"
                            }),
                            cx,
                        );
                    }),
                )
                .size(Size::Small)
                .disabled(disabled),
            );
        let body = match names.get(active) {
            None => empty("No tabs open", "Choose New tab to open a game session.", cx)
                .into_any_element(),
            Some(_) if self.grid && names.len() > 1 => {
                let columns = if names.len() <= 4 { 2 } else { 3 };
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .gap(px(1.0))
                    .bg(colors.border)
                    .children(names.chunks(columns).enumerate().map(|(row, names)| {
                        h_flex()
                            .flex_1()
                            .min_h_0()
                            .items_stretch()
                            .gap(px(1.0))
                            .children(names.iter().cloned().enumerate().map(|(column, name)| {
                                let session = row * columns + column;
                                let focus = tile_focuses[session].clone();
                                self.viewport(name.clone(), session == active, cx)
                                    .id(("tile", session))
                                    .role(Role::Button)
                                    .aria_label(format!("Select {name}"))
                                    .track_focus(&focus.clone().tab_stop(true))
                                    .on_click(cx.listener(move |view, _, window, cx| {
                                        window.focus(&focus, cx);
                                        view.sessions.update(cx, |sessions, cx| {
                                            sessions.active = session;
                                            cx.notify();
                                        });
                                    }))
                            }))
                    }))
                    .into_any_element()
            }
            Some(name) => self.viewport(name.clone(), true, cx).into_any_element(),
        };
        frame(bar, cx).child(
            h_flex()
                .items_stretch()
                .flex_1()
                .min_h_0()
                .when(self.rail, |body| {
                    body.child(
                        scroll("session-rail")
                            .flex_none()
                            .w(px(224.0))
                            .p(px(8.0))
                            .gap(px(4.0))
                            .bg(colors.sidebar)
                            .border_r_1()
                            .border_color(colors.sidebar_border)
                            .child(meta("Sessions", cx).px(px(8.0)).py(px(4.0)))
                            .children(names.into_iter().enumerate().map(|(index, name)| {
                                let sessions = rail_actions.clone();
                                Button::new(("session", index), name.clone())
                                    .variant(Variant::Ghost)
                                    .pressed(index == active)
                                    .w_full()
                                    .child(
                                        h_flex()
                                            .flex_1()
                                            .justify_between()
                                            .gap(px(8.0))
                                            .child(name)
                                            .child(meta(
                                                if index == active {
                                                    "Selected"
                                                } else {
                                                    "Online"
                                                },
                                                cx,
                                            )),
                                    )
                                    .on_click(move |_, _, cx| {
                                        sessions.update(cx, |session, cx| {
                                            session.active = index;
                                            cx.notify();
                                        })
                                    })
                            })),
                    )
                })
                .child(body),
        )
    }
}
