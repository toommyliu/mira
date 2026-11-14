use std::{
    rc::Rc,
    sync::{Arc, LazyLock},
};

use gpui::{
    AnyElement, App, AppContext as _, Bounds, Context, Entity, FocusHandle, Focusable as _, Global,
    InteractiveElement as _, IntoElement, KeyBinding, ParentElement as _, Render, ScrollHandle,
    SharedString, StatefulInteractiveElement as _, Styled as _, Subscription, TitlebarOptions,
    WeakEntity, Window, WindowBounds, WindowHandle, WindowOptions, actions, div,
    prelude::FluentBuilder as _, px, size,
};
use mira_ui::{
    ActiveTheme as _, FOCUS_SCOPE, StyledExt as _, Theme, Tone, h_flex,
    primitives::{
        Alert, Badge, Button, Dialog, Field, Icon, Input, InputEvent, Kbd, List, ListItem,
        MenuItem, Numeric, Tab, Tabs, ToggleGroup,
        button::{Size, Variant},
    },
    theme::{CONTROL_XS, TEXT_BODY},
    v_flex,
};

use super::{
    BAR_HEIGHT,
    account::Glance,
    client::{CloseSession, SIDEBAR_WIDTH, bar, bar_title, drag_area, reveal_tab, tab_face},
    profile_form::{Form, FormStep, FormTrigger, Item, Kind, Rule},
};
use crate::{
    combat_profiles,
    domain::{
        combat::Skill,
        profiles::{
            GENERIC, Holder, Library, Measure, OnCooldown, Profile, Who, check_name, same_name,
        },
        rotation::Lane,
    },
};

actions!(mira, [OpenCombatProfiles, SaveProfile]);

const CONTEXT: &str = "CombatProfiles";
const WIDTH: f32 = 1000.0;
const HEIGHT: f32 = 640.0;
const INSPECTOR_WIDTH: f32 = 300.0;
const SLOTS: [u8; 6] = [0, 1, 2, 3, 4, 5];
const MILLIS: Numeric = Numeric {
    min: 0,
    max: 600_000,
    step: 100,
};
static GENERIC_FORM: LazyLock<Form> = LazyLock::new(|| Form::of(&Profile::generic()));

struct Open(WindowHandle<Editor>);

impl Global for Open {}

pub(crate) fn bind_keys(cx: &mut App) {
    cx.bind_keys([KeyBinding::new("cmd-s", SaveProfile, Some(CONTEXT))]);
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

    fn glance(&self, library: &Library) -> Glance {
        let (label, tone) = match (&self.saved, self.edited(library)) {
            (None, _) => ("Not saved yet".into(), Tone::Accent),
            (Some(_), true) => ("Unsaved changes".into(), Tone::Warning),
            (Some(_), false) => (shape(&self.form).into(), Tone::Neutral),
        };
        Glance { label, tone }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Text {
    Includes,
    Within,
    Pause,
    Value(usize),
    Name(usize),
}

impl Text {
    fn millis(self) -> bool {
        matches!(self, Text::Within | Text::Pause)
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
    renaming: bool,
    profile_scroll: ScrollHandle,
    texts: Option<Texts>,
    problem: Option<String>,
    deleting: bool,
    closing: bool,
    _subscriptions: [Subscription; 3],
}

impl Editor {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let library = combat_profiles::library(cx);
        let drafts = library
            .names()
            .map(|name| Draft {
                saved: Some(name.to_owned()),
                name: name.to_owned(),
                form: Form::of(&library.get(name).expect("a listed profile exists")),
            })
            .collect();
        let name = cx.new(|cx| Input::new("Profile name", cx).max_length(60));
        let name_focus = name.read(cx).focus_handle(cx);
        let subscriptions = [
            cx.on_blur(&name_focus, window, |this, _, cx| {
                this.renaming = false;
                cx.notify();
            }),
            cx.subscribe_in(&name, window, Self::name_event),
            cx.observe_global::<combat_profiles::Store>(|this, cx| {
                this.library = combat_profiles::library(cx);
                cx.notify();
            }),
        ];
        Self {
            focus: cx.focus_handle(),
            library,
            drafts,
            selected: None,
            item: None,
            name,
            renaming: false,
            profile_scroll: ScrollHandle::new(),
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

    fn select(&mut self, selected: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected == selected {
            return;
        }
        self.selected = selected;
        reveal_tab(
            &self.profile_scroll,
            selected.map_or(0, |index| index + 1),
            self.drafts.len() + 1,
        );
        self.item = None;
        self.problem = None;
        self.renaming = false;
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
        self.select(index, window, cx);
    }

    fn select_item(&mut self, item: Option<Item>, window: &mut Window, cx: &mut Context<Self>) {
        self.item = item;
        self.rebuild_texts(window, cx);
        cx.notify();
    }

    fn rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected.is_none() {
            return;
        }
        self.renaming = true;
        self.name.read(cx).focus_handle(cx).focus(window, cx);
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
            }
            InputEvent::Submitted => {
                self.renaming = false;
                window.focus(&self.focus, cx);
                cx.notify();
            }
        }
    }

    fn rebuild_texts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let texts: Vec<(Text, &'static str, String)> = match self.item {
            None => Vec::new(),
            Some(Item::Trigger(index)) => match self.form().triggers.get(index) {
                Some(trigger) => vec![
                    (
                        Text::Includes,
                        "When this message appears",
                        trigger.includes.clone(),
                    ),
                    (
                        Text::Within,
                        "Give up after, in milliseconds",
                        trigger.within.clone(),
                    ),
                ],
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
            Text::Within => self.edit_trigger(cx, |trigger| trigger.within = text),
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

    fn add_rule(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.edit_step(cx, |step| step.when.push(Rule::default()));
        self.rebuild_texts(window, cx);
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

    fn duplicate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let form = self.form().clone();
        let base = self.draft().map_or_else(
            || format!("{GENERIC} copy"),
            |draft| format!("{} copy", draft.name.trim()),
        );
        self.add_profile(form, &base, window, cx);
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
        self.select(None, window, cx);
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
        let others: Vec<&str> = self
            .library
            .names()
            .filter(|name| {
                draft
                    .saved
                    .as_deref()
                    .is_none_or(|saved| !same_name(saved, name))
            })
            .collect();
        let name = check_name(&draft.name, others).map_err(|problem| (None, problem))?;
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
        let rows: Rc<[(SharedString, Glance)]> = std::iter::once((
            SharedString::from(GENERIC),
            Glance {
                label: "Built in".into(),
                tone: Tone::Neutral,
            },
        ))
        .chain(
            self.drafts
                .iter()
                .map(|draft| (draft.label(), draft.glance(&self.library))),
        )
        .collect();
        let active = self.selected.map_or(0, |index| index + 1);
        let face = theme.clone();
        let this = cx.weak_entity();
        let selector = this.clone();
        let profiles = Tabs::new("profiles", "Combat profiles", rows.len(), move |index| {
            let (title, glance) = &rows[index];
            let this = this.clone();
            Tab::new(title.clone())
                .description(glance.label.clone())
                .child(tab_face(
                    &face,
                    title,
                    Some(glance.clone()),
                    index == active,
                    None,
                ))
                .menu(move |_, _| profile_menu(&this, index.checked_sub(1)))
        })
        .vertical()
        .selected(active)
        .track_scroll(&self.profile_scroll)
        .on_select(move |index, window, cx| {
            selector
                .update(cx, |editor, cx| {
                    editor.select(index.checked_sub(1), window, cx)
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
                h_flex()
                    .flex_none()
                    .gap(px(2.0))
                    .h(BAR_HEIGHT)
                    .pr(px(8.0))
                    .child(drag_area().flex_1())
                    .child(
                        Button::icon("new-profile", Icon::Plus, "New profile")
                            .size(Size::Small)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.add_profile(
                                    Form::of(&Profile::generic()),
                                    "New profile",
                                    window,
                                    cx,
                                )
                            })),
                    ),
            )
            .child(v_flex().min_h_0().child(profiles))
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
    }

    fn header(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let title = |text: SharedString| div().min_w_0().truncate().text_title().child(text);
        let Some(draft) = self.draft() else {
            return v_flex()
                .gap(px(10.0))
                .child(
                    h_flex()
                        .gap(px(8.0))
                        .child(title(GENERIC.into()))
                        .child(Badge::new("Built in")),
                )
                .child(
                    Alert::new(
                        "generic-note",
                        Tone::Neutral,
                        "Built in profiles can't be changed.",
                    )
                    .body("Duplicate it to make your own.")
                    .action(
                        Button::new("duplicate-generic", "Duplicate")
                            .size(Size::Small)
                            .on_click(
                                cx.listener(|this, _, window, cx| this.duplicate(window, cx)),
                            ),
                    ),
                )
                .into_any_element();
        };
        let others: Vec<&str> = self
            .library
            .names()
            .filter(|name| {
                draft
                    .saved
                    .as_deref()
                    .is_none_or(|saved| !same_name(saved, name))
            })
            .collect();
        let note = match check_name(&draft.name, others) {
            Err(problem) => Some((problem, theme.danger_text)),
            Ok(name) => draft
                .saved
                .as_ref()
                .filter(|saved| !same_name(saved, &name))
                .map(|saved| {
                    (
                        format!(
                            "Scripts that use \u{201c}{saved}\u{201d} won't find it after you save."
                        ),
                        theme.warning_text,
                    )
                }),
        };
        let on_cooldown = usize::from(self.form().on_cooldown == OnCooldown::Wait);
        let this = cx.weak_entity();
        let action = |id: &'static str, label: &'static str| {
            Button::new(id, label)
                .size(Size::Small)
                .variant(Variant::Ghost)
        };
        v_flex()
            .gap(px(10.0))
            .child(
                h_flex()
                    .gap(px(4.0))
                    .h(px(30.0))
                    .child(div().flex_1().min_w_0().map(|name| {
                        if self.renaming {
                            name.child(self.name.clone())
                        } else {
                            name.child(title(draft.label()))
                        }
                    }))
                    .child(
                        action("rename-profile", "Rename")
                            .on_click(cx.listener(|this, _, window, cx| this.rename(window, cx))),
                    )
                    .child(
                        action("duplicate-profile", "Duplicate").on_click(
                            cx.listener(|this, _, window, cx| this.duplicate(window, cx)),
                        ),
                    )
                    .child(
                        action("delete-profile-button", "Delete…").on_click(cx.listener(
                            |this, _, _, cx| {
                                this.deleting = true;
                                cx.notify();
                            },
                        )),
                    ),
            )
            .children(note.map(|(note, color)| div().text_small().text_color(color).child(note)))
            .child(
                h_flex()
                    .gap(px(10.0))
                    .child(
                        div()
                            .text_color(theme.text_secondary)
                            .child("When a skill is cooling down"),
                    )
                    .child(
                        ToggleGroup::new(
                            "profile-on-cooldown",
                            "When a skill is cooling down",
                            ["Skip it", "Wait for it"],
                            on_cooldown,
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
                    ),
            )
            .into_any_element()
    }

    fn section(&self, theme: &Theme, section: Section, cx: &mut Context<Self>) -> AnyElement {
        let (title, caption, empty) = match section {
            Section::Triggers => (
                "Triggers",
                "When a message appears, cast its skill once as soon as it's ready. Answered before anything else.",
                "No triggers.",
            ),
            Section::Steps(Lane::Priority) => (
                "Priority",
                "Checked first on every attempt, top to bottom. Never waits for a cooldown.",
                "No priority skills.",
            ),
            Section::Steps(Lane::Rotation) => (
                "Rotation",
                "Cast in turn from where the last cast left off. Starts over on a new target.",
                "No skills in the rotation.",
            ),
        };
        let editable = self.draft().is_some();
        let form = self.form();
        let cards: Rc<[Card]> = match section {
            Section::Triggers => form
                .triggers
                .iter()
                .map(|trigger| Card {
                    slot: u8::from(trigger.skill),
                    summary: trigger.summary().into(),
                    detail: Some(format!("within {} ms", trigger.within.trim()).into()),
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
            div()
                .px(px(12.0))
                .py(px(8.0))
                .rounded(theme.radius.panel)
                .border_1()
                .border_color(theme.hairline)
                .text_small()
                .text_color(theme.text_tertiary)
                .child(empty)
                .into_any_element()
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
        Field::new(title, control)
            .description(caption)
            .when(editable, |field| {
                field.end_slot(self.add_strip(theme, section, cx))
            })
            .into_any_element()
    }

    fn add_strip(
        &self,
        theme: &Theme,
        section: Section,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let label = match section {
            Section::Triggers => "Add a trigger that casts",
            Section::Steps(_) => "Add skill",
        };
        h_flex()
            .gap(px(4.0))
            .child(
                div()
                    .mr(px(2.0))
                    .text_color(theme.text_tertiary)
                    .child("Add"),
            )
            .children(SLOTS.map(|slot| {
                Button::new(
                    SharedString::from(format!("add-{}-{slot}", section.id())),
                    format!("{label} {slot}"),
                )
                .size(Size::XSmall)
                .w(CONTROL_XS)
                .px_0()
                .child(slot.to_string())
                .on_click(
                    cx.listener(move |this, _, window, cx| this.add(section, slot, window, cx)),
                )
            }))
    }

    fn inspector(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let pane = v_flex()
            .id("inspector")
            .flex_none()
            .w(px(INSPECTOR_WIDTH))
            .h_full()
            .gap(px(16.0))
            .p(px(16.0))
            .overflow_y_scroll()
            .border_l_1()
            .border_color(theme.hairline);
        let note = |text: &'static str| {
            div()
                .text_small()
                .text_color(theme.text_tertiary)
                .child(text)
        };
        if self.draft().is_none() {
            return pane.child(note("Generic casts skills 1 to 4 in turn."));
        }
        let (Some(item), Some(texts)) = (self.item, &self.texts) else {
            return pane.child(note("Select a trigger or a skill to choose when it casts."));
        };
        let this = cx.weak_entity();
        let skill = match item {
            Item::Trigger(index) => self.form().triggers.get(index).map(|trigger| trigger.skill),
            Item::Step(lane, index) => self.form().lane(lane).get(index).map(|step| step.skill),
        };
        let Some(skill) = skill else {
            return pane;
        };
        let slot = u8::from(skill);
        let skill_this = this.clone();
        let mut pane = pane.child(
            Field::new(
                match item {
                    Item::Trigger(_) => "Cast",
                    Item::Step(..) => "Skill",
                },
                ToggleGroup::new(
                    "item-skill",
                    "Skill",
                    SLOTS
                        .iter()
                        .map(|slot| SharedString::from(slot.to_string())),
                    usize::from(slot),
                )
                .full_width()
                .on_select(move |index, _, cx| {
                    let _ = skill_this.update(cx, |this, cx| {
                        this.set_skill(u8::try_from(index).unwrap_or(0), cx)
                    });
                }),
            )
            .when_some(
                match slot {
                    0 => Some("Slot 0 is the auto attack."),
                    5 => Some("Slot 5 uses the equipped potion or scroll."),
                    _ => None,
                },
                Field::description,
            ),
        );
        match item {
            Item::Trigger(_) => {
                pane = pane
                    .child(
                        Field::new("When this message appears", div().children(texts.get(Text::Includes)))
                            .description("Matched anywhere in combat and server messages, ignoring case."),
                    )
                    .child(
                        Field::new("Give up after", millis_input(texts, Text::Within))
                            .inline()
                            .description("Casts once, as soon as the skill is ready. Repeats heard before then share that cast."),
                    );
            }
            Item::Step(lane, index) => {
                let Some(step) = self.form().lane(lane).get(index) else {
                    return pane;
                };
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
                    pane = pane.child(
                        Field::new(
                            "When it's cooling down",
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
                        )
                        .description(profile_default),
                    );
                }
                let mut conditions = v_flex().gap(px(8.0));
                for (rule, condition) in step.when.iter().enumerate() {
                    conditions = conditions
                        .child(condition_editor(theme, rule, condition, texts, &this, cx));
                }
                pane = pane
                    .child(
                        Field::new("Cast when all of these hold", conditions)
                            .when(step.when.is_empty(), |field| {
                                field.description("None, so it casts whenever it's ready.")
                            })
                            .end_slot(
                                Button::icon("add-rule", Icon::Plus, "Add condition")
                                    .size(Size::XSmall)
                                    .hide_tooltip()
                                    .on_click(
                                        cx.listener(|this, _, window, cx| {
                                            this.add_rule(window, cx)
                                        }),
                                    ),
                            ),
                    )
                    .child(
                        Field::new("Pause after casting", millis_input(texts, Text::Pause))
                            .inline(),
                    );
            }
        }
        pane.child(
            h_flex()
                .gap(px(8.0))
                .child(
                    Button::new("duplicate-item", "Duplicate")
                        .size(Size::Small)
                        .on_click(
                            cx.listener(|this, _, window, cx| this.duplicate_item(window, cx)),
                        ),
                )
                .child(
                    Button::new("remove-item", "Remove")
                        .size(Size::Small)
                        .variant(Variant::Danger)
                        .on_click(cx.listener(|this, _, window, cx| this.remove_item(window, cx))),
                ),
        )
    }
}

fn profile_menu(this: &WeakEntity<Editor>, entry: Option<usize>) -> Vec<MenuItem> {
    let act = |label: &'static str, run: fn(&mut Editor, &mut Window, &mut Context<Editor>)| {
        let this = this.clone();
        MenuItem::action(label, move |window, cx| {
            this.update(cx, |editor, cx| {
                editor.select(entry, window, cx);
                run(editor, window, cx);
            })
            .ok();
        })
    };
    match entry {
        None => vec![act("Duplicate", Editor::duplicate)],
        Some(_) => vec![
            act("Rename", Editor::rename),
            act("Duplicate", Editor::duplicate),
            MenuItem::separator(),
            act("Delete…", |editor, _, cx| {
                editor.deleting = true;
                cx.notify();
            })
            .danger(),
        ],
    }
}

fn shape(form: &Form) -> String {
    let parts: Vec<String> = [
        (form.triggers.len(), "trigger", "triggers"),
        (form.priority.len(), "priority", "priority"),
        (form.steps.len(), "in turn", "in turn"),
    ]
    .into_iter()
    .filter(|(count, _, _)| *count > 0)
    .map(|(count, one, many)| {
        if count == 1 {
            format!("1 {one}")
        } else {
            format!("{count} {many}")
        }
    })
    .collect();
    if parts.is_empty() {
        "Empty".into()
    } else {
        parts.join(" · ")
    }
}

fn millis_input(texts: &Texts, text: Text) -> impl IntoElement + use<> {
    div().w(px(96.0)).children(texts.get(text))
}

struct Card {
    slot: u8,
    summary: SharedString,
    detail: Option<SharedString>,
}

fn condition_editor(
    theme: &Theme,
    rule: usize,
    condition: &Rule,
    texts: &Texts,
    this: &WeakEntity<Editor>,
    cx: &mut Context<Editor>,
) -> impl IntoElement {
    let toggle = |id: &'static str,
                  label: &'static str,
                  options: &[&'static str],
                  chosen: usize,
                  apply: fn(&mut Rule, usize)| {
        let this = this.clone();
        ToggleGroup::new(
            (id, rule),
            label,
            options.iter().map(|option| SharedString::from(*option)),
            chosen,
        )
        .full_width()
        .on_select(move |index, _, cx| {
            let _ = this.update(cx, |this, cx| {
                this.edit_rule(rule, cx, |rule| apply(rule, index))
            });
        })
    };
    let kind = toggle(
        "rule-kind",
        "Condition",
        &Kind::ALL.map(Kind::label),
        Kind::ALL
            .iter()
            .position(|kind| *kind == condition.kind)
            .unwrap_or(0),
        |rule, index| rule.kind = Kind::ALL[index],
    );
    let comparison = {
        let this = this.clone();
        ToggleGroup::new(
            ("rule-bound", rule),
            "Comparison",
            ["≤", "≥"],
            usize::from(!condition.at_most),
        )
        .on_select(move |index, _, cx| {
            let _ = this.update(cx, |this, cx| {
                this.edit_rule(rule, cx, |rule| rule.at_most = index == 0)
            });
        })
    };
    let bound = |measure: Option<ToggleGroup>| {
        h_flex()
            .gap(px(6.0))
            .children(measure)
            .child(comparison)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .children(texts.get(Text::Value(rule))),
            )
    };
    let measure = {
        let this = this.clone();
        ToggleGroup::new(
            ("rule-measure", rule),
            "Measure",
            ["Stacks", "Value"],
            usize::from(condition.measure == Measure::Value),
        )
        .on_select(move |index, _, cx| {
            let _ = this.update(cx, |this, cx| {
                this.edit_rule(rule, cx, |rule| {
                    rule.measure = if index == 0 {
                        Measure::Stacks
                    } else {
                        Measure::Value
                    }
                })
            });
        })
    };
    let (body, hint) = match condition.kind {
        Kind::Hp => (
            v_flex()
                .gap(px(6.0))
                .child(toggle(
                    "rule-who",
                    "Whose HP",
                    &["Mine", "Any ally's"],
                    usize::from(condition.who == Who::AnyAlly),
                    |rule, index| rule.who = if index == 0 { Who::Me } else { Who::AnyAlly },
                ))
                .child(bound(None)),
            "Points, or a percent like 60%.",
        ),
        Kind::Mp => (
            v_flex().gap(px(6.0)).child(bound(None)),
            "Points, or a percent like 60%.",
        ),
        Kind::Aura => (
            v_flex()
                .gap(px(6.0))
                .children(texts.get(Text::Name(rule)))
                .child(toggle(
                    "rule-holder",
                    "Aura on",
                    &["On me", "On target"],
                    usize::from(condition.holder == Holder::Target),
                    |rule, index| {
                        rule.holder = if index == 0 {
                            Holder::Me
                        } else {
                            Holder::Target
                        }
                    },
                ))
                .child(bound(Some(measure))),
            "A missing aura counts as 0.",
        ),
    };
    v_flex()
        .p(px(8.0))
        .rounded(theme.radius.control)
        .bg(theme.well)
        .border_1()
        .border_color(theme.hairline)
        .child(
            Field::new(
                condition.summary(),
                v_flex().gap(px(6.0)).child(kind).child(body),
            )
            .description(hint)
            .end_slot(
                Button::icon(("remove-rule", rule), Icon::X, "Remove condition")
                    .size(Size::XSmall)
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.remove_rule(rule, window, cx)),
                    ),
            ),
        )
}

impl Render for Editor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let editable = self.draft().is_some();
        let edited = self
            .draft()
            .is_some_and(|draft| draft.edited(&self.library));
        let unsaved: Vec<SharedString> = self.edited().iter().map(|draft| draft.label()).collect();
        let deleting_name = self.draft().map(Draft::label).unwrap_or_default();
        let sidebar = self.sidebar(&theme, cx);
        let header = self.header(&theme, cx);
        let sections: Vec<AnyElement> = Section::ALL
            .into_iter()
            .map(|section| self.section(&theme, section, cx))
            .collect();
        let inspector = self.inspector(&theme, cx);
        div()
            .flex()
            .size_full()
            .bg(theme.base)
            .text_size(TEXT_BODY)
            .text_color(theme.text)
            .key_context(format!("{FOCUS_SCOPE} {CONTEXT}").as_str())
            .id("combat-profiles")
            .track_focus(&self.focus)
            .on_action(cx.listener(|this, _: &SaveProfile, window, cx| this.save(window, cx)))
            .on_action(cx.listener(|this, _: &CloseSession, window, cx| {
                if this.may_close(cx) {
                    window.remove_window();
                }
            }))
            .child(mira_ui::nav::pointer_probe())
            .child(sidebar)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(
                        bar(cx)
                            .pl(px(8.0))
                            .child(bar_title("Combat Profiles", cx))
                            .child(drag_area().flex_1())
                            .child(
                                Button::new("save-profile", "Save")
                                    .variant(Variant::Primary)
                                    .size(Size::Small)
                                    .tooltip("Save (⌘S)")
                                    .when(!editable || !edited, Button::hide_tooltip)
                                    .disabled(!editable || !edited)
                                    .on_click(
                                        cx.listener(|this, _, window, cx| this.save(window, cx)),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_1()
                            .min_h_0()
                            .child(
                                div()
                                    .id("profile")
                                    .flex()
                                    .flex_col()
                                    .flex_1()
                                    .min_w_0()
                                    .h_full()
                                    .gap(px(24.0))
                                    .p(px(20.0))
                                    .overflow_y_scroll()
                                    .child(header)
                                    .children(self.problem.clone().map(|problem| {
                                        Alert::new("save-problem", Tone::Danger, "Not saved")
                                            .body(problem)
                                    }))
                                    .children(sections),
                            )
                            .child(inspector),
                    ),
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
            editor.add_profile(Form::of(&Profile::generic()), "New profile", window, cx);
            editor.select_named("Ultra speaker", window, cx);
            editor.select_item(Some(Item::Step(Lane::Rotation, 2)), window, cx);
        });
        snapshot("profiles-trigger", |editor, window, cx| {
            editor.select_named("Ultra speaker", window, cx);
            editor
                .name
                .update(cx, |input, cx| input.set_text("Daily boss", cx));
            editor.select_item(Some(Item::Trigger(0)), window, cx);
        });
        snapshot("profiles-generic", |_, _, _| {});
    }

    fn open_editor<'a>(
        name: &str,
        cx: &'a mut gpui::TestAppContext,
    ) -> (Entity<Editor>, &'a mut gpui::VisualTestContext) {
        let path = library_file(name);
        cx.update(|cx| {
            mira_ui::init(cx);
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
        gpui::point(px(60.0), BAR_HEIGHT + px(29.0 + 52.0 * index as f32))
    }

    fn shown(editor: &Entity<Editor>, cx: &mut gpui::VisualTestContext) -> Option<String> {
        editor.read_with(cx, |editor, _| {
            editor.draft().map(|draft| draft.name.clone())
        })
    }

    #[gpui::test]
    fn arrow_keys_move_between_profiles(cx: &mut gpui::TestAppContext) {
        let (editor, cx) = open_editor("arrows", cx);
        click(cx, profile_row(1));
        assert_eq!(shown(&editor, cx).as_deref(), Some("Daily boss"));
        press(cx, "down");
        assert_eq!(shown(&editor, cx).as_deref(), Some("Farming"));
        press(cx, "up up");
        assert_eq!(shown(&editor, cx), None);
    }

    #[gpui::test]
    fn the_profile_list_scrolls_to_keep_the_selection_in_view(cx: &mut gpui::TestAppContext) {
        let (editor, cx) = open_editor("reveal", cx);
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                for _ in 0..20 {
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
        click(cx, profile_row(1));
        press(cx, "down shift-f10");
        press(cx, "down enter");
        assert_eq!(shown(&editor, cx).as_deref(), Some("Farming copy"));
        assert!(editor.read_with(cx, |editor, _| editor.renaming));
    }

    #[gpui::test]
    fn steps_take_arrow_keys_and_stay_selected_when_their_profile_is_chosen_again(
        cx: &mut gpui::TestAppContext,
    ) {
        let (editor, cx) = open_editor("steps", cx);
        let item = |cx: &mut gpui::VisualTestContext| editor.read_with(cx, |editor, _| editor.item);
        click(cx, profile_row(3));
        let third = cx
            .debug_bounds("row-2")
            .expect("the rotation lists three steps");
        click(cx, third.center());
        assert_eq!(item(cx), Some(Item::Step(Lane::Rotation, 2)));
        press(cx, "up");
        assert_eq!(item(cx), Some(Item::Step(Lane::Rotation, 1)));
        click(cx, profile_row(3));
        assert_eq!(item(cx), Some(Item::Step(Lane::Rotation, 1)));
    }

    #[gpui::test]
    fn millisecond_fields_take_digits_and_step_by_100(cx: &mut gpui::TestAppContext) {
        let (editor, cx) = open_editor("millis", cx);
        let within = cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.select_named("Ultra speaker", window, cx);
                editor.select_item(Some(Item::Trigger(0)), window, cx);
                editor.texts.as_ref().unwrap().get(Text::Within).unwrap()
            })
        });
        cx.update(|window, cx| window.focus(&within.focus_handle(cx), cx));
        press(cx, "up");
        cx.simulate_input("x");
        cx.run_until_parked();
        let saved = editor.read_with(cx, |editor, _| editor.form().triggers[0].within.clone());
        assert_eq!(saved, "3100");
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
