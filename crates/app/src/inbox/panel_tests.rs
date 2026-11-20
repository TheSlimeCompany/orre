//! Tests of the Inbox panel's keyboard cursor (ROADMAP U.9; `DESIGN.md` →
//! "Keyboard paths" → "Inbox and Rules panels"): Tab reaches the panel, ↑↓
//! move the cursor through INBOX, CHECKS, KIND and SOURCE without changing
//! what's open or filtered, and Enter or Space act on the cursor row as a
//! click does. On the scratch database from [`super::tests`], in a real
//! window (whose `Root` binds Tab), with the keys pressed as the user would.

use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{Entity, SharedString, TestAppContext, VisualTestContext, WindowOptions};
use orre_core::inbox::{ORRE_SOURCE, SuggestionKind, SuggestionStatus};

use super::describe::{InboxDescription, PanelDescription};
use super::store::InboxStore;
use super::tests::{Scratch, redraw};
use super::*;
use crate::accounts::{Accounts, fixture as accounts_fixture};
use crate::checks::{Checks, Queue};
use crate::shell::{ShellView, View};
use crate::theme::focus::RING_SELECTOR;
use crate::ui::describe::{ACT_ON_ROW, Action};

/// The shell on the Inbox over `scratch`, in a real window.
fn open(
    cx: &mut TestAppContext,
    scratch: &Scratch,
) -> (Entity<ShellView>, Entity<Inbox>, VisualTestContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::component::install(cx);
        crate::shell::init(cx);
    });
    let store = InboxStore::new(scratch.path.clone());
    let (window, shell) = cx
        .update(|cx| {
            gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
                cx.new(|cx| {
                    let accounts = cx.new(|_| Accounts::fixture(accounts_fixture::accounts()));
                    let inbox = cx.new(|_| Inbox::from_store(store));
                    ShellView::with_data(View::Inbox, accounts, inbox, window, cx)
                })
            })
        })
        .expect("open test window");
    let mut cx = VisualTestContext::from_window(window, cx);
    let inbox = cx.update(|_, cx| shell.read(cx).inbox().clone());
    redraw(&mut cx);
    (shell, inbox, cx)
}

fn press(keys: &str, cx: &mut VisualTestContext) {
    cx.simulate_keystrokes(keys);
    redraw(cx);
}

fn describe(
    shell: &Entity<ShellView>,
    inbox: &Entity<Inbox>,
    cx: &mut VisualTestContext,
) -> InboxDescription {
    cx.update(|_, cx| inbox.read(cx).describe(shell.read(cx).checks().read(cx)))
}

/// The panel as described.
fn panel(
    shell: &Entity<ShellView>,
    inbox: &Entity<Inbox>,
    cx: &mut VisualTestContext,
) -> PanelDescription {
    describe(shell, inbox, cx).panel
}

/// The label of the row the cursor is drawn on, if any.
fn cursor(
    shell: &Entity<ShellView>,
    inbox: &Entity<Inbox>,
    cx: &mut VisualTestContext,
) -> Option<String> {
    let panel = panel(shell, inbox, cx);
    let mut on = panel
        .sections
        .iter()
        .flat_map(|section| &section.rows)
        .filter(|row| row.cursor)
        .map(|row| row.label.clone());
    let first = on.next();
    assert_eq!(on.next(), None, "one cursor at most");
    first
}

/// Whether the focus ring is drawn on the row `id`.
fn ring_on(id: &str, cx: &mut VisualTestContext) -> bool {
    let Some(ring) = cx.debug_bounds(RING_SELECTOR) else {
        return false;
    };
    let id = SharedString::from(id.to_owned());
    cx.update(move |window, _| window.try_find(id).map(|row| row.bounds())) == Some(ring)
}

/// Presses Tab until the panel has focus: the cursor starts on the open
/// row.
fn tab_to_panel(shell: &Entity<ShellView>, inbox: &Entity<Inbox>, cx: &mut VisualTestContext) {
    for _ in 0..40 {
        if cursor(shell, inbox, cx).is_some() {
            return;
        }
        press("tab", cx);
    }
    panic!("Tab never reached the Inbox panel");
}

/// What's open and filtered: what ↑↓ mustn't change.
fn opened(
    shell: &Entity<ShellView>,
    inbox: &Entity<Inbox>,
    cx: &mut VisualTestContext,
) -> (
    SuggestionStatus,
    Option<Queue>,
    Option<SuggestionKind>,
    Option<String>,
) {
    cx.update(|_, cx| {
        let state = inbox.read(cx);
        (
            state.status(),
            shell.read(cx).checks().read(cx).queue(),
            state.kind_filter(),
            state.source_filter().map(ToOwned::to_owned),
        )
    })
}

fn checks(shell: &Entity<ShellView>, cx: &mut VisualTestContext) -> Entity<Checks> {
    cx.update(|_, cx| shell.read(cx).checks().clone())
}

#[gpui_kit::test]
fn tab_reaches_the_panel_and_the_arrows_move_the_cursor_across_sections(cx: &mut TestAppContext) {
    let scratch = Scratch::new(false);
    let (shell, inbox, mut cx) = open(cx, &scratch);
    let cx = &mut cx;
    let before = opened(&shell, &inbox, cx);
    assert_eq!(cursor(&shell, &inbox, cx), None, "no cursor until focused");

    tab_to_panel(&shell, &inbox, cx);
    assert_eq!(cursor(&shell, &inbox, cx).as_deref(), Some("Pending"));
    assert!(
        ring_on(&format!("inbox-status-{}", SuggestionStatus::Pending), cx),
        "the description's cursor is where the ring is drawn"
    );
    // Down past INBOX and the CHECKS label to Balance checks.
    press("down down down down", cx);
    assert_eq!(
        cursor(&shell, &inbox, cx).as_deref(),
        Some(Queue::Balances.title())
    );
    assert!(ring_on("inbox-checks-balances", cx));
    press("down down", cx);
    let first_kind = text::kind_name(KIND_ORDER[0]);
    assert_eq!(
        cursor(&shell, &inbox, cx).as_deref(),
        Some(first_kind),
        "the KIND label is skipped"
    );
    assert_eq!(
        opened(&shell, &inbox, cx),
        before,
        "the arrows change nothing that's open or filtered"
    );
    // The last row is a source: the cursor stops there.
    press(&["down"; 20].join(" "), cx);
    let shown = panel(&shell, &inbox, cx);
    let last = shown.sections.last().unwrap().rows.last().unwrap();
    assert_eq!(shown.sections.last().unwrap().title, "Source");
    assert!(last.cursor, "{shown:?}");
    press("up", cx);
    assert!(
        !panel(&shell, &inbox, cx)
            .sections
            .last()
            .unwrap()
            .rows
            .last()
            .unwrap()
            .cursor
    );
}

/// The audit found Balance checks unreachable from the keyboard: from the
/// panel, ↓ and Enter open it, and the cursor stays on it.
#[gpui_kit::test]
fn enter_opens_balance_checks_and_a_status_goes_back(cx: &mut TestAppContext) {
    let scratch = Scratch::new(false);
    let (shell, inbox, mut cx) = open(cx, &scratch);
    let cx = &mut cx;
    tab_to_panel(&shell, &inbox, cx);
    press("down down down down enter", cx);
    let checks = checks(&shell, cx);
    assert_eq!(
        cx.update(|_, cx| checks.read(cx).queue()),
        Some(Queue::Balances)
    );
    let shown = panel(&shell, &inbox, cx);
    assert_eq!(shown.open, Queue::Balances.title());
    assert_eq!(
        cursor(&shell, &inbox, cx).as_deref(),
        Some("Balance checks")
    );
    // The open queue is selected; the status isn't while it's open.
    let rows: Vec<(&str, bool)> = shown.sections[..2]
        .iter()
        .flat_map(|section| &section.rows)
        .map(|row| (row.label.as_str(), row.selected))
        .collect();
    assert_eq!(
        rows,
        [
            ("Pending", false),
            ("Approved", false),
            ("Rejected", false),
            ("Possible duplicates", false),
            ("Balance checks", true),
            ("Owed to you", false),
        ]
    );

    // Space on Approved closes the queue and shows the approved ones.
    press("up up up space", cx);
    assert_eq!(cx.update(|_, cx| checks.read(cx).queue()), None);
    assert_eq!(
        opened(&shell, &inbox, cx).0,
        SuggestionStatus::Approved,
        "Space acts as a click does"
    );
    let shown = describe(&shell, &inbox, cx);
    assert_eq!(shown.panel.open, "Approved");
    assert_eq!(shown.title, "Approved");
    assert_eq!(cursor(&shell, &inbox, cx).as_deref(), Some("Approved"));
}

#[gpui_kit::test]
fn space_turns_a_kind_or_source_filter_on_and_enter_off(cx: &mut TestAppContext) {
    let scratch = Scratch::new(false);
    let (shell, inbox, mut cx) = open(cx, &scratch);
    let cx = &mut cx;
    tab_to_panel(&shell, &inbox, cx);
    // Pending → … → Owed to you → Rules (the first kind).
    press("down down down down down down", cx);
    press("space", cx);
    assert_eq!(opened(&shell, &inbox, cx).2, Some(KIND_ORDER[0]));
    let shown = panel(&shell, &inbox, cx);
    let kind = &shown.sections[2].rows[0];
    assert!(kind.selected && kind.cursor, "{kind:?}");
    assert_eq!(shown.open, "Pending", "a filter isn't the open row");
    press("enter", cx);
    assert_eq!(opened(&shell, &inbox, cx).2, None);

    // On to Orre's own source, last.
    press(&["down"; 20].join(" "), cx);
    press("enter", cx);
    assert_eq!(opened(&shell, &inbox, cx).3.as_deref(), Some(ORRE_SOURCE));
    let shown = describe(&shell, &inbox, cx);
    let orre = shown.panel.sections[3].rows.last().unwrap();
    assert_eq!(orre.label, "Orre from your edits");
    assert!(orre.selected && orre.cursor);
    assert!(!orre.connected, "Orre's own are neutral");
    assert!(
        shown.groups.iter().all(|group| group
            .footer
            .as_ref()
            .is_some_and(|footer| footer.source.starts_with("Orre"))),
        "only Orre's suggestions are listed"
    );
}

#[gpui_kit::test]
fn the_panel_lists_its_keys_and_counts(cx: &mut TestAppContext) {
    let scratch = Scratch::new(false);
    let (shell, inbox, mut cx) = open(cx, &scratch);
    let cx = &mut cx;
    let shown = panel(&shell, &inbox, cx);
    let key = |label: &str, key: &'static str| Action::keyed(label, key, true);
    assert_eq!(
        shown.keys,
        [
            key("Move up", "up"),
            key("Move down", "down"),
            key(ACT_ON_ROW, "enter"),
            key(ACT_ON_ROW, "space"),
        ]
    );
    let titles: Vec<&str> = shown.sections.iter().map(|section| section.title).collect();
    assert_eq!(titles, ["Inbox", "Checks", "Kind", "Source"]);
    let pending = &shown.sections[0].rows[0];
    assert_eq!(
        (pending.label.as_str(), pending.count.as_str()),
        ("Pending", "15")
    );
    assert!(pending.selected);
    let agent = &shown.sections[3].rows[0];
    assert_eq!(agent.label, fixture::AGENT);
    assert!(agent.connected, "an agent with work waiting");
    assert_eq!(shown.open, "Pending");
}
