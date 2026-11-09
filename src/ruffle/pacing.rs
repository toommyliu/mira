use std::time::{Duration, Instant};

pub(super) struct Pacing {
    display_frames: bool,
    pub(super) visible: bool,
    pub(super) minimal: bool,
    next_render: Instant,
}

impl Pacing {
    pub(super) fn new(display_frames: bool, now: Instant) -> Self {
        Self {
            display_frames,
            visible: true,
            minimal: false,
            next_render: now,
        }
    }

    fn presenting(&self) -> bool {
        self.visible && !self.minimal
    }

    pub(super) fn frame_clock(&self, interpolating: bool) -> bool {
        self.display_frames && interpolating && self.presenting()
    }

    pub(super) fn draw(
        &mut self,
        now: Instant,
        needs_render: bool,
        interpolating: bool,
        frame_due: bool,
    ) -> bool {
        let paced = if self.display_frames {
            frame_due
        } else {
            now >= self.next_render
        };
        let draw = self.presenting() && needs_render && (!interpolating || paced);
        if draw {
            self.next_render = now + Duration::from_secs_f64(1.0 / 60.0);
        }
        draw
    }

    pub(super) fn wait(
        &self,
        now: Instant,
        interpolating: bool,
        until_next_frame: Duration,
    ) -> Duration {
        if interpolating && !self.display_frames && self.presenting() {
            until_next_frame.min(self.next_render.saturating_duration_since(now))
        } else {
            until_next_frame
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MODES: [(bool, bool); 4] = [(false, false), (false, true), (true, false), (true, true)];
    const STAGE_FRAME: Duration = Duration::from_millis(41);

    const HIDE: [(&str, fn(&mut Pacing, bool)); 2] = [
        ("occluded", |pacing, hidden| pacing.visible = !hidden),
        ("minimal", |pacing, hidden| pacing.minimal = hidden),
    ];

    #[test]
    fn hidden_window_is_never_drawn() {
        for (display_frames, interpolating) in MODES {
            for (cause, hide) in HIDE {
                let start = Instant::now();
                let mut pacing = Pacing::new(display_frames, start);
                hide(&mut pacing, true);
                for pass in 0..240 {
                    let now = start + STAGE_FRAME * pass;
                    assert!(
                        !pacing.draw(now, true, interpolating, true),
                        "{cause} display_frames={display_frames} interpolating={interpolating} pass={pass}"
                    );
                }
            }
        }
    }

    #[test]
    fn shown_window_draws_on_its_first_pass() {
        for (display_frames, interpolating) in MODES {
            for (cause, hide) in HIDE {
                let start = Instant::now();
                let mut pacing = Pacing::new(display_frames, start);
                assert!(pacing.draw(start, true, interpolating, true));
                hide(&mut pacing, true);
                for pass in 1..10 {
                    pacing.draw(start + STAGE_FRAME * pass, true, interpolating, true);
                }
                hide(&mut pacing, false);
                assert!(
                    pacing.draw(start + STAGE_FRAME * 10, true, interpolating, true),
                    "{cause} display_frames={display_frames} interpolating={interpolating}"
                );
            }
        }
    }

    #[test]
    fn visible_window_draws_each_stage_frame_without_smooth_motion() {
        for display_frames in [false, true] {
            let start = Instant::now();
            let mut pacing = Pacing::new(display_frames, start);
            assert!(pacing.draw(start, true, false, false));
            assert!(pacing.draw(start + Duration::from_millis(1), true, false, false));
            assert!(!pacing.draw(start + Duration::from_millis(2), false, false, true));
        }
    }

    #[test]
    fn visible_window_with_smooth_motion_draws_on_the_display_link() {
        let start = Instant::now();
        let mut pacing = Pacing::new(true, start);
        assert!(!pacing.draw(start, true, true, false));
        assert!(pacing.draw(start, true, true, true));
    }

    #[test]
    fn visible_window_with_smooth_motion_draws_at_60hz_without_a_display_link() {
        let start = Instant::now();
        let mut pacing = Pacing::new(false, start);
        assert!(pacing.draw(start, true, true, false));
        assert!(!pacing.draw(start + Duration::from_millis(10), true, true, false));
        assert!(pacing.draw(start + Duration::from_millis(17), true, true, false));
    }

    #[test]
    fn display_link_runs_only_while_smooth_motion_is_on_and_the_window_is_visible() {
        for (cause, hide) in HIDE {
            let mut pacing = Pacing::new(true, Instant::now());
            assert!(pacing.frame_clock(true));
            assert!(!pacing.frame_clock(false));
            hide(&mut pacing, true);
            assert!(!pacing.frame_clock(true), "{cause}");
            hide(&mut pacing, false);
            assert!(pacing.frame_clock(true), "{cause}");
        }
    }

    #[test]
    fn hidden_window_with_smooth_motion_waits_for_the_next_stage_frame() {
        let start = Instant::now();
        let mut pacing = Pacing::new(false, start);
        pacing.draw(start, true, true, false);
        let later = start + Duration::from_secs(1);
        assert_eq!(pacing.wait(later, true, STAGE_FRAME), Duration::ZERO);
        pacing.visible = false;
        assert_eq!(pacing.wait(later, true, STAGE_FRAME), STAGE_FRAME);
    }
}
