//! The review queues' canvas and inspector, in the Inbox's place while a
//! CHECKS row is selected (`docs/DESIGN.md` → "Review queues"; Paper
//! `8SE-0`, `98K-0`, `9MQ-0`).
//!
//! The toolbar (52px) names the queue, with the hints of its keys
//! ([`KEYS`]) for Possible duplicates (M Merge, N Not duplicates, ←→ Keep,
//! ↑↓ Select); the body is
//! [`super::duplicates`] or [`super::balances`]; after a merge or a
//! dismissal the D6.3 undo bar floats at the foot of the canvas. A change
//! core refuses is said under the toolbar in caption type, `danger`.

use gpui_kit::base::actions::Cancel;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, AppContext as _, ClickEvent, Context, Entity, FocusHandle, Focusable as _,
    InteractiveElement as _, IntoElement, ParentElement as _, Render, SharedString, Styled as _,
    Subscription, Window, div,
};

use crate::theme::tokens::{color, size, spacing, typography};
use crate::ui::icon::Glyph;
use crate::ui::keys::{Key, ViewKeys, hints};
use crate::ui::list::{Keys, ListKeysExt as _, Move};
use crate::ui::text::TextRoleExt as _;
use crate::ui::tracked::Tracked;
use crate::ui::undo_bar::UndoBar;

use super::text::count;
use super::{Checks, Outcome, Queue};

/// The key context of the queues' canvas, where M, N, ←→ and ↑↓ work.
pub const CONTEXT: &str = "CofferChecks";

/// The queues' keys, declared once (ROADMAP U.7): [`super::init`] binds
/// them and the toolbar hints them. ↑↓ move through the open queue's rows;
/// M, N and ←→ decide the selected pair in Possible duplicates; Escape
/// (not hinted) clears the selection.
pub const KEYS: ViewKeys = ViewKeys::new(
    Keys::new(CONTEXT),
    &[
        Key::new::<MergeSelected>(&["m"], "M", super::duplicates::MERGE),
        Key::new::<DismissSelected>(&["n"], "N", super::duplicates::NOT_DUPLICATES),
        Key::new::<KeepOther>(&["left", "right"], "←→", "Keep"),
        Key::new::<Cancel>(&["escape"], "Esc", "Clear the selection").unhinted(),
    ],
);

gpui_kit::actions!(
    coffer_checks,
    [
        /// M: merge the selected pair.
        MergeSelected,
        /// N: the selected pair aren't duplicates.
        DismissSelected,
        /// ←→: keep the other row of the selected pair.
        KeepOther,
    ]
);

/// The queues' canvas and inspector.
pub struct ChecksView {
    checks: Entity<Checks>,
    focus: FocusHandle,
    /// Owed to you's Name field, made on first render (M3.11).
    name: Option<Entity<InputState>>,
    /// The person the Name field was last filled for.
    name_for: Option<String>,
    /// Why the last rename was refused.
    name_error: Option<SharedString>,
    _subscriptions: Vec<Subscription>,
}

impl ChecksView {
    pub fn new(checks: Entity<Checks>, cx: &mut Context<Self>) -> Self {
        let subscriptions = vec![cx.observe(&checks, |_, _, cx| cx.notify())];
        Self {
            checks,
            focus: cx.focus_handle(),
            name: None,
            name_for: None,
            name_error: None,
            _subscriptions: subscriptions,
        }
    }

    /// The Name field, made the first time Owed to you shows a person, and
    /// filled when the person changes (not while it's typed in).
    fn sync_name(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let person = self
            .checks
            .read(cx)
            .selected_person()
            .map(super::owed::name_value);
        let Some(person) = person else {
            self.name_for = None;
            return;
        };
        let input = match &self.name {
            Some(input) => input.clone(),
            None => {
                let input = cx.new(|cx| InputState::new(window, cx));
                self._subscriptions.push(cx.subscribe_in(
                    &input,
                    window,
                    |this, input, event, _, cx| {
                        if matches!(event, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                            let typed = input.read(cx).value().trim().to_owned();
                            if this.name_for.as_deref() == Some(typed.as_str()) {
                                return;
                            }
                            let result = this
                                .checks
                                .update(cx, |checks, cx| checks.rename_person(&typed, cx));
                            this.name_error = result.err();
                            if this.name_error.is_none() {
                                this.name_for = Some(typed);
                            }
                            cx.notify();
                        }
                    },
                ));
                self.name = Some(input.clone());
                input
            }
        };
        if self.name_for.as_deref() != Some(person.as_str()) {
            let typing = input.read(cx).focus_handle(cx).is_focused(window);
            if !typing {
                input.update(cx, |input, cx| {
                    input.set_value(SharedString::from(person.clone()), window, cx)
                });
                self.name_for = Some(person);
                self.name_error = None;
            }
        }
    }

    pub fn checks(&self) -> &Entity<Checks> {
        &self.checks
    }

    /// The canvas's focus: where the queue's keys work.
    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus
    }

    fn update(&self, cx: &mut Context<Self>, f: impl FnOnce(&mut Checks, &mut Context<Checks>)) {
        self.checks.update(cx, f);
    }

    fn render_toolbar(&self, checks: &Checks, window: &Window) -> gpui_kit::Div {
        let queue = checks.queue().unwrap_or(Queue::Duplicates);
        if queue == Queue::Owed {
            return div()
                .flex()
                .flex_none()
                .items_center()
                .gap(spacing::S2)
                .h(size::CONTROL + spacing::S6)
                .py(spacing::S3)
                .px(spacing::S4)
                .border_b(size::HAIRLINE)
                .border_color(color::HAIRLINE)
                .child(
                    div()
                        .flex()
                        .text_color(color::TEXT)
                        .child(Tracked::new(typography::TITLE, queue.title())),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .whitespace_nowrap()
                        .text_ellipsis()
                        .text_role(typography::CAPTION)
                        .child(super::text::rich(
                            window,
                            typography::CAPTION,
                            color::TEXT_MUTED,
                            super::owed::toolbar_caption(&checks.data().owed),
                        )),
                );
        }
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(spacing::S2)
            .h(size::CONTROL + spacing::S6)
            .py(spacing::S3)
            .px(spacing::S4)
            .border_b(size::HAIRLINE)
            .border_color(color::HAIRLINE)
            .child(
                div()
                    .flex()
                    .text_color(color::TEXT)
                    .child(Tracked::new(typography::TITLE, queue.title())),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_role(typography::CAPTION)
                    .text_color(color::TEXT_MUTED)
                    .child(toolbar_caption(checks)),
            )
            .when(hinted(checks), |this| this.child(hints(KEYS.hints())))
    }

    fn render_bar(&self, checks: &Checks) -> Option<AnyElement> {
        let Some(Outcome::Done { message, undo }) = checks.outcome() else {
            return None;
        };
        let (undo_on, dismiss_on) = (self.checks.clone(), self.checks.clone());
        let mut bar = UndoBar::new(message.clone()).on_dismiss(click(move |_, _, cx| {
            dismiss_on.update(cx, |checks, cx| checks.dismiss_outcome(cx))
        }));
        if undo.is_some() {
            bar = bar.on_undo(click(move |_, _, cx| {
                undo_on.update(cx, |checks, cx| checks.undo(cx))
            }));
        }
        Some(
            div()
                .absolute()
                .left_0()
                .right_0()
                .bottom(spacing::S6)
                .flex()
                .justify_center()
                .child(bar)
                .into_any_element(),
        )
    }
}

/// The toolbar's caption on Possible duplicates and Balance checks (Owed
/// to you's is [`super::owed::toolbar_caption`]).
pub(super) fn toolbar_caption(checks: &Checks) -> String {
    match checks.queue() {
        Some(Queue::Balances) => {
            let failing = checks.failing();
            if failing > 0 {
                format!(
                    "Every balance you enter or import is checked against the transactions · \
                     {failing} failing"
                )
            } else {
                "Every balance you enter or import is checked against the transactions".to_owned()
            }
        }
        _ => format!(
            "{} · nothing changes until you merge",
            count(checks.pending_pairs(), "pair", "pairs")
        ),
    }
}

/// Whether the toolbar hints the keys: on Possible duplicates, while a
/// pair is pending.
pub(super) fn hinted(checks: &Checks) -> bool {
    checks.queue() == Some(Queue::Duplicates) && checks.pending_pairs() > 0
}

/// A refusal under the toolbar: caption type, `danger`.
fn refused(message: SharedString) -> impl IntoElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(spacing::S1_5)
        .pt(spacing::S2)
        .px(spacing::S4)
        .child(Glyph::Alert.render().text_color(color::DANGER))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_role(typography::CAPTION)
                .text_color(color::DANGER)
                .child(message),
        )
}

/// A centred empty queue (Paper `9WH-0`): a 36px `field` well with the
/// queue's 16px icon, the title in title type, and a caption.
pub fn empty_queue(
    glyph: Glyph,
    title: impl Into<SharedString>,
    body: SharedString,
) -> impl IntoElement {
    div()
        .flex()
        .flex_1()
        .flex_col()
        .items_center()
        .justify_center()
        .gap(spacing::S2)
        .child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .size(size::CONTROL_XL)
                .rounded(crate::theme::tokens::radius::SURFACE)
                .bg(color::FIELD)
                .child(glyph.render().text_color(color::TEXT_MUTED)),
        )
        .child(
            div()
                .flex()
                .pt(spacing::S1)
                .text_color(color::TEXT)
                .child(Tracked::new(typography::TITLE, title.into())),
        )
        .child(
            div()
                .w(size::PANEL + spacing::S6 * 2.)
                .text_center()
                .text_role(typography::CAPTION)
                .text_color(color::TEXT_MUTED)
                .child(body),
        )
}

impl Render for ChecksView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.checks.read(cx).queue() == Some(Queue::Owed) {
            self.sync_name(window, cx);
        }
        let name = self.name.clone();
        let name_error = self.name_error.clone();
        let root = div()
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .on_list_move(
                KEYS.list(),
                cx.listener(|this, to: &Move, _, cx| {
                    this.update(cx, |checks, cx| match checks.queue() {
                        Some(Queue::Balances) => checks.select_next_check(*to, cx),
                        Some(Queue::Owed) => checks.select_next_person(*to, cx),
                        _ => checks.select_next_pair(*to, cx),
                    })
                }),
            )
            .on_action(cx.listener(|this, _: &Cancel, _, cx| {
                this.update(cx, |checks, cx| checks.clear_selection(cx))
            }))
            .on_action(cx.listener(|this, _: &MergeSelected, _, cx| {
                this.update(cx, |checks, cx| {
                    if checks.queue() == Some(Queue::Duplicates) {
                        checks.merge(cx)
                    }
                })
            }))
            .on_action(cx.listener(|this, _: &DismissSelected, _, cx| {
                this.update(cx, |checks, cx| {
                    if checks.queue() == Some(Queue::Duplicates) {
                        checks.dismiss(cx)
                    }
                })
            }))
            .on_action(cx.listener(|this, _: &KeepOther, _, cx| {
                this.update(cx, |checks, cx| {
                    if checks.queue() == Some(Queue::Duplicates) {
                        checks.keep_other(cx)
                    }
                })
            }))
            .flex()
            .flex_1()
            .min_w_0()
            .min_h_0();
        let cx: &App = cx;
        let checks = self.checks.read(cx);
        let (body, inspector): (AnyElement, AnyElement) = match checks.queue() {
            Some(Queue::Owed) => (
                super::owed::render_body(&self.checks, checks, &self.focus, window),
                super::owed::render_inspector(
                    &self.checks,
                    checks,
                    name.as_ref(),
                    name_error,
                    window,
                ),
            ),
            Some(Queue::Balances) => (
                super::balances::render_body(&self.checks, checks, &self.focus, window)
                    .into_any_element(),
                super::balances::render_inspector(&self.checks, checks, window).into_any_element(),
            ),
            _ => (
                super::duplicates::render_body(&self.checks, checks, &self.focus, window)
                    .into_any_element(),
                super::duplicates::render_inspector(checks, window).into_any_element(),
            ),
        };
        let refusal = match checks.outcome() {
            Some(Outcome::Refused(message)) => Some(message.clone()),
            _ => None,
        };
        let canvas = div()
            .relative()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            .bg(color::CANVAS)
            .child(self.render_toolbar(checks, window))
            .children(refusal.map(refused))
            .child(body)
            .children(self.render_bar(checks));
        root.child(canvas).child(inspector)
    }
}

/// A click handler: the type a button's `on_click` takes.
pub(super) fn click(
    handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    handler
}
