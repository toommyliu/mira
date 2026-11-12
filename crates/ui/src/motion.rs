use std::time::{Duration, Instant};

use gpui::{
    Animation, AnimationElement, AnimationExt as _, AnyElement, App, ElementId, IntoElement,
    Styled, Svg, Transformation, Window, percentage, px,
};

pub const ARRIVE: Duration = Duration::from_millis(150);
pub const LEAVE: Duration = Duration::from_millis(100);
pub const TRAVEL: Duration = Duration::from_millis(160);
pub const GLIDE: Duration = Duration::from_millis(300);
const BREATH: Duration = Duration::from_millis(2400);
const SWEEP: Duration = Duration::from_millis(1400);
const SPIN: Duration = Duration::from_millis(800);
const ARRIVE_DISTANCE: f32 = 4.0;

pub fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(3)
}

pub fn ease_out_quint(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(5)
}

pub fn ease(t: f32) -> f32 {
    match t {
        t if t <= 0.0 => 0.0,
        t if t >= 1.0 => 1.0,
        t => cubic_bezier((0.25, 0.1), (0.25, 1.0), t),
    }
}

fn cubic_bezier(p1: (f32, f32), p2: (f32, f32), x: f32) -> f32 {
    let curve = |a: f32, b: f32, t: f32| {
        let u = 1.0 - t;
        3.0 * u * u * t * a + 3.0 * u * t * t * b + t * t * t
    };
    let (mut low, mut high) = (0.0, 1.0);
    for _ in 0..24 {
        let mid = (low + high) / 2.0;
        if curve(p1.0, p2.0, mid) < x {
            low = mid;
        } else {
            high = mid;
        }
    }
    curve(p1.1, p2.1, (low + high) / 2.0)
}

#[derive(Clone, Copy)]
pub enum From {
    Above,
    Below,
}

pub fn arrive<E: IntoElement + Styled + 'static>(
    id: impl Into<ElementId>,
    element: E,
    from: From,
) -> AnimationElement<E> {
    element.with_animation(
        id,
        Animation::new(ARRIVE).with_easing(ease_out_quint),
        move |element, t| {
            let shift = px((1.0 - t) * ARRIVE_DISTANCE);
            let element = element.relative().opacity(t);
            match from {
                From::Above => element.top(-shift),
                From::Below => element.top(shift),
            }
        },
    )
}

pub fn fade<E: IntoElement + Styled + 'static>(
    id: impl Into<ElementId>,
    element: E,
) -> AnimationElement<E> {
    element.with_animation(
        id,
        Animation::new(ARRIVE).with_easing(ease_out),
        |element, t| element.opacity(t),
    )
}

pub fn breathe<E: IntoElement + Styled + 'static>(
    id: impl Into<ElementId>,
    element: E,
) -> AnimationElement<E> {
    element.with_animation(id, Animation::new(BREATH).repeat(), |element, t| {
        element.opacity(1.0 - 0.6 * (t * std::f32::consts::PI).sin().powi(2))
    })
}

pub fn spin(id: impl Into<ElementId>, element: Svg, cx: &App) -> AnyElement {
    if cx.reduce_motion() {
        return element.into_any_element();
    }
    element
        .with_animation(id, Animation::new(SPIN).repeat(), |svg, t| {
            svg.with_transformation(Transformation::rotate(percentage(t)))
        })
        .into_any_element()
}

pub fn sweep<E: IntoElement + Styled + 'static>(
    id: impl Into<ElementId>,
    element: E,
    animate: impl Fn(E, f32) -> E + 'static,
) -> AnimationElement<E> {
    element.with_animation(
        id,
        Animation::new(SWEEP)
            .repeat()
            .with_easing(gpui::ease_in_out),
        animate,
    )
}

pub fn leaving(since: Instant, window: &mut Window, cx: &App) -> Option<f32> {
    if cx.reduce_motion() {
        return None;
    }
    let t = since.elapsed().as_secs_f32() / LEAVE.as_secs_f32();
    if t >= 1.0 {
        return None;
    }
    window.request_animation_frame();
    Some(1.0 - ease_out(t))
}

struct Tween {
    from: f32,
    to: f32,
    start: Instant,
}

pub fn travel(id: impl Into<ElementId>, target: f32, window: &mut Window, cx: &mut App) -> f32 {
    tween(id, None, target, TRAVEL, ease_out, window, cx)
}

pub fn travel_from(
    id: impl Into<ElementId>,
    from: f32,
    target: f32,
    window: &mut Window,
    cx: &mut App,
) -> f32 {
    tween(id, Some(from), target, TRAVEL, ease_out, window, cx)
}

pub fn glide(
    id: impl Into<ElementId>,
    from: Option<f32>,
    target: f32,
    window: &mut Window,
    cx: &mut App,
) -> f32 {
    tween(id, from, target, GLIDE, ease, window, cx)
}

fn tween(
    id: impl Into<ElementId>,
    from: Option<f32>,
    target: f32,
    duration: Duration,
    ease: fn(f32) -> f32,
    window: &mut Window,
    cx: &mut App,
) -> f32 {
    if cx.reduce_motion() {
        return target;
    }
    let now = Instant::now();
    let state = window.use_keyed_state(id, cx, |_, _| Tween {
        from: from.unwrap_or(target),
        to: target,
        start: if from.is_some() { now } else { now - duration },
    });
    let (value, moving) = state.update(cx, |tween, _| {
        let progress = |tween: &Tween| {
            (now.duration_since(tween.start).as_secs_f32() / duration.as_secs_f32()).min(1.0)
        };
        if tween.to != target {
            tween.from += (tween.to - tween.from) * ease(progress(tween));
            tween.to = target;
            tween.start = now;
        }
        let p = progress(tween);
        (tween.from + (tween.to - tween.from) * ease(p), p < 1.0)
    });
    if moving {
        window.request_animation_frame();
    }
    value
}

#[cfg(test)]
mod tests {
    use super::ease;

    #[test]
    fn ease_matches_the_css_curve() {
        assert_eq!(ease(0.0), 0.0);
        assert_eq!(ease(1.0), 1.0);
        assert!((ease(0.5) - 0.8024).abs() < 1e-3);
        assert!((ease(0.25) - 0.4085).abs() < 1e-3);
    }
}
