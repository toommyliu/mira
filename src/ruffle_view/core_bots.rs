use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use gpui::{
    AnyElement, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, ScrollHandle, SharedString, StatefulInteractiveElement as _,
    Styled as _, Subscription, Task, Window, div, linear_color_stop, linear_gradient,
    prelude::FluentBuilder as _, px,
};
use mira_ui::{
    ActiveTheme as _, Theme, Tone,
    components::{Empty, Group},
    h_flex,
    primitives::{Alert, Input, InputEvent, Numeric, Scrollbar, Select, Switch},
    v_flex,
};

use super::{RuffleView, form::note};
use crate::{
    accounts::AccountId,
    core_bots::{self, ClassModes, Field, Gear, Kind, Options, Role},
    domain::{grabber::Job, inventory::Item},
    ruffle::Command,
};

const NONE: &str = "None";
const PRECEDENCE: &str = "Skua scripts read these when they start. Where one covers the same thing as Mira's script options, this one wins.";
const WIDE: gpui::Pixels = px(200.0);
const NARROW: gpui::Pixels = px(96.0);

#[derive(Debug, Default, PartialEq)]
pub(super) struct Wardrobe {
    classes: Vec<SharedString>,
    gear: HashMap<&'static str, Vec<SharedString>>,
}

impl Wardrobe {
    #[cfg(test)]
    pub(super) fn fake(classes: &[&str], gear: &[(Gear, &[&str])]) -> Self {
        let names = |names: &[&str]| {
            names
                .iter()
                .map(|name| SharedString::from(name.to_string()))
                .collect()
        };
        Self {
            classes: names(classes),
            gear: gear
                .iter()
                .map(|(gear, owned)| (gear.slot(), names(owned)))
                .collect(),
        }
    }

    pub(super) fn of(items: &[Item]) -> Self {
        let names = |slot: &str| {
            let mut names: Vec<SharedString> = items
                .iter()
                .filter(|item| item.slot() == slot)
                .map(|item| SharedString::from(item.name().to_owned()))
                .collect();
            names.sort_by_key(|name| name.to_lowercase());
            names.dedup();
            names
        };
        Self {
            classes: names(core_bots::CLASS_SLOT),
            gear: Gear::ALL
                .into_iter()
                .map(|gear| (gear.slot(), names(gear.slot())))
                .collect(),
        }
    }
}

enum Reading {
    Waiting { _task: Task<()> },
    Done(Option<Wardrobe>),
}

pub(super) struct CoreBotsPage {
    skua: PathBuf,
    account: Option<AccountId>,
    options: Options,
    problem: Option<SharedString>,
    wardrobe: Reading,
    modes: Option<ClassModes>,
    inputs: Vec<(&'static Field, Entity<Input>)>,
    scroll: ScrollHandle,
    _reading_modes: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl CoreBotsPage {
    pub(super) fn new(skua: PathBuf, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut inputs = Vec::new();
        let mut subscriptions = Vec::new();
        for (_, fields) in core_bots::SECTIONS {
            for field in fields {
                let input = match field.kind {
                    Kind::Switch(_) => continue,
                    Kind::Number(default) => cx.new(|cx| {
                        Input::new(field.label, cx)
                            .placeholder(default.to_string())
                            .numeric(Numeric {
                                min: 0,
                                max: i32::MAX.into(),
                                step: 1,
                            })
                    }),
                    Kind::Text(default) => {
                        cx.new(|cx| Input::new(field.label, cx).placeholder(default))
                    }
                };
                subscriptions.push(cx.subscribe_in(
                    &input,
                    window,
                    move |this, input, event, window, cx| {
                        let text = input.read(cx).text().trim().to_owned();
                        let valid = match field.kind {
                            Kind::Number(_) => text.is_empty() || text.parse::<i32>().is_ok(),
                            _ => true,
                        };
                        let saved = this.options.get(field.key).unwrap_or_default().trim();
                        match event {
                            InputEvent::Changed if valid && text != saved => {
                                this.edit(cx, |options| options.set(field.key, Some(&text)));
                            }
                            InputEvent::Submitted if valid => window.blur(cx),
                            InputEvent::Changed | InputEvent::Submitted => {}
                        }
                        input.update(cx, |input, cx| {
                            input.set_invalid(matches!(event, InputEvent::Submitted) && !valid, cx)
                        });
                    },
                ));
                inputs.push((field, input));
            }
        }
        Self {
            skua,
            account: None,
            options: Options::default(),
            problem: None,
            wardrobe: Reading::Done(None),
            modes: None,
            inputs,
            scroll: ScrollHandle::new(),
            _reading_modes: None,
            _subscriptions: subscriptions,
        }
    }

    pub(super) fn show(
        &mut self,
        account: Option<AccountId>,
        view: Option<Entity<RuffleView>>,
        cx: &mut Context<Self>,
    ) {
        self.account = account;
        self.reload(cx);
        self.read_wardrobe(view, cx);
        self.read_modes(cx);
    }

    pub(super) fn refresh(
        &mut self,
        account: Option<AccountId>,
        view: Option<Entity<RuffleView>>,
        cx: &mut Context<Self>,
    ) {
        if account != self.account {
            self.show(account, view, cx);
        } else if matches!(self.wardrobe, Reading::Done(None)) && view.is_some() {
            self.read_wardrobe(view, cx);
        }
    }

    fn read_modes(&mut self, cx: &mut Context<Self>) {
        let advanced = core_bots::advanced_skills(&self.skua);
        self._reading_modes = Some(cx.spawn(async move |this, cx| {
            let modes = cx
                .background_spawn(async move { read_modes(&advanced) })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.modes = Some(modes);
                cx.notify();
            });
        }));
    }

    fn path(&self) -> Option<PathBuf> {
        Some(core_bots::file(&self.skua, self.account.as_ref()?))
    }

    fn reload(&mut self, cx: &mut Context<Self>) {
        let loaded = match self.path() {
            Some(path) => Options::load(&path).map_err(|error| {
                SharedString::from(format!("Couldn't read {}: {error}", path.display()))
            }),
            None => Ok(Options::default()),
        };
        let (options, problem) = match loaded {
            Ok(options) => (options, None),
            Err(problem) => (Options::default(), Some(problem)),
        };
        self.problem = problem;
        self.show_options(options, cx);
    }

    fn show_options(&mut self, options: Options, cx: &mut Context<Self>) {
        self.options = options;
        for (field, input) in &self.inputs {
            let text = self.options.get(field.key).unwrap_or_default().to_owned();
            input.update(cx, |input, cx| {
                input.set_text(text, cx);
                input.set_invalid(false, cx);
            });
        }
        cx.notify();
    }

    #[cfg(test)]
    pub(super) fn fake(
        &mut self,
        account: &str,
        options: &str,
        wardrobe: Option<Wardrobe>,
        modes: ClassModes,
        cx: &mut Context<Self>,
    ) {
        self.account = Some(AccountId::of(account));
        self.show_options(Options::parse(options), cx);
        self.wardrobe = Reading::Done(wardrobe);
        self.modes = Some(modes);
        self._reading_modes = None;
    }

    fn read_wardrobe(&mut self, view: Option<Entity<RuffleView>>, cx: &mut Context<Self>) {
        let Some(view) = view else {
            self.wardrobe = Reading::Done(None);
            return;
        };
        let (reply, answer) = async_channel::bounded(1);
        view.update(cx, |view, _| {
            view.session.command(Command::Grabber(Job::Items { reply }))
        });
        self.wardrobe = Reading::Waiting {
            _task: cx.spawn(async move |this, cx| {
                let items = answer.recv().await.ok().and_then(Result::ok).flatten();
                let _ = this.update(cx, |this, cx| {
                    this.wardrobe = Reading::Done(items.as_deref().map(Wardrobe::of));
                    cx.notify();
                });
            }),
        };
    }

    fn edit(&mut self, cx: &mut Context<Self>, edit: impl FnOnce(&mut Options)) {
        let Some(path) = self.path() else {
            return;
        };
        let mut options = match Options::load(&path) {
            Ok(options) => options,
            Err(error) => {
                self.problem = Some(format!("Couldn't read {}: {error}", path.display()).into());
                cx.notify();
                return;
            }
        };
        let before = options.clone();
        edit(&mut options);
        if options == before {
            return;
        }
        match options.save(&path) {
            Ok(()) => {
                self.problem = None;
                self.options = options;
            }
            Err(error) => {
                self.show_options(self.options.clone(), cx);
                self.problem = Some(format!("Couldn't save {}: {error}", path.display()).into());
            }
        }
        cx.notify();
    }

    fn wardrobe(&self) -> Option<&Wardrobe> {
        match &self.wardrobe {
            Reading::Done(wardrobe) => wardrobe.as_ref(),
            Reading::Waiting { .. } => None,
        }
    }

    fn field(&self, field: &'static Field, cx: &mut Context<Self>) -> AnyElement {
        let enabled = match field.key {
            core_bots::ROOM => self.options.switch(core_bots::PRIVATE_ROOMS, true),
            _ => true,
        };
        match field.kind {
            Kind::Switch(default) => {
                let this = cx.weak_entity();
                Switch::new(
                    field.key,
                    field.label,
                    self.options.switch(field.key, default),
                )
                .hide_label()
                .on_toggle(move |on, _, cx| {
                    let _ = this.update(cx, |this, cx| {
                        this.edit(cx, |options| options.set_switch(field.key, on))
                    });
                })
                .into_any_element()
            }
            Kind::Number(_) => {
                let input = self.input(field);
                input.update(cx, |input, cx| input.set_disabled(!enabled, cx));
                div().w(NARROW).child(input).into_any_element()
            }
            Kind::Text(_) => div().w(WIDE).child(self.input(field)).into_any_element(),
        }
    }

    fn input(&self, field: &'static Field) -> Entity<Input> {
        self.inputs
            .iter()
            .find(|(shown, _)| shown.key == field.key)
            .map(|(_, input)| input.clone())
            .expect("every number and text field has an input")
    }

    fn choice(
        &self,
        id: String,
        label: SharedString,
        key: String,
        owned: Option<&[SharedString]>,
        enabled: bool,
        cx: &mut Context<Self>,
    ) -> Select {
        let saved = self.options.get(&key).map(|saved| saved.to_owned());
        let mut choices: Vec<SharedString> = vec![NONE.into()];
        choices.extend(owned.unwrap_or_default().iter().cloned());
        if let Some(saved) = &saved
            && !choices.iter().skip(1).any(|choice| choice == saved)
        {
            choices.push(saved.clone().into());
        }
        let selected = saved
            .as_deref()
            .and_then(|saved| choices.iter().skip(1).position(|choice| choice == saved))
            .map_or(0, |index| index + 1);
        let this = cx.weak_entity();
        let picked = choices.clone();
        Select::new(SharedString::from(id), label, choices, Some(selected))
            .width(WIDE)
            .disabled(!enabled)
            .on_select(move |index, _, cx| {
                let value = (index > 0).then(|| picked[index].to_string());
                let key = key.clone();
                let _ = this.update(cx, |this, cx| {
                    this.edit(cx, |options| options.set(&key, value.as_deref()))
                });
            })
    }

    fn modes(&self, class: Option<&str>) -> Vec<&'static str> {
        let known = class
            .and_then(|class| self.modes.as_ref()?.get(class))
            .filter(|modes| !modes.is_empty());
        known.cloned().unwrap_or_else(|| vec!["Base"])
    }

    fn role(&self, role: Role, cx: &mut Context<Self>) -> Group {
        let wardrobe = self.wardrobe();
        let class_key = role.class_key();
        let class = self.options.get(&class_key).map(str::to_owned);
        let mode_key = role.mode_key();
        let mut modes: Vec<SharedString> = self
            .modes(class.as_deref())
            .into_iter()
            .map(SharedString::from)
            .collect();
        let mode = self.options.get(&mode_key).unwrap_or("Base").to_owned();
        if !modes.iter().any(|known| *known == mode) {
            modes.push(mode.clone().into());
        }
        let picked_mode = modes.iter().position(|known| *known == mode);
        let this = cx.weak_entity();
        let mode_choices = modes.clone();
        let mode_select = Select::new(
            SharedString::from(format!("{}-mode", role.label())),
            format!("{} class mode", role.label()),
            modes,
            picked_mode,
        )
        .width(NARROW)
        .disabled(class.is_none())
        .on_select({
            let mode_key = mode_key.clone();
            move |index, _, cx| {
                let mode = mode_choices[index].to_string();
                let key = mode_key.clone();
                let _ = this.update(cx, |this, cx| {
                    this.edit(cx, |options| options.set(&key, Some(&mode)))
                });
            }
        });
        let class_select = self.choice(
            format!("{}-class", role.label()),
            format!("{} class", role.label()).into(),
            class_key,
            wardrobe.map(|wardrobe| wardrobe.classes.as_slice()),
            true,
            cx,
        );
        let gear_on_key = role.gear_on_key();
        let gear_on = self.options.switch(&gear_on_key, true);
        let this = cx.weak_entity();
        let group = Group::new()
            .title(role.label())
            .row(
                "Class",
                None,
                h_flex()
                    .gap(cx.theme().space(2.0))
                    .child(class_select)
                    .child(mode_select),
            )
            .row(
                "Equip gear",
                Some("Wear the gear below with this class.".into()),
                Switch::new(
                    SharedString::from(gear_on_key.clone()),
                    format!("Equip {} gear", role.label().to_lowercase()),
                    gear_on,
                )
                .hide_label()
                .on_toggle(move |on, _, cx| {
                    let key = gear_on_key.clone();
                    let _ = this.update(cx, |this, cx| {
                        this.edit(cx, |options| options.set_switch(&key, on))
                    });
                }),
            );
        Gear::ALL.into_iter().fold(group, |group, gear| {
            let owned = wardrobe
                .and_then(|wardrobe| wardrobe.gear.get(gear.slot()))
                .map(Vec::as_slice);
            group.row(
                gear.label(),
                None,
                self.choice(
                    role.gear_key(gear),
                    format!("{} {}", role.label(), gear.label().to_lowercase()).into(),
                    role.gear_key(gear),
                    owned,
                    gear_on,
                    cx,
                ),
            )
        })
    }

    fn section(&self, title: &str, fields: &'static [Field], cx: &mut Context<Self>) -> Group {
        fields
            .iter()
            .fold(Group::new().title(title), |group, field| {
                let control = self.field(field, cx);
                group.row(field.label, field.description.map(Into::into), control)
            })
    }
}

fn read_modes(path: &Path) -> ClassModes {
    fs::read_to_string(path)
        .map_err(|error| error.to_string())
        .and_then(|json| core_bots::class_modes(&json).map_err(|error| error.to_string()))
        .unwrap_or_else(|error| {
            tracing::warn!(%error, path = %path.display(), "couldn't read Skua's class modes");
            ClassModes::default()
        })
}

impl Render for CoreBotsPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme: Theme = cx.theme().clone();
        let layout = theme.layout;
        if self.account.is_none() {
            return v_flex()
                .size_full()
                .justify_center()
                .items_center()
                .child(Empty::new(
                    "Not logged in",
                    "Log in to change this account's CoreBots options.",
                ))
                .into_any_element();
        }
        let [general, timing, boosts, other] = core_bots::SECTIONS;
        let mut groups: Vec<AnyElement> = Vec::new();
        groups.extend(self.problem.clone().map(|problem| {
            Alert::new(
                "core-bots-problem",
                Tone::Danger,
                "Couldn't use the options file",
            )
            .body(problem)
            .into_any_element()
        }));
        let inventory = match &self.wardrobe {
            Reading::Waiting { .. } => Some("Reading your inventory…"),
            Reading::Done(Some(_)) => None,
            Reading::Done(None) => Some("Log in to pick classes and gear from your inventory."),
        };
        groups.push(
            v_flex()
                .gap(px(2.0))
                .child(note(PRECEDENCE, true, &theme))
                .children(inventory.map(|text| note(text, true, &theme)))
                .into_any_element(),
        );
        groups.push(self.section(general.0, general.1, cx).into_any_element());
        groups.push(self.section(timing.0, timing.1, cx).into_any_element());
        for role in Role::ALL {
            groups.push(self.role(role, cx).into_any_element());
        }
        groups.push(self.section(boosts.0, boosts.1, cx).into_any_element());
        groups.push(self.section(other.0, other.1, cx).into_any_element());
        let scrolled = self.scroll.offset().y < px(0.0);
        div()
            .relative()
            .size_full()
            .child(
                div()
                    .id("core-bots")
                    .size_full()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .child(
                        v_flex()
                            .w_full()
                            .max_w(px(600.0))
                            .mx_auto()
                            .gap(layout.space(4.0))
                            .px(layout.space(4.0))
                            .pt(layout.space(3.0))
                            .pb(layout.space(4.0))
                            .children(groups),
                    ),
            )
            .when(scrolled, |pane| {
                pane.child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .right_0()
                        .h(layout.space(6.0))
                        .bg(linear_gradient(
                            180.0,
                            linear_color_stop(theme.background, 0.0),
                            linear_color_stop(theme.background.opacity(0.0), 1.0),
                        )),
                )
            })
            .child(Scrollbar::new("core-bots-scrollbar", self.scroll.clone()).persistent())
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(
        name: &str,
        cx: &mut gpui::TestAppContext,
    ) -> (PathBuf, Entity<CoreBotsPage>, gpui::VisualTestContext) {
        let mut random = [0; 8];
        getrandom::getrandom(&mut random).unwrap();
        let temp =
            std::env::temp_dir().join(format!("mira-{name}-{:x}", u64::from_ne_bytes(random)));
        let skua = temp.join("skua/Scripts");
        cx.update(mira_ui::init);
        let (page, cx) =
            cx.add_window_view(|window, cx| CoreBotsPage::new(skua.clone(), window, cx));
        let file = core_bots::file(&skua, &AccountId::of("hero"));
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        (file, page, cx.clone())
    }

    fn open(page: &Entity<CoreBotsPage>, cx: &mut gpui::VisualTestContext) {
        page.update(cx, |page, cx| {
            page.show(Some(AccountId::of("hero")), None, cx)
        });
        cx.run_until_parked();
    }

    fn type_delay(page: &Entity<CoreBotsPage>, text: &str, cx: &mut gpui::VisualTestContext) {
        let input = page.read_with(cx, |page, _| page.input(&core_bots::TIMING[0]));
        input.update(cx, |input, cx| input.set_text(text.to_owned(), cx));
        cx.run_until_parked();
    }

    #[gpui::test]
    fn opening_the_page_leaves_the_file_as_it_is(cx: &mut gpui::TestAppContext) {
        let (file, page, mut cx) = page("core-bots-open", cx);
        let text = "PrivateRoomNr: fail\nActionDelayNr: 900 \nSomethingElse\n";
        fs::write(&file, text).unwrap();

        open(&page, &mut cx);

        assert_eq!(fs::read_to_string(&file).unwrap(), text);
    }

    #[gpui::test]
    fn logging_in_shows_the_accounts_options(cx: &mut gpui::TestAppContext) {
        let (file, page, mut cx) = page("core-bots-login", cx);
        fs::write(&file, "ActionDelayNr: 900\n").unwrap();
        page.update(&mut cx, |page, cx| page.show(None, None, cx));

        page.update(&mut cx, |page, cx| {
            page.refresh(Some(AccountId::of("hero")), None, cx)
        });
        cx.run_until_parked();

        let input = page.read_with(&cx, |page, _| page.input(&core_bots::TIMING[0]));
        assert_eq!(
            input.read_with(&cx, |input, _| input.text().to_string()),
            "900"
        );
    }

    #[gpui::test]
    fn an_edit_keeps_what_is_on_disk(cx: &mut gpui::TestAppContext) {
        let (file, page, mut cx) = page("core-bots-edit", cx);
        fs::write(&file, "ActionDelayNr: 700\n").unwrap();
        open(&page, &mut cx);

        fs::write(&file, "ActionDelayNr: 700\nHelm3Select: Hood\n").unwrap();
        type_delay(&page, "800", &mut cx);
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            "ActionDelayNr: 800\nHelm3Select: Hood\n"
        );

        let folder = file.parent().unwrap();
        let mut permissions = fs::metadata(folder).unwrap().permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o555);
        fs::set_permissions(folder, permissions.clone()).unwrap();
        page.update(&mut cx, |page, cx| {
            page.edit(cx, |options| options.set_switch("AntiLag", false))
        });
        type_delay(&page, "850", &mut cx);
        let (anti_lag, problem) = page.read_with(&cx, |page, _| {
            (page.options.switch("AntiLag", true), page.problem.is_some())
        });
        let delay = page.read_with(&cx, |page, _| page.input(&core_bots::TIMING[0]));
        assert_eq!(
            delay.read_with(&cx, |input, _| input.text().to_string()),
            "800"
        );
        std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o755);
        fs::set_permissions(folder, permissions).unwrap();
        assert!(anti_lag && problem);

        fs::write(&file, b"ActionDelayNr: 800\n\xff\n").unwrap();
        type_delay(&page, "900", &mut cx);
        assert_eq!(fs::read(&file).unwrap(), b"ActionDelayNr: 800\n\xff\n");
    }
}
