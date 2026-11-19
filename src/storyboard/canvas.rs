use std::{rc::Rc, sync::Arc};

use gpui::{
    AnyElement, AnyView, App, AppContext as _, Bounds, Context, CursorStyle, FocusHandle,
    InteractiveElement as _, IntoElement, KeyDownEvent, MouseButton, ObjectFit, ParentElement as _,
    Pixels, Point, Render, RenderImage, Role, Size, StatefulInteractiveElement as _, Styled as _,
    StyledImage as _, Window, canvas, div, fill, img, point, prelude::FluentBuilder as _, px, size,
};
use mira_ui::{
    ActiveTheme as _, FOCUS_SCOPE, PreviewRenderer, Theme, h_flex, nav,
    primitives::{
        Button, DropdownMenu, Icon, MenuItem, Toast, Toaster,
        button::{Size as Control, Variant},
    },
    theme::{MIRA_CHROME, tabular},
};

use super::{FRAMES, Frame, Screen, view::*};

const MIN_ZOOM: f32 = 0.1;
const MAX_ZOOM: f32 = 2.5;
const LABEL_HEIGHT: f32 = 28.0;
const FIT_MARGIN: f32 = 32.0;
const BOTTOM_MARGIN: f32 = 60.0;

#[derive(Clone, Copy)]
struct Camera {
    // The screen position of the world's origin, in canvas coordinates.
    origin: Point<Pixels>,
    zoom: f32,
}

impl Camera {
    fn fit(viewport: Size<Pixels>) -> Self {
        let world_width = FRAMES
            .iter()
            .map(|frame| frame.x + frame.width)
            .fold(0.0, f32::max);
        let world_height = FRAMES
            .iter()
            .map(|frame| frame.y + frame.height)
            .fold(0.0, f32::max);
        let width = f32::from(viewport.width);
        let height = f32::from(viewport.height);
        let zoom = ((width - FIT_MARGIN * 2.0) / world_width)
            .min((height - FIT_MARGIN - BOTTOM_MARGIN - LABEL_HEIGHT) / world_height)
            .clamp(MIN_ZOOM, 1.0);
        Self {
            zoom,
            origin: point(
                px((width - world_width * zoom) / 2.0),
                px(
                    (height - world_height * zoom + LABEL_HEIGHT + FIT_MARGIN - BOTTOM_MARGIN)
                        / 2.0,
                ),
            ),
        }
    }

    fn frame(self, frame: Frame) -> Bounds<Pixels> {
        Bounds::new(
            self.origin + point(px(frame.x * self.zoom), px(frame.y * self.zoom)),
            frame.size().map(|dimension| dimension * self.zoom),
        )
    }

    fn zoom_at(&mut self, anchor: Point<Pixels>, zoom: f32) {
        let zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        self.origin = anchor - (anchor - self.origin) * (zoom / self.zoom);
        self.zoom = zoom;
    }
}

struct Preview(AnyView);

impl Render for Preview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div().size_full().child(self.0.clone())
    }
}

type Previews = [Option<Arc<RenderImage>>; FRAMES.len()];

enum PreviewSet {
    Empty,
    Rendering,
    Ready(Previews),
}

fn render_previews(dark: bool) -> anyhow::Result<Previews> {
    let mut renderer = PreviewRenderer::new(Theme::mira(dark))?;
    Ok(FRAMES.map(|frame| {
        match renderer.render(frame.size(), |_, cx| {
            let sessions = cx.new(|_| Sessions::default());
            let view = frame.build(None, sessions, cx);
            cx.new(|_| Preview(view))
        }) {
            Ok(preview) => Some(preview),
            Err(error) => {
                eprintln!("couldn't render {} preview: {error:#}", frame.screen.name());
                None
            }
        }
    }))
}

pub(super) struct Storyboard {
    previews: [PreviewSet; 2],
    scenes: [AnyView; FRAMES.len()],
    current: Option<Screen>,
    camera: Option<Camera>,
    pan: Option<(Point<Pixels>, MouseButton)>,
    space: bool,
    focus: FocusHandle,
    viewport_focus: FocusHandle,
    inspect_focus: FocusHandle,
}

impl Storyboard {
    fn new(cx: &mut Context<Self>) -> Self {
        let owner = cx.weak_entity();
        let open: Navigation = Some(Rc::new(move |screen, window, cx| {
            owner
                .update(cx, |board, cx| board.show(Some(screen), window, cx))
                .ok();
        }));
        let sessions = cx.new(|_| Sessions::default());
        let scenes = FRAMES.map(|frame| frame.build(open.clone(), sessions.clone(), cx));
        Self {
            previews: [PreviewSet::Empty, PreviewSet::Empty],
            scenes,
            current: None,
            camera: None,
            pan: None,
            space: false,
            focus: cx.focus_handle(),
            viewport_focus: cx.focus_handle(),
            inspect_focus: cx.focus_handle(),
        }
    }

    fn show(&mut self, screen: Option<Screen>, window: &mut Window, cx: &mut Context<Self>) {
        self.current = screen;
        self.pan = None;
        self.space = false;
        let focus = if screen.is_some() {
            self.inspect_focus.clone()
        } else {
            self.viewport_focus.clone()
        };
        cx.notify();
        window.defer(cx, move |window, cx| window.focus(&focus, cx));
    }

    fn viewer_size(window: &Window) -> Size<Pixels> {
        let mut viewport = window.viewport_size();
        viewport.height -= MIRA_CHROME;
        viewport
    }

    fn camera(&self, window: &Window) -> Camera {
        self.camera
            .unwrap_or_else(|| Camera::fit(Self::viewer_size(window)))
    }

    fn zoom(&mut self, point: Point<Pixels>, scale: f32, window: &Window, cx: &mut Context<Self>) {
        let mut camera = self.camera(window);
        camera.zoom_at(point, scale);
        self.camera = Some(camera);
        cx.notify();
    }

    fn move_by(&mut self, delta: Point<Pixels>, window: &Window, cx: &mut Context<Self>) {
        let mut camera = self.camera(window);
        camera.origin += delta;
        self.camera = Some(camera);
        cx.notify();
    }

    fn keys(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = &event.keystroke;
        let area = Self::viewer_size(window);
        let center = point(area.width / 2.0, area.height / 2.0);
        let zoom = self.camera(window).zoom;
        match (self.current, key.modifiers.platform, key.key.as_str()) {
            (Some(_), false, "escape") => self.show(None, window, cx),
            (None, true, "0") => {
                self.camera = None;
                cx.notify();
            }
            (None, true, "1") => self.zoom(center, 1.0, window, cx),
            (None, true, "-" | "minus") => self.zoom(center, zoom / 1.2, window, cx),
            (None, true, "=" | "+" | "equal") => self.zoom(center, zoom * 1.2, window, cx),
            (None, false, "space") if self.viewport_focus.is_focused(window) => {
                self.space = true;
                cx.notify();
            }
            (None, false, "left" | "right" | "up" | "down")
                if self.viewport_focus.is_focused(window) =>
            {
                let delta = match key.key.as_str() {
                    "left" => point(px(72.0), px(0.0)),
                    "right" => point(px(-72.0), px(0.0)),
                    "up" => point(px(0.0), px(72.0)),
                    _ => point(px(0.0), px(-72.0)),
                };
                self.move_by(delta, window, cx);
            }
            _ => return,
        }
        cx.stop_propagation();
    }

    fn window_menu(&self, cx: &Context<Self>) -> DropdownMenu {
        let owner = cx.weak_entity();
        let selected = self.current;
        DropdownMenu::new("windows")
            .width(px(186.0))
            .trigger(menu_button("windows-trigger", "Windows", cx))
            .items(move |_, _| {
                FRAMES
                    .iter()
                    .map(|frame| {
                        let screen = frame.screen;
                        let owner = owner.clone();
                        MenuItem::radio(
                            screen.name(),
                            selected == Some(screen),
                            move |window, cx| {
                                owner
                                    .update(cx, |view, cx| view.show(Some(screen), window, cx))
                                    .ok();
                            },
                        )
                    })
                    .collect()
            })
    }

    fn status_menu(screen: Screen, cx: &App) -> DropdownMenu {
        let preview = StatusPreview::current(cx);
        DropdownMenu::new("status-examples")
            .width(px(190.0))
            .trigger(menu_button("status-trigger", "Statuses", cx))
            .items(move |_, _| {
                let mut items = Vec::new();
                if matches!(
                    screen,
                    Screen::Game
                        | Screen::Scripts
                        | Screen::Profiles
                        | Screen::Environment
                        | Screen::Follower
                        | Screen::Loader
                        | Screen::Packets
                        | Screen::Settings
                ) {
                    items.extend(
                        [
                            ("Current", StatusPreview::Current),
                            ("In progress", StatusPreview::Working),
                            ("Needs attention", StatusPreview::Problem),
                        ]
                        .map(|(label, state)| {
                            MenuItem::radio(label, state == preview, move |_, cx| {
                                cx.set_global(state);
                                cx.refresh_windows();
                            })
                        }),
                    );
                    items.push(MenuItem::separator());
                }
                items.push(MenuItem::action("Show sample toast", move |_, cx| {
                    let toast = match screen {
                        Screen::Scripts => Toast::new("Added to queue").body("Relic circuit"),
                        Screen::Profiles => Toast::new("Profile saved").body("Relic farm"),
                        Screen::Loader => Toast::new("ID copied").body("Abbey lantern"),
                        Screen::Packets => Toast::new("Packet copied"),
                        Screen::Game => Toast::new("Relic circuit stopped"),
                        Screen::Accounts => Toast::new("Account saved").body("Aster"),
                        Screen::Environment => Toast::new("Environment copied"),
                        Screen::Follower => Toast::new("Search maps updated"),
                        Screen::Settings => Toast::new("Settings saved"),
                    };
                    Toaster::push(toast, cx);
                }));
                items
            })
    }

    fn header(&self, dark: bool, window: &Window, cx: &Context<Self>) -> gpui::Div {
        let drag = || {
            div()
                .h(MIRA_CHROME)
                .on_mouse_down(MouseButton::Left, |event, window, _| {
                    if event.click_count == 2 {
                        window.titlebar_double_click();
                    } else {
                        window.start_window_move();
                    }
                })
        };
        chrome(cx)
            .child(
                drag()
                    .w(px(if window.is_fullscreen() { 8.0 } else { 78.0 }))
                    .flex_none(),
            )
            .when(self.current.is_some(), |bar| {
                bar.child(
                    Button::icon("back", Icon::ArrowLeft, "Back to canvas")
                        .size(Control::Small)
                        .on_click(cx.listener(|view, _, window, cx| view.show(None, window, cx))),
                )
            })
            .child(title(self.current.map_or("Design canvas", Screen::name)))
            .child(vertical_edge(cx).mx(px(4.0)))
            .child(self.window_menu(cx))
            .when_some(self.current, |bar, screen| {
                bar.child(
                    div()
                        .debug_selector(|| "status-menu".into())
                        .child(Self::status_menu(screen, cx)),
                )
            })
            .child(drag().flex_1())
            .child(
                command(
                    "theme",
                    if dark { "Light mode" } else { "Dark mode" },
                    Variant::Ghost,
                    move |_, cx| {
                        let mut theme = Theme::mira(!dark);
                        theme.pointer_cursor = cx.theme().pointer_cursor;
                        cx.set_global(theme);
                        cx.refresh_windows();
                    },
                )
                .size(Control::Small),
            )
    }

    fn canvas(&self, window: &mut Window, cx: &mut Context<Self>, dark: bool) -> impl IntoElement {
        let area = Self::viewer_size(window);
        let camera = self.camera(window);
        let viewport = Bounds::new(point(px(0.0), px(0.0)), area);
        let fill_color = palette(cx).muted;
        let border = palette(cx).border;
        let ring = palette(cx).ring;
        let dot = palette(cx).ring.opacity(if dark { 0.21 } else { 0.43 });
        let background = palette(cx).background;
        let zoom = camera.zoom;
        let previews = match &self.previews[usize::from(dark)] {
            PreviewSet::Ready(previews) => Some(previews),
            _ => None,
        };
        let tiles: Vec<_> = FRAMES
            .iter()
            .enumerate()
            .filter_map(|(index, &frame)| {
                let bounds = camera.frame(frame);
                let labeled_bounds = Bounds::new(
                    bounds.origin - point(px(0.0), px(LABEL_HEIGHT)),
                    size(bounds.size.width, bounds.size.height + px(LABEL_HEIGHT)),
                );
                if labeled_bounds.intersect(&viewport).is_empty() {
                    return None;
                }
                let screen = frame.screen;
                let focus = nav::focus_handle(("frame", index), window, cx);
                let focused = nav::keyboard_focused(&focus, window, cx);
                let image = previews.and_then(|previews| previews[index].clone());
                Some(
                    div()
                        .id(("artboard", index))
                        .absolute()
                        .role(Role::Button)
                        .aria_label(format!("Inspect {}", screen.name()))
                        .track_focus(&focus.tab_stop(true))
                        .left(bounds.origin.x)
                        .top(bounds.origin.y - px(LABEL_HEIGHT))
                        .w(bounds.size.width)
                        .h(bounds.size.height + px(LABEL_HEIGHT))
                        .cursor_pointer()
                        .group("artboard")
                        .on_click(cx.listener(move |view, _, window, cx| {
                            view.show(Some(screen), window, cx)
                        }))
                        .child(
                            h_flex()
                                .min_w_0()
                                .h(px(LABEL_HEIGHT))
                                .gap(px(8.0))
                                .justify_between()
                                .child(title(screen.name()).truncate())
                                .when(bounds.size.width >= px(216.0), |label| {
                                    label.child(
                                        meta(format!("{} × {}", frame.width, frame.height), cx)
                                            .flex_none()
                                            .font_features(tabular()),
                                    )
                                }),
                        )
                        .child(
                            div()
                                .relative()
                                .rounded(px(8.0))
                                .overflow_hidden()
                                .h(bounds.size.height)
                                .bg(background)
                                .border_1()
                                .border_color(if focused { ring } else { border })
                                .group_hover("artboard", |style| style.border_color(ring))
                                .map(|tile| match image {
                                    Some(image) => tile.child(
                                        img(image)
                                            .size_full()
                                            .rounded(px(7.0))
                                            .object_fit(ObjectFit::Fill),
                                    ),
                                    None => tile.child(
                                        div()
                                            .size_full()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child("Inspect window"),
                                    ),
                                }),
                        ),
                )
            })
            .collect();
        let drag = cx.listener(|this, event: &gpui::MouseDownEvent, window, cx| {
            if event.button == MouseButton::Middle
                || (event.button == MouseButton::Left && this.space)
            {
                this.pan = Some((event.position, event.button));
                window.focus(&this.viewport_focus, cx);
                cx.stop_propagation();
                cx.notify();
            }
        });
        let end_pan = cx.listener(|this, _: &gpui::MouseUpEvent, _, cx| {
            if this.pan.take().is_some() {
                cx.stop_propagation();
                cx.notify();
            }
        });
        let center = point(area.width / 2.0, area.height / 2.0);
        div()
            .id("canvas")
            .track_focus(&self.viewport_focus)
            .flex_1()
            .min_h_0()
            .relative()
            .overflow_hidden()
            .bg(fill_color)
            .when(self.space || self.pan.is_some(), |body| {
                body.cursor(if self.pan.is_some() {
                    CursorStyle::ClosedHand
                } else {
                    CursorStyle::OpenHand
                })
            })
            .capture_any_mouse_down(drag)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| window.focus(&this.viewport_focus, cx)),
            )
            .on_mouse_move(
                cx.listener(|this, event: &gpui::MouseMoveEvent, window, cx| {
                    if let Some((last, button)) = this.pan {
                        if event.pressed_button == Some(button) {
                            this.pan = Some((event.position, button));
                            this.move_by(event.position - last, window, cx);
                        } else {
                            this.pan = None;
                            cx.notify();
                        }
                    }
                }),
            )
            .on_mouse_up(MouseButton::Left, end_pan)
            .on_mouse_up(
                MouseButton::Middle,
                cx.listener(|this, _, _, cx| {
                    this.pan = None;
                    cx.notify();
                }),
            )
            .on_scroll(cx.listener(|this, event: &gpui::ScrollEvent, window, cx| {
                let delta = event.delta.pixel_delta(px(24.0));
                if event.modifiers.platform {
                    let anchor = event.position - point(px(0.0), MIRA_CHROME);
                    this.zoom(
                        anchor,
                        this.camera(window).zoom * (f32::from(delta.y) / 240.0).exp(),
                        window,
                        cx,
                    );
                } else {
                    this.move_by(delta, window, cx);
                }
                cx.stop_propagation();
            }))
            .on_pinch(cx.listener(|this, event: &gpui::PinchEvent, window, cx| {
                let anchor = event.position - point(px(0.0), MIRA_CHROME);
                this.zoom(
                    anchor,
                    this.camera(window).zoom * (1.0 + event.delta),
                    window,
                    cx,
                );
                cx.stop_propagation();
            }))
            .child(dot_grid(camera, dot))
            .children(tiles)
            .child(
                h_flex()
                    .absolute()
                    .bottom(px(16.0))
                    .left(px(16.0))
                    .px(px(8.0))
                    .py(px(4.0))
                    .rounded(px(6.0))
                    .bg(fill_color)
                    .child(meta(
                        "Scroll to pan · Pinch to zoom · Click a window to inspect",
                        cx,
                    )),
            )
            .child(
                h_flex()
                    .absolute()
                    .bottom(px(16.0))
                    .right(px(16.0))
                    .gap(px(4.0))
                    .p(px(4.0))
                    .bg(background)
                    .border_1()
                    .border_color(border)
                    .rounded(px(10.0))
                    .shadow(cx.theme().floating())
                    .child(
                        Button::icon("zoom-out", Icon::Minus, "Zoom out")
                            .size(Control::Small)
                            .disabled(zoom <= MIN_ZOOM)
                            .on_click(cx.listener(move |view, _, window, cx| {
                                view.zoom(center, zoom / 1.2, window, cx)
                            })),
                    )
                    .child(
                        div()
                            .w(px(42.0))
                            .text_center()
                            .font_features(tabular())
                            .child(format!("{:.0}%", zoom * 100.0)),
                    )
                    .child(
                        Button::icon("zoom-in", Icon::Plus, "Zoom in")
                            .size(Control::Small)
                            .disabled(zoom >= MAX_ZOOM)
                            .on_click(cx.listener(move |view, _, window, cx| {
                                view.zoom(center, zoom * 1.2, window, cx)
                            })),
                    )
                    .child(vertical_edge(cx))
                    .child(
                        Button::new("fit", "Fit")
                            .variant(Variant::Ghost)
                            .size(Control::Small)
                            .tooltip("Fit all windows (⌘0)")
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.camera = None;
                                cx.notify();
                            })),
                    ),
            )
    }

    fn inspect(&self, screen: Screen, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let index = FRAMES
            .iter()
            .position(|frame| frame.screen == screen)
            .expect("screen has a frame");
        let area = Self::viewer_size(window);
        let size = screen.frame().size();
        let margin = px(24.0);
        let width = area.width.min(size.width);
        let height = (area.height - margin * 2.0).min(size.height);
        div()
            .id("inspect")
            .track_focus(&self.inspect_focus)
            .flex_1()
            .min_h_0()
            .flex()
            .justify_center()
            .items_center()
            .overflow_hidden()
            .bg(palette(cx).muted)
            .child(
                div()
                    .relative()
                    .w(width)
                    .h(height)
                    .rounded(px(10.0))
                    .overflow_hidden()
                    .border_1()
                    .border_color(palette(cx).border)
                    .shadow(cx.theme().floating())
                    .child(self.scenes[index].clone())
                    .child(Toaster::layer(window, cx)),
            )
    }
}

impl Render for Storyboard {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let dark = cx.theme().base.l < 0.5;
        let theme = usize::from(dark);
        if self.current.is_none() && matches!(self.previews[theme], PreviewSet::Empty) {
            self.previews[theme] = PreviewSet::Rendering;
            let task = cx.background_spawn(async move { render_previews(dark) });
            cx.spawn(async move |board, cx| {
                let previews = task.await.unwrap_or_else(|error| {
                    eprintln!("couldn't prepare storyboard previews: {error:#}");
                    [const { None }; FRAMES.len()]
                });
                board
                    .update(cx, |board, cx| {
                        board.previews[theme] = PreviewSet::Ready(previews);
                        cx.notify();
                    })
                    .ok();
            })
            .detach();
        }
        let header = self.header(dark, window, cx);
        let body = match self.current {
            Some(screen) => self.inspect(screen, window, cx).into_any_element(),
            None => self.canvas(window, cx, dark).into_any_element(),
        };
        surface(cx)
            .id("design-canvas")
            .size_full()
            .key_context(FOCUS_SCOPE)
            .track_focus(&self.focus)
            .child(nav::pointer_probe())
            .on_key_down(cx.listener(Self::keys))
            .on_key_up(cx.listener(|view, event: &gpui::KeyUpEvent, _, cx| {
                if event.keystroke.key == "space" && view.space {
                    view.space = false;
                    cx.notify();
                }
            }))
            .child(header)
            .child(body)
    }
}

fn dot_grid(camera: Camera, color: gpui::Hsla) -> AnyElement {
    canvas(
        |_, _, _| {},
        move |bounds, (), window, _| {
            let step = (64.0 * camera.zoom).max(24.0);
            let offset_x = f32::from(camera.origin.x).rem_euclid(step);
            let offset_y = f32::from(camera.origin.y).rem_euclid(step);
            let extent = bounds.size;
            let mut x = offset_x;
            while x < f32::from(extent.width) {
                let mut y = offset_y;
                while y < f32::from(extent.height) {
                    window.paint_quad(fill(
                        Bounds::new(bounds.origin + point(px(x), px(y)), size(px(1.0), px(1.0))),
                        color,
                    ));
                    y += step;
                }
                x += step;
            }
        },
    )
    .absolute()
    .inset_0()
    .into_any_element()
}

#[cfg(feature = "storyboard")]
pub(crate) fn run() -> anyhow::Result<()> {
    use gpui::{KeyBinding, TitlebarOptions, WindowAppearance, WindowBounds, WindowOptions};

    gpui_platform::application()
        .with_assets(mira_ui::Assets)
        .run(|cx| {
            mira_ui::init(cx);
            if let Err(error) = Theme::load_mira_font(cx) {
                eprintln!("couldn't load the canvas font: {error:#}");
            }
            let dark = matches!(
                cx.window_appearance(),
                WindowAppearance::Dark | WindowAppearance::VibrantDark
            );
            cx.set_global(Theme::mira(dark));
            cx.bind_keys([KeyBinding::new("cmd-q", crate::Quit, None)]);
            cx.on_action(|_: &crate::Quit, cx| cx.quit());
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1400.0), px(960.0)),
                    cx,
                ))),
                window_min_size: Some(size(px(720.0), px(480.0))),
                titlebar: Some(TitlebarOptions {
                    title: Some("Mira design canvas".into()),
                    appears_transparent: true,
                    traffic_light_position: Some(point(px(12.0), px(12.0))),
                }),
                app_owns_titlebar_drag: true,
                ..Default::default()
            };
            match cx.open_window(options, |window, cx| {
                let storyboard = cx.new(Storyboard::new);
                window.focus(&storyboard.read(cx).viewport_focus.clone(), cx);
                storyboard
            }) {
                Ok(handle) => {
                    let id = handle.window_id();
                    cx.on_window_closed(move |cx, window| {
                        if window == id {
                            cx.quit();
                        }
                    })
                    .detach();
                    cx.activate(true);
                }
                Err(error) => {
                    eprintln!("couldn't open the canvas: {error:#}");
                    cx.quit();
                }
            }
        });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storyboard::{library, tools};

    #[test]
    fn storyboard_zoom_keeps_the_pointer_over_the_same_world_point() {
        let mut camera = Camera::fit(size(px(1400.0), px(900.0)));
        let anchor = point(px(460.0), px(310.0));
        let relative = (anchor - camera.origin) * (1.0 / camera.zoom);
        camera.zoom_at(anchor, 1.8);
        let projected = camera.origin + relative * camera.zoom;
        assert!((f32::from(projected.x - anchor.x)).abs() < 0.01);
        assert!((f32::from(projected.y - anchor.y)).abs() < 0.01);
    }

    #[gpui::test]
    fn storyboard_can_inspect_a_frame_and_return_to_the_same_camera(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            mira_ui::init(cx);
            cx.set_global(Theme::mira(false));
            cx.set_reduce_motion(true);
        });
        let (storyboard, cx) = cx.add_window_view(|window, cx| {
            window.activate_window();
            let storyboard = Storyboard::new(cx);
            window.focus(&storyboard.viewport_focus, cx);
            storyboard
        });
        cx.run_until_parked();
        let (camera, hit) = cx.update(|window, cx| {
            let camera = storyboard.read(cx).camera(window);
            let hit = camera.frame(Screen::Accounts.frame()).center() + point(px(0.0), MIRA_CHROME);
            (camera, hit)
        });
        cx.simulate_click(hit, gpui::Modifiers::none());
        cx.run_until_parked();
        cx.update(|_, cx| assert_eq!(storyboard.read(cx).current, Some(Screen::Accounts)));
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        cx.update(|window, cx| {
            let storyboard = storyboard.read(cx);
            assert_eq!(storyboard.current, None);
            assert_eq!(storyboard.camera(window).origin, camera.origin);
            assert_eq!(storyboard.camera(window).zoom, camera.zoom);
        });
    }

    #[gpui::test]
    fn storyboard_status_menu_preserves_loader_layout(cx: &mut gpui::TestAppContext) {
        cx.update(|cx| {
            mira_ui::init(cx);
            cx.set_global(Theme::mira(false));
            cx.set_reduce_motion(true);
        });
        let (_, cx) = cx.add_window_view(|window, cx| {
            window.activate_window();
            let mut storyboard = Storyboard::new(cx);
            storyboard.current = Some(Screen::Loader);
            storyboard
        });
        let bounds = |cx: &mut gpui::VisualTestContext| {
            [
                cx.debug_bounds("loader-search"),
                cx.debug_bounds("loader-results"),
                cx.debug_bounds("loader-inspector"),
                cx.debug_bounds("loader-shop-controls"),
            ]
        };
        for (width, wide) in [(1000.0, true), (720.0, false)] {
            cx.simulate_resize(size(px(width), px(680.0)));
            cx.run_until_parked();
            let initial = bounds(cx);
            assert!(initial[0].is_some() && initial[1].is_some() && initial[3].is_some());
            assert_eq!(initial[2].is_some(), wide);
            assert!(cx.debug_bounds("status-preview").is_none());
            let menu = cx.debug_bounds("status-menu").unwrap().center();
            for (keys, visible) in [
                ("home down enter", true),
                ("home down down enter", true),
                ("home enter", false),
            ] {
                cx.simulate_click(menu, gpui::Modifiers::none());
                cx.simulate_keystrokes(keys);
                cx.run_until_parked();
                assert_eq!(cx.debug_bounds("status-preview").is_some(), visible);
                assert_eq!(bounds(cx), initial, "at width {width} after {keys}");
            }
        }
    }

    #[test]
    fn storyboard_snapshot() {
        mira_ui::snapshot("design-canvas", size(px(1400.0), px(960.0)), |_, cx| {
            let dark = cx.theme().base.l < 0.5;
            Theme::load_mira_font(cx).unwrap();
            cx.set_global(Theme::mira(dark));
            cx.new(Storyboard::new)
        });
        for frame in FRAMES {
            let name = frame.screen.name().to_lowercase().replace([' ', '/'], "-");
            mira_ui::snapshot(&format!("design-{name}"), frame.size(), |_, cx| {
                let dark = cx.theme().base.l < 0.5;
                Theme::load_mira_font(cx).unwrap();
                cx.set_global(Theme::mira(dark));
                let sessions = cx.new(|_| Sessions::default());
                let view = frame.build(None, sessions, cx);
                cx.new(|_| Preview(view))
            });
        }
        for (screen, width) in [
            (Screen::Scripts, 620.0),
            (Screen::Loader, 720.0),
            (Screen::Environment, 520.0),
            (Screen::Packets, 710.0),
            (Screen::Follower, 520.0),
            (Screen::Profiles, 740.0),
        ] {
            let frame = screen.frame();
            let name = screen.name().to_lowercase().replace([' ', '/'], "-");
            mira_ui::snapshot(
                &format!("design-{name}-compact"),
                size(px(width), frame.size().height),
                |_, cx| {
                    let dark = cx.theme().base.l < 0.5;
                    Theme::load_mira_font(cx).unwrap();
                    cx.set_global(Theme::mira(dark));
                    let sessions = cx.new(|_| Sessions::default());
                    let view = frame.build(None, sessions, cx);
                    cx.new(|_| Preview(view))
                },
            );
        }
    }

    #[test]
    fn storyboard_alternate_views() {
        fn mira(cx: &mut App) {
            let dark = cx.theme().base.l < 0.5;
            Theme::load_mira_font(cx).unwrap();
            cx.set_global(Theme::mira(dark));
        }
        mira_ui::snapshot(
            "design-scripts-queue",
            Screen::Scripts.frame().size(),
            |_, cx| {
                mira(cx);
                let sessions = cx.new(|_| Sessions::default());
                cx.new(|cx| library::Scripts::new(sessions, cx).preview_queue())
            },
        );
        mira_ui::snapshot(
            "design-packets-send",
            Screen::Packets.frame().size(),
            |_, cx| {
                mira(cx);
                let sessions = cx.new(|_| Sessions::default());
                cx.new(|cx| tools::Packets::new(sessions, cx).preview_send())
            },
        );
        mira_ui::snapshot(
            "design-settings-shortcuts",
            Screen::Settings.frame().size(),
            |_, cx| {
                mira(cx);
                cx.new(|cx| tools::Settings::new(cx).preview_shortcuts())
            },
        );
        mira_ui::snapshot(
            "design-settings-updates",
            Screen::Settings.frame().size(),
            |_, cx| {
                mira(cx);
                cx.set_global(StatusPreview::Working);
                cx.new(|cx| tools::Settings::new(cx).preview_updates())
            },
        );
        mira_ui::snapshot(
            "design-environment-fetch",
            Screen::Environment.frame().size(),
            |_, cx| {
                mira(cx);
                let sessions = cx.new(|_| Sessions::default());
                cx.new(|cx| tools::Environment::new(sessions, cx).preview_fetch())
            },
        );
        for (screen, preview, suffix) in [
            (Screen::Scripts, StatusPreview::Working, "status"),
            (Screen::Environment, StatusPreview::Working, "loading"),
            (Screen::Environment, StatusPreview::Problem, "status"),
            (Screen::Follower, StatusPreview::Problem, "status"),
            (Screen::Loader, StatusPreview::Working, "loading"),
            (Screen::Loader, StatusPreview::Problem, "status"),
            (Screen::Game, StatusPreview::Problem, "status"),
        ] {
            let frame = screen.frame();
            let name = screen.name().to_lowercase().replace([' ', '/'], "-");
            mira_ui::snapshot(&format!("design-{name}-{suffix}"), frame.size(), |_, cx| {
                mira(cx);
                cx.set_global(preview);
                let sessions = cx.new(|_| Sessions::default());
                let view = frame.build(None, sessions, cx);
                cx.new(|_| Preview(view))
            });
        }
        mira_ui::snapshot("design-toast", size(px(1100.0), px(760.0)), |_, cx| {
            mira(cx);
            Toaster::push(Toast::new("Profile saved").body("Relic farm"), cx);
            cx.new(|cx| {
                let mut storyboard = Storyboard::new(cx);
                storyboard.current = Some(Screen::Profiles);
                storyboard
            })
        });
    }
}
