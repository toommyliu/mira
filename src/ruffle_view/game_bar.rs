use std::{rc::Rc, time::Instant};

use gpui::{
    AnyElement, App, AppContext as _, Context, ElementId, Entity, Focusable as _, FontWeight,
    InteractiveElement as _, IntoElement, KeyDownEvent, Keystroke, MouseDownEvent,
    ParentElement as _, Pixels, Role, ScrollHandle, SharedString, StatefulInteractiveElement as _,
    Styled as _, Subscription, WeakEntity, Window, actions, div, point,
    prelude::FluentBuilder as _, px,
};

use mira_ui::{
    ActiveTheme as _, FocusNext, FocusPrevious, PointerCursor as _, StyledExt as _, Tone,
    components::Disclosure,
    h_flex,
    nav::{self, Axis, Step, Typeahead},
    primitives::{
        Button, Dot, DropdownMenu, Field, Icon, Input, InputEvent, Kbd, MenuItem, Menubar, Numeric,
        PopoverClose, Scrollbar, Spinner, Switch,
        button::{Joined, Size, Variant},
        divider, icon,
    },
    theme::DISABLED_OPACITY,
    v_flex,
};

use super::RuffleView;
use crate::{
    automation::{
        auto_attack,
        auto_relogin::{self, Countdown, Phase},
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
    shortcuts,
};

actions!(
    mira,
    [
        ToggleScript,
        EditScriptInputs,
        ToggleAutoAttack,
        OpenBank,
        OpenJump,
        ToggleOptionsMenu,
        ToggleMinimalRendering,
        ToggleInterfaceRendering
    ]
);

const LAST_SERVER: &str = "Last server";
const MAX_NUMBER: u32 = 999;
const MAX_TEXT: usize = 32;
const VISIBLE_CELLS: f32 = 10.5;
const SCROLLBAR_CLEARANCE: Pixels = px(2.0);
const OPTIONS_WIDTH: f32 = 440.0;
const COLUMN_GAP: f32 = 6.0;
const AUTOMATION_WIDTH: f32 = 248.0;
const RELOGIN_NUMBER_WIDTH: f32 = 48.0;

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
            if self.pill() { input.pill() } else { input }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GameOption {
    InfiniteRange,
    ProvokeCell,
    EnemyMagnet,
    HidePlayers,
    SkipCutscenes,
    Animations,
    Collisions,
    Ads,
    AntiCounter,
    AntiAfk,
    AutoZone,
}

impl GameOption {
    pub(crate) fn label(self) -> &'static str {
        match self {
            GameOption::InfiniteRange => "Infinite range",
            GameOption::ProvokeCell => "Provoke cell",
            GameOption::EnemyMagnet => "Enemy magnet",
            GameOption::HidePlayers => "Hide players",
            GameOption::SkipCutscenes => "Skip cutscenes",
            GameOption::Animations => "Animations",
            GameOption::Collisions => "Collisions",
            GameOption::Ads => "Ads",
            GameOption::AntiCounter => "Anti-counter",
            GameOption::AntiAfk => "Anti-AFK",
            GameOption::AutoZone => "Auto zone",
        }
    }

    fn field(self, settings: &mut GameSettings) -> &mut bool {
        match self {
            GameOption::InfiniteRange => &mut settings.infinite_range,
            GameOption::ProvokeCell => &mut settings.provoke_cell,
            GameOption::EnemyMagnet => &mut settings.enemy_magnet,
            GameOption::HidePlayers => &mut settings.hide_players,
            GameOption::SkipCutscenes => &mut settings.skip_cutscenes,
            GameOption::Animations => &mut settings.animations,
            GameOption::Collisions => &mut settings.collisions,
            GameOption::Ads => &mut settings.ads,
            GameOption::AntiCounter => &mut settings.anti_counter,
            GameOption::AntiAfk => &mut settings.anti_afk,
            GameOption::AutoZone => &mut settings.auto_zone,
        }
    }
}

#[derive(Clone, Copy)]
enum Gameplay {
    Option(GameOption),
    Rendering,
    WalkSpeed,
    FrameRate,
}

const GAMEPLAY: [Gameplay; 14] = [
    Gameplay::Option(GameOption::InfiniteRange),
    Gameplay::Option(GameOption::ProvokeCell),
    Gameplay::Option(GameOption::EnemyMagnet),
    Gameplay::Rendering,
    Gameplay::Option(GameOption::HidePlayers),
    Gameplay::Option(GameOption::SkipCutscenes),
    Gameplay::Option(GameOption::Animations),
    Gameplay::Option(GameOption::Collisions),
    Gameplay::Option(GameOption::AntiCounter),
    Gameplay::Option(GameOption::AutoZone),
    Gameplay::Option(GameOption::Ads),
    Gameplay::Option(GameOption::AntiAfk),
    Gameplay::WalkSpeed,
    Gameplay::FrameRate,
];

#[derive(Clone, PartialEq, gpui::Action)]
#[action(namespace = mira, no_json)]
pub(crate) struct ToggleOption(pub(crate) GameOption);

pub(super) struct BarState {
    inputs: Inputs,
    location: Option<Location>,
    identity: Option<Identity>,
    toggle_panel: Option<&'static str>,
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
            toggle_panel: None,
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

    pub(super) fn identity(&self) -> Option<&Identity> {
        self.identity.as_ref()
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

pub(super) fn on_actions<E: gpui::InteractiveElement>(
    element: E,
    view: Entity<RuffleView>,
    has_inputs: bool,
    client: WeakEntity<super::Client>,
) -> E {
    let on = |f: fn(&mut RuffleView, &mut Context<RuffleView>)| {
        let view = view.clone();
        move |cx: &mut App| view.update(cx, f)
    };
    let (script, attack, bank, jump, options, minimal, interface) = (
        on(RuffleView::toggle_script),
        on(RuffleView::toggle_auto_attack),
        on(RuffleView::open_bank),
        on(|view, cx| view.toggle_panel("jump", cx)),
        on(|view, cx| view.toggle_panel("options", cx)),
        on(|view, cx| view.toggle_rendering(RenderingMode::Minimal, cx)),
        on(|view, cx| view.toggle_rendering(RenderingMode::InterfaceOnly, cx)),
    );
    let edit = on(RuffleView::edit_inputs);
    let element = if has_inputs {
        element.on_action(move |_: &EditScriptInputs, _, cx| edit(cx))
    } else {
        element
    };
    element
        .on_action(move |_: &ToggleScript, _, cx| script(cx))
        .on_action(move |_: &ToggleAutoAttack, _, cx| attack(cx))
        .on_action(move |_: &OpenBank, _, cx| bank(cx))
        .on_action(move |_: &OpenJump, _, cx| jump(cx))
        .on_action(move |_: &ToggleOptionsMenu, _, cx| options(cx))
        .on_action(move |_: &ToggleMinimalRendering, _, cx| minimal(cx))
        .on_action(move |_: &ToggleInterfaceRendering, _, cx| interface(cx))
        .on_action(move |action: &ToggleOption, _, cx| {
            let option = action.0;
            let on = |view: &RuffleView| *option.field(&mut view.automation.game_settings());
            let before = on(view.read(cx));
            view.update(cx, |view, cx| view.toggle_option(option, cx));
            let after = on(view.read(cx));
            if after != before {
                let _ = client.update(cx, |client, cx| {
                    client.show_status(option.label(), after, cx)
                });
            }
        })
}

impl RuffleView {
    fn in_game(&self) -> bool {
        self.bar.location.is_some()
    }

    fn toggle_script(&mut self, cx: &mut Context<Self>) {
        if self.script.asking_inputs(cx) {
            return;
        }
        if self.script.stoppable() {
            self.stop_script(cx);
        } else {
            self.request_start(cx);
        }
    }

    fn toggle_auto_attack(&mut self, cx: &mut Context<Self>) {
        if self.in_game() {
            self.change_auto_attack(|settings| settings.enabled = !settings.enabled, cx);
        }
    }

    fn open_bank(&mut self, _: &mut Context<Self>) {
        if self.in_game() {
            self.session.command(Command::OpenBank);
        }
    }

    fn toggle_panel(&mut self, trigger: &'static str, cx: &mut Context<Self>) {
        self.bar.toggle_panel = Some(trigger);
        cx.notify();
    }

    fn toggle_option(&mut self, option: GameOption, cx: &mut Context<Self>) {
        if self.in_game() {
            self.change_game_settings(
                |settings| {
                    let on = option.field(settings);
                    *on = !*on;
                },
                cx,
            );
        }
    }

    fn toggle_rendering(&mut self, mode: RenderingMode, cx: &mut Context<Self>) {
        if self.in_game() {
            self.change_game_settings(|settings| settings.rendering.toggle(mode), cx);
        }
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

fn jump_key(
    location: &Location,
    at: Option<Spot>,
    event: &KeyDownEvent,
    typeahead: &mut Typeahead,
) -> Option<JumpKey> {
    let now = Instant::now();
    typeahead.end_unless_typed(&event.keystroke);
    let len = |column| match column {
        Column::Cell => location.cells.len(),
        Column::Pad => PADS.len(),
    };
    let labels = |column| -> Vec<(usize, &str)> {
        match column {
            Column::Cell => location
                .cells
                .iter()
                .map(String::as_str)
                .enumerate()
                .collect(),
            Column::Pad => PADS.iter().copied().enumerate().collect(),
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
        let labels = labels(start.column).into_iter();
        return typeahead
            .find(&event.keystroke, now, labels, None)
            .map(|row| {
                JumpKey::Move(Spot {
                    column: start.column,
                    row,
                })
            });
    };
    if key == "enter" || (key == "space" && !typeahead.typing(now)) {
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
        .or_else(|| {
            let labels = labels(at.column).into_iter();
            typeahead.find(&event.keystroke, now, labels, Some(at.row))
        })
        .map(|row| {
            JumpKey::Move(Spot {
                column: at.column,
                row,
            })
        })
}

enum Detail {
    Stopped,
    Working(SharedString),
    Counting(SharedString, Countdown),
}

fn relogin_value(view: &RuffleView) -> Option<Detail> {
    if !view.automation.auto_relogin().enabled {
        return None;
    }
    match &view.auto_relogin {
        Phase::Attempt { attempt, of } => {
            Some(Detail::Working(format!("Attempt {attempt} of {of}").into()))
        }
        Phase::Waiting(countdown) | Phase::Backoff { countdown, .. } => {
            Some(Detail::Counting("Waiting".into(), *countdown))
        }
        Phase::Failed(_) => Some(Detail::Stopped),
        Phase::Off | Phase::Armed => None,
    }
}

fn status(phase: &Phase) -> Option<String> {
    match phase {
        Phase::Off | Phase::Armed => None,
        Phase::Waiting(_) => Some("Connection lost, waiting to log in".into()),
        Phase::Attempt { attempt, of } => Some(format!("Logging in, attempt {attempt} of {of}")),
        Phase::Backoff { attempt, of, .. } => Some(format!(
            "Attempt {attempt} of {of} failed, waiting to retry"
        )),
        Phase::Failed(reason) => Some(format!("Stopped: {}", super::capitalized(reason))),
    }
}

fn bar_button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Button {
    Button::new(id, label)
        .variant(Variant::Ghost)
        .size(Size::Small)
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
        Some(Detail::Stopped) => format!("{title}{state}: Stopped"),
        Some(Detail::Working(value) | Detail::Counting(value, _)) => {
            format!("{title}{state}: {value}")
        }
        None => format!("{title}{state}"),
    };
    let status = match value {
        Some(Detail::Stopped) => Some(Dot::new(Tone::Attention).into_any_element()),
        Some(Detail::Working(label)) => Some(
            Spinner::new(SharedString::from(format!("{id}-working")))
                .label(label)
                .size(px(14.0))
                .into_any_element(),
        ),
        Some(Detail::Counting(label, countdown)) => Some(
            Spinner::new(SharedString::from(format!("{id}-countdown")))
                .label(label)
                .size(px(14.0))
                .countdown(countdown.from, countdown.until)
                .into_any_element(),
        ),
        None => on.map(|on| {
            div()
                .relative()
                .text_color(theme.muted_foreground)
                .child(div().invisible().child("Off"))
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .child(if on { "On" } else { "Off" }),
                )
                .into_any_element()
        }),
    };
    bar_button(id, label)
        .child(div().flex_none().child(title))
        .children(status)
}

enum ScriptControl {
    Start(Option<SharedString>),
    Stop(SharedString),
}

struct Bar {
    inset: Pixels,
    live: bool,
    relogin: bool,
    login_captured: bool,
    relogin_detail: Option<Detail>,
    attack: bool,
    script: ScriptControl,
    has_inputs: bool,
    toggle_panel: Option<&'static str>,
}

pub(super) fn bar(view: &mut RuffleView, cx: &mut Context<RuffleView>) -> impl IntoElement {
    let loaded = view
        .script
        .entry()
        .map(|entry| super::script_names::name(entry, cx));
    let script = match loaded {
        Some(name) if view.script.stoppable() => ScriptControl::Stop(name),
        loaded => ScriptControl::Start(loaded),
    };
    let bar = Bar {
        inset: view.bar_inset,
        live: view.in_game(),
        relogin: view.automation.auto_relogin().enabled,
        login_captured: view.login_captured,
        relogin_detail: relogin_value(view),
        attack: view.automation.auto_attack().enabled,
        script,
        has_inputs: view.script.has_inputs(),
        toggle_panel: view.bar.toggle_panel.take(),
    };
    menubar(bar, cx.weak_entity(), cx)
}

fn menubar(bar: Bar, this: WeakEntity<RuffleView>, cx: &App) -> Menubar {
    let has_inputs = bar.has_inputs;
    let theme = cx.theme();
    let layout = theme.layout;
    let menubar = Menubar::new(("game-bar", this.entity_id()), "Game")
        .flex_none()
        .w_full()
        .h(cx.theme().layout.chrome)
        .gap(layout.space(0.5))
        .pr(layout.space(1.5))
        .bg(theme.frame)
        .border_b_1()
        .border_color(theme.border)
        .child(super::client::drag_area().flex_none().w(bar.inset))
        .menu(trigger("windows", "Windows", None, None, cx), |_, cx| {
            windows_menu(cx)
        })
        .button(
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
        .row_popover(
            trigger(
                "auto-relogin",
                "Auto relogin",
                bar.relogin_detail,
                Some(bar.relogin),
                cx,
            )
            .disabled(!bar.relogin && !bar.login_captured),
            {
                let this = this.clone();
                move |_, window, cx| match this.read_with(cx, |view, _| view.relogin()) {
                    Ok(relogin) => relogin_panel(relogin, &this, window, cx),
                    Err(_) => div().into_any_element(),
                }
            },
        )
        .row_popover(
            trigger("auto-attack", "Auto attack", None, Some(bar.attack), cx).disabled(!bar.live),
            {
                let this = this.clone();
                move |close, _, cx| match this.read_with(cx, |view, _| {
                    (view.automation.auto_attack(), view.in_game())
                }) {
                    Ok((attack, live)) => attack_panel(attack, live, &this, close, cx),
                    Err(_) => div().into_any_element(),
                }
            },
        )
        .child(super::client::bar_separator(cx))
        .button(script_button(bar.script, this.clone()).joined(Joined::Next))
        .menu(
            Button::icon("script-menu", Icon::ChevronDown, "Script options")
                .variant(Variant::Ghost)
                .size(Size::Small)
                .joined(Joined::Previous)
                .hide_tooltip(),
            move |_, cx| script_menu(has_inputs, cx),
        )
        .child(super::client::drag_area().flex_1())
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
        .button(bar_button("bank", "Bank").disabled(!bar.live).on_click({
            let this = this.clone();
            move |_, _, cx| update(&this, cx, RuffleView::open_bank)
        }))
        .on_dismiss(move |event, window, cx| {
            update(&this, cx, |view, cx| view.bar_dismissed(event, window, cx))
        });
    match bar.toggle_panel {
        Some(trigger) => menubar.toggle(trigger),
        None => menubar,
    }
}

#[cfg(test)]
pub(super) fn preview(inset: Pixels, cx: &App) -> Menubar {
    let bar = Bar {
        inset,
        live: true,
        relogin: false,
        login_captured: true,
        relogin_detail: None,
        attack: false,
        script: ScriptControl::Start(Some("hunt.ts".into())),
        has_inputs: true,
        toggle_panel: None,
    };
    menubar(bar, WeakEntity::new_invalid(), cx)
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
        .on_click(move |_, _, cx| update(&this, cx, RuffleView::toggle_script))
}

fn script_menu(has_inputs: bool, cx: &App) -> Vec<MenuItem> {
    let load = MenuItem::action("Load script…", |window, cx| {
        window.dispatch_action(Box::new(super::LoadScript), cx)
    });
    let load = match shortcuts::keys_for(&super::LoadScript, cx) {
        Some(keys) => load.shortcut(keys),
        None => load,
    };
    vec![
        load,
        MenuItem::action("Edit inputs…", |window, cx| {
            window.dispatch_action(Box::new(EditScriptInputs), cx)
        })
        .disabled(!has_inputs),
    ]
}

fn windows_menu(cx: &App) -> Vec<MenuItem> {
    let open = |label: &'static str, action: Box<dyn gpui::Action>| {
        let keys = shortcuts::keys_for(action.as_ref(), cx);
        let item = MenuItem::action(label, move |window, cx| {
            window.dispatch_action(action.boxed_clone(), cx)
        });
        match keys {
            Some(keys) => item.shortcut(keys),
            None => item,
        }
    };
    vec![
        open("Manager", Box::new(super::OpenManager)),
        open("Combat profiles", Box::new(super::OpenCombatProfiles)),
        MenuItem::separator(),
        open("Environment", Box::new(super::OpenEnvironment)),
        open("Loader/grabber", Box::new(super::OpenLoaderGrabber)),
        open("Packets", Box::new(super::OpenPackets)),
    ]
}

fn attack_panel(
    attack: auto_attack::Settings,
    live: bool,
    this: &WeakEntity<RuffleView>,
    close: &PopoverClose,
    cx: &App,
) -> AnyElement {
    let header = {
        let this = this.clone();
        let switch = Switch::new("auto-attack-enabled", "Auto attack", attack.enabled)
            .trailing()
            .disabled(!live)
            .on_toggle(move |on, _, cx| {
                update(&this, cx, |view, cx| {
                    view.change_auto_attack(|settings| settings.enabled = on, cx)
                })
            });
        let switch = match shortcuts::keys_for(&ToggleAutoAttack, cx) {
            Some(keys) => switch.end_slot(Kbd::new(keys)),
            None => switch,
        };
        div().px(cx.theme().space(2.0)).child(switch)
    };
    let current = attack
        .profile
        .clone()
        .unwrap_or_else(|| profiles::GENERIC.to_owned());
    let profile = {
        let this = this.clone();
        let chosen = attack.profile.clone();
        DropdownMenu::row("profile", "Profile", current, cx).items(move |_, cx| {
            let library = combat_profiles::library(cx);
            let names =
                std::iter::once(None).chain(library.names().map(|name| Some(name.to_owned())));
            names
                .map(|name| {
                    let selected = match (&name, &chosen) {
                        (None, None) => true,
                        (Some(name), Some(chosen)) => profiles::same_name(name, chosen),
                        _ => false,
                    };
                    let label = name.clone().unwrap_or_else(|| profiles::GENERIC.to_owned());
                    MenuItem::radio(
                        label,
                        selected,
                        act(&this, move |view, cx| {
                            let name = name.clone();
                            view.change_auto_attack(|settings| settings.profile = name, cx)
                        }),
                    )
                })
                .collect()
        })
    };
    let edit = {
        let close = close.clone();
        let editing = attack.profile;
        Button::new("edit-profiles", "Edit profiles…")
            .variant(Variant::Ghost)
            .w_full()
            .child(div().flex_1().min_w_0().truncate().child("Edit profiles…"))
            .on_click(move |_, window, cx| {
                close.close(window, cx);
                super::open_combat_profiles(editing.clone(), cx)
            })
    };
    automation_rows(cx)
        .child(header)
        .child(profile)
        .child(divider(cx.theme()))
        .child(edit)
        .into_any_element()
}

fn automation_rows(cx: &App) -> gpui::Div {
    v_flex()
        .w(px(AUTOMATION_WIDTH))
        .max_w_full()
        .gap(cx.theme().space(0.5))
}

fn heading(title: &'static str, cx: &App) -> impl IntoElement {
    let typography = cx.theme().typography;
    div()
        .text_caption(typography)
        .font_weight(FontWeight::MEDIUM)
        .text_color(cx.theme().muted_foreground)
        .child(title)
}

fn note(text: &'static str, cx: &App) -> AnyElement {
    let layout = cx.theme().layout;
    let typography = cx.theme().typography;
    div()
        .px(layout.space(2.0))
        .py(layout.space(1.5))
        .text_caption(typography)
        .text_color(cx.theme().muted_foreground)
        .child(text)
        .into_any_element()
}

fn pair(
    layout: mira_ui::theme::Layout,
    left: impl IntoElement,
    right: impl IntoElement,
) -> impl IntoElement {
    div()
        .grid()
        .grid_cols(2)
        .gap(layout.space(COLUMN_GAP))
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
    .size(Size::Small)
    .tooltip("Reset")
    .disabled(disabled)
    .when(disabled, Button::hide_tooltip)
    .on_click(move |_, _, cx| update(&this, cx, |view, cx| view.reset(setting, cx)))
}

fn options_panel(
    options: Options,
    this: &WeakEntity<RuffleView>,
    close: &PopoverClose,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let layout = cx.theme().layout;
    let Options {
        automation,
        live,
        name,
        guild,
        inputs,
    } = options;
    let mut settings = automation.game_settings();
    let mode = settings.rendering.mode();
    let mut switch = |option: GameOption, cx: &App| {
        let this = this.clone();
        let switch = Switch::new(
            SharedString::from(format!("{option:?}")),
            option.label(),
            *option.field(&mut settings),
        )
        .disabled(!live)
        .on_toggle(move |on, _, cx| {
            update(&this, cx, |view, cx| {
                view.change_game_settings(|settings| *option.field(settings) = on, cx)
            })
        });
        let switch = switch.trailing();
        let switch = match shortcuts::keys_for(&ToggleOption(option), cx) {
            Some(keys) => switch.end_slot(Kbd::new(keys)),
            None => switch,
        };
        div().min_w_0().child(switch).into_any_element()
    };
    let rendering = {
        let this = this.clone();
        DropdownMenu::row("rendering", "Rendering", mode.label(), cx)
            .disabled(!live)
            .items(move |_, cx| {
                RenderingMode::ALL
                    .map(|choice| {
                        let this = this.clone();
                        let item = MenuItem::radio(choice.label(), choice == mode, move |_, cx| {
                            update(&this, cx, |view, cx| {
                                view.change_game_settings(
                                    |settings| settings.rendering.set(choice),
                                    cx,
                                )
                            })
                        });
                        let keys = match choice {
                            RenderingMode::Full => None,
                            RenderingMode::InterfaceOnly => {
                                shortcuts::keys_for(&ToggleInterfaceRendering, cx)
                            }
                            RenderingMode::Minimal => {
                                shortcuts::keys_for(&ToggleMinimalRendering, cx)
                            }
                        };
                        match keys {
                            Some(keys) => item.shortcut(keys),
                            None => item,
                        }
                    })
                    .into()
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
            .min_w_0()
            .min_h(layout.control)
            .justify_between()
            .gap(layout.space(2.0))
            .child(row_label(setting.label(), enabled))
            .child(
                h_flex()
                    .flex_none()
                    .gap(layout.space(0.5))
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
    let mut walk_speed = Some(chip(Setting::WalkSpeed, window, cx));
    let mut frame_rate = Some(chip(Setting::FrameRate, window, cx));
    let mut rendering = Some(rendering);
    let mut cell = |cell: Gameplay, cx: &App| match cell {
        Gameplay::Option(option) => switch(option, cx),
        Gameplay::Rendering => div()
            .min_w_0()
            .child(div().mx(-layout.space(2.0)).children(rendering.take()))
            .into_any_element(),
        Gameplay::WalkSpeed => div()
            .pt(layout.space(1.5))
            .children(walk_speed.take())
            .into_any_element(),
        Gameplay::FrameRate => div()
            .pt(layout.space(1.5))
            .children(frame_rate.take())
            .into_any_element(),
    };
    let cells = GAMEPLAY.map(|each| cell(each, cx));
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
        .w(px(OPTIONS_WIDTH))
        .max_w_full()
        .debug_selector(|| "options-form".into())
        .gap(layout.space(3.0))
        .child(
            v_flex()
                .gap(layout.space(1.0))
                .child(heading("Gameplay", cx))
                .child(
                    div()
                        .grid()
                        .grid_cols(2)
                        .gap_x(layout.space(COLUMN_GAP))
                        .children(cells),
                ),
        )
        .child(
            v_flex()
                .gap(layout.space(2.0))
                .child(heading("Identity", cx))
                .child(pair(layout, custom_name, custom_guild)),
        )
        .child(
            v_flex()
                .gap(layout.space(2.0))
                .child(heading("Actions", cx))
                .child(
                    h_flex()
                        .gap(layout.space(1.5))
                        .child(command("reload-map", "Reload map", Command::ReloadMap))
                        .child(command(
                            "set-spawnpoint",
                            "Set spawnpoint",
                            Command::SetSpawnPoint,
                        )),
                ),
        )
        .into_any_element()
}

fn relogin_panel(
    relogin: Relogin,
    this: &WeakEntity<RuffleView>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let layout = cx.theme().layout;
    let settings = relogin.automation.auto_relogin();
    let mut servers = relogin.servers.clone();
    if let Some(saved) = &settings.server
        && !servers.contains(saved)
    {
        servers.push(saved.clone());
    }
    let theme = cx.theme().clone();
    let typography = theme.typography;
    let header = {
        let this = this.clone();
        let switch = Switch::new("auto-relogin-enabled", "Auto relogin", settings.enabled)
            .trailing()
            .when_some(relogin.status, Switch::description)
            .on_toggle(move |on, _, cx| {
                update(&this, cx, |view, cx| {
                    view.change_auto_relogin(|settings| settings.enabled = on, cx)
                })
            });
        div().px(layout.space(2.0)).child(switch)
    };
    let server = {
        let this = this.clone();
        let current = settings.server.clone();
        let value = current.clone().unwrap_or_else(|| LAST_SERVER.to_owned());
        DropdownMenu::row("server", "Server", value, cx).items(move |_, _| {
            let choices = std::iter::once(None).chain(servers.iter().cloned().map(Some));
            choices
                .map(|server| {
                    let this = this.clone();
                    let label = server.clone().unwrap_or_else(|| LAST_SERVER.to_owned());
                    MenuItem::radio(label, server == current, move |_, cx| {
                        let server = server.clone();
                        update(&this, cx, |view, cx| {
                            view.change_auto_relogin(|settings| settings.server = server, cx)
                        })
                    })
                })
                .collect()
        })
    };
    let row = || h_flex().h(layout.control).px(layout.space(2.0));
    let server_hint = relogin.servers.is_empty().then(|| {
        div()
            .px(layout.space(2.0))
            .pb(layout.space(1.0))
            .text_caption(typography)
            .text_color(theme.muted_foreground)
            .child("Log in to list the other servers")
    });
    let number = |setting: Setting, window: &mut Window, cx: &mut App| {
        let enabled = setting.editable(&relogin.automation);
        row()
            .justify_between()
            .gap(layout.space(2.0))
            .child(row_label(setting.label(), enabled))
            .child(
                h_flex()
                    .flex_none()
                    .gap(layout.space(1.5))
                    .child(div().w(px(RELOGIN_NUMBER_WIDTH)).child(setting_input(
                        setting,
                        &relogin.automation,
                        enabled,
                        &relogin.inputs,
                        window,
                        cx,
                    )))
                    .child(
                        div()
                            .w(layout.space(3.0))
                            .text_color(theme.muted_foreground)
                            .children(setting.unit()),
                    ),
            )
    };
    let defaults = auto_relogin::Settings::default();
    let advanced = window.use_keyed_state("relogin-advanced", cx, |_, _| {
        settings.backoff_seconds != defaults.backoff_seconds
            || settings.backoff_cap_seconds != defaults.backoff_cap_seconds
    });
    let open = *advanced.read(cx);
    let disclosure = Disclosure::new("relogin-advanced-toggle", "Advanced", open).on_toggle(
        move |open, _, cx| {
            advanced.update(cx, |advanced, cx| {
                *advanced = open;
                cx.notify();
            })
        },
    );
    automation_rows(cx)
        .child(header)
        .child(server)
        .children(server_hint)
        .child(number(Setting::Delay, window, cx))
        .child(number(Setting::Retries, window, cx))
        .child(row().child(disclosure))
        .when(open, |rows| {
            rows.child(number(Setting::Backoff, window, cx))
                .child(number(Setting::BackoffCap, window, cx))
        })
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
    let layout = cx.theme().layout;
    let Some(location) = location else {
        return note("Log in to travel", cx);
    };
    let theme = cx.theme().clone();
    let spot = window.use_keyed_state("jump-spot", cx, |_, _| current_spot(&location));
    let typeahead = window.use_keyed_state("jump-typeahead", cx, |_, _| Typeahead::default());
    let start = *spot.read(cx);
    let cells_scroll = window
        .use_keyed_state("jump-cells", cx, |_, _| {
            let scroll = ScrollHandle::new();
            // scroll_to_item is dropped on the first frame, before the list has measured its viewport.
            let centered = layout.control * (start.row as f32 + 0.5 - VISIBLE_CELLS / 2.0);
            scroll.set_offset(point(px(0.0), -centered));
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
            let at = *spot.read(cx);
            let key = typeahead.update(cx, |typeahead, _| {
                jump_key(&location, Some(at), event, typeahead)
            });
            match key {
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
        h_flex()
            .id(id)
            .flex_none()
            .h(layout.control)
            .px(layout.space(1.5))
            .gap(layout.space(1.5))
            .rounded(theme.radius.control)
            .role(Role::MenuItemRadio)
            .aria_label(label.clone())
            .pointer_cursor(&theme)
            .when(*spot.read(cx) == spot_here, |row| {
                row.bg(theme.ghost_hover())
            })
            .child(div().flex_none().w(px(12.0)).when(here, |mark| {
                mark.child(icon(Icon::Check, theme.foreground).size(px(12.0)))
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
            .when(!in_cell, |row| row.text_color(theme.muted_foreground))
        })
        .collect();
    let column = |label| {
        v_flex().flex_1().min_w_0().child(
            div()
                .px(layout.space(1.5))
                .pb(layout.space(0.5))
                .child(heading(label, cx)),
        )
    };
    let (keys, next, previous) = (respond.clone(), respond.clone(), respond);
    h_flex()
        .id("jump")
        .items_start()
        .w(px(300.0))
        .max_w_full()
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
                            .max_h(layout.control * VISIBLE_CELLS)
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
                .mx(layout.space(1.5))
                .bg(theme.border),
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
            keystroke: gpui::Keystroke::parse(key).unwrap().with_simulated_ime(),
            is_held: false,
            prefer_character_input: false,
        };
        jump_key(location, at, &event, &mut Typeahead::default())
    }

    #[test]
    fn jump_moves_between_cells_and_pads_from_the_keyboard() {
        let location = Location {
            map: "battleon".into(),
            room: None,
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
        rows: bool,
        panel: Panel,
    }

    impl gpui::Render for Scene {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let theme = cx.theme().clone();
            let layout = theme.layout;
            let typography = theme.typography;
            let panel = self.panel.clone();
            let entry = Button::new("entry", self.title)
                .variant(Variant::Ghost)
                .size(Size::Small);
            let content = move |close: &PopoverClose, window: &mut Window, cx: &mut App| {
                panel(close, window, cx)
            };
            let bar = Menubar::new("bar", "Game")
                .h(cx.theme().layout.chrome)
                .px(layout.space(1.5))
                .border_b_1()
                .border_color(theme.border);
            let bar = if self.rows {
                bar.row_popover(entry, content)
            } else {
                bar.popover(entry, content)
            };
            div()
                .key_context(mira_ui::FOCUS_SCOPE)
                .size_full()
                .bg(theme.background)
                .text_color(theme.foreground)
                .font_family(theme.font_family.clone())
                .text_body(typography)
                .child(bar.default_open(0))
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
        options_scene_with(automation, inputs, true)
    }

    fn options_scene_with(automation: Automation, inputs: Inputs, live: bool) -> Scene {
        Scene {
            title: "Options",
            rows: false,
            panel: Rc::new(move |close, window, cx| {
                let options = Options {
                    live,
                    ..in_game_options(&automation, &inputs)
                };
                options_panel(options, &WeakEntity::new_invalid(), close, window, cx)
            }),
        }
    }

    #[gpui::test]
    fn options_form_fits_inside_its_panel(cx: &mut gpui::TestAppContext) {
        cx.update(mira_ui::init);
        let (_, cx) = cx.add_window_view(|_, cx| options_fixture(RenderingMode::Full, cx));
        cx.simulate_resize(gpui::size(px(500.0), px(520.0)));
        cx.run_until_parked();
        let panel = cx.debug_bounds("popover-panel").unwrap();
        let form = cx.debug_bounds("options-form").unwrap();
        assert_eq!(form.size.width, px(OPTIONS_WIDTH), "two-column form width");
        let inset = form.left() - panel.left();
        assert!(inset > Pixels::ZERO);
        assert_eq!(panel.right() - form.right(), inset, "inline form padding");
    }

    #[gpui::test]
    fn automation_popovers_keep_focus_rings_clear_of_their_neighbors(
        cx: &mut gpui::TestAppContext,
    ) {
        cx.update(mira_ui::init);
        let (row, gap, inset, divider, described) = (28.0, 2.0, 4.0, 9.0, 46.0);
        let (_, attack) =
            cx.add_window_view(|_, cx| attack_fixture("attack-rows", &["Farming"], true, cx));
        attack.run_until_parked();
        assert_eq!(
            attack.debug_bounds("popover-panel").unwrap().size.height,
            px(inset + row + gap + row + gap + divider + gap + row + inset),
            "Auto attack, Profile, a divider and Edit profiles…"
        );
        let (_, relogin) =
            cx.add_window_view(|_, cx| relogin_fixture(false, vec!["Twilly".into()], cx));
        relogin.run_until_parked();
        assert_eq!(
            relogin.debug_bounds("popover-panel").unwrap().size.height,
            px(inset + described + (gap + row) * 4.0 + inset),
            "Auto relogin with its status, then Server, Delay, Retries and Advanced"
        );
    }

    #[gpui::test]
    fn toggling_an_automation_keeps_its_trigger_width(cx: &mut gpui::TestAppContext) {
        struct Triggers;

        impl gpui::Render for Triggers {
            fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
                let hug = |on: bool| {
                    div()
                        .flex_none()
                        .debug_selector(move || if on { "attack-on" } else { "attack-off" }.into())
                        .child(trigger("auto-attack", "Auto attack", None, Some(on), cx))
                };
                gpui::div()
                    .flex()
                    .items_start()
                    .child(hug(true))
                    .child(hug(false))
            }
        }

        cx.update(mira_ui::init);
        let (_, cx) = cx.add_window_view(|_, _| Triggers);
        cx.run_until_parked();
        let mut width = |selector| cx.debug_bounds(selector).unwrap().size.width;
        assert_eq!(width("attack-on"), width("attack-off"));
    }

    #[gpui::test]
    fn relogin_numbers_keep_up_down_and_escape_before_the_popover(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            mira_ui::init(cx);
            cx.set_reduce_motion(true);
        });
        let inputs = cx.update(Inputs::new);
        let (_, cx) = cx.add_window_view(|window, _| {
            window.activate_window();
            relogin_scene(false, vec!["Twilly".into()], inputs.clone())
        });
        cx.run_until_parked();
        let keys = |keys: &[&str], cx: &mut gpui::VisualTestContext| {
            for key in keys {
                cx.simulate_keystrokes(key);
                cx.run_until_parked();
            }
        };
        let focused = |setting: Setting, cx: &mut gpui::VisualTestContext| {
            let handle = cx.update(|_, cx| inputs.get(setting).focus_handle(cx));
            cx.update(|window, _| handle.is_focused(window))
        };
        let delay = inputs.get(Setting::Delay).clone();
        keys(&["down", "down", "down", "up", "up", "down"], cx);
        assert!(focused(Setting::Delay, cx));
        assert_eq!(delay.read_with(cx, |input, _| input.text().to_owned()), "4");

        keys(&["escape"], cx);
        assert_eq!(delay.read_with(cx, |input, _| input.text().to_owned()), "3");
        assert!(!focused(Setting::Delay, cx), "Escape leaves the field");
        assert!(
            cx.debug_bounds("popover-panel").is_some(),
            "and keeps the popover"
        );
        keys(&["down"], cx);
        assert!(
            focused(Setting::Retries, cx),
            "Down continues from the field it left"
        );
        keys(&["escape", "shift-tab"], cx);
        assert!(
            focused(Setting::Delay, cx),
            "Shift-Tab continues from it too"
        );
        keys(&["escape", "escape"], cx);
        assert!(cx.debug_bounds("popover-panel").is_none());
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

        tab_from(Setting::CustomName, "shift-tab", cx);
        assert!(focused(Setting::FrameRate, cx));

        automation.update_game_settings(|settings| settings.rendering.set(RenderingMode::Minimal));
        cx.update(|window, _| window.refresh());
        cx.run_until_parked();
        tab_from(Setting::CustomName, "shift-tab", cx);
        assert!(!focused(Setting::FrameRate, cx));
        assert!(!focused(Setting::CustomName, cx));
        cx.simulate_keystrokes("tab");
        cx.run_until_parked();
        assert!(focused(Setting::CustomName, cx));
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
        for path in mira_ui::snapshot(name, gpui::size(px(500.0), px(height)), move |_, cx| {
            let scene = scene(cx);
            cx.new(|_| scene)
        }) {
            assert!(path.exists(), "{} was not written", path.display());
        }
    }

    fn options_fixture(rendering: RenderingMode, cx: &mut App) -> Scene {
        options_fixture_with(rendering, true, cx)
    }

    fn options_fixture_with(rendering: RenderingMode, live: bool, cx: &mut App) -> Scene {
        crate::settings::init(
            std::env::temp_dir().join("mira-options-snapshot-settings.json"),
            |_| {},
            cx,
        );
        shortcuts::apply(cx);
        cx.bind_keys([
            gpui::KeyBinding::new(
                "ctrl-alt-shift-cmd-k",
                ToggleOption(GameOption::SkipCutscenes),
                None,
            ),
            gpui::KeyBinding::new(
                "ctrl-alt-shift-cmd-e",
                ToggleOption(GameOption::EnemyMagnet),
                None,
            ),
            gpui::KeyBinding::new("ctrl-alt-shift-cmd-u", ToggleInterfaceRendering, None),
        ]);
        let automation = Automation::default();
        automation.set_frame_rate(30);
        automation.update_game_settings(|settings| {
            settings.hide_players = true;
            settings.skip_cutscenes = true;
            settings.walk_speed = 12;
            settings.rendering.set(rendering);
        });
        options_scene_with(automation, Inputs::new(cx), live)
    }

    #[test]
    fn snapshot_options() {
        snapshot("game-options", 520.0, |cx| {
            options_fixture(RenderingMode::Full, cx)
        });
        snapshot("game-options-minimal", 520.0, |cx| {
            options_fixture(RenderingMode::Minimal, cx)
        });
        snapshot("game-options-offline", 520.0, |cx| {
            options_fixture_with(RenderingMode::InterfaceOnly, false, cx)
        });
        mira_ui::snapshot_with(
            "game-options-rendering",
            gpui::size(px(800.0), px(520.0)),
            |_, cx| {
                let scene = options_fixture(RenderingMode::Full, cx);
                cx.new(|_| scene)
            },
            |cx, window| {
                for x in [340.0, 341.0] {
                    cx.update_window(window, |_, window, cx| {
                        window.dispatch_event(
                            gpui::PlatformInput::MouseMove(gpui::MouseMoveEvent {
                                position: gpui::point(px(x), px(110.0)),
                                ..Default::default()
                            }),
                            cx,
                        );
                    })
                    .unwrap();
                    cx.run_until_parked();
                }
            },
        );
    }

    struct WindowsMenu;

    impl gpui::Render for WindowsMenu {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let theme = cx.theme().clone();
            let layout = theme.layout;
            let typography = theme.typography;
            div()
                .size_full()
                .bg(theme.background)
                .text_color(theme.foreground)
                .font_family(theme.font_family.clone())
                .text_body(typography)
                .child(
                    Menubar::new("bar", "Game")
                        .h(cx.theme().layout.chrome)
                        .px(layout.space(1.5))
                        .menu(trigger("windows", "Windows", None, None, cx), |_, cx| {
                            windows_menu(cx)
                        })
                        .default_open(0),
                )
        }
    }

    #[test]
    fn snapshot_windows_menu() {
        let settings = std::env::temp_dir().join("mira-windows-menu-snapshot-settings.json");
        for path in mira_ui::snapshot(
            "game-windows-menu",
            gpui::size(px(320.0), px(220.0)),
            move |_, cx| {
                crate::settings::init(settings.clone(), |_| {}, cx);
                shortcuts::apply(cx);
                cx.new(|_| WindowsMenu)
            },
        ) {
            assert!(path.exists(), "{} was not written", path.display());
        }
    }

    fn attack_fixture(name: &str, names: &[&str], live: bool, cx: &mut App) -> Scene {
        let mut library = profiles::Library::default();
        for name in names {
            library.put(None, (*name).into(), profiles::Profile::generic());
        }
        let path = std::env::temp_dir().join(format!("mira-{name}-snapshot-profiles.json"));
        std::fs::write(&path, library.to_json()).unwrap();
        combat_profiles::init(path, cx);
        cx.bind_keys([gpui::KeyBinding::new("alt-a", ToggleAutoAttack, None)]);
        Scene {
            title: "Auto attack",
            rows: true,
            panel: Rc::new(move |close, _, cx| {
                let attack = auto_attack::Settings {
                    enabled: live,
                    profile: Some("Ultra speaker".into()),
                };
                attack_panel(attack, live, &WeakEntity::new_invalid(), close, cx)
            }),
        }
    }

    #[test]
    fn snapshot_attack() {
        let few: &[&str] = &["Daily boss", "Farming", "Ultra speaker"];
        let many: &[&str] = &[
            "Champion drakath",
            "Daily boss",
            "Farming",
            "Grim challenge",
            "Legion farming",
            "Ultra dage",
            "Ultra drago",
            "Ultra engineer",
            "Ultra speaker",
            "Ultra warden",
            "Void solo",
        ];
        snapshot("game-attack", 240.0, move |cx| {
            attack_fixture("game-attack", few, true, cx)
        });
        snapshot("game-attack-offline", 240.0, move |cx| {
            attack_fixture("game-attack-offline", few, false, cx)
        });
        let cases: [(&str, &[&str], &[&str], f32); 3] = [
            ("game-attack-profiles", few, &[], 240.0),
            ("game-attack-profiles-many", many, &[], 480.0),
            ("game-attack-profiles-typed", many, &["u"], 480.0),
        ];
        for (name, names, keys, height) in cases {
            mira_ui::snapshot_with(
                name,
                gpui::size(px(560.0), px(height)),
                move |_, cx| {
                    let scene = attack_fixture(name, names, true, cx);
                    cx.new(|_| scene)
                },
                move |cx, window| {
                    cx.update_window(window, |_, window, cx| {
                        window.dispatch_event(
                            gpui::PlatformInput::MouseDown(MouseDownEvent {
                                button: gpui::MouseButton::Left,
                                position: gpui::point(px(150.0), px(ATTACK_PROFILE_Y)),
                                click_count: 1,
                                ..Default::default()
                            }),
                            cx,
                        );
                    })
                    .unwrap();
                    cx.run_until_parked();
                    for key in keys {
                        cx.update_window(window, |_, window, cx| {
                            window.dispatch_keystroke(Keystroke::parse(key).unwrap(), cx);
                        })
                        .unwrap();
                        cx.run_until_parked();
                    }
                },
            );
        }
        let rows: [(&str, Option<f32>, &[&str]); 3] = [
            ("game-attack-hover", Some(ATTACK_SWITCH_Y), &[]),
            ("game-attack-focus", None, &["down"]),
            ("game-attack-focus-profile", None, &["down", "down"]),
        ];
        for (name, hover, keys) in rows {
            mira_ui::snapshot_with(
                name,
                gpui::size(px(500.0), px(160.0)),
                move |_, cx| {
                    let scene = attack_fixture(name, few, true, cx);
                    cx.new(|_| scene)
                },
                move |cx, window| {
                    for x in hover.map_or(Vec::new(), |_| vec![150.0, 151.0]) {
                        cx.update_window(window, |_, window, cx| {
                            window.dispatch_event(
                                gpui::PlatformInput::MouseMove(gpui::MouseMoveEvent {
                                    position: gpui::point(px(x), px(hover.unwrap_or_default())),
                                    ..Default::default()
                                }),
                                cx,
                            );
                        })
                        .unwrap();
                        cx.run_until_parked();
                    }
                    for key in keys {
                        cx.update_window(window, |_, window, cx| {
                            window.dispatch_keystroke(Keystroke::parse(key).unwrap(), cx);
                        })
                        .unwrap();
                        cx.run_until_parked();
                    }
                },
            );
        }
    }

    const ATTACK_SWITCH_Y: f32 = 56.0;
    const ATTACK_PROFILE_Y: f32 = 84.0;

    fn relogin_fixture(tuned: bool, servers: Vec<String>, cx: &mut App) -> Scene {
        relogin_scene(tuned, servers, Inputs::new(cx))
    }

    fn relogin_scene(tuned: bool, servers: Vec<String>, inputs: Inputs) -> Scene {
        let phase = if tuned {
            Phase::Armed
        } else {
            Phase::Attempt { attempt: 2, of: 3 }
        };
        relogin_scene_in(phase, tuned, servers, inputs)
    }

    fn relogin_scene_in(phase: Phase, tuned: bool, servers: Vec<String>, inputs: Inputs) -> Scene {
        let automation = Automation::default();
        automation.update_auto_relogin(|settings| {
            settings.enabled = true;
            if tuned {
                settings.backoff_seconds = 10;
            } else {
                settings.server = Some("Twilly".into());
            }
        });
        Scene {
            title: "Auto relogin",
            rows: true,
            panel: Rc::new(move |_, window, cx| {
                let relogin = Relogin {
                    automation: automation.clone(),
                    status: status(&phase),
                    servers: servers.clone(),
                    inputs: inputs.clone(),
                };
                relogin_panel(relogin, &WeakEntity::new_invalid(), window, cx)
            }),
        }
    }

    #[test]
    fn snapshot_relogin() {
        let few = || ["Artix", "Twilly", "Yorumi"].map(String::from).to_vec();
        snapshot("game-relogin", 400.0, move |cx| {
            relogin_fixture(false, few(), cx)
        });
        snapshot("game-relogin-advanced", 400.0, |cx| {
            relogin_fixture(true, Vec::new(), cx)
        });
        snapshot("game-relogin-stopped", 400.0, move |cx| {
            let failed = Phase::Failed("login didn't reach server selection".into());
            relogin_scene_in(failed, false, few(), Inputs::new(cx))
        });
        for (name, keys) in [
            ("game-relogin-focus-server", &["down", "down"][..]),
            ("game-relogin-focus-delay", &["down", "down", "down"][..]),
        ] {
            mira_ui::snapshot_after(
                name,
                gpui::size(px(500.0), px(260.0)),
                keys,
                move |_, cx| {
                    let scene = relogin_fixture(false, few(), cx);
                    cx.new(|_| scene)
                },
            );
        }
        let many = || {
            [
                "Alteon",
                "Artix",
                "Espada",
                "Galanoth",
                "Gravelyn",
                "Safiria",
                "Sepulchure",
                "Sir Ver",
                "Swordhaven (EU)",
                "Twig",
                "Twilly",
                "Yokai (SEA)",
                "Yorumi",
            ]
            .map(String::from)
            .to_vec()
        };
        mira_ui::snapshot_with(
            "game-relogin-servers",
            gpui::size(px(560.0), px(460.0)),
            move |_, cx| {
                let scene = relogin_fixture(false, many(), cx);
                cx.new(|_| scene)
            },
            |cx, window| {
                cx.update_window(window, |_, window, cx| {
                    window.dispatch_event(
                        gpui::PlatformInput::MouseDown(MouseDownEvent {
                            button: gpui::MouseButton::Left,
                            position: gpui::point(px(150.0), px(102.0)),
                            click_count: 1,
                            ..Default::default()
                        }),
                        cx,
                    );
                })
                .unwrap();
                cx.run_until_parked();
            },
        );
    }

    struct Bars(fn() -> Vec<Bar>, WeakEntity<RuffleView>);

    impl gpui::Render for Bars {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let theme = cx.theme().clone();
            let layout = theme.layout;
            let typography = theme.typography;
            v_flex()
                .size_full()
                .gap(layout.space(2.0))
                .bg(theme.card)
                .text_color(theme.foreground)
                .font_family(theme.font_family.clone())
                .text_body(typography)
                .children((self.0)().into_iter().enumerate().map(|(index, bar)| {
                    div()
                        .id(("bar", index))
                        .child(menubar(bar, self.1.clone(), cx))
                }))
        }
    }

    #[test]
    fn snapshot_bar() {
        for path in mira_ui::snapshot("game-bar", gpui::size(px(900.0), px(300.0)), |_, cx| {
            cx.new(|_| {
                Bars(
                    || {
                        let bar = |relogin, relogin_detail, attack, script| Bar {
                            inset: px(84.0),
                            live: true,
                            relogin,
                            login_captured: true,
                            relogin_detail,
                            attack,
                            script,
                            has_inputs: true,
                            toggle_panel: None,
                        };
                        let now = Instant::now();
                        let countdown = Countdown {
                            from: now - std::time::Duration::from_secs(2),
                            until: now + std::time::Duration::from_secs(3),
                        };
                        vec![
                            bar(false, None, false, ScriptControl::Start(None)),
                            bar(
                                true,
                                None,
                                true,
                                ScriptControl::Start(Some("hunt.ts".into())),
                            ),
                            bar(
                                true,
                                Some(Detail::Counting("Waiting".into(), countdown)),
                                true,
                                ScriptControl::Stop("hunt.ts".into()),
                            ),
                            bar(
                                true,
                                Some(Detail::Working("Attempt 2 of 3".into())),
                                true,
                                ScriptControl::Stop("hunt.ts".into()),
                            ),
                            bar(
                                true,
                                Some(Detail::Stopped),
                                false,
                                ScriptControl::Start(Some("hunt.ts".into())),
                            ),
                            Bar {
                                live: false,
                                login_captured: false,
                                ..bar(false, None, false, ScriptControl::Start(None))
                            },
                        ]
                    },
                    WeakEntity::new_invalid(),
                )
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
                room: None,
                cell: "r16".into(),
                pad: "Left".into(),
                cells: (1..=24).map(|cell| format!("r{cell}")).collect(),
                cell_pads: vec!["Left".into(), "Right".into()],
            };
            Scene {
                title: "Jump",
                rows: false,
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
