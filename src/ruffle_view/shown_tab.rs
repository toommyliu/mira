use gpui::{App, Context, Entity, EntityId, SharedString, Subscription, Task, WeakEntity, px};
use mira_ui::primitives::Select;
use tokio::sync::watch;

use super::{Client, RuffleView};
use crate::automation::settings::Automation;

pub(super) struct ShownTab {
    pub(super) id: EntityId,
    pub(super) view: WeakEntity<RuffleView>,
    pub(super) automation: Automation,
    title: SharedString,
    _watching: Vec<Task<()>>,
    _renamed: Subscription,
}

pub(super) fn active<W: 'static>(client: &Entity<Client>, cx: &Context<W>) -> Option<EntityId> {
    client
        .read(cx)
        .active_view(cx)
        .map(|(view, _)| view.entity_id())
}

pub(super) fn show<W: 'static>(
    shown: &mut Option<ShownTab>,
    client: &Entity<Client>,
    wanted: Option<EntityId>,
    reshow: fn(&mut W, &mut Context<W>),
    watch: impl FnOnce(&Automation, &mut Context<W>) -> Vec<Task<()>>,
    cx: &mut Context<W>,
) -> bool {
    let target = {
        let client = client.read(cx);
        wanted
            .or(shown.as_ref().map(|shown| shown.id))
            .and_then(|id| {
                client
                    .titled_views(cx)
                    .into_iter()
                    .find(|(view, _)| view.entity_id() == id)
            })
            .or_else(|| client.active_view(cx))
    };
    let Some((view, title)) = target else {
        *shown = None;
        cx.notify();
        return true;
    };
    if let Some(current) = shown
        && current.id == view.entity_id()
    {
        if current.title != title {
            current.title = title;
            cx.notify();
        }
        return false;
    }
    let automation = view.read(cx).automation.clone();
    *shown = Some(ShownTab {
        id: view.entity_id(),
        view: view.downgrade(),
        title,
        _watching: watch(&automation, cx),
        automation,
        _renamed: cx.observe(&view, move |this, _, cx| reshow(this, cx)),
    });
    cx.notify();
    true
}

pub(super) fn picker<W: 'static>(
    id: &'static str,
    client: &Entity<Client>,
    shown: Option<&ShownTab>,
    pick: fn(&mut W, EntityId, &mut Context<W>),
    cx: &mut Context<W>,
) -> Select {
    let titled = client.read(cx).titled_views(cx);
    let picked = shown.and_then(|shown| {
        titled
            .iter()
            .position(|(view, _)| view.entity_id() == shown.id)
    });
    let ids: Vec<EntityId> = titled.iter().map(|(view, _)| view.entity_id()).collect();
    let this = cx.weak_entity();
    Select::new(
        id,
        "Tab",
        titled.into_iter().map(|(_, title)| title),
        picked,
    )
    .placeholder("No tabs")
    .width(px(200.0))
    .on_select(move |index, _, cx: &mut App| {
        let _ = this.update(cx, |this, cx| pick(this, ids[index], cx));
    })
}

pub(super) fn watch_changes<T: Send + Sync + 'static, W: 'static>(
    mut changes: watch::Receiver<T>,
    cx: &mut Context<W>,
) -> Task<()> {
    cx.spawn(async move |this, cx| {
        while changes.changed().await.is_ok() {
            if this.update(cx, |_, cx| cx.notify()).is_err() {
                break;
            }
        }
    })
}
