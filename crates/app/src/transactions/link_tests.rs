//! UI integration tests for the inspector's Share and Links sections and
//! the link dialog (M3.11), on D6.5's fixture in a scratch database,
//! through [`TransactionStore`] as `orre` runs them.

use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{Bounds, Pixels, SharedString, TestAppContext, VisualTestContext};
use orre_core::db::Database;
use orre_core::link::{LinkKind, LinkStatus};
use orre_core::money::{Currency, Money};
use orre_core::reporting::register::LinkSide;
use tempfile::TempDir;

use super::describe::{FieldDescription, TransactionsDescription};
use super::link_dialog::{self, Choice, LinkDialog};
use super::link_fixture::{self, Seeded, Variant};
use super::links::LinkFrom;
use super::*;

fn gbp(minor: i64) -> Money {
    Money::new(minor, Currency::GBP)
}

fn redraw(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

fn click(id: impl Into<SharedString>, cx: &mut VisualTestContext) {
    let id = id.into();
    cx.update(move |window, cx| window.click(id, cx));
    redraw(cx);
}

fn shows(id: impl Into<SharedString>, cx: &mut VisualTestContext) -> bool {
    let id = id.into();
    cx.update(move |window, _| window.try_find(id).is_some())
}

struct Scratch {
    _dir: TempDir,
    path: std::path::PathBuf,
    seeded: Seeded,
}

fn scratch(variant: Variant) -> Scratch {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("orre.db");
    let mut db = Database::open(&path).unwrap();
    let seeded = link_fixture::seed(&mut db, variant);
    Scratch {
        _dir: dir,
        path,
        seeded,
    }
}

fn open<'a>(
    cx: &'a mut TestAppContext,
    scratch: &Scratch,
) -> (Entity<TransactionsView>, &'a mut VisualTestContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::component::install(cx);
        init(cx);
    });
    let store = TransactionStore::new(scratch.path.clone());
    let (view, cx) = cx.add_window_view(move |window, cx| {
        TransactionsView::new(Source::Database(store), window, cx)
    });
    redraw(cx);
    (view, cx)
}

fn describe(
    view: &Entity<TransactionsView>,
    cx: &mut VisualTestContext,
) -> TransactionsDescription {
    cx.update(|_, cx| view.read(cx).describe(cx))
}

fn select(view: &Entity<TransactionsView>, id: TransactionId, cx: &mut VisualTestContext) {
    view.update(cx, |view, cx| view.select([id], cx));
    redraw(cx);
}

fn statuses(
    view: &Entity<TransactionsView>,
    id: TransactionId,
    cx: &mut VisualTestContext,
) -> Vec<(LinkKind, LinkStatus)> {
    cx.update(|_, cx| {
        view.read(cx)
            .register()
            .row(id)
            .unwrap()
            .links
            .iter()
            .map(|link| (link.kind, link.status))
            .collect()
    })
}

#[gpui_kit::test]
fn the_suggestion_card_links_and_undo_brings_it_back(cx: &mut TestAppContext) {
    let scratch = scratch(Variant::Suggested);
    let (view, cx) = open(cx, &scratch);
    let refund = scratch.seeded.asos_refund;
    select(&view, refund, cx);
    assert!(shows("link-suggestion", cx));
    assert_eq!(
        statuses(&view, refund, cx),
        [(LinkKind::Refund, LinkStatus::Suggested)]
    );

    click("link-suggestion", cx);
    assert_eq!(
        statuses(&view, refund, cx),
        [(LinkKind::Refund, LinkStatus::Confirmed)]
    );
    let description = describe(&view, cx);
    assert_eq!(
        description.undo_message(),
        Some("Linked the ASOS.com refund to the 28 Oct purchase")
    );
    // The refund's category is now the purchase's, read-only.
    assert!(!shows("inspector-category", cx));
    let category = description
        .one()
        .unwrap()
        .fields
        .iter()
        .find(|field| field.label == "Category")
        .cloned();
    assert!(
        matches!(category, Some(FieldDescription { locked: true, .. })),
        "{category:?}"
    );
    assert_eq!(
        description.row(refund).unwrap().counts_as.as_deref(),
        Some("refund")
    );
    let purchase = scratch.seeded.asos_purchase;
    let left = cx.update(|_, cx| view.read(cx).register().row(purchase).unwrap().share.left);
    assert_eq!(left, gbp(3_400));

    click("undo-change", cx);
    assert_eq!(
        statuses(&view, refund, cx),
        [(LinkKind::Refund, LinkStatus::Suggested)]
    );
}

#[gpui_kit::test]
fn not_a_refund_rejects_the_pair(cx: &mut TestAppContext) {
    let scratch = scratch(Variant::Suggested);
    let (view, cx) = open(cx, &scratch);
    let refund = scratch.seeded.asos_refund;
    select(&view, refund, cx);
    click("reject-suggestion", cx);
    assert!(
        statuses(&view, refund, cx).is_empty(),
        "a rejected pair isn't listed"
    );
    assert!(!shows("link-suggestion", cx));
    assert!(shows("link", cx), "Link… stays");
}

#[gpui_kit::test]
fn a_links_cross_unlinks_it(cx: &mut TestAppContext) {
    let scratch = scratch(Variant::PartlyRepaid);
    let (view, cx) = open(cx, &scratch);
    let trainline = scratch.seeded.trainline;
    select(&view, trainline, cx);
    let (left, link) = cx.update(|_, cx| {
        let row = view.read(cx).register().row(trainline).unwrap().clone();
        let link = row
            .links
            .iter()
            .find(|link| link.side == LinkSide::Purchase)
            .unwrap()
            .id;
        (row.share.left, link)
    });
    assert_eq!(left, gbp(1_860));
    click(format!("unlink-{link}"), cx);
    let left = cx.update(|_, cx| view.read(cx).register().row(trainline).unwrap().share.left);
    assert_eq!(left, gbp(3_860));
    assert_eq!(
        describe(&view, cx).undo_message(),
        Some("Unlinked the reimbursement")
    );
}

#[gpui_kit::test]
fn share_saves_claim_back_and_a_person(cx: &mut TestAppContext) {
    let scratch = scratch(Variant::Suggested);
    let (view, cx) = open(cx, &scratch);
    let odeon = scratch.seeded.odeon;
    select(&view, odeon, cx);
    // Shared with Sam: Owed by shows.
    assert!(shows("owed-by", cx));

    click("share-claim", cx);
    let share = cx.update(|_, cx| view.read(cx).register().row(odeon).unwrap().share.clone());
    assert!(share.claim);
    assert_eq!(share.label, None);

    // Shared waits for a person, then saves with Yours.
    click("share-shared", cx);
    assert!(shows("owed-by", cx));
    let share = cx.update(|_, cx| view.read(cx).register().row(odeon).unwrap().share.clone());
    assert!(share.claim, "nothing saved until a person is chosen");
    view.update(cx, |view, cx| view.choose_person("Priya", cx));
    redraw(cx);
    let share = cx.update(|_, cx| view.read(cx).register().row(odeon).unwrap().share.clone());
    assert!(!share.claim);
    assert_eq!(share.label.as_deref(), Some("Priya"));
    assert_eq!(share.my_share, Some(gbp(-1_590)));
}

#[gpui_kit::test]
fn the_dialog_links_a_refund_to_the_chosen_purchase(cx: &mut TestAppContext) {
    let scratch = scratch(Variant::Suggested);
    let (view, cx) = open(cx, &scratch);
    let refund = scratch.seeded.asos_refund;
    let dialog = cx.update(|window, cx| {
        let view = view.clone();
        cx.new(|cx| LinkDialog::new(view, LinkFrom::Transaction(refund), window, cx))
    });
    redraw(cx);
    let (kind, chosen) =
        cx.update(|_, cx| (dialog.read(cx).kind(), dialog.read(cx).chosen().cloned()));
    assert_eq!(kind, LinkKind::Refund);
    assert_eq!(
        chosen,
        Some(Choice::Transaction(scratch.seeded.asos_purchase)),
        "the same merchant's newest purchase"
    );
    // The fully refunded one is listed, blocked.
    let candidates = cx.update(|_, cx| dialog.read(cx).candidates(cx));
    assert_eq!(candidates.len(), 3);
    assert!(candidates[2].blocked.is_some());

    dialog.update(cx, |dialog, cx| dialog.submit(cx));
    redraw(cx);
    assert_eq!(cx.update(|_, cx| dialog.read(cx).error().cloned()), None);
    assert_eq!(
        statuses(&view, refund, cx),
        [(LinkKind::Refund, LinkStatus::Confirmed)]
    );

    // A settlement from a person's "Link a payment…": the newest money in,
    // as much as both sides can take.
    let acme = scratch.seeded.acme;
    let dialog = cx.update(|window, cx| {
        let view = view.clone();
        cx.new(|cx| {
            LinkDialog::new(
                view,
                LinkFrom::Person(
                    orre_core::link::ShareLabel::new("Sam").unwrap(),
                    Currency::GBP,
                ),
                window,
                cx,
            )
        })
    });
    redraw(cx);
    dialog.update_in(cx, |dialog, window, cx| {
        dialog.choose(Choice::Transaction(acme), window, cx)
    });
    dialog.update(cx, |dialog, cx| dialog.submit(cx));
    redraw(cx);
    assert_eq!(cx.update(|_, cx| dialog.read(cx).error().cloned()), None);
    assert_eq!(
        statuses(&view, acme, cx)
            .into_iter()
            .filter(|(_, status)| *status == LinkStatus::Confirmed)
            .collect::<Vec<_>>(),
        [(LinkKind::Settlement, LinkStatus::Confirmed)]
    );
    let sam = cx.update(|_, cx| {
        view.read(cx)
            .register()
            .people()
            .iter()
            .find(|person| person.label.as_str() == "Sam")
            .map(|person| person.owed)
    });
    assert_eq!(sam, Some(gbp(0)));
}

#[gpui_kit::test]
fn the_arrows_choose_a_candidate_past_blocked_ones_and_enter_links(cx: &mut TestAppContext) {
    let scratch = scratch(Variant::Suggested);
    let (view, cx) = open(cx, &scratch);
    let refund = scratch.seeded.asos_refund;
    let transactions = view.clone();
    let (dialog, cx) = cx.add_window_view(move |window, cx| {
        LinkDialog::new(transactions, LinkFrom::Transaction(refund), window, cx)
    });
    redraw(cx);
    // Every purchase, not just ASOS.com's: the search starts with the
    // merchant, so it's cleared.
    for _ in 0.."ASOS.com".len() {
        cx.simulate_keystrokes("backspace");
    }
    redraw(cx);
    let candidates = cx.update(|_, cx| dialog.read(cx).candidates(cx));
    assert!(candidates.len() > 3, "{candidates:?}");
    assert!(candidates[0].blocked.is_none() && candidates[1].blocked.is_none());
    assert!(
        candidates[2].blocked.is_some(),
        "ASOS.com's refunded purchase"
    );
    assert!(candidates[3].blocked.is_none());
    let chosen = |cx: &mut VisualTestContext| cx.update(|_, cx| dialog.read(cx).chosen().cloned());
    let at = |index: usize| Some(Choice::Transaction(candidates[index].transaction));
    assert_eq!(chosen(cx), at(0));

    cx.simulate_keystrokes("down");
    assert_eq!(chosen(cx), at(1));
    cx.simulate_keystrokes("down");
    assert_eq!(chosen(cx), at(3), "past the blocked row");
    cx.simulate_keystrokes("up");
    assert_eq!(chosen(cx), at(1), "past it on the way back too");
    cx.simulate_keystrokes("up up");
    assert_eq!(chosen(cx), at(0), "stopping at the top");
    cx.simulate_keystrokes("down");
    assert_eq!(chosen(cx), at(1));

    cx.simulate_keystrokes("enter");
    redraw(cx);
    assert_eq!(cx.update(|_, cx| dialog.read(cx).error().cloned()), None);
    assert_eq!(
        statuses(&view, candidates[1].transaction, cx),
        [(LinkKind::Refund, LinkStatus::Confirmed)],
        "Enter linked the chosen purchase"
    );
}

/// Opens the link dialog from `from` as its own window, for the keyboard.
fn open_dialog<'a>(
    cx: &'a mut VisualTestContext,
    view: &Entity<TransactionsView>,
    from: LinkFrom,
) -> (Entity<LinkDialog>, &'a mut VisualTestContext) {
    let transactions = view.clone();
    let (dialog, cx) =
        cx.add_window_view(move |window, cx| LinkDialog::new(transactions, from, window, cx));
    redraw(cx);
    (dialog, cx)
}

/// The bounds of `id` in the last frame.
fn bounds(id: impl Into<SharedString>, cx: &mut VisualTestContext) -> Bounds<Pixels> {
    let id = id.into();
    cx.update(move |window, _| window.find(id).bounds())
}

/// The rows are eight rows tall at most, and `chosen`'s row shows whole
/// inside them while `first`'s has scrolled out above.
fn assert_scrolled_to(first: &str, chosen: &str, cx: &mut VisualTestContext) {
    redraw(cx);
    let list = bounds("link-rows", cx);
    assert_eq!(
        list.size.height,
        link_dialog::rows_height(),
        "the rows stop growing at eight"
    );
    let row = bounds(chosen.to_owned(), cx);
    assert!(
        row.top() >= list.top() && row.bottom() <= list.bottom(),
        "the chosen row {row:?} shows inside the rows {list:?}"
    );
    assert!(
        bounds(first.to_owned(), cx).bottom() <= list.top(),
        "the first row has scrolled out above"
    );
}

#[gpui_kit::test]
fn the_arrows_scroll_the_chosen_candidate_into_view(cx: &mut TestAppContext) {
    let scratch = scratch(Variant::Suggested);
    {
        let mut db = Database::open(&scratch.path).unwrap();
        link_fixture::crowd(&mut db, scratch.seeded.current, 12);
    }
    let (view, cx) = open(cx, &scratch);
    let (dialog, cx) = open_dialog(cx, &view, LinkFrom::Transaction(scratch.seeded.asos_refund));
    for _ in 0.."ASOS.com".len() {
        cx.simulate_keystrokes("backspace");
    }
    redraw(cx);
    let candidates = cx.update(|_, cx| dialog.read(cx).candidates(cx));
    assert!(candidates.len() > 10, "{}", candidates.len());
    let row = |index: usize| format!("link-candidate-{}", candidates[index].transaction);
    let last = candidates.len() - 1;
    assert!(candidates[last].blocked.is_none());
    // Before any move, the last row is out of view below.
    let list = bounds("link-rows", cx);
    assert!(bounds(row(last), cx).top() >= list.bottom());

    for _ in 0..last {
        cx.simulate_keystrokes("down");
    }
    assert_eq!(
        cx.update(|_, cx| dialog.read(cx).chosen().cloned()),
        Some(Choice::Transaction(candidates[last].transaction))
    );
    assert_scrolled_to(&row(0), &row(last), cx);
}

#[gpui_kit::test]
fn the_arrows_scroll_the_chosen_person_into_view(cx: &mut TestAppContext) {
    let scratch = scratch(Variant::Suggested);
    {
        let mut db = Database::open(&scratch.path).unwrap();
        link_fixture::crowd(&mut db, scratch.seeded.current, 12);
    }
    let (view, cx) = open(cx, &scratch);
    let (dialog, cx) = open_dialog(cx, &view, LinkFrom::Transaction(scratch.seeded.acme));
    dialog.update_in(cx, |dialog, window, cx| {
        dialog.set_kind(LinkKind::Settlement, window, cx)
    });
    redraw(cx);
    assert_eq!(
        cx.update(|_, cx| dialog.read(cx).kind()),
        LinkKind::Settlement
    );
    let first = cx.update(|_, cx| dialog.read(cx).chosen().cloned());
    let Some(Choice::Person(first)) = first else {
        panic!("a person is chosen first: {first:?}");
    };
    // Sam, Priya and the twelve stallholders.
    for _ in 0..13 {
        cx.simulate_keystrokes("down");
    }
    let last = cx.update(|_, cx| dialog.read(cx).chosen().cloned());
    let Some(Choice::Person(last)) = last else {
        panic!("a person is chosen: {last:?}");
    };
    assert_ne!(first, last);
    assert_scrolled_to(
        &format!("link-person-{}", first.as_str()),
        &format!("link-person-{}", last.as_str()),
        cx,
    );
}
