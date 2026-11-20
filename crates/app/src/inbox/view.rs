//! The Inbox's canvas and inspector (`docs/DESIGN.md` → "Inbox", "Inbox
//! impact preview"; Paper `564-0`, `ABK-0`, `DL8-0`, `EWZ-0`).
//!
//! - **Toolbar** (52px): the status in title type, a caption, and the
//!   keys' hints, from [`KEYS`]: A Approve, R Reject, ↑↓ Select.
//! - **Groups** are table sections, one per kind and source, widest reach
//!   first: a 28px header with the kind's lanes, 28px rows, and a footer
//!   with "Approve all (n)" / "Reject all" and the source. Approve and
//!   reject show on a row while it's hovered or selected.
//! - **The selected suggestion opens in place** (D6.4's pattern): a rule,
//!   a merchant name or a categorisation of several transactions lists the
//!   rows approving it would change, with the ones it leaves alone saying
//!   why. A wide rule starts with a warning and is summed by merchant.
//! - After an approval that changes rows, a bar says what changed (the
//!   D6.3 undo bar, without Undo until approvals can be undone: user,
//!   2026-11-14).
//!
//! Approved and Rejected list the same groups read-only, with the decision
//! date in the action lane (composed from the Pending specs; user,
//! 2026-11-14).

use chrono::{DateTime, NaiveDate, Utc};
use gpui_kit::base::actions::Cancel;
use gpui_kit::component::Disableable as _;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Entity, FocusHandle, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, Rgba, SharedString, StatefulInteractiveElement as _,
    Styled as _, Subscription, TestSupportExt as _, Window, div,
};
use orre_core::category::assign::NoOwnCategory;
use orre_core::inbox::preview::{
    After, ByMerchant, CategoryNow, Deciding, Line, MerchantLine, NamesNow, Now, Preview, Subject,
};
use orre_core::inbox::{
    Detail, ORRE_SOURCE, Payload, SuggestionId, SuggestionKind, SuggestionStatus,
};
use orre_core::link::LinkKind;
use orre_core::reporting::palette::Palette;

use crate::theme::tokens::{color, radius, shadow, size, spacing, typography, weight};
use crate::ui::button::{Button, ButtonVariant, IconButton, IconButtonSize};
use crate::ui::empty_state::EmptyState;
use crate::ui::icon::Glyph;
use crate::ui::keys::{Key, ViewKeys, hints};
use crate::ui::list::{Keys, ListKeysExt as _, Move};
use crate::ui::marker::Marker;
use crate::ui::text::TextRoleExt as _;
use crate::ui::tracked::Tracked;

use super::store::Entry;
use super::text::{
    self, NAME_SHOWN, Part, after_words, alias_condition, count, kind_name, marker, plain, rich,
    rule_condition, source_name,
};
use super::{GroupKey, Inbox, Outcome, Scope};

/// The key context of the Inbox's canvas, where A, R and ↑↓ work.
pub const CONTEXT: &str = "OrreInbox";

/// A: approve the selected suggestion.
pub(super) const APPROVE: Key = Key::new::<ApproveSelected>(&["a"], "A", "Approve");
/// R: reject it.
pub(super) const REJECT: Key = Key::new::<RejectSelected>(&["r"], "R", "Reject");
/// Escape: clear the selection. Not hinted.
pub(super) const CLEAR: Key =
    Key::new::<Cancel>(&["escape"], "Esc", "Clear the selection").unhinted();

/// The Inbox's keys, declared once (ROADMAP U.7): [`super::init`] binds
/// them, the toolbar hints them and the test-only `describe()` lists them.
pub const KEYS: ViewKeys = ViewKeys::new(Keys::new(CONTEXT), &[APPROVE, REJECT, CLEAR]);

gpui_kit::actions!(
    orre_inbox,
    [
        /// A: approve the selected suggestion.
        ApproveSelected,
        /// R: reject the selected suggestion.
        RejectSelected,
    ]
);

/// The date lane: 48px (`spacing-6` × 2).
const DATE_LANE: gpui_kit::Pixels = gpui_kit::px(48.);

/// The action lane: two 24px icon buttons 4px apart.
fn action_lane() -> gpui_kit::Pixels {
    size::CONTROL_SM * 2. + spacing::S1
}

/// The indent of an open suggestion's lines under its row.
fn open_indent() -> gpui_kit::Pixels {
    spacing::S2 + spacing::S4
}

/// The toolbar's title (the status) and caption.
pub(super) fn toolbar_text(inbox: &Inbox) -> (&'static str, String) {
    let shown = count(inbox.order().len(), "suggestion", "suggestions");
    match inbox.status() {
        SuggestionStatus::Pending => (
            "Pending",
            format!("{shown} · nothing changes until you approve"),
        ),
        SuggestionStatus::Approved => ("Approved", format!("{shown} you approved")),
        SuggestionStatus::Rejected => ("Rejected", format!("{shown} you rejected")),
    }
}

/// The Inbox's canvas and inspector.
pub struct InboxView {
    inbox: Entity<Inbox>,
    focus: FocusHandle,
    hovered: Option<SuggestionId>,
    _subscriptions: Vec<Subscription>,
}

impl InboxView {
    pub fn new(inbox: Entity<Inbox>, cx: &mut Context<Self>) -> Self {
        let subscriptions = vec![cx.observe(&inbox, |_, _, cx| cx.notify())];
        // An Inbox made with `Inbox::unloaded` is read in the background.
        inbox.update(cx, |inbox, cx| inbox.start_loading(cx));
        Self {
            inbox,
            focus: cx.focus_handle(),
            hovered: None,
            _subscriptions: subscriptions,
        }
    }

    pub fn inbox(&self) -> &Entity<Inbox> {
        &self.inbox
    }

    /// The canvas's focus: where A, R and ↑↓ work.
    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus
    }

    fn update_inbox(
        &self,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut Inbox, &mut Context<Inbox>),
    ) {
        self.inbox.update(cx, f);
    }

    fn approve_selected(&mut self, cx: &mut Context<Self>) {
        let inbox = self.inbox.read(cx);
        if inbox.status() != SuggestionStatus::Pending {
            return;
        }
        if let Some(id) = inbox.selected() {
            self.update_inbox(cx, |inbox, cx| inbox.approve(id, cx));
        }
    }

    fn reject_selected(&mut self, cx: &mut Context<Self>) {
        let inbox = self.inbox.read(cx);
        if inbox.status() != SuggestionStatus::Pending {
            return;
        }
        if let Some(id) = inbox.selected() {
            self.update_inbox(cx, |inbox, cx| inbox.reject(id, cx));
        }
    }

    fn render_toolbar(&self, inbox: &Inbox) -> impl IntoElement {
        let (title, caption) = toolbar_text(inbox);
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
                    .child(Tracked::new(typography::TITLE, title)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_role(typography::CAPTION)
                    .text_color(color::TEXT_MUTED)
                    .child(caption),
            )
            .when(inbox.status() == SuggestionStatus::Pending, |this| {
                this.child(hints(KEYS.hints()))
            })
    }

    fn render_canvas(
        &self,
        me: &Entity<InboxView>,
        window: &mut Window,
        cx: &App,
    ) -> impl IntoElement {
        let inbox = self.inbox.read(cx);
        let groups = inbox.groups();
        let body: AnyElement = if groups.is_empty() {
            // Before the first read is back the table says nothing, as the
            // Transactions table does: "Nothing to review" would be a guess.
            div()
                .flex()
                .flex_1()
                .items_center()
                .justify_center()
                .when(inbox.loaded(), |this| this.child(empty_state(inbox)))
                .into_any_element()
        } else {
            let mut sections = Vec::new();
            for (index, (key, entries)) in groups.iter().enumerate() {
                sections.push(self.render_group(inbox, key, entries, index == 0, me, window));
            }
            div()
                .id("inbox-body")
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_none()
                        .px(spacing::S2)
                        .pb(spacing::S4)
                        .children(sections),
                )
                .into_any_element()
        };
        let done = match inbox.outcome() {
            Some(Outcome::Done(message)) => Some(message.clone()),
            _ => None,
        };
        div()
            .relative()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            .bg(color::CANVAS)
            .child(self.render_toolbar(inbox))
            .child(body)
            .when_some(done, |this, message| this.child(self.render_done(message)))
    }

    /// The confirmation after an approval: the D6.3 undo bar's look,
    /// without Undo.
    fn render_done(&self, message: SharedString) -> impl IntoElement {
        let inbox = self.inbox.clone();
        div()
            .absolute()
            .left_0()
            .right_0()
            .bottom(spacing::S6)
            .flex()
            .justify_center()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(spacing::S3)
                    .py(spacing::S2)
                    .pl(spacing::S3)
                    .pr(spacing::S2)
                    .bg(color::FLOAT)
                    .border(size::HAIRLINE)
                    .border_color(color::BORDER)
                    .rounded(radius::LG)
                    .shadow(vec![shadow::TOOLBAR.box_shadow()])
                    .child(
                        Glyph::Check
                            .render_at(size::ICON_MD)
                            .text_color(color::TEXT),
                    )
                    .child(
                        div()
                            .text_role(typography::BODY)
                            .text_color(color::TEXT)
                            .child(message),
                    )
                    .child(
                        IconButton::new("inbox-dismiss-done", Glyph::Reject, "Dismiss")
                            .with_size(IconButtonSize::Small)
                            .with_foreground(color::TEXT_MUTED)
                            .on_click(click(move |_, _, cx| {
                                inbox.update(cx, |inbox, cx| inbox.dismiss_outcome(cx))
                            })),
                    ),
            )
    }

    fn render_group(
        &self,
        inbox: &Inbox,
        key: &GroupKey,
        entries: &[&Entry],
        first: bool,
        me: &Entity<InboxView>,
        window: &mut Window,
    ) -> AnyElement {
        let data = inbox.data();
        let today = today(data.read_at);
        let pending = inbox.status() == SuggestionStatus::Pending;
        let lanes = Lanes::of(key.kind);
        let mut rows: Vec<AnyElement> = Vec::new();
        for entry in entries {
            let selected = inbox.selected() == Some(entry.id());
            rows.push(self.render_row(
                entry,
                key,
                selected,
                pending,
                &data.palette,
                today,
                me,
                window,
            ));
            if selected
                && pending
                && let Some(open) = render_open(entry, &data.palette, today, window)
            {
                rows.push(open);
            }
        }
        div()
            .flex()
            .flex_col()
            .flex_none()
            .pt(spacing::S2)
            .pb(spacing::S3)
            .when(!first, |this| {
                this.border_t(size::HAIRLINE).border_color(color::HAIRLINE)
            })
            .child(group_header(kind_name(key.kind), entries.len(), &lanes))
            .children(rows)
            .when(pending, |this| {
                this.child(self.render_footer(inbox, key, entries))
            })
            .into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    fn render_row(
        &self,
        entry: &Entry,
        key: &GroupKey,
        selected: bool,
        pending: bool,
        palette: &Palette,
        today: NaiveDate,
        me: &Entity<InboxView>,
        window: &mut Window,
    ) -> AnyElement {
        let id = entry.id();
        let agent = key.source != ORRE_SOURCE;
        let cells = RowCells::of(entry, palette, today, agent, window);
        let hovered = self.hovered == Some(id);
        let actions: AnyElement = if pending && (hovered || selected) {
            let (approve, reject) = (self.inbox.clone(), self.inbox.clone());
            div()
                .flex()
                .items_center()
                .justify_end()
                .gap(spacing::S1)
                .child(
                    IconButton::new(
                        SharedString::from(format!("inbox-approve-{id}")),
                        Glyph::Check,
                        "Approve",
                    )
                    .with_size(IconButtonSize::Small)
                    .with_foreground(if agent {
                        color::MOLDAVITE_TEXT
                    } else {
                        color::TEXT
                    })
                    .on_click(click(move |_, _, cx| {
                        // The row under it would toggle the selection.
                        cx.stop_propagation();
                        approve.update(cx, |inbox, cx| inbox.approve(id, cx))
                    })),
                )
                .child(
                    IconButton::new(
                        SharedString::from(format!("inbox-reject-{id}")),
                        Glyph::Close,
                        "Reject",
                    )
                    .with_size(IconButtonSize::Small)
                    .with_foreground(color::TEXT_SECONDARY)
                    .on_click(click(move |_, _, cx| {
                        cx.stop_propagation();
                        reject.update(cx, |inbox, cx| inbox.reject(id, cx))
                    })),
                )
                .into_any_element()
        } else if !pending {
            let decided = text::decided_on(entry, today);
            div()
                .text_right()
                .text_role(typography::MONO_VALUE)
                .text_color(color::TEXT_MUTED)
                .child(decided)
                .into_any_element()
        } else {
            div().into_any_element()
        };
        let select = self.inbox.clone();
        let focus = self.focus.clone();
        div()
            .id(SharedString::from(format!("inbox-row-{id}")))
            // Lets UI tests find the row; a no-op outside them.
            .test_support()
            .flex()
            .flex_none()
            .items_center()
            .gap(spacing::S3)
            .h(size::CONTROL)
            .px(spacing::S2)
            .rounded(radius::MD)
            .map(|this| {
                if selected {
                    this.bg(color::MOLTEN_SOFT)
                } else if hovered {
                    this.bg(color::HOVER)
                } else {
                    this
                }
            })
            .on_hover({
                let me = me.clone();
                move |is_hovered: &bool, _, cx: &mut App| {
                    me.update(cx, |this, cx| {
                        let next = if *is_hovered {
                            Some(id)
                        } else if this.hovered == Some(id) {
                            None
                        } else {
                            this.hovered
                        };
                        if this.hovered != next {
                            this.hovered = next;
                            cx.notify();
                        }
                    })
                }
            })
            .on_click(click(move |_, window, cx| {
                focus.focus(window, cx);
                select.update(cx, |inbox, cx| inbox.toggle(id, cx));
            }))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_w_0()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_role(typography::BODY)
                    .text_color(color::TEXT)
                    .child(cells.item),
            )
            .child(
                lane(size::COLUMN_ACCOUNT)
                    .gap(spacing::S2)
                    .children(cells.now),
            )
            .child(
                lane(size::COLUMN_CATEGORY)
                    .gap(spacing::S1_5)
                    .children(cells.suggested),
            )
            .child(
                lane(DATE_LANE)
                    .justify_end()
                    .text_role(typography::MONO_VALUE)
                    .text_color(color::TEXT_MUTED)
                    .children(cells.date),
            )
            .child(
                lane(size::COLUMN_AMOUNT)
                    .justify_end()
                    .children(cells.amount),
            )
            .child(lane(action_lane()).justify_end().child(actions))
            .into_any_element()
    }

    fn render_footer(&self, inbox: &Inbox, key: &GroupKey, entries: &[&Entry]) -> impl IntoElement {
        let agent = key.source != ORRE_SOURCE;
        let FooterText {
            approve_label,
            can_approve,
            reject_label,
            note,
            source,
            refused,
        } = FooterText::of(inbox, key, entries);
        let (approve_group, reject_group) = (key.clone(), key.clone());
        let (approve, reject) = (self.inbox.clone(), self.inbox.clone());
        div()
            .flex()
            .flex_col()
            .flex_none()
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(spacing::S1_5)
                    .pt(spacing::S2)
                    .px(spacing::S2)
                    .child(
                        Button::new(
                            SharedString::from(format!(
                                "inbox-approve-{}-{}",
                                key.kind, key.source
                            )),
                            approve_label,
                        )
                        .with_variant(if agent {
                            ButtonVariant::AgentPrimary
                        } else {
                            ButtonVariant::Primary
                        })
                        .disabled(!can_approve)
                        .on_click(click(move |_, _, cx| {
                            let group = approve_group.clone();
                            approve.update(cx, |inbox, cx| inbox.approve_group(group, cx))
                        })),
                    )
                    .child(
                        Button::new(
                            SharedString::from(format!("inbox-reject-{}-{}", key.kind, key.source)),
                            reject_label,
                        )
                        .with_variant(if agent {
                            ButtonVariant::AgentOutline
                        } else {
                            ButtonVariant::Secondary
                        })
                        .on_click(click(move |_, _, cx| {
                            let group = reject_group.clone();
                            reject.update(cx, |inbox, cx| inbox.reject_group(group, cx))
                        })),
                    )
                    .children(note.map(|note| {
                        div()
                            .pl(spacing::S1_5)
                            .whitespace_nowrap()
                            .text_role(typography::CAPTION)
                            .text_color(color::TEXT_MUTED)
                            .child(note)
                    }))
                    .child(div().flex_1())
                    .child(footer_source(source)),
            )
            .children(refused.map(|message| {
                div()
                    .pt(spacing::S1_5)
                    .px(spacing::S2)
                    .text_role(typography::CAPTION)
                    .text_color(color::DANGER)
                    .child(message)
            }))
    }
}

/// Today, from when the data was read.
pub(super) fn today(read_at: Option<DateTime<Utc>>) -> NaiveDate {
    read_at.map_or_else(crate::accounts::fixture::today, |at| at.date_naive())
}

/// A fixed-width lane of a row: content in a line, vertically centred.
fn lane(width: gpui_kit::Pixels) -> gpui_kit::Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .w(width)
        .overflow_hidden()
        .whitespace_nowrap()
}

/// A caps lane label (`text-muted` 11/14 600 for a group, `text-faint`
/// 10/12 600 for an open suggestion).
fn lane_label(text: &str, role: typography::TextRole, tone: Rgba) -> gpui_kit::Div {
    div()
        .flex()
        .text_color(tone)
        .child(Tracked::new(role, SharedString::from(text.to_owned())))
}

/// The lane labels of a kind's group (`DESIGN.md` → "Inbox impact
/// preview" → "Lanes per kind").
struct Lanes {
    now: &'static str,
    suggested: &'static str,
    date: &'static str,
    amount: &'static str,
}

impl Lanes {
    fn of(kind: SuggestionKind) -> Self {
        match kind {
            SuggestionKind::Rule => Self {
                now: "Decides now",
                suggested: "Suggested",
                date: "",
                amount: "Changes",
            },
            SuggestionKind::MerchantAlias => Self {
                now: "Shows as now",
                suggested: "Suggested",
                date: "",
                amount: "Renames",
            },
            _ => Self {
                now: "Now",
                suggested: "Suggested",
                date: "Date",
                amount: "Amount",
            },
        }
    }
}

fn group_header(name: &str, shown: usize, lanes: &Lanes) -> impl IntoElement {
    let label = |text: &str| lane_label(text, typography::SECTION_LABEL, color::TEXT_MUTED);
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(spacing::S3)
        .h(size::CONTROL)
        .px(spacing::S2)
        .child(
            div()
                .flex()
                .flex_1()
                .min_w_0()
                .items_center()
                .gap(spacing::S2)
                .child(
                    div()
                        .text_role(typography::CAPTION)
                        .font_weight(weight::STRONG)
                        .text_color(color::TEXT_SECONDARY)
                        .child(SharedString::from(name.to_owned())),
                )
                .child(
                    div()
                        .text_role(typography::MONO_VALUE)
                        .text_color(color::TEXT_MUTED)
                        .child(shown.to_string()),
                ),
        )
        .child(lane(size::COLUMN_ACCOUNT).child(label(lanes.now)))
        .child(lane(size::COLUMN_CATEGORY).child(label(lanes.suggested)))
        .child(lane(DATE_LANE).justify_end().child(label(lanes.date)))
        .child(
            lane(size::COLUMN_AMOUNT)
                .justify_end()
                .child(label(lanes.amount)),
        )
        .child(lane(action_lane()))
}

/// Words in a lane, in `text-muted`.
fn muted(words: impl Into<SharedString>) -> AnyElement {
    div()
        .text_role(typography::BODY)
        .text_color(color::TEXT_MUTED)
        .child(words.into())
        .into_any_element()
}

/// The words of a category now: its label, "Uncategorised" or "Split".
pub(super) fn now_words(now: &Now) -> String {
    match now {
        Now::Category { category, .. } => plain(&category.label, NAME_SHOWN),
        Now::Uncategorised => "Uncategorised".to_owned(),
        Now::Split => "Split".to_owned(),
    }
}

/// A category now: its marker and label (`text-secondary`, or `text` when
/// `lifted`), the dashed marker and "Uncategorised", or "Split".
fn category_cell(now: &Now, palette: &Palette, lifted: bool) -> Vec<AnyElement> {
    let words = now_words(now);
    match now {
        Now::Category { category, .. } => vec![
            marker(palette, category).render().into_any_element(),
            div()
                .text_role(typography::BODY)
                .text_color(if lifted {
                    color::TEXT
                } else {
                    color::TEXT_SECONDARY
                })
                .child(words)
                .into_any_element(),
        ],
        Now::Uncategorised => vec![Marker::None.render().into_any_element(), muted(words)],
        Now::Split => vec![muted(words)],
    }
}

/// A merchant's categories now, as [`Now`]: `None` for "Several".
fn category_now(now: &CategoryNow) -> Option<Now> {
    match now {
        CategoryNow::Uncategorised => Some(Now::Uncategorised),
        CategoryNow::Category(category) => Some(Now::Category {
            category: category.clone(),
            set_by: orre_core::inbox::preview::SetBy::User,
        }),
        CategoryNow::Several => None,
    }
}

/// The words of a merchant's categories now.
pub(super) fn category_now_words(now: &CategoryNow) -> String {
    category_now(now).map_or_else(|| SEVERAL.to_owned(), |now| now_words(&now))
}

fn category_now_cell(now: &CategoryNow, palette: &Palette) -> Vec<AnyElement> {
    match category_now(now) {
        Some(now) => category_cell(&now, palette, false),
        None => vec![muted(category_now_words(now))],
    }
}

/// What a suggestion proposes, in its lane: an agent's in the table's
/// AI-suggested style (10px sparkle, `moldavite-text`); Orre's own
/// neutral, in `text-secondary` with the sparkle's slot kept empty.
fn suggested(label: impl Into<SharedString>, agent: bool) -> Vec<AnyElement> {
    let icon = if agent {
        Glyph::Sparkle
            .render_at(size::ICON_XS)
            .text_color(color::MOLDAVITE)
            .into_any_element()
    } else {
        div().flex_none().size(size::ICON_XS).into_any_element()
    };
    vec![
        icon,
        div()
            .text_role(typography::BODY)
            .text_color(if agent {
                color::MOLDAVITE_TEXT
            } else {
                color::TEXT_SECONDARY
            })
            .child(label.into())
            .into_any_element(),
    ]
}

/// "Stays, you set it": the person icon and the words, `text-muted`.
fn stays(words: String, icon: bool) -> Vec<AnyElement> {
    let mut cell = Vec::new();
    if icon {
        cell.push(
            Glyph::YouSet
                .render()
                .text_color(color::TEXT_MUTED)
                .into_any_element(),
        );
    }
    cell.push(muted(words));
    cell
}

/// "Stays, a transfer": a 10px glyph and the words, `text-muted`.
fn stays_as(words: String, glyph: Option<Glyph>) -> Vec<AnyElement> {
    let mut cell = Vec::new();
    if let Some(glyph) = glyph {
        cell.push(
            glyph
                .render_at(size::ICON_XS)
                .text_color(color::TEXT_MUTED)
                .into_any_element(),
        );
    }
    cell.push(muted(words));
    cell
}

/// The cells of a suggestion's row.
struct RowCells {
    item: AnyElement,
    now: Vec<AnyElement>,
    suggested: Vec<AnyElement>,
    date: Option<AnyElement>,
    amount: Option<AnyElement>,
}

/// A count in the amount lane (CHANGES, RENAMES): mono 11/14
/// `text-secondary`; a wide rule's in `warning` at 600.
fn count_cell(value: usize, warning: bool) -> AnyElement {
    div()
        .text_role(typography::MONO_VALUE)
        .text_color(if warning {
            color::WARNING
        } else {
            color::TEXT_SECONDARY
        })
        .when(warning, |this| this.font_weight(weight::STRONG))
        .child(value.to_string())
        .into_any_element()
}

fn amount_cell(amount: String) -> AnyElement {
    div()
        .text_role(typography::MONO_AMOUNT)
        .text_color(color::TEXT)
        .child(amount)
        .into_any_element()
}

/// A row's first cell, as text.
pub(super) enum ItemText {
    /// Mixed runs in body type, `text`: a condition and its pattern.
    Rich(Vec<Part>),
    Plain(String),
    Muted(String),
}

/// What a row's NOW lane holds, as text.
pub(super) enum NowText<'a> {
    Empty,
    Muted(String),
    /// Mixed runs in body type, `text-muted`: "Rule UBER".
    Rich(Vec<Part>),
    /// A category now: [`category_cell`].
    Category(&'a Now),
}

/// What a row's amount lane holds.
pub(super) enum AmountText {
    /// CHANGES or RENAMES: [`count_cell`].
    Count {
        value: usize,
        warning: bool,
    },
    Money(String),
}

/// What a suggestion's row says, lane by lane, before it's drawn
/// ([`RowCells`]).
pub(super) struct RowText<'a> {
    pub item: ItemText,
    pub now: NowText<'a>,
    /// The SUGGESTED lane's label, drawn by [`suggested`].
    pub suggested: Option<String>,
    pub date: Option<String>,
    pub amount: Option<AmountText>,
}

impl<'a> RowText<'a> {
    pub(super) fn of(entry: &'a Entry, today: NaiveDate) -> Self {
        let preview = entry.preview.as_ref();
        let pending = entry.suggestion.status == SuggestionStatus::Pending;
        match (&entry.suggestion.payload, &entry.subject) {
            (Payload::Rule(rule), subject) => {
                let now = match preview.and_then(|p| p.deciding.as_ref()) {
                    Some(Deciding::NoRule) => NowText::Muted("No rule".to_owned()),
                    Some(Deciding::Rule(decider)) => NowText::Rich(vec![
                        Part::text("Rule "),
                        Part::mono(text::pattern(decider)),
                    ]),
                    Some(Deciding::Several) => NowText::Muted(SEVERAL.to_owned()),
                    None => NowText::Empty,
                };
                let label = text::rule_category(subject)
                    .map(|category| plain(&category.label, NAME_SHOWN))
                    .or_else(|| {
                        text::rule_extras(rule)
                            .first()
                            .map(|extra| capitalise(extra))
                    })
                    .unwrap_or_default();
                let amount = preview.and_then(|p| match &p.impact.detail {
                    Detail::Rule(impact) => Some(AmountText::Count {
                        value: impact.recategorises,
                        warning: impact.wide,
                    }),
                    _ => None,
                });
                Self {
                    item: ItemText::Rich(vec![
                        Part::text(format!("{} ", rule_condition(rule))),
                        Part::mono(text::pattern(rule)).color(color::TEXT_STRONG),
                    ]),
                    now,
                    suggested: Some(label),
                    date: None,
                    amount,
                }
            }
            (Payload::MerchantAlias(alias), _) => {
                let now = match preview.and_then(|p| p.names_now.as_ref()) {
                    Some(NamesNow::One(name)) => NowText::Muted(plain(name, NAME_SHOWN)),
                    Some(NamesNow::Several(names)) => NowText::Muted(format!("{names} names")),
                    None => NowText::Empty,
                };
                let amount = preview.and_then(|p| match &p.impact.detail {
                    Detail::MerchantAlias(impact) => Some(AmountText::Count {
                        value: impact.renames,
                        warning: false,
                    }),
                    _ => None,
                });
                Self {
                    item: ItemText::Rich(vec![
                        Part::text(format!("{} ", alias_condition(alias.kind()))),
                        Part::mono(plain(alias.pattern(), NAME_SHOWN)).color(color::TEXT_STRONG),
                    ]),
                    now,
                    suggested: Some(plain(alias.name().as_str(), NAME_SHOWN)),
                    date: None,
                    amount,
                }
            }
            (Payload::TransferPair(_), Subject::TransferPair { outflow, inflow }) => {
                let name = |side: &Option<orre_core::inbox::preview::TransactionRef>| {
                    side.as_ref()
                        .map_or("A transaction gone since".to_owned(), |row| {
                            plain(&row.account_name, NAME_SHOWN)
                        })
                };
                Self {
                    item: ItemText::Rich(vec![
                        Part::text(name(outflow)),
                        Part::text(" → ").color(color::TEXT_MUTED),
                        Part::text(name(inflow)),
                    ]),
                    now: if pending {
                        NowText::Muted("Income and spending".to_owned())
                    } else {
                        NowText::Empty
                    },
                    suggested: Some("Transfer".to_owned()),
                    date: outflow.as_ref().map(|row| text::date(row.date, today)),
                    amount: inflow
                        .as_ref()
                        .map(|row| AmountText::Money(text::unsigned(row.amount))),
                }
            }
            (Payload::RefundLink(link), Subject::RefundLink { refund, purchase }) => {
                let (back, of) = match link.kind {
                    LinkKind::Reimbursement => (" paid back", "Reimbursement of"),
                    _ => (" money back", "Refund of"),
                };
                let of = match purchase {
                    Some(purchase) => format!("{of} {}", text::date(purchase.date, today)),
                    None => format!("{of} a transaction gone since"),
                };
                Self {
                    item: ItemText::Rich(vec![
                        Part::text(
                            refund
                                .as_ref()
                                .map_or("A transaction gone since".to_owned(), |row| {
                                    plain(row.name(), NAME_SHOWN)
                                }),
                        ),
                        Part::text(back).color(color::TEXT_MUTED),
                    ]),
                    now: if pending {
                        NowText::Muted("Income".to_owned())
                    } else {
                        NowText::Empty
                    },
                    suggested: Some(of),
                    date: refund.as_ref().map(|row| text::date(row.date, today)),
                    amount: refund
                        .as_ref()
                        .map(|row| AmountText::Money(text::signed(row.amount))),
                }
            }
            (_, Subject::Categorization(items)) => {
                let first = items.first();
                let same_now = items.iter().all(|item| {
                    item.transaction.as_ref().map(|t| &t.now)
                        == first.and_then(|f| f.transaction.as_ref().map(|t| &t.now))
                });
                let same_category = items
                    .iter()
                    .all(|item| item.category == first.and_then(|f| f.category.clone()));
                let single = items.len() == 1;
                let row = first.and_then(|item| item.transaction.as_ref());
                let item = if single {
                    row.map_or("A transaction gone since".to_owned(), |row| {
                        plain(row.name(), NAME_SHOWN)
                    })
                } else {
                    count(items.len(), "transaction", "transactions")
                };
                let now = match row {
                    Some(row) if pending && (single || same_now) => NowText::Category(&row.now),
                    _ if pending => NowText::Muted(SEVERAL.to_owned()),
                    _ => NowText::Empty,
                };
                let label = if same_category {
                    first
                        .and_then(|item| item.category.as_ref())
                        .map_or("A category gone since".to_owned(), |c| {
                            plain(&c.label, NAME_SHOWN)
                        })
                } else {
                    SEVERAL.to_owned()
                };
                Self {
                    item: ItemText::Plain(item),
                    now,
                    suggested: Some(label),
                    date: row
                        .filter(|_| single)
                        .map(|row| text::date(row.date, today)),
                    amount: row
                        .filter(|_| single)
                        .map(|row| AmountText::Money(text::signed(row.amount))),
                }
            }
            _ => Self {
                item: ItemText::Muted("This suggestion no longer fits".to_owned()),
                now: NowText::Empty,
                suggested: None,
                date: None,
                amount: None,
            },
        }
    }
}

impl RowCells {
    fn of(
        entry: &Entry,
        palette: &Palette,
        today: NaiveDate,
        agent: bool,
        window: &Window,
    ) -> Self {
        let RowText {
            item,
            now,
            suggested: label,
            date,
            amount,
        } = RowText::of(entry, today);
        Self {
            item: match item {
                ItemText::Rich(parts) => {
                    rich(window, typography::BODY, color::TEXT, parts).into_any_element()
                }
                ItemText::Plain(words) => div().child(words).into_any_element(),
                ItemText::Muted(words) => muted(words),
            },
            now: match now {
                NowText::Empty => Vec::new(),
                NowText::Muted(words) => vec![muted(words)],
                NowText::Rich(parts) => vec![
                    rich(window, typography::BODY, color::TEXT_MUTED, parts).into_any_element(),
                ],
                NowText::Category(now) => category_cell(now, palette, false),
            },
            suggested: label.map_or_else(Vec::new, |label| suggested(label, agent)),
            date: date.map(|date| div().child(date).into_any_element()),
            amount: amount.map(|amount| match amount {
                AmountText::Count { value, warning } => count_cell(value, warning),
                AmountText::Money(amount) => amount_cell(amount),
            }),
        }
    }
}

fn capitalise(words: &str) -> String {
    let mut chars = words.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

/// An open suggestion's row: 24px, indented 16px under its row.
fn open_row() -> gpui_kit::Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(spacing::S3)
        .h(size::CONTROL_SM)
        .pl(open_indent())
        .pr(spacing::S2)
}

/// Bank text with the matched part in `text-strong`, the rest
/// `text-muted`, mono 11/14.
fn bank_text(window: &Window, (before, matched, after): BankText) -> AnyElement {
    div()
        .flex_1()
        .min_w_0()
        .overflow_hidden()
        .whitespace_nowrap()
        .text_role(typography::MONO_VALUE)
        .text_color(color::TEXT_MUTED)
        .child(rich(
            window,
            typography::MONO_VALUE,
            color::TEXT_MUTED,
            vec![
                Part::mono(before),
                Part::mono(matched).color(color::TEXT_STRONG),
                Part::mono(after),
            ],
        ))
        .into_any_element()
}

fn open_header(
    first: String,
    now: &str,
    after: &str,
    date: &str,
    amount: &str,
) -> impl IntoElement {
    let label = |text: &str| lane_label(text, typography::MENU_LABEL, color::TEXT_FAINT);
    open_row()
        .child(
            div()
                .flex()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .child(label(&first)),
        )
        .child(lane(size::COLUMN_ACCOUNT).child(label(now)))
        .child(lane(size::COLUMN_CATEGORY).child(label(after)))
        .child(lane(DATE_LANE).justify_end().child(label(date)))
        .child(lane(size::COLUMN_AMOUNT).justify_end().child(label(amount)))
        .child(lane(action_lane()))
}

/// What a row gets in the AFTER lane: the suggested value in the
/// AI-suggested style, or why it's left as it is (the person icon only
/// where the user set it).
fn after_cell(after: &After, names: bool, agent: bool) -> Vec<AnyElement> {
    let label = after_label(after, names);
    match after {
        After::Category(_) | After::Name(_) | After::Split(_) => suggested(label, agent),
        After::NoOwnCategory(why) => stays_as(label, no_own_glyph(*why)),
        other => stays(label, *other == After::Stays),
    }
}

/// The glyph that says why a row takes no category of its own.
fn no_own_glyph(why: NoOwnCategory) -> Option<Glyph> {
    match why {
        NoOwnCategory::Transfer => Some(Glyph::Transfer),
        NoOwnCategory::Refund => Some(Glyph::Link),
        NoOwnCategory::Split => None,
    }
}

/// The words in a line's AFTER lane: what it's set to, or why it's left.
pub(super) fn after_label(after: &After, names: bool) -> String {
    match after {
        After::Category(category) => plain(&category.label, NAME_SHOWN),
        After::Name(name) => plain(name, NAME_SHOWN),
        After::Split(lines) => lines
            .iter()
            .map(|(category, _)| plain(&category.label, NAME_SHOWN))
            .collect::<Vec<_>>()
            .join(" / "),
        other => after_words(other, names).unwrap_or_default().to_owned(),
    }
}

/// Bank text as shown: the parts before, in and after what the pattern
/// matched ([`text::plain_highlighted`]).
pub(super) type BankText = (String, String, String);

fn bank_parts(raw: &str, highlight: Option<&std::ops::Range<usize>>) -> BankText {
    text::plain_highlighted(raw, highlight, text::BANK_TEXT_SHOWN)
}

/// A listed row of an open rule, name or categorisation, as text.
pub(super) struct LineText {
    pub bank_text: BankText,
    /// The name it shows as now, for a merchant name; else the category
    /// lane ([`now_words`]) says it.
    pub name: Option<String>,
    pub date: String,
    pub amount: String,
}

impl LineText {
    pub(super) fn of(line: &Line, names: bool, today: NaiveDate) -> Self {
        Self {
            bank_text: bank_parts(&line.transaction.bank_text, line.highlight.as_ref()),
            name: names.then(|| plain(line.transaction.name(), NAME_SHOWN)),
            date: text::date(line.transaction.date, today),
            amount: text::signed(line.transaction.amount),
        }
    }
}

/// A listed row of an open rule, name or categorisation.
fn open_line(
    line: &Line,
    names: bool,
    agent: bool,
    palette: &Palette,
    today: NaiveDate,
    window: &Window,
) -> impl IntoElement {
    let kept = line.after == After::Stays;
    let LineText {
        bank_text: bank,
        name,
        date,
        amount,
    } = LineText::of(line, names, today);
    let now: Vec<AnyElement> = match name {
        Some(name) => vec![
            div()
                .text_role(typography::BODY)
                .text_color(if kept {
                    color::TEXT
                } else {
                    color::TEXT_SECONDARY
                })
                .child(name)
                .into_any_element(),
        ],
        None => category_cell(&line.transaction.now, palette, kept),
    };
    let after = after_cell(&line.after, names, agent);
    open_row()
        .child(bank_text(window, bank))
        .child(lane(size::COLUMN_ACCOUNT).gap(spacing::S2).children(now))
        .child(
            lane(size::COLUMN_CATEGORY)
                .gap(spacing::S1_5)
                .children(after),
        )
        .child(
            lane(DATE_LANE)
                .justify_end()
                .text_role(typography::MONO_VALUE)
                .text_color(color::TEXT_MUTED)
                .child(date),
        )
        .child(
            lane(size::COLUMN_AMOUNT)
                .justify_end()
                .text_role(typography::MONO_AMOUNT)
                .text_color(color::TEXT_SECONDARY)
                .child(amount),
        )
        .child(lane(action_lane()))
}

/// The merchant cell of a wide rule's summary line: a label ("and 3 more
/// merchants") or the merchant's bank text.
pub(super) enum MerchantCell {
    Label(String),
    BankText(BankText),
}

/// A line of a wide rule's by-merchant summary, as text. Its AFTER lane is
/// [`after_label`]'s, or [`SEVERAL`] when its rows would differ.
pub(super) struct MerchantText {
    pub merchant: MerchantCell,
    /// How many of its rows match, in the date lane.
    pub count: String,
    pub total: Option<String>,
}

impl MerchantText {
    pub(super) fn of(line: &MerchantLine, label: Option<String>) -> Self {
        Self {
            merchant: match label {
                Some(label) => MerchantCell::Label(label),
                None => {
                    MerchantCell::BankText(bank_parts(&line.bank_text, line.highlight.as_ref()))
                }
            },
            count: line.count.to_string(),
            total: line.total.map(text::signed),
        }
    }
}

/// A lane whose rows differ.
pub(super) const SEVERAL: &str = "Several";

/// A line of a wide rule's by-merchant summary.
fn merchant_line(
    line: &MerchantLine,
    label: Option<String>,
    agent: bool,
    palette: &Palette,
    window: &Window,
) -> impl IntoElement {
    let MerchantText {
        merchant,
        count,
        total,
    } = MerchantText::of(line, label);
    let merchant = match merchant {
        MerchantCell::Label(label) => div()
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            .whitespace_nowrap()
            .text_role(typography::BODY)
            .text_color(color::TEXT_MUTED)
            .child(label)
            .into_any_element(),
        MerchantCell::BankText(parts) => bank_text(window, parts),
    };
    let after = match &line.after {
        Some(after) => after_cell(after, false, agent),
        None => vec![muted(SEVERAL)],
    };
    open_row()
        .child(merchant)
        .child(
            lane(size::COLUMN_ACCOUNT)
                .gap(spacing::S2)
                .children(category_now_cell(&line.now, palette)),
        )
        .child(
            lane(size::COLUMN_CATEGORY)
                .gap(spacing::S1_5)
                .children(after),
        )
        .child(
            lane(DATE_LANE)
                .justify_end()
                .text_role(typography::MONO_VALUE)
                .text_color(color::TEXT_MUTED)
                .child(count),
        )
        .child(
            lane(size::COLUMN_AMOUNT)
                .justify_end()
                .text_role(typography::MONO_AMOUNT)
                .text_color(color::TEXT_SECONDARY)
                .children(total),
        )
        .child(lane(action_lane()))
}

/// The open suggestion's footer caption.
fn open_caption(words: impl Into<SharedString>) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap(spacing::S3)
        .pt(spacing::S2)
        .pl(open_indent())
        .pr(spacing::S2)
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_role(typography::CAPTION)
                .text_color(color::TEXT_MUTED)
                .child(words.into()),
        )
}

/// "Bank text · 6 matches" or "Bank text · 6 of 14 matches".
fn listed_label(first: &str, preview: &Preview) -> String {
    let matches = count(preview.matches, "match", "matches");
    if preview.lines.len() < preview.matches {
        format!("{first} · {} of {matches}", preview.lines.len())
    } else {
        format!("{first} · {matches}")
    }
}

/// D6.4's warning before a wide rule's summary: a title, and a body whose
/// numbers are mono.
pub(super) struct WarningText {
    pub title: String,
    pub body: Vec<Part>,
}

impl WarningText {
    fn of(preview: &Preview, by: &ByMerchant, category: &str) -> Self {
        let (changes, matches) = match &preview.impact.detail {
            Detail::Rule(impact) => (impact.recategorises, impact.matches),
            _ => (0, preview.matches),
        };
        let moves_income =
            matches!(&preview.impact.detail, Detail::Rule(impact) if impact.moves_income);
        let title = format!(
            "This rule reaches {}{}",
            count(by.merchants, "merchant", "merchants"),
            if moves_income {
                ", including income"
            } else {
                ""
            }
        );
        let body = vec![
            Part::text("It would change "),
            Part::mono(changes.to_string()),
            Part::text(" of "),
            Part::mono(matches.to_string()),
            Part::text(format!(
                " past transactions. A narrower pattern is safer: approve only if every \
                 merchant below belongs in {category}."
            )),
        ];
        Self { title, body }
    }
}

/// D6.4's warning block before a wide rule's summary.
fn wide_warning(warning: WarningText, window: &Window) -> impl IntoElement {
    let WarningText { title, body } = warning;
    div()
        .flex()
        .flex_col()
        .flex_none()
        .pl(open_indent())
        .pr(spacing::S2)
        .pb(spacing::S2)
        .child(
            div()
                .flex()
                .items_start()
                .gap(spacing::S2_5)
                .p(spacing::S3)
                .rounded(radius::SURFACE)
                .bg(color::WARNING_SOFT)
                .child(
                    div()
                        .flex()
                        .flex_none()
                        .items_center()
                        .h(crate::theme::tokens::leading::UI)
                        .child(Glyph::Warning.render().text_color(color::WARNING)),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w_0()
                        .gap(spacing::S0_5)
                        .child(
                            div()
                                .text_role(typography::BODY)
                                .font_weight(weight::STRONG)
                                .text_color(color::WARNING)
                                .child(title),
                        )
                        .child(
                            div()
                                .text_role(typography::CAPTION)
                                .text_color(color::TEXT_SECONDARY)
                                .child(rich(
                                    window,
                                    typography::CAPTION,
                                    color::TEXT_SECONDARY,
                                    body,
                                )),
                        ),
                ),
        )
}

/// The labels over an open suggestion's lines: the first lane's, then
/// NOW, AFTER, the date lane's and the amount lane's.
pub(super) struct OpenHeader {
    pub first: String,
    pub lanes: [&'static str; 4],
}

/// A listed line of an open suggestion.
pub(super) enum OpenLine<'a> {
    /// A transaction; `names` for a merchant name's.
    Row { line: &'a Line, names: bool },
    /// A wide rule's merchant, or the rest of them ("and 3 more
    /// merchants", "Categories you set").
    Merchant {
        line: &'a MerchantLine,
        label: Option<String>,
    },
}

/// What the selected suggestion lists, open in place under its row.
pub(super) struct OpenText<'a> {
    pub warning: Option<WarningText>,
    pub header: Option<OpenHeader>,
    pub lines: Vec<OpenLine<'a>>,
    pub caption: String,
}

impl<'a> OpenText<'a> {
    /// `None` when there's no more to list than the row says.
    pub(super) fn of(entry: &'a Entry) -> Option<Self> {
        let preview = entry.preview.as_ref()?;
        let rows = |names: bool| -> Vec<OpenLine<'a>> {
            preview
                .lines
                .iter()
                .map(|line| OpenLine::Row { line, names })
                .collect()
        };
        match &entry.suggestion.payload {
            Payload::Rule(_) => {
                let category = text::rule_category(&preview.subject)
                    .map(|category| plain(&category.label, NAME_SHOWN))
                    .unwrap_or_else(|| "its category".to_owned());
                if let Some(by) = preview
                    .by_merchant
                    .as_ref()
                    .filter(|_| preview.impact.wide())
                {
                    let mut lines: Vec<OpenLine<'a>> = by
                        .shown
                        .iter()
                        .map(|line| OpenLine::Merchant { line, label: None })
                        .collect();
                    if let Some((rest, merchants)) = &by.rest {
                        lines.push(OpenLine::Merchant {
                            line: rest,
                            label: Some(format!(
                                "and {merchants} more {}",
                                if *merchants == 1 {
                                    "merchant"
                                } else {
                                    "merchants"
                                }
                            )),
                        });
                    }
                    if let Some(kept) = &by.kept {
                        lines.push(OpenLine::Merchant {
                            line: kept,
                            label: Some("Categories you set".to_owned()),
                        });
                    }
                    return Some(Self {
                        warning: Some(WarningText::of(preview, by, &category)),
                        header: Some(OpenHeader {
                            first: format!("Merchant · {}", by.merchants),
                            lanes: ["Now", "After", "Count", "Amount"],
                        }),
                        lines,
                        caption: "Summed by merchant because the matches span more than 5. \
                                  Categories you set stay."
                            .to_owned(),
                    });
                }
                let caption = if preview.matches == 0 {
                    format!(
                        "No past transactions match. Imports from now on that match go to \
                         {category}."
                    )
                } else {
                    format!("Imports from now on that match go to {category} too.")
                };
                let listed = preview.matches > 0;
                Some(Self {
                    warning: None,
                    header: listed.then(|| OpenHeader {
                        first: listed_label("Bank text", preview),
                        lanes: ["Now", "After", "Date", "Amount"],
                    }),
                    lines: if listed { rows(false) } else { Vec::new() },
                    caption,
                })
            }
            Payload::MerchantAlias(_) => {
                let listed = preview.matches > 0;
                Some(Self {
                    warning: None,
                    header: listed.then(|| OpenHeader {
                        first: listed_label("Bank text", preview),
                        lanes: ["Shows as now", "After", "Date", "Amount"],
                    }),
                    lines: if listed { rows(true) } else { Vec::new() },
                    caption: "Only the name you see changes. Bank text, categories and rules \
                              stay as they are."
                        .to_owned(),
                })
            }
            Payload::Categorization(items) if items.len() > 1 => Some(Self {
                warning: None,
                header: Some(OpenHeader {
                    first: format!(
                        "Bank text · {}",
                        count(items.len(), "transaction", "transactions")
                    ),
                    lanes: ["Now", "After", "Date", "Amount"],
                }),
                lines: rows(false),
                caption: "Approving sets each one's category; nothing else changes.".to_owned(),
            }),
            _ => None,
        }
    }
}

/// The selected suggestion, open in place under its row, when there's
/// more to list than the row says.
fn render_open(
    entry: &Entry,
    palette: &Palette,
    today: NaiveDate,
    window: &Window,
) -> Option<AnyElement> {
    let OpenText {
        warning,
        header,
        lines,
        caption,
    } = OpenText::of(entry)?;
    let agent = entry.suggestion.source_client != ORRE_SOURCE;
    let lines: Vec<AnyElement> = lines
        .into_iter()
        .map(|line| match line {
            OpenLine::Row { line, names } => {
                open_line(line, names, agent, palette, today, window).into_any_element()
            }
            OpenLine::Merchant { line, label } => {
                merchant_line(line, label, agent, palette, window).into_any_element()
            }
        })
        .collect();
    Some(
        div()
            .flex()
            .flex_col()
            .flex_none()
            .pt(spacing::S1)
            .pb(spacing::S2)
            .children(warning.map(|warning| wide_warning(warning, window)))
            .children(header.map(|OpenHeader { first, lanes }| {
                let [now, after, date, amount] = lanes;
                open_header(first, now, after, date, amount)
            }))
            .children(lines)
            .child(open_caption(caption))
            .into_any_element(),
    )
}

/// What a pending group's footer says.
pub(super) struct FooterText {
    /// "Approve all (n)", or "Approve" for a group of one.
    pub approve_label: String,
    /// Whether "Approve all" has anything to approve.
    pub can_approve: bool,
    pub reject_label: &'static str,
    /// Which suggestions "Approve all" leaves out.
    pub note: Option<String>,
    pub source: SourceText,
    /// Why core refused the last decision on this group.
    pub refused: Option<SharedString>,
}

impl FooterText {
    pub(super) fn of(inbox: &Inbox, key: &GroupKey, entries: &[&Entry]) -> Self {
        let approvable = inbox.approvable(key);
        let single = entries.len() == 1;
        let (approve_label, reject_label) = if single {
            ("Approve".to_owned(), "Reject")
        } else {
            (format!("Approve all ({})", approvable.len()), "Reject all")
        };
        let wide: Vec<&Entry> = entries
            .iter()
            .copied()
            .filter(|entry| entry.needs_own_approval())
            .collect();
        let note = match wide.as_slice() {
            [] => None,
            [one] => Some(match &one.suggestion.payload {
                Payload::Rule(rule) if one.preview.as_ref().is_some_and(|p| p.impact.wide()) => {
                    format!(
                        "Leaves out {}: a wide rule needs its own ✓",
                        text::pattern(rule)
                    )
                }
                _ => "Leaves out one that can't be approved now".to_owned(),
            }),
            many => Some(format!(
                "Leaves out {}: each needs its own ✓",
                count(many.len(), "suggestion", "suggestions")
            )),
        };
        let newest = entries
            .iter()
            .map(|entry| entry.suggestion.created_at.as_str())
            .max()
            .unwrap_or_default();
        let now = inbox.data().read_at.unwrap_or_default();
        let refused = match inbox.outcome() {
            Some(Outcome::Refused {
                scope: Scope::Group(group),
                message,
            }) if group == key => Some(message.clone()),
            _ => None,
        };
        Self {
            approve_label,
            can_approve: !approvable.is_empty(),
            reject_label,
            note,
            source: SourceText::of(&key.source, newest, now),
            refused,
        }
    }
}

/// The source at the end of a group's footer.
pub(super) struct SourceText {
    /// Orre's own ("Orre", its mark), or an AI client's (its name, the
    /// sparkle).
    pub orre: bool,
    pub name: String,
    /// "from your edits", or how long ago the newest one came: "2h ago".
    pub tail: String,
}

impl SourceText {
    fn of(source: &str, newest: &str, now: DateTime<Utc>) -> Self {
        let (name, suffix) = source_name(source);
        match suffix {
            Some(suffix) => Self {
                orre: true,
                name,
                tail: suffix.to_owned(),
            },
            None => Self {
                orre: false,
                name,
                tail: text::ago(newest, now),
            },
        }
    }
}

/// The source at the end of a group's footer: "✦ claude-code · 2h ago",
/// or Orre's mark and "Orre · from your edits".
fn footer_source(source: SourceText) -> impl IntoElement {
    let SourceText { orre, name, tail } = source;
    let row = div()
        .flex()
        .flex_none()
        .items_center()
        .gap(spacing::S1_5)
        .whitespace_nowrap()
        .text_role(typography::CAPTION)
        .text_color(color::TEXT_MUTED);
    if orre {
        row.child(
            Glyph::Mark
                .render_at(size::ICON_SM)
                .text_color(color::TEXT_SECONDARY),
        )
        .child(div().text_color(color::TEXT_SECONDARY).child(name))
        .child(format!("· {tail}"))
    } else {
        row.child(
            Glyph::Sparkle
                .render_at(size::ICON_XS)
                .text_color(color::MOLDAVITE),
        )
        .child(div().text_color(color::MOLDAVITE_TEXT).child(name))
        .child(format!("· {tail}"))
    }
}

/// What an empty list says (composed from Paper `13L-0`; user,
/// 2026-11-14).
fn empty_state(inbox: &Inbox) -> EmptyState {
    let (title, body) = empty_text(inbox);
    EmptyState::new(title).body(body)
}

/// The empty list's title and body.
pub(super) fn empty_text(inbox: &Inbox) -> (&'static str, SharedString) {
    if let Some(error) = inbox.load_error() {
        return ("The Inbox couldn't be read", error.clone());
    }
    let filtered = inbox.kind_filter().is_some() || inbox.source_filter().is_some();
    let any = !inbox.data().entries.is_empty();
    if filtered && any {
        return (
            "Nothing matches these filters",
            "Click the selected Kind or Source again to show every suggestion.".into(),
        );
    }
    match inbox.status() {
        SuggestionStatus::Pending => (
            "Nothing to review",
            "Suggestions from AI clients and from Orre wait here. Nothing changes until you \
             approve one."
                .into(),
        ),
        SuggestionStatus::Approved => (
            "No approved suggestions",
            "Suggestions you approve are listed here.".into(),
        ),
        SuggestionStatus::Rejected => (
            "No rejected suggestions",
            "Suggestions you reject are listed here.".into(),
        ),
    }
}

impl Render for InboxView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let me = cx.entity();
        let root = div()
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .on_list_move(
                KEYS.list(),
                cx.listener(|this, to: &Move, _, cx| {
                    this.update_inbox(cx, |inbox, cx| inbox.select_next(*to, cx))
                }),
            )
            .on_action(cx.listener(|this, _: &Cancel, _, cx| {
                this.update_inbox(cx, |inbox, cx| inbox.select(None, cx))
            }))
            .on_action(cx.listener(|this, _: &ApproveSelected, _, cx| this.approve_selected(cx)))
            .on_action(cx.listener(|this, _: &RejectSelected, _, cx| this.reject_selected(cx)))
            .flex()
            .flex_1()
            .min_w_0()
            .min_h_0();
        let cx: &App = cx;
        root.child(self.render_canvas(&me, window, cx))
            .child(super::inspector::render(self, window, cx))
    }
}

/// A click handler: the type a button's `on_click` takes.
fn click(
    handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
    handler
}
