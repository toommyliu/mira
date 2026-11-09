use serde::Deserialize;

const HIGH_TIER_LEVEL: i32 = 50;

const FIGHTER: i32 = 2;
const WIZARD: i32 = 6;
const FORGE: i32 = 10;
const DEPTHS: i32 = 23;

const FORGE_WEAPON_SHOP: i32 = 2142;
const FORGE_CAPE_SHOP: i32 = 2143;
const FORGE_HELM_SHOP: i32 = 2164;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Slot {
    Weapon,
    Class,
    Helm,
    Cape,
}

impl Slot {
    pub(super) fn of(equipment_slot: &str) -> Option<Self> {
        match equipment_slot {
            "Weapon" => Some(Slot::Weapon),
            "ar" => Some(Slot::Class),
            "he" => Some(Slot::Helm),
            "ba" => Some(Slot::Cape),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Kind {
    pub(super) pattern_id: i32,
    pub(super) proc_id: i32,
}

impl Kind {
    const fn pattern(pattern_id: i32) -> Self {
        Kind {
            pattern_id,
            proc_id: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shop {
    Basic { low: i32, high: i32 },
    Museum(i32),
    Forge(i32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Strategy {
    pub(super) kind: Kind,
    shop: Shop,
}

impl Strategy {
    fn basic(basic: &Basic) -> Self {
        Strategy {
            kind: Kind::pattern(basic.pattern_id),
            shop: Shop::Basic {
                low: basic.low,
                high: basic.high,
            },
        }
    }

    fn forge(shop_id: i32, kind: Kind) -> Self {
        Strategy {
            kind,
            shop: Shop::Forge(shop_id),
        }
    }

    pub(super) fn shop_id(&self, player_level: i32) -> i32 {
        match self.shop {
            Shop::Basic { low, high } => {
                if player_level >= HIGH_TIER_LEVEL {
                    high
                } else {
                    low
                }
            }
            Shop::Museum(shop_id) | Shop::Forge(shop_id) => shop_id,
        }
    }

    pub(super) fn map(&self) -> Option<&'static str> {
        match self.shop {
            Shop::Basic { .. } => None,
            Shop::Museum(_) => Some("museum"),
            Shop::Forge(_) => Some("forge"),
        }
    }
}

struct Basic {
    name: &'static str,
    pattern_id: i32,
    low: i32,
    high: i32,
    awe: Option<i32>,
}

const BASICS: &[Basic] = &[
    Basic {
        name: "fighter",
        pattern_id: FIGHTER,
        low: 141,
        high: 768,
        awe: Some(635),
    },
    Basic {
        name: "thief",
        pattern_id: 3,
        low: 142,
        high: 767,
        awe: Some(637),
    },
    Basic {
        name: "hybrid",
        pattern_id: 5,
        low: 143,
        high: 766,
        awe: Some(633),
    },
    Basic {
        name: "wizard",
        pattern_id: WIZARD,
        low: 144,
        high: 765,
        awe: Some(636),
    },
    Basic {
        name: "healer",
        pattern_id: 7,
        low: 145,
        high: 762,
        awe: Some(638),
    },
    Basic {
        name: "spellbreaker",
        pattern_id: 8,
        low: 146,
        high: 764,
        awe: None,
    },
    Basic {
        name: "lucky",
        pattern_id: 9,
        low: 147,
        high: 763,
        awe: Some(639),
    },
];

#[derive(Clone, Copy)]
enum Family {
    Awe,
    Forge { pattern_id: i32 },
}

struct WeaponSpecial {
    names: &'static [&'static str],
    proc_id: i32,
    family: Family,
}

const WEAPON_SPECIALS: &[WeaponSpecial] = &[
    WeaponSpecial {
        names: &["spiral carve", "scarve", "spiral"],
        proc_id: 2,
        family: Family::Awe,
    },
    WeaponSpecial {
        names: &["awe blast", "ablast", "aweblast", "blast"],
        proc_id: 3,
        family: Family::Awe,
    },
    WeaponSpecial {
        names: &["health vamp", "healthvamp", "hvamp", "hp vamp"],
        proc_id: 4,
        family: Family::Awe,
    },
    WeaponSpecial {
        names: &["mana vamp", "manavamp", "mvamp", "mp vamp"],
        proc_id: 5,
        family: Family::Awe,
    },
    WeaponSpecial {
        names: &["powerword die", "powerword", "pwd", "pw die"],
        proc_id: 6,
        family: Family::Awe,
    },
    WeaponSpecial {
        names: &["lacerate", "lac"],
        proc_id: 7,
        family: Family::Forge { pattern_id: FORGE },
    },
    WeaponSpecial {
        names: &["smite"],
        proc_id: 8,
        family: Family::Forge { pattern_id: FORGE },
    },
    WeaponSpecial {
        names: &["valiance", "val", "vali"],
        proc_id: 9,
        family: Family::Forge { pattern_id: FORGE },
    },
    WeaponSpecial {
        names: &[
            "arcana's concerto",
            "arcanas",
            "arcana concerto",
            "concerto",
        ],
        proc_id: 10,
        family: Family::Forge { pattern_id: FORGE },
    },
    WeaponSpecial {
        names: &["acheron", "ach"],
        proc_id: 11,
        family: Family::Forge { pattern_id: DEPTHS },
    },
    WeaponSpecial {
        names: &["elysium", "ely"],
        proc_id: 12,
        family: Family::Forge { pattern_id: WIZARD },
    },
    WeaponSpecial {
        names: &["praxis", "prax"],
        proc_id: 13,
        family: Family::Forge { pattern_id: FORGE },
    },
    WeaponSpecial {
        names: &["dauntless", "dtl"],
        proc_id: 14,
        family: Family::Forge {
            pattern_id: FIGHTER,
        },
    },
    WeaponSpecial {
        names: &["ravenous", "rav"],
        proc_id: 15,
        family: Family::Forge { pattern_id: FORGE },
    },
];

const FORGE_WEAPON_PATTERNS: &[(&str, i32)] = &[("forge", FORGE), ("depths", DEPTHS)];

const CAPES: &[(&[&str], i32)] = &[
    (&["forge"], FORGE),
    (&["absolution", "abso"], 11),
    (&["avarice", "ava"], 12),
    (&["vainglory"], 24),
    (&["penitence", "peni"], 29),
    (&["lament", "lam"], 30),
];

struct Helm {
    name: &'static str,
    inventory_name: &'static str,
    special: &'static str,
    pattern_id: i32,
}

const HELMS: &[Helm] = &[
    Helm {
        name: "vim",
        inventory_name: "vim",
        special: "ether",
        pattern_id: 25,
    },
    Helm {
        name: "examen",
        inventory_name: "examen",
        special: "ether",
        pattern_id: 26,
    },
    Helm {
        name: "pneuma",
        inventory_name: "pneuma",
        special: "clairvoyance",
        pattern_id: 27,
    },
    Helm {
        name: "anima",
        inventory_name: "anima",
        special: "clairvoyance",
        pattern_id: 28,
    },
    Helm {
        name: "hearty",
        inventory_name: "grimskull",
        special: "hearty",
        pattern_id: 32,
    },
];

fn normalize(label: &str) -> String {
    label
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
        .replace(['\u{2018}', '\u{2019}'], "'")
}

fn weapon_special(label: &str) -> Option<&'static WeaponSpecial> {
    WEAPON_SPECIALS
        .iter()
        .find(|special| special.names.contains(&label))
}

fn forge_weapon_pattern(label: &str) -> Option<i32> {
    FORGE_WEAPON_PATTERNS
        .iter()
        .find(|(name, _)| *name == label)
        .map(|(_, pattern_id)| *pattern_id)
}

fn cape_pattern(label: &str) -> Option<i32> {
    CAPES
        .iter()
        .find(|(names, _)| names.contains(&label))
        .map(|(_, pattern_id)| *pattern_id)
}

pub(super) fn is_plain_forge(enhancement: &str, special: Option<&str>) -> bool {
    normalize(enhancement) == "forge" && special.is_none_or(|special| normalize(special).is_empty())
}

pub(super) fn resolve(slot: Slot, enhancement: &str, special: Option<&str>) -> Option<Strategy> {
    let enhancement = normalize(enhancement);
    let special = special.map(normalize).filter(|special| !special.is_empty());
    let special = special.as_deref();
    let basic = BASICS.iter().find(|basic| basic.name == enhancement);
    match slot {
        Slot::Class => basic.filter(|_| special.is_none()).map(Strategy::basic),
        Slot::Weapon => weapon(&enhancement, special, basic),
        Slot::Helm => helm(&enhancement, special, basic),
        Slot::Cape => cape(&enhancement, special, basic),
    }
}

fn weapon(enhancement: &str, special: Option<&str>, basic: Option<&Basic>) -> Option<Strategy> {
    let forge = |pattern_id, proc_id| {
        Strategy::forge(
            FORGE_WEAPON_SHOP,
            Kind {
                pattern_id,
                proc_id,
            },
        )
    };
    let Some(special) = special else {
        if let Some(basic) = basic {
            return Some(Strategy::basic(basic));
        }
        return match weapon_special(enhancement) {
            Some(WeaponSpecial {
                proc_id,
                family: Family::Forge { pattern_id },
                ..
            }) => Some(forge(*pattern_id, *proc_id)),
            _ => (enhancement == "forge").then(|| forge(FORGE, 0)),
        };
    };
    let special = weapon_special(special)?;
    match special.family {
        Family::Awe => {
            let basic = basic?;
            Some(Strategy {
                kind: Kind {
                    pattern_id: basic.pattern_id,
                    proc_id: special.proc_id,
                },
                shop: Shop::Museum(basic.awe?),
            })
        }
        Family::Forge { pattern_id } => {
            let labelled = enhancement == "forge"
                || basic
                    .map(|basic| basic.pattern_id)
                    .or_else(|| forge_weapon_pattern(enhancement))
                    == Some(pattern_id);
            labelled.then(|| forge(pattern_id, special.proc_id))
        }
    }
}

fn helm(enhancement: &str, special: Option<&str>, basic: Option<&Basic>) -> Option<Strategy> {
    let helm = match special {
        None => {
            if let Some(basic) = basic {
                return Some(Strategy::basic(basic));
            }
            if enhancement == "forge" {
                return Some(Strategy::forge(FORGE_HELM_SHOP, Kind::pattern(FORGE)));
            }
            HELMS.iter().find(|helm| helm.name == enhancement)
        }
        Some(special) if enhancement == "forge" => HELMS.iter().find(|helm| helm.name == special),
        Some(special) => HELMS
            .iter()
            .find(|helm| helm.inventory_name == enhancement && helm.special == special),
    }?;
    Some(Strategy::forge(
        FORGE_HELM_SHOP,
        Kind::pattern(helm.pattern_id),
    ))
}

fn cape(enhancement: &str, special: Option<&str>, basic: Option<&Basic>) -> Option<Strategy> {
    let pattern_id = match special {
        None => {
            if let Some(basic) = basic {
                return Some(Strategy::basic(basic));
            }
            cape_pattern(enhancement)?
        }
        Some(special) if enhancement == "forge" => cape_pattern(special)?,
        Some(_) => return None,
    };
    Some(Strategy::forge(FORGE_CAPE_SHOP, Kind::pattern(pattern_id)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolved(
        slot: Slot,
        enhancement: &str,
        special: Option<&str>,
    ) -> Option<(i32, i32, i32, Option<&'static str>)> {
        resolve(slot, enhancement, special).map(|strategy| {
            (
                strategy.kind.pattern_id,
                strategy.kind.proc_id,
                strategy.shop_id(100),
                strategy.map(),
            )
        })
    }

    #[test]
    fn basic_enhancements_use_the_shop_for_the_players_level() {
        let lucky = resolve(Slot::Class, "Lucky", None).unwrap();
        assert_eq!(lucky.kind, Kind::pattern(9));
        assert_eq!(lucky.shop_id(49), 147);
        assert_eq!(lucky.shop_id(50), 763);
        assert_eq!(lucky.map(), None);
        for slot in [Slot::Weapon, Slot::Helm, Slot::Cape] {
            assert_eq!(resolve(slot, "lucky", None), Some(lucky), "{slot:?}");
        }
        assert_eq!(resolve(Slot::Class, "lucky", Some("spiral carve")), None);
        assert_eq!(resolve(Slot::Class, "forge", None), None);
    }

    #[test]
    fn weapon_labels_resolve_to_the_pattern_and_proc_the_inventory_shows() {
        let forge = Some("forge");
        let museum = Some("museum");
        for (enhancement, special, expected) in [
            ("wizard", Some("elysium"), Some((6, 12, 2142, forge))),
            ("forge", Some("elysium"), Some((6, 12, 2142, forge))),
            ("ely", None, Some((6, 12, 2142, forge))),
            ("fighter", Some("dauntless"), Some((2, 14, 2142, forge))),
            ("forge", Some("DTL"), Some((2, 14, 2142, forge))),
            ("acheron", None, Some((23, 11, 2142, forge))),
            ("depths", Some("acheron"), Some((23, 11, 2142, forge))),
            (
                "forge",
                Some("arcana\u{2019}s  concerto"),
                Some((10, 10, 2142, forge)),
            ),
            ("forge", None, Some((10, 0, 2142, forge))),
            ("lucky", Some("spiral carve"), Some((9, 2, 639, museum))),
            ("healer", Some("pwd"), Some((7, 6, 638, museum))),
            ("lucky", Some("dauntless"), None),
            ("spellbreaker", Some("awe blast"), None),
            ("forge", Some("spiral carve"), None),
            ("spiral carve", None, None),
            ("depths", None, None),
            ("lucky", Some("unknown"), None),
        ] {
            assert_eq!(
                resolved(Slot::Weapon, enhancement, special),
                expected,
                "{enhancement} {special:?}"
            );
        }
    }

    #[test]
    fn helm_and_cape_labels_resolve_to_forge_patterns() {
        let forge = Some("forge");
        for (slot, enhancement, special, expected) in [
            (Slot::Helm, "vim", None, Some((25, 0, 2164, forge))),
            (Slot::Helm, "vim", Some("ether"), Some((25, 0, 2164, forge))),
            (Slot::Helm, "forge", Some("vim"), Some((25, 0, 2164, forge))),
            (Slot::Helm, "hearty", None, Some((32, 0, 2164, forge))),
            (
                Slot::Helm,
                "grimskull",
                Some("hearty"),
                Some((32, 0, 2164, forge)),
            ),
            (Slot::Helm, "forge", None, Some((10, 0, 2164, forge))),
            (Slot::Helm, "vim", Some("clairvoyance"), None),
            (Slot::Cape, "vainglory", None, Some((24, 0, 2143, forge))),
            (Slot::Cape, "abso", None, Some((11, 0, 2143, forge))),
            (
                Slot::Cape,
                "forge",
                Some("lament"),
                Some((30, 0, 2143, forge)),
            ),
            (Slot::Cape, "forge", None, Some((10, 0, 2143, forge))),
            (Slot::Cape, "lucky", Some("lament"), None),
        ] {
            assert_eq!(
                resolved(slot, enhancement, special),
                expected,
                "{slot:?} {enhancement} {special:?}"
            );
        }
    }

    #[test]
    fn only_a_bare_forge_label_is_plain_forge() {
        assert!(is_plain_forge(" Forge ", None));
        assert!(is_plain_forge("forge", Some(" ")));
        assert!(!is_plain_forge("forge", Some("vim")));
        assert!(!is_plain_forge("vim", None));
    }
}
