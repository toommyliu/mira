use std::path::PathBuf;

use gpui::{
    AppContext as _, Context, Entity, EventEmitter, FocusHandle, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, ScrollHandle, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, Window, div,
    prelude::FluentBuilder as _, px,
};
use mira_ui::{
    ActiveTheme as _, h_flex,
    primitives::{Button, Dialog, button::Variant},
};

use super::{
    inputs_form::{self, InputsForm, Read, Submitted, reveal_item},
    script_names,
};
use crate::{
    script_inputs,
    scripting::{Declared, Supplied, Values},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Purpose {
    Fill,
    Start,
    Edit,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Preflight {
    Start,
    Ask,
}

pub(super) fn preflight(read: &Read, saved: &Supplied) -> Preflight {
    let Ok(Declared {
        inputs: Ok(Some(definition)),
        ..
    }) = read
    else {
        return Preflight::Start;
    };
    match saved {
        Supplied::Values(values) if definition.resolve(values).is_ok() => Preflight::Start,
        Supplied::Values(_) | Supplied::StoreUnreadable => Preflight::Ask,
    }
}

pub(super) enum InputsEvent {
    Start(Values),
    Closed,
}

impl EventEmitter<InputsEvent> for InputsDialog {}

pub(super) struct InputsDialog {
    script: PathBuf,
    purpose: Purpose,
    running: bool,
    form: Entity<InputsForm>,
    open: bool,
    scroll: ScrollHandle,
    cursor: Option<usize>,
    focus: Option<FocusHandle>,
    reveal: Option<usize>,
    _watch: [Subscription; 2],
}

impl InputsDialog {
    pub(super) fn new(
        script: PathBuf,
        read: Read,
        purpose: Purpose,
        running: bool,
        cx: &mut Context<Self>,
    ) -> Self {
        let form = cx.new(|cx| InputsForm::saved(script.clone(), read, cx));
        let watch = [
            cx.observe(&form, |_, _, cx| cx.notify()),
            cx.subscribe(&form, |this, _, _: &Submitted, cx| this.accept(cx)),
        ];
        let mut dialog = Self {
            script,
            purpose,
            running,
            form,
            open: true,
            scroll: ScrollHandle::new(),
            cursor: None,
            focus: None,
            reveal: None,
            _watch: watch,
        };
        dialog.focus_first(cx);
        dialog
    }

    pub(super) fn script(&self) -> &PathBuf {
        &self.script
    }

    pub(super) fn open(&self) -> bool {
        self.open
    }

    pub(super) fn ask_to_start(&mut self, cx: &mut Context<Self>) {
        self.purpose = Purpose::Start;
        cx.notify();
    }

    fn focus_first(&mut self, cx: &mut Context<Self>) {
        let form = self.form.read(cx);
        let field = form.errored(cx).first().copied().or_else(|| {
            form.definition().and_then(|definition| {
                (0..definition.fields().len()).find(|&field| form.text(field).is_some())
            })
        });
        self.reveal = field;
        self.focus = field.and_then(|field| form.focus_handle(field, cx));
    }

    fn accept(&mut self, cx: &mut Context<Self>) {
        if !self.open {
            return;
        }
        if self.form.read(cx).step(self.cursor, 1, cx).is_some() {
            return self.step(1, cx);
        }
        match self.purpose {
            Purpose::Start if script_inputs::unreadable(cx) => {}
            Purpose::Start => {
                let Some(values) = self.form.read(cx).ready_values(cx) else {
                    return;
                };
                self.close(cx);
                cx.emit(InputsEvent::Start(values));
            }
            Purpose::Fill | Purpose::Edit => self.close(cx),
        }
    }

    fn close(&mut self, cx: &mut Context<Self>) {
        if !self.open {
            return;
        }
        self.open = false;
        self.reveal = None;
        cx.emit(InputsEvent::Closed);
        cx.notify();
    }

    fn step(&mut self, by: isize, cx: &mut Context<Self>) {
        let form = self.form.read(cx);
        let Some((cursor, field)) = form.step(self.cursor, by, cx) else {
            return;
        };
        let item = form.item_of(field, cx);
        let focus = form.focus_handle(field, cx);
        let margin = cx.theme().space(1.0);
        reveal_item(&self.scroll, item, margin, margin);
        self.cursor = Some(cursor);
        self.focus = focus;
        cx.notify();
    }

    fn description(&self) -> Option<&'static str> {
        match self.purpose {
            Purpose::Fill => Some("Fill in its inputs before it starts."),
            Purpose::Start => Some("Fill in its inputs to start it."),
            Purpose::Edit if self.running => Some("Changes apply the next time it starts."),
            Purpose::Edit => None,
        }
    }
}

impl Render for InputsDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(focus) = self.focus.take() {
            window.defer(cx, move |window, cx| {
                window.defer(cx, move |window, cx| window.focus(&focus, cx));
            });
        }
        let theme = cx.theme().clone();
        if let Some(field) = self.reveal {
            if self.scroll.bounds_for_item(0).is_some() {
                let item = self.form.read(cx).item_of(field, cx);
                let margin = theme.space(1.0);
                reveal_item(&self.scroll, item, margin, margin);
                self.reveal = None;
            } else {
                window.request_animation_frame();
            }
        }
        let errors = self.form.read(cx).errored(cx).len();
        let title: SharedString = script_names::name(&self.script, cx);
        let start = self.purpose == Purpose::Start;
        let blocked = start && script_inputs::unreadable(cx);
        let steps = (errors > 0).then(|| {
            let step = cx.listener(|this, by: &isize, _, cx| this.step(*by, cx));
            h_flex()
                .id("script-inputs-errors")
                .role(gpui::Role::Toolbar)
                .aria_label("Input errors")
                .gap(theme.space(0.5))
                .children(inputs_form::error_steps(errors, &theme, step))
        });
        let body = inputs_form::elements(&self.form, &theme, cx);
        let dialog = Dialog::new(
            SharedString::from(format!("script-inputs-{}", cx.entity_id())),
            self.open,
            title,
        )
        .when_some(self.description(), Dialog::description)
        .max_height((window.viewport_size().height * 0.6).min(px(480.0)))
        .scroll(self.scroll.clone());
        let dialog = body.into_iter().fold(dialog, Dialog::body);
        div().child(
            dialog
                .when_some(steps, Dialog::footer_start_slot)
                .when(start, |dialog| {
                    dialog.action(
                        Button::new("script-inputs-cancel", "Cancel")
                            .on_click(cx.listener(|this, _, _, cx| this.close(cx))),
                    )
                })
                .action(
                    Button::new("script-inputs-accept", if start { "Start" } else { "Done" })
                        .variant(Variant::Primary)
                        .disabled(blocked)
                        .on_click(cx.listener(|this, _, _, cx| this.accept(cx))),
                )
                .on_close({
                    let this = cx.weak_entity();
                    move |_, cx| {
                        let _ = this.update(cx, |this, cx| this.close(cx));
                    }
                }),
        )
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, fs, path::Path, rc::Rc};

    use gpui::{
        Focusable as _, KeyDownEvent, KeyUpEvent, Keystroke, TestAppContext, VisualTestContext, px,
    };
    use mira_ui::FOCUS_SCOPE;
    use serde_json::json;

    use super::*;
    use crate::{ruffle_view::account::file_name, scripting};

    const HUNT: &str = r#"export const inputs = {
        kills: { type: "number", label: "Kills", integer: true, min: 1, default: 5 },
        monster: { type: "string", label: "Monster", required: true },
    };"#;

    struct Host {
        focus: FocusHandle,
        dialog: Entity<InputsDialog>,
        events: Rc<RefCell<Vec<&'static str>>>,
        started: Rc<RefCell<Option<Values>>>,
        _events: Subscription,
    }

    impl Render for Host {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let theme = cx.theme();
            div()
                .key_context(FOCUS_SCOPE)
                .track_focus(&self.focus)
                .size_full()
                .bg(theme.background)
                .text_color(theme.foreground)
                .font_family(theme.font_family.clone())
                .text_size(theme.typography.body)
                .child(self.dialog.clone())
        }
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let mut random = [0; 8];
        getrandom::getrandom(&mut random).unwrap();
        let folder = std::env::temp_dir().join(format!(
            "mira-inputs-dialog-{name}-{:x}",
            u64::from_ne_bytes(random)
        ));
        fs::create_dir_all(&folder).unwrap();
        folder
    }

    fn read(script: &Path) -> Read {
        let text = fs::read_to_string(script).unwrap();
        Ok(scripting::extract(&file_name(script), script, &text).unwrap())
    }

    fn values(value: serde_json::Value) -> Values {
        value.as_object().unwrap().clone()
    }

    fn host<'a>(
        cx: &'a mut TestAppContext,
        folder: &Path,
        source: &str,
        purpose: Purpose,
    ) -> (&'a mut VisualTestContext, Entity<Host>, std::path::PathBuf) {
        let script = folder.join("hunt.ts");
        fs::write(&script, source).unwrap();
        cx.update(mira_ui::init);
        let inputs = folder.join("inputs.json");
        let window = cx.open_window(gpui::size(px(960.0), px(588.0)), |window, cx| {
            script_inputs::init(inputs, cx);
            let focus = cx.focus_handle();
            window.activate_window();
            window.focus(&focus, cx);
            let read = read(&script);
            let dialog = cx.new(|cx| InputsDialog::new(script.clone(), read, purpose, false, cx));
            let events = Rc::new(RefCell::new(Vec::new()));
            let started = Rc::new(RefCell::new(None));
            let (seen, ran) = (events.clone(), started.clone());
            let subscription = cx.subscribe(&dialog, move |_, _, event: &InputsEvent, _| {
                seen.borrow_mut().push(match event {
                    InputsEvent::Start(values) => {
                        *ran.borrow_mut() = Some(values.clone());
                        "start"
                    }
                    InputsEvent::Closed => "closed",
                })
            });
            Host {
                focus,
                dialog,
                events,
                started,
                _events: subscription,
            }
        });
        let host = window.root(cx).unwrap();
        let cx = VisualTestContext::from_window(window.into(), cx).into_mut();
        cx.run_until_parked();
        (cx, host, script)
    }

    fn press(cx: &mut VisualTestContext, key: &'static str) {
        cx.simulate_keystrokes(key);
        cx.simulate_event(KeyUpEvent {
            keystroke: Keystroke::parse(key).unwrap(),
        });
        cx.run_until_parked();
    }

    fn form(host: &Entity<Host>, cx: &mut VisualTestContext) -> Entity<InputsForm> {
        host.read_with(cx, |host, cx| host.dialog.read(cx).form.clone())
    }

    fn text(
        host: &Entity<Host>,
        field: usize,
        cx: &mut VisualTestContext,
    ) -> Entity<mira_ui::primitives::Input> {
        form(host, cx).read_with(cx, |form, _| form.text(field).expect("a text field"))
    }

    fn focused(input: &Entity<mira_ui::primitives::Input>, cx: &mut VisualTestContext) -> bool {
        cx.update(|window, cx| input.focus_handle(cx).is_focused(window))
    }

    fn state(host: &Entity<Host>, cx: &mut VisualTestContext) -> (bool, Vec<&'static str>) {
        host.read_with(cx, |host, cx| {
            (host.dialog.read(cx).open(), host.events.borrow().clone())
        })
    }

    #[test]
    fn only_inputs_that_cant_resolve_hold_up_a_start() {
        let folder = scratch("preflight");
        let script = folder.join("hunt.ts");
        fs::write(&script, HUNT).unwrap();
        let declared = read(&script);
        let saved = |value| Supplied::Values(values(value));
        assert_eq!(
            preflight(&declared, &saved(json!({ "monster": "Frogzard" }))),
            Preflight::Start
        );
        assert_eq!(
            preflight(&declared, &saved(json!({}))),
            Preflight::Ask,
            "Monster has no default"
        );
        assert_eq!(
            preflight(
                &declared,
                &saved(json!({ "monster": "Frogzard", "kills": 0 }))
            ),
            Preflight::Ask
        );
        assert_eq!(
            preflight(&declared, &Supplied::StoreUnreadable),
            Preflight::Ask
        );

        fs::write(&script, "export default function* () {}").unwrap();
        assert_eq!(
            preflight(&read(&script), &Supplied::StoreUnreadable),
            Preflight::Start
        );
        let broken: Read = Err(("This script has a syntax error", "hunt.ts:1:1".into()));
        assert_eq!(
            preflight(&broken, &saved(json!({}))),
            Preflight::Start,
            "the run reports it"
        );
        fs::remove_dir_all(folder).unwrap();
    }

    #[gpui::test]
    fn start_steps_through_what_needs_filling_in_then_starts(cx: &mut TestAppContext) {
        let folder = scratch("start");
        let (cx, host, script) = host(
            cx,
            &folder,
            include_str!("../../examples/inputs.ts"),
            Purpose::Start,
        );
        let offset = text(&host, 4, cx);
        assert!(
            focused(&offset, cx),
            "the first input to fill in takes focus"
        );

        press(cx, "enter");
        assert_eq!(
            state(&host, cx),
            (true, vec![]),
            "Return steps to an error instead of starting"
        );
        assert!(focused(&offset, cx));

        offset.update(cx, |input, cx| input.set_text("3", cx));
        cx.run_until_parked();
        press(cx, "enter");
        assert_eq!(
            state(&host, cx),
            (true, vec![]),
            "Room still needs choosing"
        );
        let room = form(&host, cx)
            .read_with(cx, |form, cx| form.focus_handle(8, cx))
            .unwrap();
        assert!(cx.update(|window, _| room.is_focused(window)));

        for _ in 0..2 {
            cx.simulate_event(KeyDownEvent {
                keystroke: Keystroke::parse("enter").unwrap(),
                is_held: true,
                prefer_character_input: false,
            });
            cx.run_until_parked();
        }
        assert_eq!(
            form(&host, cx).read_with(cx, |form, cx| form.errored(cx)),
            vec![8],
            "a Return held from the field before doesn't choose a room"
        );

        press(cx, "enter");
        press(cx, "down");
        press(cx, "enter");
        cx.update(|window, cx| window.focus(&offset.focus_handle(cx), cx));
        press(cx, "enter");
        assert_eq!(state(&host, cx), (false, vec!["closed", "start"]));
        let saved = cx.update(|_, cx| script_inputs::values(&script, cx).unwrap());
        assert_eq!(saved["offset"], json!(3));
        assert_eq!(saved["room"], json!("Private"));
        fs::remove_dir_all(folder).unwrap();
    }

    #[gpui::test]
    fn opens_scrolled_to_the_first_field_to_fix(cx: &mut TestAppContext) {
        let folder = scratch("reveal");
        let fields = (0..12)
            .map(|index| {
                format!("n{index}: {{ type: \"number\", label: \"N{index}\", default: 1 }},")
            })
            .collect::<String>();
        let source = format!(
            "export const inputs = {{ {fields} last: {{ type: \"string\", label: \"Last\", required: true }} }};"
        );
        let (cx, host, _) = host(cx, &folder, &source, Purpose::Edit);
        assert!(focused(&text(&host, 12, cx), cx));
        let (viewport, item) = host.read_with(cx, |host, cx| {
            let dialog = host.dialog.read(cx);
            let item = dialog.form.read(cx).item_of(12, cx);
            (
                dialog.scroll.bounds(),
                dialog.scroll.bounds_for_item(item).unwrap(),
            )
        });
        let offset = host.read_with(cx, |host, cx| host.dialog.read(cx).scroll.offset());
        assert!(
            item.top() + offset.y >= viewport.top()
                && item.bottom() + offset.y <= viewport.bottom(),
            "{item:?} shows in {viewport:?} at {offset:?}"
        );
        fs::remove_dir_all(folder).unwrap();
    }

    #[gpui::test]
    fn opens_with_focus_on_a_dropdown_to_fix(cx: &mut TestAppContext) {
        let folder = scratch("dropdown");
        let source = r#"export const inputs = {
            monster: { type: "string", label: "Monster", default: "Frogzard" },
            room: { type: "select", label: "Room", options: ["Public", "Private"] },
        };"#;
        let (cx, host, script) = host(cx, &folder, source, Purpose::Edit);
        let room = form(&host, cx)
            .read_with(cx, |form, cx| form.focus_handle(1, cx))
            .expect("the dropdown takes focus");
        assert!(cx.update(|window, _| room.is_focused(window)));
        press(cx, "enter");
        press(cx, "enter");
        assert_eq!(
            cx.update(|_, cx| script_inputs::values(&script, cx).unwrap()),
            values(json!({ "monster": "Frogzard", "room": "Public" }))
        );
        fs::remove_dir_all(folder).unwrap();
    }

    #[gpui::test]
    fn a_field_taller_than_the_dialog_opens_at_its_input(cx: &mut TestAppContext) {
        let folder = scratch("tall");
        let description = "Long. ".repeat(400);
        let source = format!(
            "export const inputs = {{ loot: {{ type: \"boolean\", label: \"Loot\" }}, note: {{ type: \"string\", label: \"Note\", description: \"{description}\" }} }};"
        );
        let (cx, host, _) = host(cx, &folder, &source, Purpose::Edit);
        assert!(focused(&text(&host, 1, cx), cx));
        let (viewport, row, offset) = host.read_with(cx, |host, cx| {
            let scroll = &host.dialog.read(cx).scroll;
            (
                scroll.bounds(),
                scroll.bounds_for_item(1).unwrap(),
                scroll.offset(),
            )
        });
        assert!(
            row.size.height > viewport.size.height,
            "{row:?} is taller than {viewport:?}"
        );
        assert!(
            row.top() + offset.y >= viewport.top(),
            "the input at the top of {row:?} shows in {viewport:?} at {offset:?}"
        );
        fs::remove_dir_all(folder).unwrap();
    }

    #[gpui::test]
    fn escape_closes_and_keeps_what_was_saved(cx: &mut TestAppContext) {
        let folder = scratch("escape");
        fs::write(folder.join("inputs.json"), "{}").unwrap();
        let (cx, host, script) = host(cx, &folder, HUNT, Purpose::Edit);
        assert!(
            focused(&text(&host, 1, cx), cx),
            "the first field to fix takes focus"
        );
        let kills = text(&host, 0, cx);

        for _ in 0..3 {
            cx.simulate_event(KeyDownEvent {
                keystroke: Keystroke::parse("enter").unwrap(),
                is_held: true,
                prefer_character_input: false,
            });
            cx.run_until_parked();
        }
        cx.simulate_event(KeyUpEvent {
            keystroke: Keystroke::parse("enter").unwrap(),
        });
        cx.run_until_parked();
        assert_eq!(
            state(&host, cx),
            (true, vec![]),
            "a Return held from before doesn't answer"
        );

        text(&host, 1, cx).update(cx, |input, cx| input.set_text("Frogzard", cx));
        kills.update(cx, |input, cx| input.set_text("7", cx));
        cx.run_until_parked();
        press(cx, "escape");
        assert_eq!(state(&host, cx), (false, vec!["closed"]));
        assert_eq!(
            cx.update(|_, cx| script_inputs::values(&script, cx).unwrap()),
            values(json!({ "kills": 7, "monster": "Frogzard" }))
        );
        fs::remove_dir_all(folder).unwrap();
    }

    #[gpui::test]
    fn start_runs_what_the_form_shows_when_saving_fails(cx: &mut TestAppContext) {
        use std::os::unix::fs::PermissionsExt as _;

        let folder = scratch("unwritable");
        let (cx, host, script) = host(cx, &folder, HUNT, Purpose::Start);
        fs::set_permissions(&folder, fs::Permissions::from_mode(0o555)).unwrap();
        text(&host, 1, cx).update(cx, |input, cx| input.set_text("Frogzard", cx));
        cx.run_until_parked();
        press(cx, "enter");
        fs::set_permissions(&folder, fs::Permissions::from_mode(0o755)).unwrap();

        assert_eq!(state(&host, cx), (false, vec!["closed", "start"]));
        assert_eq!(
            host.read_with(cx, |host, _| host.started.borrow().clone()),
            Some(values(json!({ "kills": 5, "monster": "Frogzard" })))
        );
        assert!(cx.update(|_, cx| script_inputs::unsaved(cx)));
        assert_eq!(
            cx.update(|_, cx| script_inputs::values(&script, cx).unwrap()),
            values(json!({ "kills": 5, "monster": "Frogzard" })),
            "inputs that failed to save still apply"
        );
        fs::remove_dir_all(folder).unwrap();
    }

    #[gpui::test]
    fn start_waits_for_unreadable_inputs_to_start_fresh(cx: &mut TestAppContext) {
        let folder = scratch("unreadable");
        fs::write(folder.join("inputs.json"), "{ \"oops\": ").unwrap();
        let (cx, host, script) = host(
            cx,
            &folder,
            r#"export const inputs = { kills: { type: "number", label: "Kills", default: 5 } };"#,
            Purpose::Start,
        );
        let kills = text(&host, 0, cx);
        assert!(focused(&kills, cx));
        press(cx, "enter");
        assert_eq!(state(&host, cx), (true, vec![]));

        cx.update(|_, cx| script_inputs::start_fresh(cx));
        cx.run_until_parked();
        press(cx, "enter");
        assert_eq!(state(&host, cx), (false, vec!["closed", "start"]));
        assert_eq!(
            cx.update(|_, cx| script_inputs::values(&script, cx).unwrap()),
            values(json!({ "kills": 5 }))
        );
        fs::remove_dir_all(folder).unwrap();
    }

    fn snapshot(
        name: &str,
        width: f32,
        height: f32,
        source: &'static str,
        saved: Option<&'static str>,
        purpose: Purpose,
        running: bool,
    ) {
        let folder = scratch(name);
        let script = folder.join("hunt.ts");
        fs::write(&script, source).unwrap();
        if let Some(saved) = saved {
            fs::write(folder.join("inputs.json"), saved).unwrap();
        }
        let inputs = folder.join("inputs.json");
        let paths = mira_ui::snapshot(
            &format!("script-inputs-{name}"),
            gpui::size(px(width), px(height)),
            move |window, cx| {
                script_inputs::init(inputs.clone(), cx);
                let script = script.clone();
                cx.new(|cx| shown(script, purpose, running, window, cx))
            },
        );
        for path in paths {
            assert!(path.exists(), "{} was not written", path.display());
        }
        fs::remove_dir_all(folder).unwrap();
    }

    fn shown(
        script: PathBuf,
        purpose: Purpose,
        running: bool,
        window: &mut Window,
        cx: &mut Context<Host>,
    ) -> Host {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        let read = read(&script);
        let dialog = cx.new(|cx| InputsDialog::new(script, read, purpose, running, cx));
        let subscription = cx.subscribe(&dialog, |_, _, _: &InputsEvent, _| {});
        Host {
            focus,
            dialog,
            events: Rc::default(),
            started: Rc::default(),
            _events: subscription,
        }
    }

    #[test]
    fn snapshot_script_inputs_dialog_unsaved() {
        use std::os::unix::fs::PermissionsExt as _;

        let folder = scratch("unsaved");
        let script = folder.join("hunt.ts");
        fs::write(&script, HUNT).unwrap();
        fs::write(folder.join("inputs.json"), "{}").unwrap();
        fs::set_permissions(&folder, fs::Permissions::from_mode(0o555)).unwrap();
        let inputs = folder.join("inputs.json");
        let paths = mira_ui::snapshot(
            "script-inputs-unsaved",
            gpui::size(px(960.0), px(588.0)),
            move |window, cx| {
                script_inputs::init(inputs.clone(), cx);
                let edited = values(json!({ "kills": 7, "monster": "Frogzard" }));
                script_inputs::save(&script, edited, cx);
                let script = script.clone();
                cx.new(|cx| shown(script, Purpose::Edit, false, window, cx))
            },
        );
        fs::set_permissions(&folder, fs::Permissions::from_mode(0o755)).unwrap();
        for path in paths {
            assert!(path.exists(), "{} was not written", path.display());
        }
        fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn snapshot_script_inputs_dialog() {
        let example = include_str!("../../examples/inputs.ts");
        snapshot("fill", 960.0, 588.0, example, None, Purpose::Fill, false);
        snapshot(
            "fill-tall",
            960.0,
            1200.0,
            example,
            None,
            Purpose::Fill,
            false,
        );
        snapshot(
            "start-min",
            920.0,
            566.0,
            example,
            None,
            Purpose::Start,
            false,
        );
        snapshot(
            "unreadable",
            960.0,
            588.0,
            HUNT,
            Some("{ \"oops\": "),
            Purpose::Start,
            false,
        );
        snapshot(
            "edit-running",
            960.0,
            588.0,
            HUNT,
            Some("{}"),
            Purpose::Edit,
            true,
        );
        snapshot(
            "broken",
            960.0,
            588.0,
            r#"export const inputs = { kills: { type: "nope", label: "Kills" } };"#,
            None,
            Purpose::Edit,
            false,
        );
        snapshot(
            "no-inputs",
            960.0,
            588.0,
            "export default function* () {}",
            None,
            Purpose::Edit,
            false,
        );
    }
}
