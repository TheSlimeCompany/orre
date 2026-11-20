//! UI integration tests for the Import dialog: the shell and the dialog in
//! a headless window, against a scratch database through
//! [`ImportStore`], as `orre` runs them. Only synthetic fixtures.

use std::path::Path;

use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{Entity, TestAppContext, VisualTestContext};
use orre_core::account::{self, AccountKind, NewAccount};
use orre_core::db::Database;
use orre_core::import::{self as core_import, BatchStatus};
use tempfile::TempDir;

use super::*;
use crate::accounts::Accounts;
use crate::import::ImportNotice;
use crate::shell::{ImportStatement, ShellView, View};

/// A scratch database with one current account using Monzo's preset.
struct Scratch {
    _dir: TempDir,
    path: PathBuf,
    account: AccountId,
}

impl Scratch {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("orre.db");
        let mut db = Database::open(&path).unwrap();
        let mut new = NewAccount::new("Current account", AccountKind::Current, Currency::GBP);
        new.preset = Some("uk-monzo".to_owned());
        let account = account::create(&mut db, &new).unwrap();
        Self {
            _dir: dir,
            path,
            account,
        }
    }

    fn batches(&self) -> Vec<core_import::BatchInfo> {
        core_import::batches(&Database::open(&self.path).unwrap(), self.account).unwrap()
    }
}

fn fixture(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(path)
}

fn open<'a>(
    cx: &'a mut TestAppContext,
    scratch: &Scratch,
) -> (Entity<ShellView>, &'a mut VisualTestContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::component::install(cx);
        crate::shell::init(cx);
    });
    let store = ImportStore::new(scratch.path.clone());
    let (shell, cx) = cx.add_window_view(|window, cx| {
        let accounts = cx.new(|_| Accounts::fixture(crate::accounts::fixture::accounts()));
        let mut shell = ShellView::with_accounts(View::Transactions, accounts, window, cx);
        shell.import_into(store, cx);
        shell
    });
    redraw(cx);
    (shell, cx)
}

fn redraw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn shows(id: &str, cx: &mut VisualTestContext) -> bool {
    let id = gpui_kit::SharedString::from(id.to_owned());
    cx.update(move |window, _| window.try_find(id).is_some())
}

fn click(id: &str, cx: &mut VisualTestContext) {
    let id = gpui_kit::SharedString::from(id.to_owned());
    cx.update(move |window, cx| window.click(id, cx));
    redraw(cx);
}

fn dialog_of(shell: &Entity<ShellView>, cx: &mut VisualTestContext) -> Entity<ImportDialog> {
    cx.update(|_, cx| {
        shell
            .read(cx)
            .dialog()
            .cloned()
            .expect("a dialog is open")
            .downcast::<ImportDialog>()
            .expect("the Import dialog")
    })
}

#[gpui_kit::test]
fn a_monzo_export_is_imported_and_undone(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, cx) = open(cx, &scratch);

    // ⌘I opens the dialog on its File step.
    cx.update(|window, cx| window.dispatch_action(Box::new(ImportStatement), cx));
    redraw(cx);
    let dialog = dialog_of(&shell, cx);
    assert_eq!(cx.update(|_, cx| dialog.read(cx).step()), Step::File);
    assert!(shows("import-choose-file", cx));

    // A file, as dropped on the window: read in the background, then Map.
    shell.update_in(cx, |shell, window, cx| {
        shell.open_import(Some(fixture("presets/uk/monzo.csv")), window, cx)
    });
    redraw(cx);
    cx.update(|_, cx| {
        let dialog = dialog.read(cx);
        assert_eq!(dialog.step(), Step::Map);
        let session = dialog.session().unwrap();
        assert_eq!(session.preset_id(), Some("uk-monzo"));
        assert_eq!(session.account_id(), Some(scratch.account));
    });
    assert!(shows("import-review", cx));

    click("import-review", cx);
    let new = cx.update(|_, cx| {
        let dialog = dialog.read(cx);
        assert_eq!(dialog.step(), Step::Review);
        let review = dialog.review().unwrap();
        assert!(review.after.is_some(), "the rehearsal ran");
        review.preview.counts().new
    });
    assert!(new > 0);
    assert!(
        scratch.batches().is_empty(),
        "nothing written before Import"
    );

    click("import-commit", cx);
    assert!(
        cx.update(|_, cx| shell.read(cx).dialog().is_none()),
        "closed"
    );
    let batches = scratch.batches();
    assert_eq!(batches.len(), 1);
    assert_eq!(batches[0].status, BatchStatus::Committed);
    assert_eq!(batches[0].preset_id.as_deref(), Some("uk-monzo"));
    let imports = cx.update(|_, cx| shell.read(cx).imports().clone());
    assert!(matches!(
        cx.update(|_, cx| imports.read(cx).notice().cloned()),
        Some(ImportNotice::Imported { .. })
    ));

    click("import-undo", cx);
    assert_eq!(scratch.batches()[0].status, BatchStatus::Undone);
    assert!(matches!(
        cx.update(|_, cx| imports.read(cx).notice().cloned()),
        Some(ImportNotice::Undone { .. })
    ));
}

#[gpui_kit::test]
fn a_new_layout_is_mapped_saved_as_a_preset_and_recognised_next_time(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, cx) = open(cx, &scratch);
    shell.update_in(cx, |shell, window, cx| {
        shell.open_import(Some(fixture("csv/signed-amount.csv")), window, cx)
    });
    redraw(cx);
    let dialog = dialog_of(&shell, cx);

    // No preset recognises it, and its dates read both ways.
    let blocked = cx.update(|_, cx| {
        let dialog = dialog.read(cx);
        assert_eq!(dialog.session().unwrap().preset_id(), None);
        dialog.review_blocker()
    });
    assert_eq!(blocked.as_deref(), Some("Choose the date format."));
    dialog.update(cx, |dialog, cx| {
        dialog.choose(Pick::Format, &"%d/%m/%Y".into(), cx)
    });
    assert_eq!(cx.update(|_, cx| dialog.read(cx).review_blocker()), None);

    // Save as preset…: named, saved in the data dir, set on the account.
    click("import-save-as-preset", cx);
    dialog.update_in(cx, |dialog, window, cx| {
        dialog.set_preset_name("Example Bank", window, cx)
    });
    redraw(cx);
    click("import-save-preset", cx);
    cx.update(|_, cx| {
        let dialog = dialog.read(cx);
        assert!(!dialog.saving, "{:?}", dialog.save_problem);
        assert_eq!(
            dialog.session().unwrap().preset_id(),
            Some("uk-example-bank")
        );
    });
    let saved = orre_core::paths::presets_dir(&scratch.path).join("uk/example-bank.toml");
    assert!(saved.is_file());
    let account = account::list(&Database::open(&scratch.path).unwrap())
        .unwrap()
        .into_iter()
        .find(|a| a.id == scratch.account)
        .unwrap();
    assert_eq!(account.preset.as_deref(), Some("uk-example-bank"));

    // The next import of the same layout is recognised by its header.
    click("import-close", cx);
    shell.update_in(cx, |shell, window, cx| {
        shell.open_import(Some(fixture("csv/signed-amount.csv")), window, cx)
    });
    redraw(cx);
    let again = dialog_of(&shell, cx);
    cx.update(|_, cx| {
        let session = again.read(cx).session().unwrap().clone();
        assert_eq!(session.preset_id(), Some("uk-example-bank"));
        assert_eq!(
            session.csv().unwrap().chosen,
            crate::import::session::Chosen::Header
        );
    });
}

#[gpui_kit::test]
fn a_file_that_isnt_a_statement_stays_on_file(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, cx) = open(cx, &scratch);
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("notes.pdf");
    std::fs::write(&path, b"%PDF-1.7").unwrap();
    shell.update_in(cx, |shell, window, cx| {
        shell.open_import(Some(path), window, cx)
    });
    redraw(cx);
    let dialog = dialog_of(&shell, cx);
    cx.update(|_, cx| {
        let dialog = dialog.read(cx);
        assert_eq!(dialog.step(), Step::File);
        assert_eq!(
            dialog.problem().map(|p| p.to_string()).as_deref(),
            Some("Orre imports CSV, OFX and QFX files, and notes.pdf isn't one it can read.")
        );
    });
}

/// The bank's corrections (R.4): the Review step counts them and says what
/// importing does, its button names them, and pressing it accepts them.
#[gpui_kit::test]
fn the_banks_corrections_are_shown_and_accepted_by_import(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    {
        let mut db = Database::open(&scratch.path).unwrap();
        let file = orre_import::SourceFile::read(&fixture("ofx/corrections-first.ofx")).unwrap();
        let statement =
            orre_import::AnyImporter::import(&orre_import::ofx::OfxImporter::new(), &file).unwrap();
        let preview =
            core_import::preview(&db, scratch.account, "corrections-first.ofx", statement).unwrap();
        core_import::commit(&mut db, &preview, preview.needs_confirmation()).unwrap();
    }
    let (shell, cx) = open(cx, &scratch);
    shell.update_in(cx, |shell, window, cx| {
        shell.open_import(Some(fixture("ofx/corrections-next.ofx")), window, cx)
    });
    redraw(cx);
    let dialog = dialog_of(&shell, cx);
    // The only GBP account takes it.
    assert_eq!(
        cx.update(|_, cx| dialog.read(cx).session().unwrap().account_id()),
        Some(scratch.account)
    );
    click("import-review", cx);

    let (counts, label, block) = cx.update(|_, cx| {
        let dialog = dialog.read(cx);
        assert_eq!(dialog.step(), Step::Review);
        let preview = &dialog.review().unwrap().preview;
        (
            preview.counts(),
            text::primary_label(preview),
            text::corrections(preview).map(|block| text::joined(&block.body)),
        )
    });
    assert_eq!(counts.corrections, 3);
    assert_eq!(label, "Import 1 transaction and 3 corrections");
    assert_eq!(
        block.as_deref(),
        Some(
            "The bank sent a new date or amount for transactions you already have. Importing \
             replaces each: the old ones stay under the Excluded filter as replaced, and what you \
             set (category, notes, tags) moves to the new ones where it fits."
        )
    );

    click("import-commit", cx);
    assert!(
        cx.update(|_, cx| shell.read(cx).dialog().is_none()),
        "closed"
    );
    let batches = scratch.batches();
    assert_eq!(batches.len(), 2);
    assert_eq!(batches[1].status, BatchStatus::Committed);
    let db = Database::open(&scratch.path).unwrap();
    let superseded = (1..=3)
        .filter(|id| {
            orre_core::exclusion::get(&db, orre_core::id::TransactionId::new(*id))
                .unwrap()
                .is_some_and(|exclusion| {
                    exclusion.reason == orre_core::exclusion::ExclusionReason::Superseded
                })
        })
        .count();
    assert_eq!(superseded, 3);
}

/// What the Review step says about a file's account number, for the file
/// opened in the dialog.
fn other_account_block(
    cx: &mut TestAppContext,
    scratch: &Scratch,
    file: &str,
) -> Option<(text::Tone, String, String)> {
    let (shell, cx) = open(cx, scratch);
    shell.update_in(cx, |shell, window, cx| {
        shell.open_import(Some(fixture(file)), window, cx)
    });
    redraw(cx);
    let dialog = dialog_of(&shell, cx);
    click("import-review", cx);
    cx.update(|_, cx| {
        let dialog = dialog.read(cx);
        assert_eq!(dialog.step(), Step::Review);
        text::other_account(&dialog.review().unwrap().preview)
            .map(|block| (block.tone, block.title, text::joined(&block.body)))
    })
}

#[gpui_kit::test]
fn a_file_for_another_account_number_is_warned_about_in_review(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    // The account has no hash yet: nothing to compare with.
    assert_eq!(other_account_block(cx, &scratch, "ofx/bank-sgml.ofx"), None);

    // Current has had `multi-statement.ofx`'s first statement.
    {
        let mut db = Database::open(&scratch.path).unwrap();
        let file = orre_import::SourceFile::read(&fixture("ofx/multi-statement.ofx")).unwrap();
        let statement = orre_import::AnyImporter::import(
            &orre_import::ofx::OfxImporter::for_statement(0),
            &file,
        )
        .unwrap();
        let preview =
            core_import::preview(&db, scratch.account, "multi-statement.ofx", statement).unwrap();
        core_import::commit(&mut db, &preview, preview.needs_confirmation()).unwrap();
    }

    let (tone, title, body) = other_account_block(cx, &scratch, "ofx/bank-sgml.ofx")
        .expect("a warning: the file's account number is another");
    assert_eq!(tone, text::Tone::OtherAccount);
    assert_eq!(title, "This file looks like it’s from a different account");
    assert!(
        body.starts_with("Current account has only had files with other account numbers."),
        "{body}"
    );
    for shown in [&title, &body] {
        assert!(!shown.contains("0000"), "no part of the number: {shown}");
    }
}

#[test]
fn the_warning_names_the_account_the_file_belongs_to() {
    let scratch = Scratch::new();
    {
        let mut db = Database::open(&scratch.path).unwrap();
        let savings = account::create(
            &mut db,
            &NewAccount::new("Savings", AccountKind::Savings, Currency::GBP),
        )
        .unwrap();
        let file = orre_import::SourceFile::read(&fixture("ofx/multi-statement.ofx")).unwrap();
        for (account, index) in [(scratch.account, 0), (savings, 1)] {
            let statement = orre_import::AnyImporter::import(
                &orre_import::ofx::OfxImporter::for_statement(index),
                &file,
            )
            .unwrap();
            let preview =
                core_import::preview(&db, account, "multi-statement.ofx", statement).unwrap();
            core_import::commit(&mut db, &preview, preview.needs_confirmation()).unwrap();
        }
        // The savings statement into Current: the warning is the account's
        // name, as the dialog would show it.
        let statement = orre_import::AnyImporter::import(
            &orre_import::ofx::OfxImporter::for_statement(1),
            &file,
        )
        .unwrap();
        let preview =
            core_import::preview(&db, scratch.account, "multi-statement.ofx", statement).unwrap();
        let block = text::other_account(&preview).expect("a warning");
        assert_eq!(block.title, "This file looks like it’s from Savings");

        // An account that has had no file yet is warned only that another
        // account has had this number.
        let spare = account::create(
            &mut db,
            &NewAccount::new("Spare", AccountKind::Current, Currency::GBP),
        )
        .unwrap();
        let statement = orre_import::AnyImporter::import(
            &orre_import::ofx::OfxImporter::for_statement(1),
            &file,
        )
        .unwrap();
        let preview = core_import::preview(&db, spare, "multi-statement.ofx", statement).unwrap();
        let block = text::other_account(&preview).expect("a warning");
        assert_eq!(block.title, "This file looks like it’s from Savings");
        assert!(
            text::joined(&block.body)
                .starts_with("Files with this account number have gone into Savings."),
            "{}",
            text::joined(&block.body)
        );
    }
}
