use gpui::{
    App, Div, ElementId, InteractiveElement as _, IntoElement, ParentElement as _, RenderOnce,
    StatefulInteractiveElement as _, Styled as _, Window, div,
};

use crate::{ActiveTheme as _, Tone, motion};

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
        let (mark, label) = match self.tone {
            Tone::Neutral => (super::Icon::LoaderCircle, "Pending"),
            Tone::Accent => (super::Icon::ArrowDownToLine, "Update available"),
            Tone::Success => (super::Icon::CircleCheck, "Succeeded"),
            Tone::Warning => (super::Icon::TriangleAlert, "Warning"),
            Tone::Danger => (super::Icon::OctagonX, "Problem"),
        };
        StatusMark::new(mark, label, self.tone).render_mark(cx)
    }
}

#[derive(IntoElement)]
pub struct StatusMark {
    mark: super::Icon,
    label: &'static str,
    tone: Tone,
}
impl StatusMark {
    pub fn new(mark: super::Icon, label: &'static str, tone: Tone) -> Self {
        Self { mark, label, tone }
    }
    fn render_mark(self, cx: &App) -> gpui::Stateful<Div> {
        div()
            .id(self.label)
            .role(gpui::Role::Image)
            .aria_label(self.label)
            .flex_none()
            .flex()
            .items_center()
            .child(super::icon(self.mark, cx.theme().tone(self.tone).0))
    }
}
impl RenderOnce for StatusMark {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        self.render_mark(cx)
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
        let (mark, ink) = match self.activity {
            Activity::Idle => (super::Icon::Circle, cx.theme().text_tertiary),
            Activity::Running => (super::Icon::CircleDot, cx.theme().text),
            Activity::Waiting => (super::Icon::LoaderCircle, cx.theme().text_tertiary),
            Activity::Stopped => (super::Icon::CircleStop, cx.theme().text_tertiary),
            Activity::Failed => (super::Icon::OctagonX, cx.theme().danger),
        };
        let glyph = super::icon(mark, ink);
        if self.activity == Activity::Waiting {
            motion::spin(self.id, glyph, cx)
        } else {
            glyph.into_any_element()
        }
    }
}
