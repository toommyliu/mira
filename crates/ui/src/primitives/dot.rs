use gpui::{App, Div, ElementId, IntoElement, RenderOnce, Styled as _, Window, div, px};

use crate::{ActiveTheme as _, Theme, Tone, motion};

const SIZE: f32 = 7.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Activity {
    Idle,
    Running,
    Waiting,
    Stopped,
    Failed,
}

#[derive(IntoElement)]
pub struct Dot {
    tone: Tone,
}

impl Dot {
    pub fn new(tone: Tone) -> Self {
        Self { tone }
    }
}

impl RenderOnce for Dot {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let (solid, _, _) = cx.theme().tone(self.tone);
        shape().bg(solid)
    }
}

#[derive(IntoElement)]
pub struct StatusDot {
    id: ElementId,
    activity: Activity,
}

impl StatusDot {
    pub fn new(id: impl Into<ElementId>, activity: Activity) -> Self {
        Self {
            id: id.into(),
            activity,
        }
    }
}

impl RenderOnce for StatusDot {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let dot = activity_shape(cx.theme(), self.activity);
        match self.activity {
            Activity::Running => motion::breathe(self.id, dot).into_any_element(),
            _ => dot.into_any_element(),
        }
    }
}

fn shape() -> Div {
    div().flex_none().size(px(SIZE)).rounded_full()
}

fn activity_shape(theme: &Theme, activity: Activity) -> Div {
    match activity {
        Activity::Idle => shape().border_1().border_color(theme.text_tertiary),
        Activity::Running => shape().bg(theme.accent),
        Activity::Waiting => shape().border_2().border_color(theme.warning),
        Activity::Stopped => shape().bg(theme.text_tertiary),
        Activity::Failed => shape().bg(theme.danger),
    }
}
