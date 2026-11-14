use std::rc::Rc;

use gpui::{
    AnyElement, App, AppContext as _, Context, ElementId, Entity, Focusable as _, FontWeight,
    InteractiveElement as _, IntoElement, KeyDownEvent, Keystroke, MouseDownEvent,
    ParentElement as _, Pixels, RenderOnce, Role, ScrollHandle, SharedString,
    StatefulInteractiveElement as _, Styled as _, Subscription, WeakEntity, Window, div,
    prelude::FluentBuilder as _, px,
};

use mira_ui::{
    ActiveTheme as _, FocusNext, FocusPrevious, PointerCursor as _, StyledExt as _, h_flex, motion,
    nav::{self, Axis, Step},
    primitives::{
        Button, Field, Icon, Input, InputEvent, MenuItem, Menubar, Numeric, PopoverClose,
        Scrollbar, Select, Spinner, Switch,
        button::{Size, Variant},
        icon,
    },
    theme::{DISABLED_OPACITY, ROW, TEXT_BODY},
    v_flex,
};

use super::RuffleView;
use crate::{
    automation::{
        auto_relogin::Phase,
        game_settings::{FrameRate, RenderingMode, Settings as GameSettings},
        settings::Automation,
    },
    combat_profiles,
    domain::{
        player::Identity,
        profiles,
        world::{Location, PADS},
    },
    ruffle::Command,
};

pub(crate) const BAR_HEIGHT: Pixels = px(38.0);

const LAST_SERVER: &str = "Last server";
const MAX_NUMBER: u32 = 999;
const MAX_TEXT: usize = 32;
const VISIBLE_CELLS: f32 = 10.5;
const SCROLLBAR_CLEARANCE: Pixels = px(2.0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Setting {
    Delay,
    Retries,
    Backoff,
    BackoffCap,
    WalkSpeed,
    FrameRate,
    CustomName,
    CustomGuild,
}

impl Setting {
    const ALL: [Setting; 8] = [
        Setting::Delay,
        Setting::Retries,
        Setting::Backoff,
        Setting::BackoffCap,
        Setting::WalkSpeed,
        Setting::FrameRate,
        Setting::CustomName,
        Setting::CustomGuild,
    ];
    const RELOGIN: [Setting; 4] = [
        Setting::Delay,
        Setting::Retries,
        Setting::Backoff,
        Setting::BackoffCap,
    ];

    fn label(self) -> &'static str {
        match self {
            Setting::Delay => "Delay",
            Setting::Retries => "Retries",
            Setting::Backoff => "Backoff",
            Setting::BackoffCap => "Backoff cap",
            Setting::WalkSpeed => "Walk speed",
            Setting::FrameRate => "Frame rate",
            Setting::CustomName => "Name",
            Setting::CustomGuild => "Guild",
        }
    }

    fn range(self) -> Option<(u32, u32)> {
        match self {
            Setting::FrameRate => Some(FrameRate::RANGE),
            Setting::CustomName | Setting::CustomGuild => None,
            _ => Some((0, MAX_NUMBER)),
        }
    }

    fn unit(self) -> Option<&'static str> {
        match self {
            Setting::Delay | Setting::Backoff | Setting::BackoffCap => Some("s"),
            _ => None,
        }
    }

    fn pill(self) -> bool {
        matches!(self, Setting::WalkSpeed | Setting::FrameRate)
    }

    fn input(self, cx: &mut App) -> Entity<Input> {
        cx.new(|cx| {
            let input = Input::new(self.label(), cx).small();
            let input = match self.range() {
                Some((min, max)) => input.numeric(Numeric {
                    min: min.into(),
                    max: max.into(),
                    step: 1,
                }),
                None => input.max_length(MAX_TEXT),
            };
            let input = if self.pill() { input.pill() } else { input };
            match self.unit() {
                Some(unit) => input.suffix(unit),
                None => input,
            }
        })
    }

    fn value(self, automation: &Automation) -> String {
        let relogin = || automation.auto_relogin();
        let game = || automation.game_settings();
        match self {
            Setting::Delay => relogin().delay_seconds.to_string(),
            Setting::Retries => relogin().retries.to_string(),
            Setting::Backoff => relogin().backoff_seconds.to_string(),
            Setting::BackoffCap => relogin().backoff_cap_seconds.to_string(),
            Setting::WalkSpeed => game().walk_speed.to_string(),
            Setting::FrameRate => automation
                .frame_rate()
                .map(|fps| fps.to_string())
                .unwrap_or_default(),
            Setting::CustomName => game().custom_name.unwrap_or_default(),
            Setting::CustomGuild => game().custom_guild.unwrap_or_default(),
        }
    }

    fn commit(self, text: String, view: &mut RuffleView, cx: &mut Context<RuffleView>) {
        match self {
            Setting::CustomName => {
                return view.change_game_settings(|settings| settings.custom_name = Some(text), cx);
            }
            Setting::CustomGuild => {
                return view
                    .change_game_settings(|settings| settings.custom_guild = Some(text), cx);
            }
            _ => {}
        }
        let Ok(value) = text.parse() else {
            return;
        };
        match self {
            Setting::Delay => {
                view.change_auto_relogin(|settings| settings.delay_seconds = value, cx)
            }
            Setting::Retries => view.change_auto_relogin(|settings| settings.retries = value, cx),
            Setting::Backoff => {
                view.change_auto_relogin(|settings| settings.backoff_seconds = value, cx)
            }
            Setting::BackoffCap => {
                view.change_auto_relogin(|settings| settings.backoff_cap_seconds = value, cx)
            }
            Setting::WalkSpeed => {
                view.change_game_settings(|settings| settings.walk_speed = value, cx)
            }
            Setting::FrameRate => view.change_frame_rate(value, cx),
            Setting::CustomName | Setting::CustomGuild => {}
        }
    }

    fn is_default(self, automation: &Automation) -> bool {
        let game = automation.game_settings();
        match self {
            Setting::WalkSpeed => game.walk_speed == GameSettings::default().walk_speed,
            Setting::FrameRate => automation
                .frame_rate()
                .is_none_or(|fps| fps == FrameRate::DEFAULT),
            Setting::CustomName => game.custom_name.is_none(),
            Setting::CustomGuild => game.custom_guild.is_none(),
            _ => true,
        }
    }

    fn reset(self, view: &mut RuffleView, cx: &mut Context<RuffleView>) {
        match self {
            Setting::WalkSpeed => view.change_game_settings(
                |settings| settings.walk_speed = GameSettings::default().walk_speed,
                cx,
            ),
            Setting::FrameRate => view.change_frame_rate(FrameRate::DEFAULT, cx),
            Setting::CustomName => {
                view.change_game_settings(|settings| settings.custom_name = None, cx)
            }
            Setting::CustomGuild => {
                view.change_game_settings(|settings| settings.custom_guild = None, cx)
            }
            _ => {}
        }
    }

    fn needs_game(self) -> bool {
        matches!(
            self,
            Setting::WalkSpeed | Setting::FrameRate | Setting::CustomName | Setting::CustomGuild
        )
    }

    fn editable(self, automation: &Automation) -> bool {
        self != Setting::FrameRate || !automation.game_settings().rendering.minimal()
    }

    fn enabled(self, automation: &Automation, live: bool) -> bool {
        self.editable(automation) && (live || !self.needs_game())
    }
}

#[derive(Clone)]
pub(super) struct Inputs([Entity<Input>; Setting::ALL.len()]);

impl Inputs {
    fn new(cx: &mut App) -> Self {
        Self(Setting::ALL.map(|setting| setting.input(cx)))
    }

    fn get(&self, setting: Setting) -> &Entity<Input> {
        &self.0[setting as usize]
    }
}

type Toggle = (
    &'static str,
    &'static str,
    fn(&mut GameSettings) -> &mut bool,
);

const TOGGLES: [Toggle; 11] = [
    ("infinite-range", "Infinite range", |s| {
        &mut s.infinite_range
    }),
    ("provoke-cell", "Provoke cell", |s| &mut s.provoke_cell),
    ("enemy-magnet", "Enemy magnet", |s| &mut s.enemy_magnet),
    ("hide-players", "Hide players", |s| &mut s.hide_players),
    ("skip-cutscenes", "Skip cutscenes", |s| {
        &mut s.skip_cutscenes
    }),
    ("animations", "Animations", |s| &mut s.animations),
    ("collisions", "Collisions", |s| &mut s.collisions),
    ("ads", "Ads", |s| &mut s.ads),
    ("anti-counter", "Anti counter", |s| &mut s.anti_counter),
    ("anti-afk", "Anti AFK", |s| &mut s.anti_afk),
    ("auto-zone", "Auto zone", |s| &mut s.auto_zone),
];

pub(super) struct BarState {
    inputs: Inputs,
    location: Option<Location>,
    identity: Option<Identity>,
    _subscriptions: Vec<Subscription>,
}

impl BarState {
    pub(super) fn new(window: &mut Window, cx: &mut Context<RuffleView>) -> Self {
        let inputs = Inputs::new(cx);
        let mut subscriptions = Vec::new();
        for setting in Setting::ALL {
            let input = inputs.get(setting);
            subscriptions.push(cx.subscribe(input, move |view, _, event, cx| {
                if let InputEvent::Submitted = event {
                    view.commit(setting, cx);
                }
            }));
            subscriptions.push(
                cx.on_blur(&input.focus_handle(cx), window, move |view, _, cx| {
                    view.commit(setting, cx)
                }),
            );
        }
        Self {
            inputs,
            location: None,
            identity: None,
            _subscriptions: subscriptions,
        }
    }

    pub(super) fn located(&mut self, location: Option<Location>) {
        self.location = location;
    }

    pub(super) fn location(&self) -> Option<&Location> {
        self.location.as_ref()
    }

    pub(super) fn identified(&mut self, identity: Option<Identity>) {
        self.identity = identity;
    }

    pub(super) fn character(&self) -> Option<&str> {
        self.identity
            .as_ref()
            .map(|identity| identity.name.as_str())
    }
}

struct Options {
    automation: Automation,
    live: bool,
    name: SharedString,
    guild: SharedString,
    inputs: Inputs,
}

struct Relogin {
    automation: Automation,
    status: Option<String>,
    servers: Vec<String>,
    inputs: Inputs,
}

impl RuffleView {
    fn in_game(&self) -> bool {
        self.bar.location.is_some()
    }

    fn commit(&mut self, setting: Setting, cx: &mut Context<Self>) {
        let input = self.bar.inputs.get(setting).clone();
        let text = input.read(cx).text().to_owned();
        if setting.enabled(&self.automation, self.in_game())
            && text != setting.value(&self.automation)
        {
            setting.commit(text, self, cx);
        }
        self.show_value(setting, cx);
    }

    fn reset(&mut self, setting: Setting, cx: &mut Context<Self>) {
        setting.reset(self, cx);
        self.show_value(setting, cx);
    }

    fn show_value(&self, setting: Setting, cx: &mut Context<Self>) {
        let value = setting.value(&self.automation);
        self.bar.inputs.get(setting).update(cx, |input, cx| {
            if input.text() != value {
                input.set_text(value, cx);
            }
        });
    }

    fn bar_dismissed(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus, cx);
        if self.player_bounds.contains(&event.position) {
            self.mouse.swallow(event.button);
            cx.stop_propagation();
        }
    }

    fn options(&self) -> Options {
        let identity = self.bar.identity.as_ref();
        let name = identity
            .map(|identity| identity.name.clone())
            .or_else(|| self.username().map(str::to_owned));
        let guild = identity.and_then(|identity| identity.guild.clone());
        Options {
            automation: self.automation.clone(),
            live: self.in_game(),
            name: name.unwrap_or_default().into(),
            guild: guild.unwrap_or_default().into(),
            inputs: self.bar.inputs.clone(),
        }
    }

    fn relogin(&self) -> Relogin {
        Relogin {
            automation: self.automation.clone(),
            status: status(&self.auto_relogin),
            servers: self.servers.clone(),
            inputs: self.bar.inputs.clone(),
        }
    }
}

fn update(
    this: &WeakEntity<RuffleView>,
    cx: &mut App,
    f: impl FnOnce(&mut RuffleView, &mut Context<RuffleView>),
) {
    let _ = this.update(cx, f);
}

fn act(
    this: &WeakEntity<RuffleView>,
    f: impl Fn(&mut RuffleView, &mut Context<RuffleView>) + 'static,
) -> impl Fn(&mut Window, &mut App) + 'static {
    let this = this.clone();
    move |_, cx| update(&this, cx, &f)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Column {
    Cell,
    Pad,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Spot {
    column: Column,
    row: usize,
}

#[derive(PartialEq, Debug)]
enum JumpKey {
    Move(Spot),
    Travel {
        cell: String,
        pad: String,
        keep_open: bool,
    },
}

fn current_spot(location: &Location) -> Spot {
    if location.cells.is_empty() {
        let row = PADS
            .iter()
            .position(|pad| pad.eq_ignore_ascii_case(&location.pad));
        return Spot {
            column: Column::Pad,
            row: row.unwrap_or(0),
        };
    }
    let row = location
        .cells
        .iter()
        .position(|cell| *cell == location.cell);
    Spot {
        column: Column::Cell,
        row: row.unwrap_or(0),
    }
}

fn jump_key(location: &Location, at: Option<Spot>, event: &KeyDownEvent) -> Option<JumpKey> {
    let len = |column| match column {
        Column::Cell => location.cells.len(),
        Column::Pad => PADS.len(),
    };
    let labels = |column| -> Vec<(usize, String)> {
        match column {
            Column::Cell => location.cells.iter().cloned().enumerate().collect(),
            Column::Pad => PADS.iter().map(|pad| pad.to_string()).enumerate().collect(),
        }
    };
    let key = event.keystroke.key.as_str();
    let across = match key {
        "left" => Some(Column::Cell),
        "right" => Some(Column::Pad),
        "tab" => Some(match at.map(|spot| spot.column) {
            Some(Column::Cell) => Column::Pad,
            _ => Column::Cell,
        }),
        _ => None,
    };
    let Some(at) = at.filter(|spot| spot.row < len(spot.column)) else {
        let start = current_spot(location);
        if across.is_some() || Step::from_key(event, Axis::Vertical).is_some() {
            return Some(JumpKey::Move(start));
        }
        return nav::typeahead(key, labels(start.column).into_iter(), None).map(|row| {
            JumpKey::Move(Spot {
                column: start.column,
                row,
            })
        });
    };
    if key == "enter" || key == "space" {
        let (cell, pad) = match at.column {
            Column::Cell => (location.cells[at.row].clone(), location.pad.clone()),
            Column::Pad => (location.cell.clone(), PADS[at.row].to_string()),
        };
        return Some(JumpKey::Travel {
            cell,
            pad,
            keep_open: event.keystroke.modifiers.shift,
        });
    }
    if let Some(column) = across {
        return (len(column) > 0).then(|| {
            JumpKey::Move(Spot {
                column,
                row: at.row.min(len(column) - 1),
            })
        });
    }
    Step::from_key(event, Axis::Vertical)
        .and_then(|step| nav::step(Some(at.row), len(at.column), step, true, |_| true))
        .or_else(|| nav::typeahead(key, labels(at.column).into_iter(), Some(at.row)))
        .map(|row| {
            JumpKey::Move(Spot {
                column: at.column,
                row,
            })
        })
}

enum Detail {
    Text(SharedString),
    Working(SharedString),
    Waiting(SharedString),
}

#[derive(IntoElement)]
struct Busy {
    id: ElementId,
    label: SharedString,
    waiting: bool,
}

impl RenderOnce for Busy {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let target = if self.waiting { 0.0 } else { 1.0 };
        let spinning = motion::travel((self.id.clone(), "spinning"), target, window, cx);
        let layer = |opacity: f32| div().absolute().inset_0().opacity(opacity);
        div()
            .relative()
            .flex_none()
            .size(px(14.0))
            .when(spinning < 1.0, |busy| {
                let clock = icon(Icon::Clock, cx.theme().text);
                let clock = if cx.reduce_motion() {
                    clock.into_any_element()
                } else {
                    motion::breathe((self.id.clone(), "pulse"), clock).into_any_element()
                };
                busy.child(
                    layer(1.0 - spinning)
                        .id((self.id.clone(), "clock"))
                        .role(Role::ProgressIndicator)
                        .aria_label(self.label.clone())
                        .child(clock),
                )
            })
            .when(spinning > 0.0, |busy| {
                busy.child(layer(spinning).child(Spinner::new(self.id).label(self.label)))
            })
    }
}

fn relogin_value(view: &RuffleView) -> Option<Detail> {
    if !view.automation.auto_relogin().enabled {
        return None;
    }
    match &view.auto_relogin {
        Phase::Attempt { attempt, of } => {
            Some(Detail::Working(format!("Attempt {attempt} of {of}").into()))
        }
        Phase::Waiting | Phase::Backoff { .. } => Some(Detail::Waiting("Waiting".into())),
        Phase::Failed(_) => Some(Detail::Text("Stopped".into())),
        Phase::Off | Phase::Armed => None,
    }
}

fn status(phase: &Phase) -> Option<String> {
    match phase {
        Phase::Off => None,
        Phase::Armed => Some("Watching the connection".into()),
        Phase::Waiting => Some("Connection lost, waiting to log in".into()),
        Phase::Attempt { attempt, of } => Some(format!("Logging in, attempt {attempt} of {of}")),
        Phase::Backoff { attempt, of } => Some(format!(
            "Attempt {attempt} of {of} failed, waiting to retry"
        )),
        Phase::Failed(reason) => Some(format!("Stopped: {reason}")),
    }
}

fn bar_button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Button {
    Button::new(id, label)
        .variant(Variant::Ghost)
        .size(Size::Small)
        .text_size(TEXT_BODY)
}

fn trigger(
    id: &'static str,
    title: &'static str,
    value: Option<Detail>,
    on: Option<bool>,
    cx: &App,
) -> Button {
    let theme = cx.theme();
    let state = match on {
        Some(true) => ", on",
        Some(false) => ", off",
        None => "",
    };
    let label = match &value {
        Some(Detail::Text(value) | Detail::Working(value) | Detail::Waiting(value)) => {
            format!("{title}{state}: {value}")
        }
        None => format!("{title}{state}"),
    };
    let shrinks = matches!(value, Some(Detail::Text(_)));
    bar_button(id, label)
        .when(shrinks, |trigger| trigger.flex_shrink(1.0).min_w_0())
        .child(
            div()
                .flex_none()
                .when(on == Some(true), |title| title.text_color(theme.text))
                .child(title),
        )
        .children(value.map(|value| {
            let waiting = matches!(value, Detail::Waiting(_));
            match value {
                Detail::Text(value) => div()
                    .min_w_0()
                    .max_w(px(120.0))
                    .truncate()
                    .text_color(theme.text_tertiary)
                    .child(value)
                    .into_any_element(),
                Detail::Working(label) | Detail::Waiting(label) => Busy {
                    id: SharedString::from(format!("{id}-working")).into(),
                    label,
                    waiting,
                }
                .into_any_element(),
            }
        }))
}

enum ScriptControl {
    Start(Option<SharedString>),
    Stop(SharedString),
}

struct Bar {
    inset: Pixels,
    live: bool,
    relogin: bool,
    relogin_detail: Option<Detail>,
    attack: bool,
    script: ScriptControl,
}

pub(super) fn bar(view: &RuffleView, cx: &mut Context<RuffleView>) -> impl IntoElement {
    let script = match view.script.loaded() {
        Some(name) if view.script.running() => ScriptControl::Stop(name.clone()),
        loaded => ScriptControl::Start(loaded.cloned()),
    };
    let bar = Bar {
        inset: view.bar_inset,
        live: view.in_game(),
        relogin: view.automation.auto_relogin().enabled,
        relogin_detail: relogin_value(view),
        attack: view.automation.auto_attack().enabled,
        script,
    };
    menubar(bar, cx.weak_entity(), cx)
}

fn menubar(bar: Bar, this: WeakEntity<RuffleView>, cx: &App) -> Menubar {
    let theme = cx.theme();
    Menubar::new(("game-bar", this.entity_id()), "Game")
        .flex_none()
        .w_full()
        .h(BAR_HEIGHT)
        .gap(px(2.0))
        .pr(px(6.0))
        .bg(theme.base)
        .border_b_1()
        .border_color(theme.border)
        .child(super::client::drag_area().flex_none().w(bar.inset))
        .menu(trigger("windows", "Windows", None, None, cx), |_, _| {
            windows_menu()
        })
        .child(
            bar_button("scripts", "Scripts")
                .on_click(|_, window, cx| window.dispatch_action(Box::new(super::OpenScripts), cx)),
        )
        .popover(trigger("options", "Options", None, None, cx), {
            let this = this.clone();
            move |close, window, cx| match this.read_with(cx, |view, _| view.options()) {
                Ok(options) => options_panel(options, &this, close, window, cx),
                Err(_) => div().into_any_element(),
            }
        })
        .popover(
            trigger(
                "auto-relogin",
                "Auto relogin",
                bar.relogin_detail,
                Some(bar.relogin),
                cx,
            ),
            {
                let this = this.clone();
                move |_, window, cx| match this.read_with(cx, |view, _| view.relogin()) {
                    Ok(relogin) => relogin_panel(relogin, &this, window, cx),
                    Err(_) => div().into_any_element(),
                }
            },
        )
        .child(super::client::bar_separator(cx))
        .child(script_button(bar.script, this.clone()))
        .child(super::client::drag_area().flex_1())
        .menu(
            trigger("auto-attack", "Auto attack", None, Some(bar.attack), cx),
            {
                let this = this.clone();
                move |_, cx| {
                    this.read_with(cx, |view, cx| attack_menu(&this, view, cx))
                        .unwrap_or_default()
                }
            },
        )
        .popover(
            trigger("jump", "Jump", None, None, cx).disabled(!bar.live),
            {
                let this = this.clone();
                move |close, window, cx| {
                    let location = this
                        .read_with(cx, |view, _| view.bar.location.clone())
                        .ok()
                        .flatten();
                    jump_panel(location, &this, close, window, cx)
                }
            },
        )
        .child(super::client::bar_separator(cx))
        .child(bar_button("bank", "Bank").disabled(!bar.live).on_click({
            let this = this.clone();
            move |_, _, cx| update(&this, cx, |view, _| view.session.command(Command::OpenBank))
        }))
        .on_dismiss(move |event, window, cx| {
            update(&this, cx, |view, cx| view.bar_dismissed(event, window, cx))
        })
}

fn script_button(script: ScriptControl, this: WeakEntity<RuffleView>) -> Button {
    let (verb, name) = match &script {
        ScriptControl::Start(name) => ("Start", name.as_ref()),
        ScriptControl::Stop(name) => ("Stop", Some(name)),
    };
    let Some(name) = name else {
        return bar_button("script", verb).disabled(true);
    };
    let label = format!("{verb} {name}");
    bar_button("script", label.clone())
        .child(verb)
        .tooltip(label)
        .on_click(move |_, _, cx| {
            update(&this, cx, |view, cx| match script {
                ScriptControl::Start(_) => view.start_loaded_script(cx),
                ScriptControl::Stop(_) => view.stop_script(cx),
            })
        })
}

fn windows_menu() -> Vec<MenuItem> {
    vec![
        MenuItem::action("Combat profiles", |window, cx| {
            window.dispatch_action(Box::new(super::OpenCombatProfiles), cx)
        }),
        MenuItem::action("Environment", |window, cx| {
            window.dispatch_action(Box::new(super::OpenEnvironment), cx)
        }),
        MenuItem::action("Follower", |window, cx| {
            window.dispatch_action(Box::new(super::OpenFollower), cx)
        }),
        MenuItem::action("Loader/grabber", |window, cx| {
            window.dispatch_action(Box::new(super::OpenLoaderGrabber), cx)
        }),
        MenuItem::action("Packets", |window, cx| {
            window.dispatch_action(Box::new(super::OpenPackets), cx)
        }),
    ]
}

fn attack_menu(this: &WeakEntity<RuffleView>, view: &RuffleView, cx: &App) -> Vec<MenuItem> {
    let attack = view.automation.auto_attack();
    let library = combat_profiles::library(cx);
    let mut items = vec![
        MenuItem::switch(
            "Auto attack",
            attack.enabled,
            act(this, |view, cx| {
                view.change_auto_attack(|settings| settings.enabled = !settings.enabled, cx)
            }),
        )
        .disabled(!view.in_game()),
        MenuItem::separator(),
        MenuItem::header("Profile"),
    ];
    let names = std::iter::once(None).chain(library.names().map(|name| Some(name.to_owned())));
    items.extend(names.map(|name| {
        let selected = match (&name, &attack.profile) {
            (None, None) => true,
            (Some(name), Some(chosen)) => profiles::same_name(name, chosen),
            _ => false,
        };
        let label = name.clone().unwrap_or_else(|| profiles::GENERIC.to_owned());
        MenuItem::radio(
            label,
            selected,
            act(this, move |view, cx| {
                let name = name.clone();
                view.change_auto_attack(|settings| settings.profile = name, cx)
            }),
        )
    }));
    let editing = attack.profile;
    items.push(MenuItem::separator());
    items.push(MenuItem::action("Edit profiles…", move |_, cx| {
        super::open_combat_profiles(editing.clone(), cx)
    }));
    items
}

fn heading(title: &'static str, cx: &App) -> impl IntoElement {
    div()
        .text_caption()
        .font_weight(FontWeight::MEDIUM)
        .text_color(cx.theme().text_tertiary)
        .child(title)
}

fn note(text: &'static str, cx: &App) -> AnyElement {
    div()
        .px(px(8.0))
        .py(px(6.0))
        .text_small()
        .text_color(cx.theme().text_tertiary)
        .child(text)
        .into_any_element()
}

fn pair(left: impl IntoElement, right: impl IntoElement) -> impl IntoElement {
    div()
        .grid()
        .grid_cols(2)
        .gap(px(12.0))
        .child(div().min_w_0().child(left))
        .child(div().min_w_0().child(right))
}

fn setting_input(
    setting: Setting,
    automation: &Automation,
    enabled: bool,
    inputs: &Inputs,
    window: &mut Window,
    cx: &mut App,
) -> impl IntoElement + use<> {
    let input = inputs.get(setting).clone();
    let value = setting.value(automation);
    let focused = input.focus_handle(cx).is_focused(window);
    input.update(cx, |input, cx| {
        input.set_disabled(!enabled, cx);
        if !focused && input.text() != value {
            input.set_text(value.clone(), cx);
        }
    });
    div().child(input.clone()).on_key_down(move |event, _, cx| {
        if event.keystroke.key == "escape" {
            input.update(cx, |input, cx| input.set_text(value.clone(), cx));
        }
    })
}

fn setting_field(
    setting: Setting,
    automation: &Automation,
    enabled: bool,
    inputs: &Inputs,
    window: &mut Window,
    cx: &mut App,
) -> Field {
    let input = setting_input(setting, automation, enabled, inputs, window, cx);
    Field::new(setting.label(), input)
}

fn row_label(label: &'static str, enabled: bool) -> impl IntoElement {
    div()
        .min_w_0()
        .truncate()
        .when(!enabled, |label| label.opacity(DISABLED_OPACITY))
        .child(label)
}

fn reset_button(
    setting: Setting,
    automation: &Automation,
    enabled: bool,
    this: &WeakEntity<RuffleView>,
) -> Button {
    let this = this.clone();
    let id = SharedString::from(format!("{}-reset", setting.label()));
    let disabled = !enabled || setting.is_default(automation);
    Button::icon(
        id,
        Icon::RotateCcw,
        format!("Reset {}", setting.label().to_lowercase()),
    )
    .size(Size::XSmall)
    .disabled(disabled)
    .when(disabled || setting.pill(), Button::hide_tooltip)
    .on_click(move |_, _, cx| update(&this, cx, |view, cx| view.reset(setting, cx)))
}

fn options_panel(
    options: Options,
    this: &WeakEntity<RuffleView>,
    close: &PopoverClose,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let Options {
        automation,
        live,
        name,
        guild,
        inputs,
    } = options;
    let mut settings = automation.game_settings();
    let switches = TOGGLES.map(|(id, label, setting)| {
        let this = this.clone();
        Switch::new(id, label, *setting(&mut settings))
            .disabled(!live)
            .on_toggle(move |on, _, cx| {
                update(&this, cx, |view, cx| {
                    view.change_game_settings(|settings| *setting(settings) = on, cx)
                })
            })
    });
    let mode = settings.rendering.mode();
    let rendering = {
        let this = this.clone();
        Select::new(
            "rendering",
            "Rendering",
            RenderingMode::ALL.map(RenderingMode::label),
            RenderingMode::ALL.iter().position(|each| *each == mode),
        )
        .width(px(150.0))
        .disabled(!live)
        .on_select(move |index, _, cx| {
            update(&this, cx, |view, cx| {
                view.change_game_settings(
                    |settings| settings.rendering.set(RenderingMode::ALL[index]),
                    cx,
                )
            })
        })
    };
    for (setting, placeholder) in [(Setting::CustomName, name), (Setting::CustomGuild, guild)] {
        inputs
            .get(setting)
            .update(cx, |input, cx| input.set_placeholder(placeholder, cx));
    }
    let field =
        |setting: Setting, window: &mut Window, cx: &mut App| {
            let enabled = setting.enabled(&automation, live);
            setting_field(setting, &automation, enabled, &inputs, window, cx)
                .end_slot(reset_button(setting, &automation, enabled, this))
        };
    let chip = |setting: Setting, window: &mut Window, cx: &mut App| {
        let enabled = setting.enabled(&automation, live);
        h_flex()
            .justify_between()
            .gap(px(8.0))
            .child(row_label(setting.label(), enabled))
            .child(
                h_flex()
                    .flex_none()
                    .gap(px(4.0))
                    .child(reset_button(setting, &automation, enabled, this))
                    .child(div().w(px(44.0)).child(setting_input(
                        setting,
                        &automation,
                        enabled,
                        &inputs,
                        window,
                        cx,
                    ))),
            )
    };
    let walk_speed = chip(Setting::WalkSpeed, window, cx);
    let frame_rate = chip(Setting::FrameRate, window, cx);
    let custom_name = field(Setting::CustomName, window, cx);
    let custom_guild = field(Setting::CustomGuild, window, cx);
    let command = |id: &'static str, label: &'static str, command: Command| {
        let this = this.clone();
        let close = close.clone();
        Button::new(id, label)
            .size(Size::Small)
            .disabled(!live)
            .on_click(move |_, window, cx| {
                let command = command.clone();
                update(&this, cx, |view, _| view.session.command(command));
                close.close(window, cx);
            })
    };
    v_flex()
        .w(px(320.0))
        .gap(px(12.0))
        .child(
            v_flex()
                .gap(px(4.0))
                .child(heading("Gameplay", cx))
                .child(div().grid().grid_cols(2).gap_x(px(12.0)).children(switches))
                .child(
                    v_flex()
                        .gap(px(6.0))
                        .pt(px(6.0))
                        .child(
                            h_flex()
                                .justify_between()
                                .gap(px(12.0))
                                .child(row_label("Rendering", live))
                                .child(rendering),
                        )
                        .child(pair(walk_speed, frame_rate)),
                ),
        )
        .child(
            v_flex()
                .gap(px(8.0))
                .child(heading("Identity", cx))
                .child(pair(custom_name, custom_guild)),
        )
        .child(
            h_flex()
                .gap(px(6.0))
                .child(command("reload-map", "Reload map", Command::ReloadMap))
                .child(command(
                    "set-spawnpoint",
                    "Set spawnpoint",
                    Command::SetSpawnPoint,
                )),
        )
        .into_any_element()
}

fn relogin_panel(
    relogin: Relogin,
    this: &WeakEntity<RuffleView>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let settings = relogin.automation.auto_relogin();
    let mut servers = relogin.servers.clone();
    if let Some(saved) = &settings.server
        && !servers.contains(saved)
    {
        servers.push(saved.clone());
    }
    let selected = match &settings.server {
        None => Some(0),
        Some(saved) => servers
            .iter()
            .position(|server| server == saved)
            .map(|index| index + 1),
    };
    let toggle = {
        let this = this.clone();
        Switch::new("auto-relogin-enabled", "Auto relogin", settings.enabled).on_toggle(
            move |on, _, cx| {
                update(&this, cx, |view, cx| {
                    view.change_auto_relogin(|settings| settings.enabled = on, cx)
                })
            },
        )
    };
    let server = {
        let this = this.clone();
        let choices = servers.clone();
        let options = std::iter::once(SharedString::from(LAST_SERVER))
            .chain(servers.into_iter().map(Into::into));
        Select::new("server", "Server", options, selected)
            .full_width()
            .on_select(move |index, _, cx| {
                let server = index.checked_sub(1).map(|index| choices[index].clone());
                update(&this, cx, |view, cx| {
                    view.change_auto_relogin(|settings| settings.server = server, cx)
                })
            })
    };
    let [delay, retries, backoff, backoff_cap] = Setting::RELOGIN.map(|setting| {
        let enabled = setting.editable(&relogin.automation);
        setting_field(
            setting,
            &relogin.automation,
            enabled,
            &relogin.inputs,
            window,
            cx,
        )
    });
    v_flex()
        .w(px(264.0))
        .gap(px(12.0))
        .child(match relogin.status {
            Some(status) => toggle.description(status),
            None => toggle,
        })
        .child(
            Field::new("Server", server).when(relogin.servers.is_empty(), |field| {
                field.description("Log in to list the other servers")
            }),
        )
        .child(
            v_flex()
                .gap(px(8.0))
                .child(heading("Retry schedule", cx))
                .child(pair(delay, retries))
                .child(pair(backoff, backoff_cap)),
        )
        .into_any_element()
}

fn tab() -> KeyDownEvent {
    KeyDownEvent {
        keystroke: Keystroke {
            modifiers: Default::default(),
            key: "tab".into(),
            key_char: None,
        },
        is_held: false,
        prefer_character_input: false,
    }
}

fn jump_panel(
    location: Option<Location>,
    this: &WeakEntity<RuffleView>,
    close: &PopoverClose,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let Some(location) = location else {
        return note("Log in to travel", cx);
    };
    let theme = cx.theme().clone();
    let spot = window.use_keyed_state("jump-spot", cx, |_, _| current_spot(&location));
    let start = *spot.read(cx);
    let cells_scroll = window
        .use_keyed_state("jump-cells", cx, |_, _| {
            let scroll = ScrollHandle::new();
            scroll.scroll_to_item(start.row);
            scroll
        })
        .read(cx)
        .clone();
    let focus = nav::focus_handle("jump", window, cx);
    let location = Rc::new(location);
    let travel = {
        let this = this.clone();
        let close = close.clone();
        Rc::new(
            move |cell: String, pad: String, keep_open: bool, window: &mut Window, cx: &mut App| {
                update(&this, cx, |view, _| {
                    view.session.command(Command::Jump { cell, pad })
                });
                if !keep_open {
                    close.close(window, cx);
                }
            },
        )
    };
    let respond = {
        let (location, spot, scroll, travel) = (
            location.clone(),
            spot.clone(),
            cells_scroll.clone(),
            travel.clone(),
        );
        move |event: &KeyDownEvent, window: &mut Window, cx: &mut App| -> bool {
            match jump_key(&location, Some(*spot.read(cx)), event) {
                Some(JumpKey::Move(next)) => {
                    spot.update(cx, |spot, cx| {
                        *spot = next;
                        cx.notify();
                    });
                    if next.column == Column::Cell {
                        scroll.scroll_to_item(next.row);
                    }
                    true
                }
                Some(JumpKey::Travel {
                    cell,
                    pad,
                    keep_open,
                }) => {
                    travel(cell, pad, keep_open, window, cx);
                    true
                }
                None => false,
            }
        }
    };
    let respond = Rc::new(respond);
    let row = |spot_here: Spot, label: SharedString, here: bool, (cell, pad): (String, String)| {
        let id: ElementId = match spot_here.column {
            Column::Cell => ("cell", spot_here.row).into(),
            Column::Pad => ("pad", spot_here.row).into(),
        };
        let hover = spot.clone();
        let travel = travel.clone();
        let pressed = theme.fill_pressed;
        h_flex()
            .id(id)
            .flex_none()
            .h(ROW)
            .px(px(6.0))
            .gap(px(6.0))
            .rounded(theme.radius.item)
            .role(Role::MenuItemRadio)
            .aria_label(label.clone())
            .pointer_cursor(&theme)
            .when(*spot.read(cx) == spot_here, |row| row.bg(theme.fill_hover))
            .active(move |style| style.bg(pressed))
            .child(div().flex_none().w(px(12.0)).when(here, |mark| {
                mark.child(icon(Icon::Check, theme.text).size(px(12.0)))
            }))
            .child(div().min_w_0().truncate().child(label))
            .on_mouse_move(move |_, _, cx| {
                hover.update(cx, |spot, cx| {
                    if *spot != spot_here {
                        *spot = spot_here;
                        cx.notify();
                    }
                })
            })
            .on_click(move |event, window, cx| {
                travel(
                    cell.clone(),
                    pad.clone(),
                    event.modifiers().shift,
                    window,
                    cx,
                )
            })
    };
    let cells: Vec<_> = location
        .cells
        .iter()
        .enumerate()
        .map(|(index, cell)| {
            let spot = Spot {
                column: Column::Cell,
                row: index,
            };
            row(
                spot,
                cell.clone().into(),
                *cell == location.cell,
                (cell.clone(), location.pad.clone()),
            )
        })
        .collect();
    let pads: Vec<_> = PADS
        .iter()
        .enumerate()
        .map(|(index, pad)| {
            let spot = Spot {
                column: Column::Pad,
                row: index,
            };
            let in_cell = location
                .cell_pads
                .iter()
                .any(|known| known.eq_ignore_ascii_case(pad));
            row(
                spot,
                (*pad).into(),
                location.pad.eq_ignore_ascii_case(pad),
                (location.cell.clone(), pad.to_string()),
            )
            .when(!in_cell, |row| row.text_color(theme.text_tertiary))
        })
        .collect();
    let column = |label| {
        v_flex()
            .flex_1()
            .min_w_0()
            .child(div().px(px(6.0)).pb(px(2.0)).child(heading(label, cx)))
    };
    let (keys, next, previous) = (respond.clone(), respond.clone(), respond);
    h_flex()
        .id("jump")
        .items_start()
        .w(px(300.0))
        .track_focus(&focus.tab_stop(true))
        .on_key_down(move |event, window, cx| {
            if keys(event, window, cx) {
                cx.stop_propagation();
            }
        })
        .on_action(move |_: &FocusNext, window, cx| {
            next(&tab(), window, cx);
        })
        .on_action(move |_: &FocusPrevious, window, cx| {
            previous(&tab(), window, cx);
        })
        .child(
            column("Cell").child(
                div()
                    .relative()
                    .child(
                        v_flex()
                            .id("jump-cells")
                            .max_h(ROW * VISIBLE_CELLS)
                            .when(location.cells.len() as f32 > VISIBLE_CELLS, |list| {
                                list.pr(Scrollbar::GUTTER + SCROLLBAR_CLEARANCE)
                            })
                            .overflow_y_scroll()
                            .track_scroll(&cells_scroll)
                            .when(cells.is_empty(), |list| list.child(note("No cells", cx)))
                            .children(cells),
                    )
                    .child(Scrollbar::new("jump-cells-scrollbar", cells_scroll.clone())),
            ),
        )
        .child(
            div()
                .flex_none()
                .self_stretch()
                .w(px(1.0))
                .mx(px(6.0))
                .bg(theme.hairline),
        )
        .child(column("Pad").children(pads))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::automation::game_settings::RenderingMode;

    fn press(location: &Location, at: Option<Spot>, key: &'static str) -> Option<JumpKey> {
        let event = KeyDownEvent {
            keystroke: gpui::Keystroke::parse(key).unwrap(),
            is_held: false,
            prefer_character_input: false,
        };
        jump_key(location, at, &event)
    }

    #[test]
    fn jump_moves_between_cells_and_pads_from_the_keyboard() {
        let location = Location {
            map: "battleon".into(),
            cell: "r2".into(),
            pad: "Left".into(),
            cells: vec!["Enter".into(), "r2".into(), "r3".into()],
            cell_pads: vec!["Left".into()],
        };
        let spot = |column, row| Some(Spot { column, row });
        let moved = |column, row| Some(JumpKey::Move(Spot { column, row }));
        assert_eq!(press(&location, None, "down"), moved(Column::Cell, 1));
        assert_eq!(
            press(&location, spot(Column::Cell, 2), "down"),
            moved(Column::Cell, 0)
        );
        assert_eq!(
            press(&location, spot(Column::Cell, 2), "right"),
            moved(Column::Pad, 2)
        );
        assert_eq!(
            press(&location, spot(Column::Pad, 7), "left"),
            moved(Column::Cell, 2)
        );
        assert_eq!(
            press(&location, spot(Column::Cell, 1), "tab"),
            moved(Column::Pad, 1)
        );
        assert_eq!(
            press(&location, spot(Column::Cell, 0), "r"),
            moved(Column::Cell, 1)
        );
        assert_eq!(
            press(&location, spot(Column::Cell, 2), "enter"),
            Some(JumpKey::Travel {
                cell: "r3".into(),
                pad: "Left".into(),
                keep_open: false,
            })
        );
        assert_eq!(
            press(&location, spot(Column::Cell, 2), "shift-enter"),
            Some(JumpKey::Travel {
                cell: "r3".into(),
                pad: "Left".into(),
                keep_open: true,
            })
        );
        assert_eq!(
            press(&location, spot(Column::Pad, 4), "space"),
            Some(JumpKey::Travel {
                cell: "r2".into(),
                pad: "Top".into(),
                keep_open: false,
            })
        );
        assert_eq!(
            press(&location, spot(Column::Pad, 4), "shift-space"),
            Some(JumpKey::Travel {
                cell: "r2".into(),
                pad: "Top".into(),
                keep_open: true,
            })
        );
        assert_eq!(press(&location, spot(Column::Cell, 0), "x"), None);
    }

    type Panel = Rc<dyn Fn(&PopoverClose, &mut Window, &mut App) -> AnyElement>;

    struct Scene {
        title: &'static str,
        panel: Panel,
    }

    impl gpui::Render for Scene {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let theme = cx.theme().clone();
            let panel = self.panel.clone();
            div()
                .key_context(mira_ui::FOCUS_SCOPE)
                .size_full()
                .bg(theme.base)
                .text_color(theme.text)
                .font_family(mira_ui::theme::SYSTEM_SANS)
                .text_body()
                .child(
                    Menubar::new("bar", "Game")
                        .h(BAR_HEIGHT)
                        .px(px(6.0))
                        .border_b_1()
                        .border_color(theme.border)
                        .popover(
                            Button::new("entry", self.title)
                                .variant(Variant::Ghost)
                                .size(Size::Small)
                                .text_size(TEXT_BODY),
                            move |close, window, cx| panel(close, window, cx),
                        )
                        .default_open(0),
                )
        }
    }

    fn in_game_options(automation: &Automation, inputs: &Inputs) -> Options {
        Options {
            automation: automation.clone(),
            live: true,
            name: "Artix".into(),
            guild: "Battleon Bunch".into(),
            inputs: inputs.clone(),
        }
    }

    fn options_scene(automation: Automation, inputs: Inputs) -> Scene {
        Scene {
            title: "Options",
            panel: Rc::new(move |close, window, cx| {
                let options = in_game_options(&automation, &inputs);
                options_panel(options, &WeakEntity::new_invalid(), close, window, cx)
            }),
        }
    }

    #[gpui::test]
    fn tab_skips_the_frame_rate_while_minimal_rendering_holds_it(cx: &mut gpui::TestAppContext) {
        cx.update(mira_ui::init);
        let automation = Automation::default();
        let inputs = cx.update(Inputs::new);
        let (_, cx) = cx.add_window_view(|window, _| {
            window.activate_window();
            options_scene(automation.clone(), inputs.clone())
        });
        cx.run_until_parked();
        let focused = |setting: Setting, cx: &mut gpui::VisualTestContext| {
            let handle = cx.update(|_, cx| inputs.get(setting).focus_handle(cx));
            cx.update(|window, _| handle.is_focused(window))
        };
        let tab_from = |setting: Setting, keys: &str, cx: &mut gpui::VisualTestContext| {
            let handle = cx.update(|_, cx| inputs.get(setting).focus_handle(cx));
            cx.update(|window, cx| window.focus(&handle, cx));
            cx.run_until_parked();
            cx.simulate_keystrokes(keys);
            cx.run_until_parked();
        };

        tab_from(Setting::WalkSpeed, "tab", cx);
        assert!(focused(Setting::FrameRate, cx));

        automation.update_game_settings(|settings| settings.rendering.set(RenderingMode::Minimal));
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
        tab_from(Setting::WalkSpeed, "tab", cx);
        assert!(focused(Setting::CustomName, cx));
        tab_from(Setting::CustomName, "shift-tab", cx);
        assert!(focused(Setting::WalkSpeed, cx));
    }

    #[gpui::test]
    fn escape_discards_an_edit_instead_of_committing_it(cx: &mut gpui::TestAppContext) {
        cx.update(mira_ui::init);
        let automation = Automation::default();
        automation.update_game_settings(|settings| settings.walk_speed = 12);
        let inputs = cx.update(Inputs::new);
        let (_, cx) = cx.add_window_view(|window, _| {
            window.activate_window();
            options_scene(automation.clone(), inputs.clone())
        });
        cx.run_until_parked();
        let walk_speed = inputs.get(Setting::WalkSpeed).clone();
        cx.update(|window, cx| window.focus(&walk_speed.focus_handle(cx), cx));
        cx.run_until_parked();
        cx.simulate_input("5");
        assert_ne!(
            walk_speed.read_with(cx, |input, _| input.text().to_owned()),
            "12"
        );
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        assert_eq!(
            walk_speed.read_with(cx, |input, _| input.text().to_owned()),
            "12"
        );
    }

    fn snapshot(name: &str, height: f32, scene: impl Fn(&mut App) -> Scene + 'static) {
        for path in mira_ui::snapshot(name, gpui::size(px(460.0), px(height)), move |_, cx| {
            let scene = scene(cx);
            cx.new(|_| scene)
        }) {
            assert!(path.exists(), "{} was not written", path.display());
        }
    }

    fn options_fixture(rendering: RenderingMode, cx: &mut App) -> Scene {
        let automation = Automation::default();
        automation.set_frame_rate(30);
        automation.update_game_settings(|settings| {
            settings.hide_players = true;
            settings.skip_cutscenes = true;
            settings.walk_speed = 12;
            settings.rendering.set(rendering);
        });
        options_scene(automation, Inputs::new(cx))
    }

    #[test]
    fn snapshot_options() {
        snapshot("game-options", 520.0, |cx| {
            options_fixture(RenderingMode::Full, cx)
        });
        snapshot("game-options-minimal", 520.0, |cx| {
            options_fixture(RenderingMode::Minimal, cx)
        });
    }

    #[test]
    fn snapshot_relogin() {
        snapshot("game-relogin", 400.0, |cx| {
            let automation = Automation::default();
            automation.update_auto_relogin(|settings| {
                settings.enabled = true;
                settings.server = Some("Twilly".into());
            });
            let inputs = Inputs::new(cx);
            Scene {
                title: "Auto relogin",
                panel: Rc::new(move |_, window, cx| {
                    let relogin = Relogin {
                        automation: automation.clone(),
                        status: status(&Phase::Attempt { attempt: 2, of: 3 }),
                        servers: vec!["Artix".into(), "Twilly".into(), "Yorumi".into()],
                        inputs: inputs.clone(),
                    };
                    relogin_panel(relogin, &WeakEntity::new_invalid(), window, cx)
                }),
            }
        });
    }

    struct Bars(fn() -> Vec<Bar>);

    impl gpui::Render for Bars {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let theme = cx.theme().clone();
            v_flex()
                .size_full()
                .gap(px(8.0))
                .bg(theme.surface)
                .text_color(theme.text)
                .font_family(mira_ui::theme::SYSTEM_SANS)
                .text_body()
                .children((self.0)().into_iter().enumerate().map(|(index, bar)| {
                    div()
                        .id(("bar", index))
                        .child(menubar(bar, WeakEntity::new_invalid(), cx))
                }))
        }
    }

    #[test]
    fn snapshot_bar() {
        for path in mira_ui::snapshot("game-bar", gpui::size(px(900.0), px(160.0)), |_, cx| {
            cx.new(|_| {
                Bars(|| {
                    let bar = |relogin_detail: Option<Detail>, attack, script| Bar {
                        inset: px(84.0),
                        live: true,
                        relogin: relogin_detail.is_some(),
                        relogin_detail,
                        attack,
                        script,
                    };
                    vec![
                        bar(None, false, ScriptControl::Start(None)),
                        bar(None, true, ScriptControl::Start(Some("hunt.ts".into()))),
                        bar(
                            Some(Detail::Working("Attempt 2 of 3".into())),
                            true,
                            ScriptControl::Stop("hunt.ts".into()),
                        ),
                    ]
                })
            })
        }) {
            assert!(path.exists(), "{} was not written", path.display());
        }
    }

    #[test]
    fn snapshot_jump() {
        snapshot("game-jump", 420.0, |_| {
            let location = Location {
                map: "battleon".into(),
                cell: "r3".into(),
                pad: "Left".into(),
                cells: (1..=14).map(|cell| format!("r{cell}")).collect(),
                cell_pads: vec!["Left".into(), "Right".into()],
            };
            Scene {
                title: "Jump",
                panel: Rc::new(move |close, window, cx| {
                    jump_panel(
                        Some(location.clone()),
                        &WeakEntity::new_invalid(),
                        close,
                        window,
                        cx,
                    )
                }),
            }
        });
    }
}
