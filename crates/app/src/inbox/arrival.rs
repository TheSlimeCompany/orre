//! What a read of the suggestions brought (ROADMAP LU.5, `.scratch/lu5/spec.md`):
//! a plain function over the Inbox's data before and after a read, so the
//! diff is testable without a window. [`Inbox`](super::Inbox) emits the
//! result as [`Arrived`] after a read that replaced loaded data.

use std::collections::{BTreeMap, BTreeSet};

use orre_core::inbox::{ORRE_SOURCE, SuggestionId, SuggestionStatus};

use super::store::InboxData;
use super::text::source_name;

/// The pending suggestions a read brought.
///
/// Orre's own ([`ORRE_SOURCE`]) never count: they come from the user's own
/// edits. A suggestion decided elsewhere (a count that dropped) isn't an
/// arrival.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Arrival {
    /// How many arrived from each client, by the name the Inbox shows
    /// ([`source_name`]: sanitised, since it's untrusted; the status line
    /// cuts it at 24 characters).
    pub by_client: BTreeMap<String, usize>,
    /// The ids of the new pending suggestions the read lists, for marking
    /// their rows. Empty when the read didn't list the pending ones.
    pub ids: BTreeSet<SuggestionId>,
}

impl Arrival {
    /// Whether nothing arrived.
    pub fn is_empty(&self) -> bool {
        self.by_client.is_empty() && self.ids.is_empty()
    }

    /// How many arrived, from every client.
    pub fn total(&self) -> usize {
        self.by_client.values().sum()
    }
}

/// What the Inbox view reports after a read that replaced already-loaded
/// data and brought something. The view only reports: the shell decides
/// whether the read was unasked (the app's own decisions show an undo bar
/// and aren't news).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Arrived(pub Arrival);

/// What `after` has that `before` didn't.
///
/// - A `before` that holds no read yet (the first load) reports nothing.
/// - When both reads hold the Pending list, the arrivals are the ids in
///   `after` and not in `before`, counted per client: exact, so a
///   suggestion filed while another was decided elsewhere still counts.
/// - Otherwise the counts decide: per client, how many more are pending
///   ([`orre_core::inbox::Counts::by_source`]). The ids, if `after` lists
///   the pending ones, are the newest above `newest_seen` (the highest id
///   the view has listed; ids only grow, `AUTOINCREMENT`), as many as
///   arrived.
pub fn arrived(
    before: &InboxData,
    after: &InboxData,
    newest_seen: Option<SuggestionId>,
) -> Arrival {
    if before.status.is_none() {
        return Arrival::default();
    }
    let pending = Some(SuggestionStatus::Pending);
    if before.status == pending && after.status == pending {
        let known: BTreeSet<SuggestionId> = before.entries.iter().map(|entry| entry.id()).collect();
        let mut arrival = Arrival::default();
        for entry in &after.entries {
            if known.contains(&entry.id()) || entry.suggestion.source_client == ORRE_SOURCE {
                continue;
            }
            arrival.ids.insert(entry.id());
            *arrival
                .by_client
                .entry(source_name(&entry.suggestion.source_client).0)
                .or_default() += 1;
        }
        return arrival;
    }

    let mut arrival = Arrival::default();
    for (source, now) in &after.counts.by_source {
        if source == ORRE_SOURCE {
            continue;
        }
        let was = before.counts.by_source.get(source).copied().unwrap_or(0);
        if *now > was {
            *arrival.by_client.entry(source_name(source).0).or_default() += now - was;
        }
    }
    if after.status == pending {
        let mut newer: Vec<SuggestionId> = after
            .entries
            .iter()
            .filter(|entry| entry.suggestion.source_client != ORRE_SOURCE)
            .map(|entry| entry.id())
            .filter(|id| newest_seen.is_none_or(|seen| *id > seen))
            .collect();
        newer.sort();
        let skip = newer.len().saturating_sub(arrival.total());
        arrival.ids = newer.into_iter().skip(skip).collect();
    }
    arrival
}

#[cfg(test)]
mod tests {
    use orre_core::inbox::preview::Subject;
    use orre_core::inbox::{Counts, Payload, Suggestion, SuggestionKind};

    use super::*;
    use crate::inbox::store::Entry;

    fn id(n: i64) -> SuggestionId {
        SuggestionId::new(n)
    }

    fn entry(n: i64, source: &str, status: SuggestionStatus) -> Entry {
        Entry {
            suggestion: Suggestion {
                id: id(n),
                kind: SuggestionKind::Categorization,
                payload: Payload::Categorization(Vec::new()),
                rationale: None,
                source_client: source.to_owned(),
                created_at: String::new(),
                status,
                decided_at: None,
                decision_reason: None,
                change_batch: None,
                applied: None,
            },
            subject: Subject::Categorization(Vec::new()),
            preview: None,
            refused: None,
        }
    }

    /// A read of the Pending list: `(id, source)` for each suggestion.
    fn pending(listed: &[(i64, &str)]) -> InboxData {
        let mut counts = Counts::default();
        for (_, source) in listed {
            counts.pending += 1;
            *counts.by_source.entry((*source).to_owned()).or_default() += 1;
        }
        InboxData {
            counts,
            status: Some(SuggestionStatus::Pending),
            entries: listed
                .iter()
                .map(|(n, source)| entry(*n, source, SuggestionStatus::Pending))
                .collect(),
            ..InboxData::default()
        }
    }

    /// A read of the Approved list (one approved suggestion, id 1), while
    /// `still_pending` are pending.
    fn approved(still_pending: &[(i64, &str)]) -> InboxData {
        InboxData {
            status: Some(SuggestionStatus::Approved),
            entries: vec![entry(1, "old", SuggestionStatus::Approved)],
            ..pending(still_pending)
        }
    }

    fn names(arrival: &Arrival) -> Vec<(&str, usize)> {
        arrival
            .by_client
            .iter()
            .map(|(name, count)| (name.as_str(), *count))
            .collect()
    }

    #[test]
    fn one_client_arrived() {
        let before = pending(&[(1, "claude-code")]);
        let after = pending(&[(1, "claude-code"), (2, "claude-code"), (3, "claude-code")]);
        let arrival = arrived(&before, &after, Some(id(1)));
        assert_eq!(names(&arrival), [("claude-code", 2)]);
        assert_eq!(arrival.ids, BTreeSet::from([id(2), id(3)]));
    }

    #[test]
    fn two_clients_arrived() {
        let before = pending(&[(1, "claude-code")]);
        let after = pending(&[(1, "claude-code"), (2, "codex"), (3, "claude-code")]);
        let arrival = arrived(&before, &after, Some(id(1)));
        assert_eq!(names(&arrival), [("claude-code", 1), ("codex", 1)]);
        assert_eq!(arrival.total(), 2);
    }

    #[test]
    fn orres_own_are_left_out() {
        let before = pending(&[(1, "claude-code")]);
        let after = pending(&[(1, "claude-code"), (2, ORRE_SOURCE)]);
        assert!(arrived(&before, &after, Some(id(1))).is_empty());

        // Through the counts too.
        let before = approved(&[(5, "claude-code")]);
        let after = approved(&[(5, "claude-code"), (6, ORRE_SOURCE)]);
        assert!(arrived(&before, &after, Some(id(5))).is_empty());
    }

    #[test]
    fn a_drop_is_not_an_arrival() {
        let before = pending(&[(1, "claude-code"), (2, "codex")]);
        let after = pending(&[(1, "claude-code")]);
        assert!(arrived(&before, &after, Some(id(2))).is_empty());
    }

    #[test]
    fn one_filed_while_another_was_decided_elsewhere_still_arrived() {
        let before = pending(&[(1, "claude-code"), (2, "claude-code")]);
        let after = pending(&[(1, "claude-code"), (3, "claude-code")]);
        let arrival = arrived(&before, &after, Some(id(2)));
        assert_eq!(names(&arrival), [("claude-code", 1)]);
        assert_eq!(arrival.ids, BTreeSet::from([id(3)]));
    }

    #[test]
    fn the_first_load_reports_nothing() {
        let after = pending(&[(1, "claude-code"), (2, "codex")]);
        assert!(arrived(&InboxData::default(), &after, None).is_empty());
    }

    #[test]
    fn ids_after_another_status_are_those_newer_than_the_newest_seen() {
        // The Approved list was on show (its newest id is 4); two were
        // filed since.
        let before = approved(&[(3, "claude-code")]);
        let after = pending(&[(3, "claude-code"), (5, "claude-code"), (6, "codex")]);
        let arrival = arrived(&before, &after, Some(id(4)));
        assert_eq!(names(&arrival), [("claude-code", 1), ("codex", 1)]);
        assert_eq!(arrival.ids, BTreeSet::from([id(5), id(6)]));
    }

    #[test]
    fn ids_are_no_more_than_the_count_says_arrived() {
        // Pending 8 and 9 were there before the Approved list was opened;
        // only one more is pending now, so only the newest counts.
        let before = approved(&[(8, "claude-code"), (9, "claude-code")]);
        let after = pending(&[(8, "claude-code"), (9, "claude-code"), (10, "claude-code")]);
        let arrival = arrived(&before, &after, Some(id(2)));
        assert_eq!(names(&arrival), [("claude-code", 1)]);
        assert_eq!(arrival.ids, BTreeSet::from([id(10)]));
    }

    #[test]
    fn a_read_that_lists_no_pending_ones_still_counts_them() {
        let before = pending(&[(1, "claude-code")]);
        let after = approved(&[(1, "claude-code"), (2, "claude-code")]);
        let arrival = arrived(&before, &after, Some(id(1)));
        assert_eq!(names(&arrival), [("claude-code", 1)]);
        assert!(arrival.ids.is_empty());
    }
}
