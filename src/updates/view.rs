use gpui::{
    AnyElement, App, ElementId, InteractiveElement as _, IntoElement, ParentElement as _,
    RenderOnce, Role, SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
    prelude::FluentBuilder as _, px,
};
use mira_ui::{
    ActiveTheme as _, StyledExt as _, h_flex,
    primitives::{
        Button, Dialog, Icon, Spinner,
        button::{Size, Variant},
        icon,
    },
};

use super::Updates;

const VERSION: &str = concat!("Mira ", env!("CARGO_PKG_VERSION"));

fn failed_mark(cx: &App) -> impl IntoElement {
    div()
        .id("update-failed")
        .role(Role::Image)
        .aria_label("Update failed")
        .flex_none()
        .child(icon(Icon::TriangleAlert, cx.theme().warning).size(px(12.0)))
}

fn retry(error: &SharedString) -> SharedString {
    format!("{error}. Click to try again.").into()
}

pub(crate) fn footer(cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    let fallback = Updates::Unavailable(None);
    let state = cx.try_global::<Updates>().unwrap_or(&fallback);
    let status = match state {
        Updates::Current => Some("Up to date".into()),
        Updates::Available { update, .. } => Some(format!("{} available", update.version)),
        Updates::Downloading { progress, .. } => Some(format!("{:.0}%", progress * 100.0)),
        _ => None,
    };
    let failed = matches!(
        state,
        Updates::Failed(_)
            | Updates::Available { error: Some(_), .. }
            | Updates::Ready { error: Some(_), .. }
    );
    let check = |tooltip: Option<SharedString>| {
        let button = Button::icon("check-updates", Icon::RefreshCw, "Check for updates")
            .variant(Variant::Ghost)
            .size(Size::Small)
            .on_click(|_, _, cx| Updates::check(true, cx));
        match tooltip {
            Some(tooltip) => button.tooltip(tooltip),
            None => button.hide_tooltip(),
        }
    };
    let download = Button::icon("download-update", Icon::Download, "Download update")
        .variant(Variant::Ghost)
        .size(Size::Small);
    let restart = Button::new("restart-to-update", "Restart to update").size(Size::XSmall);
    let action: AnyElement = match state {
        Updates::Unavailable(reason) => check(reason.clone()).disabled(true).into_any_element(),
        Updates::Idle | Updates::Current => {
            check(Some("Check for updates".into())).into_any_element()
        }
        Updates::Checking => check(Some("Checking for updates…".into()))
            .busy(true)
            .into_any_element(),
        Updates::Failed(error) => check(Some(retry(error))).into_any_element(),
        Updates::Available { update, error } => download
            .tooltip(match error {
                Some(error) => retry(error),
                None => format!("Download Mira {}", update.version).into(),
            })
            .on_click(|_, _, cx| Updates::download(cx))
            .into_any_element(),
        Updates::Downloading { version, progress } => download
            .tooltip(format!("Downloading Mira {version}…"))
            .progress(*progress)
            .into_any_element(),
        Updates::Ready { staged, error } => Restart::new(
            "settings-restart",
            restart.tooltip(match error {
                Some(error) => retry(error),
                None => format!("Restart to install Mira {}", staged.version).into(),
            }),
        )
        .into_any_element(),
        Updates::Restarting(_) => restart.busy(true).into_any_element(),
    };
    h_flex()
        .debug_selector(|| "settings-update-footer".into())
        .flex_none()
        .h(theme.layout.control)
        .m(theme.space(2.0))
        .pl(theme.space(2.0))
        .gap(theme.space(2.0))
        .justify_between()
        .text_caption(theme.typography)
        .text_color(theme.muted_foreground)
        .child(
            h_flex()
                .min_w_0()
                .gap(theme.space(1.5))
                .child(div().flex_none().child(VERSION))
                .children(status.map(|status| div().min_w_0().truncate().child(status)))
                .when(failed, |row| row.child(failed_mark(cx))),
        )
        .child(action)
}

pub(crate) fn sidebar_row(cx: &App) -> Option<impl IntoElement> {
    let theme = cx.theme();
    let state = cx.try_global::<Updates>()?;
    let version = state.version()?.to_string();
    let muted = theme.muted_foreground;
    let (label, error) = match state {
        Updates::Available { error, .. } => ("Update available", error.clone()),
        Updates::Downloading { .. } => ("Downloading…", None),
        Updates::Ready { error, .. } => ("Restart to update", error.clone()),
        _ => ("Restarting…", None),
    };
    let mark = match state {
        Updates::Downloading { progress, .. } => Spinner::new("update-progress")
            .label("Downloading update")
            .color(muted)
            .size(px(16.0))
            .value(Some(*progress))
            .into_any_element(),
        Updates::Restarting(_) => Spinner::new("update-restarting")
            .label("Restarting")
            .color(muted)
            .size(px(16.0))
            .into_any_element(),
        _ if error.is_some() => icon(Icon::TriangleAlert, theme.warning)
            .size(px(14.0))
            .into_any_element(),
        _ => icon(Icon::Download, muted)
            .size(px(14.0))
            .into_any_element(),
    };
    let button = Button::new("sidebar-update", label)
        .variant(Variant::Ghost)
        .w_full()
        .child(
            h_flex()
                .flex_1()
                .justify_between()
                .child(h_flex().gap(theme.space(2.0)).child(mark).child(label))
                .child(div().text_color(muted).child(version.clone())),
        );
    let tooltip = |fallback: String| error.as_ref().map_or(fallback.into(), retry);
    let row = match state {
        Updates::Available { .. } => button
            .tooltip(tooltip(format!("Download Mira {version}")))
            .on_click(|_, _, cx| Updates::download(cx))
            .into_any_element(),
        Updates::Ready { .. } => Restart::new(
            "sidebar-restart",
            button.tooltip(tooltip(format!("Restart to install Mira {version}"))),
        )
        .into_any_element(),
        _ => button.disabled(true).into_any_element(),
    };
    Some(div().debug_selector(|| "sidebar-update".into()).child(row))
}

#[derive(IntoElement)]
struct Restart {
    id: ElementId,
    button: Button,
}

impl Restart {
    fn new(id: impl Into<ElementId>, button: Button) -> Self {
        Self {
            id: id.into(),
            button,
        }
    }
}

impl RenderOnce for Restart {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let asking = window.use_keyed_state(self.id.clone(), cx, |_, _| false);
        let open = *asking.read(cx);
        let tabs = crate::ruffle_view::open_tabs(cx);
        let set = move |to: bool| {
            let asking = asking.clone();
            move |cx: &mut App| {
                asking.update(cx, |asking, cx| {
                    *asking = to;
                    cx.notify();
                })
            }
        };
        let (ask, cancel, close, accept) = (set(true), set(false), set(false), set(false));
        div()
            .child(self.button.on_click(move |_, _, cx| {
                if crate::ruffle_view::open_tabs(cx) == 0 {
                    Updates::restart(cx);
                } else {
                    ask(cx);
                }
            }))
            .child(
                Dialog::new((self.id, "dialog"), open, "Restart to update?")
                    .alert()
                    .description(match tabs {
                        1 => "Restarting closes your tab and stops its script.".to_owned(),
                        n => format!("Restarting closes your {n} tabs and stops their scripts."),
                    })
                    .action(
                        Button::new("restart-cancel", "Cancel")
                            .on_click(move |_, _, cx| cancel(cx)),
                    )
                    .action(
                        Button::new("restart-accept", "Restart")
                            .variant(Variant::Primary)
                            .on_click(move |_, _, cx| {
                                accept(cx);
                                Updates::restart(cx);
                            }),
                    )
                    .on_close(move |_, cx| close(cx)),
            )
    }
}
