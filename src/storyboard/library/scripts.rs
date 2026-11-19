use std::rc::Rc;

use gpui::{
    Context, Entity, IntoElement, ParentElement as _, Render, Styled as _, Window, div,
    prelude::FluentBuilder as _, px,
};
use mira_ui::{
    h_flex,
    primitives::{
        Badge, Button, Checkbox, CheckedState, DropdownMenu, Field, Icon, Input, List, ListItem,
        MenuItem, Select, Tab, Tabs, Toast, Toaster,
        button::{Size, Variant},
    },
    v_flex,
};

use super::super::{Screen, view::*};

struct Script {
    name: &'static str,
    path: &'static str,
    source: usize,
    area: &'static str,
    description: &'static str,
}

const LIBRARY: &[Script] = &[
    Script {
        name: "Relic circuit",
        path: "relic-circuit.ts",
        source: 2,
        area: "Stone causeway",
        description: "Collect relic fragments and turn them in at the abbey.",
    },
    Script {
        name: "Daily route",
        path: "daily-route.ts",
        source: 1,
        area: "Sanctuary",
        description: "Complete the daily quests on your route.",
    },
    Script {
        name: "Essence farm",
        path: "essence-farm.ts",
        source: 2,
        area: "Sunken crypt",
        description: "Farm essence until the requested quantity is reached.",
    },
    Script {
        name: "Reputation loop",
        path: "reputation-loop.ts",
        source: 2,
        area: "Northwatch",
        description: "Repeat the local reputation quest.",
    },
    Script {
        name: "Guild supplies",
        path: "guild-supplies.ts",
        source: 1,
        area: "Moonlit crossing",
        description: "Gather supplies for the guild quartermaster.",
    },
    Script {
        name: "Bank cleanup",
        path: "bank-cleanup.ts",
        source: 1,
        area: "Sanctuary",
        description: "Move materials to the bank after a session.",
    },
];

pub(in crate::storyboard) struct Scripts {
    target: Entity<Target>,
    pane: usize,
    scope: usize,
    selected: usize,
    queued: usize,
    queue: Vec<usize>,
    running: Option<usize>,
    search: Entity<Input>,
    room: Entity<Input>,
    quantity: Entity<Input>,
    stop_when_full: bool,
    detail: bool,
}

impl Scripts {
    pub(in crate::storyboard) fn new(sessions: Entity<Sessions>, cx: &mut Context<Self>) -> Self {
        let search = field("Search scripts", "Search scripts", "", true, cx);
        observe_input(&search, cx);
        let target = Target::new(sessions, cx);
        cx.observe(&target, |_, _, cx| cx.notify()).detach();
        Self {
            target,
            pane: 0,
            scope: 0,
            selected: 0,
            queued: 0,
            queue: vec![1, 4],
            running: None,
            search,
            room: field("Room number", "Optional", "4071", false, cx),
            quantity: field("Quantity", "Quantity", "15", false, cx),
            stop_when_full: true,
            detail: false,
        }
    }

    #[cfg(test)]
    pub(in crate::storyboard) fn preview_queue(mut self) -> Self {
        self.pane = 1;
        self
    }

    fn start_stop(&mut self, script: usize, cx: &mut Context<Self>) {
        let starting = self.running != Some(script);
        self.running = starting.then_some(script);
        Toaster::push(
            Toast::new(format!(
                "{} {}",
                LIBRARY[script].name,
                if starting { "started" } else { "stopped" }
            )),
            cx,
        );
    }

    fn move_queued(&mut self, from: usize, to: usize) {
        if from >= self.queue.len() || to >= self.queue.len() {
            return;
        }
        self.queue.swap(from, to);
        self.queued = match self.queued {
            index if index == from => to,
            index if index == to => from,
            index => index,
        };
    }
}

impl Render for Scripts {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = palette(cx);
        let wide = window_width(Screen::Scripts, window) >= px(704.0);
        let preview = StatusPreview::current(cx);
        let library = self.pane == 0;
        let query = self.search.read(cx).text().trim().to_lowercase();
        let matches = Rc::new(if library {
            LIBRARY
                .iter()
                .enumerate()
                .filter_map(|(index, entry)| {
                    ((self.scope == 0 || self.scope == entry.source)
                        && entry.name.to_lowercase().contains(&query))
                    .then_some(index)
                })
                .collect::<Vec<_>>()
        } else {
            self.queue.clone()
        });
        let row = if library {
            matches
                .iter()
                .position(|&index| index == self.selected)
                .unwrap_or(0)
        } else {
            self.queued.min(matches.len().saturating_sub(1))
        };
        let selection = matches.get(row).copied();
        let count = self.queue.len();
        let tabs = Tabs::new("panes", "Script views", 2, move |index| match index {
            0 => Tab::new("Library"),
            _ => Tab::new("Queue").end_slot(Badge::new(count.to_string())),
        })
        .selected(self.pane)
        .on_select(choose(cx, |view, pane, _| {
            view.pane = pane;
            view.detail = false;
        }));
        let toolbar = compact_toolbar().child(tabs).when(library, |bar| {
            bar.child(spring()).child(
                div()
                    .flex_1()
                    .min_w(px(90.0))
                    .max_w(px(200.0))
                    .child(self.search.clone()),
            )
        });
        let available = self.target.read(cx).available(cx);
        let actions = selection.filter(|_| available && (wide || self.detail));
        let owner = cx.weak_entity();
        let footer = footer(cx)
            .when(library, |bar| {
                bar.child(
                    Select::new(
                        "source",
                        "Script source",
                        ["All sources", "My scripts", "Installed packages"],
                        Some(self.scope),
                    )
                    .width(px(140.0))
                    .on_select(choose(cx, |view, selected, _| {
                        view.scope = selected;
                        view.detail = false;
                    })),
                )
                .child(
                    DropdownMenu::new("packages")
                        .trigger(menu_button("packages-trigger", "Packages", cx))
                        .items(move |_, _| {
                            let browse = owner.clone();
                            let refresh = owner.clone();
                            vec![
                                MenuItem::header("Installed · Field notes"),
                                MenuItem::action("Show package scripts", move |_, cx| {
                                    browse
                                        .update(cx, |view, cx| {
                                            view.scope = 2;
                                            view.detail = false;
                                            cx.notify();
                                        })
                                        .ok();
                                }),
                                MenuItem::action("Refresh library", move |_, cx| {
                                    refresh
                                        .update(cx, |view, cx| {
                                            view.search
                                                .update(cx, |search, cx| search.set_text("", cx));
                                            Toaster::push(Toast::new("Library refreshed"), cx);
                                            cx.notify();
                                        })
                                        .ok();
                                }),
                            ]
                        }),
                )
            })
            .child(spring())
            .when_some(actions, |bar, index| {
                let running = self.running == Some(index) && preview == StatusPreview::Current;
                bar.children(status_preview(
                    "Starting script…",
                    "Couldn't start script",
                    "The room is full. Try a different room.",
                    cx,
                ))
                .when(running, |bar| {
                    bar.child(status("Script running", false, cx))
                })
                .when(library, |bar| {
                    bar.child(command(
                        "queue",
                        "Add to queue",
                        Variant::Outline,
                        change(cx, move |view, cx| {
                            view.queue.push(index);
                            Toaster::push(
                                Toast::new("Added to queue").body(LIBRARY[index].name),
                                cx,
                            );
                        }),
                    ))
                })
                .child(
                    command(
                        "run",
                        if running { "Stop" } else { "Run" },
                        if running {
                            Variant::Outline
                        } else {
                            Variant::Primary
                        },
                        change(cx, move |view, cx| view.start_stop(index, cx)),
                    )
                    .disabled(preview == StatusPreview::Working),
                )
            });
        let model = matches.clone();
        let choose_item = choose(cx, move |view, row: usize, _| {
            if let Some(&script) = model.get(row) {
                if library {
                    view.selected = script;
                } else {
                    view.queued = row;
                }
                view.detail = true;
            }
        });
        let list = if matches.is_empty() {
            empty(
                if library {
                    "No matching scripts"
                } else {
                    "Queue is empty"
                },
                if library {
                    "Try another search or source."
                } else {
                    "Add a script from the Library."
                },
                cx,
            )
            .into_any_element()
        } else {
            let rows = matches.clone();
            let running = self.running;
            let count = rows.len();
            let owner = cx.weak_entity();
            List::new(
                "script-list",
                if library { "Scripts" } else { "Queued scripts" },
                count,
                move |row| {
                    let index = rows[row];
                    let script = &LIBRARY[index];
                    let item = ListItem::new(script.name).description(if library {
                        script.path
                    } else if running == Some(index) {
                        "Running"
                    } else {
                        "Waiting"
                    });
                    if library || count < 2 {
                        item
                    } else {
                        let move_button = |up| {
                            let owner = owner.clone();
                            let (icon, label, to) = if up {
                                (Icon::ChevronUp, "Move up", row.checked_sub(1))
                            } else {
                                (
                                    Icon::ChevronDown,
                                    "Move down",
                                    (row + 1 < count).then_some(row + 1),
                                )
                            };
                            Button::icon(("reorder", up as usize), icon, label)
                                .tooltip(format!("{label} {}", script.name))
                                .size(Size::Small)
                                .disabled(to.is_none())
                                .on_click(move |_, _, cx| {
                                    if let Some(to) = to {
                                        owner
                                            .update(cx, |view, cx| {
                                                view.move_queued(row, to);
                                                cx.notify();
                                            })
                                            .ok();
                                    }
                                })
                        };
                        item.end_slot(h_flex().child(move_button(true)).child(move_button(false)))
                    }
                },
            )
            .selected(Some(row))
            .fill()
            .on_select(choose_item)
            .into_any_element()
        };
        let detail = if let Some(index) = selection {
            let script = &LIBRARY[index];
            v_flex()
                .flex_1()
                .min_w_0()
                .min_h_0()
                .child(
                    scroll("script-detail")
                        .p(px(16.0))
                        .gap(px(16.0))
                        .when(!wide, |body| {
                            body.child(
                                command(
                                    "back",
                                    "Back to scripts",
                                    Variant::Ghost,
                                    change(cx, |view, _| view.detail = false),
                                )
                                .self_start(),
                            )
                        })
                        .child(
                            v_flex()
                                .gap(px(2.0))
                                .child(title(script.name))
                                .child(meta(
                                    format!(
                                        "{} / {}",
                                        if script.source == 1 {
                                            "Documents/Mira/scripts"
                                        } else {
                                            "Field notes"
                                        },
                                        script.path
                                    ),
                                    cx,
                                ))
                                .child(div().pt(px(6.0)).child(script.description)),
                        )
                        .child(divider(cx))
                        .child(
                            v_flex()
                                .gap(px(12.0))
                                .child(title("Inputs"))
                                .child(Field::new("Area", div().child(script.area)))
                                .child(
                                    h_flex()
                                        .items_start()
                                        .gap(px(12.0))
                                        .child(
                                            div()
                                                .flex_1()
                                                .child(Field::new("Room", self.room.clone())),
                                        )
                                        .child(
                                            div().flex_1().child(Field::new(
                                                "Quantity",
                                                self.quantity.clone(),
                                            )),
                                        ),
                                )
                                .child(
                                    Checkbox::new(
                                        "stop-full",
                                        "Stop when inventory is full",
                                        if self.stop_when_full {
                                            CheckedState::Checked
                                        } else {
                                            CheckedState::Unchecked
                                        },
                                    )
                                    .on_toggle(choose(cx, |view, on, _| view.stop_when_full = on)),
                                ),
                        ),
                )
                .into_any_element()
        } else {
            empty("Select a script", "Its inputs and actions appear here.", cx).into_any_element()
        };
        let body = if !available {
            empty("No tabs open", "Open a game session to run a script.", cx).into_any_element()
        } else if wide {
            h_flex()
                .items_stretch()
                .flex_1()
                .min_h_0()
                .child(
                    v_flex()
                        .flex_none()
                        .w(px(248.0))
                        .px(px(8.0))
                        .py(px(8.0))
                        .border_r_1()
                        .border_color(colors.border)
                        .child(list),
                )
                .child(detail)
                .into_any_element()
        } else if self.detail && selection.is_some() {
            detail
        } else {
            list
        };
        frame(
            tool_chrome(Screen::Scripts, Some(self.target.clone()), cx),
            cx,
        )
        .child(toolbar)
        .child(body)
        .when(library || actions.is_some(), |frame| frame.child(footer))
    }
}
