//! The arrivals model: copy, merging, replacing, and the unseen → hold →
//! fade → gone clock, driven by injected instants.

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use orre_core::id::TransactionId;
use orre_core::inbox::SuggestionId;

use super::*;

fn tx(ids: &[i64]) -> BTreeSet<TransactionId> {
    ids.iter().map(|id| TransactionId::new(*id)).collect()
}

fn sg(ids: &[i64]) -> BTreeSet<SuggestionId> {
    ids.iter().map(|id| SuggestionId::new(*id)).collect()
}

fn suggestions(clients: &[(&str, usize)], ids: &[i64]) -> Arrival {
    Arrival::Suggestions(SuggestionArrival {
        per_client: clients
            .iter()
            .map(|(name, n)| (name.to_string(), *n))
            .collect::<BTreeMap<_, _>>(),
        new_ids: sg(ids),
    })
}

fn new_rows(ids: &[i64]) -> Arrival {
    Arrival::Ledger(LedgerArrival {
        new: tx(ids),
        ..LedgerArrival::default()
    })
}

fn changed_rows(ids: &[i64]) -> Arrival {
    Arrival::Ledger(LedgerArrival {
        changed: tx(ids),
        ..LedgerArrival::default()
    })
}

fn removed_rows(removed: usize, import_undone: bool) -> Arrival {
    Arrival::Ledger(LedgerArrival {
        removed,
        import_undone,
        ..LedgerArrival::default()
    })
}

fn line_text(arrivals: &Arrivals) -> String {
    arrivals.line().expect("a line").text()
}

fn secs(n: u64) -> Duration {
    Duration::from_secs(n)
}

fn copy_of(arrival: Arrival) -> ArrivalLine {
    let mut arrivals = Arrivals::new();
    arrivals.arrive(arrival, false, Instant::now());
    arrivals.line().expect("a line")
}

#[test]
fn one_client_names_it_in_emphasis() {
    let line = copy_of(suggestions(&[("Claude Code", 3)], &[1, 2, 3]));
    assert_eq!(line.text(), "3 suggestions from Claude Code");
    assert_eq!(line.lead, Lead::Sparkle);
    assert_eq!(line.opens, Some(Channel::Inbox));
    let emphasised: Vec<&str> = line
        .spans
        .iter()
        .filter(|span| span.emphasis)
        .map(|span| span.text.as_str())
        .collect();
    assert_eq!(emphasised, ["Claude Code"]);
}

#[test]
fn one_suggestion_is_singular() {
    let line = copy_of(suggestions(&[("Claude Code", 1)], &[1]));
    assert_eq!(line.text(), "1 suggestion from Claude Code");
}

#[test]
fn two_clients_are_counted_not_named() {
    let line = copy_of(suggestions(&[("Claude Code", 3), ("Codex", 2)], &[]));
    assert_eq!(line.text(), "5 suggestions from 2 AI clients");
    assert_eq!(line.lead, Lead::Sparkle);
    assert!(line.spans.iter().all(|span| !span.emphasis));
    assert_eq!(line.opens, Some(Channel::Inbox));
}

#[test]
fn long_client_names_are_cut_at_24_characters() {
    let name = "abcdefghijklmnopqrstuvwxyz";
    let line = copy_of(suggestions(&[(name, 2)], &[]));
    assert_eq!(line.text(), "2 suggestions from abcdefghijklmnopqrstuvwx…");
    let exact = "abcdefghijklmnopqrstuvwx";
    assert_eq!(exact.chars().count(), 24);
    let line = copy_of(suggestions(&[(exact, 2)], &[]));
    assert_eq!(line.text(), format!("2 suggestions from {exact}"));
}

#[test]
fn client_names_are_made_plain() {
    let line = copy_of(suggestions(&[("Evil\nname\u{200B}", 1)], &[]));
    assert_eq!(line.text(), "1 suggestion from Evil name");
}

#[test]
fn new_transactions_copy() {
    let line = copy_of(new_rows(&[1, 2, 3, 4, 5, 6, 7, 8, 9]));
    assert_eq!(line.text(), "9 new transactions");
    assert_eq!(line.lead, Lead::Dot);
    assert_eq!(line.opens, Some(Channel::Transactions));
    assert_eq!(copy_of(new_rows(&[1])).text(), "1 new transaction");
}

#[test]
fn changed_transactions_copy() {
    let line = copy_of(changed_rows(&(1..=12).collect::<Vec<_>>()));
    assert_eq!(line.text(), "12 transactions changed");
    assert_eq!(line.lead, Lead::Dot);
    assert_eq!(line.opens, Some(Channel::Transactions));
    assert_eq!(copy_of(changed_rows(&[7])).text(), "1 transaction changed");
}

#[test]
fn removed_transactions_copy() {
    let line = copy_of(removed_rows(9, true));
    assert_eq!(line.text(), "9 transactions removed · import undone");
    assert_eq!(line.lead, Lead::None);
    assert_eq!(line.opens, None);
    assert_eq!(
        copy_of(removed_rows(9, false)).text(),
        "9 transactions removed"
    );
    assert_eq!(
        copy_of(removed_rows(1, true)).text(),
        "1 transaction removed · import undone"
    );
}

#[test]
fn a_mixed_ledger_arrival_says_new_then_removed_then_changed() {
    let mixed = |new: &[i64], changed: &[i64], removed: usize| {
        Arrival::Ledger(LedgerArrival {
            new: tx(new),
            changed: tx(changed),
            removed,
            import_undone: true,
        })
    };
    assert_eq!(
        copy_of(mixed(&[1, 2], &[3], 4)).text(),
        "2 new transactions"
    );
    assert_eq!(
        copy_of(mixed(&[], &[3], 4)).text(),
        "4 transactions removed · import undone"
    );
    assert_eq!(copy_of(mixed(&[], &[3], 0)).text(), "1 transaction changed");

    let mut arrivals = Arrivals::new();
    arrivals.arrive(mixed(&[1], &[2], 3), false, Instant::now());
    let marks = arrivals.transaction_marks().expect("marks");
    assert_eq!(marks.ids, &tx(&[1, 2]));
}

#[test]
fn an_empty_arrival_is_ignored() {
    let mut arrivals = Arrivals::new();
    let now = Instant::now();
    arrivals.arrive(Arrival::Ledger(LedgerArrival::default()), false, now);
    arrivals.arrive(suggestions(&[("Claude Code", 0)], &[]), false, now);
    assert_eq!(arrivals.line(), None);
    assert_eq!(arrivals.inbox_badge_unseen(), None);
    assert_eq!(arrivals.next_deadline(), None);
}

#[test]
fn suggestions_from_the_same_client_add_while_unseen() {
    let mut arrivals = Arrivals::new();
    let now = Instant::now();
    arrivals.arrive(suggestions(&[("Claude Code", 3)], &[1, 2, 3]), false, now);
    arrivals.arrive(suggestions(&[("Claude Code", 2)], &[4, 5]), false, now);
    assert_eq!(line_text(&arrivals), "5 suggestions from Claude Code");
    assert_eq!(
        arrivals.suggestion_marks().unwrap().ids,
        &sg(&[1, 2, 3, 4, 5])
    );
}

#[test]
fn new_transactions_add_while_unseen() {
    let mut arrivals = Arrivals::new();
    let now = Instant::now();
    arrivals.arrive(new_rows(&[1, 2, 3, 4, 5, 6, 7, 8, 9]), false, now);
    arrivals.arrive(new_rows(&[10, 11, 12]), false, now);
    assert_eq!(line_text(&arrivals), "12 new transactions");
    assert_eq!(arrivals.transaction_marks().unwrap().ids.len(), 12);
}

#[test]
fn changed_and_removed_transactions_add_while_unseen() {
    let mut arrivals = Arrivals::new();
    let now = Instant::now();
    arrivals.arrive(changed_rows(&[1, 2]), false, now);
    arrivals.arrive(changed_rows(&[3]), false, now);
    assert_eq!(line_text(&arrivals), "3 transactions changed");

    let mut arrivals = Arrivals::new();
    arrivals.arrive(removed_rows(2, false), false, now);
    arrivals.arrive(removed_rows(3, true), false, now);
    assert_eq!(
        line_text(&arrivals),
        "5 transactions removed · import undone"
    );
}

#[test]
fn the_same_clients_add_per_client() {
    let mut arrivals = Arrivals::new();
    let now = Instant::now();
    arrivals.arrive(
        suggestions(&[("Claude Code", 3), ("Codex", 1)], &[]),
        false,
        now,
    );
    arrivals.arrive(
        suggestions(&[("Claude Code", 1), ("Codex", 1)], &[]),
        false,
        now,
    );
    assert_eq!(line_text(&arrivals), "6 suggestions from 2 AI clients");
}

#[test]
fn a_different_client_replaces_the_line_but_the_badge_and_marks_stay() {
    let mut arrivals = Arrivals::new();
    let now = Instant::now();
    arrivals.arrive(suggestions(&[("Claude Code", 3)], &[1, 2, 3]), false, now);
    arrivals.arrive(suggestions(&[("Codex", 1)], &[4]), false, now);
    assert_eq!(line_text(&arrivals), "1 suggestion from Codex");
    assert_eq!(arrivals.suggestion_marks().unwrap().ids, &sg(&[1, 2, 3, 4]));
    assert_eq!(arrivals.inbox_badge_unseen(), Some(Phase::Shown));
}

#[test]
fn a_different_kind_replaces_the_line_but_earlier_marks_stay() {
    let mut arrivals = Arrivals::new();
    let now = Instant::now();
    arrivals.arrive(suggestions(&[("Claude Code", 3)], &[1, 2, 3]), false, now);
    arrivals.arrive(new_rows(&[10, 11]), false, now);
    assert_eq!(line_text(&arrivals), "2 new transactions");
    assert_eq!(arrivals.line().unwrap().opens, Some(Channel::Transactions));
    assert_eq!(arrivals.inbox_badge_unseen(), Some(Phase::Shown));
    assert_eq!(arrivals.suggestion_marks().unwrap().ids, &sg(&[1, 2, 3]));
    assert_eq!(arrivals.transaction_marks().unwrap().ids, &tx(&[10, 11]));

    // New then changed: replaced, not added, and the marks are the union.
    arrivals.arrive(changed_rows(&[10, 20]), false, now);
    assert_eq!(line_text(&arrivals), "2 transactions changed");
    assert_eq!(
        arrivals.transaction_marks().unwrap().ids,
        &tx(&[10, 11, 20])
    );
}

#[test]
fn seen_holds_ten_seconds_then_fades_then_is_gone() {
    let mut arrivals = Arrivals::new();
    let t0 = Instant::now();
    arrivals.arrive(new_rows(&[1, 2, 3]), false, t0);
    assert_eq!(arrivals.next_deadline(), None);

    let seen = t0 + secs(30);
    arrivals.seen(Channel::Transactions, seen);
    assert_eq!(arrivals.next_deadline(), Some(seen + HOLD));
    assert_eq!(arrivals.line().unwrap().phase, Phase::Shown);

    // Just before the hold ends nothing changes.
    assert!(!arrivals.tick(seen + HOLD - Duration::from_millis(1)));
    assert_eq!(arrivals.transaction_marks().unwrap().phase, Phase::Shown);

    // The hold ends: everything fades together.
    let fade_from = seen + HOLD;
    assert!(arrivals.tick(fade_from));
    let fading = Phase::Fading { since: fade_from };
    assert_eq!(arrivals.line().unwrap().phase, fading);
    assert_eq!(arrivals.transaction_marks().unwrap().phase, fading);
    assert_eq!(arrivals.next_deadline(), Some(fade_from + FADE_OUT));

    // Mid-fade nothing changes; at the end it is gone.
    assert!(!arrivals.tick(fade_from + FADE_OUT / 2));
    assert!(arrivals.tick(fade_from + FADE_OUT));
    assert_eq!(arrivals.line(), None);
    assert_eq!(arrivals.transaction_marks(), None);
    assert_eq!(arrivals.next_deadline(), None);
    assert!(!arrivals.tick(fade_from + FADE_OUT * 2));
}

#[test]
fn a_late_tick_fades_from_the_end_of_the_hold() {
    let mut arrivals = Arrivals::new();
    let t0 = Instant::now();
    arrivals.arrive(new_rows(&[1]), true, t0);
    assert!(arrivals.tick(t0 + HOLD + FADE_OUT / 2));
    assert_eq!(
        arrivals.line().unwrap().phase,
        Phase::Fading { since: t0 + HOLD }
    );
    assert!(arrivals.tick(t0 + secs(60)));
    assert_eq!(arrivals.line(), None);
}

#[test]
fn unseen_never_times_out() {
    let mut arrivals = Arrivals::new();
    let t0 = Instant::now();
    arrivals.arrive(suggestions(&[("Claude Code", 3)], &[1, 2, 3]), false, t0);
    arrivals.arrive(new_rows(&[1]), false, t0);
    let later = t0 + secs(3600);
    assert!(!arrivals.tick(later));
    assert_eq!(arrivals.line().unwrap().phase, Phase::Shown);
    assert_eq!(arrivals.inbox_badge_unseen(), Some(Phase::Shown));
    assert!(arrivals.suggestion_marks().is_some());
    assert!(arrivals.transaction_marks().is_some());
    assert_eq!(arrivals.next_deadline(), None);
}

#[test]
fn seen_twice_does_not_restart_the_hold() {
    let mut arrivals = Arrivals::new();
    let t0 = Instant::now();
    arrivals.arrive(new_rows(&[1]), false, t0);
    arrivals.seen(Channel::Transactions, t0 + secs(1));
    arrivals.seen(Channel::Transactions, t0 + secs(5));
    arrivals.clicked(t0 + secs(6));
    assert_eq!(arrivals.next_deadline(), Some(t0 + secs(1) + HOLD));
}

#[test]
fn seen_through_one_channel_leaves_the_other_alone() {
    let mut arrivals = Arrivals::new();
    let t0 = Instant::now();
    arrivals.arrive(suggestions(&[("Claude Code", 2)], &[1, 2]), false, t0);
    arrivals.arrive(new_rows(&[10]), false, t0);
    // The line is the ledger's; seeing the Inbox starts the Inbox's hold.
    arrivals.seen(Channel::Inbox, t0 + secs(1));
    assert_eq!(arrivals.next_deadline(), Some(t0 + secs(1) + HOLD));
    assert!(arrivals.tick(t0 + secs(1) + HOLD));
    assert_eq!(
        arrivals.inbox_badge_unseen(),
        Some(Phase::Fading {
            since: t0 + secs(1) + HOLD
        })
    );
    assert_eq!(arrivals.line().unwrap().phase, Phase::Shown);
    assert_eq!(arrivals.transaction_marks().unwrap().phase, Phase::Shown);
    assert!(arrivals.tick(t0 + secs(1) + HOLD + FADE_OUT));
    assert_eq!(arrivals.inbox_badge_unseen(), None);
    assert_eq!(arrivals.suggestion_marks(), None);
    assert!(arrivals.line().is_some());
}

#[test]
fn seen_on_arrival_never_makes_the_badge_unseen() {
    let mut arrivals = Arrivals::new();
    let t0 = Instant::now();
    arrivals.arrive(suggestions(&[("Claude Code", 3)], &[1, 2, 3]), true, t0);
    assert_eq!(arrivals.inbox_badge_unseen(), None);
    // The marks and the line still show, and hold from now.
    assert_eq!(arrivals.suggestion_marks().unwrap().ids.len(), 3);
    assert_eq!(arrivals.line().unwrap().phase, Phase::Shown);
    assert_eq!(arrivals.next_deadline(), Some(t0 + HOLD));
    assert!(arrivals.tick(t0 + HOLD + FADE_OUT));
    assert_eq!(arrivals.line(), None);
    assert_eq!(arrivals.suggestion_marks(), None);
}

#[test]
fn an_unseen_badge_fades_with_the_marks() {
    let mut arrivals = Arrivals::new();
    let t0 = Instant::now();
    arrivals.arrive(suggestions(&[("Claude Code", 3)], &[]), false, t0);
    // Empty marks: only the badge and the line.
    assert_eq!(arrivals.suggestion_marks(), None);
    assert_eq!(arrivals.inbox_badge_unseen(), Some(Phase::Shown));
    arrivals.seen(Channel::Inbox, t0);
    assert_eq!(arrivals.next_deadline(), Some(t0 + HOLD));
    arrivals.tick(t0 + HOLD);
    assert_eq!(
        arrivals.inbox_badge_unseen(),
        Some(Phase::Fading { since: t0 + HOLD })
    );
    arrivals.tick(t0 + HOLD + FADE_OUT);
    assert_eq!(arrivals.inbox_badge_unseen(), None);
    assert_eq!(arrivals.line(), None);
}

#[test]
fn a_click_sees_the_lines_channel() {
    let mut arrivals = Arrivals::new();
    let t0 = Instant::now();
    arrivals.arrive(suggestions(&[("Claude Code", 2)], &[1, 2]), false, t0);
    arrivals.arrive(new_rows(&[10]), false, t0);
    arrivals.clicked(t0 + secs(2));
    // The line is Transactions': its hold started, the Inbox's did not.
    assert_eq!(arrivals.next_deadline(), Some(t0 + secs(2) + HOLD));
    assert!(arrivals.tick(t0 + secs(2) + HOLD + FADE_OUT));
    assert_eq!(arrivals.line(), None);
    assert_eq!(arrivals.transaction_marks(), None);
    assert_eq!(arrivals.inbox_badge_unseen(), Some(Phase::Shown));
    assert!(arrivals.suggestion_marks().is_some());
}

#[test]
fn a_click_on_nothing_does_nothing() {
    let mut arrivals = Arrivals::new();
    arrivals.clicked(Instant::now());
    assert_eq!(arrivals.next_deadline(), None);
}

#[test]
fn the_removed_line_is_seen_through_transactions_and_has_no_marks() {
    let mut arrivals = Arrivals::new();
    let t0 = Instant::now();
    arrivals.arrive(removed_rows(9, true), false, t0);
    assert_eq!(arrivals.transaction_marks(), None);
    arrivals.seen(Channel::Inbox, t0);
    assert_eq!(arrivals.next_deadline(), None);
    arrivals.seen(Channel::Transactions, t0 + secs(1));
    assert_eq!(arrivals.next_deadline(), Some(t0 + secs(1) + HOLD));
    arrivals.tick(t0 + secs(1) + HOLD + FADE_OUT);
    assert_eq!(arrivals.line(), None);
}

#[test]
fn a_seen_line_is_replaced_not_added_to() {
    let mut arrivals = Arrivals::new();
    let t0 = Instant::now();
    arrivals.arrive(new_rows(&[1, 2, 3]), true, t0);
    arrivals.arrive(new_rows(&[4, 5]), true, t0 + secs(3));
    assert_eq!(line_text(&arrivals), "2 new transactions");
    // The marks are the union, and the hold restarted.
    assert_eq!(arrivals.transaction_marks().unwrap().ids.len(), 5);
    assert_eq!(arrivals.next_deadline(), Some(t0 + secs(3) + HOLD));
}

#[test]
fn an_unseen_arrival_after_a_seen_one_waits_for_its_own_seeing() {
    let mut arrivals = Arrivals::new();
    let t0 = Instant::now();
    arrivals.arrive(new_rows(&[1]), true, t0);
    arrivals.arrive(new_rows(&[2]), false, t0 + secs(2));
    assert_eq!(arrivals.next_deadline(), None);
    assert!(!arrivals.tick(t0 + secs(100)));
    assert_eq!(arrivals.transaction_marks().unwrap().ids, &tx(&[1, 2]));
}

#[test]
fn next_deadline_is_the_earliest_of_the_items() {
    let mut arrivals = Arrivals::new();
    let t0 = Instant::now();
    arrivals.arrive(suggestions(&[("Claude Code", 2)], &[1, 2]), false, t0);
    arrivals.seen(Channel::Inbox, t0);
    arrivals.arrive(new_rows(&[10]), false, t0 + secs(4));
    arrivals.seen(Channel::Transactions, t0 + secs(5));
    assert_eq!(arrivals.next_deadline(), Some(t0 + HOLD));
    arrivals.tick(t0 + HOLD);
    assert_eq!(arrivals.next_deadline(), Some(t0 + HOLD + FADE_OUT));
    arrivals.tick(t0 + HOLD + FADE_OUT);
    assert_eq!(arrivals.next_deadline(), Some(t0 + secs(5) + HOLD));
}
