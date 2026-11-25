use std::time::{Duration, Instant};

pub(super) struct Pacing {
    pub(super) visible: bool,
    pub(super) minimal: bool,
    pub(super) resized: bool,
    throttle: Option<Duration>,
    next_render: Instant,
}

impl Pacing {
    pub(super) fn new(now: Instant) -> Self {
        Self {
            visible: true,
            minimal: false,
            resized: false,
            throttle: None,
            next_render: now,
        }
    }

    pub(super) fn set_throttle(&mut self, throttle: Option<Duration>, now: Instant) {
        if throttle != self.throttle {
            self.throttle = throttle;
            self.next_render = now;
        }
    }

    fn presenting(&self) -> bool {
        self.visible && !self.minimal
    }

    pub(super) fn frame_clock(&self, interpolating: bool) -> bool {
        interpolating && self.presenting() && self.throttle.is_none()
    }

    pub(super) fn draw(
        &mut self,
        now: Instant,
        needs_render: bool,
        interpolating: bool,
        frame_due: bool,
    ) -> bool {
        let due = match self.throttle {
            Some(_) => now >= self.next_render,
            None => !interpolating || frame_due,
        };
        let draw = self.presenting() && (self.resized || needs_render && due);
        if draw {
            self.resized = false;
            if let Some(throttle) = self.throttle {
                self.next_render = now + throttle;
            }
        }
        draw
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MODES: [bool; 2] = [false, true];
    const STAGE_FRAME: Duration = Duration::from_millis(41);

    const HIDE: [(&str, fn(&mut Pacing, bool)); 2] = [
        ("occluded", |pacing, hidden| pacing.visible = !hidden),
        ("minimal", |pacing, hidden| pacing.minimal = hidden),
    ];

    #[test]
    fn hidden_window_is_never_drawn() {
        for interpolating in MODES {
            for (cause, hide) in HIDE {
                let start = Instant::now();
                let mut pacing = Pacing::new(start);
                hide(&mut pacing, true);
                for pass in 0..240 {
                    let now = start + STAGE_FRAME * pass;
                    pacing.resized = pass % 2 == 0;
                    assert!(
                        !pacing.draw(now, true, interpolating, true),
                        "{cause} interpolating={interpolating} pass={pass}"
                    );
                }
            }
        }
    }

    #[test]
    fn throttled_view_draws_no_more_often_than_its_throttle() {
        for (interval, per_second) in [(125, 8), (500, 2)] {
            for interpolating in MODES {
                let start = Instant::now();
                let mut pacing = Pacing::new(start);
                pacing.set_throttle(Some(Duration::from_millis(interval)), start);
                let draws = (0..=24)
                    .filter(|pass| {
                        pacing.draw(start + STAGE_FRAME * *pass, true, interpolating, true)
                    })
                    .count();
                assert!(
                    (1..=per_second).contains(&draws),
                    "{draws} draws at {interval} ms with interpolating={interpolating}"
                );
                assert!(!pacing.frame_clock(interpolating));
            }
        }
    }

    #[test]
    fn removing_the_throttle_draws_on_the_next_pass() {
        for interpolating in MODES {
            let start = Instant::now();
            let mut pacing = Pacing::new(start);
            pacing.set_throttle(Some(Duration::from_millis(500)), start);
            assert!(pacing.draw(start, true, interpolating, true));
            let selected = start + STAGE_FRAME;
            pacing.set_throttle(None, selected);
            assert!(
                pacing.draw(selected, true, interpolating, true),
                "interpolating={interpolating}"
            );
        }
    }

    #[test]
    fn resized_view_draws_once_on_the_next_pass_without_a_stage_change() {
        for interpolating in MODES {
            for throttle in [None, Some(Duration::from_millis(500))] {
                let start = Instant::now();
                let mut pacing = Pacing::new(start);
                pacing.set_throttle(throttle, start);
                assert!(pacing.draw(start, true, interpolating, true));
                let next = start + Duration::from_millis(1);
                pacing.resized = true;
                assert!(
                    pacing.draw(next, false, interpolating, false),
                    "interpolating={interpolating} throttle={throttle:?}"
                );
                assert!(!pacing.draw(next, false, interpolating, false));
            }
        }
    }

    #[test]
    fn shown_window_draws_on_its_first_pass() {
        for interpolating in MODES {
            for (cause, hide) in HIDE {
                let start = Instant::now();
                let mut pacing = Pacing::new(start);
                assert!(pacing.draw(start, true, interpolating, true));
                hide(&mut pacing, true);
                for pass in 1..10 {
                    pacing.draw(start + STAGE_FRAME * pass, true, interpolating, true);
                }
                hide(&mut pacing, false);
                assert!(
                    pacing.draw(start + STAGE_FRAME * 10, true, interpolating, true),
                    "{cause} interpolating={interpolating}"
                );
            }
        }
    }

    #[test]
    fn visible_window_draws_each_stage_frame_without_smooth_motion() {
        let start = Instant::now();
        let mut pacing = Pacing::new(start);
        assert!(pacing.draw(start, true, false, false));
        assert!(pacing.draw(start + Duration::from_millis(1), true, false, false));
        assert!(!pacing.draw(start + Duration::from_millis(2), false, false, true));
    }

    #[test]
    fn visible_window_with_smooth_motion_draws_on_the_display_link() {
        let start = Instant::now();
        let mut pacing = Pacing::new(start);
        assert!(!pacing.draw(start, true, true, false));
        assert!(pacing.draw(start, true, true, true));
    }

    #[test]
    fn display_link_runs_only_while_smooth_motion_is_on_and_the_window_is_visible() {
        for (cause, hide) in HIDE {
            let mut pacing = Pacing::new(Instant::now());
            assert!(pacing.frame_clock(true));
            assert!(!pacing.frame_clock(false));
            hide(&mut pacing, true);
            assert!(!pacing.frame_clock(true), "{cause}");
            hide(&mut pacing, false);
            assert!(pacing.frame_clock(true), "{cause}");
        }
    }
}
