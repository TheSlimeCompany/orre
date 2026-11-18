//! UI integration tests for the review queues: the shell's Inbox with its
//! CHECKS in a headless window, against a scratch database seeded with
//! [`fixture`] through [`ChecksStore`], as `coffer` runs it.

use chrono::NaiveDate;
use coffer_core::db::Database;
use coffer_core::duplicate::{self, CandidateStatus};
use coffer_core::exclusion::{self, ExclusionReason};
use coffer_core::reconciliation;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{AppContext as _, Entity, TestAppContext, VisualTestContext};
use tempfile::TempDir;

use super::describe::{
    Action, BalancesDescription, Body, CheckStatus, ChecksDescription, ClaimDescription,
    ComparisonDescription, Dialog, DuplicatesDescription, Header, Inspector, InspectorDescription,
    OwedDescription, PanelRow, PersonDescription, Section,
};
use super::*;
use crate::accounts::{Accounts, fixture as accounts_fixture};
use crate::inbox::Inbox;
use crate::shell::{ShellView, View};

struct Scratch {
    _dir: TempDir,
    path: std::path::PathBuf,
    seeded: fixture::Seeded,
}

impl Scratch {
    fn new(locked: bool) -> Self {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("coffer.db");
        let mut db = Database::open(&path).unwrap();
        let seeded = fixture::seed(&mut db);
        if locked {
            fixture::seed_locked_pair(&mut db, &seeded);
        }
        Self {
            _dir: dir,
            path,
            seeded,
        }
    }

    fn db(&self) -> Database {
        Database::open(&self.path).unwrap()
    }

    fn status(&self, id: DuplicateCandidateId) -> CandidateStatus {
        duplicate::get(&self.db(), id).unwrap().status
    }

    fn excluded(&self, id: TransactionId) -> Option<ExclusionReason> {
        exclusion::get(&self.db(), id)
            .unwrap()
            .map(|exclusion| exclusion.reason)
    }
}

fn open<'a>(
    cx: &'a mut TestAppContext,
    scratch: &Scratch,
) -> (Entity<ShellView>, Entity<Checks>, &'a mut VisualTestContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::component::install(cx);
        crate::shell::init(cx);
    });
    let store = ChecksStore::new(scratch.path.clone());
    let (shell, cx) = cx.add_window_view(|window, cx| {
        let accounts = cx.new(|_| Accounts::fixture(accounts_fixture::accounts()));
        let inbox = cx.new(|_| Inbox::empty());
        let mut shell = ShellView::with_data(View::Inbox, accounts, inbox, window, cx);
        let checks = cx.new(|_| Checks::from_store(store));
        shell.install_checks(checks, window, cx);
        shell
    });
    let checks = cx.update(|_, cx| shell.read(cx).checks().clone());
    redraw(cx);
    (shell, checks, cx)
}

fn redraw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn click(id: &str, cx: &mut VisualTestContext) {
    let id = gpui_kit::SharedString::from(id.to_owned());
    cx.update(move |window, cx| window.click(id, cx));
    redraw(cx);
}

fn shows(id: &str, cx: &mut VisualTestContext) -> bool {
    let id = gpui_kit::SharedString::from(id.to_owned());
    cx.update(move |window, _| window.try_find(id).is_some())
}

/// What the queues show, with the shell's dialog.
fn describe(
    shell: &Entity<ShellView>,
    checks: &Entity<Checks>,
    cx: &mut VisualTestContext,
) -> ChecksDescription {
    cx.update(|_, cx| {
        checks
            .read(cx)
            .describe()
            .with_dialog(shell.read(cx).dialog(), cx)
    })
}

/// Possible duplicates' table.
fn duplicates(shown: &ChecksDescription) -> &DuplicatesDescription {
    match &shown.body {
        Body::Duplicates(table) => table,
        body => panic!("not Possible duplicates: {body:?}"),
    }
}

/// The open pair's comparison.
fn open_pair(shown: &ChecksDescription) -> &ComparisonDescription {
    duplicates(shown).open.as_ref().expect("a pair is open")
}

/// The selected pair's names.
fn selected_names(shown: &ChecksDescription) -> Option<String> {
    duplicates(shown)
        .pairs
        .iter()
        .find(|pair| pair.selected)
        .map(|pair| pair.names.clone())
}

fn balances(shown: &ChecksDescription) -> &BalancesDescription {
    match &shown.body {
        Body::Balances(table) => table,
        body => panic!("not Balance checks: {body:?}"),
    }
}

fn owed(shown: &ChecksDescription) -> &OwedDescription {
    match &shown.body {
        Body::Owed(table) => table,
        body => panic!("not Owed to you: {body:?}"),
    }
}

fn inspected(shown: &ChecksDescription) -> &Inspector {
    match &shown.inspector {
        InspectorDescription::Shown(inspector) => inspector,
        InspectorDescription::Nothing(words) => panic!("nothing selected: {words}"),
    }
}

/// The inspector section titled `title`.
fn section<'a>(shown: &'a ChecksDescription, title: &str) -> &'a Section {
    inspected(shown)
        .sections
        .iter()
        .find(|section| section.title.as_deref() == Some(title))
        .unwrap_or_else(|| panic!("no {title} section"))
}

/// A panel row's count.
fn count(shown: &ChecksDescription, label: &str) -> String {
    let row = shown
        .queues
        .iter()
        .find(|row| row.label == label)
        .expect("a CHECKS row");
    row.count.clone().unwrap_or_default()
}

/// Whether the key `key` would do anything now.
fn key_works(shown: &ChecksDescription, key: &str) -> bool {
    shown
        .actions
        .iter()
        .find(|action| action.key == Some(key))
        .is_some_and(|action| action.enabled)
}

fn button(label: &str, enabled: bool) -> Action {
    Action::button(label, enabled)
}

/// The selected pair, as Checks holds it (for its IDs).
fn selected_pair(checks: &Entity<Checks>, cx: &mut VisualTestContext) -> Option<Pair> {
    cx.update(|_, cx| checks.read(cx).selected_pair().cloned())
}

/// The pending pair on `account`.
fn pair_on(checks: &Entity<Checks>, account: AccountId, cx: &mut VisualTestContext) -> Pair {
    cx.update(|_, cx| {
        checks
            .read(cx)
            .pairs()
            .iter()
            .find(|pair| pair.account == account)
            .cloned()
            .expect("a pair on the account")
    })
}

const OCADO: &str = "OCADO RETAIL LTD HATFIELD GB and OCADO.COM";

/// A 2026 fixture date as the queues write it today ("9 Nov" this year,
/// "9 Nov 26" after).
fn on(checks: &Entity<Checks>, cx: &mut VisualTestContext) -> impl Fn(u32, u32) -> String + use<> {
    let today = cx.update(|_, cx| checks.read(cx).today());
    move |month, day| text::date(NaiveDate::from_ymd_opt(2026, month, day).unwrap(), today)
}

#[gpui_kit::test]
fn checks_lists_the_queues_and_opens_the_first_pair(cx: &mut TestAppContext) {
    let scratch = Scratch::new(false);
    let (shell, checks, cx) = open(cx, &scratch);
    let shown = describe(&shell, &checks, cx);
    let queues: Vec<(&str, String, bool, bool)> = shown
        .queues
        .iter()
        .map(|row| {
            (
                row.label,
                row.count.clone().unwrap_or_default(),
                row.flagged,
                row.selected,
            )
        })
        .collect();
    assert_eq!(
        queues,
        [
            ("Possible duplicates", "3".to_owned(), false, false),
            ("Balance checks", "1".to_owned(), true, false),
            ("Owed to you", "0".to_owned(), false, false),
        ],
        "Ocado, Trainline and Pret; the joint account's 9 Nov fails"
    );
    assert!(shows("inbox-checks-duplicates", cx));

    click("inbox-checks-duplicates", cx);
    let shown = describe(&shell, &checks, cx);
    assert_eq!(shown.title, "Possible duplicates");
    assert_eq!(shown.caption, "3 pairs · nothing changes until you merge");
    assert!(shown.queues[0].selected);
    assert_eq!(selected_names(&shown).as_deref(), Some(OCADO));
    assert_eq!(duplicates(&shown).pairs[0].account, "Joint account");
    let pair = open_pair(&shown);
    assert_eq!(
        pair.actions,
        [
            button("Keep this instead", true),
            button("Merge", true),
            button("Not duplicates", true),
        ],
        "the pair is open in place"
    );
    assert!(shows("checks-merge", cx));
    assert_eq!(
        inspected(&shown).header.title,
        "OCADO RETAIL LTD HATFIELD GB"
    );

    // A suggestion row goes back to the suggestions.
    click("inbox-status-pending", cx);
    let shown = describe(&shell, &checks, cx);
    assert!(shown.queues.iter().all(|row| !row.selected));
    assert_eq!(cx.update(|_, cx| checks.read(cx).queue()), None);
    assert_eq!(cx.update(|_, cx| shell.read(cx).view()), View::Inbox);
}

#[gpui_kit::test]
fn merging_excludes_the_copy_fixes_the_check_and_undoes(cx: &mut TestAppContext) {
    let scratch = Scratch::new(false);
    let (shell, checks, cx) = open(cx, &scratch);
    click("inbox-checks-duplicates", cx);
    let pair = selected_pair(&checks, cx).unwrap();
    let excluded = pair.rows[1].transaction;
    let on = on(&checks, cx);
    let shown = describe(&shell, &checks, cx);
    // The inspector says what merging does: it fixes the 9 Nov check.
    let merge = section(&shown, "If you merge");
    assert!(
        merge
            .rows
            .contains(&("Check".to_owned(), format!("{} passes", on(11, 9)))),
        "{merge:?}"
    );
    assert!(
        merge
            .rows
            .iter()
            .any(|(_, line)| line.ends_with("This pair explains all of it.")),
        "{merge:?}"
    );

    cx.simulate_keystrokes("m");
    redraw(cx);
    assert_eq!(scratch.status(pair.candidate), CandidateStatus::Merged);
    assert_eq!(scratch.excluded(excluded), Some(ExclusionReason::Duplicate));
    let shown = describe(&shell, &checks, cx);
    assert_eq!(count(&shown, "Balance checks"), "0", "the gap is closed");
    assert!(!shown.queues[1].flagged);
    assert_eq!(
        shown.undo_bar,
        Some((
            "Merged OCADO RETAIL LTD HATFIELD GB: OCADO.COM excluded as a duplicate".to_owned(),
            vec![button("Undo", true)],
        ))
    );
    assert_eq!(duplicates(&shown).pairs.len(), 2);
    assert_ne!(selected_names(&shown).as_deref(), Some(OCADO));
    assert!(selected_names(&shown).is_some(), "the next pair opens");

    click("undo-change", cx);
    assert_eq!(scratch.status(pair.candidate), CandidateStatus::Pending);
    assert_eq!(scratch.excluded(excluded), None);
    let shown = describe(&shell, &checks, cx);
    assert_eq!(count(&shown, "Balance checks"), "1");
    assert_eq!(shown.undo_bar, None);
    assert_eq!(
        selected_names(&shown).as_deref(),
        Some(OCADO),
        "the pair opens again"
    );
}

#[gpui_kit::test]
fn keep_this_instead_swaps_the_rows(cx: &mut TestAppContext) {
    let scratch = Scratch::new(false);
    let (shell, checks, cx) = open(cx, &scratch);
    click("inbox-checks-duplicates", cx);
    let pair = selected_pair(&checks, cx).unwrap();
    let (kept, other) = (pair.rows[0].transaction, pair.rows[1].transaction);
    let on = on(&checks, cx);
    let merchant = |shown: &ChecksDescription| {
        open_pair(shown)
            .fields
            .iter()
            .find(|(label, ..)| *label == "Merchant")
            .map(|(_, kept, other)| (kept.clone(), other.clone()))
            .unwrap()
    };
    let before = describe(&shell, &checks, cx);
    assert_eq!(
        merchant(&before),
        (
            "OCADO RETAIL LTD HATFIELD GB".to_owned(),
            "OCADO.COM".to_owned()
        )
    );

    cx.simulate_keystrokes("left");
    redraw(cx);
    let swapped = describe(&shell, &checks, cx);
    assert_eq!(
        merchant(&swapped),
        (
            "OCADO.COM".to_owned(),
            "OCADO RETAIL LTD HATFIELD GB".to_owned()
        ),
        "the typed-in row is kept now"
    );
    assert_eq!(open_pair(&swapped).keep, open_pair(&before).exclude);
    assert_eq!(
        section(&swapped, "If you merge").rows[0],
        (
            "Excludes".to_owned(),
            format!("OCADO RETAIL LTD HATFIELD GB · {}", on(11, 4))
        )
    );

    click("checks-keep-other", cx);
    assert_eq!(merchant(&describe(&shell, &checks, cx)), merchant(&before));

    cx.simulate_keystrokes("right m");
    redraw(cx);
    assert_eq!(scratch.excluded(kept), Some(ExclusionReason::Duplicate));
    assert_eq!(scratch.excluded(other), None);
}

#[gpui_kit::test]
fn not_duplicates_dismisses_and_undoes(cx: &mut TestAppContext) {
    let scratch = Scratch::new(false);
    let (shell, checks, cx) = open(cx, &scratch);
    click("inbox-checks-duplicates", cx);
    let pair = selected_pair(&checks, cx).unwrap();
    click("checks-dismiss", cx);
    assert_eq!(scratch.status(pair.candidate), CandidateStatus::Dismissed);
    let shown = describe(&shell, &checks, cx);
    assert_eq!(count(&shown, "Possible duplicates"), "2");
    assert_eq!(shown.caption, "2 pairs · nothing changes until you merge");
    assert_eq!(
        shown.undo_bar.map(|(message, _)| message).as_deref(),
        Some("OCADO RETAIL LTD HATFIELD GB and OCADO.COM marked as not duplicates")
    );
    click("undo-change", cx);
    assert_eq!(scratch.status(pair.candidate), CandidateStatus::Pending);
    assert_eq!(
        count(&describe(&shell, &checks, cx), "Possible duplicates"),
        "3"
    );
}

#[gpui_kit::test]
fn a_locked_duplicate_needs_an_unlock(cx: &mut TestAppContext) {
    let scratch = Scratch::new(true);
    let (shell, checks, cx) = open(cx, &scratch);
    click("inbox-checks-duplicates", cx);
    let pair = pair_on(&checks, scratch.seeded.savings, cx);
    click(&format!("checks-pair-{}", pair.candidate), cx);
    let on = on(&checks, cx);
    let shown = describe(&shell, &checks, cx);
    let open = open_pair(&shown);
    assert_eq!(
        open.locked,
        Some((
            "The excluded row is in a locked period",
            format!(
                "Savings is locked through {}, and this merge would exclude a row dated {}. \
                 Unlock to merge, or keep the other row instead if it’s outside the lock.",
                on(11, 5),
                on(11, 2)
            )
        ))
    );
    assert_eq!(
        open.actions,
        [
            button("Keep this instead", true),
            button("Unlock…", true),
            button("Merge", false),
            button("Not duplicates", true),
        ]
    );
    assert_eq!(
        open.caption,
        "Dismissing changes no transaction, so it isn’t locked."
    );
    assert!(!key_works(&shown, "m") && key_works(&shown, "n"));
    assert!(
        section(&shown, "If you merge")
            .rows
            .iter()
            .any(|(_, line)| line.ends_with(" fails")),
        "it breaks a check"
    );

    // M does nothing while it's locked.
    cx.simulate_keystrokes("m");
    redraw(cx);
    assert_eq!(scratch.status(pair.candidate), CandidateStatus::Pending);

    // The block's Unlock… opens the unlock dialog; unlocking lets it merge.
    click("checks-pair-unlock", cx);
    let dialog = describe(&shell, &checks, cx)
        .dialog
        .expect("the unlock dialog");
    assert_eq!(dialog.title, "Unlock Savings?");
    assert_eq!(
        dialog.actions,
        [button("Cancel", true), button("Unlock", true)]
    );
    click("unlock-confirm", cx);
    assert_eq!(
        reconciliation::locked_through(&scratch.db(), scratch.seeded.savings).unwrap(),
        None
    );
    let shown = describe(&shell, &checks, cx);
    assert_eq!(shown.dialog, None);
    assert_eq!(open_pair(&shown).locked, None);
    assert!(key_works(&shown, "m"));
}

#[gpui_kit::test]
fn balance_checks_open_the_failure_and_review_its_pair(cx: &mut TestAppContext) {
    let scratch = Scratch::new(false);
    let (shell, checks, cx) = open(cx, &scratch);
    click("inbox-checks-balances", cx);
    let on = on(&checks, cx);
    let shown = describe(&shell, &checks, cx);
    assert_eq!(shown.title, "Balance checks");
    assert_eq!(
        shown.caption,
        "Every balance you enter or import is checked against the transactions · 1 failing"
    );
    assert!(shown.toolbar_keys.is_empty());
    // Failing accounts first, the failure selected.
    let joint = &balances(&shown).accounts[0];
    assert_eq!(joint.name, "Joint account");
    assert_eq!(joint.locked, Some(format!("Locked through {}", on(9, 30))));
    let failed = &joint.rows[0];
    assert_eq!(
        (
            failed.status,
            failed.date.clone(),
            failed.balance.as_str(),
            failed.coffer.as_str(),
            failed.difference.as_str(),
            failed.selected,
        ),
        (
            Some(CheckStatus::Failed),
            on(11, 9),
            "£18.40",
            "−£24.10",
            "−£42.50",
            true
        )
    );
    let (words, review) = failed.window.clone().expect("the window under it");
    assert!(
        words.ends_with("A possible duplicate of −£42.50 explains all of it."),
        "{words}"
    );
    assert_eq!(review, [button("Review pair", true)]);
    assert_eq!(
        inspected(&shown).header.caption,
        Some(format!("Check on {} failed · Coffer is short", on(11, 9)))
    );

    click("checks-review-pair", cx);
    let shown = describe(&shell, &checks, cx);
    assert_eq!(shown.title, "Possible duplicates");
    assert_eq!(selected_names(&shown).as_deref(), Some(OCADO));
}

#[gpui_kit::test]
fn lock_and_unlock_through_the_dialogs(cx: &mut TestAppContext) {
    let scratch = Scratch::new(false);
    let savings = scratch.seeded.savings;
    let (shell, checks, cx) = open(cx, &scratch);
    click("inbox-checks-balances", cx);
    let on = on(&checks, cx);
    let savings_foot = |shown: &ChecksDescription| {
        balances(shown)
            .accounts
            .iter()
            .find(|account| account.name == "Savings")
            .map(|account| (account.locked.clone(), account.foot.clone()))
            .unwrap()
    };
    let (locked, foot) = savings_foot(&describe(&shell, &checks, cx));
    assert_eq!(locked, Some(format!("Locked through {}", on(9, 30))));
    assert_eq!(
        foot.map(|(_, action)| action),
        Some(button(&format!("Lock through {}…", on(10, 31)), true))
    );
    click(&format!("checks-lock-{savings}"), cx);
    assert_eq!(
        describe(&shell, &checks, cx).dialog,
        Some(Dialog {
            title: format!("Lock Savings through {}?", on(10, 31)),
            fields: vec![("Through".to_owned(), "31 Oct 2026".to_owned())],
            error: None,
            actions: vec![button("Cancel", true), button("Lock", true)],
        })
    );
    click("lock-confirm", cx);
    let through = NaiveDate::from_ymd_opt(2026, 10, 31).unwrap();
    assert_eq!(
        reconciliation::locked_through(&scratch.db(), savings).unwrap(),
        Some(through),
        "the latest passing check"
    );
    let shown = describe(&shell, &checks, cx);
    assert_eq!(shown.dialog, None, "the dialog closes");
    let (locked, foot) = savings_foot(&shown);
    assert_eq!(locked, Some(format!("Locked through {}", on(10, 31))));
    assert_eq!(foot, None, "nothing later to lock");

    cx.update(|_, cx| checks.update(cx, |checks, cx| checks.request_unlock(savings, cx)));
    redraw(cx);
    click("unlock-confirm", cx);
    assert_eq!(
        reconciliation::locked_through(&scratch.db(), savings).unwrap(),
        None
    );
    assert_eq!(savings_foot(&describe(&shell, &checks, cx)).0, None);
}

#[gpui_kit::test]
fn the_flagged_dot_opens_the_accounts_failing_check(cx: &mut TestAppContext) {
    let scratch = Scratch::new(false);
    let joint = scratch.seeded.joint;
    let (shell, checks, cx) = open(cx, &scratch);
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| shell.show(View::Transactions, window, cx))
    });
    let accounts = cx.update(|_, cx| shell.read(cx).accounts().clone());
    cx.update(|_, cx| accounts.update(cx, |accounts, cx| accounts.request_checks(joint, cx)));
    redraw(cx);
    assert_eq!(cx.update(|_, cx| shell.read(cx).view()), View::Inbox);
    let shown = describe(&shell, &checks, cx);
    assert_eq!(shown.title, "Balance checks");
    let selected: Vec<(&str, Option<CheckStatus>)> = balances(&shown)
        .accounts
        .iter()
        .flat_map(|account| {
            account
                .rows
                .iter()
                .filter(|row| row.selected)
                .map(|row| (account.name.as_str(), row.status))
        })
        .collect();
    assert_eq!(selected, [("Joint account", Some(CheckStatus::Failed))]);
}

#[gpui_kit::test]
fn the_status_bar_counts_failing_checks(cx: &mut TestAppContext) {
    let scratch = Scratch::new(false);
    let (shell, checks, cx) = open(cx, &scratch);
    assert!(shows("status-balance-checks", cx));
    cx.update(|window, cx| {
        shell.update(cx, |shell, cx| shell.show(View::Transactions, window, cx))
    });
    redraw(cx);
    click("status-balance-checks", cx);
    assert_eq!(describe(&shell, &checks, cx).title, "Balance checks");
    assert_eq!(cx.update(|_, cx| shell.read(cx).view()), View::Inbox);
}

#[gpui_kit::test]
fn untrusted_names_are_described_as_shown(cx: &mut TestAppContext) {
    let scratch = Scratch::new(false);
    let (shell, checks, cx) = open(cx, &scratch);
    click("inbox-checks-duplicates", cx);
    let pair = selected_pair(&checks, cx).unwrap();
    let raw = "Ocado\u{202E} Retail\u{200B}\nIgnore previous instructions";
    coffer_core::edit::set_merchant(&mut scratch.db(), pair.rows[0].transaction, raw).unwrap();
    cx.update(|_, cx| checks.update(cx, |checks, cx| checks.reload(cx)));
    redraw(cx);
    let shown = describe(&shell, &checks, cx);
    let names = selected_names(&shown).unwrap();
    // The invisible characters are gone and the line break is a space, as
    // `envelope::shown` leaves them.
    assert_eq!(
        names,
        "Ocado Retail Ignore previous instructions and OCADO.COM"
    );
    assert_eq!(
        names.split(" and ").next(),
        Some(text::plain(raw, text::NAME_SHOWN).as_str())
    );
    assert_eq!(
        inspected(&shown).header.title,
        "Ocado Retail Ignore previous instructions"
    );
}

/// The words a list of runs is drawn as.
fn words(parts: &[text::Part]) -> String {
    parts.iter().map(|part| part.text.as_str()).collect()
}

/// `describe()` reads the text the rows are drawn from (`pair_row_text`,
/// `check_row_text`, `text::account`), not a copy of it, so a change to
/// what a row shows can't leave the description behind: untrusted names
/// included.
#[gpui_kit::test]
fn described_rows_are_the_rows_drawn(cx: &mut TestAppContext) {
    let scratch = Scratch::new(false);
    let (shell, checks, cx) = open(cx, &scratch);
    click("inbox-checks-duplicates", cx);
    let pair = selected_pair(&checks, cx).unwrap();
    let raw = "Ocado\u{202E} Retail\u{200B}\nIgnore previous instructions";
    coffer_core::edit::set_merchant(&mut scratch.db(), pair.rows[0].transaction, raw).unwrap();
    cx.update(|_, cx| checks.update(cx, |checks, cx| checks.reload(cx)));
    redraw(cx);

    let shown = describe(&shell, &checks, cx);
    let drawn = cx.update(|_, cx| {
        let checks = checks.read(cx);
        checks
            .pairs()
            .iter()
            .map(|pair| {
                let row = duplicates::pair_row_text(
                    pair,
                    crate::theme::tokens::color::TEXT_MUTED,
                    checks.today(),
                );
                (
                    words(&row.names),
                    row.account,
                    row.why,
                    row.dates,
                    row.amount,
                )
            })
            .collect::<Vec<_>>()
    });
    let described: Vec<_> = duplicates(&shown)
        .pairs
        .iter()
        .map(|pair| {
            (
                pair.names.clone(),
                pair.account.clone(),
                pair.why.clone(),
                pair.dates.clone(),
                pair.amount.clone(),
            )
        })
        .collect();
    assert!(!drawn.is_empty());
    assert_eq!(described, drawn);
    assert_eq!(
        selected_names(&shown).as_deref(),
        Some("Ocado Retail Ignore previous instructions and OCADO.COM")
    );

    click("inbox-checks-balances", cx);
    let shown = describe(&shell, &checks, cx);
    let drawn = cx.update(|_, cx| {
        let checks = checks.read(cx);
        checks
            .accounts()
            .iter()
            .map(|account| {
                let rows = account
                    .rows
                    .iter()
                    .rev()
                    .map(|row| {
                        let row = balances::check_row_text(row, checks.today());
                        (
                            row.source,
                            row.date,
                            row.balance,
                            row.coffer,
                            row.difference,
                        )
                    })
                    .collect::<Vec<_>>();
                (text::account(&account.name), rows)
            })
            .collect::<Vec<_>>()
    });
    let described: Vec<_> = balances(&shown)
        .accounts
        .iter()
        .map(|account| {
            let rows = account
                .rows
                .iter()
                .map(|row| {
                    (
                        row.source.clone(),
                        row.date.clone(),
                        row.balance.clone(),
                        row.coffer.clone(),
                        row.difference.clone(),
                    )
                })
                .collect::<Vec<_>>();
            (account.name.clone(), rows)
        })
        .collect();
    assert!(!drawn.is_empty());
    assert_eq!(described, drawn);
}

// Owed to you (M3.11) ----------------------------------------------------------

fn open_owed<'a>(
    cx: &'a mut TestAppContext,
    path: &std::path::Path,
) -> (Entity<ShellView>, Entity<Checks>, &'a mut VisualTestContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::component::install(cx);
        crate::shell::init(cx);
    });
    let store = ChecksStore::new(path.to_owned());
    let (shell, cx) = cx.add_window_view(|window, cx| {
        let accounts = cx.new(|_| Accounts::fixture(accounts_fixture::accounts()));
        let inbox = cx.new(|_| Inbox::empty());
        let mut shell = ShellView::with_data(View::Inbox, accounts, inbox, window, cx);
        let checks = cx.new(|_| Checks::from_store(store));
        shell.install_checks(checks, window, cx);
        shell
    });
    let checks = cx.update(|_, cx| shell.read(cx).checks().clone());
    redraw(cx);
    (shell, checks, cx)
}

fn owed_scratch() -> (TempDir, std::path::PathBuf) {
    use crate::transactions::link_fixture::{self, Variant};

    let dir = TempDir::new().unwrap();
    let path = dir.path().join("coffer.db");
    let mut db = Database::open(&path).unwrap();
    link_fixture::seed(&mut db, Variant::Suggested);
    (dir, path)
}

#[gpui_kit::test]
fn owed_to_you_links_a_matched_payment_and_renames_a_person(cx: &mut TestAppContext) {
    let (_dir, path) = owed_scratch();
    let (shell, checks, cx) = open_owed(cx, &path);

    assert_eq!(count(&describe(&shell, &checks, cx), "Owed to you"), "5");
    click("inbox-checks-owed", cx);
    let shown = describe(&shell, &checks, cx);
    assert_eq!(shown.title, "Owed to you");
    // The first person opens: Sam, who owes most.
    assert_eq!(inspected(&shown).header.title, "Sam");
    assert_eq!(owed(&shown).claims.len(), 3);
    let (strip, buttons) = owed(&shown).matches[0].clone();
    assert!(
        strip.contains("matches the two marked claims exactly"),
        "{strip}"
    );
    assert_eq!(
        buttons,
        [
            button("Link as reimbursement", true),
            button("Not this", true)
        ]
    );

    // "Link as reimbursement" clears both claims, and Undo brings them back.
    let inflow = cx.update(|_, cx| checks.read(cx).data().owed.matches[0].inflow);
    click(&format!("owed-link-{inflow}"), cx);
    let shown = describe(&shell, &checks, cx);
    assert_eq!(owed(&shown).claims.len(), 1);
    assert!(owed(&shown).matches.is_empty());
    let (message, undo) = shown.undo_bar.clone().expect("the undo bar");
    assert!(message.starts_with("Linked "), "{message}");
    assert_eq!(undo, [button("Undo", true)]);
    click("undo-change", cx);
    assert_eq!(owed(&describe(&shell, &checks, cx)).claims.len(), 3);

    // Renaming keeps the person selected under the new name.
    checks.update(cx, |checks, cx| checks.rename_person("Sam K", cx).unwrap());
    redraw(cx);
    let shown = describe(&shell, &checks, cx);
    assert_eq!(inspected(&shown).header.title, "Sam K");
    assert_eq!(
        owed(&shown)
            .people
            .iter()
            .find(|person| person.selected)
            .map(|person| person.name.as_str()),
        Some("Sam K")
    );
    // A blank name is refused.
    assert!(
        checks
            .update(cx, |checks, cx| checks.rename_person("  ", cx))
            .is_err()
    );
}

#[gpui_kit::test]
fn owed_to_you_is_described_in_full(cx: &mut TestAppContext) {
    let (_dir, path) = owed_scratch();
    let (shell, checks, cx) = open_owed(cx, &path);
    click("inbox-checks-owed", cx);
    let on = on(&checks, cx);
    let queue = |label, count: &str, selected| PanelRow {
        label,
        count: Some(count.to_owned()),
        flagged: false,
        selected,
    };
    let claim = |merchant: &str, date, owed: &str, matched| ClaimDescription {
        merchant: merchant.to_owned(),
        account: "Current account".to_owned(),
        date,
        owed: owed.to_owned(),
        matched,
    };
    let row = |date, what: &str| (date, what.to_owned());
    let key = |label: &str, key: &'static str, enabled| Action::keyed(label, key, enabled);
    assert_eq!(
        describe(&shell, &checks, cx),
        ChecksDescription {
            queues: vec![
                queue("Possible duplicates", "0", false),
                queue("Balance checks", "0", false),
                queue("Owed to you", "5", true),
            ],
            title: "Owed to you",
            caption: "£225.73 to come back · 3 expense claims and 2 people".to_owned(),
            toolbar_keys: Vec::new(),
            refused: None,
            body: Body::Owed(OwedDescription {
                claims: vec![
                    claim("Trainline", on(11, 2), "£38.60", true),
                    claim("Premier Inn Leeds City Centre", on(11, 3), "£112.00", true),
                    claim("Uber", on(11, 8), "£14.30", false),
                ],
                matches: vec![(
                    format!(
                        "+£150.60 from ACME LTD EXPENSES on {} matches the two marked claims \
                         exactly. Link it and both clear.",
                        on(11, 9)
                    ),
                    vec![
                        button("Link as reimbursement", true),
                        button("Not this", true),
                    ],
                )],
                people: vec![
                    PersonDescription {
                        name: "Sam".to_owned(),
                        costs: format!("2 since {}", on(10, 30)),
                        last_paid: on(10, 12),
                        owed: "£39.90".to_owned(),
                        selected: true,
                    },
                    PersonDescription {
                        name: "Priya".to_owned(),
                        costs: format!("1 on {}", on(11, 8)),
                        last_paid: "—".to_owned(),
                        owed: "£20.93".to_owned(),
                        selected: false,
                    },
                ],
            }),
            inspector: InspectorDescription::Shown(Box::new(Inspector {
                header: Header {
                    title: "Sam".to_owned(),
                    figure: Some("£39.90".to_owned()),
                    caption: Some("Owes you for 2 shared costs".to_owned()),
                },
                sections: vec![
                    Section {
                        title: Some("Costs".to_owned()),
                        rows: vec![
                            row(on(10, 30), "Dishoom Shoreditch £24.00"),
                            row(on(11, 7), "Odeon Luxe Leicester Square £15.90"),
                        ],
                        actions: Vec::new(),
                    },
                    Section {
                        title: Some("Payments from Sam".to_owned()),
                        rows: vec![row(on(10, 12), "SAM K +£30.00")],
                        actions: vec![button("Link a payment…", true)],
                    },
                    Section {
                        title: None,
                        rows: vec![row("Name".to_owned(), "Sam")],
                        actions: Vec::new(),
                    },
                ],
            })),
            undo_bar: None,
            dialog: None,
            actions: vec![
                key("Merge", "m", false),
                key("Not duplicates", "n", false),
                key("Keep", "left", false),
                key("Keep", "right", false),
                key("Clear the selection", "escape", true),
                key("Move up", "up", true),
                key("Move down", "down", true),
            ],
        }
    );
}

/// Owed to you's rows and the person's inspector are described from the
/// text they're drawn from (`claim_row_text`, `person_row_text`,
/// `payments_title`, `name_value`).
#[gpui_kit::test]
fn described_owed_rows_are_the_rows_drawn(cx: &mut TestAppContext) {
    let (_dir, path) = owed_scratch();
    let (shell, checks, cx) = open_owed(cx, &path);
    click("inbox-checks-owed", cx);
    let shown = describe(&shell, &checks, cx);
    let (claims, people, title, name) = cx.update(|_, cx| {
        let checks = checks.read(cx);
        let today = checks.today();
        let queue = &checks.data().owed;
        let claims = queue
            .claims
            .iter()
            .map(|claim| {
                let row = owed::claim_row_text(claim, today);
                (row.merchant, row.account, row.date, row.owed)
            })
            .collect::<Vec<_>>();
        let people = queue
            .people
            .iter()
            .map(|person| {
                let row = owed::person_row_text(person, today);
                (row.name, words(&row.costs), row.last_paid, row.owed)
            })
            .collect::<Vec<_>>();
        let person = checks.selected_person().expect("a person is selected");
        (
            claims,
            people,
            owed::payments_title(person),
            owed::name_value(person),
        )
    });
    let table = owed(&shown);
    let described: Vec<_> = table
        .claims
        .iter()
        .map(|claim| {
            (
                claim.merchant.clone(),
                claim.account.clone(),
                claim.date.clone(),
                claim.owed.clone(),
            )
        })
        .collect();
    assert!(!claims.is_empty());
    assert_eq!(described, claims);
    let described: Vec<_> = table
        .people
        .iter()
        .map(|person| {
            (
                person.name.clone(),
                person.costs.clone(),
                person.last_paid.clone(),
                person.owed.clone(),
            )
        })
        .collect();
    assert!(!people.is_empty());
    assert_eq!(described, people);
    assert_eq!(
        section(&shown, &title).actions,
        [button(owed::LINK_PAYMENT, true)]
    );
    let names = inspected(&shown)
        .sections
        .iter()
        .find(|section| section.title.is_none())
        .expect("the Name section");
    assert_eq!(names.rows, [(owed::NAME.to_owned(), name)]);
}

/// U.7: the keys bound in the queues' context are exactly [`view::KEYS`],
/// every hinted key is bound and listed by `describe()`, and every bound
/// key but Escape is hinted, so the toolbar, the keymap and `describe()`
/// can't drift.
#[gpui_kit::test]
fn the_toolbar_hints_every_bound_key(cx: &mut TestAppContext) {
    let scratch = Scratch::new(false);
    let (shell, checks, cx) = open(cx, &scratch);
    cx.update(|_, cx| view::KEYS.assert_bound(cx));
    click("inbox-checks-duplicates", cx);
    let shown = describe(&shell, &checks, cx);
    assert_eq!(
        shown.toolbar_keys,
        [
            ("M", "Merge"),
            ("N", "Not duplicates"),
            ("←→", "Keep"),
            ("↑↓", "Select"),
        ]
    );
    for key in view::KEYS.keys() {
        for stroke in key.keys() {
            assert!(
                shown
                    .actions
                    .iter()
                    .any(|action| action.key == Some(*stroke)),
                "{stroke} isn't listed"
            );
        }
        assert_eq!(key.hint().is_none(), key.keys() == ["escape"], "{key:?}");
    }
    let keys: Vec<Option<&str>> = shown.actions.iter().map(|action| action.key).collect();
    assert_eq!(
        keys,
        [
            Some("m"),
            Some("n"),
            Some("left"),
            Some("right"),
            Some("escape"),
            Some("up"),
            Some("down"),
        ]
    );
    // Escape clears the selection, and with it M, N and ←→.
    cx.simulate_keystrokes("escape");
    redraw(cx);
    let shown = describe(&shell, &checks, cx);
    assert_eq!(selected_names(&shown), None);
    assert!(
        ["m", "n", "left", "escape"]
            .iter()
            .all(|key| !key_works(&shown, key))
    );
    assert!(key_works(&shown, "down"));
    cx.simulate_keystrokes("down");
    redraw(cx);
    assert_eq!(
        selected_names(&describe(&shell, &checks, cx)).as_deref(),
        Some(OCADO)
    );
}
