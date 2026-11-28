use std::{
    rc::Rc,
    sync::{Arc, LazyLock},
};

use gpui::{
    AnyElement, App, AppContext as _, Bounds, Context, Div, Entity, FocusHandle, Focusable as _,
    FontWeight, Global, InteractiveElement as _, IntoElement, KeyBinding, ParentElement as _,
    Pixels, Render, Role, ScrollHandle, SharedString, StatefulInteractiveElement as _, Styled as _,
    Subscription, TitlebarOptions, WeakEntity, Window, WindowBounds, WindowHandle, WindowOptions,
    actions, div, prelude::FluentBuilder as _, px, size,
};
use mira_ui::{
    ActiveTheme as _, FOCUS_SCOPE, StyledExt as _, Theme, Tone,
    components::Empty,
    h_flex,
    primitives::{
        Alert, Align, Button, Dialog, DropdownMenu, Icon, Input, InputEvent, Kbd, List, ListItem,
        MenuItem, Numeric, Scrollbar, ToggleGroup,
        button::{Size, Variant},
    },
    v_flex,
};

use super::{
    client::{
        CloseSession, bar_separator, bar_title, drag_area, refocus_root_when_lost, window_bar,
    },
    form,
    profile_form::{Form, FormStep, FormTrigger, Item, Rule, Subject},
};
use crate::{
    combat_profiles,
    domain::{
        combat::Skill,
        profiles::{GENERIC, Library, Measure, OnCooldown, Profile, check_name, same_name},
        rotation::Lane,
    },
};

actions!(mira, [OpenCombatProfiles, SaveProfile, NewProfile]);

const CONTEXT: &str = "CombatProfiles";
const WIDTH: f32 = 800.0;
const HEIGHT: f32 = 640.0;
const MIN_WIDTH: f32 = 660.0;
const MIN_HEIGHT: f32 = 420.0;

const PADDING: Pixels = px(16.0);
const PROFILE_WIDTH: Pixels = px(180.0);
const SUBJECT_WIDTH: Pixels = px(96.0);
const SUBJECT_MENU_WIDTH: Pixels = px(140.0);
const UNIT_WIDTH: Pixels = px(56.0);
const MILLIS_WIDTH: Pixels = px(72.0);
const VALUE_WIDTH: Pixels = px(44.0);
const CONJUNCTION_WIDTH: Pixels = px(24.0);
const CONDITION_GAP: Pixels = px(8.0);
const NEW_NAME: &str = "Untitled profile";
const SLOTS: [u8; 6] = [0, 1, 2, 3, 4, 5];
const SKILLS: [&str; 6] = [
    "Auto attack",
    "Skill 1",
    "Skill 2",
    "Skill 3",
    "Skill 4",
    "Consumable",
];
const MILLIS: Numeric = Numeric {
    min: 0,
    max: 600_000,
    step: 100,
};
static GENERIC_FORM: LazyLock<Form> = LazyLock::new(|| Form::of(&Profile::generic()));

struct Open(WindowHandle<Editor>);

impl Global for Open {}

pub(crate) fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-s", SaveProfile, Some(CONTEXT)),
        KeyBinding::new("cmd-n", NewProfile, Some(CONTEXT)),
    ]);
}

pub(crate) fn open(selected: Option<String>, cx: &mut App) {
    if let Some(editor) = cx.try_global::<Open>().map(|open| open.0)
        && editor
            .update(cx, |editor, window, cx| {
                window.activate_window();
                if let Some(name) = &selected {
                    editor.select_named(name, window, cx);
                }
            })
            .is_ok()
    {
        return;
    }
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
            None,
            size(px(WIDTH), px(HEIGHT)),
            cx,
        ))),
        window_min_size: Some(size(px(MIN_WIDTH), px(MIN_HEIGHT))),
        titlebar: Some(TitlebarOptions {
            title: Some("Combat profiles".into()),
            appears_transparent: true,
            traffic_light_position: Some(gpui::point(px(12.0), px(12.0))),
        }),
        app_owns_titlebar_drag: true,
        ..Default::default()
    };
    match cx.open_window(options, |window, cx| {
        let editor = cx.new(|cx| Editor::new(window, cx));
        editor.update(cx, |editor, cx| {
            if let Some(name) = &selected {
                editor.select_named(name, window, cx);
            }
        });
        let root = editor.read(cx).focus.clone();
        window.focus(&root, cx);
        refocus_root_when_lost(&editor, root, window, cx);
        let weak = editor.downgrade();
        window.on_window_should_close(cx, move |_, cx| {
            weak.update(cx, |editor, cx| editor.may_close(cx))
                .unwrap_or(true)
        });
        editor
    }) {
        Ok(editor) => cx.set_global(Open(editor)),
        Err(error) => tracing::error!(%error, "couldn't open combat profiles"),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Section {
    Triggers,
    Steps(Lane),
}

impl Section {
    const ALL: [Section; 3] = [
        Section::Triggers,
        Section::Steps(Lane::Priority),
        Section::Steps(Lane::Rotation),
    ];

    fn item(self, index: usize) -> Item {
        match self {
            Section::Triggers => Item::Trigger(index),
            Section::Steps(lane) => Item::Step(lane, index),
        }
    }

    fn id(self) -> &'static str {
        match self {
            Section::Triggers => "triggers",
            Section::Steps(Lane::Priority) => "priority",
            Section::Steps(Lane::Rotation) => "rotation",
        }
    }

    fn copy(self) -> (&'static str, &'static str, &'static str) {
        match self {
            Section::Triggers => (
                "Triggers",
                "Casts once when a message appears, before anything else.",
                "No triggers.",
            ),
            Section::Steps(Lane::Priority) => (
                "Priority",
                "Checked first, top to bottom. Never waits for a cooldown.",
                "No priority skills.",
            ),
            Section::Steps(Lane::Rotation) => (
                "Rotation",
                "Cast in turn from where the last cast left off. Starts over on a new target.",
                "No skills in the rotation.",
            ),
        }
    }
}

fn section_of(item: Item) -> (Section, usize) {
    match item {
        Item::Trigger(index) => (Section::Triggers, index),
        Item::Step(lane, index) => (Section::Steps(lane), index),
    }
}

enum Origin {
    Saved(String),
    New { name: String, form: Form },
}

struct Draft {
    origin: Origin,
    name: String,
    form: Form,
    item: Option<Item>,
}

impl Draft {
    fn of(saved: &str, library: &Library) -> Option<Self> {
        let saved = library.names().find(|name| same_name(name, saved))?;
        Some(Self {
            origin: Origin::Saved(saved.to_owned()),
            name: saved.to_owned(),
            form: Form::of(&library.get(saved)?),
            item: None,
        })
    }

    fn saved(&self) -> Option<&str> {
        match &self.origin {
            Origin::Saved(saved) => Some(saved),
            Origin::New { .. } => None,
        }
    }

    fn changed(&self, library: &Library) -> bool {
        match &self.origin {
            Origin::Saved(saved) => {
                self.name != *saved || self.form.parse().ok() != library.get(saved)
            }
            Origin::New { name, form } => self.name != *name || self.form != *form,
        }
    }

    fn label(&self) -> SharedString {
        match self.name.trim() {
            "" => "Untitled".into(),
            name => name.to_owned().into(),
        }
    }

    fn unsaved(&self, library: &Library) -> Option<(&'static str, Tone)> {
        match (&self.origin, self.changed(library)) {
            (Origin::New { .. }, _) => Some(("Not saved yet", Tone::Accent)),
            (Origin::Saved(_), true) => Some(("Unsaved changes", Tone::Warning)),
            (Origin::Saved(_), false) => None,
        }
    }

    fn taken<'a>(&'a self, library: &'a Library) -> impl Iterator<Item = &'a str> + 'a {
        library
            .names()
            .filter(move |name| self.saved().is_none_or(|saved| !same_name(saved, name)))
    }

    fn name_rejected(&self, library: &Library) -> bool {
        check_name(&self.name, self.taken(library)).is_err()
    }

    fn name_note(&self, library: &Library) -> Option<(String, Tone)> {
        match check_name(&self.name, self.taken(library)) {
            Err(problem) => Some((problem, Tone::Danger)),
            Ok(name) => self
                .saved()
                .filter(|saved| !same_name(saved, &name))
                .map(|saved| {
                    (
                        format!(
                            "Scripts that use \u{201c}{saved}\u{201d} won't find it after you save."
                        ),
                        Tone::Warning,
                    )
                }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Text {
    Includes,
    Pause,
    Value(usize),
    Name(usize),
}

impl Text {
    fn millis(self) -> bool {
        matches!(self, Text::Pause)
    }
}

struct Texts {
    inputs: Vec<(Text, Entity<Input>)>,
    _subscriptions: Vec<Subscription>,
}

impl Texts {
    fn get(&self, text: Text) -> Option<Entity<Input>> {
        self.inputs
            .iter()
            .find(|(at, _)| *at == text)
            .map(|(_, input)| input.clone())
    }
}

enum Blocked {
    Name,
    Item(Item, String),
    Write(String),
}

enum Leave {
    To(String),
    New,
    Close,
}

struct Editor {
    focus: FocusHandle,
    library: Arc<Library>,
    draft: Option<Draft>,
    previous: Option<String>,
    name: Entity<Input>,
    lanes_scroll: ScrollHandle,
    inspector_scroll: ScrollHandle,
    texts: Option<Texts>,
    problem: Option<String>,
    deleting: bool,
    leaving: Option<Leave>,
    _subscriptions: [Subscription; 2],
}

impl Editor {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let library = combat_profiles::library(cx);
        let draft = library
            .names()
            .next()
            .and_then(|first| Draft::of(first, &library));
        let first = draft
            .as_ref()
            .map(|draft| draft.name.clone())
            .unwrap_or_default();
        let name = cx.new(|cx| {
            let mut input = Input::new("Profile name", cx).max_length(60);
            input.set_text(first, cx);
            input
        });
        let subscriptions = [
            cx.subscribe_in(&name, window, Self::name_event),
            cx.observe_global::<combat_profiles::Store>(|this, cx| {
                this.library = combat_profiles::library(cx);
                cx.notify();
            }),
        ];
        Self {
            focus: cx.focus_handle(),
            library,
            draft,
            previous: None,
            name,
            lanes_scroll: ScrollHandle::new(),
            inspector_scroll: ScrollHandle::new(),
            texts: None,
            problem: None,
            deleting: false,
            leaving: None,
            _subscriptions: subscriptions,
        }
    }

    fn draft(&self) -> Option<&Draft> {
        self.draft.as_ref()
    }

    fn item(&self) -> Option<Item> {
        self.draft()?.item
    }

    fn form(&self) -> &Form {
        self.draft().map_or(&GENERIC_FORM, |draft| &draft.form)
    }

    fn form_mut(&mut self) -> Option<&mut Form> {
        self.draft.as_mut().map(|draft| &mut draft.form)
    }

    fn changed(&self) -> bool {
        self.draft()
            .is_some_and(|draft| draft.changed(&self.library))
    }

    fn confirming(&self) -> bool {
        self.deleting || self.leaving.is_some()
    }

    fn show(&mut self, draft: Option<Draft>, window: &mut Window, cx: &mut Context<Self>) {
        self.draft = draft;
        self.problem = None;
        let name = self
            .draft()
            .map(|draft| draft.name.clone())
            .unwrap_or_default();
        self.name.update(cx, |input, cx| input.set_text(name, cx));
        self.rebuild_texts(window, cx);
        cx.notify();
    }

    fn select_named(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let shown = self
            .draft()
            .and_then(Draft::saved)
            .is_some_and(|saved| same_name(saved, name));
        if !shown && !self.confirming() && self.library.names().any(|saved| same_name(saved, name))
        {
            self.leave(Leave::To(name.to_owned()), window, cx);
        }
    }

    fn leave(&mut self, leave: Leave, window: &mut Window, cx: &mut Context<Self>) {
        if self.changed() {
            self.leaving = Some(leave);
            cx.notify();
        } else {
            self.go(leave, window, cx);
        }
    }

    fn go(&mut self, leave: Leave, window: &mut Window, cx: &mut Context<Self>) {
        match leave {
            Leave::To(name) => self.show(Draft::of(&name, &self.library), window, cx),
            Leave::New => {
                let form = Form::of(&Profile::generic());
                self.start(form.clone(), form, NEW_NAME, window, cx);
            }
            Leave::Close => window.remove_window(),
        }
    }

    fn leave_after_saving(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(leave) = self.leaving.take() else {
            return;
        };
        match self.save_draft(cx) {
            Ok(()) => self.go(leave, window, cx),
            Err(blocked) => self.show_blocked(blocked, window, cx),
        }
    }

    fn select_item(&mut self, item: Option<Item>, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(draft) = &mut self.draft {
            draft.item = item;
        }
        self.rebuild_texts(window, cx);
        cx.notify();
    }

    fn edit_name(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.lanes_scroll.set_offset(gpui::Point::default());
        self.name.update(cx, |input, cx| {
            input.select_all(cx);
            input.focus_handle(cx).focus(window, cx);
        });
    }

    fn name_event(
        &mut self,
        input: &Entity<Input>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::Changed => {
                let text = input.read(cx).text().to_owned();
                if let Some(draft) = &mut self.draft
                    && draft.name != text
                {
                    draft.name = text;
                    self.problem = None;
                    cx.notify();
                }
                let invalid = self
                    .draft()
                    .is_some_and(|draft| draft.name_rejected(&self.library));
                input.update(cx, |input, cx| input.set_invalid(invalid, cx));
            }
            InputEvent::Submitted => window.focus(&self.focus, cx),
        }
    }

    fn rebuild_texts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let texts: Vec<(Text, &'static str, String)> = match self.item() {
            None => Vec::new(),
            Some(Item::Trigger(index)) => match self.form().triggers.get(index) {
                Some(trigger) => vec![(
                    Text::Includes,
                    "When this message appears",
                    trigger.includes.clone(),
                )],
                None => Vec::new(),
            },
            Some(Item::Step(lane, index)) => match self.form().lane(lane).get(index) {
                Some(step) => std::iter::once((
                    Text::Pause,
                    "Pause after casting, in milliseconds",
                    step.pause.clone(),
                ))
                .chain(step.when.iter().enumerate().flat_map(|(rule, condition)| {
                    [
                        (Text::Value(rule), "Value", condition.value.clone()),
                        (Text::Name(rule), "Aura name", condition.name.clone()),
                    ]
                }))
                .collect(),
                None => Vec::new(),
            },
        };
        if texts.is_empty() {
            if let Some(draft) = &mut self.draft {
                draft.item = None;
            }
            self.texts = None;
            return;
        }
        let mut subscriptions = Vec::new();
        let inputs = texts
            .into_iter()
            .map(|(field, label, text)| {
                let input = cx.new(|cx| {
                    let mut input = Input::new(label, cx);
                    if field.millis() {
                        input = input.numeric(MILLIS);
                    }
                    if let Text::Name(_) | Text::Value(_) = field {
                        input = input.small();
                    }
                    if let Text::Name(_) = field {
                        input = input.placeholder("Aura name");
                    }
                    input.set_text(text, cx);
                    input
                });
                subscriptions.push(cx.subscribe_in(
                    &input,
                    window,
                    move |this, input, event, _, cx| {
                        if let InputEvent::Changed = event {
                            let text = input.read(cx).text().to_owned();
                            this.set_text(field, text, cx);
                        }
                    },
                ));
                (field, input)
            })
            .collect();
        self.texts = Some(Texts {
            inputs,
            _subscriptions: subscriptions,
        });
    }

    fn edited_now(&mut self, cx: &mut Context<Self>) {
        self.problem = None;
        cx.notify();
    }

    fn edit_step(&mut self, cx: &mut Context<Self>, change: impl FnOnce(&mut FormStep)) {
        let Some(Item::Step(lane, index)) = self.item() else {
            return;
        };
        if let Some(step) = self
            .form_mut()
            .and_then(|form| form.lane_mut(lane).get_mut(index))
        {
            change(step);
            self.edited_now(cx);
        }
    }

    fn edit_trigger(&mut self, cx: &mut Context<Self>, change: impl FnOnce(&mut FormTrigger)) {
        let Some(Item::Trigger(index)) = self.item() else {
            return;
        };
        if let Some(trigger) = self
            .form_mut()
            .and_then(|form| form.triggers.get_mut(index))
        {
            change(trigger);
            self.edited_now(cx);
        }
    }

    fn edit_rule(&mut self, rule: usize, cx: &mut Context<Self>, change: impl FnOnce(&mut Rule)) {
        self.edit_step(cx, |step| {
            if let Some(rule) = step.when.get_mut(rule) {
                change(rule);
            }
        });
    }

    fn set_skill(&mut self, slot: u8, cx: &mut Context<Self>) {
        let skill = Skill::try_from(slot).expect("listed slots are skills");
        match self.item() {
            Some(Item::Trigger(_)) => self.edit_trigger(cx, |trigger| trigger.skill = skill),
            Some(Item::Step(..)) => self.edit_step(cx, |step| step.skill = skill),
            None => {}
        }
    }

    fn set_text(&mut self, field: Text, text: String, cx: &mut Context<Self>) {
        match field {
            Text::Includes => self.edit_trigger(cx, |trigger| trigger.includes = text),
            Text::Pause => self.edit_step(cx, |step| step.pause = text),
            Text::Value(rule) => self.edit_rule(rule, cx, |rule| rule.value = text),
            Text::Name(rule) => self.edit_rule(rule, cx, |rule| rule.name = text),
        }
    }

    fn add(&mut self, section: Section, slot: u8, window: &mut Window, cx: &mut Context<Self>) {
        let skill = Skill::try_from(slot).expect("listed slots are skills");
        let Some(form) = self.form_mut() else {
            return;
        };
        let index = match section {
            Section::Triggers => {
                form.triggers.push(FormTrigger::new(skill));
                form.triggers.len() - 1
            }
            Section::Steps(lane) => {
                let steps = form.lane_mut(lane);
                steps.push(FormStep::new(skill));
                steps.len() - 1
            }
        };
        self.problem = None;
        self.select_item(Some(section.item(index)), window, cx);
        if section == Section::Triggers
            && let Some(input) = self
                .texts
                .as_ref()
                .and_then(|texts| texts.get(Text::Includes))
        {
            input.read(cx).focus_handle(cx).focus(window, cx);
        }
    }

    fn reorder(
        &mut self,
        section: Section,
        from: usize,
        to: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        fn shift<T>(items: &mut Vec<T>, from: usize, to: usize) -> bool {
            if from >= items.len() || to >= items.len() {
                return false;
            }
            let item = items.remove(from);
            items.insert(to, item);
            true
        }
        let Some(form) = self.form_mut() else {
            return;
        };
        let moved = match section {
            Section::Triggers => shift(&mut form.triggers, from, to),
            Section::Steps(lane) => shift(form.lane_mut(lane), from, to),
        };
        if moved {
            self.select_item(Some(section.item(to)), window, cx);
        }
    }

    fn remove_item(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = self.item() else {
            return;
        };
        if let Some(form) = self.form_mut() {
            match item {
                Item::Trigger(index) => {
                    form.triggers.remove(index);
                }
                Item::Step(lane, index) => {
                    form.lane_mut(lane).remove(index);
                }
            }
        }
        self.problem = None;
        self.select_item(None, window, cx);
    }

    fn duplicate_item(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = self.item() else {
            return;
        };
        let Some(form) = self.form_mut() else {
            return;
        };
        let copy = match item {
            Item::Trigger(index) => {
                let copy = form.triggers[index].clone();
                form.triggers.insert(index + 1, copy);
                Item::Trigger(index + 1)
            }
            Item::Step(lane, index) => {
                let steps = form.lane_mut(lane);
                let copy = steps[index].clone();
                steps.insert(index + 1, copy);
                Item::Step(lane, index + 1)
            }
        };
        self.select_item(Some(copy), window, cx);
    }

    fn add_rule(&mut self, subject: Subject, window: &mut Window, cx: &mut Context<Self>) {
        let mut added = None;
        self.edit_step(cx, |step| {
            step.when.push(Rule::new(subject));
            added = Some(step.when.len() - 1);
        });
        self.rebuild_texts(window, cx);
        let field = added.map(|rule| {
            if subject.aura() {
                Text::Name(rule)
            } else {
                Text::Value(rule)
            }
        });
        if let Some(input) = field.and_then(|field| self.texts.as_ref()?.get(field)) {
            input.read(cx).focus_handle(cx).focus(window, cx);
        }
    }

    fn remove_rule(&mut self, rule: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.edit_step(cx, |step| {
            if rule < step.when.len() {
                step.when.remove(rule);
            }
        });
        self.rebuild_texts(window, cx);
    }

    fn unique_name(&self, base: &str) -> String {
        let taken = |name: &str| {
            same_name(name, GENERIC) || self.library.names().any(|saved| same_name(saved, name))
        };
        if !taken(base) {
            return base.to_owned();
        }
        (2..)
            .map(|number| format!("{base} {number}"))
            .find(|name| !taken(name))
            .expect("some numbered name is free")
    }

    fn start(
        &mut self,
        form: Form,
        unchanged: Form,
        base: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = self.unique_name(base);
        if let Some(saved) = self.draft().and_then(Draft::saved) {
            self.previous = Some(saved.to_owned());
        }
        let draft = Draft {
            origin: Origin::New {
                name: name.clone(),
                form: unchanged,
            },
            name,
            form,
            item: None,
        };
        self.show(Some(draft), window, cx);
        self.edit_name(window, cx);
    }

    fn new_profile(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.leave(Leave::New, window, cx);
    }

    fn duplicate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(draft) = self.draft() else {
            return;
        };
        let Some(saved) = draft.saved().and_then(|saved| self.library.get(saved)) else {
            return;
        };
        let form = draft.form.clone();
        let base = format!("{} copy", draft.name.trim());
        self.start(form, Form::of(&saved), &base, window, cx);
    }

    fn revert(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(draft) = self.draft() else {
            return;
        };
        let back = match draft.saved() {
            Some(saved) => Draft::of(saved, &self.library).map(|saved| Draft {
                item: draft.item,
                ..saved
            }),
            None => self
                .previous
                .as_deref()
                .or_else(|| self.library.names().next())
                .and_then(|name| Draft::of(name, &self.library)),
        };
        self.show(back, window, cx);
    }

    fn delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.deleting = false;
        let Some(saved) = self.draft().and_then(Draft::saved).map(str::to_owned) else {
            return;
        };
        let names: Vec<&str> = self.library.names().collect();
        let next = names
            .iter()
            .position(|name| same_name(name, &saved))
            .and_then(|at| names.get(at + 1).or_else(|| names.get(at.checked_sub(1)?)))
            .map(|name| (*name).to_owned());
        if let Err(error) = combat_profiles::delete(&saved, cx) {
            self.problem = Some(format!("Couldn't delete the profile: {error}"));
            cx.notify();
            return;
        }
        self.library = combat_profiles::library(cx);
        let next = next.and_then(|name| Draft::of(&name, &self.library));
        self.show(next, window, cx);
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Err(blocked) = self.save_draft(cx) {
            self.show_blocked(blocked, window, cx);
        }
        cx.notify();
    }

    fn show_blocked(&mut self, blocked: Blocked, window: &mut Window, cx: &mut Context<Self>) {
        match blocked {
            Blocked::Name => self.edit_name(window, cx),
            Blocked::Item(at, problem) => {
                self.select_item(Some(at), window, cx);
                self.problem = Some(problem);
            }
            Blocked::Write(problem) => self.problem = Some(problem),
        }
        cx.notify();
    }

    fn save_draft(&mut self, cx: &mut Context<Self>) -> Result<(), Blocked> {
        let Some(draft) = &self.draft else {
            return Ok(());
        };
        let name =
            check_name(&draft.name, draft.taken(&self.library)).map_err(|_| Blocked::Name)?;
        let profile = draft
            .form
            .parse()
            .map_err(|problem| Blocked::Item(problem.at, problem.message))?;
        combat_profiles::save(draft.saved(), name.clone(), profile, cx)
            .map_err(|error| Blocked::Write(format!("Couldn't save the profile: {error}")))?;
        self.library = combat_profiles::library(cx);
        if let Some(draft) = &mut self.draft {
            draft.origin = Origin::Saved(name.clone());
            draft.name.clone_from(&name);
        }
        if self.name.read(cx).text() != name {
            self.name.update(cx, |input, cx| input.set_text(name, cx));
        }
        self.problem = None;
        Ok(())
    }

    fn may_close(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.changed() {
            return true;
        }
        self.leaving = Some(Leave::Close);
        cx.notify();
        false
    }

    fn profile_menu(&self, theme: &Theme, cx: &mut Context<Self>) -> Option<DropdownMenu> {
        let draft = self.draft()?;
        let shown = draft.label();
        let saved = draft.saved().map(str::to_owned);
        let rows: Rc<[(SharedString, Option<String>)]> = self
            .library
            .names()
            .map(|name| match &saved {
                Some(saved) if same_name(saved, name) => (name.to_owned().into(), None),
                _ => (name.to_owned().into(), Some(name.to_owned())),
            })
            .chain(saved.is_none().then(|| (shown.clone(), None)))
            .collect();
        let this = cx.weak_entity();
        let menu = DropdownMenu::new("profile-menu")
            .trigger(
                Button::new("profile-menu-trigger", format!("Profile: {shown}"))
                    .variant(Variant::Ghost)
                    .size(Size::Small)
                    .shows_value()
                    .flex_shrink(1.0)
                    .min_w_0()
                    .child(
                        div()
                            .min_w_0()
                            .max_w(PROFILE_WIDTH)
                            .truncate()
                            .text_color(theme.foreground)
                            .child(shown),
                    ),
            )
            .items(move |_, _| {
                let profiles = rows.iter().map(|(label, other)| {
                    let this = this.clone();
                    let other = other.clone();
                    let shown = other.is_none();
                    MenuItem::action(label.clone(), move |window, cx| {
                        if let Some(name) = &other {
                            this.update(cx, |editor, cx| editor.select_named(name, window, cx))
                                .ok();
                        }
                    })
                    .checked(shown)
                });
                let this = this.clone();
                profiles
                    .chain([
                        MenuItem::separator().pinned(),
                        MenuItem::action("New profile", move |window, cx| {
                            this.update(cx, |editor, cx| editor.new_profile(window, cx))
                                .ok();
                        })
                        .shortcut("⌘N")
                        .pinned(),
                    ])
                    .collect()
            });
        Some(menu)
    }

    fn name_row(&self, theme: &Theme) -> impl IntoElement + use<> {
        let layout = theme.layout;
        let note = self
            .draft()
            .and_then(|draft| draft.name_note(&self.library));
        v_flex()
            .gap(theme.space(4.0))
            .child(form::row(layout, "Name", true, self.name.clone()))
            .children(note.map(|(text, tone)| {
                let text = SharedString::from(text);
                row_note(theme, text.clone())
                    .id("profile-name-note")
                    .role(Role::Status)
                    .aria_label(text)
                    .text_color(theme.tone(tone).2)
            }))
    }

    fn unread_alert(cx: &App) -> Option<Alert> {
        let title = match combat_profiles::issues(cx).len() {
            0 => return None,
            1 => "1 saved profile couldn't be read, so it's left out.".to_owned(),
            issues => format!("{issues} saved profiles couldn't be read, so they're left out."),
        };
        Some(Alert::new("unread-profiles", Tone::Warning, title))
    }

    fn section(&self, theme: &Theme, section: Section, cx: &mut Context<Self>) -> AnyElement {
        let layout = theme.layout;
        let (title, subtitle, empty) = section.copy();
        let form = self.form();
        let cards: Rc<[Card]> = match section {
            Section::Triggers => form
                .triggers
                .iter()
                .map(|trigger| Card {
                    slot: u8::from(trigger.skill),
                    summary: trigger.summary().into(),
                    detail: None,
                })
                .collect(),
            Section::Steps(lane) => form
                .lane(lane)
                .iter()
                .map(|step| {
                    let cooldown = match step.on_cooldown {
                        Some(OnCooldown::Wait) => Some("waits".to_owned()),
                        Some(OnCooldown::Skip) => Some("skips".to_owned()),
                        None => None,
                    };
                    let pause = Some(step.pause.trim())
                        .filter(|pause| !pause.is_empty())
                        .map(|pause| format!("pause {pause} ms"));
                    let detail: Vec<String> = cooldown.into_iter().chain(pause).collect();
                    Card {
                        slot: u8::from(step.skill),
                        summary: step.summary().into(),
                        detail: (!detail.is_empty()).then(|| detail.join(" · ").into()),
                    }
                })
                .collect(),
        };
        let selected = self
            .item()
            .map(section_of)
            .filter(|(at, _)| *at == section)
            .map(|(_, index)| index);
        let control = if cards.is_empty() {
            empty_box(theme, empty).into_any_element()
        } else {
            let count = cards.len();
            let list = List::new(section.id(), title, count, move |index| {
                let card = &cards[index];
                let item =
                    ListItem::new(card.summary.clone()).start_slot(Kbd::new(card.slot.to_string()));
                match &card.detail {
                    Some(detail) => item.end_slot(detail.clone()),
                    None => item,
                }
            })
            .selected(selected);
            let this = cx.weak_entity();
            let mover = this.clone();
            list.on_select(move |index, window, cx| {
                this.update(cx, |editor, cx| {
                    editor.select_item(Some(section.item(index)), window, cx)
                })
                .ok();
            })
            .on_reorder(move |from, to, window, cx| {
                mover
                    .update(cx, |editor, cx| {
                        editor.reorder(section, from, to, window, cx)
                    })
                    .ok();
            })
            .into_any_element()
        };
        let add = self.add_menu(section, cx);
        v_flex()
            .gap(layout.space(2.0))
            .child(form::section(title, subtitle, None, add, theme))
            .when(section == Section::Steps(Lane::Rotation), |lane| {
                lane.child(self.on_cooldown_row(cx))
            })
            .child(control)
            .into_any_element()
    }

    fn add_menu(&self, section: Section, cx: &mut Context<Self>) -> DropdownMenu {
        let this = cx.weak_entity();
        DropdownMenu::new(SharedString::from(format!("add-{}", section.id())))
            .align(Align::End)
            .width(px(200.0))
            .trigger(
                Button::new(
                    SharedString::from(format!("add-{}-trigger", section.id())),
                    "Add",
                )
                .size(Size::Small)
                .end_slot(Icon::ChevronDown),
            )
            .items(move |_, _| {
                SLOTS
                    .into_iter()
                    .zip(SKILLS)
                    .map(|(slot, label)| {
                        let this = this.clone();
                        MenuItem::action(label, move |window, cx| {
                            this.update(cx, |editor, cx| editor.add(section, slot, window, cx))
                                .ok();
                        })
                        .start_slot(Kbd::new(slot.to_string()))
                        .key(slot.to_string())
                    })
                    .collect()
            })
    }

    fn on_cooldown_row(&self, cx: &mut Context<Self>) -> Div {
        let layout = cx.theme().layout;
        let this = cx.weak_entity();
        form::row(
            layout,
            "On cooldown",
            true,
            ToggleGroup::new(
                "profile-on-cooldown",
                "When a skill is cooling down",
                ["Skip it", "Wait for it"],
                usize::from(self.form().on_cooldown == OnCooldown::Wait),
            )
            .on_select(move |choice, _, cx| {
                let _ = this.update(cx, |this, cx| {
                    if let Some(form) = this.form_mut() {
                        form.on_cooldown = if choice == 0 {
                            OnCooldown::Skip
                        } else {
                            OnCooldown::Wait
                        };
                        this.edited_now(cx);
                    }
                });
            }),
        )
    }

    fn footer(&self, theme: &Theme, cx: &mut Context<Self>) -> Option<Div> {
        let typography = theme.typography;
        let draft = self.draft()?;
        let saved = draft.saved().is_some();
        let unsaved = draft.unsaved(&self.library);
        let status = match (&self.problem, unsaved) {
            (Some(problem), _) => Some((SharedString::from(problem.clone()), theme.destructive)),
            (None, Some((text, tone))) => Some((text.into(), theme.tone(tone).2)),
            (None, None) => None,
        };
        let revert = unsaved.is_some().then(|| {
            Button::new("revert-profile", if saved { "Revert" } else { "Discard" })
                .variant(Variant::Ghost)
                .on_click(cx.listener(|this, _, window, cx| this.revert(window, cx)))
        });
        let footer = theme
            .footer()
            .child(
                h_flex()
                    .flex_none()
                    .gap(theme.space(2.0))
                    .debug_selector(|| "profile-actions".into())
                    .child(
                        Button::new("duplicate-profile", "Duplicate")
                            .variant(Variant::Ghost)
                            .disabled(!saved)
                            .on_click(
                                cx.listener(|this, _, window, cx| this.duplicate(window, cx)),
                            ),
                    )
                    .child(
                        Button::new("delete-saved-profile", "Delete…")
                            .variant(Variant::Ghost)
                            .disabled(!saved)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.deleting = true;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .justify_end()
                    .children(status.map(|(text, color)| {
                        div()
                            .id("profile-status")
                            .role(Role::Status)
                            .aria_label(text.clone())
                            .min_w_0()
                            .truncate()
                            .text_body(typography)
                            .text_color(color)
                            .child(text)
                    })),
            )
            .children(revert)
            .child(
                Button::new("save-profile", "Save")
                    .variant(Variant::Primary)
                    .end_slot(Kbd::hint("⌘S"))
                    .disabled(unsaved.is_none())
                    .on_click(cx.listener(|this, _, window, cx| this.save(window, cx))),
            );
        Some(footer)
    }

    fn inspector(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let layout = theme.layout;
        let typography = theme.typography;
        let pane = v_flex()
            .id("inspector")
            .size_full()
            .gap(theme.space(4.0))
            .p(PADDING)
            .pt(layout.space(3.0))
            .overflow_y_scroll()
            .track_scroll(&self.inspector_scroll);
        let (Some(item), Some(texts)) = (self.item(), &self.texts) else {
            return pane.child(form::note(
                "Select a trigger or a step to edit it.",
                true,
                theme,
            ));
        };
        let skill = match item {
            Item::Trigger(index) => self.form().triggers.get(index).map(|trigger| trigger.skill),
            Item::Step(lane, index) => self.form().lane(lane).get(index).map(|step| step.skill),
        };
        let Some(skill) = skill else {
            return pane;
        };
        let slot = u8::from(skill);
        let this = cx.weak_entity();
        let skill_this = this.clone();
        let skills = ToggleGroup::new(
            "item-skill",
            "Skill",
            SLOTS.map(|key| SharedString::from(key.to_string())),
            usize::from(slot),
        )
        .full_width()
        .on_select(move |index, _, cx| {
            let _ = skill_this.update(cx, |this, cx| this.set_skill(SLOTS[index], cx));
        });
        let pane = pane.child(
            h_flex()
                .h(layout.control)
                .justify_between()
                .gap(layout.space(2.0))
                .child(heading(item_title(item)))
                .child(
                    h_flex()
                        .gap(layout.space(0.5))
                        .child(
                            Button::new("duplicate-item", "Duplicate")
                                .variant(Variant::Ghost)
                                .size(Size::Small)
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.duplicate_item(window, cx)
                                })),
                        )
                        .child(
                            Button::new("remove-item", "Remove")
                                .variant(Variant::Ghost)
                                .size(Size::Small)
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.remove_item(window, cx)),
                                ),
                        ),
                ),
        );
        match item {
            Item::Trigger(_) => pane
                .child(form::row(
                    layout,
                    "Message",
                    true,
                    div().children(texts.get(Text::Includes)),
                ))
                .child(row_note(
                    theme,
                    "Matched anywhere in combat and server messages, ignoring case.",
                ))
                .child(form::row(layout, "Cast", true, skills))
                .child(row_note(
                    theme,
                    "Casts once, as soon as the skill is ready. Gives up if that takes more than 3 seconds.",
                )),
            Item::Step(lane, index) => {
                let Some(step) = self.form().lane(lane).get(index) else {
                    return pane;
                };
                let mut pane = pane.child(form::row(layout, "Skill", true, skills));
                if lane == Lane::Rotation {
                    let chosen = match step.on_cooldown {
                        None => 0,
                        Some(OnCooldown::Skip) => 1,
                        Some(OnCooldown::Wait) => 2,
                    };
                    let profile_default = match self.form().on_cooldown {
                        OnCooldown::Skip => "Default skips it.",
                        OnCooldown::Wait => "Default waits for it.",
                    };
                    let cooldown_this = this.clone();
                    pane = pane
                        .child(form::row(
                    layout,
                            "On cooldown",
                            true,
                            ToggleGroup::new(
                                "step-on-cooldown",
                                "When it's cooling down",
                                ["Default", "Skip", "Wait"],
                                chosen,
                            )
                            .full_width()
                            .on_select(move |choice, _, cx| {
                                let _ = cooldown_this.update(cx, |this, cx| {
                                    this.edit_step(cx, |step| {
                                        step.on_cooldown = match choice {
                                            1 => Some(OnCooldown::Skip),
                                            2 => Some(OnCooldown::Wait),
                                            _ => None,
                                        }
                                    })
                                });
                            }),
                        ))
                        .child(row_note(theme, profile_default));
                }
                let conditions = step
                    .when
                    .iter()
                    .enumerate()
                    .map(|(rule, condition)| condition_editor(rule, condition, texts, &this, cx))
                    .collect::<Vec<_>>();
                let hint = step
                    .when
                    .iter()
                    .any(|rule| rule.subject.aura())
                    .then_some("A missing aura counts as 0.");
                let adder = this.clone();
                pane.child(form::row(
                    layout,
                    "Pause after",
                    true,
                    millis_input(theme, texts, Text::Pause),
                ))
                .child(
                    v_flex()
                        .gap(layout.space(1.5))
                        .child(
                            h_flex()
                                .h(layout.control)
                                .justify_between()
                                .child(heading("Conditions".into()))
                                .child(
                                    DropdownMenu::new("add-rule")
                                        .align(Align::End)
                                        .width(SUBJECT_MENU_WIDTH)
                                        .trigger(
                                            Button::new("add-rule-trigger", "Add")
                                                .size(Size::Small)
                                                .end_slot(Icon::ChevronDown),
                                        )
                                        .items(move |_, _| {
                                            Subject::ALL
                                                .into_iter()
                                                .map(|subject| {
                                                    let adder = adder.clone();
                                                    MenuItem::action(
                                                        subject.label(),
                                                        move |window, cx| {
                                                            adder
                                                                .update(cx, |this, cx| {
                                                                    this.add_rule(
                                                                        subject, window, cx,
                                                                    )
                                                                })
                                                                .ok();
                                                        },
                                                    )
                                                })
                                                .collect()
                                        }),
                                ),
                        )
                        .child(if conditions.is_empty() {
                            empty_box(theme, "None, so it casts as soon as it's ready.")
                        } else {
                            v_flex().gap(layout.space(3.0)).pt(layout.space(0.5)).children(
                                conditions.into_iter().enumerate().map(|(index, row)| {
                                    h_flex()
                                        .items_start()
                                        .gap(CONDITION_GAP)
                                        .child(
                                            div()
                                                .flex_none()
                                                .flex()
                                                .items_center()
                                                .h(layout.control_sm)
                                                .w(CONJUNCTION_WIDTH)
                                                .text_caption(typography)
                                                .text_color(theme.muted_foreground)
                                                .child(if index == 0 { "If" } else { "and" }),
                                        )
                                        .child(div().flex_1().min_w_0().child(row))
                                }),
                            )
                        })
                        .children(hint.map(|hint| form::note(hint, true, theme).mt(layout.space(1.0)))),
                )
            }
        }
    }
}

fn empty_box(theme: &Theme, text: &'static str) -> Div {
    let layout = theme.layout;
    let typography = theme.typography;
    div()
        .px(layout.space(3.0))
        .py(layout.space(2.0))
        .rounded(theme.radius.panel)
        .border_1()
        .border_color(theme.border)
        .text_caption(typography)
        .text_color(theme.muted_foreground)
        .child(text)
}

fn item_title(item: Item) -> SharedString {
    match item {
        Item::Trigger(index) => format!("Trigger {}", index + 1),
        Item::Step(Lane::Priority, index) => format!("Priority step {}", index + 1),
        Item::Step(Lane::Rotation, index) => format!("Rotation step {}", index + 1),
    }
    .into()
}

fn heading(text: SharedString) -> Div {
    div()
        .min_w_0()
        .truncate()
        .font_weight(FontWeight::MEDIUM)
        .child(text)
}

fn row_note(theme: &Theme, text: impl Into<SharedString>) -> Div {
    let layout = theme.layout;
    form::note(text, true, theme)
        .ml(form::LABEL_WIDTH + layout.space(2.0))
        .mt(layout.space(-1.5))
}

fn millis_input(theme: &Theme, texts: &Texts, text: Text) -> impl IntoElement + use<> {
    h_flex()
        .gap(theme.space(2.0))
        .child(div().w(MILLIS_WIDTH).children(texts.get(text)))
        .child(div().text_color(theme.muted_foreground).child("ms"))
}

struct Card {
    slot: u8,
    summary: SharedString,
    detail: Option<SharedString>,
}

fn condition_editor(
    rule: usize,
    condition: &Rule,
    texts: &Texts,
    this: &WeakEntity<Editor>,
    cx: &mut Context<Editor>,
) -> impl IntoElement + use<> {
    let theme = cx.theme().clone();
    let layout = theme.layout;
    let edit = |change: fn(&mut Rule)| {
        let this = this.clone();
        move |_: &gpui::ClickEvent, _: &mut Window, cx: &mut App| {
            let _ = this.update(cx, |this, cx| this.edit_rule(rule, cx, change));
        }
    };
    let subject = {
        let this = this.clone();
        DropdownMenu::new(("rule-subject", rule))
            .width(SUBJECT_MENU_WIDTH)
            .trigger(
                Button::new(("rule-subject-trigger", rule), condition.subject.label())
                    .size(Size::Small)
                    .w(SUBJECT_WIDTH)
                    .child(
                        h_flex()
                            .flex_1()
                            .justify_between()
                            .child(condition.subject.label())
                            .child(mira_ui::primitives::icon(
                                Icon::ChevronDown,
                                theme.muted_foreground,
                            )),
                    ),
            )
            .items({
                let chosen = condition.subject;
                move |_, _| {
                    Subject::ALL
                        .into_iter()
                        .map(|subject| {
                            let this = this.clone();
                            MenuItem::radio(subject.label(), subject == chosen, move |_, cx| {
                                let _ = this.update(cx, |this, cx| {
                                    this.edit_rule(rule, cx, |rule| rule.subject = subject)
                                });
                            })
                        })
                        .collect()
                }
            })
    };
    let aura = condition.subject.aura();
    let bound = Button::new(
        ("rule-bound", rule),
        if condition.at_most {
            "At most"
        } else {
            "At least"
        },
    )
    .size(Size::Small)
    .w(layout.control_sm)
    .px_0()
    .child(if condition.at_most { "≤" } else { "≥" })
    .on_click(edit(|rule| rule.at_most = !rule.at_most));
    let unit = Button::new(
        ("rule-unit", rule),
        match (aura, condition.measure, condition.percent) {
            (true, Measure::Stacks, _) => "stacks",
            (true, Measure::Value, _) => "value",
            (false, _, true) => "%",
            (false, _, false) => "pts",
        },
    )
    .size(Size::Small)
    .w(UNIT_WIDTH)
    .on_click(edit(if aura {
        |rule: &mut Rule| {
            rule.measure = match rule.measure {
                Measure::Stacks => Measure::Value,
                Measure::Value => Measure::Stacks,
            }
        }
    } else {
        |rule: &mut Rule| rule.percent = !rule.percent
    }));
    let remove = Button::icon(("remove-rule", rule), Icon::X, "Remove condition")
        .variant(Variant::Ghost)
        .size(Size::Small)
        .hide_tooltip()
        .on_click(cx.listener(move |this, _, window, cx| this.remove_rule(rule, window, cx)));
    let measure = h_flex()
        .gap(CONDITION_GAP)
        .child(bound)
        .child(div().w(VALUE_WIDTH).children(texts.get(Text::Value(rule))))
        .child(unit);
    if !aura {
        return h_flex()
            .gap(CONDITION_GAP)
            .child(subject)
            .child(measure)
            .child(div().flex_1())
            .child(remove)
            .into_any_element();
    }
    v_flex()
        .gap(CONDITION_GAP)
        .child(
            h_flex()
                .gap(CONDITION_GAP)
                .child(subject)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .children(texts.get(Text::Name(rule))),
                )
                .child(remove),
        )
        .child(measure)
        .into_any_element()
}

impl Render for Editor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let label = self.draft().map(Draft::label).unwrap_or_default();
        let undo = if self.draft().is_some_and(|draft| draft.saved().is_some()) {
            "Revert"
        } else {
            "Discard"
        };
        let deleting_name = self
            .draft()
            .and_then(Draft::saved)
            .unwrap_or_default()
            .to_owned();
        let body = if self.draft().is_some() {
            let sections = Section::ALL.map(|section| self.section(&theme, section, cx));
            let lanes = div()
                .relative()
                .flex_1()
                .min_w_0()
                .h_full()
                .child(
                    div()
                        .id("profile")
                        .flex()
                        .flex_col()
                        .size_full()
                        .gap(theme.space(4.0))
                        .p(PADDING)
                        .overflow_y_scroll()
                        .track_scroll(&self.lanes_scroll)
                        .children(Self::unread_alert(cx))
                        .child(self.name_row(&theme))
                        .children(sections),
                )
                .child(Scrollbar::new(
                    "profile-scrollbar",
                    self.lanes_scroll.clone(),
                ));
            let inspector = theme
                .inspector_pane()
                .relative()
                .h_full()
                .child(self.inspector(&theme, cx))
                .child(Scrollbar::new(
                    "inspector-scrollbar",
                    self.inspector_scroll.clone(),
                ));
            div()
                .flex()
                .flex_1()
                .min_h_0()
                .child(lanes)
                .child(inspector)
        } else {
            v_flex()
                .flex_1()
                .min_h_0()
                .gap(theme.space(4.0))
                .p(PADDING)
                .children(Self::unread_alert(cx))
                .child(
                    v_flex().flex_1().justify_center().child(
                        Empty::new(
                            "No combat profiles",
                            "Make one to choose the skills auto attack and scripts cast.",
                        )
                        .action(
                            Button::new("first-profile", "New profile")
                                .end_slot(Kbd::hint("⌘N"))
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.new_profile(window, cx)),
                                ),
                        ),
                    ),
                )
        };
        let profiles = self.profile_menu(&theme, cx);
        let footer = self.footer(&theme, cx);
        let bar = window_bar(window, cx)
            .child(bar_title("Combat profiles", cx))
            .when_some(profiles, |bar, profiles| {
                bar.child(bar_separator(cx)).child(profiles)
            })
            .child(drag_area().flex_1());
        div()
            .flex()
            .flex_col()
            .size_full()
            .window_surface(&theme)
            .key_context(format!("{FOCUS_SCOPE} {CONTEXT}").as_str())
            .id("combat-profiles")
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &SaveProfile, window, cx| {
                if !this.confirming() {
                    this.save(window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &NewProfile, window, cx| {
                if !this.confirming() {
                    this.new_profile(window, cx);
                }
            }))
            .on_action(cx.listener(|this, _: &CloseSession, window, cx| {
                if this.may_close(cx) {
                    window.remove_window();
                }
            }))
            .child(mira_ui::nav::pointer_probe())
            .child(crate::quit::Overlay)
            .child(bar)
            .child(body)
            .children(footer)
            .child(
                Dialog::new(
                    "delete-profile",
                    self.deleting,
                    format!("Delete \u{201c}{deleting_name}\u{201d}?"),
                )
                .alert()
                .description(
                    "Tabs whose auto attack uses it switch to Generic. Scripts that name it will stop with an error.",
                )
                .action(Button::new("keep-profile", "Keep").on_click(cx.listener(
                    |this, _, _, cx| {
                        this.deleting = false;
                        cx.notify();
                    },
                )))
                .action(
                    Button::new("confirm-delete", "Delete")
                        .variant(Variant::Danger)
                        .on_click(cx.listener(|this, _, window, cx| this.delete(window, cx))),
                )
                .on_close({
                    let this = cx.weak_entity();
                    move |_, cx| {
                        let _ = this.update(cx, |this, cx| {
                            this.deleting = false;
                            cx.notify();
                        });
                    }
                }),
            )
            .child(
                Dialog::new(
                    "unsaved-profile",
                    self.leaving.is_some(),
                    format!("Save changes to \u{201c}{label}\u{201d}?"),
                )
                .alert()
                .description(format!(
                    "To leave without saving, cancel and choose {undo}."
                ))
                .action(Button::new("keep-editing", "Cancel").on_click(cx.listener(
                    |this, _, _, cx| {
                        this.leaving = None;
                        cx.notify();
                    },
                )))
                .default_action(
                    Button::new("save-and-leave", "Save")
                        .variant(Variant::Primary)
                        .on_click(
                            cx.listener(|this, _, window, cx| this.leave_after_saving(window, cx)),
                        ),
                )
                .on_close({
                    let this = cx.weak_entity();
                    move |_, cx| {
                        let _ = this.update(cx, |this, cx| {
                            this.leaving = None;
                            cx.notify();
                        });
                    }
                }),
            )
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn library_file(name: &str) -> std::path::PathBuf {
        let mut library = Library::default();
        let profile = |value| serde_json::from_value::<Profile>(value).unwrap();
        library.put(
            None,
            "Ultra speaker".into(),
            profile(json!({
                "onCooldown": "wait",
                "triggers": [{ "includes": "prepares a massive attack", "skill": 3 }],
                "priority": [
                    { "skill": 5, "when": [{ "type": "hp", "of": "me", "atMost": "30%" }] }
                ],
                "steps": [
                    { "skill": 1, "onCooldown": "skip" },
                    { "skill": 2, "pauseAfter": 400 },
                    { "skill": 4, "when": [
                        { "type": "aura", "on": "target", "name": "Burn", "atMost": 0 },
                        { "type": "mp", "atLeast": "40%" }
                    ] }
                ]
            })),
        );
        library.put(
            None,
            "Farming".into(),
            profile(json!({ "onCooldown": "skip", "steps": [{ "skill": 1 }, { "skill": 2 }, { "skill": 3 }] })),
        );
        library.put(
            None,
            "Daily boss".into(),
            profile(json!({ "onCooldown": "skip", "steps": [{ "skill": 2 }, { "skill": 4 }] })),
        );
        let path =
            std::env::temp_dir().join(format!("mira-profiles-{name}-{}.json", std::process::id()));
        std::fs::write(&path, library.to_json()).unwrap();
        path
    }

    struct Host(Entity<Editor>);

    impl Render for Host {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .font_family(cx.theme().font_family.clone())
                .child(self.0.clone())
        }
    }

    fn snapshot(
        name: &str,
        prepare: impl Fn(&mut Editor, &mut Window, &mut Context<Editor>) + 'static,
    ) {
        snapshot_at(name, size(px(WIDTH), px(HEIGHT)), prepare, &[]);
    }

    fn snapshot_at(
        name: &str,
        window_size: gpui::Size<Pixels>,
        prepare: impl Fn(&mut Editor, &mut Window, &mut Context<Editor>) + 'static,
        keys: &'static [&'static str],
    ) {
        let path = library_file(name);
        for written in mira_ui::snapshot_after(name, window_size, keys, move |window, cx| {
            combat_profiles::init(path.clone(), cx);
            let editor = cx.new(|cx| Editor::new(window, cx));
            editor.update(cx, |editor, cx| prepare(editor, window, cx));
            let root = editor.read(cx).focus.clone();
            window.focus(&root, cx);
            cx.new(|_| Host(editor))
        }) {
            assert!(written.exists(), "{} was not written", written.display());
        }
    }

    #[test]
    fn snapshot_editor() {
        snapshot("profiles-step", |editor, window, cx| {
            editor.select_named("Ultra speaker", window, cx);
            editor.form_mut().unwrap().steps[1].pause = "500".into();
            editor.select_item(Some(Item::Step(Lane::Rotation, 2)), window, cx);
        });
        snapshot("profiles-trigger", |editor, window, cx| {
            editor.select_named("Ultra speaker", window, cx);
            editor
                .name
                .update(cx, |input, cx| input.set_text("Daily boss", cx));
            editor.select_item(Some(Item::Trigger(0)), window, cx);
        });
        snapshot_at(
            "profiles-step-min",
            size(px(MIN_WIDTH), px(MIN_HEIGHT)),
            |editor, window, cx| {
                editor.select_named("Ultra speaker", window, cx);
                editor
                    .name
                    .update(cx, |input, cx| input.set_text("Ultra speaker solo", cx));
                editor.select_item(Some(Item::Step(Lane::Rotation, 2)), window, cx);
            },
            &[],
        );
        snapshot("profiles-empty", |editor, window, cx| {
            editor.show(None, window, cx);
        });
    }

    #[test]
    fn snapshot_menu() {
        let full = size(px(WIDTH), px(HEIGHT));
        snapshot_at(
            "profiles-menu",
            full,
            |editor, window, cx| editor.select_named("Ultra speaker", window, cx),
            &["tab", "down"],
        );
        snapshot_at(
            "profiles-menu-new",
            full,
            |editor, window, cx| editor.new_profile(window, cx),
            &["tab", "down"],
        );
    }

    #[test]
    fn snapshot_unsaved() {
        snapshot("profiles-unsaved", |editor, window, cx| {
            editor.select_named("Ultra speaker", window, cx);
            editor.form_mut().unwrap().steps[1].pause = "500".into();
            editor.select_named("Farming", window, cx);
            window.defer(cx, |window, cx| {
                window.dispatch_keystroke(gpui::Keystroke::parse("f19").unwrap(), cx);
            });
        });
    }

    fn open_editor<'a>(
        name: &str,
        cx: &'a mut gpui::TestAppContext,
    ) -> (Entity<Editor>, &'a mut gpui::VisualTestContext) {
        let path = library_file(name);
        let characters = path.with_extension("characters");
        cx.update(|cx| {
            mira_ui::init(cx);
            bind_keys(cx);
            combat_profiles::init(path, cx);
            crate::characters::init(characters, cx);
        });
        let (editor, cx) = cx.add_window_view(|window, cx| {
            window.activate_window();
            let editor = Editor::new(window, cx);
            window.focus(&editor.focus, cx);
            editor
        });
        cx.run_until_parked();
        (editor, cx)
    }

    fn press(cx: &mut gpui::VisualTestContext, keys: &'static str) {
        for key in keys.split(' ') {
            cx.simulate_keystrokes(key);
            cx.simulate_event(gpui::KeyUpEvent {
                keystroke: gpui::Keystroke::parse(key).unwrap(),
            });
            cx.run_until_parked();
        }
    }

    fn type_text(cx: &mut gpui::VisualTestContext, text: &str) {
        cx.simulate_input(text);
        cx.run_until_parked();
    }

    fn click(cx: &mut gpui::VisualTestContext, at: gpui::Point<gpui::Pixels>) {
        cx.simulate_click(at, gpui::Modifiers::none());
        cx.run_until_parked();
    }

    fn show(editor: &Entity<Editor>, name: &str, cx: &mut gpui::VisualTestContext) {
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| editor.select_named(name, window, cx))
        });
        cx.run_until_parked();
    }

    fn shown(editor: &Entity<Editor>, cx: &mut gpui::VisualTestContext) -> Option<String> {
        editor.read_with(cx, |editor, _| {
            editor.draft().map(|draft| draft.name.clone())
        })
    }

    fn saved(cx: &mut gpui::VisualTestContext) -> Vec<String> {
        cx.update(|_, cx| {
            combat_profiles::library(cx)
                .names()
                .map(str::to_owned)
                .collect()
        })
    }

    fn steps(name: &str, cx: &mut gpui::VisualTestContext) -> usize {
        cx.update(|_, cx| combat_profiles::library(cx).get(name).unwrap().steps.len())
    }

    #[gpui::test]
    fn the_title_bar_menu_switches_profiles_and_starts_new_ones(cx: &mut gpui::TestAppContext) {
        let (editor, cx) = open_editor("menu", cx);
        assert_eq!(shown(&editor, cx).as_deref(), Some("Daily boss"));
        press(cx, "tab down u enter");
        assert_eq!(shown(&editor, cx).as_deref(), Some("Ultra speaker"));
        press(cx, "down n enter");
        assert_eq!(shown(&editor, cx).as_deref(), Some(NEW_NAME));
        type_text(cx, "Boss rush");
        assert_eq!(shown(&editor, cx).as_deref(), Some("Boss rush"));
    }

    #[gpui::test]
    fn duplicate_copies_the_shown_profile_under_a_new_name(cx: &mut gpui::TestAppContext) {
        let (editor, cx) = open_editor("duplicate", cx);
        show(&editor, "Farming", cx);
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.add(Section::Steps(Lane::Rotation), 4, window, cx)
            })
        });
        cx.run_until_parked();
        let actions = cx.debug_bounds("profile-actions").expect("the footer");
        click(cx, actions.origin + gpui::point(px(20.0), px(10.0)));
        assert_eq!(shown(&editor, cx).as_deref(), Some("Farming copy"));
        type_text(cx, "Grinding");
        press(cx, "enter cmd-s");
        assert_eq!(
            saved(cx),
            ["Daily boss", "Farming", "Grinding", "Ultra speaker"]
        );
        assert_eq!((steps("Farming", cx), steps("Grinding", cx)), (3, 4));
    }

    #[gpui::test]
    fn leaving_unsaved_changes_asks_whether_to_save_them(cx: &mut gpui::TestAppContext) {
        let (editor, cx) = open_editor("leave", cx);
        let add_step = |cx: &mut gpui::VisualTestContext| {
            cx.update(|window, cx| {
                editor.update(cx, |editor, cx| {
                    editor.add(Section::Steps(Lane::Rotation), 4, window, cx)
                })
            });
            cx.run_until_parked();
        };
        show(&editor, "Farming", cx);
        add_step(cx);
        let asking = |cx: &mut gpui::VisualTestContext| {
            editor.read_with(cx, |editor, _| editor.leaving.is_some())
        };
        show(&editor, "Ultra speaker", cx);
        assert!(asking(cx));
        press(cx, "escape");
        assert!(!asking(cx));
        show(&editor, "Ultra speaker", cx);
        press(cx, "shift-tab enter");
        assert!(!asking(cx));
        assert_eq!(shown(&editor, cx).as_deref(), Some("Farming"));
        assert!(editor.read_with(cx, |editor, _| editor.changed()));
        show(&editor, "Ultra speaker", cx);
        press(cx, "cmd-s");
        assert!(asking(cx));
        assert_eq!(steps("Farming", cx), 3);
        press(cx, "enter");
        assert_eq!(shown(&editor, cx).as_deref(), Some("Ultra speaker"));
        assert_eq!(steps("Farming", cx), 4);

        press(cx, "cmd-n");
        show(&editor, "Farming", cx);
        assert_eq!(shown(&editor, cx).as_deref(), Some("Farming"));

        editor.update(cx, |editor, cx| {
            editor.deleting = true;
            cx.notify();
        });
        cx.run_until_parked();
        press(cx, "cmd-n");
        assert_eq!(shown(&editor, cx).as_deref(), Some("Farming"));
    }

    #[gpui::test]
    fn steps_take_arrow_keys(cx: &mut gpui::TestAppContext) {
        let (editor, cx) = open_editor("steps", cx);
        let item =
            |cx: &mut gpui::VisualTestContext| editor.read_with(cx, |editor, _| editor.item());
        show(&editor, "Ultra speaker", cx);
        let third = cx
            .debug_bounds("row-2")
            .expect("the rotation lists three steps");
        click(cx, third.center());
        assert_eq!(item(cx), Some(Item::Step(Lane::Rotation, 2)));
        press(cx, "up");
        assert_eq!(item(cx), Some(Item::Step(Lane::Rotation, 1)));
    }

    #[gpui::test]
    fn a_new_profiles_name_is_replaced_by_typing_however_it_was_started(
        cx: &mut gpui::TestAppContext,
    ) {
        let (editor, cx) = open_editor("new", cx);
        cx.update(|window, cx| editor.update(cx, |editor, cx| editor.new_profile(window, cx)));
        cx.run_until_parked();
        type_text(cx, "Boss rush");
        assert_eq!(shown(&editor, cx).as_deref(), Some("Boss rush"));
        press(cx, "cmd-s cmd-n");
        assert_eq!(shown(&editor, cx).as_deref(), Some(NEW_NAME));
        type_text(cx, "Void solo");
        press(cx, "cmd-s");
        assert_eq!(
            saved(cx),
            [
                "Boss rush",
                "Daily boss",
                "Farming",
                "Ultra speaker",
                "Void solo"
            ]
        );
    }

    #[gpui::test]
    fn saving_under_a_taken_name_saves_nothing_and_puts_the_caret_in_the_name(
        cx: &mut gpui::TestAppContext,
    ) {
        let (editor, cx) = open_editor("taken", cx);
        show(&editor, "Farming", cx);
        cx.update(|_, cx| {
            editor.update(cx, |editor, cx| {
                editor
                    .name
                    .update(cx, |input, cx| input.set_text("daily boss", cx));
            })
        });
        press(cx, "cmd-s");
        assert_eq!(saved(cx), ["Daily boss", "Farming", "Ultra speaker"]);
        type_text(cx, "Grinding");
        press(cx, "cmd-s");
        assert_eq!(saved(cx), ["Daily boss", "Grinding", "Ultra speaker"]);
    }

    #[gpui::test]
    fn discarding_a_new_profile_clears_its_save_problem_and_returns_to_the_last_one(
        cx: &mut gpui::TestAppContext,
    ) {
        let (editor, cx) = open_editor("discard", cx);
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.select_named("Farming", window, cx);
                editor.new_profile(window, cx);
                editor.add(Section::Triggers, 3, window, cx);
                editor.save(window, cx);
                assert!(editor.problem.is_some());
                editor.revert(window, cx);
                assert_eq!(
                    editor.draft().map(|draft| draft.name.as_str()),
                    Some("Farming")
                );
                assert_eq!(editor.problem, None);
                assert_eq!(editor.item(), None);
            })
        });
    }

    #[gpui::test]
    fn revert_restores_the_saved_profile(cx: &mut gpui::TestAppContext) {
        let (editor, cx) = open_editor("revert", cx);
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.select_named("Farming", window, cx);
                editor.add(Section::Steps(Lane::Rotation), 4, window, cx);
                editor
                    .name
                    .update(cx, |input, cx| input.set_text("Grinding", cx));
                assert!(editor.changed());
                editor.revert(window, cx);
                assert_eq!(editor.draft().unwrap().name, "Farming");
                assert!(!editor.changed());
            })
        });
    }

    #[gpui::test]
    fn millisecond_fields_take_digits_and_step_by_100(cx: &mut gpui::TestAppContext) {
        let (editor, cx) = open_editor("millis", cx);
        let pause = cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.select_named("Ultra speaker", window, cx);
                editor.select_item(Some(Item::Step(Lane::Rotation, 1)), window, cx);
                editor.texts.as_ref().unwrap().get(Text::Pause).unwrap()
            })
        });
        cx.update(|window, cx| window.focus(&pause.focus_handle(cx), cx));
        press(cx, "up");
        cx.simulate_input("x");
        cx.run_until_parked();
        let saved = editor.read_with(cx, |editor, _| editor.form().steps[1].pause.clone());
        assert_eq!(saved, "500");
    }

    #[test]
    fn a_saved_draft_with_equivalent_text_is_not_changed() {
        let mut library = Library::default();
        library.put(None, "Generic".into(), Profile::generic());
        let mut draft = Draft::of("Generic", &library).unwrap();
        draft.form.steps[0].pause = "0".into();
        assert!(!draft.changed(&library));
        draft.form.steps[0].pause = "100".into();
        assert!(draft.changed(&library));
    }
}
