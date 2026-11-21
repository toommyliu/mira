use std::{cell::Cell, rc::Rc};

use gpui::{
    Context, Entity, FocusHandle, InteractiveElement as _, IntoElement, KeyUpEvent, Keystroke,
    ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _,
    TestAppContext, VisualTestContext, Window, div, prelude::FluentBuilder as _,
};
use mira_ui::{
    FOCUS_SCOPE,
    components::ShortcutRefusal,
    primitives::{Button, Checkbox, CheckedState, Dialog, DropdownMenu, Icon, MenuItem, Select},
};

struct Rooms {
    focus: FocusHandle,
    selected: Option<usize>,
    picked: Rc<Cell<Option<usize>>>,
}

impl Render for Rooms {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let options = [
            "Yulgar",
            "Battleon",
            "Swordhaven",
            "Safiria",
            "Swordhaven (EU)",
        ]
        .map(SharedString::from);
        let picked = self.picked.clone();
        div()
            .key_context(FOCUS_SCOPE)
            .track_focus(&self.focus)
            .size_full()
            .child(
                Select::new("room", "Room", options.to_vec(), self.selected)
                    .on_select(move |index, _, _| picked.set(Some(index))),
            )
    }
}

fn rooms(selected: Option<usize>, keys: &'static str, cx: &mut TestAppContext) -> Option<usize> {
    cx.update(mira_ui::init);
    let picked = Rc::new(Cell::new(None));
    let (_, cx) = cx.add_window_view(|window, cx| {
        let focus = cx.focus_handle();
        window.activate_window();
        window.focus(&focus, cx);
        Rooms {
            focus,
            selected,
            picked: picked.clone(),
        }
    });
    cx.run_until_parked();
    for key in keys.split(' ') {
        press(cx, key);
    }
    picked.get()
}

fn press(cx: &mut VisualTestContext, key: &'static str) {
    cx.simulate_keystrokes(key);
    cx.simulate_event(KeyUpEvent {
        keystroke: Keystroke::parse(key).unwrap(),
    });
    cx.run_until_parked();
}

#[gpui::test]
fn select_picks_the_next_option_from_the_keyboard(cx: &mut TestAppContext) {
    assert_eq!(rooms(Some(0), "tab enter down enter", cx), Some(1));
}

#[gpui::test]
fn select_jumps_to_the_option_matching_what_was_typed(cx: &mut TestAppContext) {
    assert_eq!(rooms(Some(0), "tab enter s a enter", cx), Some(3));
}

#[gpui::test]
fn select_typeahead_takes_spaces_inside_a_name(cx: &mut TestAppContext) {
    let keys = "tab enter s w o r d h a v e n space enter";
    assert_eq!(rooms(Some(0), keys, cx), Some(4));
}

#[gpui::test]
fn select_typeahead_cycles_through_options_on_a_repeated_letter(cx: &mut TestAppContext) {
    assert_eq!(rooms(Some(0), "tab enter s s s enter", cx), Some(4));
}

#[gpui::test]
fn select_typeahead_forgets_its_prefix_when_the_menu_closes(cx: &mut TestAppContext) {
    let escaped = "tab enter s escape enter space";
    assert_eq!(rooms(Some(0), escaped, cx), Some(0));
    let tabbed = "tab enter s tab shift-tab enter a enter";
    assert_eq!(rooms(Some(0), tabbed, cx), Some(0));
}

#[gpui::test]
fn select_opens_on_the_first_option_without_a_valid_selection(cx: &mut TestAppContext) {
    assert_eq!(rooms(None, "tab enter enter", cx), Some(0));
}

#[gpui::test]
fn select_treats_a_selection_past_its_options_as_none(cx: &mut TestAppContext) {
    assert_eq!(rooms(Some(9), "tab enter enter", cx), Some(0));
}

struct LongSelect {
    focus: FocusHandle,
    selected: usize,
}

impl Render for LongSelect {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let options = (0..40usize).map(|index| {
            let option = mira_ui::primitives::SelectOption::new(format!("Profile {index}"));
            if index == 39 {
                option.start_slot(div().debug_selector(|| "last".into()).child("•"))
            } else {
                option
            }
        });
        div()
            .key_context(FOCUS_SCOPE)
            .track_focus(&self.focus)
            .size_full()
            .child(Select::new(
                "profile",
                "Profile",
                options.collect::<Vec<_>>(),
                Some(self.selected),
            ))
    }
}

fn last_option_after(
    selected: usize,
    keys: &[&'static str],
    window: gpui::Size<gpui::Pixels>,
    cx: &mut TestAppContext,
) -> gpui::Bounds<gpui::Pixels> {
    cx.update(mira_ui::init);
    let (_, cx) = cx.add_window_view(|window, cx| {
        let focus = cx.focus_handle();
        window.activate_window();
        window.focus(&focus, cx);
        LongSelect { focus, selected }
    });
    cx.simulate_resize(window);
    cx.run_until_parked();
    for key in keys {
        press(cx, key);
    }
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    cx.debug_bounds("last").unwrap()
}

#[gpui::test]
fn a_long_select_scrolls_to_the_option_picked_from_the_keyboard(cx: &mut TestAppContext) {
    use gpui::{px, size};
    let last = last_option_after(0, &["tab", "enter", "end"], size(px(400.0), px(600.0)), cx);
    assert!(last.bottom() <= px(600.0), "{last:?}");
}

#[gpui::test]
fn a_long_select_opens_scrolled_to_its_selected_option(cx: &mut TestAppContext) {
    use gpui::{px, size};
    let last = last_option_after(39, &["tab", "enter"], size(px(400.0), px(600.0)), cx);
    assert!(last.bottom() <= px(600.0), "{last:?}");
}

#[gpui::test]
fn a_long_select_fits_a_window_shorter_than_its_menu(cx: &mut TestAppContext) {
    use gpui::{px, size};
    let last = last_option_after(0, &["tab", "enter", "end"], size(px(340.0), px(240.0)), cx);
    assert!(
        last.top() >= px(0.0) && last.bottom() <= px(240.0),
        "{last:?}"
    );
}

struct Sources {
    focus: FocusHandle,
}

impl Render for Sources {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .key_context(FOCUS_SCOPE)
            .track_focus(&self.focus)
            .size_full()
            .child(
                DropdownMenu::new("sources")
                    .trigger(Button::new("sources", "Sources"))
                    .items(|_, _| {
                        (0..30usize)
                            .map(|index| {
                                let item = MenuItem::submenu(
                                    format!("~/scripts/folder-{index}"),
                                    vec![MenuItem::action("Show in Finder", |_, _| {})],
                                );
                                if index == 29 {
                                    item.start_slot(div().debug_selector(|| "last".into()))
                                } else {
                                    item
                                }
                            })
                            .collect()
                    }),
            )
    }
}

#[gpui::test]
fn a_long_menu_with_submenus_scrolls_inside_the_window(cx: &mut TestAppContext) {
    use gpui::{px, size};
    cx.update(mira_ui::init);
    let (_, cx) = cx.add_window_view(|window, cx| {
        let focus = cx.focus_handle();
        window.activate_window();
        window.focus(&focus, cx);
        Sources { focus }
    });
    cx.simulate_resize(size(px(600.0), px(360.0)));
    cx.run_until_parked();
    for key in ["tab", "enter", "end"] {
        press(cx, key);
    }
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    let last = cx.debug_bounds("last").unwrap();
    assert!(
        last.top() >= px(0.0) && last.bottom() <= px(360.0),
        "{last:?}"
    );
}

struct ProfileRow {
    focus: FocusHandle,
    picked: Rc<Cell<Option<&'static str>>>,
}

impl Render for ProfileRow {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let picked = self.picked.clone();
        div()
            .key_context(FOCUS_SCOPE)
            .track_focus(&self.focus)
            .size_full()
            .child(div().w(gpui::px(200.0)).child(
                DropdownMenu::row("profile", "Profile", "Generic", cx).items(move |_, _| {
                    ["Generic", "Daily boss", "Ultra dage", "Ultra speaker"]
                        .map(|name| {
                            let picked = picked.clone();
                            MenuItem::radio(name, name == "Generic", move |_, _| {
                                picked.set(Some(name))
                            })
                        })
                        .into()
                }),
            ))
    }
}

#[gpui::test]
fn typing_a_name_picks_it_from_a_menu_row(cx: &mut TestAppContext) {
    cx.update(mira_ui::init);
    let picked = Rc::new(Cell::new(None));
    let (_, cx) = cx.add_window_view(|window, cx| {
        let focus = cx.focus_handle();
        window.activate_window();
        window.focus(&focus, cx);
        ProfileRow {
            focus,
            picked: picked.clone(),
        }
    });
    cx.run_until_parked();
    for key in ["tab", "enter", "u", "u", "enter"] {
        press(cx, key);
    }
    assert_eq!(picked.get(), Some("Ultra speaker"));
}

struct RenderingRow {
    disabled: bool,
}

impl Render for RenderingRow {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().p(gpui::px(40.0)).child(
            div().w(gpui::px(200.0)).child(
                DropdownMenu::row("rendering", "Rendering", "Full", cx)
                    .disabled(self.disabled)
                    .items(|_, _| {
                        vec![
                            MenuItem::radio("Full", true, |_, _| {})
                                .start_slot(div().debug_selector(|| "full".into())),
                            MenuItem::radio("Minimal", false, |_, _| {}),
                        ]
                    }),
            ),
        )
    }
}

struct PickedRendering {
    focus: FocusHandle,
    picked: Rc<Cell<Option<&'static str>>>,
}

impl Render for PickedRendering {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let picked = self.picked.clone();
        div()
            .key_context(FOCUS_SCOPE)
            .track_focus(&self.focus)
            .size_full()
            .p(gpui::px(40.0))
            .child(div().w(gpui::px(200.0)).child(
                DropdownMenu::row("rendering", "Rendering", "Full", cx).items(move |_, _| {
                    ["Full", "Minimal"]
                        .map(|mode| {
                            let picked = picked.clone();
                            MenuItem::radio(mode, mode == "Full", move |_, _| {
                                picked.set(Some(mode))
                            })
                            .start_slot(div().debug_selector(move || mode.to_lowercase()))
                        })
                        .into()
                }),
            ))
    }
}

#[derive(PartialEq)]
enum Pointer {
    Away,
    Hover,
    Click,
}

fn rendering_window<'a>(
    picked: Rc<Cell<Option<&'static str>>>,
    cx: &'a mut TestAppContext,
) -> &'a mut VisualTestContext {
    let (_, cx) = cx.add_window_view(|window, cx| {
        let focus = cx.focus_handle();
        window.activate_window();
        window.focus(&focus, cx);
        PickedRendering { focus, picked }
    });
    cx.run_until_parked();
    cx
}

const RENDERING_ROW: (f32, f32) = (100.0, 54.0);

fn hover_rendering_row(cx: &mut VisualTestContext) {
    use gpui::{MouseMoveEvent, point, px};
    let (x, y) = RENDERING_ROW;
    for x in [x, x + 1.0] {
        cx.simulate_event(MouseMoveEvent {
            position: point(px(x), px(y)),
            ..Default::default()
        });
        cx.run_until_parked();
    }
}

fn pick_rendering(
    pointer: Pointer,
    keys: &[&'static str],
    cx: &mut TestAppContext,
) -> Option<&'static str> {
    use gpui::{Modifiers, point, px};
    cx.update(|cx| {
        mira_ui::init(cx);
        cx.set_reduce_motion(true);
    });
    let picked = Rc::new(Cell::new(None));
    let cx = rendering_window(picked.clone(), cx);
    if pointer != Pointer::Away {
        hover_rendering_row(cx);
    }
    if pointer == Pointer::Click {
        let (x, y) = RENDERING_ROW;
        cx.simulate_click(point(px(x + 1.0), px(y)), Modifiers::none());
        cx.run_until_parked();
    }
    for key in keys {
        press(cx, key);
    }
    picked.get()
}

#[gpui::test]
fn shift_keeps_a_menu_row_open_after_a_pick(cx: &mut TestAppContext) {
    use gpui::{Modifiers, point};
    cx.update(|cx| {
        mira_ui::init(cx);
        cx.set_reduce_motion(true);
    });
    let picked = Rc::new(Cell::new(None));
    let cx = rendering_window(picked.clone(), cx);
    press(cx, "tab");
    press(cx, "enter");
    press(cx, "down");
    press(cx, "shift-enter");
    assert_eq!(picked.take(), Some("Minimal"));
    let full = cx
        .debug_bounds("full")
        .expect("Shift-Return keeps the menu open");
    cx.simulate_click(point(full.left(), full.center().y), Modifiers::shift());
    cx.run_until_parked();
    assert_eq!(picked.take(), Some("Full"));
    let minimal = cx
        .debug_bounds("minimal")
        .expect("Shift-click keeps it open too");
    cx.simulate_click(point(minimal.left(), minimal.center().y), Modifiers::none());
    cx.run_until_parked();
    assert_eq!(picked.take(), Some("Minimal"));
    assert!(
        cx.debug_bounds("minimal").is_none(),
        "a plain pick closes it"
    );
}

#[gpui::test]
fn clicking_a_menu_row_its_hover_opened_keeps_the_menu_open(cx: &mut TestAppContext) {
    assert_eq!(
        pick_rendering(Pointer::Click, &["down", "down", "enter"], cx),
        Some("Minimal")
    );
}

#[gpui::test]
fn a_menu_row_opens_in_place_from_the_keyboard_as_on_hover(cx: &mut TestAppContext) {
    cx.update(mira_ui::init);
    let cx = rendering_window(Rc::default(), cx);
    press(cx, "tab");
    press(cx, "enter");
    let keyed = cx.debug_bounds("full").expect("Return opens the menu");
    press(cx, "escape");
    hover_rendering_row(cx);
    let hovered = cx.debug_bounds("full").expect("hovering opens the menu");
    assert_eq!(
        keyed, hovered,
        "the menu appears where it rests, with no slide"
    );
}

#[gpui::test]
fn a_menu_row_opens_and_picks_from_the_keyboard(cx: &mut TestAppContext) {
    assert_eq!(
        pick_rendering(Pointer::Away, &["tab", "enter", "down", "enter"], cx),
        Some("Minimal")
    );
}

#[gpui::test]
fn right_opens_a_menu_row_and_left_returns_to_it(cx: &mut TestAppContext) {
    assert_eq!(
        pick_rendering(
            Pointer::Away,
            &["tab", "right", "down", "left", "right", "enter"],
            cx
        ),
        Some("Full"),
        "Left closes the menu, so Right reopens it on its first choice"
    );
}

#[gpui::test]
fn left_from_a_menu_opened_by_hover_lands_on_its_row(cx: &mut TestAppContext) {
    assert_eq!(
        pick_rendering(Pointer::Hover, &["left", "enter", "enter"], cx),
        Some("Full"),
        "after Left, Return reopens the row's menu instead of acting elsewhere"
    );
}

#[gpui::test]
fn a_menu_row_opened_by_hover_takes_the_arrow_keys(cx: &mut TestAppContext) {
    assert_eq!(
        pick_rendering(Pointer::Hover, &["down", "down", "enter"], cx),
        Some("Minimal")
    );
}

#[gpui::test]
fn hovering_a_disabled_menu_row_leaves_its_menu_closed(cx: &mut TestAppContext) {
    use gpui::{MouseMoveEvent, point, px};
    cx.update(|cx| {
        mira_ui::init(cx);
        cx.set_reduce_motion(true);
    });
    let (_, cx) = cx.add_window_view(|_, _| RenderingRow { disabled: true });
    cx.run_until_parked();
    for x in [100.0, 101.0] {
        cx.simulate_event(MouseMoveEvent {
            position: point(px(x), px(54.0)),
            ..Default::default()
        });
        cx.run_until_parked();
    }
    assert!(cx.debug_bounds("full").is_none());
}

#[gpui::test]
fn a_menu_row_opens_beside_itself_on_hover_and_closes_on_another_row(cx: &mut TestAppContext) {
    use gpui::{MouseMoveEvent, Pixels, Point, point, px};
    cx.update(|cx| {
        mira_ui::init(cx);
        cx.set_reduce_motion(true);
    });
    let (_, cx) = cx.add_window_view(|_, _| RenderingRow { disabled: false });
    cx.run_until_parked();
    let pointer = |cx: &mut VisualTestContext, at: Point<Pixels>| {
        for position in [at, at + point(px(1.0), px(0.0))] {
            cx.simulate_event(MouseMoveEvent {
                position,
                ..Default::default()
            });
            cx.run_until_parked();
        }
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
    };
    assert!(cx.debug_bounds("full").is_none());
    let row = point(px(100.0), px(54.0));
    pointer(cx, row);
    let full = cx
        .debug_bounds("full")
        .expect("hovering the row opens its menu");
    assert!(
        full.top() <= row.y && full.bottom() >= row.y && full.left() >= px(244.0),
        "the menu opens beside the row with its first choice level with it: {full:?}"
    );
    pointer(cx, point(px(242.0), row.y));
    pointer(cx, point(full.left() + px(40.0), full.top()));
    assert!(
        cx.debug_bounds("full").is_some(),
        "crossing from the row into the menu keeps it open"
    );
    pointer(cx, point(px(100.0), px(120.0)));
    assert!(
        cx.debug_bounds("full").is_none(),
        "moving to another row closes it at once"
    );
}

struct Servers {
    focus: FocusHandle,
}

impl Render for Servers {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .key_context(FOCUS_SCOPE)
            .track_focus(&self.focus)
            .size_full()
            .child(div().w(gpui::px(200.0)).child(
                DropdownMenu::row("server", "Server", "Server 1", cx).items(|_, _| {
                    let mut items = vec![
                        MenuItem::action("Their own servers", |_, _| {}).pinned(),
                        MenuItem::separator().pinned(),
                    ];
                    items.extend((0..30usize).map(|index| {
                        let item =
                            MenuItem::action(format!("A long server name {index}"), |_, _| {});
                        if index == 29 {
                            item.start_slot(div().debug_selector(|| "last".into()))
                        } else {
                            item
                        }
                    }));
                    items
                }),
            ))
    }
}

#[gpui::test]
fn a_long_menu_with_pinned_rows_fits_a_short_window(cx: &mut TestAppContext) {
    use gpui::{px, size};
    cx.update(mira_ui::init);
    let (_, cx) = cx.add_window_view(|window, cx| {
        let focus = cx.focus_handle();
        window.activate_window();
        window.focus(&focus, cx);
        Servers { focus }
    });
    cx.simulate_resize(size(px(600.0), px(240.0)));
    cx.run_until_parked();
    for key in ["tab", "enter", "end"] {
        press(cx, key);
    }
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    let last = cx.debug_bounds("last").unwrap();
    assert!(
        last.top() >= px(0.0) && last.bottom() <= px(240.0),
        "{last:?}"
    );
}

struct QueueOnAPage {
    page: gpui::ScrollHandle,
}

impl Render for QueueOnAPage {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("page")
            .h(gpui::px(200.0))
            .overflow_y_scroll()
            .track_scroll(&self.page)
            .child(
                div()
                    .debug_selector(|| "queue".into())
                    .child(mira_ui::primitives::List::new(
                        "queue",
                        "Queue",
                        20,
                        |index| {
                            mira_ui::primitives::ListItem::new(format!("%xt%zm%packet{index}%"))
                        },
                    )),
            )
    }
}

#[gpui::test]
fn scrolling_over_a_content_height_list_scrolls_the_page(cx: &mut TestAppContext) {
    use gpui::{Modifiers, ScrollDelta, ScrollEvent, TouchPhase, point, px};
    cx.update(mira_ui::init);
    let page = gpui::ScrollHandle::new();
    let (_, cx) = cx.add_window_view(|_, _| QueueOnAPage { page: page.clone() });
    cx.run_until_parked();
    let over = cx.debug_bounds("queue").unwrap().origin + point(px(40.0), px(40.0));
    cx.simulate_event(ScrollEvent {
        position: over,
        delta: ScrollDelta::Pixels(point(px(0.0), px(-120.0))),
        modifiers: Modifiers::none(),
        touch_phase: TouchPhase::Moved,
        source: Default::default(),
    });
    cx.run_until_parked();
    assert!(page.offset().y < px(0.0), "{:?}", page.offset());
}

struct Confirm {
    focus: FocusHandle,
    pressed: Rc<Cell<Option<&'static str>>>,
}

impl Render for Confirm {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let press = |name: &'static str| {
            let pressed = self.pressed.clone();
            move |_: &gpui::ClickEvent, _: &mut Window, _: &mut gpui::App| pressed.set(Some(name))
        };
        div().track_focus(&self.focus).size_full().child(
            Dialog::new("confirm", true, "Leave the room?")
                .action(Button::new("stay", "Stay").on_click(press("stay")))
                .action(Button::new("leave", "Leave").on_click(press("leave"))),
        )
    }
}

#[gpui::test]
fn dialog_moves_tab_between_its_controls_without_a_focus_scope(cx: &mut TestAppContext) {
    cx.update(mira_ui::init);
    let pressed = Rc::new(Cell::new(None));
    let (_, cx) = cx.add_window_view(|window, cx| {
        let focus = cx.focus_handle();
        window.activate_window();
        window.focus(&focus, cx);
        Confirm {
            focus,
            pressed: pressed.clone(),
        }
    });
    cx.run_until_parked();
    press(cx, "enter");
    assert_eq!(pressed.take(), Some("stay"));
    press(cx, "tab");
    press(cx, "enter");
    assert_eq!(pressed.get(), Some("leave"));
}

struct SaveChanges {
    pressed: Rc<Cell<Option<&'static str>>>,
}

impl Render for SaveChanges {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let press = |name: &'static str| {
            let pressed = self.pressed.clone();
            move |_: &gpui::ClickEvent, _: &mut Window, _: &mut gpui::App| pressed.set(Some(name))
        };
        div().size_full().child(
            Dialog::new("unsaved", true, "Save your changes?")
                .alert()
                .footer_start_slot(Button::new("discard", "Don't save").on_click(press("discard")))
                .action(Button::new("cancel", "Cancel").on_click(press("cancel")))
                .action(Button::new("save", "Save all").on_click(press("save"))),
        )
    }
}

#[gpui::test]
fn return_in_a_dialog_never_starts_on_its_footer_start_control(cx: &mut TestAppContext) {
    cx.update(mira_ui::init);
    let pressed = Rc::new(Cell::new(None));
    let (_, cx) = cx.add_window_view(|window, _| {
        window.activate_window();
        SaveChanges {
            pressed: pressed.clone(),
        }
    });
    cx.run_until_parked();
    press(cx, "enter");
    assert_eq!(pressed.get(), Some("cancel"));
}

struct BoostPicker;

impl Render for BoostPicker {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(
            Dialog::new("boosts", true, "Add boosts you own")
                .width(gpui::px(420.0))
                .body(mira_ui::v_flex().children((0..20usize).map(|index| {
                    div()
                        .when(index == 12, |row| row.debug_selector(|| "twelfth".into()))
                        .child(Checkbox::new(
                            ("boost", index),
                            format!("Boost {index}"),
                            CheckedState::Unchecked,
                        ))
                })))
                .action(
                    div()
                        .debug_selector(|| "add".into())
                        .child(Button::new("add", "Add")),
                ),
        )
    }
}

#[gpui::test]
fn dialog_keeps_its_actions_inside_a_window_smaller_than_it(cx: &mut TestAppContext) {
    use gpui::{px, size};
    cx.update(mira_ui::init);
    let (_, cx) = cx.add_window_view(|_, _| BoostPicker);
    cx.simulate_resize(size(px(340.0), px(280.0)));
    cx.run_until_parked();
    let add = cx.debug_bounds("add").unwrap();
    assert!(add.left() >= px(0.0) && add.right() <= px(340.0), "{add:?}");
    assert!(add.top() >= px(0.0) && add.bottom() <= px(280.0), "{add:?}");
}

#[gpui::test]
fn tabbing_through_a_long_dialog_scrolls_the_focused_control_into_view(cx: &mut TestAppContext) {
    use gpui::{px, size};
    cx.update(mira_ui::init);
    let (_, cx) = cx.add_window_view(|window, _| {
        window.activate_window();
        BoostPicker
    });
    cx.simulate_resize(size(px(340.0), px(280.0)));
    cx.run_until_parked();
    for _ in 0..12 {
        press(cx, "tab");
    }
    let twelfth = cx.debug_bounds("twelfth").unwrap();
    let add = cx.debug_bounds("add").unwrap();
    assert!(
        twelfth.top() >= px(0.0) && twelfth.bottom() <= add.top(),
        "{twelfth:?}"
    );
}

struct AddAccount {
    username: gpui::Entity<mira_ui::primitives::Input>,
    closed: Rc<Cell<bool>>,
}

impl Render for AddAccount {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(
            Dialog::new("add-account", true, "Add account")
                .body(self.username.clone())
                .action(Button::new("save", "Save"))
                .on_close({
                    let closed = self.closed.clone();
                    move |_, _| closed.set(true)
                }),
        )
    }
}

#[gpui::test]
fn dialog_opens_on_its_first_field_and_reaches_close_last(cx: &mut TestAppContext) {
    use gpui::{AppContext as _, Focusable as _};
    cx.update(mira_ui::init);
    let closed = Rc::new(Cell::new(false));
    let (view, cx) = cx.add_window_view(|window, cx| {
        window.activate_window();
        AddAccount {
            username: cx.new(|cx| mira_ui::primitives::Input::new("Username", cx)),
            closed: closed.clone(),
        }
    });
    cx.run_until_parked();
    let username = view.read_with(cx, |view, cx| view.username.focus_handle(cx));
    assert!(cx.update(|window, _| username.is_focused(window)));
    for key in ["tab", "tab", "enter"] {
        press(cx, key);
    }
    assert!(closed.get());
}

struct SearchAndOptions {
    search: gpui::Entity<mira_ui::primitives::Input>,
}

impl Render for SearchAndOptions {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(
            Dialog::new("scripts", true, "Run script")
                .header_end_slot(div().w(gpui::px(200.0)).child(self.search.clone()))
                .body(
                    div().debug_selector(|| "options".into()).child(
                        mira_ui::primitives::Popover::new("options")
                            .trigger(mira_ui::primitives::Button::new("options", "Options"))
                            .content(|_, _, _| {
                                mira_ui::primitives::Switch::new(
                                    "safe",
                                    "Safe start and stop",
                                    false,
                                )
                                .into_any_element()
                            }),
                    ),
                ),
        )
    }
}

#[gpui::test]
fn toggling_a_popover_in_a_dialog_keeps_focus_off_the_dialog_search(cx: &mut TestAppContext) {
    use gpui::{AppContext as _, Focusable as _};
    cx.update(mira_ui::init);
    let (view, cx) = cx.add_window_view(|window, cx| {
        window.activate_window();
        SearchAndOptions {
            search: cx.new(|cx| mira_ui::primitives::Input::new("Search", cx).search()),
        }
    });
    cx.run_until_parked();
    let trigger = cx.debug_bounds("options").unwrap().center();
    let search = view.read_with(cx, |view, cx| view.search.focus_handle(cx));
    for _ in 0..2 {
        cx.simulate_click(trigger, gpui::Modifiers::none());
        cx.run_until_parked();
        assert!(!cx.update(|window, _| search.is_focused(window)));
    }
}

struct OptionsThenGo {
    focus: FocusHandle,
    pressed: Rc<Cell<bool>>,
    toggled: Rc<std::cell::RefCell<Vec<&'static str>>>,
}

impl Render for OptionsThenGo {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let pressed = self.pressed.clone();
        let toggled = self.toggled.clone();
        div()
            .key_context(FOCUS_SCOPE)
            .track_focus(&self.focus)
            .size_full()
            .child(
                mira_ui::primitives::Popover::new("options")
                    .trigger(Button::new("options", "Options"))
                    .content(move |_, _, _| {
                        let switch = |id: &'static str| {
                            let toggled = toggled.clone();
                            mira_ui::primitives::Switch::new(id, id, false)
                                .on_toggle(move |_, _, _| toggled.borrow_mut().push(id))
                        };
                        div()
                            .debug_selector(|| "options-panel".into())
                            .child(switch("safe"))
                            .child(switch("restart"))
                            .into_any_element()
                    }),
            )
            .child(Button::new("go", "Go").on_click(move |_, _, _| pressed.set(true)))
    }
}

#[gpui::test]
fn tab_cycles_inside_an_open_popover_and_escape_still_closes_it(cx: &mut TestAppContext) {
    cx.update(|cx| {
        mira_ui::init(cx);
        cx.set_reduce_motion(true);
    });
    let pressed = Rc::new(Cell::new(false));
    let toggled = Rc::default();
    let (_, cx) = cx.add_window_view(|window, cx| {
        let focus = cx.focus_handle();
        window.activate_window();
        window.focus(&focus, cx);
        OptionsThenGo {
            focus,
            pressed: pressed.clone(),
            toggled: Rc::clone(&toggled),
        }
    });
    cx.run_until_parked();
    for key in [
        "tab",
        "enter",
        "tab",
        "space",
        "tab",
        "space",
        "shift-tab",
        "space",
        "escape",
    ] {
        press(cx, key);
    }
    assert_eq!(*toggled.borrow(), ["restart", "safe", "restart"]);
    assert!(cx.debug_bounds("options-panel").is_none());
    assert!(!pressed.get());
}

struct ServerInPopover {
    picked: Rc<Cell<Option<usize>>>,
}

impl Render for ServerInPopover {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let picked = self.picked.clone();
        div().size_full().child(
            mira_ui::primitives::Popover::new("relogin")
                .default_open(true)
                .trigger(Button::new("relogin", "Auto relogin"))
                .content(move |_, _, _| {
                    let picked = picked.clone();
                    let servers =
                        (0..12).map(|index| SharedString::from(format!("Server {index}")));
                    div()
                        .debug_selector(|| "server".into())
                        .child(
                            Select::new("server", "Server", servers, Some(0))
                                .on_select(move |index, _, _| picked.set(Some(index))),
                        )
                        .into_any_element()
                }),
        )
    }
}

#[gpui::test]
fn a_select_in_a_popover_picks_options_that_hang_below_the_popover(cx: &mut TestAppContext) {
    use gpui::{Modifiers, MouseButton, point, px};
    cx.update(mira_ui::init);
    let picked = Rc::new(Cell::new(None));
    let (_, cx) = cx.add_window_view(|window, _| {
        window.activate_window();
        ServerInPopover {
            picked: picked.clone(),
        }
    });
    cx.run_until_parked();
    let select = cx.debug_bounds("server").unwrap();
    cx.simulate_click(select.center(), Modifiers::none());
    cx.run_until_parked();
    let eighth = point(
        select.center().x,
        select.bottom() + px(4.0 + 4.0 + 28.0 * 8.0 + 14.0),
    );
    cx.simulate_mouse_down(eighth, MouseButton::Left, Modifiers::none());
    std::thread::sleep(std::time::Duration::from_millis(150));
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    cx.simulate_mouse_up(eighth, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    assert_eq!(picked.get(), Some(8));
}

struct RoomThenGo {
    focus: FocusHandle,
    pressed: Rc<Cell<bool>>,
}

impl Render for RoomThenGo {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let options = ["Yulgar", "Battleon"].map(SharedString::from);
        let pressed = self.pressed.clone();
        div()
            .key_context(FOCUS_SCOPE)
            .track_focus(&self.focus)
            .size_full()
            .child(Select::new("room", "Room", options.to_vec(), None))
            .child(Button::new("go", "Go").on_click(move |_, _, _| pressed.set(true)))
    }
}

#[gpui::test]
fn tab_closes_an_open_select_and_moves_on(cx: &mut TestAppContext) {
    cx.update(mira_ui::init);
    let pressed = Rc::new(Cell::new(false));
    let (_, cx) = cx.add_window_view(|window, cx| {
        let focus = cx.focus_handle();
        window.activate_window();
        window.focus(&focus, cx);
        RoomThenGo {
            focus,
            pressed: pressed.clone(),
        }
    });
    cx.run_until_parked();
    for key in ["tab", "enter", "tab", "enter"] {
        press(cx, key);
    }
    assert!(pressed.get());
}

struct Advanced {
    focus: FocusHandle,
    open: bool,
}

impl Render for Advanced {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.entity().downgrade();
        div()
            .key_context(FOCUS_SCOPE)
            .track_focus(&self.focus)
            .size_full()
            .child(
                mira_ui::components::Disclosure::new("advanced", "Advanced", self.open).on_toggle(
                    move |open, _, cx| {
                        let _ = this.update(cx, |view, cx| {
                            view.open = open;
                            cx.notify();
                        });
                    },
                ),
            )
    }
}

#[gpui::test]
fn a_disclosure_toggles_once_per_key_press(cx: &mut TestAppContext) {
    cx.update(mira_ui::init);
    let (view, cx) = cx.add_window_view(|window, cx| {
        let focus = cx.focus_handle();
        window.activate_window();
        window.focus(&focus, cx);
        Advanced { focus, open: false }
    });
    cx.run_until_parked();
    press(cx, "tab");
    for (key, open) in [("space", true), ("enter", false), ("enter", true)] {
        press(cx, key);
        assert_eq!(view.read_with(cx, |view, _| view.open), open, "after {key}");
    }
}

gpui::actions!(test, [CloseWindow, Quit]);

struct Shortcut {
    focus: FocusHandle,
    recorded: Rc<Cell<Option<Keystroke>>>,
    next: Rc<Cell<Option<Keystroke>>>,
}

impl Render for Shortcut {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let (recorded, next) = (self.recorded.clone(), self.next.clone());
        div()
            .key_context(FOCUS_SCOPE)
            .track_focus(&self.focus)
            .size_full()
            .child(
                mira_ui::components::ShortcutField::new("hotkey", "Hotkey", None).on_change(
                    move |keystroke, _, _| {
                        if keystroke == Keystroke::parse("cmd-q").ok() {
                            return Err(ShortcutRefusal::Reserved);
                        }
                        if keystroke == Keystroke::parse("cmd-b").ok() {
                            return Err(ShortcutRefusal::Taken("Used by Bank".into()));
                        }
                        recorded.set(keystroke);
                        Ok(())
                    },
                ),
            )
            .child(
                mira_ui::components::ShortcutField::new("next-hotkey", "Next hotkey", None)
                    .on_change(move |keystroke, _, _| {
                        next.set(keystroke);
                        Ok(())
                    }),
            )
    }
}

#[gpui::test]
fn shortcut_field_records_combinations_the_app_binds(cx: &mut TestAppContext) {
    let fired = Rc::new(Cell::new(false));
    let quit = Rc::new(Cell::new(false));
    cx.update(|cx| {
        mira_ui::init(cx);
        cx.bind_keys([
            gpui::KeyBinding::new("cmd-w", CloseWindow, None),
            gpui::KeyBinding::new("ctrl-shift-tab", CloseWindow, None),
            gpui::KeyBinding::new("cmd-q", Quit, None),
        ]);
        let fired = fired.clone();
        cx.on_action(move |_: &CloseWindow, _| fired.set(true));
        let quit = quit.clone();
        cx.on_action(move |_: &Quit, _| quit.set(true));
    });
    let recorded = Rc::new(Cell::new(None));
    let next = Rc::new(Cell::new(None));
    let (view, cx) = cx.add_window_view(|window, cx| {
        let focus = cx.focus_handle();
        window.activate_window();
        window.focus(&focus, cx);
        Shortcut {
            focus,
            recorded: recorded.clone(),
            next: next.clone(),
        }
    });
    for combination in ["cmd-w", "ctrl-shift-tab"] {
        cx.update(|window, cx| {
            let focus = view.read(cx).focus.clone();
            window.focus(&focus, cx);
        });
        cx.run_until_parked();
        press(cx, "tab");
        press(cx, combination);
        assert!(!fired.get(), "{combination} reached the app");
        assert_eq!(
            recorded.take(),
            Some(Keystroke::parse(combination).unwrap())
        );
    }

    cx.update(|window, cx| {
        let focus = view.read(cx).focus.clone();
        window.focus(&focus, cx);
    });
    cx.run_until_parked();
    press(cx, "tab");
    press(cx, "cmd-q");
    assert!(quit.get(), "a reserved combination didn't reach the app");
    assert_eq!(recorded.take(), None);
    assert!(cx.debug_bounds("shortcut-error").is_none());
    press(cx, "cmd-k");
    assert_eq!(
        recorded.take(),
        Some(Keystroke::parse("cmd-k").unwrap()),
        "the field stopped recording after a reserved combination"
    );

    cx.update(|window, cx| {
        let focus = view.read(cx).focus.clone();
        window.focus(&focus, cx);
    });
    cx.run_until_parked();
    press(cx, "tab");
    recorded.set(Some(Keystroke::parse("cmd-w").unwrap()));
    press(cx, "backspace");
    assert_eq!(recorded.take(), None);
    recorded.set(Some(Keystroke::parse("cmd-w").unwrap()));
    press(cx, "cmd-k");
    assert_eq!(
        recorded.take(),
        Some(Keystroke::parse("cmd-w").unwrap()),
        "clearing should stop recording"
    );
    press(cx, "tab");
    press(cx, "cmd-j");
    assert_eq!(
        next.take(),
        Some(Keystroke::parse("cmd-j").unwrap()),
        "tab should reach the next field after clearing"
    );
}

#[gpui::test]
fn input_keeps_the_cursor_on_a_character_boundary_while_composing(cx: &mut TestAppContext) {
    use gpui::{EntityInputHandler as _, Focusable as _};
    use mira_ui::primitives::Input;
    cx.update(mira_ui::init);
    let (input, cx) = cx.add_window_view(|window, cx| {
        window.activate_window();
        Input::new("Name", cx)
    });
    cx.update(|window, cx| window.focus(&input.focus_handle(cx), cx));
    cx.simulate_input("a");
    cx.update(|window, cx| {
        input.update(cx, |input, cx| {
            input.replace_and_mark_text_in_range(None, "\u{3042}", Some(1..1), window, cx);
            input.unmark_text(window, cx);
        })
    });
    press(cx, "backspace");
    assert_eq!(
        input.read_with(cx, |input, _| input.text().to_string()),
        "a"
    );
}

fn focused_input(
    build: impl FnOnce(mira_ui::primitives::Input) -> mira_ui::primitives::Input + 'static,
    cx: &mut TestAppContext,
) -> (
    gpui::Entity<mira_ui::primitives::Input>,
    &mut VisualTestContext,
) {
    use gpui::Focusable as _;
    cx.update(mira_ui::init);
    let (input, cx) = cx.add_window_view(|window, cx| {
        window.activate_window();
        build(mira_ui::primitives::Input::new("Field", cx))
    });
    cx.update(|window, cx| window.focus(&input.focus_handle(cx), cx));
    cx.run_until_parked();
    (input, cx)
}

fn text(input: &gpui::Entity<mira_ui::primitives::Input>, cx: &mut VisualTestContext) -> String {
    input.read_with(cx, |input, _| input.text().to_string())
}

#[gpui::test]
fn clearing_a_search_keeps_it_ready_for_the_next_query(cx: &mut TestAppContext) {
    let (input, cx) = focused_input(|input| input.search(), cx);
    cx.simulate_input("Twig");
    cx.run_until_parked();
    let clear = cx.debug_bounds("clear-search").unwrap().center();
    cx.simulate_click(clear, gpui::Modifiers::none());
    cx.run_until_parked();
    assert_eq!(text(&input, cx), "");

    for key in ["enter", "space"] {
        cx.simulate_input("Yulgar");
        assert_eq!(text(&input, cx), "Yulgar");
        cx.run_until_parked();
        cx.update(|window, cx| window.focus_next(cx));
        cx.run_until_parked();
        press(cx, key);
        assert_eq!(text(&input, cx), "");
    }
    cx.simulate_input("Battleon");
    assert_eq!(text(&input, cx), "Battleon");
    press(cx, "escape");
    assert_eq!(text(&input, cx), "");
}

#[gpui::test]
fn input_moves_and_deletes_by_word(cx: &mut TestAppContext) {
    let (input, cx) = focused_input(|input| input, cx);
    cx.simulate_input("kill the boss");
    press(cx, "alt-left");
    cx.simulate_input("big ");
    assert_eq!(text(&input, cx), "kill the big boss");
    press(cx, "alt-backspace");
    assert_eq!(text(&input, cx), "kill the boss");
    press(cx, "cmd-backspace");
    assert_eq!(text(&input, cx), "boss");
}

#[gpui::test]
fn input_undoes_typing_as_one_step_and_redoes_it(cx: &mut TestAppContext) {
    let (input, cx) = focused_input(|input| input, cx);
    cx.simulate_input("Yulgar");
    press(cx, "alt-backspace");
    cx.simulate_input("Battleon");
    press(cx, "cmd-z");
    assert_eq!(text(&input, cx), "");
    press(cx, "cmd-z");
    assert_eq!(text(&input, cx), "Yulgar");
    press(cx, "cmd-shift-z");
    assert_eq!(text(&input, cx), "");
    press(cx, "cmd-shift-z");
    assert_eq!(text(&input, cx), "Battleon");
}

#[gpui::test]
fn numeric_input_takes_digits_and_steps_within_its_range(cx: &mut TestAppContext) {
    use mira_ui::primitives::Numeric;
    let (input, cx) = focused_input(
        |input| {
            input.numeric(Numeric {
                min: 0,
                max: 30,
                step: 1,
            })
        },
        cx,
    );
    cx.simulate_input("2a5");
    assert_eq!(text(&input, cx), "25");
    press(cx, "up");
    assert_eq!(text(&input, cx), "26");
    press(cx, "shift-up");
    assert_eq!(text(&input, cx), "30");
    press(cx, "shift-down");
    press(cx, "shift-down");
    press(cx, "shift-down");
    assert_eq!(text(&input, cx), "0");
}

#[gpui::test]
fn password_input_hides_its_text_until_revealed_and_keeps_it_off_the_clipboard(
    cx: &mut TestAppContext,
) {
    use gpui::{Bounds, ClipboardItem, EntityInputHandler as _, Modifiers, point, px};
    let (input, cx) = focused_input(|input| input.password(), cx);
    cx.simulate_input("ab");
    cx.update(|window, cx| {
        input.update(cx, |input, cx| {
            input.replace_text_in_range(None, "\u{1F642}", window, cx)
        })
    });
    cx.run_until_parked();
    let caret_x = |utf16: usize, cx: &mut VisualTestContext| {
        cx.update(|window, cx| {
            input.update(cx, |input, cx| {
                let bounds = input.bounds_for_range(utf16..utf16, Bounds::default(), window, cx);
                f32::from(bounds.unwrap().left())
            })
        })
    };
    let bullet = caret_x(1, cx);
    assert!((caret_x(4, cx) - 3.0 * bullet).abs() < 0.01);

    press(cx, "home");
    cx.simulate_click(
        point(px(300.0), mira_ui::theme::CONTROL_MD / 2.0),
        Modifiers::none(),
    );
    cx.simulate_input("c");
    press(cx, "shift-left");
    cx.write_to_clipboard(ClipboardItem::new_string("unchanged".into()));
    press(cx, "cmd-c");
    press(cx, "cmd-x");
    assert_eq!(
        cx.read_from_clipboard().and_then(|item| item.text()),
        Some("unchanged".into())
    );
    assert_eq!(
        input.read_with(cx, |input, _| input.text().to_string()),
        "ab\u{1F642}c"
    );

    cx.update(|_, cx| input.update(cx, |input, cx| input.reveal(true, cx)));
    cx.run_until_parked();
    assert!((caret_x(4, cx) - 3.0 * bullet).abs() > 1.0);
    press(cx, "shift-left");
    press(cx, "cmd-c");
    assert_eq!(
        cx.read_from_clipboard().and_then(|item| item.text()),
        Some("unchanged".into())
    );

    press(cx, "home");
    let viewport = cx.update(|window, _| window.viewport_size());
    let middle = mira_ui::theme::CONTROL_MD / 2.0;
    cx.simulate_click(point(viewport.width - px(12.0), middle), Modifiers::none());
    assert!((caret_x(4, cx) - 3.0 * bullet).abs() < 0.01);
    cx.simulate_mouse_move(point(px(12.0), middle), None, Modifiers::none());
    cx.simulate_input("X");
    assert_eq!(text(&input, cx), "Xab\u{1F642}c");
}

#[gpui::test]
fn input_scrolls_to_keep_the_caret_in_view(cx: &mut TestAppContext) {
    use gpui::{EntityInputHandler as _, Modifiers, MouseButton, point, px};
    let (input, cx) = focused_input(|input| input, cx);
    let long = "a".repeat(400);
    cx.simulate_input(&format!("{long}bcd"));
    let viewport = cx.update(|window, _| window.viewport_size());
    let middle = mira_ui::theme::CONTROL_MD / 2.0;
    cx.simulate_click(point(viewport.width - px(12.0), middle), Modifiers::none());
    cx.simulate_input("X");
    assert!(text(&input, cx).find('X').unwrap() >= 400);

    press(cx, "home");
    cx.simulate_click(point(px(12.0), middle), Modifiers::none());
    cx.simulate_input("Y");
    assert!(text(&input, cx).find('Y').unwrap() < 5);

    let selected = |cx: &mut VisualTestContext| {
        cx.update(|window, cx| {
            input.update(cx, |field, cx| {
                field.selected_text_range(false, window, cx).unwrap().range
            })
        })
    };
    let length = text(&input, cx).len();
    let next_frame = |cx: &mut VisualTestContext| {
        cx.background_executor
            .advance_clock(std::time::Duration::from_millis(50));
        cx.update(|window, cx| window.simulate_next_frame(cx));
        cx.run_until_parked();
    };
    for (key, outside, endpoint) in [
        ("end", -px(60.0), 0),
        ("home", viewport.width + px(60.0), length),
    ] {
        press(cx, key);
        let start = point(viewport.width / 2.0, middle);
        let outside = point(outside, middle);
        cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
        let anchor = selected(cx).start;
        assert!(anchor > 0 && anchor < length);
        cx.simulate_mouse_move(outside, MouseButton::Left, Modifiers::none());
        let expected = anchor.min(endpoint)..anchor.max(endpoint);
        let partial = selected(cx);
        assert!(!partial.is_empty());
        assert_ne!(partial, expected);
        cx.simulate_mouse_up(outside, MouseButton::Left, Modifiers::none());
        next_frame(cx);
        assert_eq!(selected(cx), partial);

        cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::shift());
        cx.simulate_mouse_move(outside, MouseButton::Left, Modifiers::none());
        for _ in 0..80 {
            next_frame(cx);
            if selected(cx) == expected {
                break;
            }
        }
        cx.simulate_mouse_up(outside, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_move(start, None, Modifiers::none());
        assert_eq!(selected(cx), expected);
    }
}

#[gpui::test]
fn input_stops_growing_at_its_max_length(cx: &mut TestAppContext) {
    use gpui::{ClipboardItem, EntityInputHandler as _};
    let (input, cx) = focused_input(|input| input.max_length(4), cx);
    let text =
        |cx: &mut VisualTestContext| input.read_with(cx, |input, _| input.text().to_string());
    cx.simulate_input("abcdef");
    assert_eq!(text(cx), "abcd");

    press(cx, "backspace");
    press(cx, "backspace");
    cx.write_to_clipboard(ClipboardItem::new_string("xyz".into()));
    press(cx, "cmd-v");
    assert_eq!(text(cx), "abxy");

    press(cx, "cmd-a");
    cx.update(|window, cx| {
        input.update(cx, |input, cx| {
            input.replace_and_mark_text_in_range(None, "ありがとう", Some(5..5), window, cx);
            input.unmark_text(window, cx);
        })
    });
    assert_eq!(text(cx), "ありがと");
}

struct NarrowStatus;

impl Render for NarrowStatus {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        use mira_ui::{components::StatusLabel, primitives::Activity};
        div().w(gpui::px(120.0)).flex().flex_col().child(
            div()
                .debug_selector(|| "status".into())
                .child(StatusLabel::new(
                    "status",
                    Activity::Failed,
                    "Login failed: the server closed the connection",
                )),
        )
    }
}

#[gpui::test]
fn status_label_wraps_inside_a_narrow_container(cx: &mut TestAppContext) {
    cx.update(mira_ui::init);
    let (_, cx) = cx.add_window_view(|_, _| NarrowStatus);
    cx.run_until_parked();
    let bounds = cx.debug_bounds("status").unwrap();
    assert!(bounds.size.width <= gpui::px(120.0));
    assert!(bounds.size.height > mira_ui::theme::LINE_BODY);
}

struct NarrowAlert;

impl Render for NarrowAlert {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().w(gpui::px(300.0)).child(
            mira_ui::primitives::Alert::new(
                "save",
                mira_ui::Tone::Danger,
                "Couldn't save the setups",
            )
            .body("Couldn't write ~/Library/Application Support/Mira/setups.json: the disk is full")
            .action(
                div()
                    .debug_selector(|| "retry".into())
                    .child(Button::new("retry", "Retry")),
            ),
        )
    }
}

#[gpui::test]
fn alert_wraps_its_body_and_keeps_its_action_inside(cx: &mut TestAppContext) {
    cx.update(mira_ui::init);
    let (_, cx) = cx.add_window_view(|_, _| NarrowAlert);
    cx.run_until_parked();
    let retry = cx.debug_bounds("retry").unwrap();
    assert!(retry.right() <= gpui::px(300.0), "{retry:?}");
}

struct NarrowSwitch;

impl Render for NarrowSwitch {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().w(gpui::px(160.0)).flex().flex_col().child(
            div().debug_selector(|| "switch".into()).child(
                mira_ui::primitives::Switch::new("safe-start", "Safe start and stop", false)
                    .description("Best effort attempt to start or stop the script at your house."),
            ),
        )
    }
}

#[gpui::test]
fn switch_description_wraps_inside_a_narrow_container(cx: &mut TestAppContext) {
    cx.update(mira_ui::init);
    let (_, cx) = cx.add_window_view(|_, _| NarrowSwitch);
    cx.run_until_parked();
    let one_line = gpui::px(6.0) + mira_ui::theme::LINE_BODY + mira_ui::theme::LINE_CAPTION;
    assert!(cx.debug_bounds("switch").unwrap().size.height > one_line);
}

struct Host {
    editor: FocusHandle,
    stack: Entity<mira_ui::primitives::Toaster>,
    renders: Rc<Cell<usize>>,
}

impl Render for Host {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.renders.set(self.renders.get() + 1);
        div()
            .size_full()
            .child(div().id("editor").track_focus(&self.editor).child("Editor"))
            .child(mira_ui::primitives::Toaster::layer(&self.stack, window, cx))
    }
}

#[gpui::test]
fn pushing_a_toast_redraws_the_window_that_shows_the_stack(cx: &mut TestAppContext) {
    cx.update(mira_ui::init);
    let renders = Rc::new(Cell::new(0));
    let (host, cx) = cx.add_window_view(|window, cx| Host {
        editor: cx.focus_handle(),
        stack: mira_ui::primitives::Toaster::new(window, cx),
        renders: renders.clone(),
    });
    cx.run_until_parked();
    let before = renders.get();
    cx.update(|_, cx| {
        mira_ui::primitives::Toaster::push(
            &host.read(cx).stack.clone(),
            mira_ui::primitives::Toast::new("Saved"),
            cx,
        )
    });
    cx.run_until_parked();
    assert!(renders.get() > before);
}

#[gpui::test]
fn toast_action_returns_keyboard_focus_to_the_originating_control(cx: &mut TestAppContext) {
    use mira_ui::primitives::{Toast, Toaster};
    cx.update(mira_ui::init);
    let (host, cx) = cx.add_window_view(|window, cx| {
        let editor = cx.focus_handle();
        window.activate_window();
        window.focus(&editor, cx);
        Host {
            editor,
            stack: Toaster::new(window, cx),
            renders: Rc::default(),
        }
    });
    let invoked = Rc::new(Cell::new(false));
    let action = invoked.clone();
    cx.update(|_, cx| {
        let toast = Toast::new("Saved").action("Undo", move |_, _| action.set(true));
        Toaster::push(&host.read(cx).stack.clone(), toast, cx);
    });
    cx.run_until_parked();
    assert!(cx.update(|window, cx| host.read(cx).editor.is_focused(window)));
    press(cx, "f6");
    assert!(!cx.update(|window, cx| host.read(cx).editor.is_focused(window)));
    press(cx, "tab");
    press(cx, "enter");
    assert!(invoked.get());
    assert!(cx.update(|window, cx| host.read(cx).editor.is_focused(window)));
}

#[gpui::test]
fn a_loading_toast_stays_until_its_result_replaces_it(cx: &mut TestAppContext) {
    use mira_ui::primitives::{Toast, Toaster};
    cx.update(|cx| {
        mira_ui::init(cx);
        cx.set_reduce_motion(true);
    });
    let (host, cx) = cx.add_window_view(|window, cx| {
        window.activate_window();
        Host {
            editor: cx.focus_handle(),
            stack: Toaster::new(window, cx),
            renders: Rc::default(),
        }
    });
    let stack = host.read_with(cx, |host, _| host.stack.clone());
    let settle = |cx: &mut gpui::VisualTestContext, wait: std::time::Duration| {
        cx.executor().advance_clock(wait);
        cx.run_until_parked();
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
    };
    let long = std::time::Duration::from_secs(10);

    let loading = cx.update(|_, cx| Toaster::push(&stack, Toast::new("Installing").loading(), cx));
    settle(cx, long);
    assert!(cx.debug_bounds("toast: Installing").is_some());

    let replaced =
        cx.update(|_, cx| Toaster::replace(&stack, loading, Toast::new("Installed"), cx));
    assert_eq!(replaced, loading);
    settle(cx, std::time::Duration::ZERO);
    assert!(cx.debug_bounds("toast: Installing").is_none());
    assert!(cx.debug_bounds("toast: Installed").is_some());
    settle(cx, long);
    assert!(cx.debug_bounds("toast: Installed").is_none());

    let renewed = cx.update(|_, cx| Toaster::replace(&stack, loading, Toast::new("Again"), cx));
    assert_ne!(renewed, loading);
    settle(cx, std::time::Duration::ZERO);
    assert!(cx.debug_bounds("toast: Again").is_some());
}

#[gpui::test]
fn a_replaced_toast_shows_its_final_size_on_the_first_frame(cx: &mut TestAppContext) {
    use mira_ui::primitives::{Toast, Toaster};
    cx.update(|cx| {
        mira_ui::init(cx);
        cx.set_reduce_motion(false);
    });
    let (host, cx) = cx.add_window_view(|window, cx| {
        window.activate_window();
        Host {
            editor: cx.focus_handle(),
            stack: Toaster::new(window, cx),
            renders: Rc::default(),
        }
    });
    let stack = host.read_with(cx, |host, _| host.stack.clone());
    let draw = |cx: &mut gpui::VisualTestContext| {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
    };
    let settle = |cx: &mut gpui::VisualTestContext| {
        for _ in 0..3 {
            std::thread::sleep(mira_ui::motion::GLIDE / 2);
            draw(cx);
        }
    };
    let id = cx.update(|_, cx| Toaster::push(&stack, Toast::new("Installing").loading(), cx));
    settle(cx);

    let title = "toast: Couldn't install package";
    cx.update(|_, cx| {
        let failed = Toast::new("Couldn't install package")
            .body("Couldn't find o/missing. If it's private, add a GitHub token in Settings.")
            .tone(mira_ui::Tone::Danger);
        Toaster::replace(&stack, id, failed, cx)
    });
    draw(cx);
    let first = cx.debug_bounds(title).unwrap();
    settle(cx);
    assert_eq!(first, cx.debug_bounds(title).unwrap());
}

struct Pane {
    stack: Entity<mira_ui::primitives::Toaster>,
    clicks: Rc<Cell<usize>>,
}

impl Render for Pane {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let clicks = self.clicks.clone();
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_1()
                    .relative()
                    .child(mira_ui::primitives::Toaster::layer(&self.stack, window, cx)),
            )
            .child(div().flex().justify_end().p(gpui::px(8.0)).child(
                div().debug_selector(|| "footer-action".into()).child(
                    Button::new("run", "Run").on_click(move |_, _, _| {
                        clicks.set(clicks.get() + 1);
                    }),
                ),
            ))
    }
}

#[gpui::test]
fn an_entering_toast_leaves_controls_outside_its_pane_clickable(cx: &mut TestAppContext) {
    use mira_ui::primitives::{Toast, Toaster};
    cx.update(|cx| {
        mira_ui::init(cx);
        cx.set_reduce_motion(false);
    });
    let clicks = Rc::new(Cell::new(0));
    let (pane, cx) = cx.add_window_view(|window, cx| Pane {
        stack: Toaster::new(window, cx),
        clicks: clicks.clone(),
    });
    cx.run_until_parked();
    let run = cx.debug_bounds("footer-action").unwrap();
    let stack = pane.read_with(cx, |pane, _| pane.stack.clone());
    cx.update(|window, cx| {
        Toaster::push(&stack, Toast::new("Installed Dailies"), cx);
        window.draw(cx).clear(cx);
    });
    cx.simulate_click(run.center(), gpui::Modifiers::default());
    assert_eq!(clicks.get(), 1);
}

#[gpui::test]
fn a_toast_with_line_breaks_renders(cx: &mut TestAppContext) {
    use mira_ui::primitives::{Toast, Toaster};
    cx.update(mira_ui::init);
    let (host, cx) = cx.add_window_view(|window, cx| Host {
        editor: cx.focus_handle(),
        stack: Toaster::new(window, cx),
        renders: Rc::default(),
    });
    let stack = host.read_with(cx, |host, _| host.stack.clone());
    cx.update(|_, cx| {
        let toast = Toast::new("Installed Daily\nquests").body("First line\nsecond line");
        Toaster::push(&stack, toast, cx)
    });
    cx.run_until_parked();
    assert!(cx.debug_bounds("toast: Installed Daily\nquests").is_some());
}

struct ThemedToolbar;

impl Render for ThemedToolbar {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use mira_ui::ActiveTheme as _;
        let action = |name: &'static str| {
            div()
                .flex()
                .debug_selector(move || name.into())
                .child(Button::new(name, name))
        };
        div().size_full().flex().flex_col().child(
            cx.theme()
                .toolbar()
                .debug_selector(|| "tools".into())
                .child(action("first"))
                .child(action("second")),
        )
    }
}

#[gpui::test]
fn a_runtime_theme_changes_the_toolbar_and_its_controls_together(cx: &mut TestAppContext) {
    use gpui::px;
    cx.update(mira_ui::init);
    let (_, cx) = cx.add_window_view(|_, _| ThemedToolbar);
    cx.run_until_parked();
    let sizes = |cx: &mut VisualTestContext| {
        let bar = cx.debug_bounds("tools").unwrap();
        let first = cx.debug_bounds("first").unwrap();
        let second = cx.debug_bounds("second").unwrap();
        (
            bar.size.height,
            first.size.height,
            first.left() - bar.left(),
            second.left() - first.right(),
        )
    };
    assert_eq!(sizes(cx), (px(40.0), px(28.0), px(12.0), px(8.0)));
    cx.update(|_, cx| {
        let mut theme = mira_ui::Theme::dark();
        theme.layout.unit = px(6.0);
        theme.layout.control = px(36.0);
        theme.layout.toolbar = px(56.0);
        cx.set_global(theme);
        cx.refresh_windows();
    });
    cx.run_until_parked();
    assert_eq!(sizes(cx), (px(56.0), px(36.0), px(18.0), px(12.0)));
}

struct Loot;

impl Render for Loot {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        use mira_ui::primitives::{Column, Table};
        let row = ["Cape", "1", "extra"].map(SharedString::from).to_vec();
        div().size_full().child(Table::new(
            "loot",
            "Loot",
            vec![Column::new("Item"), Column::new("Count")],
            vec![row],
        ))
    }
}

#[gpui::test]
fn table_skips_cells_without_a_column(cx: &mut TestAppContext) {
    cx.update(mira_ui::init);
    let (_, cx) = cx.add_window_view(|_, _| Loot);
    cx.run_until_parked();
}

struct TooltipBesideFocus {
    focus: FocusHandle,
}

impl Render for TooltipBesideFocus {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .track_focus(&self.focus)
            .size_full()
            .child(mira_ui::primitives::Tooltip::new(
                "tip",
                "Hide sidebar",
                div().id("trigger").size(gpui::px(26.0)),
            ))
    }
}

#[gpui::test]
fn clicking_a_tooltip_trigger_leaves_focus_where_it_was(cx: &mut TestAppContext) {
    cx.update(mira_ui::init);
    let (view, cx) = cx.add_window_view(|window, cx| {
        let focus = cx.focus_handle();
        window.activate_window();
        window.focus(&focus, cx);
        TooltipBesideFocus { focus }
    });
    cx.run_until_parked();
    cx.simulate_click(
        gpui::point(gpui::px(13.0), gpui::px(13.0)),
        gpui::Modifiers::none(),
    );
    let focus = view.read_with(cx, |view, _| view.focus.clone());
    assert!(cx.update(|window, _| focus.is_focused(window)));
}

#[gpui::test]
fn a_tooltip_fades_as_soon_as_the_pointer_leaves_its_trigger(cx: &mut TestAppContext) {
    fn move_to(cx: &mut VisualTestContext, at: gpui::Point<gpui::Pixels>) {
        cx.simulate_mouse_move(at, None, gpui::Modifiers::none());
        cx.run_until_parked();
    }
    fn gone_after_fade(cx: &mut VisualTestContext) -> bool {
        cx.executor()
            .advance_clock(mira_ui::motion::LEAVE + std::time::Duration::from_millis(1));
        cx.run_until_parked();
        cx.debug_bounds("tip-tooltip").is_none()
    }

    cx.update(mira_ui::init);
    let (_, cx) = cx.add_window_view(|window, cx| {
        let focus = cx.focus_handle();
        window.activate_window();
        TooltipBesideFocus { focus }
    });
    cx.run_until_parked();
    let trigger = gpui::point(gpui::px(13.0), gpui::px(13.0));

    move_to(cx, trigger);
    let panel = cx.debug_bounds("tip-tooltip").expect("hovering opens it");
    move_to(cx, gpui::point(gpui::px(200.0), gpui::px(200.0)));
    assert!(gone_after_fade(cx), "leaving for empty space");

    move_to(cx, trigger);
    move_to(cx, panel.center());
    assert!(gone_after_fade(cx), "leaving onto the tooltip itself");
}

struct Cards;

impl Render for Cards {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        use mira_ui::primitives::{Align, HoverCard, Placement, Side};
        let card = |id: &'static str| {
            div().flex().w(gpui::px(100.0)).h(gpui::px(30.0)).child(
                HoverCard::new(id, id)
                    .placement(Placement::new(Side::Right, Align::Start))
                    .row("Error", "Stopped", mira_ui::Tone::Danger)
                    .child(div().size_full()),
            )
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(card("a"))
            .child(card("b"))
    }
}

#[gpui::test]
fn hover_cards_wait_for_a_rest_hand_off_without_stacking_and_close_at_once(
    cx: &mut TestAppContext,
) {
    fn move_to(cx: &mut VisualTestContext, x: f32, y: f32) -> (bool, bool) {
        use gpui::{Modifiers, point, px};
        cx.simulate_mouse_move(point(px(x), px(y)), None, Modifiers::none());
        cx.run_until_parked();
        (
            cx.debug_bounds("a-card").is_some(),
            cx.debug_bounds("b-card").is_some(),
        )
    }
    fn wait(cx: &mut VisualTestContext, millis: u64) {
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(millis));
        cx.run_until_parked();
    }

    cx.update(mira_ui::init);
    let (_, cx) = cx.add_window_view(|_, _| Cards);
    cx.run_until_parked();
    let shown = move_to(cx, 50.0, 15.0);
    assert_eq!(shown, (false, false), "a passing pointer opens nothing");
    wait(cx, 1000);
    let shown = move_to(cx, 50.0, 16.0);
    assert_eq!(shown, (true, false), "resting on a opens its card");
    let shown = move_to(cx, 50.0, 45.0);
    assert_eq!(shown, (false, true), "b takes over at once");
    move_to(cx, 50.0, 300.0);
    wait(cx, 150);
    let shown = move_to(cx, 50.0, 301.0);
    assert_eq!(
        shown,
        (false, false),
        "b fades out as soon as the pointer leaves"
    );
    let shown = move_to(cx, 50.0, 15.0);
    assert_eq!(shown, (true, false), "a opens at once right after b closed");
    move_to(cx, 50.0, 300.0);
    wait(cx, 1000);
    let shown = move_to(cx, 50.0, 45.0);
    assert_eq!(
        shown,
        (false, false),
        "b waits again once the group goes cold"
    );
}

struct InvalidAboveTheFold {
    focus: FocusHandle,
    pane: gpui::ScrollHandle,
    field: Entity<mira_ui::primitives::Input>,
}

impl Render for InvalidAboveTheFold {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use gpui::px;
        mira_ui::nav::set_scroll_root(&self.pane, window, cx);
        div()
            .key_context(FOCUS_SCOPE)
            .track_focus(&self.focus)
            .size_full()
            .child(
                div()
                    .id("pane")
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.pane)
                    .child(self.field.clone())
                    .child(div().h(px(2000.0)))
                    .child(Button::new("below", "Below the fold")),
            )
    }
}

#[gpui::test]
fn a_resting_invalid_field_leaves_the_reveal_to_the_focused_control(cx: &mut TestAppContext) {
    use gpui::{AppContext as _, Focusable as _, px};
    cx.update(mira_ui::init);
    let (view, cx) = cx.add_window_view(|window, cx| {
        window.activate_window();
        let field = cx.new(|cx| {
            let mut field = mira_ui::primitives::Input::new("Offset", cx);
            field.set_invalid(true, cx);
            field
        });
        InvalidAboveTheFold {
            focus: cx.focus_handle(),
            pane: gpui::ScrollHandle::new(),
            field,
        }
    });
    cx.run_until_parked();
    let field = view.read_with(cx, |view, _| view.field.clone());
    cx.update(|window, cx| window.focus(&field.focus_handle(cx), cx));
    cx.run_until_parked();
    press(cx, "tab");
    let offset = view.read_with(cx, |view, _| view.pane.offset().y);
    assert!(
        offset < px(0.0),
        "the invalid field kept the pane from revealing the focused button"
    );
}

struct CardOverPane {
    pane: gpui::ScrollHandle,
    hovered: Rc<Cell<bool>>,
}

impl Render for CardOverPane {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        use gpui::px;
        use mira_ui::primitives::{Align, HoverCard, Placement, Side};
        let hovered = self.hovered.clone();
        div()
            .size_full()
            .flex()
            .child(
                div().flex_none().w(px(100.0)).h(px(30.0)).flex().child(
                    HoverCard::new("card", "Card")
                        .placement(Placement::new(Side::Right, Align::Start))
                        .row("Error", "Stopped", mira_ui::Tone::Danger)
                        .child(div().size_full()),
                ),
            )
            .child(
                div()
                    .id("pane")
                    .flex_1()
                    .h_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.pane)
                    .on_hover(move |over, _, _| {
                        if *over {
                            hovered.set(true);
                        }
                    })
                    .children((0..40).map(|index| div().h(px(40.0)).child(format!("Row {index}")))),
            )
    }
}

#[gpui::test]
fn an_open_hover_card_keeps_the_pointer_and_wheel_from_what_lies_beneath(cx: &mut TestAppContext) {
    use gpui::{Modifiers, ScrollDelta, ScrollEvent, point, px};
    cx.update(mira_ui::init);
    let pane = gpui::ScrollHandle::new();
    let hovered = Rc::new(Cell::new(false));
    let (_, cx) = cx.add_window_view({
        let (pane, hovered) = (pane.clone(), hovered.clone());
        move |window, _| {
            window.activate_window();
            CardOverPane { pane, hovered }
        }
    });
    cx.run_until_parked();
    cx.simulate_mouse_move(point(px(50.0), px(15.0)), None, Modifiers::none());
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(1000));
    cx.run_until_parked();
    cx.simulate_mouse_move(point(px(50.0), px(16.0)), None, Modifiers::none());
    cx.run_until_parked();
    let card = cx
        .debug_bounds("card-card")
        .expect("resting opens the card");
    hovered.set(false);

    cx.simulate_mouse_move(card.center(), None, Modifiers::none());
    cx.run_until_parked();
    cx.simulate_event(ScrollEvent {
        position: card.center(),
        delta: ScrollDelta::Pixels(point(px(0.0), px(-80.0))),
        ..Default::default()
    });
    cx.run_until_parked();

    assert!(
        cx.debug_bounds("card-card").is_some(),
        "the card stays open while pointed at"
    );
    assert_eq!(
        pane.offset().y,
        px(0.0),
        "the wheel scrolled the pane under the card"
    );
    assert!(!hovered.get(), "the pane under the card saw the pointer");
}

struct FittedCard(&'static str);

impl Render for FittedCard {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        use gpui::px;
        use mira_ui::primitives::HoverCard;
        div().size_full().child(
            div().w(px(40.0)).h(px(20.0)).flex().child(
                HoverCard::new("fit", "9 more tags")
                    .fit()
                    .body(self.0)
                    .child(div().size_full()),
            ),
        )
    }
}

#[gpui::test]
fn a_fitted_hover_card_is_as_wide_as_its_text_up_to_the_popover_width(cx: &mut TestAppContext) {
    use gpui::{Modifiers, point, px};
    cx.update(mira_ui::init);
    let mut width = |body: &'static str| {
        let (_, cx) = cx.add_window_view(move |window, _| {
            window.activate_window();
            FittedCard(body)
        });
        cx.run_until_parked();
        cx.simulate_mouse_move(point(px(20.0), px(10.0)), None, Modifiers::none());
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(1000));
        cx.run_until_parked();
        cx.simulate_mouse_move(point(px(20.0), px(11.0)), None, Modifiers::none());
        cx.run_until_parked();
        cx.debug_bounds("fit-card")
            .expect("resting opens the card")
            .size
            .width
    };
    let short = width("boom, went");
    let long = width(
        "boom, went, dynamite, thewicked, oblivion, nulgath, maces, overlords, doomblade, blessed",
    );
    assert!(short < px(200.0), "{short:?}");
    assert_eq!(long, px(288.0));
}

struct FocusRing {
    root: FocusHandle,
    target: FocusHandle,
}

impl Render for FocusRing {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .key_context(FOCUS_SCOPE)
            .track_focus(&self.root)
            .size_full()
            .child(mira_ui::nav::pointer_probe())
            .child(div().track_focus(&self.target).size(gpui::px(20.0)))
    }
}

#[gpui::test]
fn the_focus_ring_survives_pointer_moves_until_a_click(cx: &mut TestAppContext) {
    cx.update(mira_ui::init);
    let (view, cx) = cx.add_window_view(|window, cx| {
        let root = cx.focus_handle();
        window.activate_window();
        window.focus(&root, cx);
        FocusRing {
            root,
            target: cx.focus_handle().tab_stop(true),
        }
    });
    cx.run_until_parked();
    let target = view.read_with(cx, |view, _| view.target.clone());
    let ring = |cx: &mut VisualTestContext| {
        cx.update(|window, cx| mira_ui::nav::keyboard_focused(&target, window, cx))
    };
    let at = gpui::point(gpui::px(100.0), gpui::px(100.0));

    press(cx, "tab");
    assert!(ring(cx));

    cx.simulate_mouse_move(at, None, gpui::Modifiers::none());
    assert!(ring(cx));

    cx.simulate_mouse_down(at, gpui::MouseButton::Left, gpui::Modifiers::none());
    assert!(!ring(cx));

    press(cx, "tab");
    assert!(ring(cx));
}

type Moves = Rc<std::cell::RefCell<Vec<(usize, usize, usize)>>>;

struct Rows {
    lists: usize,
    count: usize,
    pinned: usize,
    scroll: Option<gpui::ScrollHandle>,
    moves: Moves,
}

impl Render for Rows {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        use mira_ui::primitives::{Flow, Sortable};
        div().flex().children((0..self.lists).map(|list| {
            let moves = self.moves.clone();
            let rows = Sortable::new(
                div()
                    .id(("rows", list))
                    .w(gpui::px(100.0))
                    .flex()
                    .flex_col()
                    .when(self.scroll.is_some(), |rows| rows.h(gpui::px(60.0))),
                Flow::Column,
                0..self.count,
                |_, _, _| div().h(gpui::px(20.0)).into_any_element(),
                move |from, to, _, _| moves.borrow_mut().push((list, from, to)),
            )
            .pin_first(self.pinned);
            match &self.scroll {
                Some(scroll) => rows.scrolls(scroll),
                None => rows,
            }
        }))
    }
}

fn rows(lists: usize, pinned: usize, cx: &mut TestAppContext) -> (Moves, &mut VisualTestContext) {
    show(lists, 4, pinned, None, cx)
}

fn show(
    lists: usize,
    count: usize,
    pinned: usize,
    scroll: Option<gpui::ScrollHandle>,
    cx: &mut TestAppContext,
) -> (Moves, &mut VisualTestContext) {
    cx.update(mira_ui::init);
    let moves = Moves::default();
    let (_, cx) = cx.add_window_view(|window, _| {
        window.activate_window();
        Rows {
            lists,
            count,
            pinned,
            scroll,
            moves: Rc::clone(&moves),
        }
    });
    cx.run_until_parked();
    (moves, cx)
}

fn hold(cx: &mut VisualTestContext, x: f32, y: f32) {
    use gpui::{Modifiers, MouseButton, point, px};
    for _ in 0..8 {
        std::thread::sleep(std::time::Duration::from_millis(50));
        cx.simulate_mouse_move(point(px(x), px(y)), MouseButton::Left, Modifiers::none());
    }
}

fn lift(cx: &mut VisualTestContext, x: f32, from_y: f32, to_y: f32) {
    use gpui::{Modifiers, MouseButton, point, px};
    let start = point(px(x), px(from_y));
    cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
    cx.simulate_mouse_move(
        start + point(px(0.0), px(5.0)),
        MouseButton::Left,
        Modifiers::none(),
    );
    cx.simulate_mouse_move(point(px(x), px(to_y)), MouseButton::Left, Modifiers::none());
}

fn release(cx: &mut VisualTestContext, x: f32, y: f32) {
    use gpui::{Modifiers, MouseButton, point, px};
    cx.simulate_mouse_up(point(px(x), px(y)), MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
}

#[gpui::test]
fn a_dragged_row_lands_on_the_row_nearest_its_center(cx: &mut TestAppContext) {
    let (moves, cx) = rows(1, 0, cx);
    lift(cx, 10.0, 10.0, 60.0);
    release(cx, 10.0, 60.0);
    assert_eq!(*moves.borrow(), [(0, 0, 2)]);
}

#[gpui::test]
fn escape_puts_a_dragged_row_back(cx: &mut TestAppContext) {
    let (moves, cx) = rows(1, 0, cx);
    lift(cx, 10.0, 10.0, 60.0);
    press(cx, "escape");
    release(cx, 10.0, 60.0);
    assert!(moves.borrow().is_empty());
}

#[gpui::test]
fn releasing_outside_the_list_drops_at_the_end_the_row_was_held_against(cx: &mut TestAppContext) {
    let (moves, cx) = rows(2, 0, cx);
    lift(cx, 10.0, 10.0, 500.0);
    release(cx, 150.0, 500.0);
    assert_eq!(*moves.borrow(), [(0, 0, 3)]);
}

#[gpui::test]
fn releasing_a_row_where_it_started_reorders_nothing(cx: &mut TestAppContext) {
    let (moves, cx) = rows(1, 0, cx);
    lift(cx, 10.0, 10.0, 14.0);
    release(cx, 10.0, 14.0);
    assert!(moves.borrow().is_empty());
}

#[gpui::test]
fn pinned_rows_stay_first(cx: &mut TestAppContext) {
    let (moves, cx) = rows(1, 1, cx);
    lift(cx, 10.0, 10.0, 60.0);
    release(cx, 10.0, 60.0);
    lift(cx, 10.0, 50.0, -100.0);
    release(cx, 10.0, -100.0);
    assert_eq!(*moves.borrow(), [(0, 2, 1)]);
}

#[gpui::test]
fn holding_a_row_against_the_bottom_edge_scrolls_to_the_end(cx: &mut TestAppContext) {
    let scroll = gpui::ScrollHandle::new();
    let (moves, cx) = show(1, 8, 0, Some(scroll.clone()), cx);
    lift(cx, 10.0, 10.0, 58.0);
    hold(cx, 10.0, 58.0);
    release(cx, 10.0, 58.0);
    assert_eq!(*moves.borrow(), [(0, 0, 7)]);
}

#[gpui::test]
fn a_row_grabbed_at_the_edge_waits_for_a_move_toward_it(cx: &mut TestAppContext) {
    let scroll = gpui::ScrollHandle::new();
    let (_, cx) = show(1, 8, 0, Some(scroll.clone()), cx);
    lift(cx, 10.0, 45.0, 50.0);
    hold(cx, 10.0, 50.0);
    assert_eq!(scroll.offset().y, gpui::px(0.0));
}

struct Panes {
    virtual_rows: bool,
}

impl Render for Panes {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        use mira_ui::primitives::{List, ListItem, VirtualList};
        let item = |index: usize| ListItem::new(format!("Script {index}"));
        let height = gpui::px(200.0);
        div().w(gpui::px(300.0)).child(if self.virtual_rows {
            VirtualList::new("library", "Library", 3, item)
                .height(height)
                .into_any_element()
        } else {
            List::new("queue", "Queue", 3, item)
                .height(height)
                .on_reorder(|_, _, _, _| {})
                .into_any_element()
        })
    }
}

#[gpui::test]
fn virtual_and_plain_lists_space_their_rows_alike(cx: &mut TestAppContext) {
    cx.update(mira_ui::init);
    let mut last_row = |virtual_rows: bool| {
        let (_, cx) = cx.add_window_view(|_, _| Panes { virtual_rows });
        cx.run_until_parked();
        cx.debug_bounds("row-2")
    };
    assert_eq!(last_row(true), last_row(false));
}

#[gpui::test]
fn dragging_the_last_row_up_keeps_a_bottom_scrolled_list_in_place(cx: &mut TestAppContext) {
    use gpui::{point, px};
    let scroll = gpui::ScrollHandle::new();
    let (_, cx) = show(1, 8, 0, Some(scroll.clone()), cx);
    scroll.set_offset(point(px(0.0), px(-100.0)));
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    lift(cx, 10.0, 45.0, 38.0);
    assert_eq!(scroll.offset().y, px(-100.0));
}

struct GameBar {
    focus: FocusHandle,
    chosen: Rc<std::cell::RefCell<Vec<&'static str>>>,
    dismissed: Rc<Cell<usize>>,
    reached: Rc<Cell<usize>>,
    open: Option<&'static str>,
}

impl Render for GameBar {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        use gpui::{MouseButton, px};
        use mira_ui::primitives::{MenuItem, Menubar};
        let pick = |name: &'static str| {
            let chosen = self.chosen.clone();
            move |_: &mut Window, _: &mut gpui::App| chosen.borrow_mut().push(name)
        };
        let (new, undo, run) = (pick("new"), pick("undo"), pick("run"));
        let (dismissed, reached) = (self.dismissed.clone(), self.reached.clone());
        let mut menubar = Menubar::new("bar", "Game")
            .menu(Button::new("file", "File").w(px(80.0)), move |_, _| {
                let new = new.clone();
                vec![MenuItem::action("New", move |window, cx| new(window, cx))]
            })
            .menu(Button::new("edit", "Edit").w(px(80.0)), move |_, _| {
                let undo = undo.clone();
                vec![MenuItem::action("Undo", move |window, cx| undo(window, cx))]
            })
            .button(
                Button::new("run", "Run")
                    .w(px(80.0))
                    .on_click(move |_, window, cx| run(window, cx)),
            )
            .on_dismiss(move |_, _, cx| {
                dismissed.set(dismissed.get() + 1);
                cx.stop_propagation();
            });
        if let Some(entry) = self.open.take() {
            menubar = menubar.toggle(entry);
        }
        div()
            .key_context(FOCUS_SCOPE)
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .flex_col()
            .child(menubar)
            .child(
                div()
                    .flex_1()
                    .on_mouse_down(MouseButton::Left, move |_, _, _| {
                        reached.set(reached.get() + 1)
                    }),
            )
    }
}

struct Bar<'a> {
    cx: &'a mut VisualTestContext,
    view: gpui::Entity<GameBar>,
}

impl Bar<'_> {
    const FILE: (f32, f32) = (40.0, 13.0);
    const EDIT: (f32, f32) = (120.0, 13.0);
    const GAME: (f32, f32) = (200.0, 300.0);

    fn click(&mut self, (x, y): (f32, f32)) {
        use gpui::{Modifiers, point, px};
        self.cx
            .simulate_click(point(px(x), px(y)), Modifiers::none());
        self.cx.run_until_parked();
    }

    fn hover(&mut self, (x, y): (f32, f32)) {
        use gpui::{Modifiers, point, px};
        self.cx
            .simulate_mouse_move(point(px(x), px(y)), None, Modifiers::none());
        self.cx.run_until_parked();
    }

    fn keys(&mut self, keys: &'static str) {
        for key in keys.split(' ') {
            press(self.cx, key);
        }
    }

    fn chosen(&self) -> Vec<&'static str> {
        self.view
            .read_with(self.cx, |view, _| view.chosen.borrow().clone())
    }

    fn root_focused(&mut self) -> bool {
        let focus = self.view.read_with(self.cx, |view, _| view.focus.clone());
        self.cx.update(|window, _| focus.is_focused(window))
    }

    fn open(&mut self, entry: &'static str) {
        self.view.update(self.cx, |view, cx| {
            view.open = Some(entry);
            cx.notify();
        });
        self.cx.run_until_parked();
    }

    fn counts(&self) -> (usize, usize) {
        self.view.read_with(self.cx, |view, _| {
            (view.dismissed.get(), view.reached.get())
        })
    }
}

fn game_bar(cx: &mut TestAppContext) -> Bar<'_> {
    cx.update(mira_ui::init);
    let (view, cx) = cx.add_window_view(|window, cx| {
        let focus = cx.focus_handle();
        window.activate_window();
        window.focus(&focus, cx);
        GameBar {
            focus,
            chosen: Rc::default(),
            dismissed: Rc::default(),
            reached: Rc::default(),
            open: None,
        }
    });
    cx.run_until_parked();
    Bar { cx, view }
}

#[gpui::test]
fn menubar_switches_to_a_hovered_trigger_once_an_entry_is_open(cx: &mut TestAppContext) {
    let mut bar = game_bar(cx);
    bar.hover(Bar::EDIT);
    bar.click(Bar::FILE);
    bar.hover(Bar::EDIT);
    bar.keys("down enter");
    assert_eq!(bar.chosen(), ["undo"]);
}

#[gpui::test]
fn menubar_moves_between_entries_with_left_and_right(cx: &mut TestAppContext) {
    let mut bar = game_bar(cx);
    bar.keys("tab right enter enter");
    assert_eq!(bar.chosen(), ["undo"]);
    bar.keys("enter left enter");
    assert_eq!(bar.chosen(), ["undo", "new"]);
    bar.click(Bar::FILE);
    bar.keys("right right enter");
    assert_eq!(bar.chosen(), ["undo", "new", "new"]);
}

#[gpui::test]
fn menubar_moves_onto_buttons_with_left_and_right(cx: &mut TestAppContext) {
    let mut bar = game_bar(cx);
    bar.keys("tab right right enter");
    assert_eq!(bar.chosen(), ["run"]);
    bar.keys("left enter enter");
    assert_eq!(bar.chosen(), ["run", "undo"]);
    bar.click(Bar::EDIT);
    bar.keys("right enter");
    assert_eq!(bar.chosen(), ["run", "undo", "new"]);
}

#[gpui::test]
fn menubar_escape_closes_the_entry_and_returns_focus(cx: &mut TestAppContext) {
    let mut bar = game_bar(cx);
    bar.click(Bar::FILE);
    assert!(!bar.root_focused());
    bar.keys("escape");
    assert!(bar.root_focused());

    bar.keys("tab enter escape right enter enter");
    assert_eq!(bar.chosen(), ["undo"]);
}

#[gpui::test]
fn menubar_toggles_a_requested_panel_but_not_a_plain_button(cx: &mut TestAppContext) {
    let mut bar = game_bar(cx);
    bar.open("edit");
    bar.keys("enter");
    assert_eq!(bar.chosen(), ["undo"]);
    assert!(bar.root_focused());
    bar.keys("enter");
    assert_eq!(bar.chosen(), ["undo"]);

    bar.open("edit");
    assert!(!bar.root_focused());
    bar.open("edit");
    assert!(bar.root_focused());
    bar.keys("enter");
    assert_eq!(bar.chosen(), ["undo"]);
    bar.open("run");
    assert!(bar.root_focused());
}

#[gpui::test]
fn menubar_hands_the_dismissing_press_to_on_dismiss(cx: &mut TestAppContext) {
    let mut bar = game_bar(cx);
    bar.click(Bar::FILE);
    bar.click(Bar::EDIT);
    assert_eq!(bar.counts(), (0, 0));
    bar.click(Bar::GAME);
    assert_eq!(bar.counts(), (1, 0));
    bar.click(Bar::GAME);
    assert_eq!(bar.counts(), (1, 1));
    bar.keys("down enter");
    assert!(bar.chosen().is_empty());
}

struct AttackBar {
    focus: FocusHandle,
    chosen: Rc<std::cell::RefCell<Vec<&'static str>>>,
}

impl Render for AttackBar {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        use gpui::px;
        use mira_ui::primitives::{Menubar, Switch, button::Variant};
        let chosen = self.chosen.clone();
        let pick = move |name: &'static str| {
            let chosen = chosen.clone();
            move || chosen.borrow_mut().push(name)
        };
        let (switch, generic, farming, edit) = (
            pick("switch"),
            pick("generic"),
            pick("farming"),
            pick("edit"),
        );
        div()
            .key_context(FOCUS_SCOPE)
            .track_focus(&self.focus)
            .size_full()
            .child(mira_ui::nav::pointer_probe())
            .child(
                Menubar::new("bar", "Game")
                    .row_popover(
                        Button::new("attack", "Auto attack").w(px(80.0)),
                        move |_, _, cx| {
                            let (switch, generic, farming, edit) = (
                                switch.clone(),
                                generic.clone(),
                                farming.clone(),
                                edit.clone(),
                            );
                            mira_ui::v_flex()
                                .w(px(240.0))
                                .child(
                                    Switch::new("enabled", "Auto attack", false)
                                        .trailing()
                                        .on_toggle(move |_, _, _| switch()),
                                )
                                .child(
                                    DropdownMenu::row("profile", "Profile", "Generic", cx).items(
                                        move |_, _| {
                                            let (generic, farming) =
                                                (generic.clone(), farming.clone());
                                            vec![
                                                MenuItem::radio("Generic", true, move |_, _| {
                                                    generic()
                                                }),
                                                MenuItem::radio("Farming", false, move |_, _| {
                                                    farming()
                                                }),
                                            ]
                                        },
                                    ),
                                )
                                .child(
                                    Button::new("edit", "Edit profiles…")
                                        .variant(Variant::Ghost)
                                        .w_full()
                                        .on_click(move |_, _, _| edit()),
                                )
                                .into_any_element()
                        },
                    )
                    .menu(Button::new("windows", "Windows").w(px(80.0)), |_, _| {
                        vec![MenuItem::action("Packets", |_, _| {})]
                    }),
            )
    }
}

fn attack_bar(
    cx: &mut TestAppContext,
) -> (
    Rc<std::cell::RefCell<Vec<&'static str>>>,
    &mut VisualTestContext,
) {
    cx.update(|cx| {
        mira_ui::init(cx);
        cx.set_reduce_motion(true);
    });
    let chosen = Rc::default();
    let (_, cx) = cx.add_window_view(|window, cx| {
        let focus = cx.focus_handle();
        window.activate_window();
        window.focus(&focus, cx);
        AttackBar {
            focus,
            chosen: Rc::clone(&chosen),
        }
    });
    cx.run_until_parked();
    (chosen, cx)
}

#[gpui::test]
fn arrows_move_between_the_rows_of_a_row_popover(cx: &mut TestAppContext) {
    let (chosen, cx) = attack_bar(cx);
    for keys in [
        "tab enter down down enter",
        "down space",
        "up up enter down enter",
        "end enter home space",
    ] {
        for key in keys.split(' ') {
            press(cx, key);
        }
    }
    assert_eq!(
        *chosen.borrow(),
        ["edit", "switch", "farming", "edit", "switch"],
        "Down passes over the Profile row, the ends wrap, and Return opens Profile"
    );
}

#[gpui::test]
fn an_open_popover_closes_when_its_window_stops_being_active(cx: &mut TestAppContext) {
    let (_, cx) = attack_bar(cx);
    press(cx, "tab");
    press(cx, "enter");
    assert!(cx.debug_bounds("popover-panel").is_some());
    cx.deactivate_window();
    cx.run_until_parked();
    assert!(
        cx.debug_bounds("popover-panel").is_none(),
        "a popover left open behind another window would still react to the pointer"
    );
}

#[gpui::test]
fn arrows_continue_from_a_clicked_row_in_a_row_popover(cx: &mut TestAppContext) {
    use gpui::{Modifiers, point, px};
    let (chosen, cx) = attack_bar(cx);
    press(cx, "tab");
    press(cx, "enter");
    let panel = cx.debug_bounds("popover-panel").unwrap();
    cx.simulate_click(
        point(panel.left() + px(40.0), panel.top() + px(18.0)),
        Modifiers::none(),
    );
    cx.run_until_parked();
    for key in ["down", "down", "enter"] {
        press(cx, key);
    }
    assert_eq!(*chosen.borrow(), ["switch", "edit"]);
}

#[gpui::test]
fn a_row_popover_left_behind_another_window_returns_focus_to_its_trigger(cx: &mut TestAppContext) {
    let (_, cx) = attack_bar(cx);
    for key in ["tab", "enter", "down", "enter"] {
        press(cx, key);
    }
    cx.deactivate_window();
    cx.update(|window, _| window.activate_window());
    cx.run_until_parked();
    assert!(cx.debug_bounds("popover-panel").is_none());
    press(cx, "enter");
    assert!(
        cx.debug_bounds("popover-panel").is_some(),
        "Return on the trigger reopens the popover"
    );
}

#[gpui::test]
fn a_row_popover_reached_by_hover_starts_like_a_clicked_one(cx: &mut TestAppContext) {
    use gpui::{Modifiers, point, px};
    let (chosen, cx) = attack_bar(cx);
    for key in ["tab", "right", "enter"] {
        press(cx, key);
    }
    for x in [40.0, 41.0] {
        cx.simulate_mouse_move(point(px(x), px(13.0)), None, Modifiers::none());
        cx.run_until_parked();
    }
    assert!(cx.debug_bounds("popover-panel").is_some());
    press(cx, "down");
    press(cx, "space");
    assert_eq!(*chosen.borrow(), ["switch"]);
}

#[gpui::test]
fn the_first_arrow_in_a_clicked_row_popover_lands_on_an_end_row(cx: &mut TestAppContext) {
    use gpui::{Modifiers, point, px};
    let (chosen, cx) = attack_bar(cx);
    for keys in ["down space escape", "up enter"] {
        cx.simulate_click(point(px(40.0), px(13.0)), Modifiers::none());
        cx.run_until_parked();
        for key in keys.split(' ') {
            press(cx, key);
        }
    }
    assert_eq!(*chosen.borrow(), ["switch", "edit"]);
}

struct SafeStart {
    focus: FocusHandle,
    toggled: Rc<Cell<usize>>,
}

impl Render for SafeStart {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let toggled = self.toggled.clone();
        div()
            .key_context(FOCUS_SCOPE)
            .track_focus(&self.focus)
            .size_full()
            .child(
                mira_ui::primitives::Popover::new("options")
                    .trigger(Button::new("options", "Options"))
                    .content(move |_, _, _| {
                        let toggled = toggled.clone();
                        mira_ui::primitives::Switch::new("safe", "Safe start", false)
                            .on_toggle(move |_, _, _| toggled.set(toggled.get() + 1))
                            .into_any_element()
                    }),
            )
    }
}

#[gpui::test]
fn escape_closes_a_popover_back_onto_its_trigger(cx: &mut TestAppContext) {
    cx.update(mira_ui::init);
    let toggled = Rc::new(Cell::new(0));
    let (_, cx) = cx.add_window_view(|window, cx| {
        let focus = cx.focus_handle();
        window.activate_window();
        window.focus(&focus, cx);
        SafeStart {
            focus,
            toggled: toggled.clone(),
        }
    });
    cx.run_until_parked();
    for key in ["tab", "enter", "escape", "enter", "space"] {
        press(cx, key);
    }
    assert_eq!(toggled.get(), 1);
}

#[gpui::test]
fn input_drops_its_selection_when_focus_leaves(cx: &mut TestAppContext) {
    use gpui::EntityInputHandler as _;
    let (input, cx) = focused_input(|input| input, cx);
    cx.simulate_input("walk");
    press(cx, "cmd-a");
    cx.update(|window, cx| window.blur(cx));
    cx.run_until_parked();
    let selection = cx.update(|window, cx| {
        input.update(cx, |input, cx| input.selected_text_range(false, window, cx))
    });
    assert!(selection.unwrap().range.is_empty());
}

struct Profiles {
    focus: FocusHandle,
    selected: usize,
    chosen: Rc<std::cell::RefCell<Vec<String>>>,
}

impl Render for Profiles {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use mira_ui::primitives::{HoverCard, MenuItem, Tab, Tabs};
        let chosen = self.chosen.clone();
        let this = cx.weak_entity();
        div()
            .key_context(FOCUS_SCOPE)
            .track_focus(&self.focus)
            .size_full()
            .child(
                Tabs::new("profiles", "Profiles", 3, move |index| {
                    let chosen = chosen.clone();
                    let name = ["Generic", "Farming", "Boss"][index];
                    Tab::new(name)
                        .hover_card(HoverCard::new(("details", index), name))
                        .start_slot(
                            div()
                                .size(gpui::px(4.0))
                                .debug_selector(move || format!("profile-{index}")),
                        )
                        .menu(move |_, _| {
                            let chosen = chosen.clone();
                            vec![MenuItem::action("Rename", move |_, _| {
                                chosen.borrow_mut().push(format!("rename {index}"))
                            })]
                        })
                })
                .vertical()
                .selected(self.selected)
                .on_select(move |index, _, cx| {
                    this.update(cx, |view, cx| {
                        view.selected = index;
                        cx.notify();
                    })
                    .ok();
                }),
            )
    }
}

#[gpui::test]
fn a_tab_menu_opens_for_the_selected_tab_from_the_keyboard(cx: &mut TestAppContext) {
    use gpui::{Modifiers, MouseButton};
    cx.update(mira_ui::init);
    let chosen = Rc::new(std::cell::RefCell::new(Vec::new()));
    let (_, cx) = cx.add_window_view(|window, cx| {
        let focus = cx.focus_handle();
        window.activate_window();
        window.focus(&focus, cx);
        Profiles {
            focus,
            selected: 0,
            chosen: chosen.clone(),
        }
    });
    cx.run_until_parked();

    press(cx, "tab");
    press(cx, "down");
    assert!(cx.debug_bounds("details-1-card").is_some());
    press(cx, "shift-f10");
    assert!(cx.debug_bounds("details-1-card").is_none());
    press(cx, "enter");
    assert_eq!(*chosen.borrow(), ["rename 1"]);

    let third = cx.debug_bounds("profile-2").unwrap().center();
    cx.simulate_mouse_move(third, None, Modifiers::none());
    cx.run_until_parked();
    cx.simulate_mouse_down(third, MouseButton::Right, Modifiers::none());
    cx.simulate_mouse_up(third, MouseButton::Right, Modifiers::none());
    cx.run_until_parked();
    cx.executor()
        .advance_clock(std::time::Duration::from_secs(1));
    cx.run_until_parked();
    assert!(cx.debug_bounds("details-2-card").is_none());
    press(cx, "down");
    press(cx, "enter");
    assert_eq!(*chosen.borrow(), ["rename 1", "rename 2"]);
}

struct Skills {
    focus: FocusHandle,
    chosen: Rc<Cell<Option<usize>>>,
}

impl Render for Skills {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let chosen = self.chosen.clone();
        div()
            .key_context(FOCUS_SCOPE)
            .track_focus(&self.focus)
            .size_full()
            .child(
                DropdownMenu::new("add")
                    .trigger(Button::new("add-trigger", "Add"))
                    .items(move |_, _| {
                        (0..3)
                            .map(|slot| {
                                let chosen = chosen.clone();
                                MenuItem::action(format!("Skill {slot}"), move |_, _| {
                                    chosen.set(Some(slot))
                                })
                                .key(slot.to_string())
                            })
                            .collect()
                    }),
            )
    }
}

#[gpui::test]
fn an_open_menu_chooses_the_item_whose_key_is_pressed(cx: &mut TestAppContext) {
    cx.update(mira_ui::init);
    let chosen = Rc::new(Cell::new(None));
    let (_, cx) = cx.add_window_view(|window, cx| {
        let focus = cx.focus_handle();
        window.activate_window();
        window.focus(&focus, cx);
        Skills {
            focus,
            chosen: chosen.clone(),
        }
    });
    cx.run_until_parked();
    press(cx, "2");
    assert_eq!(chosen.get(), None);
    press(cx, "tab");
    press(cx, "enter");
    press(cx, "2");
    assert_eq!(chosen.get(), Some(2));
}

struct NoTabs(FocusHandle);

impl Render for NoTabs {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        use mira_ui::primitives::Tabs;
        div()
            .key_context(FOCUS_SCOPE)
            .track_focus(&self.0)
            .size_full()
            .child(Tabs::new("none", "None", 0, |_| unreachable!("there are no tabs")).vertical())
    }
}

#[gpui::test]
fn shift_f10_on_an_empty_tab_list_does_nothing(cx: &mut TestAppContext) {
    cx.update(mira_ui::init);
    let (_, cx) = cx.add_window_view(|window, cx| {
        let focus = cx.focus_handle();
        window.activate_window();
        window.focus(&focus, cx);
        NoTabs(focus)
    });
    cx.run_until_parked();
    press(cx, "tab");
    press(cx, "shift-f10");
}

struct Maps {
    focus: FocusHandle,
    items: Vec<SharedString>,
    marked: Option<Vec<bool>>,
    editable: bool,
}

impl Render for Maps {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .key_context(FOCUS_SCOPE)
            .track_focus(&self.focus)
            .size_full()
            .child({
                let (removing, moving, marking, editing) = (
                    cx.weak_entity(),
                    cx.weak_entity(),
                    cx.weak_entity(),
                    cx.weak_entity(),
                );
                mira_ui::components::Chips::new("maps", "Maps", self.items.clone())
                    .on_remove(move |index, _, cx| {
                        let _ = removing.update(cx, |this, cx| {
                            this.items.remove(index);
                            if let Some(marked) = &mut this.marked {
                                marked.remove(index);
                            }
                            cx.notify();
                        });
                    })
                    .on_reorder(move |from, to, _, cx| {
                        let _ = moving.update(cx, |this, cx| {
                            let item = this.items.remove(from);
                            this.items.insert(to, item);
                            cx.notify();
                        });
                    })
                    .when(self.editable, |chips| {
                        chips.on_edit(move |index, text, _, cx| {
                            let _ = editing.update(cx, |this, cx| {
                                this.items[index] = text.into();
                                cx.notify();
                            });
                        })
                    })
                    .when_some(self.marked.clone(), |chips, marked| {
                        chips
                            .marked(Icon::Bell, Icon::BellRing, "Beeps", marked)
                            .on_mark(move |index, on, _, cx| {
                                let _ = marking.update(cx, |this, cx| {
                                    if let Some(marked) = &mut this.marked {
                                        marked[index] = on;
                                    }
                                    cx.notify();
                                });
                            })
                    })
            })
    }
}

#[gpui::test]
fn chips_move_with_arrows_reorder_with_option_and_remove_from_the_keyboard(
    cx: &mut TestAppContext,
) {
    cx.update(mira_ui::init);
    let (view, cx) = cx.add_window_view(|window, cx| {
        let focus = cx.focus_handle();
        window.activate_window();
        window.focus(&focus, cx);
        Maps {
            focus,
            items: ["battleon", "yulgar", "doomwood", "swordhaven"]
                .map(SharedString::from)
                .to_vec(),
            marked: None,
            editable: false,
        }
    });
    cx.run_until_parked();
    let items = |cx: &mut VisualTestContext| view.read_with(cx, |maps, _| maps.items.clone());
    for key in ["tab", "right", "down", "alt-left", "right", "delete"] {
        press(cx, key);
    }
    assert_eq!(items(cx), ["battleon", "doomwood", "swordhaven"]);
    for key in ["up", "backspace"] {
        press(cx, key);
    }
    assert_eq!(items(cx), ["battleon", "swordhaven"]);
}

#[gpui::test]
fn marked_chips_toggle_with_space_and_remove_with_backspace(cx: &mut TestAppContext) {
    cx.update(mira_ui::init);
    let (view, cx) = cx.add_window_view(|window, cx| {
        let focus = cx.focus_handle();
        window.activate_window();
        window.focus(&focus, cx);
        Maps {
            focus,
            items: ["battleon", "yulgar", "doomwood"]
                .map(SharedString::from)
                .to_vec(),
            marked: Some(vec![false; 3]),
            editable: false,
        }
    });
    cx.run_until_parked();
    let state = |cx: &mut VisualTestContext| {
        view.read_with(cx, |maps, _| {
            (maps.items.clone(), maps.marked.clone().unwrap())
        })
    };
    for key in ["tab", "right", "space"] {
        press(cx, key);
    }
    assert_eq!(state(cx).1, [false, true, false]);
    press(cx, "space");
    assert_eq!(state(cx).1, [false, false, false]);
    for key in ["right", "space", "left", "backspace"] {
        press(cx, key);
    }
    assert_eq!(
        state(cx),
        (
            vec!["battleon".into(), "doomwood".into()],
            vec![false, true]
        )
    );
}

#[gpui::test]
fn enter_edits_a_chip_return_saves_and_escape_cancels(cx: &mut TestAppContext) {
    cx.update(mira_ui::init);
    let (view, cx) = cx.add_window_view(|window, cx| {
        let focus = cx.focus_handle();
        window.activate_window();
        window.focus(&focus, cx);
        Maps {
            focus,
            items: ["battleon", "yulgar"].map(SharedString::from).to_vec(),
            marked: None,
            editable: true,
        }
    });
    cx.run_until_parked();
    let items = |cx: &mut VisualTestContext| view.read_with(cx, |maps, _| maps.items.clone());
    for key in ["tab", "right", "enter", "right"] {
        press(cx, key);
    }
    cx.simulate_input("-9999");
    press(cx, "enter");
    assert_eq!(items(cx), ["battleon", "yulgar-9999"]);

    for key in ["left", "enter"] {
        press(cx, key);
    }
    cx.simulate_input("x");
    press(cx, "escape");
    assert_eq!(items(cx), ["battleon", "yulgar-9999"]);
    press(cx, "backspace");
    assert_eq!(items(cx), ["yulgar-9999"], "the list has focus again");
}

struct PaneUnderPopup {
    pane: gpui::ScrollHandle,
}

impl Render for PaneUnderPopup {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        use gpui::px;
        let rows =
            |count: usize| (0..count).map(|index| div().h(px(40.0)).child(format!("Row {index}")));
        div()
            .size_full()
            .relative()
            .child(
                div()
                    .id("pane")
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.pane)
                    .children(rows(40)),
            )
            .child(mira_ui::primitives::Scrollbar::new(
                "pane-scrollbar",
                self.pane.clone(),
            ))
            .child(
                div()
                    .id("popup")
                    .absolute()
                    .top(px(20.0))
                    .left(px(20.0))
                    .w(px(200.0))
                    .h(px(160.0))
                    .occlude()
                    .overflow_y_scroll()
                    .children(rows(20)),
            )
    }
}

#[gpui::test]
fn scrolling_a_popup_over_a_pane_leaves_the_panes_scrollbar_hidden(cx: &mut TestAppContext) {
    use gpui::{ScrollDelta, ScrollEvent, point, px};
    cx.update(mira_ui::init);
    let (_, cx) = cx.add_window_view(|window, _| {
        window.activate_window();
        PaneUnderPopup {
            pane: gpui::ScrollHandle::new(),
        }
    });
    cx.run_until_parked();
    let wheel = |cx: &mut VisualTestContext, x: f32, y: f32| {
        cx.simulate_event(ScrollEvent {
            position: point(px(x), px(y)),
            delta: ScrollDelta::Pixels(point(px(0.0), px(-80.0))),
            ..Default::default()
        });
        cx.run_until_parked();
        cx.update(|window, cx| window.simulate_next_frame(cx));
        cx.run_until_parked();
    };
    wheel(cx, 100.0, 100.0);
    assert!(
        cx.debug_bounds("scrollbar-thumb").is_none(),
        "scrolling the popup revealed the pane's scrollbar"
    );
    wheel(cx, 400.0, 300.0);
    assert!(cx.debug_bounds("scrollbar-thumb").is_some());
}

struct PreviewsAboveTheFold {
    focus: FocusHandle,
    pane: gpui::ScrollHandle,
}

impl Render for PreviewsAboveTheFold {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        use gpui::px;
        use mira_ui::primitives::{Radio, RadioGroup};
        mira_ui::nav::set_scroll_root(&self.pane, window, cx);
        let thumbnail = || div().w(px(60.0)).h(px(40.0));
        div()
            .key_context(FOCUS_SCOPE)
            .track_focus(&self.focus)
            .size_full()
            .child(
                div()
                    .id("pane")
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.pane)
                    .child(RadioGroup::new(
                        "mode",
                        "Mode",
                        vec![
                            Radio::new("Light").preview(thumbnail()),
                            Radio::new("Dark").preview(thumbnail()),
                        ],
                        Some(0),
                    ))
                    .child(div().h(px(2000.0)))
                    .child(Button::new("below", "Below the fold")),
            )
    }
}

#[gpui::test]
fn tabbing_past_a_selected_preview_reveals_the_next_control(cx: &mut TestAppContext) {
    cx.update(mira_ui::init);
    let (view, cx) = cx.add_window_view(|window, cx| {
        let focus = cx.focus_handle();
        window.activate_window();
        window.focus(&focus, cx);
        PreviewsAboveTheFold {
            focus,
            pane: gpui::ScrollHandle::new(),
        }
    });
    cx.run_until_parked();
    press(cx, "tab");
    press(cx, "tab");
    let offset = cx.update(|_, cx| view.read(cx).pane.offset().y);
    assert!(
        offset < gpui::px(0.0),
        "the button below the fold stayed hidden"
    );
}

#[gpui::test]
fn a_shortcut_field_started_by_a_click_hands_keys_back_when_done(cx: &mut TestAppContext) {
    use gpui::{Modifiers, point, px};
    let fired = Rc::new(Cell::new(false));
    cx.update(|cx| {
        mira_ui::init(cx);
        cx.bind_keys([gpui::KeyBinding::new("cmd-w", CloseWindow, None)]);
        let fired = fired.clone();
        cx.on_action(move |_: &CloseWindow, _| fired.set(true));
    });
    let recorded = Rc::new(Cell::new(Some(Keystroke::parse("cmd-w").unwrap())));
    let next = Rc::new(Cell::new(None));
    let (_, cx) = cx.add_window_view(|window, cx| {
        let focus = cx.focus_handle();
        window.activate_window();
        window.focus(&focus, cx);
        Shortcut {
            focus,
            recorded: recorded.clone(),
            next: next.clone(),
        }
    });
    cx.run_until_parked();
    cx.simulate_click(point(px(20.0), px(12.0)), Modifiers::none());
    cx.run_until_parked();
    press(cx, "backspace");
    assert_eq!(recorded.take(), None);
    press(cx, "cmd-w");
    assert!(
        fired.get(),
        "the app's shortcut stopped working after clearing"
    );
    press(cx, "tab");
    press(cx, "cmd-j");
    assert_eq!(
        next.take(),
        Some(Keystroke::parse("cmd-j").unwrap()),
        "tab should reach the next field after a click-started clear"
    );
}

#[gpui::test]
fn a_rejected_shortcut_keeps_recording_until_a_free_one_is_pressed(cx: &mut TestAppContext) {
    cx.update(mira_ui::init);
    let recorded = Rc::new(Cell::new(None));
    let next = Rc::new(Cell::new(None));
    let (_, cx) = cx.add_window_view(|window, cx| {
        let focus = cx.focus_handle();
        window.activate_window();
        window.focus(&focus, cx);
        Shortcut {
            focus,
            recorded: recorded.clone(),
            next: next.clone(),
        }
    });
    cx.run_until_parked();
    press(cx, "tab");
    press(cx, "cmd-b");
    assert_eq!(recorded.take(), None);
    for key in ["a", "enter", "left"] {
        press(cx, key);
        assert!(
            cx.debug_bounds("shortcut-error").is_some(),
            "{key} cleared the rejection while recording"
        );
    }
    press(cx, "cmd-k");
    assert_eq!(
        recorded.take(),
        Some(Keystroke::parse("cmd-k").unwrap()),
        "the field stopped recording after a rejected shortcut"
    );
}

#[gpui::test]
fn a_keyboard_restart_after_a_click_shows_its_ring_when_done(cx: &mut TestAppContext) {
    use gpui::{Modifiers, point, px};
    cx.update(mira_ui::init);
    let recorded = Rc::new(Cell::new(None));
    let next = Rc::new(Cell::new(None));
    let (_, cx) = cx.add_window_view(|window, cx| {
        let focus = cx.focus_handle();
        window.activate_window();
        window.focus(&focus, cx);
        Shortcut {
            focus,
            recorded: recorded.clone(),
            next: next.clone(),
        }
    });
    cx.run_until_parked();
    cx.simulate_click(point(px(20.0), px(12.0)), Modifiers::none());
    cx.run_until_parked();
    press(cx, "cmd-k");
    assert!(cx.debug_bounds("shortcut-ring").is_none());
    press(cx, "enter");
    press(cx, "cmd-j");
    assert_eq!(recorded.take(), Some(Keystroke::parse("cmd-j").unwrap()));
    assert!(
        cx.debug_bounds("shortcut-ring").is_some(),
        "a recording restarted from the keyboard hid its ring"
    );
}

struct TrailingSwitch(Rc<Cell<Option<bool>>>);

impl Render for TrailingSwitch {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let toggled = self.0.clone();
        div().w(gpui::px(240.0)).child(
            mira_ui::primitives::Switch::new("relogin", "Auto relogin", false)
                .trailing()
                .description("Logs back in when you disconnect")
                .on_toggle(move |on, _, _| toggled.set(Some(on))),
        )
    }
}

#[gpui::test]
fn clicking_a_trailing_switch_label_toggles_it(cx: &mut TestAppContext) {
    use gpui::{Modifiers, point, px};
    cx.update(mira_ui::init);
    let toggled = Rc::new(Cell::new(None));
    let (_, cx) = cx.add_window_view(|_, _| TrailingSwitch(toggled.clone()));
    cx.run_until_parked();
    cx.simulate_click(point(px(20.0), px(14.0)), Modifiers::none());
    cx.run_until_parked();
    assert_eq!(toggled.get(), Some(true));
}
