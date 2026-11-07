use serde::Deserialize;
use serde_json::json;

use super::calls::Reply;
use crate::{
    bridge::{Event, Loader, Subscription},
    domain::{
        self, auth,
        world::{self, MonsterQuery},
    },
};

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(
    tag = "op",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum Request {
    AreaId,
    RoomNumber,
    Cells,
    CellPads,
    Monsters {
        cell: Option<String>,
    },
    Monster {
        query: MonsterQuery,
    },
    MonsterAvailable {
        query: MonsterQuery,
    },
    MonsterDrops {
        query: MonsterQuery,
    },
    SetSpawnPoint {
        cell: Option<String>,
        pad: Option<String>,
    },
    MapItem {
        item_id: i32,
    },
    LoadSwf {
        swf: String,
    },
    Reload,
}

impl Request {
    fn without_avatar(&self) -> serde_json::Value {
        match self {
            Request::AreaId => json!(0),
            Request::Cells | Request::CellPads | Request::Monsters { .. } => json!([]),
            Request::RoomNumber | Request::Monster { .. } | Request::MonsterDrops { .. } => {
                serde_json::Value::Null
            }
            Request::MonsterAvailable { .. }
            | Request::SetSpawnPoint { .. }
            | Request::MapItem { .. }
            | Request::LoadSwf { .. }
            | Request::Reload => json!(false),
        }
    }
}

pub(super) async fn serve(
    loader: &impl Loader,
    subscribe: impl Fn(fn(&Event) -> bool) -> Subscription,
    request: Request,
) -> Reply {
    Ok(match request {
        Request::MonsterDrops { query } => {
            json!(world::monster_drops(loader, subscribe(world::dropped), &query).await?)
        }
        Request::SetSpawnPoint { cell, pad } => {
            json!(world::set_spawn_point(loader, cell.as_deref(), pad.as_deref()).await?)
        }
        Request::MapItem { item_id } => json!(world::get_map_item(loader, item_id).await?),
        Request::LoadSwf { swf } => json!(world::load_swf(loader, &swf).await?),
        Request::Reload => json!(world::reload(loader).await?),
        Request::Cells | Request::CellPads if !auth::in_game(loader)? => json!([]),
        Request::Cells => json!(world::cells(loader)?),
        Request::CellPads => json!(world::cell_pads(loader)?),
        request if !domain::has_avatar(loader)? => request.without_avatar(),
        Request::AreaId => json!(world::area_id(loader)?),
        Request::RoomNumber => json!(world::room_number(loader)?),
        Request::Monsters { cell } => json!(world::monsters(loader, cell.as_deref())?),
        Request::Monster { query } => json!(world::monster(loader, &query)?),
        Request::MonsterAvailable { query } => json!(world::monster_available(loader, &query)?),
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
        let frogzard = || MonsterQuery::Name("Frogzard".into());
        for (request, expected) in [
            (Request::AreaId, json!(0)),
            (Request::RoomNumber, json!(null)),
            (Request::Cells, json!([])),
            (Request::CellPads, json!([])),
            (Request::Monsters { cell: None }, json!([])),
            (Request::Monster { query: frogzard() }, json!(null)),
            (
                Request::MonsterAvailable { query: frogzard() },
                json!(false),
            ),
            (Request::MonsterDrops { query: frogzard() }, json!(null)),
            (
                Request::SetSpawnPoint {
                    cell: None,
                    pad: None,
                },
                json!(false),
            ),
            (Request::MapItem { item_id: 3 }, json!(false)),
            (
                Request::LoadSwf {
                    swf: "town.swf".into(),
                },
                json!(false),
            ),
            (Request::Reload, json!(false)),
        ] {
            let answer = serve(&loader, |wants| events.subscribe(wants), request.clone()).await;
            assert_eq!(answer, Ok(expected), "{request:?}");
        }
    }
}
