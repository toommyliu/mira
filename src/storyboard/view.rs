use std::rc::Rc;

use gpui::{
    App, AppContext as _, Context, Div, ElementId, Entity, FontWeight, Hsla,
    InteractiveElement as _, IntoElement, ParentElement as _, Pixels, Render, Role, SharedString,
    Stateful, StatefulInteractiveElement as _, Styled as _, Window, div,
    prelude::FluentBuilder as _, px,
};
use mira_ui::{
    ActiveTheme as _, FOCUS_SCOPE, Palette, h_flex,
    primitives::{
        Button, DropdownMenu, Icon, Input, MenuItem, Spinner,
        button::{Size, Variant},
        icon, small_icon,
    },
    theme::{LINE_BODY, LINE_SMALL, MIRA_CHROME, MIRA_FONT, TEXT_BODY, TEXT_CAPTION},
    v_flex,
};

use super::Screen;

pub(super) type Navigation = Option<Rc<dyn Fn(Screen, &mut Window, &mut App)>>;

pub(super) fn palette(cx: &App) -> &Palette {
    cx.theme().palette().expect("Mira canvas theme")
}

pub(super) fn change<T: 'static>(
    cx: &Context<T>,
    apply: impl Fn(&mut T, &mut Context<T>) + 'static,
) -> impl Fn(&mut Window, &mut App) + 'static {
    let entity = cx.weak_entity();
    move |_, cx| {
        entity
            .update(cx, |view, cx| {
                apply(view, cx);
                cx.notify();
            })
            .ok();
    }
}

pub(super) fn choose<T: 'static, V: 'static>(
    cx: &Context<T>,
    apply: impl Fn(&mut T, V, &mut Context<T>) + 'static,
) -> impl Fn(V, &mut Window, &mut App) + 'static {
    let entity = cx.weak_entity();
    move |value, _, cx| {
        entity
            .update(cx, |view, cx| {
                apply(view, value, cx);
                cx.notify();
            })
            .ok();
    }
}

pub(super) fn command(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    variant: Variant,
    action: impl Fn(&mut Window, &mut App) + 'static,
) -> Button {
    Button::new(id, label)
        .variant(variant)
        .on_click(move |_, window, cx| action(window, cx))
}

pub(super) fn menu_button(id: &'static str, label: impl Into<SharedString>, cx: &App) -> Button {
    Button::new(id, label)
        .variant(Variant::Ghost)
        .size(Size::Small)
        .end_slot(small_icon(Icon::ChevronDown, palette(cx).muted_foreground))
}

pub(super) fn title(text: impl Into<SharedString>) -> Div {
    div().font_weight(FontWeight::SEMIBOLD).child(text.into())
}

pub(super) fn meta(text: impl Into<SharedString>, cx: &App) -> Div {
    div()
        .text_size(TEXT_CAPTION)
        .line_height(LINE_SMALL)
        .text_color(palette(cx).muted_foreground)
        .child(text.into())
}

pub(super) fn spring() -> Div {
    div().flex_1().min_w_0()
}

pub(super) fn divider(cx: &App) -> Div {
    div().flex_none().w_full().h(px(1.0)).bg(palette(cx).border)
}

pub(super) fn vertical_edge(cx: &App) -> Div {
    div()
        .flex_none()
        .w(px(1.0))
        .h(px(16.0))
        .bg(palette(cx).border)
}

pub(super) fn toolbar(cx: &App) -> Div {
    h_flex()
        .flex_none()
        .gap(px(8.0))
        .min_h(px(44.0))
        .px(px(16.0))
        .py(px(8.0))
        .border_b_1()
        .border_color(palette(cx).border)
}

pub(super) fn compact_toolbar() -> Div {
    h_flex().flex_none().gap(px(8.0)).px(px(16.0)).py(px(4.0))
}

pub(super) fn footer(cx: &App) -> Div {
    h_flex()
        .flex_none()
        .gap(px(8.0))
        .px(px(16.0))
        .py(px(12.0))
        .border_t_1()
        .border_color(palette(cx).border)
}

pub(super) fn scroll(id: &'static str) -> Stateful<Div> {
    v_flex().id(id).flex_1().min_h_0().overflow_y_scroll()
}

pub(super) fn empty(heading: &'static str, note: &'static str, cx: &App) -> Div {
    v_flex()
        .flex_1()
        .min_h(px(96.0))
        .justify_center()
        .items_center()
        .p(px(16.0))
        .gap(px(4.0))
        .child(title(heading))
        .child(meta(note, cx))
}

pub(super) fn field(
    label: &str,
    hint: &str,
    initial: &str,
    search: bool,
    cx: &mut App,
) -> Entity<Input> {
    let input = cx.new(|cx| {
        let field = Input::new(label.to_owned(), cx).placeholder(hint.to_owned());
        if search { field.search() } else { field }
    });
    input.update(cx, |input, cx| input.set_text(initial.to_owned(), cx));
    input
}

pub(super) fn traffic_lights() -> Div {
    h_flex()
        .flex_none()
        .w(px(78.0))
        .gap(px(8.0))
        .pl(px(12.0))
        .children([0xff5f57, 0xfebc2e, 0x28c840].map(|color| {
            div()
                .flex_none()
                .size(px(12.0))
                .rounded_full()
                .bg(gpui::rgb(color))
        }))
}

pub(super) fn chrome(cx: &App) -> Div {
    h_flex()
        .flex_none()
        .w_full()
        .h(MIRA_CHROME)
        .gap(px(4.0))
        .pr(px(8.0))
        .bg(palette(cx).background)
        .border_b_1()
        .border_color(palette(cx).border)
}

pub(super) fn surface(cx: &App) -> Div {
    v_flex()
        .bg(palette(cx).background)
        .text_color(palette(cx).foreground)
        .font_family(MIRA_FONT)
        .text_size(TEXT_BODY)
        .line_height(LINE_BODY)
        .key_context(FOCUS_SCOPE)
}

pub(super) fn frame(bar: impl IntoElement, cx: &App) -> Div {
    surface(cx)
        .size_full()
        .child(chrome(cx).child(traffic_lights()).child(bar))
}

pub(super) fn tool_chrome(screen: Screen, target: Option<Entity<Target>>, cx: &App) -> Div {
    h_flex()
        .flex_1()
        .min_w_0()
        .gap(px(8.0))
        .child(title(screen.name()))
        .when_some(target, |bar, target| {
            bar.child(vertical_edge(cx)).child(target)
        })
}

pub(super) struct Sessions {
    pub names: Vec<SharedString>,
    pub active: usize,
    next: usize,
}

impl Default for Sessions {
    fn default() -> Self {
        Self {
            names: vec!["Aster".into(), "Vesper".into(), "Sable".into()],
            active: 0,
            next: 4,
        }
    }
}

impl Sessions {
    pub fn add(&mut self) {
        let name = format!("Session {}", self.next).into();
        self.next += 1;
        self.open(name);
    }

    pub fn open(&mut self, name: SharedString) {
        self.active = self
            .names
            .iter()
            .position(|existing| existing == &name)
            .unwrap_or_else(|| {
                self.names.push(name);
                self.names.len() - 1
            });
    }

    pub fn close(&mut self) {
        if self.active < self.names.len() {
            self.names.remove(self.active);
            self.active = self.active.min(self.names.len().saturating_sub(1));
        }
    }

    pub fn current(&self) -> Option<&SharedString> {
        self.names.get(self.active)
    }
}

pub(super) struct Target {
    sessions: Entity<Sessions>,
    pinned: Option<SharedString>,
}

impl Target {
    pub fn new(sessions: Entity<Sessions>, cx: &mut App) -> Entity<Self> {
        cx.new(|cx| {
            cx.observe(&sessions, |this: &mut Self, sessions, cx| {
                if this
                    .pinned
                    .as_ref()
                    .is_some_and(|target| !sessions.read(cx).names.contains(target))
                {
                    this.pinned = None;
                }
                cx.notify();
            })
            .detach();
            Self {
                sessions,
                pinned: None,
            }
        })
    }

    pub fn resolved<'a>(&'a self, cx: &'a App) -> Option<&'a SharedString> {
        self.pinned
            .as_ref()
            .or_else(|| self.sessions.read(cx).current())
    }

    pub fn available(&self, cx: &App) -> bool {
        self.resolved(cx).is_some()
    }
}

impl Render for Target {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let sessions = self.sessions.read(cx);
        let pinned = self.pinned.clone();
        let name = pinned.clone().or_else(|| sessions.current().cloned());
        let Some(name) = name else {
            return meta("No tabs open", cx).into_any_element();
        };
        let names = sessions.names.clone();
        let entity = cx.weak_entity();
        let label: SharedString = if pinned.is_some() {
            format!("{name}, pinned").into()
        } else {
            name.clone()
        };
        DropdownMenu::new("target")
            .trigger(
                menu_button("target-trigger", label, cx)
                    .child(name)
                    .when(pinned.is_some(), |button| {
                        button.start_slot(small_icon(Icon::Pin, palette(cx).foreground))
                    }),
            )
            .width(px(210.0))
            .items(move |_, _| {
                let follow = entity.clone();
                let mut items = vec![
                    MenuItem::radio("Follow selected tab", pinned.is_none(), move |_, cx| {
                        follow
                            .update(cx, |view, cx| {
                                view.pinned = None;
                                cx.notify();
                            })
                            .ok();
                    }),
                    MenuItem::separator(),
                    MenuItem::header("Pin to"),
                ];
                items.extend(names.iter().map(|name| {
                    let on = pinned.as_ref() == Some(name);
                    let name = name.clone();
                    let entity = entity.clone();
                    MenuItem::radio(name.clone(), on, move |_, cx| {
                        entity
                            .update(cx, |view, cx| {
                                view.pinned = Some(name.clone());
                                cx.notify();
                            })
                            .ok();
                    })
                }));
                items
            })
            .into_any_element()
    }
}

pub(super) fn observe_input<T: 'static>(input: &Entity<Input>, cx: &mut Context<T>) {
    cx.observe(input, |_, _, cx| cx.notify()).detach();
}

pub(super) fn window_width(screen: Screen, window: &Window) -> Pixels {
    window.viewport_size().width.min(px(screen.frame().width))
}

pub(super) fn status(label: impl Into<SharedString>, muted: bool, cx: &App) -> Stateful<Div> {
    let color: Hsla = if muted {
        palette(cx).muted_foreground
    } else {
        palette(cx).foreground
    };
    div()
        .id("status")
        .role(Role::Status)
        .text_size(TEXT_CAPTION)
        .line_height(LINE_SMALL)
        .text_color(color)
        .child(label.into())
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum StatusPreview {
    #[default]
    Current,
    Working,
    Problem,
}

impl gpui::Global for StatusPreview {}

impl StatusPreview {
    pub(super) fn current(cx: &App) -> Self {
        cx.try_global::<Self>().copied().unwrap_or_default()
    }
}

pub(super) fn status_preview(
    working: impl Into<SharedString>,
    problem: impl Into<SharedString>,
    detail: impl Into<SharedString>,
    cx: &App,
) -> Option<Stateful<Div>> {
    let colors = palette(cx);
    let body = |role| {
        h_flex()
            .id("status-preview")
            .debug_selector(|| "status-preview".into())
            .role(role)
            .min_w_0()
            .gap(px(8.0))
    };
    match StatusPreview::current(cx) {
        StatusPreview::Current => None,
        StatusPreview::Working => Some(
            body(Role::Status)
                .child(Spinner::new("working").color(colors.muted_foreground))
                .child(meta(working, cx)),
        ),
        StatusPreview::Problem => Some(
            body(Role::Alert)
                .items_start()
                .child(
                    div()
                        .flex_none()
                        .mt(px(1.0))
                        .child(icon(Icon::CircleAlert, colors.destructive).size(px(16.0))),
                )
                .child(
                    v_flex()
                        .min_w_0()
                        .child(problem.into())
                        .child(meta(detail, cx)),
                ),
        ),
    }
}
