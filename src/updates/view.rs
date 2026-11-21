use gpui::{
    App, Div, ElementId, Hsla, InteractiveElement as _, IntoElement, ParentElement as _,
    RenderOnce, Role, SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
    prelude::FluentBuilder as _, px,
};
use mira_ui::{
    ActiveTheme as _, StyledExt as _, Tone, h_flex,
    primitives::{
        Align, Button, Dialog, HoverCard, Icon, Placement, Side,
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
    let status = matches!(state, Updates::Current).then_some("Up to date");
    let failed = matches!(state, Updates::Failed(_));
    let action = match button(Placement::new(Side::Top, Align::Start), theme.sidebar, cx) {
        Some(update) => update.into_any_element(),
        None => {
            let button = Button::icon("check-updates", Icon::RefreshCw, "Check for updates")
                .variant(Variant::Ghost)
                .size(Size::Small)
                .icon_size(px(14.0))
                .busy(matches!(state, Updates::Checking))
                .disabled(matches!(state, Updates::Unavailable(_)))
                .on_click(|_, _, cx| Updates::check(true, cx));
            match state {
                Updates::Unavailable(None) => button.hide_tooltip(),
                Updates::Unavailable(Some(reason)) => button.tooltip(reason.clone()),
                Updates::Failed(error) => button.tooltip(retry(error)),
                Updates::Checking => button.tooltip("Checking for updates…"),
                _ => button.tooltip("Check for updates"),
            }
            .into_any_element()
        }
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

const NOTES: usize = 8;

fn with_notes(mut card: HoverCard, notes: &str) -> HoverCard {
    let mut paragraph = Vec::new();
    let mut items = 0;
    for line in notes.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let Some(item) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) else {
            paragraph.push(line);
            continue;
        };
        items += 1;
        if items > NOTES {
            continue;
        }
        card = match item
            .strip_prefix("**")
            .and_then(|rest| rest.split_once("**"))
        {
            Some((lead, text)) => card.note(Some(lead.to_owned().into()), text.trim().to_owned()),
            None => card.note(None, item.to_owned()),
        };
    }
    if items > NOTES {
        card = card.note(None, format!("and {} more", items - NOTES));
    }
    match paragraph.is_empty() {
        true => card,
        false => card.body(paragraph.join(" ")),
    }
}

pub(crate) fn dot(behind: Hsla, cx: &App) -> Option<Div> {
    let theme = cx.theme();
    let color = match cx.try_global::<Updates>()? {
        Updates::Available { error: None, .. } => theme.foreground,
        Updates::Available { error: Some(_), .. } | Updates::Ready { error: Some(_), .. } => {
            theme.warning
        }
        _ => return None,
    };
    Some(
        div()
            .absolute()
            .top(px(1.0))
            .right(px(1.0))
            .size(px(10.0))
            .rounded_full()
            .border_2()
            .border_color(behind)
            .bg(color),
    )
}

fn ready_badge(behind: Hsla, cx: &App) -> Option<Div> {
    let theme = cx.theme();
    matches!(
        cx.try_global::<Updates>(),
        Some(Updates::Ready { error: None, .. })
    )
    .then(|| {
        div()
            .absolute()
            .right(px(-1.0))
            .bottom(px(-1.0))
            .size(px(14.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .border_2()
            .border_color(behind)
            .bg(theme.foreground)
            .child(icon(Icon::Check, theme.background).size(px(8.0)))
    })
}

pub(crate) fn button(placement: Placement, behind: Hsla, cx: &App) -> Option<impl IntoElement> {
    let state = cx.try_global::<Updates>()?;
    let version = state.version()?;
    let card = |title: String| {
        let card = HoverCard::new("update-card", title)
            .placement(placement)
            .row(
                "Version",
                format!("{} → {version}", env!("CARGO_PKG_VERSION")),
                Tone::Neutral,
            );
        match state.update().and_then(|update| update.notes.as_deref()) {
            Some(notes) => with_notes(card, notes),
            None => card,
        }
    };
    let with_error = |card: HoverCard, error: &Option<SharedString>| match error {
        Some(error) => card.row("Error", error.clone(), Tone::Attention),
        None => card,
    };
    let (button, card) = match state {
        Updates::Available { update, error } => (
            Button::icon("bar-update", Icon::Download, "Download update")
                .on_click(|_, _, cx| Updates::download(cx)),
            with_error(
                card(if error.is_some() {
                    "Download failed. Click to try again.".into()
                } else {
                    format!("Mira {version} is available. Click to download.")
                }),
                error,
            )
            .row(
                "Size",
                format!("{:.1} MB", update.size() as f64 / 1e6),
                Tone::Neutral,
            ),
        ),
        Updates::Downloading { progress, .. } => (
            Button::icon("bar-update", Icon::Download, "Downloading update").progress(*progress),
            card(format!(
                "Downloading Mira {version}… {:.0}%",
                progress * 100.0
            )),
        ),
        Updates::Ready { error, .. } => (
            Button::icon("bar-update", Icon::RotateCw, "Restart to update"),
            with_error(
                card(if error.is_some() {
                    "Install failed. Click to try again.".into()
                } else {
                    format!("Mira {version} is ready. Click to restart.")
                }),
                error,
            ),
        ),
        _ => (
            Button::icon("bar-update", Icon::RotateCw, "Restarting").busy(true),
            card(format!("Installing Mira {version}…")),
        ),
    };
    let button = button
        .variant(Variant::Ghost)
        .size(Size::Small)
        .icon_size(px(14.0))
        .hide_tooltip();
    let dot = dot(behind, cx).or_else(|| ready_badge(behind, cx));
    let button = match state {
        Updates::Ready { .. } => Restart::new("bar-restart", button).into_any_element(),
        _ => button.into_any_element(),
    };
    Some(
        div()
            .debug_selector(|| "bar-update".into())
            .relative()
            .flex_none()
            .child(card.child(button))
            .children(dot),
    )
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
