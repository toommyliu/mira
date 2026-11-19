use std::collections::BTreeMap;

use gpui::{Action, App, KeyBinding, Keystroke};
use mira_ui::{components::format_keystroke, keyboard};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::{
    ruffle_view::{self as game, GameOption},
    settings,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Category {
    Scripts,
    Game,
    Options,
    View,
    Windows,
}

impl Category {
    pub(crate) const ALL: [Category; 5] = [
        Category::Scripts,
        Category::Game,
        Category::Options,
        Category::View,
        Category::Windows,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Category::Scripts => "Scripts",
            Category::Game => "Client",
            Category::Options => "Client options",
            Category::View => "View",
            Category::Windows => "Windows",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Shortcut {
    LoadScript,
    StartStopScript,
    AutoAttack,
    Bank,
    Jump,
    OptionsMenu,
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
    MinimalRendering,
    InterfaceOnlyRendering,
    Sidebar,
    GridView,
    Accounts,
    Scripts,
    CombatProfiles,
    Environment,
    Follower,
    LoaderGrabber,
    Packets,
}

pub(crate) type Overrides = BTreeMap<Shortcut, Option<String>>;

impl Shortcut {
    pub(crate) const ALL: [Shortcut; 28] = [
        Shortcut::LoadScript,
        Shortcut::StartStopScript,
        Shortcut::AutoAttack,
        Shortcut::Bank,
        Shortcut::Jump,
        Shortcut::OptionsMenu,
        Shortcut::InfiniteRange,
        Shortcut::ProvokeCell,
        Shortcut::EnemyMagnet,
        Shortcut::HidePlayers,
        Shortcut::SkipCutscenes,
        Shortcut::Animations,
        Shortcut::Collisions,
        Shortcut::Ads,
        Shortcut::AntiCounter,
        Shortcut::AntiAfk,
        Shortcut::AutoZone,
        Shortcut::MinimalRendering,
        Shortcut::InterfaceOnlyRendering,
        Shortcut::Sidebar,
        Shortcut::GridView,
        Shortcut::Accounts,
        Shortcut::Scripts,
        Shortcut::CombatProfiles,
        Shortcut::Environment,
        Shortcut::Follower,
        Shortcut::LoaderGrabber,
        Shortcut::Packets,
    ];

    fn option(self) -> Option<GameOption> {
        Some(match self {
            Shortcut::InfiniteRange => GameOption::InfiniteRange,
            Shortcut::ProvokeCell => GameOption::ProvokeCell,
            Shortcut::EnemyMagnet => GameOption::EnemyMagnet,
            Shortcut::HidePlayers => GameOption::HidePlayers,
            Shortcut::SkipCutscenes => GameOption::SkipCutscenes,
            Shortcut::Animations => GameOption::Animations,
            Shortcut::Collisions => GameOption::Collisions,
            Shortcut::Ads => GameOption::Ads,
            Shortcut::AntiCounter => GameOption::AntiCounter,
            Shortcut::AntiAfk => GameOption::AntiAfk,
            Shortcut::AutoZone => GameOption::AutoZone,
            _ => return None,
        })
    }

    pub(crate) fn category(self) -> Category {
        match self {
            Shortcut::LoadScript | Shortcut::StartStopScript => Category::Scripts,
            Shortcut::AutoAttack | Shortcut::Bank | Shortcut::Jump => Category::Game,
            Shortcut::OptionsMenu
            | Shortcut::MinimalRendering
            | Shortcut::InterfaceOnlyRendering => Category::Options,
            _ if self.option().is_some() => Category::Options,
            Shortcut::Sidebar | Shortcut::GridView => Category::View,
            _ => Category::Windows,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        if let Some(option) = self.option() {
            return option.label();
        }
        match self {
            Shortcut::LoadScript => "Load script",
            Shortcut::StartStopScript => "Start/stop script",
            Shortcut::AutoAttack => "Auto attack",
            Shortcut::Bank => "Bank",
            Shortcut::Jump => "Jump",
            Shortcut::OptionsMenu => "Options menu",
            Shortcut::MinimalRendering => "Minimal rendering",
            Shortcut::InterfaceOnlyRendering => "Interface-only rendering",
            Shortcut::Sidebar => "Sidebar",
            Shortcut::GridView => "Grid view",
            Shortcut::Accounts => "Accounts",
            Shortcut::Scripts => "Scripts",
            Shortcut::CombatProfiles => "Combat profiles",
            Shortcut::Environment => "Environment",
            Shortcut::Follower => "Follower",
            Shortcut::LoaderGrabber => "Loader/grabber",
            Shortcut::Packets => "Packets",
            _ => unreachable!("{self:?} is a game option"),
        }
    }

    fn default_keys(self) -> Option<&'static str> {
        Some(match self {
            Shortcut::LoadScript => "cmd-o",
            Shortcut::StartStopScript => "cmd-r",
            Shortcut::Bank => "cmd-b",
            Shortcut::OptionsMenu => "cmd-shift-,",
            Shortcut::InfiniteRange => "alt-i",
            Shortcut::MinimalRendering => "cmd-l",
            Shortcut::Sidebar => "ctrl-cmd-s",
            Shortcut::GridView => "ctrl-cmd-g",
            _ => return None,
        })
    }

    fn action(self) -> Box<dyn Action> {
        if let Some(option) = self.option() {
            return Box::new(game::ToggleOption(option));
        }
        match self {
            Shortcut::LoadScript => Box::new(game::LoadScript),
            Shortcut::StartStopScript => Box::new(game::ToggleScript),
            Shortcut::AutoAttack => Box::new(game::ToggleAutoAttack),
            Shortcut::Bank => Box::new(game::OpenBank),
            Shortcut::Jump => Box::new(game::OpenJump),
            Shortcut::OptionsMenu => Box::new(game::ToggleOptionsMenu),
            Shortcut::MinimalRendering => Box::new(game::ToggleMinimalRendering),
            Shortcut::InterfaceOnlyRendering => Box::new(game::ToggleInterfaceRendering),
            Shortcut::Sidebar => Box::new(game::ToggleSidebar),
            Shortcut::GridView => Box::new(game::ToggleGrid),
            Shortcut::Accounts => Box::new(game::OpenAccounts),
            Shortcut::Scripts => Box::new(game::OpenScripts),
            Shortcut::CombatProfiles => Box::new(game::OpenCombatProfiles),
            Shortcut::Environment => Box::new(game::OpenEnvironment),
            Shortcut::Follower => Box::new(game::OpenFollower),
            Shortcut::LoaderGrabber => Box::new(game::OpenLoaderGrabber),
            Shortcut::Packets => Box::new(game::OpenPackets),
            _ => unreachable!("{self:?} is a game option"),
        }
    }

    pub(crate) fn keys(self, overrides: &Overrides) -> Option<&str> {
        match overrides.get(&self) {
            Some(chosen) => chosen.as_deref(),
            None => self.default_keys(),
        }
    }

    pub(crate) fn default_keystroke(self) -> Option<Keystroke> {
        Keystroke::parse(self.default_keys()?).ok()
    }

    pub(crate) fn is_default(self, overrides: &Overrides) -> bool {
        !overrides.contains_key(&self)
    }

    fn owns(self, binding: &KeyBinding) -> bool {
        binding.action().partial_eq(self.action().as_ref())
    }
}

#[derive(Debug)]
pub(crate) struct Taken;

pub(crate) fn assign(
    overrides: &mut Overrides,
    shortcut: Shortcut,
    keys: Option<String>,
    taken_by_app: impl Fn(&str) -> bool,
) -> Result<(), Taken> {
    if let Some(keys) = &keys {
        if taken_by_app(keys) {
            return Err(Taken);
        }
        for other in Shortcut::ALL {
            if other != shortcut && other.keys(overrides) == Some(keys) {
                set(overrides, other, None);
            }
        }
    }
    set(overrides, shortcut, keys);
    Ok(())
}

pub(crate) fn serialize<S: Serializer>(overrides: &Overrides, to: S) -> Result<S::Ok, S::Error> {
    to.collect_map(
        Shortcut::ALL
            .iter()
            .map(|&shortcut| (shortcut, shortcut.keys(overrides))),
    )
}

pub(crate) fn deserialize<'de, D: Deserializer<'de>>(from: D) -> Result<Overrides, D::Error> {
    let mut saved = Overrides::deserialize(from)?;
    saved.retain(|shortcut, keys| keys.as_deref() != shortcut.default_keys());
    Ok(saved)
}

fn set(overrides: &mut Overrides, shortcut: Shortcut, keys: Option<String>) {
    if keys.as_deref() == shortcut.default_keys() {
        overrides.remove(&shortcut);
    } else {
        overrides.insert(shortcut, keys);
    }
}

pub(crate) fn apply(cx: &mut App) {
    let os = cx.platform_info().os;
    let overrides = settings::shortcuts(cx);
    let bindings: Vec<_> = Shortcut::ALL
        .iter()
        .filter_map(|&shortcut| {
            let keys = shortcut.keys(overrides)?;
            let typed = Keystroke::parse_with_os(keys, os)
                .map(|keys| keyboard::typed(&keys).unparse(os))
                .unwrap_or_else(|_| keys.to_owned());
            KeyBinding::load(&typed, shortcut.action(), None, false, None, cx)
                .inspect_err(|error| tracing::warn!(%error, keys, "couldn't bind a shortcut"))
                .ok()
        })
        .collect();
    let keymap = cx.key_bindings();
    {
        let mut keymap = keymap.borrow_mut();
        let fixed: Vec<_> = keymap
            .bindings()
            .filter(|binding| !Shortcut::ALL.iter().any(|shortcut| shortcut.owns(binding)))
            .cloned()
            .collect();
        keymap.clear();
        keymap.add_bindings(fixed);
        keymap.add_bindings(bindings);
    }
    cx.refresh_windows();
}

pub(crate) fn taken_by_app(keys: &str, cx: &App) -> bool {
    let Ok(wanted) = Keystroke::parse_with_os(keys, cx.platform_info().os) else {
        return false;
    };
    let wanted = keyboard::typed(&wanted);
    cx.key_bindings().borrow().bindings().any(|binding| {
        !Shortcut::ALL.iter().any(|shortcut| shortcut.owns(binding))
            && matches!(binding.keystrokes(), [only] if same(only.inner(), &wanted))
    })
}

fn same(a: &Keystroke, b: &Keystroke) -> bool {
    a.key == b.key && a.modifiers == b.modifiers
}

pub(crate) fn keys_for(action: &dyn Action, cx: &App) -> Option<gpui::SharedString> {
    let keymap = cx.key_bindings();
    let keymap = keymap.borrow();
    let binding = keymap.bindings_for_action(action).next_back()?;
    Some(format_keystroke(binding.keystrokes().first()?.inner()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shortcut_moves_its_keys_off_another_and_back_to_default_saves_nothing() {
        let mut overrides = Overrides::new();
        let free = |_: &str| false;
        assign(&mut overrides, Shortcut::Bank, Some("cmd-r".into()), free).unwrap();
        assert_eq!(Shortcut::Bank.keys(&overrides), Some("cmd-r"));
        assert_eq!(Shortcut::StartStopScript.keys(&overrides), None);

        assign(&mut overrides, Shortcut::Bank, Some("cmd-b".into()), free).unwrap();
        assign(
            &mut overrides,
            Shortcut::StartStopScript,
            Some("cmd-r".into()),
            free,
        )
        .unwrap();
        assert!(overrides.is_empty());

        assign(
            &mut overrides,
            Shortcut::InterfaceOnlyRendering,
            Some("cmd-i".into()),
            free,
        )
        .unwrap();
        assign(&mut overrides, Shortcut::InterfaceOnlyRendering, None, free).unwrap();
        assert!(overrides.is_empty());
    }

    #[test]
    fn every_shortcut_has_a_label_category_and_its_own_action() {
        for (index, shortcut) in Shortcut::ALL.iter().enumerate() {
            assert!(!shortcut.label().is_empty());
            shortcut.category();
            for other in &Shortcut::ALL[index + 1..] {
                assert!(
                    !shortcut.action().partial_eq(other.action().as_ref()),
                    "{shortcut:?} and {other:?} share an action"
                );
            }
        }
    }

    #[test]
    fn keys_the_app_already_uses_are_refused() {
        let mut overrides = Overrides::new();
        let taken = assign(
            &mut overrides,
            Shortcut::Bank,
            Some("cmd-q".into()),
            |keys| keys == "cmd-q",
        );
        assert!(taken.is_err());
        assert_eq!(Shortcut::Bank.keys(&overrides), Some("cmd-b"));
    }

    #[gpui::test]
    fn saved_shortcuts_replace_the_defaults_they_override(cx: &mut gpui::TestAppContext) {
        let path = std::env::temp_dir().join(format!(
            "mira-shortcuts-{}-{:?}.json",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::write(
            &path,
            r#"{"shortcuts":{"packets":"cmd-shift-k","bank":null,"interfaceOnlyRendering":"cmd-i"}}"#,
        )
        .unwrap();
        cx.update(|cx| {
            settings::init(path.clone(), |_| {}, cx);
            apply(cx);
            apply(cx);
            assert_eq!(keys_for(&game::OpenPackets, cx).as_deref(), Some("⇧⌘K"));
            assert_eq!(keys_for(&game::OpenBank, cx), None);
            assert_eq!(
                keys_for(&game::ToggleInterfaceRendering, cx).as_deref(),
                Some("⌘I")
            );
            assert_eq!(keys_for(&game::ToggleScript, cx).as_deref(), Some("⌘R"));
            assert!(!taken_by_app("cmd-shift-p", cx));
            assert!(taken_by_app("cmd-,", cx));
            let bound = cx.key_bindings().borrow().bindings().count();
            apply(cx);
            assert_eq!(cx.key_bindings().borrow().bindings().count(), bound);
        });
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn every_shortcut_saves_its_keys_under_a_plain_name_and_loads_back_as_changes() {
        let mut overrides = Overrides::new();
        overrides.insert(Shortcut::StartStopScript, Some("cmd-k".into()));
        overrides.insert(Shortcut::Bank, None);
        let saved = serialize(&overrides, serde_json::value::Serializer).unwrap();
        let saved = saved.as_object().unwrap();
        assert_eq!(saved.len(), Shortcut::ALL.len());
        assert_eq!(saved["startStopScript"], "cmd-k");
        assert_eq!(saved["bank"], serde_json::Value::Null);
        assert_eq!(saved["loadScript"], "cmd-o");
        assert_eq!(saved["jump"], serde_json::Value::Null);
        assert_eq!(saved["interfaceOnlyRendering"], serde_json::Value::Null);
        let loaded = deserialize(serde_json::Value::Object(saved.clone())).unwrap();
        assert_eq!(loaded, overrides);
    }
}
