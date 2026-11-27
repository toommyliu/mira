use std::{hint::black_box, time::Instant};

use super::{Event, Events, Packets, Value};
use crate::{
    automation::{anti_afk, auto_relogin, game_settings},
    domain::{
        CombatCooldown, counter_attack,
        drops::{DropItemPacket, GetDropPacket},
        events::{Kind, Me},
        inventory::AddItemsPacket,
        rotation,
        world::{self, EventPacket},
    },
};

struct Player;

impl Me for Player {
    fn username(&self) -> Option<String> {
        Some("player1".into())
    }

    fn user_id(&self) -> Option<i32> {
        Some(1)
    }
}

const FIGHT: &str = r#"{"type":"json","dataObj":{"cmd":"ct","p":{"player1":{"intState":2,"intHP":4850,"intMP":76},"player2":{"intState":2,"intHP":5910,"intMP":89}},"m":{"2":{"intState":2,"intHP":7775},"3":{"intState":2,"intHP":9130}},"anims":[{"cInf":"p:1","tInf":"m:2","strAnim":"Attack1"},{"cInf":"m:2","tInf":"p:1"}],"a":[{"cmd":"aura+","tInf":"p:1","auras":[{"nam":"Focus","dur":5,"stk":1}]}],"sara":[{"actionResult":{"cInf":"p:1","tInf":"m:2","ct":0}}]}}"#;
const COUNTER: &str = r#"{"type":"json","dataObj":{"cmd":"ct","m":{"2":{"intState":2,"intHP":8000}},"anims":[{"msg":"<mon> prepares a counter attack!","cInf":"m:2","tInf":"p:1"}],"a":[{"cmd":"aura++","tInf":"m:2","auras":[{"nam":"Counter Attack","dur":5,"stk":1}]}],"sara":[{"actionResult":{"cInf":"m:2","ct":4500}}]}}"#;
const FADE: &str = r#"{"type":"json","dataObj":{"cmd":"ct","m":{"2":{"intState":0,"intHP":0}},"a":[{"cmd":"aura--","tInf":"m:2","aura":{"nam":"Counter Attack"}},{"cmd":"aura-","tInf":"p:1","aura":{"nam":"Sun's Heat"}}]}}"#;
const STREAM: [&str; 10] = [
    FIGHT, FIGHT, FIGHT, COUNTER, FIGHT, FIGHT, FIGHT, FIGHT, FIGHT, FADE,
];

#[tokio::test]
#[ignore = "manual packet benchmark"]
async fn packet_pipeline_benchmark() {
    let events = Events::default();
    let cooldown = events.subscribe(CombatCooldown::wants);
    let counter = game_settings::Settings::default()
        .anti_counter
        .then(|| events.subscribe(counter_attack::wants));
    let _afk = events.subscribe(anti_afk::wants);
    let _relogin = events.subscribe(auto_relogin::wants);
    let _sign_in = events.subscribe(auto_relogin::wants);
    let _zone = Packets::<EventPacket>::subscribe(|wants| events.subscribe(wants));
    let messages = events.subscribe(rotation::wants);
    let _quests = events.subscribe(|event| {
        Packets::<AddItemsPacket>::wants(event) || Packets::<GetDropPacket>::wants(event)
    });
    let _drops = Packets::<DropItemPacket>::subscribe(|wants| events.subscribe(wants));
    let _location = events.subscribe(world::moves);
    let _extras = events.subscribe(|event| *event == Event::Loaded);
    let mut read = Vec::new();

    let packets = std::env::var("MIRA_BENCH_PACKETS")
        .map(|count| count.parse::<usize>().unwrap())
        .unwrap_or(50_000);
    assert_eq!(packets % STREAM.len(), 0);
    for run in 0..8 {
        let start = Instant::now();
        let mut outputs = 0;
        for packet in STREAM.into_iter().cycle().take(packets) {
            let event = Event::parse(
                "onExtensionResponse",
                &[Value::List(vec![
                    "json".into(),
                    "ct".into(),
                    black_box(packet).into(),
                ])],
            )
            .unwrap()
            .unwrap();
            events.publish(&event);
            Kind::PlayerState.read(&cooldown.next().await.unwrap(), &Player, &mut read);
            Kind::Message.read(&messages.next().await.unwrap(), &Player, &mut read);
            outputs += black_box(&read).len();
            read.clear();
            if let Some(counter) = &counter {
                let signals = counter_attack::signals(&counter.next().await.unwrap(), &Player);
                outputs += black_box(&signals).len();
            }
        }
        let elapsed = start.elapsed();
        assert_eq!(outputs, packets / STREAM.len() * 22);
        if run > 0 {
            eprintln!(
                "run {run} ({packets} packets): {:.1} ns/packet",
                elapsed.as_nanos() as f64 / packets as f64
            );
        }
    }
}
