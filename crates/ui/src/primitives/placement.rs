use gpui::{Anchor, Anchored, Bounds, Pixels, Size, anchored, point, px};

use crate::motion::From;

pub const COLLISION_MARGIN: Pixels = px(5.0);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Align {
    #[default]
    Start,
    Center,
    End,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Top,
    Bottom,
    Left,
    Right,
}

#[derive(Clone, Copy)]
pub struct Placement {
    pub side: Side,
    pub align: Align,
    pub side_offset: Pixels,
    pub align_offset: Pixels,
}

impl Placement {
    pub fn new(side: Side, align: Align) -> Self {
        Self {
            side,
            align,
            side_offset: px(4.0),
            align_offset: Pixels::ZERO,
        }
    }

    pub(super) fn menubar() -> Self {
        Self {
            side_offset: px(8.0),
            align_offset: px(-4.0),
            ..Self::new(Side::Bottom, Align::Start)
        }
    }

    pub(super) fn resolve(
        self,
        target: Bounds<Pixels>,
        panel: Size<Pixels>,
        viewport: Size<Pixels>,
    ) -> (Anchored, From) {
        let room = |side| match side {
            Side::Top => target.top(),
            Side::Bottom => viewport.height - target.bottom(),
            Side::Left => target.left(),
            Side::Right => viewport.width - target.right(),
        };
        let (opposite, extent) = match self.side {
            Side::Top => (Side::Bottom, panel.height),
            Side::Bottom => (Side::Top, panel.height),
            Side::Left => (Side::Right, panel.width),
            Side::Right => (Side::Left, panel.width),
        };
        let side = if room(self.side) < extent + self.side_offset + COLLISION_MARGIN
            && room(opposite) > room(self.side)
        {
            opposite
        } else {
            self.side
        };

        let x = align_at(self.align, target.left(), target.center().x, target.right())
            + self.align_offset;
        let y = align_at(self.align, target.top(), target.center().y, target.bottom())
            + self.align_offset;
        let (origin, corner, from) = match side {
            Side::Top => (
                point(x, target.top() - self.side_offset),
                align_at(
                    self.align,
                    Anchor::BottomLeft,
                    Anchor::BottomCenter,
                    Anchor::BottomRight,
                ),
                From::Below,
            ),
            Side::Bottom => (
                point(x, target.bottom() + self.side_offset),
                align_at(
                    self.align,
                    Anchor::TopLeft,
                    Anchor::TopCenter,
                    Anchor::TopRight,
                ),
                From::Above,
            ),
            Side::Left => (
                point(target.left() - self.side_offset, y),
                align_at(
                    self.align,
                    Anchor::TopRight,
                    Anchor::RightCenter,
                    Anchor::BottomRight,
                ),
                From::Right,
            ),
            Side::Right => (
                point(target.right() + self.side_offset, y),
                align_at(
                    self.align,
                    Anchor::TopLeft,
                    Anchor::LeftCenter,
                    Anchor::BottomLeft,
                ),
                From::Left,
            ),
        };
        (
            anchored()
                .anchor(corner)
                .position(origin)
                .snap_to_window_with_margin(COLLISION_MARGIN),
            from,
        )
    }
}

fn align_at<T>(align: Align, start: T, center: T, end: T) -> T {
    match align {
        Align::Start => start,
        Align::Center => center,
        Align::End => end,
    }
}
