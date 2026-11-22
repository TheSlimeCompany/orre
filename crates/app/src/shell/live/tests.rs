//! The shell's live updates over a scratch database: another process's
//! writes reach the views on a check, the app's own writes reload every
//! view of what they changed and aren't reported again, a rebase never
//! drops a write, and a restore pauses the checks.
//!
//! "Another process" is a second connection, as `orre import` or the MCP
//! server would open. Checks are run by hand ([`ShellView::check_changes`]),
//! except in the timer's tests, which start it ([`ShellView::keep_watching`])
//! and advance the test clock.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::time::Duration;

use chrono::NaiveDate;
use gpui_kit::{Entity, TestAppContext, VisualTestContext};
use orre_core::account::{self, AccountEdit, AccountKind, NewAccount};
use orre_core::db::Database;
use orre_core::exclusion::ExclusionReason;
use orre_core::inbox::{self as core_inbox, Filter, NewSuggestion, SuggestionId, SuggestionKind};
use orre_core::manual::{self, ManualTransaction};
use orre_core::mcp_client::{self, AccessLevel, ClientName};
use orre_core::merchant::{self, AliasDefinition, AliasKind, DisplayName};
use orre_core::money::{Currency, Money};
use orre_core::watch::Topic;
use tempfile::TempDir;

use super::super::status::Ai;
use super::*;
use crate::inbox::fixture;
use crate::settings::RestoreTarget;
use crate::settings::restore::RestoreDialog;
use crate::settings::store::SettingsStore;

struct Scratch {
    _dir: TempDir,
    path: PathBuf,
}

impl Scratch {
    /// The Inbox's sample data: accounts, transactions, rules, suggestions
    /// and two enabled clients.
    fn seeded() -> Self {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("orre.db");
        fixture::seed(&mut Database::open(&path).unwrap());
        Self { _dir: dir, path }
    }

    /// A connection of another process's.
    fn db(&self) -> Database {
        Database::open(&self.path).unwrap()
    }

    fn pending(&self) -> Vec<SuggestionId> {
        core_inbox::list(
            &self.db(),
            &Filter {
                status: Some(core_inbox::SuggestionStatus::Pending),
                kind: None,
            },
        )
        .unwrap()
        .into_iter()
        .filter(|suggestion| suggestion.source_client == fixture::AGENT)
        .map(|suggestion| suggestion.id)
        .collect()
    }

    /// Files a rule suggestion as the MCP server would.
    fn file_outside(&self, pattern: &str) -> SuggestionId {
        core_inbox::file_suggestion(
            &mut self.db(),
            &ClientName::new(fixture::AGENT).unwrap(),
            &NewSuggestion {
                kind: SuggestionKind::Rule,
                payload: serde_json::json!({"match_field": "description", "match_kind": "contains",
                    "pattern": pattern, "actions": [{"kind": "set_category", "value": "bills"}]}),
                rationale: Some("From outside.".to_owned()),
            },
        )
        .unwrap()
        .id
    }

    /// Renames an account as `orre accounts` would.
    fn rename_outside(&self, from: &str, to: &str) {
        let mut db = self.db();
        let account = account::list(&db)
            .unwrap()
            .into_iter()
            .find(|account| account.name == from)
            .expect("the fixture has it");
        let mut edit = AccountEdit::of(&account);
        edit.name = to.to_owned();
        account::update(&mut db, account.id, &edit).unwrap();
    }
}

fn init(cx: &mut TestAppContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::component::install(cx);
        crate::shell::init(cx);
    });
}

/// The shell on `view` over `scratch`, as `open` builds it: the baseline
/// first, then the views' first reads, everything read.
fn open<'a>(
    cx: &'a mut TestAppContext,
    scratch: &Scratch,
    view: View,
) -> (Entity<ShellView>, &'a mut VisualTestContext) {
    let watched = baseline(&scratch.path);
    open_watched(cx, scratch, view, watched)
}

fn open_watched<'a>(
    cx: &'a mut TestAppContext,
    scratch: &Scratch,
    view: View,
    watched: Watched,
) -> (Entity<ShellView>, &'a mut VisualTestContext) {
    init(cx);
    let database = scratch.path.clone();
    let (shell, cx) = cx.add_window_view(move |window, cx| {
        let mut shell = ShellView::from_database(view, Ok(database), window, cx);
        shell.watch(watched, cx);
        shell
    });
    settle(cx);
    (shell, cx)
}

fn settle(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// Runs one check and what it reloads; the topics it reported.
fn check(shell: &Entity<ShellView>, cx: &mut VisualTestContext) -> BTreeSet<Topic> {
    cx.update(|_, cx| shell.update(cx, |shell, cx| shell.check_changes(cx)));
    settle(cx);
    last_reported(shell, cx).expect("a check ran")
}

fn last_reported(shell: &Entity<ShellView>, cx: &mut VisualTestContext) -> Option<BTreeSet<Topic>> {
    cx.update(|_, cx| {
        shell
            .read(cx)
            .live
            .as_ref()
            .expect("watching")
            .last_reported
            .clone()
    })
}

fn topics(topics: &[Topic]) -> BTreeSet<Topic> {
    topics.iter().copied().collect()
}

fn account_names(shell: &Entity<ShellView>, cx: &mut VisualTestContext) -> Vec<String> {
    cx.update(|_, cx| {
        let accounts = shell.read(cx).accounts().read(cx);
        accounts
            .data()
            .tree
            .rows()
            .map(|node| node.account.name.clone())
            .collect()
    })
}

fn listed(shell: &Entity<ShellView>, id: SuggestionId, cx: &mut VisualTestContext) -> bool {
    cx.update(|_, cx| shell.read(cx).inbox().read(cx).entry(id).is_some())
}

fn rows(shell: &Entity<ShellView>, cx: &mut VisualTestContext) -> usize {
    cx.update(|_, cx| {
        shell
            .read(cx)
            .transactions()
            .read(cx)
            .register()
            .rows()
            .len()
    })
}

// The topic map ------------------------------------------------------------------

#[test]
fn each_topic_reloads_its_views() {
    let none = Views::default();
    assert_eq!(
        views(&topics(&[Topic::Inbox]), false),
        Views {
            inbox: true,
            ..none
        }
    );
    let ledger = Views {
        transactions: true,
        accounts: true,
        checks: true,
        ..none
    };
    assert_eq!(views(&topics(&[Topic::Ledger]), false), ledger);
    assert_eq!(
        views(&topics(&[Topic::Ledger]), true),
        Views {
            rules: true,
            ..ledger
        },
        "a Ledger change reloads Rules only while it's on show"
    );
    let rules = Views {
        transactions: true,
        rules: true,
        ..none
    };
    assert_eq!(views(&topics(&[Topic::Rules]), false), rules);
    assert_eq!(views(&topics(&[Topic::Rules]), true), rules);
    assert_eq!(
        views(&topics(&[Topic::Settings]), false),
        Views {
            settings: true,
            ..none
        }
    );
    let every = Views {
        inbox: true,
        transactions: true,
        accounts: true,
        checks: true,
        rules: true,
        settings: true,
    };
    assert_eq!(views(&Topic::ALL.into_iter().collect(), false), every);
    assert_eq!(views(&BTreeSet::new(), true), none);
}

#[test]
fn it_checks_every_second_while_active_and_every_five_when_not() {
    assert_eq!(poll_interval(true), Duration::from_secs(1));
    assert_eq!(poll_interval(false), Duration::from_secs(5));
}

// Another process's writes --------------------------------------------------------

/// An MCP client's suggestion shows in the Inbox and its badge on the next
/// check, and the suggestion the user had selected stays selected.
#[gpui_kit::test]
fn a_suggestion_filed_by_a_client_shows_with_the_badge(cx: &mut TestAppContext) {
    let scratch = Scratch::seeded();
    let (shell, cx) = open(cx, &scratch, View::Inbox);
    let inbox = cx.update(|_, cx| shell.read(cx).inbox().clone());
    let selected = scratch.pending()[0];
    cx.update(|_, cx| inbox.update(cx, |inbox, cx| inbox.select(Some(selected), cx)));
    let badge = cx.update(|_, cx| inbox.read(cx).badge());
    assert_eq!(check(&shell, cx), topics(&[]), "nothing changed yet");

    let filed = scratch.file_outside("FROMOUTSIDE");
    assert_eq!(check(&shell, cx), topics(&[Topic::Inbox]));
    assert!(listed(&shell, filed, cx));
    assert_eq!(cx.update(|_, cx| inbox.read(cx).badge()), badge + 1);
    assert_eq!(
        cx.update(|_, cx| inbox.read(cx).selected()),
        Some(selected),
        "the new suggestion takes nothing"
    );
}

/// A row added in a terminal shows in Transactions, and the accounts panel
/// and the status bar follow; the selected transaction stays selected.
#[gpui_kit::test]
fn a_row_added_outside_shows_in_transactions_and_the_status_bar(cx: &mut TestAppContext) {
    let scratch = Scratch::seeded();
    let (shell, cx) = open(cx, &scratch, View::Transactions);
    let transactions = cx.update(|_, cx| shell.read(cx).transactions().clone());
    let first = cx.update(|_, cx| transactions.read(cx).register().rows()[0].id);
    cx.update(|_, cx| transactions.update(cx, |view, cx| view.select([first], cx)));
    let before = rows(&shell, cx);
    let status_before = cx.update(|_, cx| shell.read(cx).status(cx).transactions);
    let account = cx.update(|_, cx| {
        shell.read(cx).accounts().read(cx).data().tree.accounts[0]
            .account
            .id
    });

    let entry = ManualTransaction::new(
        account,
        NaiveDate::from_ymd_opt(2026, 11, 12).unwrap(),
        Money::new(-1234, Currency::GBP),
        "OUTSIDE SHOP",
    );
    manual::create(&mut scratch.db(), &entry).unwrap();
    assert_eq!(check(&shell, cx), topics(&[Topic::Ledger]));

    assert_eq!(rows(&shell, cx), before + 1);
    let status_after = cx.update(|_, cx| shell.read(cx).status(cx).transactions);
    assert_ne!(status_after, status_before, "the status bar counts it");
    let selected: Vec<_> = cx.update(|_, cx| {
        transactions
            .read(cx)
            .selected()
            .iter()
            .map(|row| row.id)
            .collect()
    });
    assert_eq!(selected, [first]);
}

/// Rules counts the ledger, so a Ledger change reloads it, but only while
/// it's the view on show.
#[gpui_kit::test]
fn a_ledger_change_reloads_rules_only_while_it_is_on_show(cx: &mut TestAppContext) {
    let renamed_in_rules = |shell: &Entity<ShellView>, cx: &mut VisualTestContext| {
        cx.update(|_, cx| {
            shell
                .read(cx)
                .rules()
                .read(cx)
                .data()
                .accounts
                .iter()
                .any(|account| account.name == "Renamed outside")
        })
    };

    let scratch = Scratch::seeded();
    let (shell, cx) = open(cx, &scratch, View::Transactions);
    scratch.rename_outside("Savings", "Renamed outside");
    assert_eq!(check(&shell, cx), topics(&[Topic::Ledger]));
    assert!(account_names(&shell, cx).contains(&"Renamed outside".to_owned()));
    assert!(!renamed_in_rules(&shell, cx), "Rules isn't on show");

    let scratch = Scratch::seeded();
    let (shell, cx) = open(cx, &scratch, View::Rules);
    scratch.rename_outside("Savings", "Renamed outside");
    assert_eq!(check(&shell, cx), topics(&[Topic::Ledger]));
    assert!(renamed_in_rules(&shell, cx));
}

/// A merchant name added elsewhere (an approved suggestion, `orre`)
/// moves only the Rules fingerprint, and Transactions shows it.
#[gpui_kit::test]
fn a_merchant_name_added_outside_shows_in_transactions(cx: &mut TestAppContext) {
    let scratch = Scratch::seeded();
    let (shell, cx) = open(cx, &scratch, View::Transactions);
    let named = |shell: &Entity<ShellView>, cx: &mut VisualTestContext| {
        cx.update(|_, cx| {
            shell
                .read(cx)
                .transactions()
                .read(cx)
                .register()
                .rows()
                .iter()
                .filter(|row| row.merchant == "Wasabi Sushi")
                .count()
        })
    };
    assert_eq!(named(&shell, cx), 0);

    let alias = AliasDefinition::new(
        AliasKind::Prefix,
        "WASABI",
        DisplayName::new("Wasabi Sushi").unwrap(),
    )
    .unwrap();
    merchant::add(&mut scratch.db(), &alias).unwrap();
    assert_eq!(check(&shell, cx), topics(&[Topic::Rules]));
    assert!(named(&shell, cx) > 0);
}

/// A client enabled from a terminal shows in Settings' list, and the
/// status bar counts it.
#[gpui_kit::test]
fn a_client_enabled_outside_shows_in_settings_and_the_status_bar(cx: &mut TestAppContext) {
    let scratch = Scratch::seeded();
    let (shell, cx) = open(cx, &scratch, View::Settings);
    let name = ClientName::new("cursor").unwrap();
    let enabled = |shell: &Entity<ShellView>, cx: &mut VisualTestContext| {
        cx.update(|_, cx| shell.read(cx).settings().read(cx).ai().enabled())
    };
    let before = enabled(&shell, cx);

    mcp_client::enable_at(&mut scratch.db(), &name, AccessLevel::Aggregate).unwrap();
    assert_eq!(check(&shell, cx), topics(&[Topic::Settings]));
    assert_eq!(enabled(&shell, cx), before + 1);
    assert!(cx.update(|_, cx| {
        shell
            .read(cx)
            .settings()
            .read(cx)
            .ai()
            .client(&name)
            .is_some()
    }));
    assert_eq!(
        cx.update(|_, cx| shell.read(cx).status(cx).ai),
        Ai::Connected(before + 1)
    );
}

// The app's own writes ---------------------------------------------------------------

/// After the app's own approval or rejection, the next check reports
/// nothing: the shell rebased what the decision changed before the views
/// read it.
#[gpui_kit::test]
fn the_apps_own_decisions_are_not_reported_again(cx: &mut TestAppContext) {
    let scratch = Scratch::seeded();
    let (shell, cx) = open(cx, &scratch, View::Inbox);
    let inbox = cx.update(|_, cx| shell.read(cx).inbox().clone());
    let pending = scratch.pending();
    assert_eq!(check(&shell, cx), topics(&[]));

    cx.update(|_, cx| inbox.update(cx, |inbox, cx| inbox.reject(pending[0], cx)));
    settle(cx);
    assert!(!listed(&shell, pending[0], cx));
    assert_eq!(check(&shell, cx), topics(&[]));

    cx.update(|_, cx| inbox.update(cx, |inbox, cx| inbox.approve(pending[1], cx)));
    settle(cx);
    assert!(!listed(&shell, pending[1], cx));
    assert_eq!(check(&shell, cx), topics(&[]));
}

/// The trap a rebase per view falls into: a write from another process
/// lands, then the app makes its own change to the same topic. The shell
/// rebases before every view of the topic reads, so the outside write is in
/// those reads, and the next check has nothing left to report.
#[gpui_kit::test]
fn a_rebase_never_drops_another_processs_write(cx: &mut TestAppContext) {
    let scratch = Scratch::seeded();
    let (shell, cx) = open(cx, &scratch, View::Transactions);
    assert_eq!(check(&shell, cx), topics(&[]));

    scratch.rename_outside("Savings", "Renamed outside");
    let transactions = cx.update(|_, cx| shell.read(cx).transactions().clone());
    let row = cx.update(|_, cx| transactions.read(cx).register().rows()[0].id);
    cx.update(|_, cx| {
        transactions.update(cx, |view, cx| {
            view.exclude(row, ExclusionReason::Other, Some("Testing"), cx)
        })
    })
    .unwrap();
    settle(cx);

    assert!(
        account_names(&shell, cx).contains(&"Renamed outside".to_owned()),
        "the accounts read after the rebase"
    );
    let excluded = cx.update(|_, cx| {
        transactions
            .read(cx)
            .register()
            .row(row)
            .and_then(|row| row.excluded)
    });
    assert_eq!(excluded, Some(ExclusionReason::Other));
    assert!(
        !check(&shell, cx).contains(&Topic::Ledger),
        "nothing about the ledger is left to report"
    );
}

/// R.19: a new account is selected even with a read of the accounts in
/// flight when it's created: that read started before the write, so it's
/// dropped, and the shell's read after the write has the account.
#[gpui_kit::test]
fn a_created_account_is_selected_with_a_read_in_flight(cx: &mut TestAppContext) {
    let scratch = Scratch::seeded();
    let (shell, cx) = open(cx, &scratch, View::Transactions);
    let accounts = cx.update(|_, cx| shell.read(cx).accounts().clone());
    let created = cx
        .update(|_, cx| {
            accounts.update(cx, |accounts, cx| {
                accounts.reload(cx);
                accounts.create(
                    &NewAccount::new("Holiday fund", AccountKind::Savings, Currency::GBP),
                    None,
                    cx,
                )
            })
        })
        .unwrap();
    settle(cx);
    let (selected, listed) = cx.update(|_, cx| {
        let accounts = accounts.read(cx);
        (accounts.selected(), accounts.data().contains(created))
    });
    assert_eq!(selected, Some(created));
    assert!(listed);
    assert!(!check(&shell, cx).contains(&Topic::Ledger));
}

/// R.19: typing into a rule's form after a save and before the shell's
/// read lands is kept.
#[gpui_kit::test]
fn typing_after_a_rules_save_is_kept_when_the_read_lands(cx: &mut TestAppContext) {
    let scratch = Scratch::seeded();
    let (shell, cx) = open(cx, &scratch, View::Rules);
    let rules = cx.update(|_, cx| shell.read(cx).rules().clone());
    let rule = cx.update(|_, cx| rules.read(cx).data().rules[0].rule.id());
    cx.update(|_, cx| rules.update(cx, |rules, cx| rules.select_rule(Some(rule), cx)));
    settle(cx);
    let saved = cx.update(|_, cx| rules.read(cx).form().unwrap().text.clone()) + " LTD";

    cx.update(|_, cx| {
        rules.update(cx, |rules, cx| {
            let text = saved.clone();
            rules.edit(true, move |form| form.text = text, cx);
            rules.edit(false, |form| form.text = "TYPED".into(), cx);
        })
    });
    settle(cx);

    let text = cx.update(|_, cx| rules.read(cx).form().map(|form| form.text.clone()));
    assert_eq!(text.as_deref(), Some("TYPED"));
    let stored = orre_core::rule::get(&scratch.db(), rule).unwrap();
    assert_eq!(stored.definition().pattern().as_str(), saved);
    assert!(!check(&shell, cx).contains(&Topic::Rules));
}

// Restore, the baseline, the timer --------------------------------------------------

/// The app's own restore: no checks while it runs; afterwards every view
/// shows the restored data, and the next check reports nothing.
#[gpui_kit::test]
fn a_restore_pauses_the_checks_and_every_view_reads_the_restored_data(cx: &mut TestAppContext) {
    let scratch = Scratch::seeded();
    let (shell, cx) = open(cx, &scratch, View::Transactions);
    let store = SettingsStore::new(scratch.path.clone());
    let (backup, _) = store.back_up().unwrap();
    let filed = scratch.file_outside("AFTERTHEBACKUP");
    scratch.rename_outside("Savings", "Renamed outside");
    assert_eq!(check(&shell, cx), topics(&[Topic::Inbox, Topic::Ledger]));
    assert!(listed(&shell, filed, cx));

    let settings = cx.update(|_, cx| shell.read(cx).settings().clone());
    cx.update(|_, cx| settings.update(cx, |settings, cx| settings.restoring(cx)));
    settle(cx);
    cx.update(|_, cx| shell.update(cx, |shell, cx| shell.check_changes(cx)));
    let checking = cx.update(|_, cx| shell.read(cx).live.as_ref().unwrap().checking.is_some());
    assert!(!checking, "no check while the restore runs");

    let restored = store.restore(&backup.path).unwrap();
    cx.update(|_, cx| settings.update(cx, |settings, cx| settings.restored(&restored, cx)));
    settle(cx);

    assert!(!listed(&shell, filed, cx));
    assert!(account_names(&shell, cx).contains(&"Savings".to_owned()));
    assert!(!cx.update(|_, cx| shell.read(cx).live.as_ref().unwrap().paused));
    assert_eq!(
        check(&shell, cx),
        topics(&[]),
        "the restored file is the baseline"
    );
}

/// A restore that fails resumes the checks at once.
#[gpui_kit::test]
fn a_failed_restore_resumes_the_checks(cx: &mut TestAppContext) {
    let scratch = Scratch::seeded();
    let (shell, cx) = open(cx, &scratch, View::Transactions);
    let settings = cx.update(|_, cx| shell.read(cx).settings().clone());
    cx.update(|_, cx| settings.update(cx, |settings, cx| settings.restoring(cx)));
    settle(cx);
    let filed = scratch.file_outside("WHILEPAUSED");
    cx.update(|_, cx| settings.update(cx, |settings, cx| settings.restore_failed(cx)));
    settle(cx);
    assert_eq!(last_reported(&shell, cx), Some(topics(&[Topic::Inbox])));
    assert!(listed(&shell, filed, cx));
}

/// A Restore dialog over `backup`, checked and ready to press Restore.
fn restore_dialog(
    shell: &Entity<ShellView>,
    backup: &std::path::Path,
    cx: &mut VisualTestContext,
) -> Entity<RestoreDialog> {
    let settings = cx.update(|_, cx| shell.read(cx).settings().clone());
    let target = RestoreTarget {
        path: backup.to_path_buf(),
        taken_at: None,
        local: None,
    };
    let dialog =
        cx.update(|window, cx| cx.new(|cx| RestoreDialog::new(settings, target, window, cx)));
    settle(cx);
    assert!(cx.update(|_, cx| dialog.read(cx).can_restore()), "checked");
    dialog
}

fn paused(shell: &Entity<ShellView>, cx: &mut VisualTestContext) -> bool {
    cx.update(|_, cx| shell.read(cx).live.as_ref().unwrap().paused)
}

/// Pressing Restore in the dialog pauses the checks, and a restore that
/// fails (the file went bad after the dialog checked it) leaves them
/// running: an outside write is reported.
#[gpui_kit::test]
fn a_restore_the_dialog_could_not_do_leaves_the_checks_running(cx: &mut TestAppContext) {
    let scratch = Scratch::seeded();
    let (shell, cx) = open(cx, &scratch, View::Transactions);
    let (backup, _) = SettingsStore::new(scratch.path.clone()).back_up().unwrap();
    let dialog = restore_dialog(&shell, &backup.path, cx);
    std::fs::write(&backup.path, b"not a database").unwrap();

    cx.update(|_, cx| dialog.update(cx, |dialog, cx| dialog.restore(cx)));
    assert!(paused(&shell, cx), "paused while it runs");
    settle(cx);
    assert!(
        cx.update(|_, cx| dialog.read(cx).stage() != &crate::settings::restore::Stage::Restoring),
        "the dialog is back and says why"
    );
    assert!(!paused(&shell, cx), "the failure resumed the checks");

    let filed = scratch.file_outside("AFTERTHEFAILURE");
    assert_eq!(check(&shell, cx), topics(&[Topic::Inbox]));
    assert!(listed(&shell, filed, cx));
}

/// A restore done through the dialog leaves the checks running once the
/// round after it has finished.
#[gpui_kit::test]
fn a_restore_through_the_dialog_leaves_the_checks_running(cx: &mut TestAppContext) {
    let scratch = Scratch::seeded();
    let (shell, cx) = open(cx, &scratch, View::Transactions);
    let (backup, _) = SettingsStore::new(scratch.path.clone()).back_up().unwrap();
    let dialog = restore_dialog(&shell, &backup.path, cx);
    let filed = scratch.file_outside("AFTERTHEBACKUP");
    assert_eq!(check(&shell, cx), topics(&[Topic::Inbox]));

    cx.update(|_, cx| dialog.update(cx, |dialog, cx| dialog.restore(cx)));
    assert!(paused(&shell, cx), "paused while it runs");
    settle(cx);
    assert!(!paused(&shell, cx), "resumed after the round");
    assert!(!listed(&shell, filed, cx), "the restored data is shown");
    assert_eq!(check(&shell, cx), topics(&[]));
    let again = scratch.file_outside("AFTERTHERESTORE");
    assert_eq!(check(&shell, cx), topics(&[Topic::Inbox]));
    assert!(listed(&shell, again, cx));
}

/// A write between the baseline and the views' first reads is reported.
#[gpui_kit::test]
fn a_write_between_the_baseline_and_the_first_reads_is_reported(cx: &mut TestAppContext) {
    let scratch = Scratch::seeded();
    let watched = baseline(&scratch.path);
    let filed = scratch.file_outside("BEFORETHEWINDOW");
    let (shell, cx) = open_watched(cx, &scratch, View::Inbox, watched);
    assert!(listed(&shell, filed, cx));
    assert_eq!(check(&shell, cx), topics(&[Topic::Inbox]));
}

/// Without a baseline (no database file yet when it was taken), the first
/// check that compares reloads every view.
#[gpui_kit::test]
fn without_a_baseline_the_first_check_reloads_every_view(cx: &mut TestAppContext) {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("orre.db");
    let watched = baseline(&path);
    fixture::seed(&mut Database::open(&path).unwrap());
    let scratch = Scratch { _dir: dir, path };
    let (shell, cx) = open_watched(cx, &scratch, View::Inbox, watched);
    assert_eq!(check(&shell, cx), Topic::ALL.into_iter().collect());
    assert_eq!(check(&shell, cx), topics(&[]));
}

/// The window becoming active checks at once.
#[gpui_kit::test]
fn becoming_active_checks_at_once(cx: &mut TestAppContext) {
    let scratch = Scratch::seeded();
    let (shell, cx) = open(cx, &scratch, View::Inbox);
    let filed = scratch.file_outside("ONACTIVATION");
    cx.update(|_, cx| shell.update(cx, |shell, cx| shell.activation_changed(false, cx)));
    settle(cx);
    assert_eq!(
        last_reported(&shell, cx),
        None,
        "going inactive doesn't check"
    );
    cx.update(|_, cx| shell.update(cx, |shell, cx| shell.activation_changed(true, cx)));
    settle(cx);
    assert_eq!(last_reported(&shell, cx), Some(topics(&[Topic::Inbox])));
    assert!(listed(&shell, filed, cx));
}

/// Tests (and the snapshot example) never start the timer: time passing
/// checks nothing.
#[gpui_kit::test]
fn no_timer_runs_unless_the_app_starts_it(cx: &mut TestAppContext) {
    let scratch = Scratch::seeded();
    let (shell, cx) = open(cx, &scratch, View::Inbox);
    let filed = scratch.file_outside("NOBODYLOOKED");
    cx.executor().advance_clock(Duration::from_secs(10));
    settle(cx);
    assert_eq!(last_reported(&shell, cx), None);
    assert!(!listed(&shell, filed, cx));
}

/// The timer started by `keep_watching` checks every second while the
/// window is active, and every five once it isn't (its loop, restarted when
/// the activation changes).
#[gpui_kit::test]
fn the_timer_checks_every_second_while_active_and_every_five_when_not(cx: &mut TestAppContext) {
    let scratch = Scratch::seeded();
    let (shell, cx) = open(cx, &scratch, View::Inbox);
    cx.update(|window, cx| shell.update(cx, |shell, cx| shell.keep_watching(window, cx)));
    // The test window's own activation state is not what is under test.
    cx.update(|_, cx| shell.update(cx, |shell, cx| shell.activation_changed(true, cx)));
    settle(cx);
    assert!(cx.update(|_, cx| shell.read(cx).live.as_ref().unwrap().active));
    let advance = |cx: &mut VisualTestContext, by: Duration| {
        cx.executor().advance_clock(by);
        settle(cx);
    };

    let filed = scratch.file_outside("ONTHETIMER");
    advance(cx, Duration::from_millis(500));
    assert!(!listed(&shell, filed, cx), "not before the tick");
    advance(cx, Duration::from_millis(600));
    assert_eq!(last_reported(&shell, cx), Some(topics(&[Topic::Inbox])));
    assert!(listed(&shell, filed, cx));

    cx.update(|_, cx| shell.update(cx, |shell, cx| shell.activation_changed(false, cx)));
    settle(cx);
    let second = scratch.file_outside("WHILEINACTIVE");
    advance(cx, Duration::from_secs(4));
    assert!(!listed(&shell, second, cx), "inactive: not every second");
    advance(cx, Duration::from_millis(1100));
    assert!(listed(&shell, second, cx), "inactive: every five seconds");

    let third = scratch.file_outside("ACTIVEAGAIN");
    cx.update(|_, cx| shell.update(cx, |shell, cx| shell.activation_changed(true, cx)));
    settle(cx);
    assert!(listed(&shell, third, cx), "active again: a check at once");
    let fourth = scratch.file_outside("BACKTOSECONDS");
    advance(cx, Duration::from_millis(1100));
    assert!(listed(&shell, fourth, cx), "active again: every second");
}
