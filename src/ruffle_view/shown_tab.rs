use std::rc::Rc;

use gpui::{
    App, Context, Div, Entity, EntityId, ParentElement as _, SharedString, Styled as _,
    Subscription, Task, Transformation, WeakEntity, Window, div, prelude::FluentBuilder as _, px,
    radians,
};
use mira_ui::{
    ActiveTheme as _, Tone,
    primitives::{
        Button, Dot, DropdownMenu, Icon, MenuItem,
        button::{Size, Variant},
        icon,
    },
    theme::TEXT_BODY,
};
use tokio::sync::watch;

use super::{
    Client, RuffleView,
    client::{bar_separator, bar_title, drag_area, window_bar},
};
use crate::automation::settings::Automation;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Target {
    Selected,
    Pinned(EntityId),
}

pub(super) struct ShownTab {
    pub(super) id: EntityId,
    pub(super) view: WeakEntity<RuffleView>,
    pub(super) automation: Automation,
    title: SharedString,
    _watching: Vec<Task<()>>,
    _renamed: Subscription,
}

fn resolve(
    target: Target,
    tabs: &[EntityId],
    selected: Option<EntityId>,
) -> (Target, Option<EntityId>) {
    match target {
        Target::Pinned(id) if tabs.contains(&id) => (target, Some(id)),
        _ => (Target::Selected, selected),
    }
}

pub(super) fn show<W: 'static>(
    shown: &mut Option<ShownTab>,
    target: &mut Target,
    client: &Entity<Client>,
    reshow: fn(&mut W, &mut Context<W>),
    watch: impl FnOnce(&Automation, &mut Context<W>) -> Vec<Task<()>>,
    cx: &mut Context<W>,
) -> bool {
    let found = {
        let client = client.read(cx);
        let titled = client.titled_views(cx);
        let ids: Vec<EntityId> = titled.iter().map(|(view, _)| view.entity_id()).collect();
        let selected = client.active_view(cx).map(|(view, _)| view.entity_id());
        let (resolved, id) = resolve(*target, &ids, selected);
        *target = resolved;
        id.and_then(|id| titled.into_iter().find(|(view, _)| view.entity_id() == id))
    };
    let Some((view, title)) = found else {
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

pub(super) fn header<W: 'static>(
    tool: &'static str,
    client: &Entity<Client>,
    shown: Option<&ShownTab>,
    target: Target,
    retarget: fn(&mut W, Target, &mut Context<W>),
    window: &Window,
    cx: &mut Context<W>,
) -> Div {
    let (tabs, selected) = {
        let client = client.read(cx);
        (
            client
                .titled_views(cx)
                .into_iter()
                .map(|(view, title)| (view.entity_id(), title))
                .collect(),
            client.active_view(cx).map(|(view, _)| view.entity_id()),
        )
    };
    let this = cx.weak_entity();
    let menu = shown.map(|shown| {
        tab_menu(
            shown.title.clone(),
            target,
            tabs,
            selected,
            move |target, _, cx| {
                let _ = this.update(cx, |this, cx| retarget(this, target, cx));
            },
            cx,
        )
    });
    title_bar(tool, menu, window, cx)
}

fn tab_menu(
    shown: SharedString,
    target: Target,
    tabs: Vec<(EntityId, SharedString)>,
    selected: Option<EntityId>,
    pick: impl Fn(Target, &mut Window, &mut App) + 'static,
    cx: &App,
) -> DropdownMenu {
    let pick = Rc::new(pick);
    let pinned = matches!(target, Target::Pinned(_));
    let theme = cx.theme();
    let label = if pinned {
        format!("{shown}, pinned").into()
    } else {
        shown.clone()
    };
    DropdownMenu::new("tab-menu")
        .trigger(
            Button::new("tab-menu-trigger", label)
                .variant(Variant::Ghost)
                .size(Size::Small)
                .text_size(TEXT_BODY)
                .when(pinned, |trigger| {
                    trigger.start_slot(icon(Icon::Pin, theme.accent_text).with_transformation(
                        Transformation::rotate(radians(std::f32::consts::FRAC_PI_4)),
                    ))
                })
                .child(
                    div()
                        .min_w_0()
                        .max_w(px(180.0))
                        .truncate()
                        .text_color(theme.text)
                        .child(shown),
                )
                .end_slot(Icon::ChevronDown),
        )
        .items(move |_, _| {
            let choose = |choice: Target| {
                let pick = pick.clone();
                move |window: &mut Window, cx: &mut App| pick(choice, window, cx)
            };
            let mut items = vec![
                MenuItem::radio(
                    "Follow selected tab",
                    target == Target::Selected,
                    choose(Target::Selected),
                ),
                MenuItem::separator(),
                MenuItem::header("Pin to"),
            ];
            items.extend(tabs.iter().map(|(id, title)| {
                let item = MenuItem::radio(
                    title.clone(),
                    target == Target::Pinned(*id),
                    choose(Target::Pinned(*id)),
                );
                if selected == Some(*id) {
                    item.end_slot(Dot::new(Tone::Accent))
                } else {
                    item
                }
            }));
            items
        })
}

fn title_bar(tool: &'static str, menu: Option<DropdownMenu>, window: &Window, cx: &App) -> Div {
    window_bar(window, cx)
        .child(bar_title(tool, cx))
        .when_some(menu, |bar, menu| bar.child(bar_separator(cx)).child(menu))
        .child(drag_area().flex_1())
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

#[cfg(test)]
mod tests {
    use gpui::{AppContext as _, IntoElement, Render, size};

    use super::*;

    #[test]
    fn a_tool_follows_the_selected_tab_until_pinned_and_follows_again_when_its_tab_closes() {
        let [main, alt, gone] = [1, 2, 3].map(EntityId::from);
        let tabs = [main, alt];
        assert_eq!(
            resolve(Target::Selected, &tabs, Some(main)),
            (Target::Selected, Some(main))
        );
        assert_eq!(
            resolve(Target::Selected, &tabs, Some(alt)),
            (Target::Selected, Some(alt))
        );
        assert_eq!(
            resolve(Target::Pinned(alt), &tabs, Some(main)),
            (Target::Pinned(alt), Some(alt))
        );
        assert_eq!(
            resolve(Target::Pinned(gone), &tabs, Some(main)),
            (Target::Selected, Some(main))
        );
        assert_eq!(
            resolve(Target::Pinned(gone), &[], None),
            (Target::Selected, None)
        );
    }

    struct Scene(Target);

    impl Render for Scene {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let theme = cx.theme().clone();
            let [main, alt] = [1, 2].map(EntityId::from);
            let tabs = vec![(main, "Artix".into()), (alt, "Alt 1".into())];
            let target = self.0;
            let (tool, shown) = match target {
                Target::Selected => ("Packets", "Artix"),
                Target::Pinned(_) => ("Follower", "Alt 1"),
            };
            let menu = tab_menu(shown.into(), target, tabs, Some(main), |_, _, _| {}, cx)
                .default_open(matches!(target, Target::Pinned(_)));
            div()
                .size_full()
                .bg(theme.base)
                .text_color(theme.text)
                .font_family(mira_ui::theme::SYSTEM_SANS)
                .text_size(TEXT_BODY)
                .child(title_bar(tool, Some(menu), window, cx))
        }
    }

    #[test]
    fn snapshot_tool_header() {
        for (name, target) in [
            ("tool-header-following", Target::Selected),
            ("tool-header-pinned", Target::Pinned(EntityId::from(2))),
        ] {
            for path in mira_ui::snapshot(name, size(px(420.0), px(260.0)), move |_, cx| {
                cx.new(|_| Scene(target))
            }) {
                assert!(path.exists(), "{} was not written", path.display());
            }
        }
    }
}
