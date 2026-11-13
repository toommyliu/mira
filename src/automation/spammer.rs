use std::time::Duration;

use tokio::{
    sync::watch,
    time::{Instant, sleep_until},
};

use crate::{
    bridge::Loader,
    domain::packets::{self, Delivery, Target},
};

pub(crate) const DELAY_MS: (u64, u64) = (10, 60_000);

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Settings {
    pub(crate) enabled: bool,
    pub(crate) target: Target,
    pub(crate) packets: Vec<String>,
    pub(crate) delay: Duration,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: false,
            target: Target::Server,
            packets: Vec::new(),
            delay: Duration::from_secs(1),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) enum Status {
    #[default]
    Idle,
    Sent {
        index: usize,
        count: usize,
    },
    NotInGame,
    Failed(String),
}

pub(crate) async fn run(
    loader: impl Loader,
    mut settings: watch::Receiver<Settings>,
    report: impl Fn(Status),
    stop: impl Fn(),
) {
    let mut next = 0;
    let mut last: Option<Instant> = None;
    let mut failed = false;
    loop {
        let current = settings.borrow_and_update().clone();
        if !current.enabled || current.packets.is_empty() {
            if !failed {
                report(Status::Idle);
            }
            next = 0;
            last = None;
            if settings.changed().await.is_err() {
                return;
            }
            continue;
        }
        failed = false;
        let now = Instant::now();
        let due = last.map_or(now, |last| last + current.delay);
        if now >= due {
            let index = next % current.packets.len();
            let failure = match packets::send(&loader, current.target, &current.packets[index]) {
                Ok(Delivery::Sent) => {
                    next = index + 1;
                    report(Status::Sent {
                        index,
                        count: current.packets.len(),
                    });
                    None
                }
                Ok(Delivery::NotInGame) => {
                    report(Status::NotInGame);
                    None
                }
                Ok(Delivery::Refused(problem)) => {
                    Some(format!("packet {}: {}", index + 1, problem.message()))
                }
                Err(error) => Some(error.to_string()),
            };
            if let Some(reason) = failure {
                tracing::warn!(%reason, "the packet spammer stopped");
                failed = true;
                report(Status::Failed(reason));
                stop();
            }
            last = Some(if now - due < current.delay { due } else { now });
            continue;
        }
        tokio::select! {
            () = sleep_until(due) => {}
            changed = settings.changed() => if changed.is_err() {
                return;
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use tokio::{task::LocalSet, time::sleep};

    use super::*;
    use crate::{
        bridge::{CallError, Value},
        domain::testing::FakeLoader,
    };

    struct Game {
        in_game: bool,
        broken: bool,
        sent: Vec<(Duration, String)>,
    }

    fn start(
        packets: &[&str],
    ) -> (
        Rc<RefCell<Game>>,
        watch::Sender<Settings>,
        Rc<RefCell<Status>>,
    ) {
        let started = Instant::now();
        let game = Rc::new(RefCell::new(Game {
            in_game: true,
            broken: false,
            sent: Vec::new(),
        }));
        let seen = Rc::clone(&game);
        let loader = FakeLoader::new(move |callback, args| {
            let mut game = seen.borrow_mut();
            match callback {
                "auth.isLoggedIn" | "world.isLoaded" => Ok(Value::Bool(game.in_game)),
                "flash.sendClientPacket" => Ok(Value::Null),
                "flash.callGameFunction" if game.broken => Err(CallError::Stopped),
                "flash.callGameFunction" => {
                    let Value::String(packet) = &args[1] else {
                        panic!("a packet is text");
                    };
                    game.sent.push((started.elapsed(), packet.clone()));
                    Ok(Value::Null)
                }
                other => panic!("unexpected {other}"),
            }
        });
        let (settings, watching) = watch::channel(Settings {
            enabled: true,
            packets: packets.iter().map(|packet| (*packet).to_owned()).collect(),
            ..Settings::default()
        });
        let status = Rc::new(RefCell::new(Status::Idle));
        let reported = Rc::clone(&status);
        let stopping = settings.clone();
        tokio::task::spawn_local(run(
            loader,
            watching,
            move |status| *reported.borrow_mut() = status,
            move || stopping.send_modify(|settings| settings.enabled = false),
        ));
        (game, settings, status)
    }

    fn sent(game: &Rc<RefCell<Game>>) -> Vec<(u64, String)> {
        game.borrow()
            .sent
            .iter()
            .map(|(at, packet)| (at.as_millis() as u64, packet.clone()))
            .collect()
    }

    fn at(ms: u64, packet: &str) -> (u64, String) {
        (ms, packet.to_owned())
    }

    #[tokio::test(start_paused = true)]
    async fn packets_go_out_in_order_on_a_steady_beat_and_start_over() {
        LocalSet::new()
            .run_until(async {
                let (game, _settings, status) = start(&["%a%", "%b%"]);
                sleep(Duration::from_millis(2_500)).await;
                assert_eq!(
                    sent(&game),
                    [at(0, "%a%"), at(1_000, "%b%"), at(2_000, "%a%")]
                );
                assert_eq!(*status.borrow(), Status::Sent { index: 0, count: 2 });
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn edits_while_running_apply_from_the_next_packet() {
        LocalSet::new()
            .run_until(async {
                let (game, settings, _) = start(&["%a%", "%b%"]);
                sleep(Duration::from_millis(500)).await;
                settings.send_modify(|settings| {
                    settings.packets = vec!["%c%".into(), "%d%".into()];
                    settings.delay = Duration::from_millis(200);
                });
                sleep(Duration::from_millis(500)).await;
                assert_eq!(
                    sent(&game),
                    [at(0, "%a%"), at(500, "%d%"), at(700, "%c%"), at(900, "%d%")]
                );
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn out_of_game_it_waits_and_resumes_with_the_packet_it_was_on() {
        LocalSet::new()
            .run_until(async {
                let (game, _settings, status) = start(&["%a%", "%b%"]);
                sleep(Duration::from_millis(500)).await;
                game.borrow_mut().in_game = false;
                sleep(Duration::from_millis(1_000)).await;
                assert_eq!(*status.borrow(), Status::NotInGame);
                game.borrow_mut().in_game = true;
                sleep(Duration::from_millis(1_000)).await;
                assert_eq!(sent(&game), [at(0, "%a%"), at(2_000, "%b%")]);
            })
            .await;
    }

    #[tokio::test(start_paused = true)]
    async fn a_failed_or_unreadable_packet_switches_it_off_and_keeps_the_reason() {
        LocalSet::new()
            .run_until(async {
                let (game, settings, status) = start(&["%a%"]);
                game.borrow_mut().broken = true;
                sleep(Duration::from_millis(1_500)).await;
                assert!(!settings.borrow().enabled);
                assert_eq!(
                    *status.borrow(),
                    Status::Failed(CallError::Stopped.to_string())
                );
                assert!(sent(&game).is_empty());

                game.borrow_mut().broken = false;
                settings.send_modify(|settings| {
                    settings.packets = vec!["%a%".into(), "hello".into()];
                    settings.target = Target::Client;
                    settings.enabled = true;
                });
                sleep(Duration::from_millis(1_500)).await;
                assert!(!settings.borrow().enabled);
                assert_eq!(
                    *status.borrow(),
                    Status::Failed("packet 2: The client reads %xt% text, JSON or XML".into())
                );
            })
            .await;
    }
}
