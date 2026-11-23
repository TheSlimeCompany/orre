//! The status bar (`docs/DESIGN.md` → "Components" → **Status bar**, Paper
//! `RG-0`; on the Transactions artboard `1CU-0`): import status, the agent
//! connection, counts and the home currency, along the bottom of the window.

use std::rc::Rc;

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    Animation, AnimationExt as _, App, HighlightStyle, InteractiveElement as _, IntoElement,
    ParentElement as _, RenderOnce, Rgba, SharedString, StatefulInteractiveElement as _,
    Styled as _, StyledText, TestSupportExt as _, Window, div,
};

use crate::theme::tokens::{color, motion, radius, size, spacing, typography};
use crate::ui::icon::Glyph;
use crate::ui::separator;
use crate::ui::text::TextRoleExt as _;

/// What leads a [`StatusItem`].
#[derive(Clone, Copy, Debug, PartialEq)]
enum Lead {
    /// A 6px status dot.
    Dot(Rgba),
    /// A 12px icon in `text`.
    Icon(Glyph),
    /// A 12px icon in `text-muted`: a state that's off ("AI access off").
    MutedIcon(Glyph),
    /// A 10px sparkle in `moldavite`: an AI client's arrival (LU.5).
    Sparkle,
}

/// One item in the status bar: caption text in `text-muted` (or
/// `text-secondary`), optionally in Geist Mono, optionally led by a status
/// dot or an icon, optionally with spans in `moldavite-text`.
#[derive(Clone, Debug, PartialEq)]
pub struct StatusItem {
    text: SharedString,
    /// The text after `text`, in order, each with whether it is drawn in
    /// `moldavite-text`.
    spans: Vec<(SharedString, bool)>,
    secondary: bool,
    mono: bool,
    lead: Option<Lead>,
    action: Option<Action>,
    fade: Option<Fade>,
}

/// An item fading in or out (LU.5's arrival line): `--motion-base` in, its
/// exit out. GPUI skips both under reduced motion.
#[derive(Clone, Debug, PartialEq)]
struct Fade {
    /// The animation's key: a new key starts the fade again.
    id: SharedString,
    out: bool,
}

/// A click handler.
type Handler = Rc<dyn Fn(&mut Window, &mut App)>;

/// What clicking an item does, under an element id.
#[derive(Clone)]
struct Action {
    id: SharedString,
    handler: Handler,
}

impl std::fmt::Debug for Action {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Action").field(&self.id).finish()
    }
}

impl PartialEq for Action {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl StatusItem {
    /// Caption text in Inter.
    pub fn text(text: impl Into<SharedString>) -> Self {
        Self {
            text: text.into(),
            spans: Vec::new(),
            secondary: false,
            mono: false,
            lead: None,
            action: None,
            fade: None,
        }
    }

    /// Fades the item in over `--motion-base` (`out` false) or out at its
    /// exit, keyed by `id`: the fade runs once per key.
    pub fn fade(mut self, id: impl Into<SharedString>, out: bool) -> Self {
        self.fade = Some(Fade { id: id.into(), out });
        self
    }

    /// Runs `handler` when the item is clicked: "1 agent connected" and
    /// "AI access off" open Settings → AI clients (`DESIGN.md` →
    /// "Settings"). `id` is the item's element id.
    pub fn on_click(
        mut self,
        id: impl Into<SharedString>,
        handler: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        self.action = Some(Action {
            id: id.into(),
            handler: Rc::new(handler),
        });
        self
    }

    /// A count or a code ("328 transactions", "GBP"), in Geist Mono.
    pub fn mono(text: impl Into<SharedString>) -> Self {
        Self {
            mono: true,
            ..Self::text(text)
        }
    }

    /// Draws the text in `text-secondary`, one step above the other items:
    /// the arrival line (LU.5).
    pub fn secondary(mut self) -> Self {
        self.secondary = true;
        self
    }

    /// Adds `text` after the item's text, in `moldavite-text` when
    /// `emphasis`: a client's name in the arrival line.
    pub fn span(mut self, text: impl Into<SharedString>, emphasis: bool) -> Self {
        self.spans.push((text.into(), emphasis));
        self
    }

    /// Leads the text with a 10px `moldavite` sparkle: an AI client's
    /// arrival.
    pub fn sparkle(mut self) -> Self {
        self.lead = Some(Lead::Sparkle);
        self
    }

    /// Leads the text with a `moldavite` dot: an agent is connected over
    /// MCP.
    pub fn agent_dot(mut self) -> Self {
        self.lead = Some(Lead::Dot(color::MOLDAVITE));
        self
    }

    /// Leads the text with a `molten` dot: something unread or new.
    pub fn new_dot(mut self) -> Self {
        self.lead = Some(Lead::Dot(color::MOLTEN));
        self
    }

    /// Leads the text with a `danger` dot: a balance check failing (D6.4,
    /// Paper `8SE-0`).
    pub fn danger_dot(mut self) -> Self {
        self.lead = Some(Lead::Dot(color::DANGER));
        self
    }

    /// Leads the text with `glyph` in `text-muted`: the `refused` icon
    /// before "AI access off" (Paper `GXO-0`).
    pub fn muted_icon(mut self, glyph: Glyph) -> Self {
        self.lead = Some(Lead::MutedIcon(glyph));
        self
    }

    /// Leads the text with `glyph` in `text`, as the check before
    /// "Reconciled" (success is neutral: `DESIGN.md` → "Colour semantics").
    pub fn icon(mut self, glyph: Glyph) -> Self {
        self.lead = Some(Lead::Icon(glyph));
        self
    }

    fn render(self) -> Vec<gpui_kit::AnyElement> {
        let lead = self.lead.map(|lead| match lead {
            Lead::Dot(fill) => div()
                .flex_none()
                .size(size::DOT)
                .rounded(radius::LABEL)
                .bg(fill)
                .into_any_element(),
            Lead::Icon(glyph) => glyph.render().text_color(color::TEXT).into_any_element(),
            Lead::MutedIcon(glyph) => glyph
                .render()
                .text_color(color::TEXT_MUTED)
                .into_any_element(),
            Lead::Sparkle => Glyph::Sparkle
                .render_at(size::ICON_XS)
                .text_color(color::MOLDAVITE)
                .into_any_element(),
        });
        // One run of text, so a span in `moldavite-text` sits inline.
        let mut full = self.text.to_string();
        let mut highlights = Vec::new();
        for (span, emphasis) in &self.spans {
            let start = full.len();
            full.push_str(span);
            if *emphasis {
                highlights.push((
                    start..full.len(),
                    HighlightStyle {
                        color: Some(color::MOLDAVITE_TEXT.into()),
                        ..HighlightStyle::default()
                    },
                ));
            }
        }
        let text = div()
            .flex_none()
            .whitespace_nowrap()
            .text_role(if self.mono {
                typography::MONO_VALUE
            } else {
                typography::CAPTION
            })
            .text_color(if self.secondary {
                color::TEXT_SECONDARY
            } else {
                color::TEXT_MUTED
            })
            .child(StyledText::new(full).with_highlights(highlights))
            .into_any_element();
        let parts: Vec<gpui_kit::AnyElement> =
            lead.into_iter().chain(std::iter::once(text)).collect();
        let parts = match self.fade {
            Some(fade) => parts
                .into_iter()
                .enumerate()
                .map(|(index, part)| faded(part, &fade, index))
                .collect(),
            None => parts,
        };
        match self.action {
            // One target for the lead and the text, spaced as the bar spaces
            // them.
            Some(action) => vec![
                div()
                    .id(gpui_kit::ElementId::Name(action.id))
                    // Lets UI tests find it; a no-op outside them.
                    .test_support()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(spacing::S2)
                    .cursor_pointer()
                    .on_click(move |_, window, cx| (action.handler)(window, cx))
                    .children(parts)
                    .into_any_element(),
            ],
            None => parts,
        }
    }
}

/// `part` of an item, fading as `fade` says; `index` keeps each part's
/// animation apart.
fn faded(part: gpui_kit::AnyElement, fade: &Fade, index: usize) -> gpui_kit::AnyElement {
    let id = gpui_kit::ElementId::Name(format!("{}-{index}", fade.id).into());
    let part = div().flex().flex_none().items_center().child(part);
    if fade.out {
        part.with_animation(
            id,
            Animation::new(motion::exit(motion::BASE)),
            |part, delta| part.opacity(1. - delta),
        )
        .into_any_element()
    } else {
        part.with_animation(id, Animation::new(motion::BASE), |part, delta| {
            part.opacity(delta)
        })
        .into_any_element()
    }
}

/// The status bar: 28px, `float`, top `hairline`, 12px inline padding.
/// Items at the start and at the end, each group's items separated by 1×12
/// `border-strong` rules.
#[derive(IntoElement, Default)]
pub struct StatusBar {
    start: Vec<StatusItem>,
    end: Vec<StatusItem>,
}

impl StatusBar {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds an item after the others at the start of the bar.
    pub fn start(mut self, item: StatusItem) -> Self {
        self.start.push(item);
        self
    }

    /// Adds an item after the others at the end of the bar.
    pub fn end(mut self, item: StatusItem) -> Self {
        self.end.push(item);
        self
    }
}

/// `items` with a separator between each pair.
fn separated(items: Vec<StatusItem>) -> Vec<gpui_kit::AnyElement> {
    let mut children = Vec::new();
    for (index, item) in items.into_iter().enumerate() {
        if index > 0 {
            children.push(separator(spacing::S3, color::BORDER_STRONG).into_any_element());
        }
        children.extend(item.render());
    }
    children
}

impl RenderOnce for StatusBar {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(spacing::S2)
            .w_full()
            .h(size::STATUS_BAR)
            .px(spacing::S3)
            .bg(color::FLOAT)
            .border_t(size::HAIRLINE)
            .border_color(color::HAIRLINE)
            .overflow_hidden()
            .children(separated(self.start))
            .child(div().flex_1())
            .when(!self.end.is_empty(), |this| {
                this.children(separated(self.end))
            })
    }
}
