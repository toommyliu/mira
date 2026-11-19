use std::time::Duration;

use gpui::{
    App, Context, Div, Entity, IntoElement, ParentElement as _, Render, SharedString, Styled as _,
    Window, div, prelude::FluentBuilder as _, px,
};
use mira_ui::{
    h_flex,
    primitives::{
        Field, Input, InputEvent, List, ListItem, Select, Toast, Toaster,
        button::{Size, Variant},
    },
    v_flex,
};

use crate::domain::{
    combat::Skill,
    profiles::{
        Amount, Bound, Condition, OnCooldown, PriorityStep, Profile, Step, Text, Trigger, Who,
    },
};

use super::super::{Screen, view::*};

const SKILLS: [&str; 6] = [
    "Auto attack",
    "Skill 1",
    "Skill 2",
    "Skill 3",
    "Skill 4",
    "Consumable",
];
const CONDITIONS: [&str; 4] = ["Always", "My HP ≤ 35%", "Any ally HP ≤ 50%", "Mana ≥ 20%"];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Lane {
    Triggers,
    Priority,
    Rotation,
}

impl Lane {
    fn label(self) -> &'static str {
        match self {
            Self::Triggers => "Triggers",
            Self::Priority => "Priority",
            Self::Rotation => "Rotation",
        }
    }

    fn item(self) -> &'static str {
        match self {
            Self::Triggers => "trigger",
            Self::Priority => "priority rule",
            Self::Rotation => "rotation step",
        }
    }

    fn len(self, profile: &Profile) -> usize {
        match self {
            Self::Triggers => profile.triggers.len(),
            Self::Priority => profile.priority.len(),
            Self::Rotation => profile.steps.len(),
        }
    }
}

struct NamedProfile {
    name: SharedString,
    profile: Profile,
}

pub(in crate::storyboard) struct Profiles {
    profiles: Vec<NamedProfile>,
    active: usize,
    draft: Profile,
    selected: Option<(Lane, usize)>,
    search: Entity<Input>,
    name: Entity<Input>,
    message: Entity<Input>,
    pause: Entity<Input>,
}

fn skill(slot: usize) -> Skill {
    Skill::try_from(slot as u8).expect("valid skill slot")
}

fn condition(preset: usize) -> Option<Condition> {
    match preset {
        1 => Some(Condition::Hp {
            of: Who::Me,
            bound: Bound::AtMost(Amount::Percent(35)),
        }),
        2 => Some(Condition::Hp {
            of: Who::AnyAlly,
            bound: Bound::AtMost(Amount::Percent(50)),
        }),
        3 => Some(Condition::Mp {
            bound: Bound::AtLeast(Amount::Percent(20)),
        }),
        _ => None,
    }
}

fn condition_index(conditions: &[Condition]) -> usize {
    (1..CONDITIONS.len())
        .find(|&preset| conditions.first() == condition(preset).as_ref())
        .unwrap_or(0)
}

impl Profiles {
    pub(in crate::storyboard) fn new(cx: &mut Context<Self>) -> Self {
        let profiles = [
            ("Relic farm", [2, 1, 4], 3, 1),
            ("Boss opener", [4, 2, 1], 3, 3),
            ("Guild support", [1, 2, 4], 2, 2),
            ("Solo sustain", [2, 4, 3], 1, 1),
            ("Daily quests", [1, 3, 4], 2, 1),
        ]
        .map(|(name, slots, priority, when)| {
            let mut profile = Profile::from_skills(&slots.map(skill));
            profile.triggers.push(Trigger {
                includes: Text::try_from("prepares a heavy strike".to_owned()).unwrap(),
                skill: skill(4),
            });
            profile.priority.push(PriorityStep {
                skill: skill(priority),
                when: condition(when).into_iter().collect(),
                pause_after: Duration::ZERO,
            });
            profile.steps[1].when = condition(3).into_iter().collect();
            profile.steps[1].on_cooldown = Some(OnCooldown::Wait);
            profile.steps[1].pause_after = Duration::from_millis(350);
            NamedProfile {
                name: name.into(),
                profile,
            }
        })
        .into();
        let profiles: Vec<NamedProfile> = profiles;
        let search = field("Find a profile", "Find a profile", "", true, cx);
        let name = field("Profile name", "Profile name", &profiles[0].name, false, cx);
        let message = field("Message contains", "Message fragment", "", false, cx);
        let pause = field("Pause after (ms)", "0", "", false, cx);
        observe_input(&search, cx);
        observe_input(&name, cx);
        cx.subscribe(&message, |view, input, event, cx| {
            if matches!(event, InputEvent::Changed)
                && let Some((Lane::Triggers, index)) = view.selected
            {
                let text = Text::try_from(input.read(cx).text().to_owned());
                if let Ok(text) = text {
                    view.draft.triggers[index].includes = text;
                }
                cx.notify();
            }
        })
        .detach();
        cx.subscribe(&pause, |view, input, event, cx| {
            if matches!(event, InputEvent::Changed)
                && let Some((lane, index)) = view.selected
                && lane != Lane::Triggers
            {
                if let Some(pause) = pause_value(input.read(cx).text()) {
                    match lane {
                        Lane::Priority => view.draft.priority[index].pause_after = pause,
                        Lane::Rotation => view.draft.steps[index].pause_after = pause,
                        Lane::Triggers => unreachable!(),
                    }
                }
                cx.notify();
            }
        })
        .detach();
        let mut view = Self {
            draft: profiles[0].profile.clone(),
            profiles,
            active: 0,
            selected: None,
            search,
            name,
            message,
            pause,
        };
        view.select(Some((Lane::Rotation, 1)), cx);
        view
    }

    fn select(&mut self, selection: Option<(Lane, usize)>, cx: &mut Context<Self>) {
        self.selected = selection;
        match selection {
            Some((Lane::Triggers, index)) => {
                let message = self.draft.triggers[index].includes.as_str().to_owned();
                self.message
                    .update(cx, |field, cx| field.set_text(message, cx));
            }
            Some((lane, index)) => {
                let pause = match lane {
                    Lane::Priority => self.draft.priority[index].pause_after,
                    Lane::Rotation => self.draft.steps[index].pause_after,
                    Lane::Triggers => unreachable!(),
                };
                let text = if pause.is_zero() {
                    String::new()
                } else {
                    pause.as_millis().to_string()
                };
                self.pause.update(cx, |field, cx| field.set_text(text, cx));
            }
            None => {}
        }
    }

    fn open(&mut self, index: usize, cx: &mut Context<Self>) {
        self.active = index;
        self.draft = self.profiles[index].profile.clone();
        let name = self.profiles[index].name.clone();
        self.name.update(cx, |input, cx| input.set_text(name, cx));
        self.select(
            (!self.draft.steps.is_empty()).then_some((Lane::Rotation, 0)),
            cx,
        );
    }

    fn editable(&self, cx: &App) -> bool {
        !self.name.read(cx).text().trim().is_empty()
            && match self.selected {
                Some((Lane::Triggers, _)) => !self.message.read(cx).text().trim().is_empty(),
                Some(_) => pause_value(self.pause.read(cx).text()).is_some(),
                None => true,
            }
    }

    fn unsaved(&self, cx: &App) -> bool {
        let saved = &self.profiles[self.active];
        self.name.read(cx).text() != saved.name.as_ref()
            || self.draft != saved.profile
            || !self.editable(cx)
    }

    fn add(&mut self, lane: Lane, cx: &mut Context<Self>) {
        let index = lane.len(&self.draft);
        match lane {
            Lane::Triggers => self.draft.triggers.push(Trigger {
                includes: Text::try_from("prepares a heavy strike".to_owned()).unwrap(),
                skill: skill(1),
            }),
            Lane::Priority => self.draft.priority.push(PriorityStep {
                skill: skill(1),
                when: condition(1).into_iter().collect(),
                pause_after: Duration::ZERO,
            }),
            Lane::Rotation => self.draft.steps.push(Step {
                skill: skill(1),
                when: Vec::new(),
                pause_after: Duration::ZERO,
                on_cooldown: None,
            }),
        }
        self.select(Some((lane, index)), cx);
    }

    fn remove(&mut self, lane: Lane, index: usize, cx: &mut Context<Self>) {
        match lane {
            Lane::Triggers => {
                self.draft.triggers.remove(index);
            }
            Lane::Priority => {
                self.draft.priority.remove(index);
            }
            Lane::Rotation => {
                self.draft.steps.remove(index);
            }
        }
        let remaining = lane.len(&self.draft);
        self.select(
            (remaining > 0).then(|| (lane, index.min(remaining - 1))),
            cx,
        );
    }

    fn library(&self, wide: bool, cx: &Context<Self>) -> Div {
        let query = self.search.read(cx).text().to_lowercase();
        let items: Vec<_> = self
            .profiles
            .iter()
            .enumerate()
            .filter(|(_, profile)| profile.name.to_lowercase().contains(&query))
            .map(|(index, profile)| (index, profile.name.clone()))
            .collect();
        let current = items.iter().position(|(index, _)| *index == self.active);
        let rows = items.clone();
        let choices = List::new("profiles", "Combat profiles", rows.len(), move |index| {
            ListItem::new(rows[index].1.clone())
        })
        .selected(current)
        .fill()
        .on_select(choose(cx, move |view, index: usize, cx| {
            if let Some((profile, _)) = items.get(index)
                && *profile != view.active
            {
                if view.unsaved(cx) {
                    Toaster::push(Toast::new("Save or revert before switching profiles"), cx);
                } else {
                    view.open(*profile, cx);
                }
            }
        }));
        v_flex()
            .flex_none()
            .w(px(if wide { 224.0 } else { 184.0 }))
            .border_r_1()
            .border_color(palette(cx).border)
            .p(px(12.0))
            .gap(px(8.0))
            .child(self.search.clone())
            .child(choices)
            .child(
                command(
                    "new",
                    "New profile",
                    Variant::Outline,
                    change(cx, |view, cx| {
                        if view.unsaved(cx) {
                            Toaster::push(
                                Toast::new("Save or revert before creating a profile"),
                                cx,
                            );
                        } else {
                            view.profiles.push(NamedProfile {
                                name: "Untitled profile".into(),
                                profile: Profile::from_skills(&[]),
                            });
                            view.open(view.profiles.len() - 1, cx);
                        }
                    }),
                )
                .self_start(),
            )
    }

    fn sequence(&self, cx: &Context<Self>) -> Div {
        let mut sequence = v_flex().p(px(16.0)).gap(px(16.0));
        for lane in [Lane::Triggers, Lane::Priority, Lane::Rotation] {
            let rows: Vec<(SharedString, Option<SharedString>)> = match lane {
                Lane::Triggers => self
                    .draft
                    .triggers
                    .iter()
                    .map(|trigger| {
                        (
                            skill_label(trigger.skill).into(),
                            Some(
                                format!("Message contains ‘{}’", trigger.includes.as_str()).into(),
                            ),
                        )
                    })
                    .collect(),
                Lane::Priority => self
                    .draft
                    .priority
                    .iter()
                    .map(|step| {
                        (
                            skill_label(step.skill).into(),
                            step_note(&step.when, step.pause_after, None),
                        )
                    })
                    .collect(),
                Lane::Rotation => self
                    .draft
                    .steps
                    .iter()
                    .map(|step| {
                        (
                            skill_label(step.skill).into(),
                            step_note(&step.when, step.pause_after, step.on_cooldown),
                        )
                    })
                    .collect(),
            };
            let count = rows.len();
            let selected = self
                .selected
                .filter(|(group, _)| *group == lane)
                .map(|(_, index)| index);
            let color = palette(cx).muted_foreground;
            let list = List::new(lane.label(), lane.label(), count, move |index| {
                let (label, detail) = &rows[index];
                let row = ListItem::new(label.clone()).start_slot(
                    div()
                        .w(px(16.0))
                        .text_size(px(11.0))
                        .text_color(color)
                        .child((index + 1).to_string()),
                );
                if let Some(detail) = detail {
                    row.description(detail.clone())
                } else {
                    row
                }
            })
            .on_select(choose(cx, move |view, index, cx| {
                view.select(Some((lane, index)), cx)
            }))
            .selected(selected);
            let owner = cx.weak_entity();
            sequence = sequence.child(
                v_flex()
                    .gap(px(4.0))
                    .child(
                        h_flex()
                            .gap(px(8.0))
                            .min_h(px(28.0))
                            .child(title(lane.label()))
                            .child(spring())
                            .child(
                                command(
                                    ("add", lane as usize),
                                    format!("Add {}", lane.item()),
                                    Variant::Ghost,
                                    change(cx, move |view, cx| view.add(lane, cx)),
                                )
                                .size(Size::Small),
                            ),
                    )
                    .when(lane == Lane::Rotation, |section| {
                        section.child(
                            Select::new(
                                "default-cooldown",
                                "Rotation default on cooldown",
                                ["Skip on cooldown", "Wait on cooldown"],
                                Some(usize::from(self.draft.on_cooldown == OnCooldown::Wait)),
                            )
                            .width(px(144.0))
                            .on_select(choose(
                                cx,
                                |view, choice, _| {
                                    view.draft.on_cooldown = if choice == 0 {
                                        OnCooldown::Skip
                                    } else {
                                        OnCooldown::Wait
                                    };
                                },
                            )),
                        )
                    })
                    .when(count == 0, |section| {
                        section.child(meta(
                            format!("No {} added", lane.label().to_lowercase()),
                            cx,
                        ))
                    })
                    .when(count != 0, |section| {
                        section.child(list.on_reorder(move |from, to, _, cx| {
                            owner
                                .update(cx, |view, cx| {
                                    match lane {
                                        Lane::Triggers => {
                                            move_rule(&mut view.draft.triggers, from, to)
                                        }
                                        Lane::Priority => {
                                            move_rule(&mut view.draft.priority, from, to)
                                        }
                                        Lane::Rotation => {
                                            move_rule(&mut view.draft.steps, from, to)
                                        }
                                    }
                                    view.select(Some((lane, to)), cx);
                                    cx.notify();
                                })
                                .ok();
                        }))
                    }),
            );
        }
        sequence
    }

    fn inspector(&self, cx: &Context<Self>) -> Div {
        let panel = v_flex().p(px(16.0)).gap(px(12.0));
        let Some((lane, index)) = self.selected else {
            return panel.child(meta("Select a trigger or step to edit its rules.", cx));
        };
        let (cast, when) = match lane {
            Lane::Triggers => (self.draft.triggers[index].skill, [].as_slice()),
            Lane::Priority => {
                let step = &self.draft.priority[index];
                (step.skill, step.when.as_slice())
            }
            Lane::Rotation => {
                let step = &self.draft.steps[index];
                (step.skill, step.when.as_slice())
            }
        };
        let panel = panel
            .child(title(format!(
                "{} {}",
                match lane {
                    Lane::Triggers => "Trigger",
                    Lane::Priority => "Priority rule",
                    Lane::Rotation => "Rotation step",
                },
                index + 1
            )))
            .child(Field::new(
                "Skill slot",
                Select::new("skill", "Skill slot", SKILLS, Some(u8::from(cast).into()))
                    .full_width()
                    .on_select(choose(cx, move |view, slot, _| {
                        let skill = skill(slot);
                        match lane {
                            Lane::Triggers => view.draft.triggers[index].skill = skill,
                            Lane::Priority => view.draft.priority[index].skill = skill,
                            Lane::Rotation => view.draft.steps[index].skill = skill,
                        }
                    })),
            ));
        let panel = if lane == Lane::Triggers {
            panel.child(
                Field::new("Message contains", self.message.clone())
                    .description("Matches combat and server messages, ignoring case.")
                    .error(
                        self.message
                            .read(cx)
                            .text()
                            .trim()
                            .is_empty()
                            .then(|| "Enter a message fragment.".into()),
                    ),
            )
        } else {
            panel
                .child(Field::new(
                    "Use when",
                    Select::new(
                        "condition",
                        "Use when",
                        CONDITIONS,
                        Some(condition_index(when)),
                    )
                    .full_width()
                    .on_select(choose(cx, move |view, choice, _| {
                        let conditions = condition(choice).into_iter().collect();
                        match lane {
                            Lane::Priority => view.draft.priority[index].when = conditions,
                            Lane::Rotation => view.draft.steps[index].when = conditions,
                            Lane::Triggers => unreachable!(),
                        }
                    })),
                ))
                .when(lane == Lane::Rotation, |panel| {
                    let current = match self.draft.steps[index].on_cooldown {
                        None => 0,
                        Some(OnCooldown::Skip) => 1,
                        Some(OnCooldown::Wait) => 2,
                    };
                    panel.child(Field::new(
                        "On cooldown",
                        Select::new(
                            "override",
                            "On cooldown",
                            ["Use rotation default", "Skip step", "Wait for skill"],
                            Some(current),
                        )
                        .full_width()
                        .on_select(choose(cx, move |view, choice, _| {
                            view.draft.steps[index].on_cooldown = match choice {
                                1 => Some(OnCooldown::Skip),
                                2 => Some(OnCooldown::Wait),
                                _ => None,
                            };
                        })),
                    ))
                })
                .child(
                    Field::new("Pause after (ms)", self.pause.clone()).error(
                        pause_value(self.pause.read(cx).text())
                            .is_none()
                            .then(|| "Enter a whole number of milliseconds.".into()),
                    ),
                )
        };
        panel.child(
            command(
                "remove-rule",
                format!("Remove {}", lane.item()),
                Variant::Ghost,
                change(cx, move |view, cx| view.remove(lane, index, cx)),
            )
            .size(Size::Small)
            .self_start(),
        )
    }
}

fn skill_label(skill: Skill) -> &'static str {
    SKILLS[usize::from(u8::from(skill))]
}

fn pause_value(text: &str) -> Option<Duration> {
    if text.trim().is_empty() {
        Some(Duration::ZERO)
    } else {
        text.trim()
            .parse::<u32>()
            .ok()
            .map(|ms| Duration::from_millis(ms.into()))
    }
}

fn step_note(
    when: &[Condition],
    pause: Duration,
    cooldown: Option<OnCooldown>,
) -> Option<SharedString> {
    let mut details = Vec::new();
    if !when.is_empty() {
        details.push(CONDITIONS[condition_index(when)].to_owned());
    }
    if let Some(cooldown) = cooldown {
        details.push(
            match cooldown {
                OnCooldown::Skip => "Skip on cooldown",
                OnCooldown::Wait => "Wait on cooldown",
            }
            .to_owned(),
        );
    }
    if !pause.is_zero() {
        details.push(format!("Pause {} ms", pause.as_millis()));
    }
    (!details.is_empty()).then(|| details.join(" · ").into())
}

fn move_rule<T>(items: &mut Vec<T>, from: usize, to: usize) {
    let item = items.remove(from);
    items.insert(to, item);
}

impl Render for Profiles {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let wide = window_width(Screen::Profiles, window) >= px(880.0);
        let preview = StatusPreview::current(cx);
        let dirty = self.unsaved(cx);
        let editor = v_flex().flex_1().min_w_0().min_h_0().child(
            v_flex()
                .flex_none()
                .px(px(16.0))
                .py(px(12.0))
                .gap(px(8.0))
                .border_b_1()
                .border_color(palette(cx).border)
                .child(
                    h_flex()
                        .items_end()
                        .gap(px(8.0))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .max_w(px(260.0))
                                .child(Field::new("Profile name", self.name.clone())),
                        )
                        .child(spring())
                        .child(
                            command(
                                "revert",
                                "Revert",
                                Variant::Ghost,
                                change(cx, |view, cx| view.open(view.active, cx)),
                            )
                            .disabled(!dirty || preview == StatusPreview::Working),
                        )
                        .child(
                            command(
                                "save",
                                "Save",
                                Variant::Primary,
                                change(cx, |view, cx| {
                                    let name: SharedString =
                                        view.name.read(cx).text().trim().to_owned().into();
                                    view.profiles[view.active] = NamedProfile {
                                        name: name.clone(),
                                        profile: view.draft.clone(),
                                    };
                                    view.name
                                        .update(cx, |input, cx| input.set_text(name.clone(), cx));
                                    Toaster::push(Toast::new("Profile saved").body(name), cx);
                                }),
                            )
                            .disabled(
                                preview == StatusPreview::Working
                                    || (preview == StatusPreview::Current && !dirty)
                                    || !self.editable(cx),
                            ),
                        ),
                )
                .children(status_preview(
                    "Saving profile…",
                    "Couldn't save profile",
                    "The profiles folder is read-only. Your edits are still here.",
                    cx,
                ))
                .when(dirty && preview == StatusPreview::Current, |bar| {
                    bar.child(status("Unsaved changes", true, cx))
                }),
        );
        let body = if wide {
            h_flex()
                .items_stretch()
                .flex_1()
                .min_h_0()
                .child(scroll("sequence").min_w_0().child(self.sequence(cx)))
                .child(
                    v_flex()
                        .flex_none()
                        .w(px(280.0))
                        .border_l_1()
                        .border_color(palette(cx).border)
                        .child(scroll("rule-inspector").child(self.inspector(cx))),
                )
                .into_any_element()
        } else {
            scroll("sequence")
                .min_w_0()
                .child(self.sequence(cx))
                .child(divider(cx))
                .child(self.inspector(cx))
                .into_any_element()
        };
        frame(tool_chrome(Screen::Profiles, None, cx), cx).child(
            h_flex()
                .items_stretch()
                .flex_1()
                .min_h_0()
                .child(self.library(wide, cx))
                .child(editor.child(body)),
        )
    }
}
