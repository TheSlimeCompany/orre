//! The Inbox's left panel (`docs/DESIGN.md` → "Inbox"; Paper `564-0`,
//! `ABK-0`): INBOX (Pending, Approved, Rejected, with counts), KIND (the
//! pending suggestions by kind, widest reach first) and SOURCE (each agent,
//! the connected dot on the ones that have filed, and "Coffer from your
//! edits"). KIND and SOURCE filter the list; a second click clears the
//! filter. CHECKS (D6.4, M3.12; Paper `8SE-0`) sits between INBOX and
//! KIND: Possible duplicates and Balance checks, Coffer's own checks, so
//! neutral, with a `danger` dot before the count while a check fails.
//! Selecting one shows its queue ([`crate::checks`]) in the canvas; any
//! other row goes back to the suggestions.
//!
//! A Tab stop (`DESIGN.md` → "Keyboard paths"): ↑↓ move a keyboard cursor
//! through every row, across the sections, and Enter or Space act on its
//! row as a click does ([`crate::ui::panel_cursor`]). The keys don't change
//! what's open or filtered; the cursor starts on the open row: the open
//! queue, else the open status.

use coffer_core::inbox::{COFFER_SOURCE, SuggestionKind, SuggestionStatus};
use gpui_kit::{
    App, Entity, InteractiveElement as _, IntoElement, ParentElement as _, RenderOnce,
    SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
};

use crate::theme::tokens::{color, size, spacing, typography};
use crate::ui::icon::Glyph;
use crate::ui::list_row::{ListRow, TrailingTone};
use crate::ui::panel_cursor::{PanelCursor, PanelCursorExt as _, PanelKeys};
use crate::ui::section_label::SectionLabelRow;
use crate::ui::text::TextRoleExt as _;

use crate::checks::{Checks, Queue};

use super::text::{kind_glyph, kind_name, source_name};
use super::{Inbox, KIND_ORDER};

/// The panel's key context, where its keyboard cursor moves.
pub const CONTEXT: &str = "CofferInboxPanel";

/// The panel's keys: ↑↓ move the cursor, Enter and Space act on its row.
pub const KEYS: PanelKeys = PanelKeys::new(CONTEXT);

/// The statuses under INBOX, in order.
const STATUSES: [SuggestionStatus; 3] = [
    SuggestionStatus::Pending,
    SuggestionStatus::Approved,
    SuggestionStatus::Rejected,
];

/// A row of the panel, by what a click (or Enter or Space on the cursor)
/// does with it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InboxPanelRow {
    /// Shows the suggestions with a status.
    Status(SuggestionStatus),
    /// Opens a review queue.
    Queue(Queue),
    /// Toggles KIND.
    Kind(SuggestionKind),
    /// Toggles SOURCE: the client's name as filed.
    Source(String),
}

/// What a row shows, as the panel draws it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct RowText {
    pub label: String,
    /// After the label, quieter: "from your edits".
    pub suffix: Option<&'static str>,
    pub count: usize,
    /// The `danger` dot before the count: a balance check fails.
    pub flagged: bool,
    /// The moldavite dot of an agent with work waiting.
    pub connected: bool,
    /// The open status or queue, or a filter that's on.
    pub selected: bool,
}

impl InboxPanelRow {
    /// The row's element id.
    fn id(&self) -> SharedString {
        match self {
            Self::Status(status) => format!("inbox-status-{status}").into(),
            Self::Queue(Queue::Duplicates) => "inbox-checks-duplicates".into(),
            Self::Queue(Queue::Balances) => "inbox-checks-balances".into(),
            Self::Queue(Queue::Owed) => "inbox-checks-owed".into(),
            Self::Kind(kind) => format!("inbox-kind-{kind}").into(),
            Self::Source(source) => format!("inbox-source-{source}").into(),
        }
    }

    /// The icon before the label; a source has none.
    fn glyph(&self) -> Option<Glyph> {
        Some(match self {
            Self::Status(SuggestionStatus::Pending) => Glyph::Inbox,
            Self::Status(SuggestionStatus::Approved) => Glyph::Check,
            Self::Status(SuggestionStatus::Rejected) => Glyph::Reject,
            Self::Queue(Queue::Duplicates) => Glyph::Duplicate,
            Self::Queue(Queue::Balances) => Glyph::Balance,
            // M3.11 (Paper `DKR-0`): Coffer's own list, outside the badge.
            Self::Queue(Queue::Owed) => Glyph::Owed,
            Self::Kind(kind) => kind_glyph(*kind),
            Self::Source(_) => return None,
        })
    }

    /// What the row shows with `inbox` and `checks` as they are.
    pub(super) fn text(&self, inbox: &Inbox, checks: &Checks) -> RowText {
        let counts = &inbox.data().counts;
        let queue = checks.queue();
        let plain = |label: String, count: usize, selected: bool| RowText {
            label,
            suffix: None,
            count,
            flagged: false,
            connected: false,
            selected,
        };
        match self {
            Self::Status(status) => plain(
                status_label(*status).to_owned(),
                counts.with_status(*status),
                queue.is_none() && inbox.status() == *status,
            ),
            Self::Queue(row) => RowText {
                flagged: checks.queue_flagged(*row),
                ..plain(
                    row.title().to_owned(),
                    checks.queue_count(*row),
                    queue == Some(*row),
                )
            },
            Self::Kind(kind) => plain(
                kind_name(*kind).to_owned(),
                counts.pending_by_kind.get(kind).copied().unwrap_or(0),
                inbox.kind_filter() == Some(*kind),
            ),
            Self::Source(source) => {
                let pending = counts.by_source.get(source).copied().unwrap_or(0);
                let (label, suffix) = source_name(source);
                RowText {
                    suffix,
                    // Coffer's own suggestions are neutral.
                    connected: pending > 0 && source != COFFER_SOURCE,
                    ..plain(
                        label,
                        pending,
                        inbox.source_filter() == Some(source.as_str()),
                    )
                }
            }
        }
    }

    /// Does what clicking the row does. A suggestion row closes an open
    /// queue.
    pub fn act(&self, inbox: &Entity<Inbox>, checks: &Entity<Checks>, cx: &mut App) {
        if let Self::Queue(queue) = self {
            let queue = *queue;
            checks.update(cx, |checks, cx| checks.show(queue, cx));
            return;
        }
        checks.update(cx, |checks, cx| checks.hide(cx));
        inbox.update(cx, |inbox, cx| match self {
            Self::Status(status) => inbox.show_status(*status, cx),
            Self::Kind(kind) => inbox.toggle_kind(*kind, cx),
            Self::Source(source) => inbox.toggle_source(source, cx),
            Self::Queue(_) => unreachable!("handled above"),
        });
    }
}

/// The panel's sections, each its label and rows, in panel order: INBOX,
/// CHECKS, KIND, SOURCE.
pub(super) fn sections(inbox: &Inbox) -> [(&'static str, Vec<InboxPanelRow>); 4] {
    [
        ("Inbox", STATUSES.map(InboxPanelRow::Status).to_vec()),
        ("Checks", Queue::ALL.map(InboxPanelRow::Queue).to_vec()),
        ("Kind", KIND_ORDER.map(InboxPanelRow::Kind).to_vec()),
        (
            "Source",
            inbox
                .data()
                .sources
                .iter()
                .cloned()
                .map(InboxPanelRow::Source)
                .collect(),
        ),
    ]
}

/// Every row of the panel, in panel order.
pub fn rows(inbox: &Inbox) -> Vec<InboxPanelRow> {
    sections(inbox)
        .into_iter()
        .flat_map(|(_, rows)| rows)
        .collect()
}

/// The open row, where the cursor starts: the open queue if any, else the
/// open status.
pub fn home(inbox: &Inbox, checks: &Checks) -> InboxPanelRow {
    match checks.queue() {
        Some(queue) => InboxPanelRow::Queue(queue),
        None => InboxPanelRow::Status(inbox.status()),
    }
}

/// A section: 8px (the first) or 4px above, 6px inline, 10px below.
fn section(first: bool, label: &str) -> gpui_kit::Div {
    div()
        .flex()
        .flex_col()
        .flex_none()
        .pt(if first { spacing::S2 } else { spacing::S1 })
        .px(spacing::S1_5)
        .pb(spacing::S2_5)
        .child(SectionLabelRow::new(SharedString::from(label.to_owned())))
}

pub(super) fn status_label(status: SuggestionStatus) -> &'static str {
    match status {
        SuggestionStatus::Pending => "Pending",
        SuggestionStatus::Approved => "Approved",
        SuggestionStatus::Rejected => "Rejected",
    }
}

/// The panel. It draws itself when laid out, where it has the window its
/// keyboard cursor needs.
pub fn render(inbox: &Entity<Inbox>, checks: &Entity<Checks>, _: &App) -> impl IntoElement {
    InboxPanel {
        inbox: inbox.clone(),
        checks: checks.clone(),
    }
}

#[derive(IntoElement)]
struct InboxPanel {
    inbox: Entity<Inbox>,
    checks: Entity<Checks>,
}

impl RenderOnce for InboxPanel {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let cursor = PanelCursor::<InboxPanelRow>::keyed("inbox-panel-cursor", window, cx);
        let (inbox, checks) = (&self.inbox, &self.checks);
        let state = inbox.read(cx);
        let checks_state = checks.read(cx);
        let sections = sections(state);
        let rows: Vec<InboxPanelRow> = sections
            .iter()
            .flat_map(|(_, rows)| rows.iter().cloned())
            .collect();
        let home = Some(home(state, checks_state));
        let frame = cursor.read(cx).frame(&rows, home.as_ref(), window);
        #[cfg(test)]
        state.record_panel_cursor(rows.iter().find(|row| frame.on(row)).cloned());

        // Every row: its look, the cursor's, and what a click does.
        let row = |key: InboxPanelRow| {
            let text = key.text(state, checks_state);
            let list_row = ListRow::new(key.id(), text.label)
                .trailing(text.count.to_string())
                .trailing_tone(TrailingTone::Faint)
                .selected(text.selected)
                .flagged(text.flagged);
            let list_row = match key.glyph() {
                Some(glyph) => list_row.icon(glyph),
                // The moldavite dot marks an agent with work waiting.
                None if text.connected => list_row.dot(color::MOLDAVITE),
                None => list_row.blank_slot(),
            };
            let list_row = match text.suffix {
                Some(suffix) => list_row.label_suffix(suffix),
                None => list_row,
            };
            let (inbox, checks) = (inbox.clone(), checks.clone());
            frame
                .mark(&key, list_row)
                .on_click(move |_, _, cx| key.act(&inbox, &checks, cx))
        };
        let sections = sections.into_iter().enumerate().map(|(at, (label, rows))| {
            section(at == 0, label).children(rows.into_iter().map(&row))
        });

        let load_error = state.load_error().cloned();
        let act = (inbox.clone(), checks.clone());
        div()
            .id("inbox-panel")
            .panel_cursor(
                &KEYS,
                &cursor,
                rows,
                home,
                move |row, _, cx| row.act(&act.0, &act.1, cx),
                cx,
            )
            .flex()
            .flex_col()
            .flex_none()
            .w(size::PANEL)
            .overflow_y_scroll()
            .bg(color::CHROME)
            .border_r(size::HAIRLINE)
            .border_color(color::HAIRLINE)
            .children(sections)
            .children(load_error.map(|error| {
                div()
                    .px(spacing::S3_5)
                    .text_role(typography::CAPTION)
                    .text_color(color::DANGER)
                    .child(error)
            }))
    }
}
