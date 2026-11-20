//! Tests of the Rules view's state ([`super::state`]) through [`Rules`]'
//! public methods, with no window: each section keeps its own selection,
//! leaving Rules drops a draft, the filters switch sections as `show` does,
//! a delete selects a neighbour and the panel's counts are the table's. The
//! database is the scratch one from [`super::tests`].

use gpui_kit::{AppContext as _, Entity, TestAppContext};

use super::tests::Scratch;
use super::*;

fn open(cx: &mut TestAppContext, scratch: &Scratch) -> Entity<Rules> {
    let store = RulesStore::new(scratch.path.clone());
    cx.new(|_| Rules::from_store(store))
}

/// The ID of the name in row `row` of the unfiltered table.
fn name_at(rules: &Rules, row: usize) -> MerchantAliasId {
    rules.names()[row].alias.id()
}

fn batch_at(rules: &Rules, row: usize) -> ChangeBatchId {
    rules.history()[row].batch.id
}

#[gpui_kit::test]
fn each_section_keeps_its_selection_while_another_is_open(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let rules = open(cx, &scratch);
    let third = scratch.seeded.rules[3];
    rules.update(cx, |rules, cx| {
        assert_eq!(rules.selected_rule_id(), Some(scratch.seeded.rules[0]));
        rules.select_rule(Some(third), cx);

        rules.show(Section::Names, cx);
        let second = name_at(rules, 1);
        rules.select_name(Some(NameSelection::Stored(second)), cx);

        rules.show(Section::History, cx);
        let batch = batch_at(rules, 1);
        rules.select_batch(Some(batch), cx);

        rules.show(Section::Categories, cx);
        let category = rules.category_rows()[2].summary.category.id();
        rules.select_category(Some(CategorySelection::Stored(category)), cx);

        rules.show(Section::Rules, cx);
        assert_eq!(rules.selected_rule_id(), Some(third));
        assert_eq!(rules.form_key(), FormKey::Rule(third));
        rules.show(Section::Names, cx);
        assert_eq!(
            rules.selected_name(),
            Some(NameSelection::Stored(name_at(rules, 1)))
        );
        assert!(rules.name_form().is_some());
        rules.show(Section::History, cx);
        assert_eq!(rules.selected_batch().map(|e| e.batch.id), Some(batch));
        rules.show(Section::Categories, cx);
        assert_eq!(
            rules.selected_category(),
            Some(CategorySelection::Stored(category))
        );
        assert!(rules.category_form().is_some());
    });
}

#[gpui_kit::test]
fn leaving_rules_drops_a_new_rule_and_coming_back_selects_the_first(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let rules = open(cx, &scratch);
    rules.update(cx, |rules, cx| {
        rules.new_rule(cx);
        assert!(rules.is_new());
        assert_eq!(rules.form_key(), FormKey::NewRule);
        assert!(rules.apply().is_some());

        rules.show(Section::Names, cx);
        assert!(!rules.is_new());
        assert!(rules.form().is_none());
        assert!(rules.apply().is_none());
        assert_eq!(rules.selected_rule_id(), None);

        rules.show(Section::Rules, cx);
        assert!(!rules.is_new());
        assert_eq!(rules.selected_rule_id(), Some(scratch.seeded.rules[0]));
        assert_eq!(rules.form_key(), FormKey::Rule(scratch.seeded.rules[0]));
        assert!(rules.form().is_some());
        assert!(rules.apply().is_none());
        assert_eq!(rules.new_position(), 0);
    });
}

#[gpui_kit::test]
fn leaving_rules_closes_a_stored_rules_preview_and_keeps_its_selection(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let rules = open(cx, &scratch);
    let dishoom = scratch.seeded.rules[12];
    rules.update(cx, |rules, cx| {
        rules.select_rule(Some(dishoom), cx);
        rules.apply_selected(cx);
        assert!(rules.apply().is_some());
        assert!(!rules.is_new());

        rules.show(Section::History, cx);
        rules.show(Section::Rules, cx);
        assert!(rules.apply().is_none());
        assert_eq!(rules.selected_rule_id(), Some(dishoom));
        assert!(rules.form().is_some());
    });
}

#[gpui_kit::test]
fn a_filter_toggle_from_another_section_switches_as_show_does(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let rules = open(cx, &scratch);
    let dishoom = scratch.seeded.rules[12];
    rules.update(cx, |rules, cx| {
        rules.select_rule(Some(dishoom), cx);
        rules.apply_selected(cx);
        rules.outcome = Some(Outcome::Refused("Old news.".into()));

        let kind = text::ALIAS_KINDS[0];
        rules.toggle_match_filter(kind, cx);
        assert_eq!(rules.section(), Section::Names);
        assert_eq!(rules.match_filter(), Some(kind));
        assert!(rules.outcome().is_none(), "an actual switch clears it");
        assert!(rules.apply().is_none(), "the preview closed");
        assert!(
            rules.selected_name().is_some() || rules.names().is_empty(),
            "the default row is selected"
        );

        // Toggling again, in the same section, only clears the filter.
        rules.outcome = Some(Outcome::Refused("Kept.".into()));
        rules.toggle_match_filter(kind, cx);
        assert_eq!(rules.match_filter(), None);
        assert!(rules.outcome().is_some());

        rules.toggle_action_filter(text::PANEL_ACTIONS[0], cx);
        assert_eq!(rules.section(), Section::Rules);
        assert_eq!(rules.selected_rule_id(), Some(dishoom));
        assert!(rules.apply().is_none());
        assert!(rules.outcome().is_none());
    });
}

#[gpui_kit::test]
fn a_filter_toggle_leaving_rules_drops_a_new_rule(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let rules = open(cx, &scratch);
    rules.update(cx, |rules, cx| {
        rules.new_rule(cx);
        rules.toggle_kind_filter(text::CHANGE_KINDS[0], cx);
        assert_eq!(rules.section(), Section::History);
        assert!(!rules.is_new());
        rules.show(Section::Rules, cx);
        assert_eq!(rules.selected_rule_id(), Some(scratch.seeded.rules[0]));
    });
}

#[gpui_kit::test]
fn a_filter_toggle_selects_the_first_row_the_filter_keeps(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let rules = open(cx, &scratch);
    rules.update(cx, |rules, cx| {
        // A kind whose first change isn't the first change in the history.
        let kind = text::CHANGE_KINDS
            .iter()
            .copied()
            .find(|kind| rules.count_history(*kind) > 0 && rules.history()[0].batch.kind != *kind)
            .expect("a kind that isn't the newest change's");
        rules.show(Section::Names, cx);

        rules.toggle_kind_filter(kind, cx);

        let first_kept = rules.history()[0].batch.id;
        assert_eq!(rules.selected_batch().map(|e| e.batch.kind), Some(kind));
        assert_eq!(rules.selected_batch().map(|e| e.batch.id), Some(first_kept));
    });
}

#[gpui_kit::test]
fn deleting_a_name_selects_the_row_after_it_else_the_one_before(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let rules = open(cx, &scratch);
    rules.update(cx, |rules, cx| {
        rules.show(Section::Names, cx);
        let order: Vec<MerchantAliasId> = rules.names().iter().map(|n| n.alias.id()).collect();
        assert!(order.len() > 3);

        rules.select_name(Some(NameSelection::Stored(order[1])), cx);
        rules.delete_name(order[1], cx).unwrap();
        assert_eq!(rules.selected_name(), Some(NameSelection::Stored(order[2])));
        assert!(rules.name_form().is_some());

        let last = *order.last().unwrap();
        let before_last = order[order.len() - 2];
        rules.select_name(Some(NameSelection::Stored(last)), cx);
        rules.delete_name(last, cx).unwrap();
        assert_eq!(
            rules.selected_name(),
            Some(NameSelection::Stored(before_last))
        );
    });
}

#[gpui_kit::test]
fn a_deleted_rule_or_category_leaves_before_the_read_lands(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let seeded = fixture::seed_categories(&mut scratch.db());
    let rules = open(cx, &scratch);
    let [first, second, third] = [0, 1, 2].map(|n| scratch.seeded.rules[n]);
    rules.update(cx, |rules, cx| {
        rules.delete_rule(first, cx).unwrap();
        assert!(rules.loading.is_some(), "the read hasn't landed");
        assert!(rules.data().rule(first).is_none());
        assert_eq!(rules.selected_rule_id(), Some(second));
        rules.delete_rule(second, cx).unwrap();
        assert_eq!(rules.selected_rule_id(), Some(third));
        assert_eq!(rules.rules().len(), 12);

        // Coffee is unused, so it can go; the row after it is selected.
        rules.select_category(Some(CategorySelection::Stored(seeded.coffee)), cx);
        let order: Vec<i64> = rules
            .category_rows()
            .iter()
            .map(|row| row.summary.category.id())
            .collect();
        let at = order.iter().position(|id| *id == seeded.coffee).unwrap();
        rules.delete_category(seeded.coffee, cx).unwrap();
        assert!(rules.data().category_entry(seeded.coffee).is_none());
        assert!(rules.data().categories.iter().all(|c| c.label != "Coffee"));
        assert_eq!(
            rules.selected_category(),
            Some(CategorySelection::Stored(order[at + 1]))
        );
    });
    cx.run_until_parked();
    rules.read_with(cx, |rules, _| {
        assert_eq!(rules.rules().len(), 12);
        assert_eq!(rules.selected_rule_id(), Some(third));
        assert!(rules.data().category_entry(seeded.coffee).is_none());
    });
}

#[gpui_kit::test]
fn deleting_the_only_row_left_selects_nothing(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let rules = open(cx, &scratch);
    rules.update(cx, |rules, cx| {
        rules.show(Section::Names, cx);
        while let Some(last) = rules.names().last().map(|n| n.alias.id()) {
            rules.delete_name(last, cx).unwrap();
        }
        assert_eq!(rules.selected_name(), None);
        assert!(rules.name_form().is_none());
        assert_eq!(rules.form_key(), FormKey::None);
    });
}

#[gpui_kit::test]
fn the_panel_counts_are_what_the_tables_list(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let rules = open(cx, &scratch);
    rules.update(cx, |rules, cx| {
        for kind in text::ALIAS_KINDS {
            rules.toggle_match_filter(kind, cx);
            assert_eq!(rules.names().len(), rules.count_names(kind), "{kind:?}");
            rules.toggle_match_filter(kind, cx);
        }
        for kind in text::CHANGE_KINDS {
            rules.toggle_kind_filter(kind, cx);
            assert_eq!(rules.history().len(), rules.count_history(kind), "{kind:?}");
            rules.toggle_kind_filter(kind, cx);
        }
        for kind in text::PANEL_ACTIONS {
            rules.toggle_action_filter(kind, cx);
            assert_eq!(
                rules.rules().len(),
                rules.count_with_action(kind),
                "{kind:?}"
            );
            rules.toggle_action_filter(kind, cx);
        }
        let total: usize = text::ALIAS_KINDS
            .iter()
            .map(|k| rules.count_names(*k))
            .sum();
        assert_eq!(total, rules.data().names.len());
    });
}

#[gpui_kit::test]
fn escape_from_a_stored_rules_preview_returns_to_the_rule_with_its_form(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let rules = open(cx, &scratch);
    let dishoom = scratch.seeded.rules[12];
    rules.update(cx, |rules, cx| {
        rules.select_rule(Some(dishoom), cx);
        rules.apply_selected(cx);
        assert!(rules.apply().is_some());

        rules.cancel(cx);
        assert!(rules.apply().is_none());
        assert_eq!(rules.selected_rule_id(), Some(dishoom));
        assert!(rules.form().is_some());

        rules.cancel(cx);
        assert_eq!(rules.selected_rule_id(), None);
        assert!(rules.form().is_none());
    });
}

#[gpui_kit::test]
fn back_from_a_new_rule_drops_it_and_selects_the_first(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let rules = open(cx, &scratch);
    rules.update(cx, |rules, cx| {
        rules.select_rule(Some(scratch.seeded.rules[3]), cx);
        rules.new_rule(cx);
        assert!(rules.is_new());
        assert_eq!(rules.selected_rule_id(), None);

        rules.cancel(cx);
        assert!(!rules.is_new());
        assert_eq!(rules.selected_rule_id(), Some(scratch.seeded.rules[0]));
    });
}
