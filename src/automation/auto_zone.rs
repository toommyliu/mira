use std::{ops::RangeInclusive, time::Duration};

use tokio::{
    sync::watch,
    time::{Instant, sleep_until},
};

use super::game_settings;
use crate::bridge::{self, CallError, Loader, Packets};
use crate::domain::{
    auras, player,
    world::{EventArgs, EventPacket},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Room {
    Ledgermayne,
    Moreskulls,
    Ultradage,
    Darkcarnax,
    Astralshrine,
    Queeniona,
    Magnumopus,
}

impl Room {
    const ALL: [Room; 7] = [
        Room::Ledgermayne,
        Room::Moreskulls,
        Room::Ultradage,
        Room::Darkcarnax,
        Room::Astralshrine,
        Room::Queeniona,
        Room::Magnumopus,
    ];

    fn of(map: &str) -> Option<Room> {
        Room::ALL
            .into_iter()
            .find(|room| map.eq_ignore_ascii_case(room.map()))
    }

    fn map(self) -> &'static str {
        match self {
            Room::Ledgermayne => "ledgermayne",
            Room::Moreskulls => "moreskulls",
            Room::Ultradage => "ultradage",
            Room::Darkcarnax => "darkcarnax",
            Room::Astralshrine => "astralshrine",
            Room::Queeniona => "queeniona",
            Room::Magnumopus => "magnumopus",
        }
    }

    fn settle(self) -> Duration {
        match self {
            Room::Queeniona => Duration::from_millis(500),
            _ => Duration::ZERO,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Charge {
    Positive,
    Negative,
    None,
}

type Area = (RangeInclusive<i32>, RangeInclusive<i32>);

fn area(room: Room, zone: &str, charge: Charge) -> Option<Area> {
    let zones: [[[i32; 2]; 2]; 3] = match room {
        Room::Ledgermayne => [
            [[431, 547], [234, 239]],
            [[147, 276], [353, 357]],
            [[727, 852], [353, 356]],
        ],
        Room::Moreskulls => [
            [[778, 806], [358, 361]],
            [[696, 802], [445, 452]],
            [[677, 766], [321, 324]],
        ],
        Room::Ultradage => [
            [[481, 483], [296, 300]],
            [[49, 164], [406, 412]],
            [[797, 900], [400, 402]],
        ],
        Room::Darkcarnax => [
            [[480, 530], [419, 432]],
            [[731, 850], [431, 432]],
            [[54, 155], [431, 432]],
        ],
        Room::Astralshrine => [
            [[461, 465], [320, 325]],
            [[643, 708], [445, 447]],
            [[199, 287], [181, 205]],
        ],
        Room::Magnumopus => [
            [[466, 470], [344, 420]],
            [[682, 813], [367, 384]],
            [[170, 285], [377, 384]],
        ],
        Room::Queeniona => {
            let [x, y] = match (zone, charge) {
                ("A", Charge::Positive) | ("B", Charge::Negative) => [[746, 869], [369, 379]],
                ("A", Charge::Negative) | ("B", Charge::Positive) => [[111, 272], [369, 379]],
                ("A" | "B", Charge::None) => return None,
                _ => [[490, 490], [320, 320]],
            };
            return Some((x[0]..=x[1], y[0]..=y[1]));
        }
    };
    let [x, y] = match zone {
        "" => zones[0],
        "A" => zones[1],
        "B" => zones[2],
        _ => return None,
    };
    Some((x[0]..=x[1], y[0]..=y[1]))
}

struct Pending {
    room: Room,
    zone: String,
    at: Instant,
}

pub(crate) async fn run(
    loader: impl Loader,
    zones: Packets<EventPacket>,
    mut settings: watch::Receiver<game_settings::Settings>,
    pick: impl Fn(&Area) -> (i32, i32),
) {
    let mut pending: Option<Pending> = None;
    loop {
        let enabled = settings.borrow_and_update().auto_zone;
        if !enabled {
            pending = None;
        }
        let due = pending.as_ref().map(|pending| pending.at);
        tokio::select! {
            event = zones.next() => match event {
                Ok(EventPacket { args: EventArgs { zone_set: zone } }) if enabled => {
                    match current_room(&loader) {
                        Ok(Some(room)) => {
                            tracing::debug!(?room, zone, "auto zone received an event packet");
                            pending = Some(Pending { room, zone, at: Instant::now() + room.settle() });
                        }
                        Ok(None) => {}
                        Err(error) => tracing::warn!(%error, "auto zone couldn't read the map"),
                    }
                }
                Ok(_) | Err(_) => {}
            },
            changed = settings.changed() => if changed.is_err() {
                return;
            },
            () = async { sleep_until(due.unwrap()).await }, if due.is_some() => {
                if let Some(pending) = pending.take()
                    && let Err(error) = move_to_zone(&loader, &pending, &pick)
                {
                    tracing::warn!(%error, "auto zone couldn't move");
                }
            }
        }
    }
}

fn current_room(loader: &impl Loader) -> Result<Option<Room>, CallError> {
    Ok(bridge::player::get_map(loader)?
        .as_deref()
        .and_then(Room::of))
}

fn move_to_zone(
    loader: &impl Loader,
    pending: &Pending,
    pick: &impl Fn(&Area) -> (i32, i32),
) -> Result<(), CallError> {
    if current_room(loader)? != Some(pending.room) {
        tracing::debug!(room = ?pending.room, "auto zone left the room before it moved");
        return Ok(());
    }
    let charge = if pending.room == Room::Queeniona {
        charge(loader)?
    } else {
        Charge::None
    };
    let Some(area) = area(pending.room, &pending.zone, charge) else {
        tracing::debug!(zone = %pending.zone, ?charge, "auto zone has no area for this zone and charge");
        return Ok(());
    };
    let (x, y) = pick(&area);
    let walked = player::walk_to(loader, x, y)?;
    tracing::debug!(zone = %pending.zone, ?charge, x, y, walked, "auto zone walked");
    Ok(())
}

fn charge(loader: &impl Loader) -> Result<Charge, CallError> {
    let auras = auras::player(loader)?;
    Ok(
        if auras::has_any(&auras, &["Positive Charge", "Negative Charge?"]) {
            Charge::Positive
        } else if auras::has_any(&auras, &["Negative Charge", "Positive Charge?"]) {
            Charge::Negative
        } else {
            Charge::None
        },
    )
}

pub(crate) fn random_point(area: &Area) -> (i32, i32) {
    let mut bytes = [0; 8];
    if getrandom::getrandom(&mut bytes).is_err() {
        return (*area.0.start(), *area.1.start());
    }
    let pick = |range: &RangeInclusive<i32>, random: u32| {
        let span = (range.end() - range.start()) as u32 + 1;
        range.start() + (random % span) as i32
    };
    (
        pick(&area.0, u32::from_ne_bytes(bytes[..4].try_into().unwrap())),
        pick(&area.1, u32::from_ne_bytes(bytes[4..].try_into().unwrap())),
    )
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use tokio::{task::LocalSet, time::sleep};

    use super::*;
    use crate::bridge::{Event, Events, Value};
    use crate::domain::testing::FakeLoader;

    struct Boss {
        map: &'static str,
        hp: i32,
        auras: Vec<&'static str>,
        walks: Vec<(i32, i32, Duration)>,
    }

    struct Fight {
        boss: Rc<RefCell<Boss>>,
        events: Rc<Events>,
        settings: watch::Sender<game_settings::Settings>,
    }

    fn fight(map: &'static str) -> Fight {
        let boss = Rc::new(RefCell::new(Boss {
            map,
            hp: 100,
            auras: Vec::new(),
            walks: Vec::new(),
        }));
        let events = Rc::new(Events::default());
        let (settings, receiver) = watch::channel(game_settings::Settings {
            auto_zone: true,
            ..game_settings::Settings::default()
        });
        let state = boss.clone();
        let start = Instant::now();
        let loader = FakeLoader::new(move |callback, args| {
            let mut boss = state.borrow_mut();
            let path = match args.first() {
                Some(Value::String(path)) => path.as_str(),
                _ => "",
            };
            Ok(match (callback, path) {
                ("player.getMap", _) => Value::String(boss.map.into()),
                ("player.getHp", _) => Value::Number(f64::from(boss.hp)),
                ("player.getAuras", _) => Value::List(
                    boss.auras
                        .iter()
                        .map(|name| {
                            Value::Object(
                                [("name".to_owned(), Value::String((*name).into()))].into(),
                            )
                        })
                        .collect(),
                ),
                ("player.walkTo", _) => {
                    let [Value::Number(x), Value::Number(y), _] = args else {
                        panic!("walkTo needs a point: {args:?}");
                    };
                    boss.walks.push((*x as i32, *y as i32, start.elapsed()));
                    Value::Bool(true)
                }
                (other, _) => panic!("unexpected call {other} {path}"),
            })
        });
        let zones = Packets::subscribe(|wants| events.subscribe(wants));
        tokio::task::spawn_local(run(loader, zones, receiver, |area: &Area| {
            (*area.0.start(), *area.1.end())
        }));
        Fight {
            boss,
            events,
            settings,
        }
    }

    impl Fight {
        fn zone(&self, zone: Option<&str>) {
            self.event(&zone.map_or(String::new(), |zone| format!(r#""zoneSet":"{zone}""#)));
        }

        fn event(&self, args: &str) {
            self.events.publish(&Event::ExtensionResponse {
                packet: format!(
                    r#"{{"type":"json","dataObj":{{"cmd":"event","args":{{{args}}}}}}}"#
                ),
            });
        }

        fn walks(&self) -> Vec<(i32, i32)> {
            self.boss
                .borrow()
                .walks
                .iter()
                .map(|(x, y, _)| (*x, *y))
                .collect()
        }
    }

    async fn settle() {
        sleep(Duration::from_secs(1)).await;
    }

    #[tokio::test(start_paused = true)]
    async fn each_zone_walks_into_its_area() {
        LocalSet::new()
            .run_until(async {
                let fight = fight("UltraDage");
                for zone in [Some("A"), Some("B"), None] {
                    fight.zone(zone);
                    settle().await;
                }
                assert_eq!(fight.walks(), [(49, 412), (797, 402), (481, 300)]);
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn queen_iona_picks_the_side_from_the_charge_after_it_settles() {
        LocalSet::new()
            .run_until(async {
                let fight = fight("queeniona");
                fight.boss.borrow_mut().auras = vec!["Negative Charge"];
                fight.zone(Some("A"));
                settle().await;
                fight.boss.borrow_mut().auras = vec!["Positive Charge?"];
                fight.zone(Some("A"));
                settle().await;
                fight.boss.borrow_mut().auras = vec!["Positive Charge?"];
                fight.zone(Some("B"));
                settle().await;
                fight.boss.borrow_mut().auras = vec!["Negative Charge?"];
                fight.zone(Some("A"));
                settle().await;
                fight.boss.borrow_mut().auras = Vec::new();
                fight.zone(Some("B"));
                settle().await;
                assert_eq!(
                    fight.walks(),
                    [(111, 379), (111, 379), (746, 379), (746, 379)]
                );
                let first_walk = fight.boss.borrow().walks[0].2;
                assert_eq!(first_walk, Duration::from_millis(500));
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn it_stays_put_outside_the_room_when_dead_or_off() {
        LocalSet::new()
            .run_until(async {
                let fight = fight("battleon");
                fight.zone(Some("A"));
                settle().await;
                fight.boss.borrow_mut().map = "darkcarnax";
                fight.boss.borrow_mut().hp = 0;
                fight.zone(Some("A"));
                settle().await;
                fight.boss.borrow_mut().hp = 100;
                fight
                    .settings
                    .send_modify(|settings| settings.auto_zone = false);
                settle().await;
                fight.zone(Some("A"));
                settle().await;
                assert!(fight.walks().is_empty());
                fight
                    .settings
                    .send_modify(|settings| settings.auto_zone = true);
                settle().await;
                fight.zone(Some("A"));
                settle().await;
                assert_eq!(fight.walks(), [(731, 432)]);
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn a_newer_zone_replaces_one_still_settling() {
        LocalSet::new()
            .run_until(async {
                let fight = fight("queeniona");
                fight.zone(Some("A"));
                sleep(Duration::from_millis(100)).await;
                fight.zone(None);
                settle().await;
                assert_eq!(fight.walks(), [(490, 320)]);
                fight.zone(None);
                sleep(Duration::from_millis(100)).await;
                fight.boss.borrow_mut().map = "battleon";
                settle().await;
                assert_eq!(fight.walks(), [(490, 320)]);
            })
            .await;
    }

    #[test]
    fn random_points_stay_inside_the_area() {
        let area = (10..=12, 5..=5);
        for _ in 0..200 {
            let (x, y) = random_point(&area);
            assert!(area.0.contains(&x) && y == 5);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn an_event_without_a_zone_returns_to_the_center_and_a_null_zone_is_ignored() {
        LocalSet::new()
            .run_until(async {
                let fight = fight("queeniona");
                fight.event(r#""zoneSet":null"#);
                settle().await;
                assert!(fight.walks().is_empty());
                fight.event(r#""sound":"boss""#);
                settle().await;
                assert_eq!(fight.walks(), [(490, 320)]);
            })
            .await;
    }
}
