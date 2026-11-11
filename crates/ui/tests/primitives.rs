use std::{cell::Cell, rc::Rc};

use gpui::{
    Context, FocusHandle, InteractiveElement as _, IntoElement, KeyUpEvent, Keystroke,
    ParentElement as _, Render, SharedString, Styled as _, TestAppContext, VisualTestContext,
    Window, div,
};
use mira_ui::{
    FOCUS_SCOPE,
    primitives::{Button, Dialog, Select},
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
fn select_ignores_a_selection_past_its_options(cx: &mut TestAppContext) {
    assert_eq!(rooms(Some(3), "tab enter enter", cx), None);
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
        use mira_ui::components::{Activity, StatusLabel};
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
