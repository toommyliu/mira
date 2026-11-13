use std::sync::{Arc, LazyLock};

use gpui::{
    AnyElement, App, AppContext as _, Bounds, Context, Div, ElementId, Entity, FocusHandle,
    Focusable as _, FontWeight, Global, InteractiveElement as _, IntoElement, KeyBinding,
    ParentElement as _, Pixels, Render, Role, SharedString, Stateful,
    StatefulInteractiveElement as _, Styled as _, Subscription, TitlebarOptions, WeakEntity,
    Window, WindowBounds, WindowHandle, WindowOptions, actions, div, prelude::FluentBuilder as _,
    px, size,
};
use mira_ui::{
    ActiveTheme as _, FOCUS_SCOPE, PointerCursor as _, Theme, Tone,
    primitives::{
        Alert, Badge, Button, ContextMenu, Dialog, Flow, Icon, Input, InputEvent, MenuItem, Pose,
        Sortable, ToggleGroup,
        button::{Size, Variant},
    },
    theme::{LINE_SMALL, TEXT_BODY, TEXT_CAPTION, TEXT_SMALL, TEXT_TITLE, tabular},
};

use super::{
    BAR_HEIGHT,
    account::Glance,
    client::{
        CloseSession, SIDEBAR_WIDTH, TAB_BOTTOM, TAB_GAP, TAB_PADDING, TAB_ROW, TAB_TOP, drag_area,
        tab_face,
    },
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Includes,
    Within,
    Pause,
    Value(usize),
    Name(usize),
}

struct Fields {
    inputs: Vec<(Field, Entity<Input>)>,
    _subscriptions: Vec<Subscription>,
}

impl Fields {
    fn get(&self, field: Field) -> Option<Entity<Input>> {
        self.inputs
            .iter()
            .find(|(at, _)| *at == field)
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
    fields: Option<Fields>,
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
            fields: None,
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
        self.selected = selected;
        self.item = None;
        self.problem = None;
        self.renaming = false;
        let name = self
            .draft()
            .map(|draft| draft.name.clone())
            .unwrap_or_default();
        self.name.update(cx, |input, cx| input.set_text(name, cx));
        self.rebuild_fields(window, cx);
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
        self.rebuild_fields(window, cx);
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

    fn rebuild_fields(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let texts: Vec<(Field, &'static str, String)> = match self.item {
            None => Vec::new(),
            Some(Item::Trigger(index)) => match self.form().triggers.get(index) {
                Some(trigger) => vec![
                    (Field::Includes, "Message text", trigger.includes.clone()),
                    (
                        Field::Within,
                        "Time limit in milliseconds",
                        trigger.within.clone(),
                    ),
                ],
                None => Vec::new(),
            },
            Some(Item::Step(lane, index)) => match self.form().lane(lane).get(index) {
                Some(step) => std::iter::once((
                    Field::Pause,
                    "Pause after casting in milliseconds",
                    step.pause.clone(),
                ))
                .chain(step.when.iter().enumerate().flat_map(|(rule, condition)| {
                    [
                        (Field::Value(rule), "Value", condition.value.clone()),
                        (Field::Name(rule), "Aura name", condition.name.clone()),
                    ]
                }))
                .collect(),
                None => Vec::new(),
            },
        };
        if texts.is_empty() {
            self.item = None;
            self.fields = None;
            return;
        }
        let mut subscriptions = Vec::new();
        let inputs = texts
            .into_iter()
            .map(|(field, label, text)| {
                let input = cx.new(|cx| {
                    let mut input = Input::new(label, cx);
                    input.set_text(text, cx);
                    input
                });
                subscriptions.push(cx.subscribe_in(
                    &input,
                    window,
                    move |this, input, event, _, cx| {
                        if let InputEvent::Changed = event {
                            let text = input.read(cx).text().to_owned();
                            this.set_field(field, text, cx);
                        }
                    },
                ));
                (field, input)
            })
            .collect();
        self.fields = Some(Fields {
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

    fn set_field(&mut self, field: Field, text: String, cx: &mut Context<Self>) {
        match field {
            Field::Includes => self.edit_trigger(cx, |trigger| trigger.includes = text),
            Field::Within => self.edit_trigger(cx, |trigger| trigger.within = text),
            Field::Pause => self.edit_step(cx, |step| step.pause = text),
            Field::Value(rule) => self.edit_rule(rule, cx, |rule| rule.value = text),
            Field::Name(rule) => self.edit_rule(rule, cx, |rule| rule.name = text),
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
                .fields
                .as_ref()
                .and_then(|fields| fields.get(Field::Includes))
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
        self.rebuild_fields(window, cx);
    }

    fn remove_rule(&mut self, rule: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.edit_step(cx, |step| {
            if rule < step.when.len() {
                step.when.remove(rule);
            }
        });
        self.rebuild_fields(window, cx);
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
        let rows: Vec<AnyElement> = std::iter::once(None)
            .chain((0..self.drafts.len()).map(Some))
            .map(|entry| self.profile_row(theme, entry, cx))
            .collect();
        let issues = combat_profiles::issues(cx).len();
        div()
            .flex()
            .flex_col()
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
                    .flex()
                    .flex_none()
                    .items_center()
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
            .child(
                div()
                    .id("profile-rows")
                    .flex()
                    .flex_col()
                    .gap(TAB_GAP)
                    .px(px(8.0))
                    .py(px(4.0))
                    .overflow_y_scroll()
                    .role(Role::TabList)
                    .aria_label("Combat profiles")
                    .children(rows),
            )
            .child(drag_area().flex_1())
            .when(issues > 0, |sidebar| {
                sidebar.child(
                    div()
                        .flex_none()
                        .p(px(12.0))
                        .text_size(TEXT_CAPTION)
                        .line_height(LINE_SMALL)
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

    fn profile_row(
        &self,
        theme: &Theme,
        entry: Option<usize>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let active = self.selected == entry;
        let key = entry.map_or(0, |index| index + 1);
        let (title, glance) = match entry {
            None => (
                SharedString::from(GENERIC),
                Glance {
                    label: "Built in".into(),
                    tone: Tone::Neutral,
                },
            ),
            Some(index) => {
                let draft = &self.drafts[index];
                let glance = match (&draft.saved, draft.edited(&self.library)) {
                    (None, _) => Glance {
                        label: "Not saved yet".into(),
                        tone: Tone::Accent,
                    },
                    (Some(_), true) => Glance {
                        label: "Unsaved changes".into(),
                        tone: Tone::Warning,
                    },
                    (Some(_), false) => Glance {
                        label: shape(&draft.form).into(),
                        tone: Tone::Neutral,
                    },
                };
                (draft.label(), glance)
            }
        };
        let row = div()
            .id(("profile-row", key))
            .relative()
            .flex()
            .flex_none()
            .h(TAB_ROW)
            .px(TAB_PADDING)
            .pt(TAB_TOP)
            .pb(TAB_BOTTOM)
            .rounded(theme.radius.item)
            .role(Role::Tab)
            .aria_label(title.clone())
            .aria_selected(active)
            .child(tab_face(theme, &title, Some(glance), active, None))
            .map(|row| {
                if active {
                    row.bg(theme.selected).text_color(theme.text)
                } else {
                    let (hover, pressed) = (theme.fill_hover, theme.fill_pressed);
                    row.pointer_cursor(theme)
                        .hover(move |style| style.bg(hover))
                        .active(move |style| style.bg(pressed))
                        .text_color(theme.text_secondary)
                }
            })
            .on_click(cx.listener(move |this, _, window, cx| this.select(entry, window, cx)));
        let this = cx.weak_entity();
        ContextMenu::new(("profile-menu", key), title)
            .items(move |_, _| {
                let act =
                    |label: &'static str,
                     run: fn(&mut Editor, &mut Window, &mut Context<Editor>)| {
                        let this = this.clone();
                        MenuItem::action(label, move |window, cx| {
                            let _ = this.update(cx, |editor, cx| {
                                editor.select(entry, window, cx);
                                run(editor, window, cx);
                            });
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
            })
            .child(row)
            .into_any_element()
    }

    fn header(&self, theme: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let title = |text: SharedString| {
            div()
                .min_w_0()
                .truncate()
                .text_size(TEXT_TITLE)
                .font_weight(FontWeight::SEMIBOLD)
                .child(text)
        };
        let Some(draft) = self.draft() else {
            return div()
                .flex()
                .flex_col()
                .gap(px(10.0))
                .child(
                    div()
                        .flex()
                        .items_center()
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
        let name: AnyElement = if self.renaming {
            div()
                .flex_1()
                .min_w_0()
                .child(self.name.clone())
                .into_any_element()
        } else {
            div()
                .id("profile-title")
                .flex_1()
                .min_w_0()
                .flex()
                .pointer_cursor(theme)
                .role(Role::Button)
                .aria_label("Rename profile")
                .on_click(cx.listener(|this, _, window, cx| this.rename(window, cx)))
                .child(title(draft.label()))
                .into_any_element()
        };
        let on_cooldown = usize::from(self.form().on_cooldown == OnCooldown::Wait);
        let this = cx.weak_entity();
        div()
            .flex()
            .flex_col()
            .gap(px(10.0))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(4.0))
                    .h(px(30.0))
                    .child(name)
                    .child(
                        Button::new("rename-profile", "Rename")
                            .size(Size::Small)
                            .variant(Variant::Ghost)
                            .on_click(cx.listener(|this, _, window, cx| this.rename(window, cx))),
                    )
                    .child(
                        Button::new("duplicate-profile", "Duplicate")
                            .size(Size::Small)
                            .variant(Variant::Ghost)
                            .on_click(
                                cx.listener(|this, _, window, cx| this.duplicate(window, cx)),
                            ),
                    )
                    .child(
                        Button::new("delete-profile-button", "Delete…")
                            .size(Size::Small)
                            .variant(Variant::Ghost)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.deleting = true;
                                cx.notify();
                            })),
                    ),
            )
            .children(note.map(|(note, color)| {
                div()
                    .text_size(TEXT_SMALL)
                    .line_height(LINE_SMALL)
                    .text_color(color)
                    .child(note)
            }))
            .child(
                div()
                    .flex()
                    .items_center()
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
        let cards: Vec<Card> = match section {
            Section::Triggers => form
                .triggers
                .iter()
                .map(|trigger| Card {
                    slot: u8::from(trigger.skill),
                    summary: trigger.summary().into(),
                    tags: vec![format!("within {} ms", trigger.within.trim()).into()],
                })
                .collect(),
            Section::Steps(lane) => form
                .lane(lane)
                .iter()
                .map(|step| {
                    let mut tags: Vec<SharedString> = Vec::new();
                    match step.on_cooldown {
                        Some(OnCooldown::Wait) => tags.push("waits".into()),
                        Some(OnCooldown::Skip) => tags.push("skips".into()),
                        None => {}
                    }
                    if !step.pause.trim().is_empty() {
                        tags.push(format!("pause {} ms", step.pause.trim()).into());
                    }
                    Card {
                        slot: u8::from(step.skill),
                        summary: step.summary().into(),
                        tags,
                    }
                })
                .collect(),
        };
        let count = cards.len();
        let selected = self
            .item
            .map(section_of)
            .filter(|(at, _)| *at == section)
            .map(|(_, index)| index);
        let id = section.id();
        let entity = cx.weak_entity();
        let row_theme = theme.clone();
        let list = Sortable::new(
            div().id(id).flex().flex_col().gap(px(6.0)),
            Flow::Column,
            (0..count).map(move |index| SharedString::from(format!("{id}-{index}"))),
            move |(index, pose), _, _| {
                card(
                    &row_theme,
                    &cards[index],
                    (id, index),
                    selected == Some(index),
                    pose,
                )
                .when(editable, |row| {
                    let entity = entity.clone();
                    row.pointer_cursor(&row_theme)
                        .on_click(move |_, window, cx| {
                            let _ = entity.update(cx, |this, cx| {
                                this.select_item(Some(section.item(index)), window, cx)
                            });
                        })
                })
                .into_any_element()
            },
            {
                let entity = cx.weak_entity();
                move |from, to, window, cx| {
                    let _ =
                        entity.update(cx, |this, cx| this.reorder(section, from, to, window, cx));
                }
            },
        );
        div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
                    .child(
                        div()
                            .text_size(TEXT_SMALL)
                            .line_height(LINE_SMALL)
                            .text_color(theme.text_tertiary)
                            .child(caption),
                    ),
            )
            .child(if count == 0 {
                div()
                    .text_size(TEXT_SMALL)
                    .text_color(theme.text_tertiary)
                    .child(empty)
                    .into_any_element()
            } else {
                list.into_any_element()
            })
            .when(editable, |block| {
                block.child(self.add_strip(theme, section, cx))
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
        let (hover, pressed) = (theme.fill_hover, theme.fill_pressed);
        div()
            .flex()
            .items_center()
            .gap(px(6.0))
            .child(
                div()
                    .text_size(TEXT_SMALL)
                    .text_color(theme.text_tertiary)
                    .mr(px(2.0))
                    .child(label),
            )
            .children(SLOTS.map(|slot| {
                keycap(theme, slot, px(24.0))
                    .id(SharedString::from(format!("add-{}-{slot}", section.id())))
                    .role(Role::Button)
                    .aria_label(format!("{label} {slot}"))
                    .pointer_cursor(theme)
                    .hover(move |keycap| keycap.bg(hover))
                    .active(move |keycap| keycap.bg(pressed))
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.add(section, slot, window, cx)),
                    )
            }))
    }

    fn inspector(&self, theme: &Theme, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let pane = div()
            .id("inspector")
            .flex()
            .flex_col()
            .flex_none()
            .w(px(INSPECTOR_WIDTH))
            .h_full()
            .gap(px(16.0))
            .p(px(16.0))
            .overflow_y_scroll()
            .border_l_1()
            .border_color(theme.hairline);
        if self.draft().is_none() {
            return pane.child(caption(theme, "Generic casts skills 1 to 4 in turn."));
        }
        let (Some(item), Some(fields)) = (self.item, &self.fields) else {
            return pane.child(caption(
                theme,
                "Select a trigger or a skill to choose when it casts.",
            ));
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
        let mut pane = pane.child(field(
            theme,
            match item {
                Item::Trigger(_) => "Cast",
                Item::Step(..) => "Skill",
            },
            div()
                .flex()
                .flex_col()
                .gap(px(4.0))
                .child(
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
                .children(match slot {
                    0 => Some(caption(theme, "Slot 0 is the auto attack.")),
                    5 => Some(caption(theme, "Slot 5 uses the equipped potion or scroll.")),
                    _ => None,
                }),
        ));
        match item {
            Item::Trigger(_) => {
                pane = pane
                    .child(field(
                        theme,
                        "When this message appears",
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.0))
                            .children(fields.get(Field::Includes))
                            .child(caption(
                                theme,
                                "Matched anywhere in combat and server messages, ignoring case.",
                            )),
                    ))
                    .child(field(
                        theme,
                        "Give up after, in milliseconds",
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.0))
                            .children(fields.get(Field::Within))
                            .child(caption(
                                theme,
                                "Casts once, as soon as the skill is ready. Repeats heard before then share that cast.",
                            )),
                    ));
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
                    pane = pane.child(field(
                        theme,
                        "When it's cooling down",
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.0))
                            .child(
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
                            .child(caption(theme, profile_default)),
                    ));
                }
                let mut conditions = div().flex().flex_col().gap(px(8.0));
                if step.when.is_empty() {
                    conditions =
                        conditions.child(caption(theme, "None, so it casts whenever it's ready."));
                }
                for (rule, condition) in step.when.iter().enumerate() {
                    conditions =
                        conditions.child(rule_card(theme, rule, condition, fields, &this, cx));
                }
                pane = pane
                    .child(field(
                        theme,
                        "Cast when all of these hold",
                        conditions.child(
                            Button::new("add-rule", "Add condition")
                                .size(Size::Small)
                                .on_click(
                                    cx.listener(|this, _, window, cx| this.add_rule(window, cx)),
                                ),
                        ),
                    ))
                    .child(field(
                        theme,
                        "Pause after casting, in milliseconds",
                        div().children(fields.get(Field::Pause)),
                    ));
            }
        }
        pane.child(
            div()
                .flex()
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

struct Card {
    slot: u8,
    summary: SharedString,
    tags: Vec<SharedString>,
}

fn keycap(theme: &Theme, slot: u8, side: Pixels) -> Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(side)
        .rounded(theme.radius.item)
        .bg(theme.surface)
        .border_1()
        .border_color(theme.border_strong)
        .shadow(theme.edge())
        .font_weight(FontWeight::SEMIBOLD)
        .font_features(tabular())
        .text_color(theme.text)
        .child(slot.to_string())
}

fn card(
    theme: &Theme,
    card: &Card,
    id: (&'static str, usize),
    selected: bool,
    pose: Pose,
) -> Stateful<Div> {
    div()
        .id(ElementId::from((SharedString::from(id.0), id.1)))
        .role(Role::Button)
        .aria_label(card.summary.clone())
        .flex()
        .items_center()
        .gap(px(10.0))
        .px(px(10.0))
        .py(px(8.0))
        .rounded(theme.radius.control)
        .border_1()
        .border_color(if selected { theme.focus } else { theme.border })
        .bg(match (selected, pose) {
            (_, Pose::Lifted) => theme.overlay,
            (true, _) => theme.selected,
            (false, _) => theme.surface,
        })
        .child(keycap(theme, card.slot, px(28.0)))
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .gap(px(4.0))
                .child(div().truncate().child(card.summary.clone()))
                .when(!card.tags.is_empty(), |text| {
                    text.child(
                        div()
                            .flex()
                            .gap(px(4.0))
                            .children(card.tags.iter().map(|tag| Badge::new(tag.clone()))),
                    )
                }),
        )
}

fn caption(theme: &Theme, text: &'static str) -> Div {
    div()
        .text_size(TEXT_CAPTION)
        .line_height(LINE_SMALL)
        .text_color(theme.text_tertiary)
        .child(text)
}

fn field(theme: &Theme, label: &'static str, control: impl IntoElement) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(
            div()
                .text_size(TEXT_SMALL)
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.text_secondary)
                .child(label),
        )
        .child(control)
}

fn rule_card(
    theme: &Theme,
    rule: usize,
    condition: &Rule,
    fields: &Fields,
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
    let bound = div()
        .flex()
        .items_center()
        .gap(px(6.0))
        .child(comparison)
        .child(div().flex_1().children(fields.get(Field::Value(rule))));
    let body = match condition.kind {
        Kind::Hp => div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(toggle(
                "rule-who",
                "Whose HP",
                &["Mine", "Any ally's"],
                usize::from(condition.who == Who::AnyAlly),
                |rule, index| rule.who = if index == 0 { Who::Me } else { Who::AnyAlly },
            ))
            .child(bound)
            .child(caption(theme, "Points, or a percent like 60%.")),
        Kind::Mp => div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(bound)
            .child(caption(theme, "Points, or a percent like 60%.")),
        Kind::Aura => div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .children(fields.get(Field::Name(rule)))
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
            .child(toggle(
                "rule-measure",
                "Measure",
                &["Stacks", "Value"],
                usize::from(condition.measure == Measure::Value),
                |rule, index| {
                    rule.measure = if index == 0 {
                        Measure::Stacks
                    } else {
                        Measure::Value
                    }
                },
            ))
            .child(bound)
            .child(caption(theme, "A missing aura counts as 0.")),
    };
    div()
        .flex()
        .flex_col()
        .gap(px(8.0))
        .p(px(10.0))
        .rounded(theme.radius.control)
        .bg(theme.well)
        .border_1()
        .border_color(theme.hairline)
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .child(div().flex_1().child(kind))
                .child(
                    Button::icon(("remove-rule", rule), Icon::X, "Remove condition")
                        .size(Size::Small)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.remove_rule(rule, window, cx)
                        })),
                ),
        )
        .child(body)
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
                        div()
                            .flex()
                            .flex_none()
                            .items_center()
                            .gap(px(8.0))
                            .h(BAR_HEIGHT)
                            .pl(px(16.0))
                            .pr(px(8.0))
                            .border_b_1()
                            .border_color(theme.hairline)
                            .child(
                                drag_area()
                                    .flex_1()
                                    .flex()
                                    .items_center()
                                    .text_color(theme.text_secondary)
                                    .child("Combat Profiles"),
                            )
                            .child(
                                Button::new("save-profile", "Save")
                                    .variant(Variant::Primary)
                                    .size(Size::Small)
                                    .tooltip("Save (⌘S)")
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
    use super::*;

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
