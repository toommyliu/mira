use std::{cell::Cell, rc::Rc};

use gpui::{
    Context, FocusHandle, InteractiveElement as _, IntoElement, KeyUpEvent, Keystroke,
    ParentElement as _, Render, SharedString, Styled as _, TestAppContext, VisualTestContext,
    Window, div, prelude::FluentBuilder as _,
};
use mira_ui::{
    FOCUS_SCOPE,
    primitives::{Button, Dialog, DropdownMenu, Icon, MenuItem, Select},
};

struct Rooms {
    focus: FocusHandle,
    selected: Option<usize>,
    picked: Rc<Cell<Option<usize>>>,
}

impl Render for Rooms {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let options = ["Yulgar", "Battleon", "Swordhaven"].map(SharedString::from);
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
fn select_opens_on_the_first_option_without_a_valid_selection(cx: &mut TestAppContext) {
    assert_eq!(rooms(None, "tab enter enter", cx), Some(0));
}

#[gpui::test]
fn select_treats_a_selection_past_its_options_as_none(cx: &mut TestAppContext) {
    assert_eq!(rooms(Some(3), "tab enter enter", cx), Some(0));
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
    let tenth = point(
        select.center().x,
        select.bottom() + px(4.0 + 5.0 + 24.0 * 10.0 + 12.0),
    );
    cx.simulate_mouse_down(tenth, MouseButton::Left, Modifiers::none());
    std::thread::sleep(std::time::Duration::from_millis(150));
    cx.update(|window, _| window.refresh());
    cx.run_until_parked();
    cx.simulate_mouse_up(tenth, MouseButton::Left, Modifiers::none());
    cx.run_until_parked();
    assert_eq!(picked.get(), Some(10));
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

gpui::actions!(test, [CloseWindow]);

struct Shortcut {
    focus: FocusHandle,
    recorded: Rc<Cell<Option<Keystroke>>>,
}

impl Render for Shortcut {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let recorded = self.recorded.clone();
        div()
            .key_context(FOCUS_SCOPE)
            .track_focus(&self.focus)
            .size_full()
            .child(
                mira_ui::components::ShortcutField::new("hotkey", "Hotkey", None)
                    .on_change(move |keystroke, _, _| recorded.set(keystroke)),
            )
    }
}

#[gpui::test]
fn shortcut_field_records_combinations_the_app_binds(cx: &mut TestAppContext) {
    let fired = Rc::new(Cell::new(false));
    cx.update(|cx| {
        mira_ui::init(cx);
        cx.bind_keys([
            gpui::KeyBinding::new("cmd-w", CloseWindow, None),
            gpui::KeyBinding::new("ctrl-shift-tab", CloseWindow, None),
        ]);
        let fired = fired.clone();
        cx.on_action(move |_: &CloseWindow, _| fired.set(true));
    });
    let recorded = Rc::new(Cell::new(None));
    let (view, cx) = cx.add_window_view(|window, cx| {
        let focus = cx.focus_handle();
        window.activate_window();
        window.focus(&focus, cx);
        Shortcut {
            focus,
            recorded: recorded.clone(),
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
fn password_input_shows_a_bullet_per_character_and_keeps_it_off_the_clipboard(
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
}

#[gpui::test]
fn input_scrolls_to_keep_the_caret_in_view(cx: &mut TestAppContext) {
    use gpui::{Modifiers, point, px};
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
    let one_line = gpui::px(6.0) + mira_ui::theme::LINE_BODY + mira_ui::theme::LINE_SMALL;
    assert!(cx.debug_bounds("switch").unwrap().size.height > one_line);
}

struct Host {
    renders: Rc<Cell<usize>>,
}

impl Render for Host {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.renders.set(self.renders.get() + 1);
        div()
            .size_full()
            .child(mira_ui::primitives::Toaster::layer(window, cx))
    }
}

#[gpui::test]
fn pushing_a_toast_redraws_the_window_that_shows_the_stack(cx: &mut TestAppContext) {
    cx.update(mira_ui::init);
    let renders = Rc::new(Cell::new(0));
    let (_, cx) = cx.add_window_view(|_, _| Host {
        renders: renders.clone(),
    });
    cx.run_until_parked();
    let before = renders.get();
    cx.update(|_, cx| {
        mira_ui::primitives::Toaster::push(mira_ui::primitives::Toast::new("Saved"), cx)
    });
    cx.run_until_parked();
    assert!(renders.get() > before);
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
        div()
            .key_context(FOCUS_SCOPE)
            .track_focus(&self.focus)
            .size_full()
            .flex()
            .flex_col()
            .child(
                Menubar::new("bar", "Game")
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
                    }),
            )
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
        use mira_ui::primitives::{MenuItem, Tab, Tabs};
        let chosen = self.chosen.clone();
        let this = cx.weak_entity();
        div()
            .key_context(FOCUS_SCOPE)
            .track_focus(&self.focus)
            .size_full()
            .child(
                Tabs::new("profiles", "Profiles", 3, move |index| {
                    let chosen = chosen.clone();
                    Tab::new(["Generic", "Farming", "Boss"][index]).menu(move |_, _| {
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
    use gpui::{Modifiers, MouseButton, point, px};
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
    press(cx, "shift-f10");
    press(cx, "enter");
    assert_eq!(*chosen.borrow(), ["rename 1"]);

    let third = point(px(20.0), px(83.0));
    cx.simulate_mouse_down(third, MouseButton::Right, Modifiers::none());
    cx.simulate_mouse_up(third, MouseButton::Right, Modifiers::none());
    cx.run_until_parked();
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
                        chips.marked(Icon::Bell, "Beeps", marked).on_mark(
                            move |index, on, _, cx| {
                                let _ = marking.update(cx, |this, cx| {
                                    if let Some(marked) = &mut this.marked {
                                        marked[index] = on;
                                    }
                                    cx.notify();
                                });
                            },
                        )
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
