use std::rc::Rc;

use gpui::{
    ClipboardItem, Context, Div, Entity, InteractiveElement as _, IntoElement, ParentElement as _,
    Render, Role, StatefulInteractiveElement as _, Styled as _, Window, div,
    prelude::FluentBuilder as _, px,
};
use mira_ui::{
    h_flex,
    primitives::{
        Align, Button, Checkbox, CheckedState, Column, Dialog, DropdownMenu, Icon, Input, InputEvent,
        MenuItem, Select, Sort, Spinner, Table, Toast, Toaster,
        button::{Joined, Size, Variant},
        icon,
    },
    theme::SYSTEM_MONO,
    v_flex,
};

use crate::domain::grabber::{Load, LoadKind, Source};

use super::super::{Screen, view::*};

#[derive(Clone, Copy)]
struct ResultRow(&'static str, u32, u32, &'static str);

const SHOP: &[ResultRow] = &[
    ResultRow("Abbey lantern", 1200, 30481, "Off-hand"),
    ResultRow("Wayfarer's cloak", 3600, 30482, "Cape"),
    ResultRow("Stoneward blade", 6500, 30483, "Sword"),
    ResultRow("Bronze ring", 850, 30484, "Accessory"),
    ResultRow("Mossweave hood", 1800, 30485, "Helm"),
    ResultRow("Restoration draught", 240, 30486, "Consumable"),
    ResultRow("Abbey travel kit", 1000, 30487, "Item"),
    ResultRow("Patrol's pack", 1500, 30488, "Cape"),
];
const QUESTS: &[ResultRow] = &[
    ResultRow("Recover the relics", 1, 8102, "Repeatable"),
    ResultRow("Abbey patrol", 1, 8103, "Daily"),
    ResultRow("The abbey route", 1, 8104, "Story quest"),
];
const INVENTORY: &[ResultRow] = &[
    ResultRow("Relic fragment", 12, 30490, "Material"),
    ResultRow("Abbey seal", 2, 30496, "Material"),
    ResultRow("Ancient ore", 8, 30498, "Material"),
    ResultRow("Reputation boost", 1, 30499, "Boost"),
];
const MONSTERS: &[ResultRow] = &[
    ResultRow("Abbey sentinel", 1, 2049, "Causeway"),
    ResultRow("Mosshound", 2, 2050, "West gate"),
    ResultRow("Ruins keeper", 3, 2051, "East arch"),
];

struct ShopWarning {
    load: Load,
    quiet: bool,
}

pub(in crate::storyboard) struct Loader {
    target: Entity<Target>,
    source: Source,
    search: Entity<Input>,
    shop_id: Entity<Input>,
    shop_problem: Option<&'static str>,
    shop_warning: Option<ShopWarning>,
    warn_before_loading: bool,
    selected: Option<u32>,
    sort: Sort,
    detail: bool,
}

impl Loader {
    pub(in crate::storyboard) fn new(sessions: Entity<Sessions>, cx: &mut Context<Self>) -> Self {
        let target = Target::new(sessions, cx);
        cx.observe(&target, |view, _, cx| {
            view.shop_warning = None;
            cx.notify();
        })
        .detach();
        let search = field("Search results", "Search results", "", true, cx);
        observe_input(&search, cx);
        let shop_id = field("Shop ID", LoadKind::Shop.placeholder(), "120", false, cx);
        cx.subscribe(&shop_id, |view, input, event, cx| match event {
            InputEvent::Changed => {
                if view.shop_problem.take().is_some() {
                    input.update(cx, |input, cx| input.set_invalid(false, cx));
                }
                cx.notify();
            }
            InputEvent::Submitted => view.request_shop_load(LoadKind::Shop, cx),
        })
        .detach();
        Self {
            target,
            source: Source::Shop,
            search,
            shop_id,
            shop_problem: None,
            shop_warning: None,
            warn_before_loading: true,
            selected: Some(SHOP[0].2),
            sort: Sort {
                column: 0,
                ascending: true,
            },
            detail: false,
        }
    }

    fn results(&self) -> &'static [ResultRow] {
        match self.source {
            Source::Shop => SHOP,
            Source::Quests => QUESTS,
            Source::Monsters => MONSTERS,
            _ => INVENTORY,
        }
    }

    fn request_shop_load(&mut self, kind: LoadKind, cx: &mut Context<Self>) {
        if self.source != Source::Shop
            || !self.target.read(cx).available(cx)
            || StatusPreview::current(cx) == StatusPreview::Working
        {
            return;
        }
        let text = self.shop_id.read(cx).text();
        if kind.takes_ids() && text.trim().is_empty() {
            return;
        }
        match kind.parse(text) {
            Ok(load)
                if self.warn_before_loading
                    && matches!(load, Load::Shop(_) | Load::HairShop(_)) =>
            {
                self.shop_warning = Some(ShopWarning { load, quiet: false });
            }
            Ok(load) => self.complete_shop_load(load, cx),
            Err(problem) => {
                self.shop_problem = Some(problem.message());
                self.shop_id
                    .update(cx, |input, cx| input.set_invalid(true, cx));
            }
        }
        cx.notify();
    }

    fn complete_shop_load(&mut self, load: Load, cx: &mut Context<Self>) {
        let message = if matches!(load, Load::Shop(_)) {
            self.search.update(cx, |input, cx| input.set_text("", cx));
            format!("Loaded {load}")
        } else {
            format!("Opened {load}")
        };
        Toaster::push(Toast::new(message), cx);
    }

    fn confirm_shop_load(&mut self, cx: &mut Context<Self>) {
        if let Some(warning) = self.shop_warning.take() {
            self.warn_before_loading = !warning.quiet;
            self.complete_shop_load(warning.load, cx);
        }
    }

    fn shop_controls(&self, blocked: bool, hint: Option<&'static str>, cx: &Context<Self>) -> Div {
        let empty = self.shop_id.read(cx).text().trim().is_empty();
        let warn = self.warn_before_loading;
        let owner = cx.weak_entity();
        let more = DropdownMenu::new("shop-load-more")
            .align(Align::End)
            .trigger(
                Button::icon(
                    "shop-load-more-trigger",
                    Icon::ChevronDown,
                    "Other shop actions",
                )
                .variant(Variant::Primary)
                .joined(Joined::Previous)
                .disabled(blocked)
                .hide_tooltip(),
            )
            .items(move |_, _| {
                let toggle = owner.clone();
                [
                    ("Load as hair shop", LoadKind::HairShop),
                    ("Open armor customizer", LoadKind::ArmorCustomizer),
                ]
                .map(|(label, kind)| {
                    let owner = owner.clone();
                    MenuItem::action(label, move |_, cx| {
                        owner
                            .update(cx, |view, cx| view.request_shop_load(kind, cx))
                            .ok();
                    })
                    .disabled(empty && kind.takes_ids())
                })
                .into_iter()
                .chain([
                    MenuItem::separator(),
                    MenuItem::action("Confirm shop loads", move |_, cx| {
                        toggle
                            .update(cx, |view, cx| {
                                view.warn_before_loading = !view.warn_before_loading;
                                cx.notify();
                            })
                            .ok();
                    })
                    .checked(warn)
                    .keep_open(),
                ])
                .collect()
            });
        h_flex()
            .debug_selector(|| "loader-shop-controls".into())
            .flex_none()
            .gap(px(8.0))
            .child(div().w(px(112.0)).child(self.shop_id.clone()))
            .child(
                h_flex()
                    .id("shop-load-group")
                    .role(Role::Group)
                    .aria_label("Load shop")
                    .child(
                        command(
                            "shop-load",
                            "Load",
                            Variant::Primary,
                            change(cx, |view, cx| view.request_shop_load(LoadKind::Shop, cx)),
                        )
                        .joined(Joined::Next)
                        .disabled(blocked || empty)
                        .when_some(hint, Button::tooltip),
                    )
                    .child(
                        div()
                            .debug_selector(|| "loader-shop-more".into())
                            .child(more),
                    ),
            )
    }

    fn shop_warning_dialog(&self, cx: &Context<Self>) -> Dialog {
        let warning = self.shop_warning.as_ref();
        let heading = warning.map_or_else(String::new, |warning| format!("Load {}?", warning.load));
        let quiet = if warning.is_some_and(|warning| warning.quiet) {
            CheckedState::Checked
        } else {
            CheckedState::Unchecked
        };
        Dialog::new("shop-warning", warning.is_some(), heading)
            .alert()
            .description("Make sure you're in the correct map when loading this shop.")
            .footer_start_slot(
                Checkbox::new("shop-warning-quiet", "Don't warn me again", quiet).on_toggle(
                    choose(cx, |view, quiet, _| {
                        if let Some(warning) = &mut view.shop_warning {
                            warning.quiet = quiet;
                        }
                    }),
                ),
            )
            .on_close(change(cx, |view, _| view.shop_warning = None))
            .action(command(
                "shop-warning-cancel",
                "Cancel",
                Variant::Outline,
                change(cx, |view, _| view.shop_warning = None),
            ))
            .action(command(
                "shop-warning-load",
                "Load",
                Variant::Primary,
                change(cx, Self::confirm_shop_load),
            ))
    }

    fn copy_menu(&self, ResultRow(name, amount, id, category): ResultRow) -> DropdownMenu {
        let source = self.source;
        let (copy_id, amount_key) = match source {
            Source::Shop => ("Copy item ID", "price"),
            Source::Quests => ("Copy quest ID", "count"),
            Source::Monsters => ("Copy monster ID", "cell"),
            _ => ("Copy item ID", "quantity"),
        };
        DropdownMenu::new("result-copy")
            .align(Align::End)
            .trigger(
                Button::icon("result-copy-trigger", Icon::Ellipsis, "Copy result details")
                    .size(Size::Small)
                    .variant(Variant::Ghost),
            )
            .items(move |_, _| {
                let json = serde_json::json!({
                    "id": id,
                    "name": name,
                    "category": category,
                    "source": source.name(),
                    amount_key: amount,
                });
                [
                    ("Copy name", "Name copied", name.to_owned()),
                    (copy_id, "ID copied", id.to_string()),
                    ("Copy JSON", "JSON copied", format!("{json:#}")),
                ]
                .into_iter()
                .map(|(label, message, value)| {
                    MenuItem::action(label, move |_, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(value.clone()));
                        Toaster::push(Toast::new(message), cx);
                    })
                })
                .collect()
            })
    }
}

impl Render for Loader {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let wide = window_width(Screen::Loader, window) >= px(824.0);
        let available = self.target.read(cx).available(cx);
        let preview = if available {
            StatusPreview::current(cx)
        } else {
            StatusPreview::Current
        };
        let query = self.search.read(cx).text().trim().to_lowercase();
        let mut results: Vec<_> = self
            .results()
            .iter()
            .copied()
            .filter(|row| {
                row.0.to_lowercase().contains(&query) || row.2.to_string().contains(&query)
            })
            .collect();
        results.sort_by(|a, b| match self.sort.column {
            0 => a.0.cmp(b.0),
            1 => a.1.cmp(&b.1),
            _ => a.2.cmp(&b.2),
        });
        if !self.sort.ascending {
            results.reverse();
        }
        let rows = Rc::new(results);
        let selected = self
            .selected
            .and_then(|id| rows.iter().position(|row| row.2 == id))
            .or_else(|| (!rows.is_empty()).then_some(0));
        let kinds = match self.source {
            Source::Shop => ("Price", "Item ID"),
            Source::Quests => ("Count", "Quest ID"),
            Source::Monsters => ("Cell", "Monster ID"),
            _ => ("Quantity", "Item ID"),
        };
        let picked = rows.clone();
        let table = Table::new(
            "results",
            "Loaded items",
            vec![
                Column::new("Name"),
                Column::new(kinds.0).width(px(88.0)).numeric(),
                Column::new(kinds.1).width(px(92.0)).numeric(),
            ],
            rows.iter()
                .map(|row| {
                    vec![
                        row.0.into(),
                        row.1.to_string().into(),
                        row.2.to_string().into(),
                    ]
                })
                .collect(),
        )
        .sort(Some(self.sort))
        .selected(selected)
        .on_sort(choose(cx, |view, column, _| {
            view.sort = Sort {
                column,
                ascending: view.sort.column != column || !view.sort.ascending,
            }
        }))
        .on_select(choose(cx, move |view, row: usize, _| {
            if let Some(row) = picked.get(row) {
                view.selected = Some(row.2);
                view.detail = true;
            }
        }));
        let list = if rows.is_empty() {
            empty("No matching results", "Try another name or ID.", cx).into_any_element()
        } else {
            scroll("results-scroll")
                .debug_selector(|| "loader-results".into())
                .px(px(16.0))
                .pt(px(8.0))
                .child(table)
                .into_any_element()
        };
        let detail = match selected.and_then(|row| rows.get(row).copied()) {
            Some(row @ ResultRow(name, amount, id, category)) => scroll("item-detail")
                .debug_selector(|| "loader-inspector".into())
                .p(px(16.0))
                .gap(px(12.0))
                .when(!wide, |body| {
                    body.child(
                        command(
                            "back",
                            "Back to results",
                            Variant::Ghost,
                            change(cx, |view, _| view.detail = false),
                        )
                        .self_start(),
                    )
                })
                .child(
                    h_flex()
                        .items_start()
                        .gap(px(8.0))
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .gap(px(2.0))
                                .child(title(name).truncate())
                                .child(meta(category, cx)),
                        )
                        .child(
                            div()
                                .debug_selector(|| "loader-copy-menu".into())
                                .child(self.copy_menu(row)),
                        ),
                )
                .child(
                    command("grab", "Grab", Variant::Primary, move |_, cx| {
                        Toaster::push(Toast::new("Item grabbed").body(name), cx);
                    })
                    .size(Size::Small)
                    .self_start(),
                )
                .child(
                    v_flex()
                        .gap(px(6.0))
                        .child(
                            h_flex()
                                .min_h(px(24.0))
                                .justify_between()
                                .child(meta(kinds.1, cx))
                                .child(div().font_family(SYSTEM_MONO).child(id.to_string())),
                        )
                        .child(
                            h_flex()
                                .min_h(px(24.0))
                                .justify_between()
                                .child(meta(kinds.0, cx))
                                .child(amount.to_string()),
                        )
                        .child(
                            h_flex()
                                .min_h(px(24.0))
                                .justify_between()
                                .child(meta("Source", cx))
                                .child(self.source.name()),
                        ),
                )
                .into_any_element(),
            None => empty("Select a result", "Its details appear here.", cx).into_any_element(),
        };
        let loading = preview == StatusPreview::Working;
        let failed = preview == StatusPreview::Problem;
        let shop = self.source == Source::Shop;
        let problem = self
            .shop_problem
            .filter(|_| shop && preview == StatusPreview::Current);
        let alert = failed || problem.is_some();
        let hint = if failed {
            Some("Showing previous results. Load again to retry.")
        } else {
            problem
        };
        let source_name = self.source.name().to_lowercase();
        let message = match preview {
            StatusPreview::Current => {
                problem.map_or_else(|| format!("{} results", rows.len()), str::to_owned)
            }
            StatusPreview::Working => format!("Loading {source_name}…"),
            StatusPreview::Problem => format!("Couldn't load {source_name}"),
        };
        let title_status = h_flex()
            .id("load-status")
            .debug_selector(|| "loader-title-status".into())
            .role(if alert { Role::Alert } else { Role::Status })
            .aria_label(message.clone())
            .flex_1()
            .min_w_0()
            .justify_end()
            .mr(px(8.0))
            .gap(px(6.0))
            .when(loading || failed, |status| {
                status.debug_selector(|| "status-preview".into())
            })
            .when(loading, |status| {
                status.child(Spinner::new("loading").color(palette(cx).muted_foreground))
            })
            .when_some(hint.filter(|_| failed), |status, hint| {
                status.aria_description(hint)
            })
            .when(alert, |status| {
                status.child(icon(Icon::CircleAlert, palette(cx).destructive))
            })
            .child(
                if alert {
                    div().child(message)
                } else {
                    meta(message, cx)
                }
                .min_w_0()
                .truncate(),
            );
        let tools = toolbar(cx).child(
            Select::new(
                "source",
                "Source",
                Source::ALL.map(Source::name),
                Source::ALL.iter().position(|&source| source == self.source),
            )
            .width(px(144.0))
            .on_select(choose(cx, |view, index: usize, _| {
                view.source = Source::ALL[index];
                view.selected = None;
                view.detail = false;
                view.shop_warning = None;
            })),
        );
        let load = if shop {
            self.shop_controls(!available || loading, hint, cx)
                .into_any_element()
        } else {
            command(
                "load",
                "Load",
                Variant::Outline,
                change(cx, |view, cx| {
                    view.search.update(cx, |input, cx| input.set_text("", cx));
                    Toaster::push(Toast::new("Results loaded").body(view.source.name()), cx);
                }),
            )
            .disabled(!available || loading)
            .when_some(hint, Button::tooltip)
            .into_any_element()
        };
        let tools = tools.child(load).child(spring()).child(
            div()
                .debug_selector(|| "loader-search".into())
                .w(px(224.0))
                .min_w_0()
                .child(self.search.clone()),
        );
        let body = if !available {
            empty("No tabs open", "Open a tab to inspect its items.", cx).into_any_element()
        } else if wide {
            h_flex()
                .items_stretch()
                .flex_1()
                .min_h_0()
                .child(list)
                .child(
                    v_flex()
                        .flex_none()
                        .w(px(320.0))
                        .border_l_1()
                        .border_color(palette(cx).border)
                        .child(detail),
                )
                .into_any_element()
        } else if self.detail && selected.is_some() {
            detail
        } else {
            list
        };
        frame(
            tool_chrome(Screen::Loader, Some(self.target.clone()), cx)
                .when(available, |bar| bar.child(title_status)),
            cx,
        )
        .child(tools)
        .child(body)
        .child(self.shop_warning_dialog(cx))
    }
}

#[cfg(test)]
mod tests {
    use gpui::{AppContext as _, Modifiers, TestAppContext, size};

    use super::*;

    #[gpui::test]
    fn storyboard_shop_menu_uses_the_current_shop_id(cx: &mut TestAppContext) {
        cx.update(|cx| {
            mira_ui::init(cx);
            cx.set_global(mira_ui::Theme::mira(false));
            cx.set_reduce_motion(true);
        });
        let (loader, cx) = cx.add_window_view(|window, cx| {
            window.activate_window();
            let sessions = cx.new(|_| Sessions::default());
            let loader = Loader::new(sessions, cx);
            loader
                .shop_id
                .update(cx, |input, cx| input.set_text("2468", cx));
            loader
        });
        cx.simulate_resize(size(px(720.0), px(504.0)));
        cx.run_until_parked();

        let more = cx.debug_bounds("loader-shop-more").unwrap().center();
        cx.simulate_click(more, Modifiers::none());
        cx.simulate_keystrokes("home enter");
        cx.run_until_parked();
        assert_eq!(
            loader.read_with(cx, |view, _| view
                .shop_warning
                .as_ref()
                .map(|warning| warning.load.clone())),
            Some(Load::HairShop(2468))
        );

        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        assert!(loader.read_with(cx, |view, _| view.shop_warning.is_none()));
    }
}
