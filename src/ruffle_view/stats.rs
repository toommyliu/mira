use gpui::{
    Anchor, App, Bounds, DispatchPhase, Entity, Hitbox, HitboxBehavior, InteractiveElement as _,
    IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement as _,
    PathBuilder, Pixels, Point, Size, StatefulInteractiveElement as _, Styled as _, Window,
    anchored, canvas, div, fill, point, px, size,
};
use mira_ui::{
    Theme,
    theme::{SYSTEM_MONO, TEXT_CAPTION},
};

use crate::ruffle::stats::{GRAPH_SAMPLES, Report, Timing};

const GRAPH_WIDTH: f32 = 208.0;
const GRAPH_HEIGHT: f32 = 40.0;
const MARGIN: Pixels = px(8.0);

#[derive(Default)]
pub(super) struct Placement {
    position: Option<Point<Pixels>>,
    grab: Option<Point<Pixels>>,
}

pub(super) fn overlay(
    report: Report,
    placement: &Placement,
    area: Bounds<Pixels>,
    this: Entity<super::RuffleView>,
) -> impl IntoElement {
    let fps = report.game_fps.map_or_else(
        || format!("FPS:        - / {:.0}", report.target_fps),
        |fps| format!("FPS:    {fps:>5.1} / {:.0}", report.target_fps),
    );
    let logic = format!("CPU tick: {}", timing(report.logic));
    let render = format!("CPU draw: {}", timing(report.render));
    let (anchor, position) = placement.position.map_or_else(
        || {
            (
                Anchor::TopRight,
                point(area.right() - MARGIN, area.top() + MARGIN),
            )
        },
        |position| (Anchor::TopLeft, position),
    );
    let scale_ms = report
        .frames()
        .map(|frame| frame.logic_ms.max(frame.render_ms))
        .fold(4.0, f64::max);
    let theme = Theme::dark();
    let (logic_color, render_color) = (theme.accent, theme.text);
    let (column_line, row_line, axis) = (theme.border_strong, theme.border, theme.text_secondary);
    let panel = div()
        .id("player-stats")
        .role(gpui::Role::Group)
        .aria_label(format!("Player statistics. {fps}. {logic}. {render}."))
        .aria_description("Movie FPS and target FPS. CPU elapsed times are average, minimum, and maximum milliseconds per render. GPU execution is excluded. Drag to move.")
        .relative()
        .occlude()
        .cursor_move()
        .p_2()
        .w(px(GRAPH_WIDTH + 16.0))
        .bg(theme.overlay.opacity(0.75))
        .text_color(theme.text)
        .text_size(TEXT_CAPTION)
        .line_height(gpui::relative(1.3))
        .font_family(SYSTEM_MONO)
        .flex()
        .flex_col()
        .child(div().flex().justify_between().child("Metal").child(format!("[{}×{}]", report.width, report.height)))
        .child(fps)
        .child(div().text_color(logic_color).child(logic))
        .child(div().text_color(render_color).child(render))
        .child(
            div().relative().mt_1().w(px(GRAPH_WIDTH)).h(px(GRAPH_HEIGHT))
                .child(canvas(
                    |_, _, _| (),
                    move |bounds, _, window, _| {
                        for x in 0..=8 {
                            window.paint_quad(fill(
                                Bounds::new(bounds.origin + point(px(x as f32 * (GRAPH_WIDTH - 1.0) / 8.0), px(0.0)), size(px(1.0), px(GRAPH_HEIGHT))),
                                column_line,
                            ));
                        }
                        for y in [0.0, GRAPH_HEIGHT / 2.0, GRAPH_HEIGHT - 1.0] {
                            window.paint_quad(fill(
                                Bounds::new(bounds.origin + point(px(0.0), px(y)), size(px(GRAPH_WIDTH), px(1.0))),
                                row_line,
                            ));
                        }
                        if report.frames().len() >= 2 {
                            for (color, render) in [(logic_color, false), (render_color, true)] {
                                let mut path = PathBuilder::stroke(px(1.0));
                                for (index, frame) in report.frames().enumerate() {
                                    let ms = if render { frame.render_ms } else { frame.logic_ms };
                                    let x = GRAPH_WIDTH - 1.0 - (report.frames().len() - 1 - index) as f32 * (GRAPH_WIDTH - 2.0) / (GRAPH_SAMPLES - 1) as f32;
                                    let y = GRAPH_HEIGHT - 1.0 - (ms / scale_ms) as f32 * (GRAPH_HEIGHT - 2.0);
                                    let position = bounds.origin + point(px(x), px(y));
                                    if index == 0 { path.move_to(position); } else { path.line_to(position); }
                                }
                                if let Ok(path) = path.build() { window.paint_path(path, color); }
                            }
                        }
                        for rect in [
                            Bounds::new(bounds.origin, size(px(1.0), px(GRAPH_HEIGHT))),
                            Bounds::new(bounds.origin + point(px(0.0), px(GRAPH_HEIGHT - 1.0)), size(px(GRAPH_WIDTH), px(1.0))),
                        ] {
                            window.paint_quad(fill(rect, axis));
                        }
                    },
                ).size_full())
                .child(div().absolute().top_0().left_1().text_size(px(9.0)).text_color(theme.text_secondary).child(format!("{scale_ms:.0} ms"))),
        )
        .child(canvas(
            |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::BlockMouse),
            move |_, hitbox, window, cx| paint_drag(this.clone(), hitbox, window, cx),
        ).absolute().top_0().left_0().size_full());
    anchored()
        .anchor(anchor)
        .position(position)
        .snap_to_window_with_margin(gpui::Edges {
            top: area.top() + MARGIN,
            ..gpui::Edges::all(MARGIN)
        })
        .child(panel)
}

fn timing(timing: Option<Timing>) -> String {
    timing.map_or_else(
        || "   - [   -    -] ms".to_owned(),
        |timing| {
            format!(
                "{:>4.1} [{:>4.1} {:>4.1}] ms",
                timing.average_ms, timing.min_ms, timing.max_ms
            )
        },
    )
}

fn paint_drag(this: Entity<super::RuffleView>, hitbox: Hitbox, window: &mut Window, _: &mut App) {
    let down = this.clone();
    let down_hitbox = hitbox.clone();
    window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
        if phase == DispatchPhase::Capture
            && event.button == MouseButton::Left
            && down_hitbox.is_hovered(window)
        {
            down.update(cx, |this, cx| {
                this.stats_placement.grab = Some(event.position - down_hitbox.origin);
                window.capture_pointer(down_hitbox.id);
                cx.stop_propagation();
            });
        }
    });
    let moving = this.clone();
    window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
        if phase != DispatchPhase::Capture {
            return;
        }
        moving.update(cx, |this, cx| {
            let Some(grab) = this.stats_placement.grab else {
                return;
            };
            if event.pressed_button == Some(MouseButton::Left) {
                this.stats_placement.position = Some(clamp_position(
                    event.position - grab,
                    hitbox.bounds.size,
                    this.player_bounds,
                ));
                cx.stop_propagation();
                cx.notify();
            } else {
                this.stats_placement.grab = None;
                window.release_pointer();
            }
        });
    });
    window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
        if phase == DispatchPhase::Capture && event.button == MouseButton::Left {
            this.update(cx, |this, cx| {
                if this.stats_placement.grab.take().is_some() {
                    cx.stop_propagation();
                }
            });
        }
    });
}

fn clamp_position(
    origin: Point<Pixels>,
    panel: Size<Pixels>,
    area: Bounds<Pixels>,
) -> Point<Pixels> {
    let min = area.origin + point(MARGIN, MARGIN);
    let max = point(
        (area.right() - panel.width - MARGIN).max(min.x),
        (area.bottom() - panel.height - MARGIN).max(min.y),
    );
    point(origin.x.clamp(min.x, max.x), origin.y.clamp(min.y, max.y))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dragging_keeps_the_panel_inside_the_player() {
        let panel = size(px(224.0), px(120.0));
        let player = Bounds::new(point(px(0.0), px(34.0)), size(px(960.0), px(550.0)));
        assert_eq!(
            clamp_position(point(px(100.0), px(200.0)), panel, player),
            point(px(100.0), px(200.0))
        );
        assert_eq!(
            clamp_position(point(px(-500.0), px(-500.0)), panel, player),
            point(px(8.0), px(42.0))
        );
        assert_eq!(
            clamp_position(point(px(1500.0), px(1500.0)), panel, player),
            point(px(728.0), px(456.0))
        );
    }
}
