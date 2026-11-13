use std::time::SystemTime;

use tokio::sync::watch;

use crate::bridge::{
    Drained, Event, Subscription,
    packets::{Sender, raw},
};

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Captured {
    Packet {
        at: SystemTime,
        from: Sender,
        text: String,
    },
    Lost {
        at: SystemTime,
    },
}

pub(crate) fn wants(event: &Event) -> bool {
    raw(event).is_some()
}

pub(crate) async fn capture(
    subscribe: impl Fn() -> Subscription,
    mut capturing: watch::Receiver<bool>,
    send: impl Fn(Vec<Captured>),
) {
    loop {
        if capturing.wait_for(|on| *on).await.is_err() {
            return;
        }
        let packets = subscribe();
        loop {
            tokio::select! {
                changed = capturing.changed() => {
                    if changed.is_err() {
                        return;
                    }
                    if !*capturing.borrow_and_update() {
                        break;
                    }
                }
                drained = packets.drain() => send(batch(drained, SystemTime::now())),
            }
        }
    }
}

fn batch(drained: Drained, at: SystemTime) -> Vec<Captured> {
    let lost = drained.lagged.then_some(Captured::Lost { at });
    lost.into_iter()
        .chain(
            drained
                .events
                .iter()
                .filter_map(raw)
                .map(|packet| Captured::Packet {
                    at,
                    from: packet.from,
                    text: packet.text.to_owned(),
                }),
        )
        .collect()
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc, time::Duration};

    use tokio::{task::LocalSet, time::sleep};

    use super::*;
    use crate::bridge::Events;

    fn line(line: &str) -> Event {
        Event::SmartFoxDebug { line: line.into() }
    }

    fn texts(batches: &[Vec<Captured>]) -> Vec<(Sender, String)> {
        batches
            .iter()
            .flatten()
            .filter_map(|captured| match captured {
                Captured::Packet { from, text, .. } => Some((*from, text.clone())),
                Captured::Lost { .. } => None,
            })
            .collect()
    }

    #[tokio::test(start_paused = true)]
    async fn only_packets_sent_while_capturing_arrive_in_order() {
        LocalSet::new()
            .run_until(async {
                let events = Rc::new(Events::default());
                let (capturing, watching) = watch::channel(false);
                let batches = Rc::new(RefCell::new(Vec::new()));
                let subscriber = Rc::clone(&events);
                let sink = Rc::clone(&batches);
                tokio::task::spawn_local(capture(
                    move || subscriber.subscribe(wants),
                    watching,
                    move |batch| sink.borrow_mut().push(batch),
                ));
                let step = || sleep(Duration::from_millis(10));

                events.publish(&line("[Sending - STR]: %xt%zm%before%1%"));
                step().await;
                capturing.send_replace(true);
                step().await;
                events.publish(&line("[Sending - STR]: %xt%zm%afk%1%true%, (len: 24)"));
                events.publish(&line("Connection lost"));
                events.publish(&line("[ RECEIVED ]: %xt%afk%-1%, (len: 11)"));
                step().await;
                capturing.send_replace(false);
                step().await;
                events.publish(&line("[Sending - STR]: %xt%zm%after%1%"));
                step().await;

                assert_eq!(
                    texts(&batches.borrow()),
                    [
                        (Sender::Client, "%xt%zm%afk%1%true%".to_owned()),
                        (Sender::Server, "%xt%afk%-1%".to_owned()),
                    ]
                );
            })
            .await;
    }

    #[test]
    fn a_lagged_mailbox_marks_the_gap_before_the_packets_that_follow_it() {
        let at = SystemTime::UNIX_EPOCH;
        let drained = Drained {
            events: vec![line("[ RECEIVED ]: %xt%ct%-1%")],
            lagged: true,
        };
        assert_eq!(
            batch(drained, at),
            [
                Captured::Lost { at },
                Captured::Packet {
                    at,
                    from: Sender::Server,
                    text: "%xt%ct%-1%".into(),
                },
            ]
        );
    }
}
