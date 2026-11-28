use gpui::{Anchor, Anchored, Axis, Bounds, Pixels, Point, Size, anchored, point, px};

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

        let (anchor, corner, offset, from) = self.anchors(side);
        (
            anchored()
                .anchor(anchor)
                .position(target.corner(corner) + offset)
                .snap_to_window_with_margin(COLLISION_MARGIN),
            from,
        )
    }

    pub(super) fn anchored_to(self, target: Bounds<Pixels>) -> Anchored {
        let (anchor, corner, offset, _) = self.anchors(self.side);
        anchored()
            .anchor(anchor)
            .attach(target, corner)
            .offset(offset)
            .snap_to_window_with_margin(COLLISION_MARGIN)
    }

    fn anchors(self, side: Side) -> (Anchor, Anchor, Point<Pixels>, From) {
        let align = |start, center, end| match self.align {
            Align::Start => start,
            Align::Center => center,
            Align::End => end,
        };
        let (anchor, axis, offset, from) = match side {
            Side::Top => (
                align(
                    Anchor::BottomLeft,
                    Anchor::BottomCenter,
                    Anchor::BottomRight,
                ),
                Axis::Vertical,
                point(self.align_offset, -self.side_offset),
                From::Below,
            ),
            Side::Bottom => (
                align(Anchor::TopLeft, Anchor::TopCenter, Anchor::TopRight),
                Axis::Vertical,
                point(self.align_offset, self.side_offset),
                From::Above,
            ),
            Side::Left => (
                align(Anchor::TopRight, Anchor::RightCenter, Anchor::BottomRight),
                Axis::Horizontal,
                point(-self.side_offset, self.align_offset),
                From::Right,
            ),
            Side::Right => (
                align(Anchor::TopLeft, Anchor::LeftCenter, Anchor::BottomLeft),
                Axis::Horizontal,
                point(self.side_offset, self.align_offset),
                From::Left,
            ),
        };
        (anchor, anchor.other_side_along(axis), offset, from)
    }
}
