use std::{fs, path::PathBuf, rc::Rc};

use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, EventEmitter, FocusHandle, Focusable as _,
    FontWeight, InteractiveElement as _, IntoElement, ParentElement as _, Pixels, ScrollHandle,
    SharedString, StatefulInteractiveElement as _, Styled as _, Subscription, Task, Window, div,
    prelude::FluentBuilder as _, px,
};
use mira_ui::{
    StyledExt as _, Theme, Tone, h_flex,
    primitives::{
        Alert, Button, Checkbox, CheckedState, Icon, Input, InputEvent, Select, Switch,
        button::{Size, Variant},
    },
    v_flex,
};

use super::{
    account::file_name,
    form::{muted, note},
    script_form::{Draft, FieldDraft, drafts, error, starting, values},
};
use crate::{
    script_inputs,
    scripting::{self, Declared, Definition, ExtractError, Kind, Meta, Supplied, Values},
};

pub(super) type Read = Result<Declared, (&'static str, String)>;

pub(super) fn read(script: PathBuf, cx: &App) -> Task<Read> {
    cx.background_spawn(async move {
        if scripting::is_skua(&script) {
            let described = match scripting::describe_skua(&script, &|| false) {
                Ok(described) => Ok(described),
                Err(scripting::SkuaHalt::Failed(message)) => Err(message),
                Err(scripting::SkuaHalt::Stopped) => unreachable!("nothing stops this read"),
            };
            return skua_read(described).0;
        }
        let display = file_name(&script);
        let text = fs::read_to_string(&script).map_err(|error| {
            (
                "This script can't be read",
                format!("Couldn't read {display}: {error}"),
            )
        })?;
        scripting::extract(&display, &script, &text).map_err(|error| match error {
            ExtractError::Syntax(message) => ("This script has a syntax error", message),
            ExtractError::Meta(message) => ("This script's meta is invalid", message),
        })
    })
}

pub(super) fn skua_read(
    described: Result<scripting::SkuaDescribed, String>,
) -> (Read, Option<SharedString>) {
    let (inputs, unread) = match described {
        Err(message) => return (Err(("This Skua script can't start", message)), None),
        Ok(scripting::SkuaDescribed::Inputs(definition)) => (definition, None),
        Ok(scripting::SkuaDescribed::Unread(reason)) => (None, Some(reason.into())),
    };
    let declared = Declared {
        meta: Meta::default(),
        inputs: Ok(inputs),
    };
    (Ok(declared), unread)
}

pub(super) enum FormState {
    Broken {
        title: &'static str,
        message: SharedString,
    },
    Plain {
        unread: Option<SharedString>,
    },
    Fields {
        definition: Definition,
        drafts: Vec<FieldDraft>,
        texts: Vec<Option<Entity<Input>>>,
        picks: Vec<Option<FocusHandle>>,
        _changes: Vec<Subscription>,
    },
}

pub(super) struct InputsForm {
    script: Option<PathBuf>,
    pub(super) meta: Meta,
    pub(super) state: FormState,
    saved: Option<Values>,
    touched: bool,
    _store: Option<Subscription>,
}

pub(super) struct Edited(pub(super) Option<Values>);

impl EventEmitter<Edited> for InputsForm {}

pub(super) struct Submitted;

impl EventEmitter<Submitted> for InputsForm {}

impl InputsForm {
    pub(super) fn saved(
        script: PathBuf,
        read: Read,
        unread: Option<SharedString>,
        cx: &mut Context<Self>,
    ) -> Self {
        let saved = saved_values(&script, cx);
        let shown = saved.clone().unwrap_or_default();
        let mut form = Self::new(read, unread, &shown, cx);
        form.script = Some(script);
        form.saved = saved;
        form._store = Some(cx.observe_global::<script_inputs::Store>(Self::sync_saved));
        form
    }

    pub(super) fn queued(
        values: &Values,
        read: Read,
        unread: Option<SharedString>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::new(read, unread, values, cx)
    }

    fn new(
        read: Read,
        unread: Option<SharedString>,
        shown: &Values,
        cx: &mut Context<Self>,
    ) -> Self {
        let (meta, read) = match read {
            Ok(Declared { meta, inputs }) => (
                meta,
                inputs.map_err(|message| ("This script's inputs are invalid", message)),
            ),
            Err(broken) => (Meta::default(), Err(broken)),
        };
        let state = match read {
            Err((title, message)) => FormState::Broken {
                title,
                message: message.into(),
            },
            Ok(None) => FormState::Plain { unread },
            Ok(Some(definition)) => {
                let drafts = drafts(&definition, shown);
                let mut texts = Vec::new();
                let picks = definition
                    .fields()
                    .iter()
                    .map(|field| {
                        matches!(
                            field.kind,
                            Kind::Boolean { .. } | Kind::Select { .. } | Kind::MultiSelect { .. }
                        )
                        .then(|| cx.focus_handle())
                    })
                    .collect();
                let mut changes = Vec::new();
                for (index, (field, draft)) in definition.fields().iter().zip(&drafts).enumerate() {
                    let Draft::Text(text) = &draft.draft else {
                        texts.push(None);
                        continue;
                    };
                    let invalid = error(field, draft).is_some();
                    let label = field.label.clone();
                    let text = text.clone();
                    let input = cx.new(|cx| {
                        let mut input = Input::new(label, cx);
                        input.set_text(text, cx);
                        input.set_invalid(invalid, cx);
                        input
                    });
                    changes.push(cx.subscribe(
                        &input,
                        move |this, input, event: &InputEvent, cx| match event {
                            InputEvent::Changed => {
                                let text = input.read(cx).text().to_owned().into();
                                this.set_draft(index, Draft::Text(text), cx);
                            }
                            InputEvent::Submitted => cx.emit(Submitted),
                        },
                    ));
                    texts.push(Some(input));
                }
                FormState::Fields {
                    definition,
                    drafts,
                    texts,
                    picks,
                    _changes: changes,
                }
            }
        };
        Self {
            script: None,
            meta,
            state,
            saved: None,
            touched: false,
            _store: None,
        }
    }

    pub(super) fn definition(&self) -> Option<&Definition> {
        match &self.state {
            FormState::Fields { definition, .. } => Some(definition),
            FormState::Broken { .. } | FormState::Plain { .. } => None,
        }
    }

    pub(super) fn edited(&self) -> Option<Values> {
        match &self.state {
            FormState::Fields {
                definition, drafts, ..
            } => values(definition, drafts),
            FormState::Broken { .. } | FormState::Plain { .. } => None,
        }
    }

    pub(super) fn ready_values(&self, cx: &App) -> Option<Values> {
        match &self.state {
            FormState::Broken { .. } => None,
            FormState::Plain { unread: None } => Some(Values::new()),
            FormState::Plain { unread: Some(_) } => Some(self.saved.clone().unwrap_or_default()),
            FormState::Fields { .. } if self.unreadable(cx) => None,
            FormState::Fields {
                definition, drafts, ..
            } => values(definition, drafts),
        }
    }

    pub(super) fn errored(&self, cx: &App) -> Vec<usize> {
        match &self.state {
            FormState::Fields {
                definition, drafts, ..
            } if !self.unreadable(cx) => definition
                .fields()
                .iter()
                .zip(drafts)
                .enumerate()
                .filter(|(_, (field, draft))| error(field, draft).is_some())
                .map(|(index, _)| index)
                .collect(),
            _ => Vec::new(),
        }
    }

    pub(super) fn step(
        &self,
        cursor: Option<usize>,
        by: isize,
        cx: &App,
    ) -> Option<(usize, usize)> {
        let errored = self.errored(cx);
        let count = errored.len() as isize;
        if count == 0 {
            return None;
        }
        let next = match cursor {
            Some(at) => (at as isize + by).rem_euclid(count),
            None if by > 0 => 0,
            None => count - 1,
        } as usize;
        Some((next, errored[next]))
    }

    pub(super) fn item_of(&self, field: usize, cx: &App) -> usize {
        let (unreadable, stale) = self.leading(cx);
        usize::from(unreadable) + usize::from(stale) + field
    }

    pub(super) fn focus_handle(&self, field: usize, cx: &App) -> Option<FocusHandle> {
        if let Some(input) = self.text(field) {
            return Some(input.focus_handle(cx));
        }
        match &self.state {
            FormState::Fields { picks, .. } => picks.get(field).cloned().flatten(),
            FormState::Broken { .. } | FormState::Plain { .. } => None,
        }
    }

    pub(super) fn text(&self, field: usize) -> Option<Entity<Input>> {
        match &self.state {
            FormState::Fields { texts, .. } => texts.get(field).cloned().flatten(),
            FormState::Broken { .. } | FormState::Plain { .. } => None,
        }
    }

    pub(super) fn set_draft(&mut self, index: usize, draft: Draft, cx: &mut Context<Self>) {
        let FormState::Fields {
            definition,
            drafts,
            texts,
            ..
        } = &mut self.state
        else {
            return;
        };
        let (Some(field), Some(slot)) = (definition.fields().get(index), drafts.get_mut(index))
        else {
            return;
        };
        if matches!(draft, Draft::Text(_)) && slot.draft == draft {
            return;
        }
        *slot = FieldDraft { draft, stale: None };
        self.touched = true;
        if let Some(Some(input)) = texts.get(index) {
            let invalid = error(field, slot).is_some();
            input.update(cx, |input, cx| input.set_invalid(invalid, cx));
        }
        self.store(cx);
    }

    pub(super) fn reset(&mut self, cx: &mut Context<Self>) {
        let FormState::Fields {
            definition,
            drafts,
            texts,
            ..
        } = &mut self.state
        else {
            return;
        };
        for (field, draft) in definition.fields().iter().zip(drafts.iter_mut()) {
            *draft = FieldDraft {
                draft: starting(field),
                stale: None,
            };
        }
        show_drafts(definition, drafts, texts, cx);
        self.touched = true;
        self.store(cx);
    }

    pub(super) fn holds_edits(&self) -> bool {
        self.touched && self.edited() != self.saved
    }

    fn store(&mut self, cx: &mut Context<Self>) {
        let edited = self.edited();
        if let (Some(script), Some(edited)) = (&self.script, &edited)
            && !script_inputs::unreadable(cx)
        {
            script_inputs::save(script, edited.clone(), cx);
        }
        cx.emit(Edited(edited));
        cx.notify();
    }

    fn sync_saved(&mut self, cx: &mut Context<Self>) {
        let Some(script) = &self.script else {
            return;
        };
        cx.notify();
        let saved = saved_values(script, cx);
        if saved == self.saved {
            return;
        }
        let readable_before = std::mem::replace(&mut self.saved, saved.clone()).is_some();
        let (
            Some(saved),
            FormState::Fields {
                definition,
                drafts: shown,
                texts,
                ..
            },
        ) = (saved, &mut self.state)
        else {
            return;
        };
        let own = values(definition, shown);
        if !readable_before {
            if let Some(own) = own {
                script_inputs::save(script, own, cx);
            }
            return;
        }
        let fresh = drafts(definition, &saved);
        if own.as_ref() == Some(&saved) || fresh == *shown {
            return;
        }
        *shown = fresh;
        show_drafts(definition, shown, texts, cx);
    }

    fn unreadable(&self, cx: &App) -> bool {
        self.script.is_some() && script_inputs::unreadable(cx)
    }

    fn leading(&self, cx: &App) -> (bool, bool) {
        let stale = match &self.state {
            FormState::Fields { drafts, .. } => drafts.iter().any(|draft| draft.stale.is_some()),
            FormState::Broken { .. } | FormState::Plain { .. } => false,
        };
        let store =
            self.script.is_some() && (script_inputs::unreadable(cx) || script_inputs::unsaved(cx));
        (store, stale)
    }
}

fn saved_values(script: &std::path::Path, cx: &App) -> Option<Values> {
    match script_inputs::saved(script, cx) {
        Supplied::Values(values) => Some(values),
        Supplied::StoreUnreadable => None,
    }
}

fn show_drafts(
    definition: &Definition,
    drafts: &[FieldDraft],
    texts: &[Option<Entity<Input>>],
    cx: &mut App,
) {
    for ((field, draft), input) in definition.fields().iter().zip(drafts).zip(texts) {
        if let (Some(input), Draft::Text(text)) = (input, &draft.draft) {
            let invalid = error(field, draft).is_some();
            let text = text.clone();
            input.update(cx, |input, cx| {
                if input.text() != text.as_ref() {
                    input.set_text(text, cx);
                }
                input.set_invalid(invalid, cx);
            });
        }
    }
}

pub(super) fn elements(form: &Entity<InputsForm>, theme: &Theme, cx: &App) -> Vec<AnyElement> {
    let layout = theme.layout;
    let inputs = form.read(cx);
    let (definition, drafts, texts, picks) = match &inputs.state {
        FormState::Broken { title, message } => {
            return vec![
                Alert::new("script-broken", Tone::Danger, *title)
                    .body(message.clone())
                    .into_any_element(),
            ];
        }
        FormState::Plain { unread } => return vec![no_inputs(theme, unread.as_ref())],
        FormState::Fields {
            definition,
            drafts,
            texts,
            picks,
            ..
        } => (definition, drafts, texts, picks),
    };
    let this = form.downgrade();
    let saved = inputs.script.is_some();
    let (store, stale) = inputs.leading(cx);
    let store = store.then(|| {
        if script_inputs::unreadable(cx) {
            Alert::new(
                "inputs-unreadable",
                Tone::Warning,
                "Saved inputs couldn't be read",
            )
            .body("Start fresh to back up the file and use each script's defaults.")
            .action(
                Button::new("inputs-start-fresh", "Start fresh")
                    .size(Size::Small)
                    .on_click(|_, _, cx| script_inputs::start_fresh(cx)),
            )
        } else {
            Alert::new("inputs-unsaved", Tone::Warning, "Inputs couldn't be saved")
                .body("They apply until Mira quits. Your next change tries saving them again.")
        }
    });
    let stale = stale.then(|| {
        let this = this.clone();
        h_flex()
            .justify_between()
            .gap(layout.space(3.0))
            .child(muted(
                theme,
                if saved {
                    "Some saved values don't fit this script anymore."
                } else {
                    "Some queued values don't fit this script anymore."
                },
            ))
            .child(
                Button::new("inputs-reset", "Reset")
                    .size(Size::Small)
                    .on_click(move |_, _, cx| {
                        let _ = this.update(cx, |form, cx| form.reset(cx));
                    }),
            )
    });
    let fields =
        definition
            .fields()
            .iter()
            .zip(drafts)
            .enumerate()
            .map(|(index, (field, draft))| {
                let set = {
                    let this = this.clone();
                    move |draft: Draft, cx: &mut App| {
                        let _ = this.update(cx, |form, cx| form.set_draft(index, draft, cx));
                    }
                };
                let label = SharedString::from(field.label.clone());
                let control = match (&field.kind, &draft.draft, &texts[index]) {
                    (Kind::Boolean { .. }, Draft::Toggle(on), _) => {
                        Switch::new(("input", index), label.clone(), *on)
                            .trailing()
                            .when_some(picks[index].clone(), Switch::focus)
                            .on_toggle(move |on, _, cx| set(Draft::Toggle(on), cx))
                            .into_any_element()
                    }
                    (Kind::Select { options, .. }, Draft::Choice(chosen), _) => Select::new(
                        ("input", index),
                        label.clone(),
                        options.iter().cloned().map(SharedString::from),
                        *chosen,
                    )
                    .placeholder("Choose…")
                    .full_width()
                    .when_some(picks[index].clone(), Select::focus)
                    .invalid(error(field, draft).is_some())
                    .on_select(move |chosen, _, cx| set(Draft::Choice(Some(chosen)), cx))
                    .into_any_element(),
                    (Kind::MultiSelect { options, .. }, Draft::Choices(chosen), _) => {
                        let set = Rc::new(set);
                        v_flex()
                            .gap(layout.space(3.0))
                            .children(options.iter().enumerate().map(|(option, name)| {
                                let set = set.clone();
                                let chosen = chosen.clone();
                                let state = if chosen[option] {
                                    CheckedState::Checked
                                } else {
                                    CheckedState::Unchecked
                                };
                                Checkbox::new(
                                    SharedString::from(format!("input-{index}-{option}")),
                                    name.clone(),
                                    state,
                                )
                                .when_some(
                                    picks[index].as_ref().filter(|_| option == 0).cloned(),
                                    Checkbox::focus,
                                )
                                .on_toggle(move |on, _, cx| {
                                    let mut chosen = chosen.clone();
                                    chosen[option] = on;
                                    set(Draft::Choices(chosen), cx);
                                })
                            }))
                            .into_any_element()
                    }
                    (kind, _, Some(input)) => div()
                        .when(matches!(kind, Kind::Number { .. }), |slot| {
                            slot.flex_none().w(px(80.0))
                        })
                        .child(input.clone())
                        .into_any_element(),
                    _ => div().into_any_element(),
                };
                let control = match field.kind {
                    Kind::Boolean { .. } => control,
                    Kind::Number { .. } => h_flex()
                        .justify_between()
                        .gap(layout.space(2.0))
                        .child(div().min_w_0().font_weight(FontWeight::MEDIUM).child(label))
                        .child(control)
                        .into_any_element(),
                    _ => mira_ui::primitives::Field::new(label, control).into_any_element(),
                };
                let starts_group = field.group.is_some()
                    && definition
                        .fields()
                        .get(index.wrapping_sub(1))
                        .map(|previous| &previous.group)
                        != Some(&field.group);
                v_flex()
                    .gap(layout.space(1.0))
                    .when_some(
                        field.group.clone().filter(|_| starts_group),
                        |column, group| {
                            column.child(
                                div()
                                    .pt(layout.space(2.0))
                                    .text_caption(theme.typography)
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.muted_foreground)
                                    .child(group),
                            )
                        },
                    )
                    .child(control)
                    .map(|column| match (error(field, draft), &field.description) {
                        (Some(problem), _) => {
                            column.child(note(problem, true, theme).text_color(theme.destructive))
                        }
                        (None, Some(description)) => {
                            column.child(note(description.clone(), true, theme))
                        }
                        (None, None) => column,
                    })
            });
    store
        .map(IntoElement::into_any_element)
        .into_iter()
        .chain(stale.map(IntoElement::into_any_element))
        .chain(fields.map(IntoElement::into_any_element))
        .collect()
}

pub(super) fn no_inputs(theme: &Theme, unread: Option<&SharedString>) -> AnyElement {
    match unread {
        None => muted(theme, "This script has no inputs.").into_any_element(),
        Some(reason) => Alert::new(
            "options-unread",
            Tone::Warning,
            "Couldn't read this script's options",
        )
        .body(format!("It runs with its defaults. {reason}"))
        .into_any_element(),
    }
}

pub(super) fn error_steps(
    errors: usize,
    theme: &Theme,
    step: impl Fn(&isize, &mut Window, &mut App) + 'static,
) -> [AnyElement; 3] {
    let label: SharedString = match errors {
        1 => "1 error".into(),
        count => format!("{count} errors").into(),
    };
    let step = Rc::new(step);
    let button = |id: &'static str, glyph: Icon, name: &'static str, by: isize| {
        let step = step.clone();
        Button::icon(id, glyph, name)
            .variant(Variant::Ghost)
            .size(Size::Small)
            .hide_tooltip()
            .on_click(move |_, window, cx| step(&by, window, cx))
            .into_any_element()
    };
    [
        div()
            .pr(theme.space(1.0))
            .child(status_text("input-errors", label))
            .into_any_element(),
        button("previous-error", Icon::ChevronUp, "Previous error", -1),
        button("next-error", Icon::ChevronDown, "Next error", 1),
    ]
}

pub(super) fn reveal_item(
    scroll: &ScrollHandle,
    index: usize,
    above: Pixels,
    below: Pixels,
) -> bool {
    let Some(item) = scroll.bounds_for_item(index) else {
        return false;
    };
    let viewport = scroll.bounds();
    let shown = scroll.offset();
    let mut offset = shown;
    let top = item.top() + offset.y;
    let bottom = item.bottom() + offset.y;
    if top < viewport.top() + above {
        offset.y += viewport.top() + above - top;
    } else if bottom > viewport.bottom() - below {
        offset.y -= (bottom - (viewport.bottom() - below)).min(top - (viewport.top() + above));
    }
    offset.y = offset.y.clamp(-scroll.max_offset().y, px(0.0));
    scroll.set_offset(offset);
    offset != shown
}

pub(super) fn status_text(id: &'static str, text: impl Into<SharedString>) -> impl IntoElement {
    let text = text.into();
    div()
        .id(id)
        .role(gpui::Role::Status)
        .aria_label(text.clone())
        .child(text)
}
