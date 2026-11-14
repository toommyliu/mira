use std::{
    rc::Rc,
    sync::{Arc, LazyLock},
};

use gpui::{
    AnyElement, App, AppContext as _, Bounds, Context, Div, Entity, FocusHandle, Focusable as _,
    FontWeight, Global, InteractiveElement as _, IntoElement, KeyBinding, KeyDownEvent,
    MouseButton, ParentElement as _, Pixels, Render, Role, ScrollHandle, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, TitlebarOptions, WeakEntity,
    Window, WindowBounds, WindowHandle, WindowOptions, actions, div, prelude::FluentBuilder as _,
    px, size,
};
use mira_ui::{
    ActiveTheme as _, FOCUS_SCOPE, StyledExt as _, Theme, Tone,
    components::Empty,
    h_flex,
    nav::{Axis, Step},
    primitives::{
        Align, Button, Dialog, Dot, DropdownMenu, Icon, Input, InputEvent, Kbd, List, ListItem,
        MenuItem, Numeric, Scrollbar, Tab, Tabs, ToggleGroup,
        button::{Size, Variant},
    },
    theme::{CONTROL_MD, CONTROL_SM, ROW, TEXT_BODY, TEXT_SMALL},
    v_flex,
};

use super::{
    client::{CloseSession, SIDEBAR_WIDTH, bar_title, drag_area, reveal_tab, window_bar},
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

actions!(
    mira,
    [OpenCombatProfiles, SaveProfile, NewProfile, FindProfile]
);

const CONTEXT: &str = "CombatProfiles";
const WIDTH: f32 = 1000.0;
const HEIGHT: f32 = 640.0;
const INSPECTOR_WIDTH: Pixels = px(340.0);
const PADDING: Pixels = px(16.0);
const STATUS_WIDTH: Pixels = px(420.0);
const RENAME_INSET: Pixels = px(9.0);
const SUBJECT_WIDTH: Pixels = px(96.0);
const SUBJECT_MENU_WIDTH: Pixels = px(140.0);
const UNIT_WIDTH: Pixels = px(56.0);
const VALUE_WIDTH: Pixels = px(44.0);
const CONJUNCTION_WIDTH: Pixels = px(24.0);
const CONDITION_GAP: Pixels = px(8.0);
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
        KeyBinding::new("cmd-f", FindProfile, Some(CONTEXT)),
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
        titlebar: Some(TitlebarOptions {
            title: Some("Combat Profiles".into()),
            appears_transparent: true,
            traffic_light_position: Some(gpui::point(px(12.0), px(12.0))),
        }),
        app_owns_titlebar_drag: true,
        is_resizable: false,
        is_minimizable: false,
        ..Default::default()
    };
    match cx.open_window(options, |window, cx| {
        let editor = cx.new(|cx| Editor::new(window, cx));
        editor.update(cx, |editor, cx| {
            if let Some(name) = &selected {
                editor.select_named(name, window, cx);
            }
        });
        window.focus(&editor.read(cx).focus.clone(), cx);
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

struct Draft {
    saved: Option<String>,
    name: String,
    form: Form,
}

impl Draft {
    fn edited(&self, library: &Library) -> bool {
        let Some(saved) = &self.saved else {
            return true;
        };
        self.name != *saved || self.form.parse().ok() != library.get(saved)
    }

    fn label(&self) -> SharedString {
        match self.name.trim() {
            "" => "Untitled".into(),
            name => name.to_owned().into(),
        }
    }

    fn unsaved(&self, library: &Library) -> Option<(&'static str, Tone)> {
        match (&self.saved, self.edited(library)) {
            (None, _) => Some(("Not saved yet", Tone::Accent)),
            (Some(_), true) => Some(("Unsaved changes", Tone::Warning)),
            (Some(_), false) => None,
        }
    }

    fn taken<'a>(&'a self, library: &'a Library) -> impl Iterator<Item = &'a str> + 'a {
        library.names().filter(move |name| {
            self.saved
                .as_deref()
                .is_none_or(|saved| !same_name(saved, name))
        })
    }

    fn name_note(&self, library: &Library) -> Option<(String, Tone)> {
        match check_name(&self.name, self.taken(library)) {
            Err(problem) => Some((problem, Tone::Danger)),
            Ok(name) => self
                .saved
                .as_ref()
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

struct Editor {
    focus: FocusHandle,
    library: Arc<Library>,
    drafts: Vec<Draft>,
    selected: Option<usize>,
    item: Option<Item>,
    name: Entity<Input>,
    renaming: Option<String>,
    search: Entity<Input>,
    profile_scroll: ScrollHandle,
    lanes_scroll: ScrollHandle,
    inspector_scroll: ScrollHandle,
    texts: Option<Texts>,
    problem: Option<String>,
    deleting: bool,
    closing: bool,
    _subscriptions: [Subscription; 4],
}

impl Editor {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let library = combat_profiles::library(cx);
        let drafts: Vec<Draft> = library
            .names()
            .map(|name| Draft {
                saved: Some(name.to_owned()),
                name: name.to_owned(),
                form: Form::of(&library.get(name).expect("a listed profile exists")),
            })
            .collect();
        let selected = (!drafts.is_empty()).then_some(0);
        let first = drafts
            .first()
            .map(|draft| draft.name.clone())
            .unwrap_or_default();
        let name = cx.new(|cx| {
            let mut input = Input::new("Profile name", cx).max_length(60);
            input.set_text(first, cx);
            input
        });
        let name_focus = name.read(cx).focus_handle(cx);
        let search = cx.new(|cx| {
            Input::new("Search profiles", cx)
                .placeholder("Search")
                .search()
        });
        let subscriptions = [
            cx.on_blur(&name_focus, window, |this, _, cx| {
                this.renaming = None;
                cx.notify();
            }),
            cx.subscribe_in(&name, window, Self::name_event),
            cx.subscribe_in(&search, window, |this, _, event, window, cx| match event {
                InputEvent::Changed => cx.notify(),
                InputEvent::Submitted => this.step_profile(0, window, cx),
            }),
            cx.observe_global::<combat_profiles::Store>(|this, cx| {
                this.library = combat_profiles::library(cx);
                cx.notify();
            }),
        ];
        Self {
            focus: cx.focus_handle(),
            library,
            drafts,
            selected,
            item: None,
            name,
            renaming: None,
            search,
            profile_scroll: ScrollHandle::new(),
            lanes_scroll: ScrollHandle::new(),
            inspector_scroll: ScrollHandle::new(),
            texts: None,
            problem: None,
            deleting: false,
            closing: false,
            _subscriptions: subscriptions,
        }
    }

    fn draft(&self) -> Option<&Draft> {
        self.selected.and_then(|index| self.drafts.get(index))
    }

    fn form(&self) -> &Form {
        self.draft().map_or(&GENERIC_FORM, |draft| &draft.form)
    }

    fn form_mut(&mut self) -> Option<&mut Form> {
        let index = self.selected?;
        self.drafts.get_mut(index).map(|draft| &mut draft.form)
    }

    fn edited(&self) -> Vec<&Draft> {
        self.drafts
            .iter()
            .filter(|draft| draft.edited(&self.library))
            .collect()
    }

    fn entries(&self, cx: &App) -> Vec<usize> {
        let query = self.search.read(cx).text().trim().to_lowercase();
        (0..self.drafts.len())
            .filter(|index| self.drafts[*index].label().to_lowercase().contains(&query))
            .collect()
    }

    fn select(&mut self, selected: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected != selected {
            self.show(selected, window, cx);
        }
    }

    fn reveal(&self, cx: &App) {
        let entries = self.entries(cx);
        if let Some(row) = entries
            .iter()
            .position(|entry| Some(*entry) == self.selected)
        {
            reveal_tab(&self.profile_scroll, row, entries.len());
        }
    }

    fn show(&mut self, selected: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        self.selected = selected;
        self.reveal(cx);
        self.item = None;
        self.problem = None;
        self.renaming = None;
        let name = self
            .draft()
            .map(|draft| draft.name.clone())
            .unwrap_or_default();
        self.name.update(cx, |input, cx| input.set_text(name, cx));
        self.rebuild_texts(window, cx);
        cx.notify();
    }

    fn select_named(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        let index = self.drafts.iter().position(|draft| {
            draft
                .saved
                .as_deref()
                .is_some_and(|saved| same_name(saved, name))
        });
        if index.is_some() {
            self.select(index, window, cx);
        }
    }

    fn select_near(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let nearest = index.min(self.drafts.len().saturating_sub(1));
        self.show((!self.drafts.is_empty()).then_some(nearest), window, cx);
    }

    fn step_profile(&mut self, by: isize, window: &mut Window, cx: &mut Context<Self>) {
        let entries = self.entries(cx);
        let next = match entries
            .iter()
            .position(|entry| Some(*entry) == self.selected)
        {
            Some(at) => at
                .checked_add_signed(by)
                .filter(|next| *next < entries.len()),
            None => (!entries.is_empty()).then_some(0),
        };
        if let Some(next) = next {
            self.select(Some(entries[next]), window, cx);
        }
    }

    fn select_item(&mut self, item: Option<Item>, window: &mut Window, cx: &mut Context<Self>) {
        self.item = item;
        self.rebuild_texts(window, cx);
        cx.notify();
    }

    fn rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(draft) = self.draft() else {
            return;
        };
        self.renaming = Some(draft.name.clone());
        self.search.update(cx, |search, cx| search.set_text("", cx));
        self.reveal(cx);
        self.name.read(cx).focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    fn cancel_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(name) = self.renaming.take() else {
            return;
        };
        self.name.update(cx, |input, cx| input.set_text(name, cx));
        window.focus(&self.focus, cx);
        cx.notify();
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
                if let Some(index) = self.selected
                    && let Some(draft) = self.drafts.get_mut(index)
                    && draft.name != text
                {
                    draft.name = text;
                    cx.notify();
                }
                let invalid = self.draft().is_some_and(|draft| {
                    draft
                        .name_note(&self.library)
                        .is_some_and(|(_, tone)| tone == Tone::Danger)
                });
                input.update(cx, |input, cx| input.set_invalid(invalid, cx));
            }
            InputEvent::Submitted => {
                self.renaming = None;
                window.focus(&self.focus, cx);
                cx.notify();
            }
        }
    }

    fn rebuild_texts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let texts: Vec<(Text, &'static str, String)> = match self.item {
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
            self.item = None;
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
                        input = input.numeric(MILLIS).suffix("ms");
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
        let Some(Item::Step(lane, index)) = self.item else {
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
        let Some(Item::Trigger(index)) = self.item else {
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
        match self.item {
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
        let Some(item) = self.item else {
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
        let Some(item) = self.item else {
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
            same_name(name, GENERIC)
                || self.library.names().any(|saved| same_name(saved, name))
                || self.drafts.iter().any(|draft| same_name(&draft.name, name))
        };
        if !taken(base) {
            return base.to_owned();
        }
        (2..)
            .map(|number| format!("{base} {number}"))
            .find(|name| !taken(name))
            .expect("some numbered name is free")
    }

    fn add_profile(&mut self, form: Form, base: &str, window: &mut Window, cx: &mut Context<Self>) {
        let name = self.unique_name(base);
        self.drafts.push(Draft {
            saved: None,
            name,
            form,
        });
        self.select(Some(self.drafts.len() - 1), window, cx);
        self.rename(window, cx);
    }

    fn new_profile(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.add_profile(Form::of(&Profile::generic()), "New profile", window, cx);
    }

    fn duplicate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(draft) = self.draft() else {
            return;
        };
        let form = draft.form.clone();
        let base = format!("{} copy", draft.name.trim());
        self.add_profile(form, &base, window, cx);
    }

    fn revert(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(index) = self.selected else {
            return;
        };
        let Some(saved) = self.drafts[index].saved.clone() else {
            self.drafts.remove(index);
            self.select_near(index, window, cx);
            return;
        };
        let Some(profile) = self.library.get(&saved) else {
            return;
        };
        let draft = &mut self.drafts[index];
        draft.form = Form::of(&profile);
        draft.name = saved.clone();
        self.name.update(cx, |input, cx| input.set_text(saved, cx));
        self.problem = None;
        self.renaming = None;
        self.rebuild_texts(window, cx);
        cx.notify();
    }

    fn delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.deleting = false;
        let Some(index) = self.selected else {
            return;
        };
        if let Some(saved) = &self.drafts[index].saved
            && let Err(error) = combat_profiles::delete(saved, cx)
        {
            self.problem = Some(format!("Couldn't delete the profile: {error}"));
            cx.notify();
            return;
        }
        self.drafts.remove(index);
        self.select_near(index, window, cx);
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(index) = self.selected
            && let Err((at, problem)) = self.save_draft(index, cx)
        {
            self.show_problem(at, problem, window, cx);
        }
        cx.notify();
    }

    fn show_problem(
        &mut self,
        at: Option<Item>,
        problem: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if at.is_some() {
            self.select_item(at, window, cx);
        }
        self.problem = Some(problem);
        cx.notify();
    }

    fn save_draft(
        &mut self,
        index: usize,
        cx: &mut Context<Self>,
    ) -> Result<(), (Option<Item>, String)> {
        let draft = &self.drafts[index];
        let name = check_name(&draft.name, draft.taken(&self.library))
            .map_err(|problem| (None, problem))?;
        let profile = draft
            .form
            .parse()
            .map_err(|problem| (Some(problem.at), problem.message))?;
        combat_profiles::save(draft.saved.as_deref(), name.clone(), profile, cx)
            .map_err(|error| (None, format!("Couldn't save the profile: {error}")))?;
        self.library = combat_profiles::library(cx);
        let draft = &mut self.drafts[index];
        draft.saved = Some(name.clone());
        draft.name = name;
        self.problem = None;
        Ok(())
    }

    fn may_close(&mut self, cx: &mut Context<Self>) -> bool {
        if self.edited().is_empty() {
            return true;
        }
        self.closing = true;
        cx.notify();
        false
    }

    fn save_all_and_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let edited: Vec<usize> = (0..self.drafts.len())
            .filter(|index| self.drafts[*index].edited(&self.library))
            .collect();
        for index in edited {
            if let Err((at, problem)) = self.save_draft(index, cx) {
                self.closing = false;
                self.select(Some(index), window, cx);
                self.show_problem(at, problem, window, cx);
                return;
            }
        }
        window.remove_window();
    }

    fn sidebar(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let entries: Rc<[usize]> = self.entries(cx).into();
        let rows: Rc<[ProfileRow]> = entries
            .iter()
            .map(|index| {
                let draft = &self.drafts[*index];
                let invalid = draft
                    .name_note(&self.library)
                    .is_some_and(|(_, tone)| tone == Tone::Danger);
                ProfileRow {
                    title: draft.label(),
                    mark: if invalid {
                        Some(Tone::Danger)
                    } else {
                        draft.unsaved(&self.library).map(|(_, tone)| tone)
                    },
                }
            })
            .collect();
        let active = entries
            .iter()
            .position(|entry| Some(*entry) == self.selected)
            .unwrap_or(entries.len());
        let rename = self.renaming.is_some().then(|| self.name.clone());
        let this = cx.weak_entity();
        let selector = this.clone();
        let selectable = entries.clone();
        let profiles = Tabs::new("profiles", "Combat profiles", rows.len(), move |index| {
            let ProfileRow { title, mark } = &rows[index];
            let entry = entries[index];
            let current = index == active;
            let menu = {
                let this = this.clone();
                move |_: &mut Window, _: &mut App| profile_menu(&this, entry)
            };
            let mut tab = Tab::new(title.clone()).menu(menu.clone());
            if let (Some(input), true) = (&rename, current) {
                let canceller = this.clone();
                tab = tab.child(
                    div()
                        .mx(-RENAME_INSET)
                        .my(px(-4.0))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_key_down(move |event, window, cx| {
                            if event.keystroke.key == "escape" {
                                canceller
                                    .update(cx, |editor, cx| editor.cancel_rename(window, cx))
                                    .ok();
                                cx.stop_propagation();
                            } else if Step::from_key(event, Axis::Vertical).is_some() {
                                cx.stop_propagation();
                            }
                        })
                        .child(input.clone()),
                );
            }
            if current && rename.is_none() {
                tab = tab.end_slot(
                    DropdownMenu::new(("profile-actions", index))
                        .width(px(180.0))
                        .trigger(
                            Button::icon(
                                ("profile-actions-trigger", index),
                                Icon::Ellipsis,
                                "Profile actions",
                            )
                            .variant(Variant::Ghost)
                            .size(Size::XSmall)
                            .hide_tooltip(),
                        )
                        .items(menu),
                );
            } else if !current && let Some(tone) = mark {
                tab = tab.end_slot(Dot::new(*tone));
            }
            tab
        })
        .vertical()
        .selected(active)
        .track_scroll(&self.profile_scroll)
        .on_select(move |index, window, cx| {
            selector
                .update(cx, |editor, cx| {
                    editor.select(Some(selectable[index]), window, cx)
                })
                .ok();
        });
        let issues = combat_profiles::issues(cx).len();
        v_flex()
            .flex_none()
            .w(SIDEBAR_WIDTH)
            .h_full()
            .bg(theme.well)
            .border_r_1()
            .border_color(theme.hairline)
            .text_size(TEXT_BODY)
            .text_color(theme.text)
            .child(
                div()
                    .flex_none()
                    .px(px(8.0))
                    .pt(px(8.0))
                    .pb(px(6.0))
                    .capture_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                        let by = match event.keystroke.key.as_str() {
                            "up" => -1,
                            "down" => 1,
                            _ => return,
                        };
                        this.step_profile(by, window, cx);
                        cx.stop_propagation();
                    }))
                    .child(self.search.clone()),
            )
            .child(
                div()
                    .relative()
                    .flex()
                    .flex_col()
                    .min_h_0()
                    .child(v_flex().min_h_0().child(profiles))
                    .child(Scrollbar::new(
                        "profiles-scrollbar",
                        self.profile_scroll.clone(),
                    )),
            )
            .child(drag_area().flex_1())
            .when(issues > 0, |sidebar| {
                sidebar.child(
                    div()
                        .flex_none()
                        .p(px(12.0))
                        .text_small()
                        .text_color(theme.warning_text)
                        .child(if issues == 1 {
                            "1 saved profile couldn't be read, so it's left out.".to_owned()
                        } else {
                            format!(
                                "{issues} saved profiles couldn't be read, so they're left out."
                            )
                        }),
                )
            })
            .child(
                div().flex_none().p(px(8.0)).child(
                    Button::new("new-profile", "New profile")
                        .variant(Variant::Ghost)
                        .w_full()
                        .child(
                            h_flex()
                                .flex_1()
                                .justify_between()
                                .child("New profile")
                                .child(div().text_color(theme.text_tertiary).child("⌘N")),
                        )
                        .on_click(cx.listener(|this, _, window, cx| this.new_profile(window, cx))),
                ),
            )
    }

    fn section(&self, theme: &Theme, section: Section, cx: &mut Context<Self>) -> AnyElement {
        let (title, subtitle, empty) = section.copy();
        let editable = self.draft().is_some();
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
            .item
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
            if editable {
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
            } else {
                list.into_any_element()
            }
        };
        let add = editable.then(|| self.add_menu(section, cx));
        v_flex()
            .gap(px(8.0))
            .child(form::section(
                title,
                subtitle,
                None,
                div().children(add),
                theme,
            ))
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
        let this = cx.weak_entity();
        form::row(
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

    fn save_controls(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let draft = self.draft();
        let unsaved = draft.and_then(|draft| draft.unsaved(&self.library));
        let note = draft.and_then(|draft| draft.name_note(&self.library));
        let status = match (&self.problem, note, unsaved) {
            (Some(problem), _, _) => Some((SharedString::from(problem.clone()), theme.danger_text)),
            (None, Some((note, tone)), _) => Some((note.into(), theme.tone(tone).2)),
            (None, None, Some((text, tone))) => Some((text.into(), theme.tone(tone).2)),
            (None, None, None) => None,
        };
        let revert = unsaved.is_some().then(|| {
            let label = if draft.is_some_and(|draft| draft.saved.is_some()) {
                "Revert"
            } else {
                "Discard"
            };
            Button::new("revert-profile", label)
                .variant(Variant::Ghost)
                .size(Size::Small)
                .on_click(cx.listener(|this, _, window, cx| this.revert(window, cx)))
        });
        h_flex()
            .flex_none()
            .gap(px(6.0))
            .children(status.map(|(text, color)| {
                div()
                    .id("profile-status")
                    .role(Role::Status)
                    .aria_label(text.clone())
                    .max_w(STATUS_WIDTH)
                    .mr(px(6.0))
                    .truncate()
                    .text_small()
                    .text_color(color)
                    .child(text)
            }))
            .children(revert)
            .child(
                Button::new("save-profile", "Save")
                    .variant(Variant::Primary)
                    .size(Size::Small)
                    .end_slot(key_hint("⌘S"))
                    .disabled(unsaved.is_none())
                    .on_click(cx.listener(|this, _, window, cx| this.save(window, cx))),
            )
    }

    fn inspector(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let pane = v_flex()
            .id("inspector")
            .size_full()
            .gap(px(14.0))
            .p(PADDING)
            .pt(px(12.0))
            .overflow_y_scroll()
            .track_scroll(&self.inspector_scroll);
        if self.draft().is_none() {
            return pane;
        }
        let (Some(item), Some(texts)) = (self.item, &self.texts) else {
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
        let skills = h_flex().gap(px(8.0)).children(SLOTS.map(|key| {
            let on = key == slot;
            let this = skill_this.clone();
            Button::new(("item-skill", usize::from(key)), key.to_string())
                .variant(if on {
                    Variant::Primary
                } else {
                    Variant::Secondary
                })
                .pressed(on)
                .w(CONTROL_MD)
                .px_0()
                .on_click(move |_, _, cx| {
                    let _ = this.update(cx, |this, cx| this.set_skill(key, cx));
                })
        }));
        let pane = pane.child(
            h_flex()
                .h(ROW)
                .justify_between()
                .gap(px(8.0))
                .child(heading(item_title(item)))
                .child(
                    h_flex()
                        .gap(px(2.0))
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
                    "Message",
                    true,
                    div().children(texts.get(Text::Includes)),
                ))
                .child(row_note(
                    theme,
                    "Matched anywhere in combat and server messages, ignoring case.",
                ))
                .child(form::row("Cast", true, skills))
                .child(row_note(
                    theme,
                    "Casts once, as soon as the skill is ready.",
                )),
            Item::Step(lane, index) => {
                let Some(step) = self.form().lane(lane).get(index) else {
                    return pane;
                };
                let mut pane = pane.child(form::row("Skill", true, skills));
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
                    "Pause after",
                    true,
                    millis_input(texts, Text::Pause),
                ))
                .child(
                    v_flex()
                        .gap(px(6.0))
                        .child(
                            h_flex()
                                .h(ROW)
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
                            v_flex().gap(px(12.0)).pt(px(2.0)).children(
                                conditions.into_iter().enumerate().map(|(index, row)| {
                                    h_flex()
                                        .items_start()
                                        .gap(CONDITION_GAP)
                                        .child(
                                            div()
                                                .flex_none()
                                                .flex()
                                                .items_center()
                                                .h(CONTROL_SM)
                                                .w(CONJUNCTION_WIDTH)
                                                .text_small()
                                                .text_color(theme.text_tertiary)
                                                .child(if index == 0 { "If" } else { "and" }),
                                        )
                                        .child(div().flex_1().min_w_0().child(row))
                                }),
                            )
                        })
                        .children(hint.map(|hint| form::note(hint, true, theme).mt(px(4.0)))),
                )
            }
        }
    }
}

fn profile_menu(this: &WeakEntity<Editor>, index: usize) -> Vec<MenuItem> {
    let act = |label: &'static str, run: fn(&mut Editor, &mut Window, &mut Context<Editor>)| {
        let this = this.clone();
        MenuItem::action(label, move |window, cx| {
            this.update(cx, |editor, cx| {
                editor.select(Some(index), window, cx);
                run(editor, window, cx);
            })
            .ok();
        })
    };
    vec![
        act("Rename", Editor::rename),
        act("Duplicate", Editor::duplicate),
        MenuItem::separator(),
        act("Delete…", |editor, _, cx| {
            editor.deleting = true;
            cx.notify();
        })
        .danger(),
    ]
}

fn empty_box(theme: &Theme, text: &'static str) -> Div {
    div()
        .px(px(12.0))
        .py(px(8.0))
        .rounded(theme.radius.panel)
        .border_1()
        .border_color(theme.hairline)
        .text_small()
        .text_color(theme.text_tertiary)
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

fn row_note(theme: &Theme, text: &'static str) -> Div {
    form::note(text, true, theme)
        .ml(form::LABEL_WIDTH + px(12.0))
        .mt(px(-6.0))
}

fn key_hint(keys: &'static str) -> impl IntoElement {
    div().text_size(TEXT_SMALL).opacity(0.55).child(keys)
}

fn millis_input(texts: &Texts, text: Text) -> impl IntoElement + use<> {
    div().w(px(96.0)).children(texts.get(text))
}

struct ProfileRow {
    title: SharedString,
    mark: Option<Tone>,
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
                                theme.text_tertiary,
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
    .w(CONTROL_SM)
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
        let unsaved: Vec<SharedString> = self.edited().iter().map(|draft| draft.label()).collect();
        let deleting_name = self.draft().map(Draft::label).unwrap_or_default();
        let sidebar = self.sidebar(&theme, cx);
        let sections: Vec<AnyElement> = if self.draft().is_some() {
            Section::ALL
                .into_iter()
                .map(|section| self.section(&theme, section, cx))
                .collect()
        } else {
            vec![
                Empty::new(
                    "No combat profiles",
                    "Make one to choose the skills auto attack, Follower and scripts cast.",
                )
                .action(
                    Button::new("first-profile", "New profile")
                        .end_slot(key_hint("⌘N"))
                        .on_click(cx.listener(|this, _, window, cx| this.new_profile(window, cx))),
                )
                .into_any_element(),
            ]
        };
        let save = self.save_controls(&theme, cx);
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
                    .gap(px(20.0))
                    .p(PADDING)
                    .overflow_y_scroll()
                    .track_scroll(&self.lanes_scroll)
                    .children(sections),
            )
            .child(Scrollbar::new(
                "profile-scrollbar",
                self.lanes_scroll.clone(),
            ));
        let inspector_pane = div()
            .relative()
            .flex_none()
            .w(INSPECTOR_WIDTH)
            .h_full()
            .border_l_1()
            .border_color(theme.hairline)
            .child(self.inspector(&theme, cx))
            .child(Scrollbar::new(
                "inspector-scrollbar",
                self.inspector_scroll.clone(),
            ));
        let bar = window_bar(window, cx)
            .child(bar_title("Combat Profiles", cx))
            .child(drag_area().flex_1())
            .child(save);
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(theme.base)
            .text_size(TEXT_BODY)
            .text_color(theme.text)
            .key_context(format!("{FOCUS_SCOPE} {CONTEXT}").as_str())
            .id("combat-profiles")
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &SaveProfile, window, cx| this.save(window, cx)))
            .on_action(cx.listener(|this, _: &NewProfile, window, cx| this.new_profile(window, cx)))
            .on_action(cx.listener(|this, _: &FindProfile, window, cx| {
                this.search.read(cx).focus_handle(cx).focus(window, cx)
            }))
            .on_action(cx.listener(|this, _: &CloseSession, window, cx| {
                if this.may_close(cx) {
                    window.remove_window();
                }
            }))
            .child(mira_ui::nav::pointer_probe())
            .child(bar)
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(sidebar)
                    .child(lanes)
                    .child(inspector_pane),
            )
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
                Dialog::new("unsaved-profiles", self.closing, "Save your changes?")
                    .alert()
                    .description(format!("Unsaved: {}.", unsaved.join(", ")))
                    .action(
                        Button::new("discard-profiles", "Don't save")
                            .on_click(cx.listener(|_, _, window, _| window.remove_window())),
                    )
                    .action(Button::new("keep-editing", "Cancel").on_click(cx.listener(
                        |this, _, _, cx| {
                            this.closing = false;
                            cx.notify();
                        },
                    )))
                    .action(
                        Button::new("save-profiles", "Save all")
                            .variant(Variant::Primary)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.save_all_and_close(window, cx)
                            })),
                    )
                    .on_close({
                        let this = cx.weak_entity();
                        move |_, cx| {
                            let _ = this.update(cx, |this, cx| {
                                this.closing = false;
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
    use crate::ruffle_view::BAR_HEIGHT;

    fn library_file(name: &str) -> std::path::PathBuf {
        let mut library = Library::default();
        let profile = |value| serde_json::from_value::<Profile>(value).unwrap();
        library.put(
            None,
            "Ultra speaker".into(),
            profile(json!({
                "onCooldown": "wait",
                "triggers": [{ "includes": "prepares a massive attack", "skill": 3, "within": 3000 }],
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
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
                .size_full()
                .font_family(mira_ui::theme::SYSTEM_SANS)
                .child(self.0.clone())
        }
    }

    fn snapshot(
        name: &str,
        prepare: impl Fn(&mut Editor, &mut Window, &mut Context<Editor>) + 'static,
    ) {
        let path = library_file(name);
        for written in mira_ui::snapshot(name, size(px(WIDTH), px(HEIGHT)), move |window, cx| {
            combat_profiles::init(path.clone(), cx);
            let editor = cx.new(|cx| Editor::new(window, cx));
            editor.update(cx, |editor, cx| prepare(editor, window, cx));
            cx.new(|_| Host(editor))
        }) {
            assert!(written.exists(), "{} was not written", written.display());
        }
    }

    #[test]
    fn snapshot_editor() {
        snapshot("profiles-step", |editor, window, cx| {
            editor.drafts[1].form.steps.pop();
            editor.new_profile(window, cx);
            editor.select_named("Ultra speaker", window, cx);
            editor.form_mut().unwrap().steps[1].pause = "500".into();
            editor.select_item(Some(Item::Step(Lane::Rotation, 2)), window, cx);
        });
        snapshot("profiles-trigger", |editor, window, cx| {
            editor.select_named("Ultra speaker", window, cx);
            editor.rename(window, cx);
            editor
                .name
                .update(cx, |input, cx| input.set_text("Daily boss", cx));
            editor.select_item(Some(Item::Trigger(0)), window, cx);
        });
        snapshot("profiles-empty", |editor, window, cx| {
            editor.drafts.clear();
            editor.select(None, window, cx);
        });
    }

    fn open_editor<'a>(
        name: &str,
        cx: &'a mut gpui::TestAppContext,
    ) -> (Entity<Editor>, &'a mut gpui::VisualTestContext) {
        let path = library_file(name);
        cx.update(|cx| {
            mira_ui::init(cx);
            bind_keys(cx);
            combat_profiles::init(path, cx);
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

    fn press(cx: &mut gpui::VisualTestContext, keys: &str) {
        cx.simulate_keystrokes(keys);
        cx.run_until_parked();
    }

    fn click(cx: &mut gpui::VisualTestContext, at: gpui::Point<gpui::Pixels>) {
        cx.simulate_click(at, gpui::Modifiers::none());
        cx.run_until_parked();
    }

    fn profile_row(index: usize) -> gpui::Point<gpui::Pixels> {
        gpui::point(
            px(60.0),
            BAR_HEIGHT + px(40.0 + 4.0 + 15.0 + 32.0 * index as f32),
        )
    }

    fn shown(editor: &Entity<Editor>, cx: &mut gpui::VisualTestContext) -> Option<String> {
        editor.read_with(cx, |editor, _| {
            editor.draft().map(|draft| draft.name.clone())
        })
    }

    #[gpui::test]
    fn arrow_keys_move_between_profiles(cx: &mut gpui::TestAppContext) {
        let (editor, cx) = open_editor("arrows", cx);
        assert_eq!(shown(&editor, cx).as_deref(), Some("Daily boss"));
        click(cx, profile_row(1));
        assert_eq!(shown(&editor, cx).as_deref(), Some("Farming"));
        press(cx, "down");
        assert_eq!(shown(&editor, cx).as_deref(), Some("Ultra speaker"));
        press(cx, "up up");
        assert_eq!(shown(&editor, cx).as_deref(), Some("Daily boss"));
    }

    #[gpui::test]
    fn the_profile_list_scrolls_to_keep_the_selection_in_view(cx: &mut gpui::TestAppContext) {
        let (editor, cx) = open_editor("reveal", cx);
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                for _ in 0..40 {
                    editor.add_profile(Form::of(&Profile::generic()), "Extra", window, cx);
                }
            })
        });
        cx.run_until_parked();
        let scrolled = |cx: &mut gpui::VisualTestContext| {
            editor.read_with(cx, |editor, _| {
                let scroll = &editor.profile_scroll;
                (scroll.offset().y, scroll.max_offset().y)
            })
        };
        click(cx, profile_row(0));
        press(cx, "home");
        assert_eq!(scrolled(cx).0, px(0.0));
        press(cx, "end");
        let (offset, max) = scrolled(cx);
        assert!(max > px(0.0));
        assert_eq!(offset, -max);
    }

    #[gpui::test]
    fn the_selected_profiles_menu_opens_from_the_keyboard(cx: &mut gpui::TestAppContext) {
        let (editor, cx) = open_editor("menu", cx);
        click(cx, profile_row(0));
        press(cx, "down shift-f10");
        press(cx, "down enter");
        assert_eq!(shown(&editor, cx).as_deref(), Some("Farming copy"));
        assert!(editor.read_with(cx, |editor, _| editor.renaming.is_some()));
    }

    #[gpui::test]
    fn steps_take_arrow_keys_and_stay_selected_when_their_profile_is_chosen_again(
        cx: &mut gpui::TestAppContext,
    ) {
        let (editor, cx) = open_editor("steps", cx);
        let item = |cx: &mut gpui::VisualTestContext| editor.read_with(cx, |editor, _| editor.item);
        click(cx, profile_row(2));
        let third = cx
            .debug_bounds("row-2")
            .expect("the rotation lists three steps");
        click(cx, third.center());
        assert_eq!(item(cx), Some(Item::Step(Lane::Rotation, 2)));
        press(cx, "up");
        assert_eq!(item(cx), Some(Item::Step(Lane::Rotation, 1)));
        click(cx, profile_row(2));
        assert_eq!(item(cx), Some(Item::Step(Lane::Rotation, 1)));
    }

    #[gpui::test]
    fn search_narrows_the_profiles_and_arrows_step_through_the_matches(
        cx: &mut gpui::TestAppContext,
    ) {
        let (editor, cx) = open_editor("search", cx);
        press(cx, "cmd-f");
        cx.simulate_input("a");
        cx.run_until_parked();
        press(cx, "down");
        assert_eq!(shown(&editor, cx).as_deref(), Some("Farming"));
        press(cx, "down");
        assert_eq!(shown(&editor, cx).as_deref(), Some("Ultra speaker"));
        press(cx, "down");
        assert_eq!(shown(&editor, cx).as_deref(), Some("Ultra speaker"));
        cx.simulate_input("k");
        cx.run_until_parked();
        press(cx, "up");
        assert_eq!(shown(&editor, cx).as_deref(), Some("Ultra speaker"));
        press(cx, "escape up");
        assert_eq!(shown(&editor, cx).as_deref(), Some("Farming"));
    }

    #[gpui::test]
    fn command_n_starts_a_new_profile_named_in_its_row(cx: &mut gpui::TestAppContext) {
        let (editor, cx) = open_editor("new", cx);
        press(cx, "cmd-n");
        assert_eq!(shown(&editor, cx).as_deref(), Some("New profile"));
        assert!(editor.read_with(cx, |editor, _| editor.renaming.is_some()));
        cx.simulate_input("Boss rush");
        press(cx, "enter");
        assert_eq!(shown(&editor, cx).as_deref(), Some("Boss rush"));
        assert!(!editor.read_with(cx, |editor, _| editor.renaming.is_some()));
    }

    #[gpui::test]
    fn escape_puts_the_name_back_and_stops_renaming(cx: &mut gpui::TestAppContext) {
        let (editor, cx) = open_editor("cancel", cx);
        click(cx, profile_row(1));
        press(cx, "shift-f10");
        press(cx, "enter");
        cx.simulate_input("Grinding");
        cx.run_until_parked();
        assert_eq!(shown(&editor, cx).as_deref(), Some("Grinding"));
        press(cx, "escape");
        assert_eq!(shown(&editor, cx).as_deref(), Some("Farming"));
        assert!(!editor.read_with(cx, |editor, _| editor.renaming.is_some()));
    }

    #[gpui::test]
    fn renaming_under_a_search_keeps_the_name_field(cx: &mut gpui::TestAppContext) {
        let (editor, cx) = open_editor("rename-search", cx);
        press(cx, "cmd-f");
        cx.simulate_input("farm");
        cx.run_until_parked();
        press(cx, "down");
        cx.update(|window, cx| editor.update(cx, |editor, cx| editor.rename(window, cx)));
        cx.run_until_parked();
        cx.simulate_input("Solo");
        press(cx, "enter");
        assert_eq!(shown(&editor, cx).as_deref(), Some("Solo"));
    }

    #[gpui::test]
    fn a_new_profile_under_a_search_shows_its_name_field(cx: &mut gpui::TestAppContext) {
        let (editor, cx) = open_editor("new-search", cx);
        press(cx, "cmd-f");
        cx.simulate_input("zzz");
        cx.run_until_parked();
        press(cx, "cmd-n");
        cx.simulate_input("Boss rush");
        press(cx, "enter");
        assert_eq!(shown(&editor, cx).as_deref(), Some("Boss rush"));
    }

    #[gpui::test]
    fn discarding_the_last_profile_clears_its_save_problem(cx: &mut gpui::TestAppContext) {
        let (editor, cx) = open_editor("discard-last", cx);
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.drafts.clear();
                editor.select(None, window, cx);
                editor.new_profile(window, cx);
                editor.add(Section::Triggers, 3, window, cx);
                editor.save(window, cx);
                assert!(editor.problem.is_some());
                editor.revert(window, cx);
                assert_eq!(editor.draft().map(|draft| draft.name.as_str()), None);
                assert_eq!(editor.problem, None);
                assert_eq!(editor.item, None);
            })
        });
    }

    #[gpui::test]
    fn revert_restores_the_saved_profile_and_discards_a_new_one(cx: &mut gpui::TestAppContext) {
        let (editor, cx) = open_editor("revert", cx);
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.select_named("Farming", window, cx);
                editor.add(Section::Steps(Lane::Rotation), 4, window, cx);
                editor.rename(window, cx);
                editor
                    .name
                    .update(cx, |input, cx| input.set_text("Grinding", cx));
                assert!(editor.draft().unwrap().edited(&editor.library));
                editor.revert(window, cx);
                let draft = editor.draft().unwrap();
                assert_eq!(draft.name, "Farming");
                assert!(!draft.edited(&editor.library));
                let saved = editor.drafts.len();
                editor.new_profile(window, cx);
                editor.revert(window, cx);
                assert_eq!(editor.drafts.len(), saved);
                assert_eq!(
                    editor.draft().map(|draft| draft.name.as_str()),
                    Some("Ultra speaker")
                );
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
    fn a_saved_draft_with_equivalent_text_is_not_edited() {
        let mut library = Library::default();
        library.put(None, "Generic".into(), Profile::generic());
        let mut draft = Draft {
            saved: Some("Generic".into()),
            name: "Generic".into(),
            form: Form::of(&Profile::generic()),
        };
        draft.form.steps[0].pause = "0".into();
        assert!(!draft.edited(&library));
        draft.form.steps[0].pause = "100".into();
        assert!(draft.edited(&library));
    }
}
