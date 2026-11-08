use serde::Deserialize;
use serde_json::json;

use super::calls::Reply;
use crate::{
    bridge::{self, Event, Loader, Subscription},
    domain::{self, quests},
};

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(
    tag = "op",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum Request {
    Load {
        quest_id: i32,
        silent: Option<bool>,
    },
    LoadBatch {
        quest_ids: Vec<i32>,
        silent: Option<bool>,
    },
    Get {
        quest_id: i32,
    },
    Accepted,
    InProgress {
        quest_id: i32,
    },
    Available {
        quest_id: i32,
    },
    CanComplete {
        quest_id: i32,
    },
    MaxTurnIns {
        quest_id: i32,
    },
    Accept {
        quest_id: i32,
        silent: Option<bool>,
    },
    AcceptBatch {
        quest_ids: Vec<i32>,
        silent: Option<bool>,
    },
    Complete {
        quest_id: i32,
        turn_ins: Option<i32>,
        reward_item_id: Option<i32>,
    },
    Abandon {
        quest_id: i32,
    },
}

impl Request {
    fn without_avatar(&self) -> serde_json::Value {
        match self {
            Request::Get { .. } => serde_json::Value::Null,
            Request::Accepted => json!([]),
            Request::MaxTurnIns { .. } => json!(0),
            _ => json!(false),
        }
    }
}

pub(super) async fn serve(
    loader: &impl Loader,
    subscribe: impl Fn(fn(&Event) -> bool) -> Subscription,
    request: Request,
) -> Reply {
    Ok(match request {
        Request::Load { quest_id, silent } => {
            json!(quests::load(loader, quest_id, silent.unwrap_or(false)).await?)
        }
        Request::LoadBatch { quest_ids, silent } => {
            json!(quests::load_batch(loader, &quest_ids, silent.unwrap_or(false)).await?)
        }
        Request::AcceptBatch { quest_ids, silent } => json!(
            quests::accept_batch(loader, subscribe, &quest_ids, silent.unwrap_or(false)).await?
        ),
        Request::Accept { quest_id, silent } => {
            json!(quests::accept(loader, subscribe, quest_id, silent.unwrap_or(false)).await?)
        }
        Request::Complete {
            quest_id,
            turn_ins,
            reward_item_id,
        } => json!(
            quests::complete(
                loader,
                subscribe,
                quest_id,
                turn_ins.unwrap_or(1),
                reward_item_id
            )
            .await?
        ),
        request if !domain::has_avatar(loader)? => request.without_avatar(),
        Request::Get { quest_id } => json!(quests::quest(loader, quest_id)?),
        Request::Accepted => json!(quests::accepted(loader)?),
        Request::InProgress { quest_id } => {
            json!(bridge::quests::is_in_progress(loader, quest_id)?)
        }
        Request::Available { quest_id } => json!(bridge::quests::is_available(loader, quest_id)?),
        Request::CanComplete { quest_id } => json!(bridge::quests::can_complete(loader, quest_id)?),
        Request::MaxTurnIns { quest_id } => {
            json!(bridge::quests::get_max_turn_ins(loader, quest_id)?)
        }
        Request::Abandon { quest_id } => json!(bridge::quests::abandon(loader, quest_id)?),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        bridge::{Events, Value},
        domain::testing::FakeLoader,
    };

    #[tokio::test]
    async fn out_of_game_reads_are_empty_and_actions_do_nothing() {
        let loader = FakeLoader::new(|callback, _| match callback {
            "auth.isLoggedIn" => Ok(Value::Bool(false)),
            other => panic!("{other} was called while out of game"),
        });
        let events = Events::default();
        for (request, expected) in [
            (
                Request::Load {
                    quest_id: 1,
                    silent: None,
                },
                json!(false),
            ),
            (
                Request::LoadBatch {
                    quest_ids: vec![1, 2],
                    silent: None,
                },
                json!([false, false]),
            ),
            (
                Request::AcceptBatch {
                    quest_ids: vec![1, 2],
                    silent: None,
                },
                json!([false, false]),
            ),
            (Request::Get { quest_id: 1 }, json!(null)),
            (Request::Accepted, json!([])),
            (Request::InProgress { quest_id: 1 }, json!(false)),
            (Request::Available { quest_id: 1 }, json!(false)),
            (Request::CanComplete { quest_id: 1 }, json!(false)),
            (Request::MaxTurnIns { quest_id: 1 }, json!(0)),
            (
                Request::Accept {
                    quest_id: 1,
                    silent: Some(true),
                },
                json!(false),
            ),
            (
                Request::Complete {
                    quest_id: 1,
                    turn_ins: None,
                    reward_item_id: None,
                },
                json!(false),
            ),
            (Request::Abandon { quest_id: 1 }, json!(false)),
        ] {
            assert_eq!(
                serve(&loader, |wants| events.subscribe(wants), request.clone()).await,
                Ok(expected),
                "{request:?}"
            );
        }
    }
}
