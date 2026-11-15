//! UI integration tests for the Rules view: the shell's Rules tab in a
//! headless window, against a scratch database seeded with [`fixture`]
//! through [`RulesStore`], as `coffer` runs it.

use coffer_core::change::{self as core_change, BatchStatus};
use coffer_core::db::Database;
use coffer_core::id::TransactionId;
use coffer_core::rule::{self as core_rule, MatchKind};
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{AppContext as _, Entity, TestAppContext, VisualTestContext};
use tempfile::TempDir;

use super::view::{Input, Menu};
use super::*;
use crate::accounts::{Accounts, fixture as accounts_fixture};
use crate::inbox::Inbox;
use crate::shell::{ShellView, View};
use crate::transactions::{self, store::TransactionStore};

struct Scratch {
    _dir: TempDir,
    path: std::path::PathBuf,
    seeded: fixture::Seeded,
}

impl Scratch {
    fn new() -> Self {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("coffer.db");
        let mut db = Database::open(&path).unwrap();
        let seeded = fixture::seed(&mut db);
        Self {
            _dir: dir,
            path,
            seeded,
        }
    }

    fn db(&self) -> Database {
        Database::open(&self.path).unwrap()
    }

    fn category(&self, id: TransactionId) -> Option<String> {
        coffer_core::reporting::register::Register::load(
            &self.db().reader().unwrap(),
            chrono::NaiveDate::from_ymd_opt(2026, 11, 14).unwrap(),
        )
        .unwrap()
        .rows()
        .iter()
        .find(|row| row.id == id)
        .and_then(|row| match row.category {
            coffer_core::reporting::register::RowCategory::Category(id) => Some(id),
            _ => None,
        })
        .and_then(|category| {
            self.db()
                .categories()
                .unwrap()
                .iter()
                .find(|c| c.id() == category)
                .map(|c| c.key().as_str().to_owned())
        })
    }
}

fn open<'a>(
    cx: &'a mut TestAppContext,
    scratch: &Scratch,
) -> (Entity<ShellView>, Entity<Rules>, &'a mut VisualTestContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::component::install(cx);
        crate::shell::init(cx);
    });
    let store = RulesStore::new(scratch.path.clone());
    let transactions = TransactionStore::new(scratch.path.clone());
    let (shell, cx) = cx.add_window_view(|window, cx| {
        let accounts = cx.new(|_| Accounts::fixture(accounts_fixture::accounts()));
        let inbox = cx.new(|_| Inbox::empty());
        let settings = cx.new(crate::settings::SettingsView::fixture);
        let mut shell = ShellView::with_all(
            View::Rules,
            accounts,
            transactions::Source::Database(transactions),
            inbox,
            settings,
            window,
            cx,
        );
        let rules = cx.new(|_| Rules::from_store(store));
        shell.install_rules(rules, window, cx);
        shell
    });
    let rules = cx.update(|_, cx| shell.read(cx).rules().clone());
    redraw(cx);
    (shell, rules, cx)
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

#[gpui_kit::test]
fn rules_list_in_order_with_what_they_decide(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (_, rules, cx) = open(cx, &scratch);
    let (ids, never, selected) = cx.update(|_, cx| {
        let rules = rules.read(cx);
        (
            rules
                .rules()
                .iter()
                .map(|s| s.rule.id())
                .collect::<Vec<_>>(),
            rules
                .rules()
                .iter()
                .filter(|s| s.never_reached())
                .map(|s| s.rule.id())
                .collect::<Vec<_>>(),
            rules.selected_rule_id(),
        )
    });
    assert_eq!(ids, scratch.seeded.rules);
    assert_eq!(never, [scratch.seeded.rules[2]], "UBER TRIP loses to UBER");
    assert_eq!(
        selected,
        Some(scratch.seeded.rules[0]),
        "the first rule opens"
    );
    assert!(shows(&format!("rule-{}", scratch.seeded.rules[1]), cx));

    // The ACTION filter narrows the table; a second click clears it.
    click("rules-action-add_tag", cx);
    let listed = cx.update(|_, cx| rules.read(cx).rules().len());
    assert_eq!(listed, 2, "WAITROSE and JUSTGIVING add tags");
    click("rules-action-add_tag", cx);
    let listed = cx.update(|_, cx| rules.read(cx).rules().len());
    assert_eq!(listed, 14);

    // Selecting the never-reached rule offers to move it above UBER.
    let trip = scratch.seeded.rules[2];
    click(&format!("rule-{trip}"), cx);
    assert!(shows("rules-move-above", cx));
    click("rules-move-above", cx);
    let order = cx.update(|_, cx| {
        rules
            .read(cx)
            .rules()
            .iter()
            .map(|s| s.rule.id())
            .collect::<Vec<_>>()
    });
    assert_eq!(order[1], trip, "moved above rule 2");
    let decides = cx.update(|_, cx| rules.read(cx).data().rule(trip).unwrap().decides);
    assert_eq!(decides, 2, "it decides its UBER TRIP rows now");
}

#[gpui_kit::test]
fn editing_a_rule_saves_without_touching_history(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, rules, cx) = open(cx, &scratch);
    let pret = scratch.seeded.rules[3];
    cx.update(|_, cx| rules.update(cx, |rules, cx| rules.select_rule(Some(pret), cx)));
    redraw(cx);
    let view = cx.update(|_, cx| shell.read(cx).rules_view().clone());

    // A keystroke changes the form; leaving the field (Enter) saves.
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.type_into(Input::Pattern, "PRET", window, cx)
        })
    });
    let stored = core_rule::get(&scratch.db(), pret).unwrap();
    assert_eq!(
        stored.definition().pattern().as_str(),
        "PRET A MANGER",
        "not yet"
    );
    cx.update(|_, cx| rules.update(cx, |rules, cx| rules.save_rule(cx)));
    let stored = core_rule::get(&scratch.db(), pret).unwrap();
    assert_eq!(stored.definition().pattern().as_str(), "PRET");

    // A pattern that can't be used isn't saved, and says why.
    cx.update(|_, cx| {
        rules.update(cx, |rules, cx| {
            rules.edit(
                true,
                |form| {
                    form.kind = MatchKind::Regex;
                    form.text = "^(PRET".into();
                },
                cx,
            )
        })
    });
    redraw(cx);
    let error = cx.update(|_, cx| rules.read(cx).form_error());
    assert!(matches!(error, Some(form::FormError::Pattern(_))));
    let stored = core_rule::get(&scratch.db(), pret).unwrap();
    assert_eq!(stored.definition().pattern().kind(), MatchKind::Contains);

    // Adding an action takes the menu; Split isn't offered with Category.
    cx.update(|_, cx| {
        rules.update(cx, |rules, cx| {
            rules.edit(false, |form| form.kind = MatchKind::Contains, cx)
        })
    });
    cx.update(|window, cx| view.update(cx, |view, cx| view.show_menu(Menu::AddAction, window, cx)));
    redraw(cx);
    let available = cx.update(|_, cx| {
        rules
            .read(cx)
            .form()
            .unwrap()
            .availability(coffer_core::rule::ActionKind::SplitTemplate)
    });
    assert_eq!(
        available,
        Some(form::Unavailable::NotWith(
            coffer_core::rule::ActionKind::SetCategory
        ))
    );
}

#[gpui_kit::test]
fn applying_a_rule_previews_then_changes_and_undoes(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (_, rules, cx) = open(cx, &scratch);
    let dishoom = scratch.seeded.rules[12];
    cx.update(|_, cx| rules.update(cx, |rules, cx| rules.select_rule(Some(dishoom), cx)));
    redraw(cx);
    click("rules-apply", cx);
    let (rows, changing) = cx.update(|_, cx| {
        let apply = rules.read(cx).apply().cloned().unwrap();
        (apply.rows.len(), apply.changing())
    });
    assert_eq!((rows, changing), (1, 1), "the one DISHOOM row");
    assert!(shows("rules-commit", cx));

    click("rules-commit", cx);
    let outcome = cx.update(|_, cx| rules.read(cx).outcome().cloned());
    let Some(Outcome::Done {
        message,
        undo: Some(batch),
    }) = outcome
    else {
        panic!("the apply says what changed, with Undo");
    };
    assert_eq!(message.as_ref(), "Changed 1 transaction to Eating out");
    let batch_info = core_change::get(&scratch.db(), batch).unwrap();
    assert_eq!(batch_info.rule, Some(dishoom));

    cx.update(|_, cx| rules.update(cx, |rules, cx| rules.undo(cx)));
    let batch_info = core_change::get(&scratch.db(), batch).unwrap();
    assert_eq!(batch_info.status, BatchStatus::Undone);
}

#[gpui_kit::test]
fn a_rule_from_an_edit_is_previewed_then_created_and_applied(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, rules, cx) = open(cx, &scratch);
    // Transactions proposes the rule after the VINTED edit (`3WT-0`).
    let transactions = cx.update(|_, cx| shell.read(cx).transactions().clone());
    cx.update(|_, cx| {
        transactions.update(cx, |view, cx| view.propose_rule(scratch.seeded.vinted, cx))
    });
    let db = TransactionStore::new(scratch.path.clone()).open().unwrap();
    let proposal = core_rule::propose(&db, scratch.seeded.vinted)
        .unwrap()
        .expect("VINTED proposes a rule");
    drop(db);
    assert_eq!(proposal.definition.pattern().as_str(), "VINTED");
    assert_eq!(proposal.matches, 6);
    assert_eq!(proposal.uncategorized, 4);
    assert_eq!(proposal.user_set, 2);

    // "Create rule…" opens Rules on the preview, nothing saved.
    cx.update(|_, cx| {
        rules.update(cx, |rules, cx| {
            rules.new_rule_from(&proposal.definition, core_rule::RuleOrigin::Edit, cx)
        })
    });
    redraw(cx);
    let (will, user_set, stored) = cx.update(|_, cx| {
        let rules = rules.read(cx);
        let apply = rules.apply().unwrap();
        (
            apply.will_change().count(),
            apply.user_set().count(),
            rules.data().rules.len(),
        )
    });
    assert_eq!((will, user_set, stored), (4, 2, 14));
    assert!(shows("rules-create-only", cx));

    // "Change these too" includes the two the user set.
    cx.update(|_, cx| rules.update(cx, |rules, cx| rules.set_include(true, cx)));
    click("rules-commit", cx);
    let (count, first) = cx.update(|_, cx| {
        let rules = rules.read(cx);
        (
            rules.data().rules.len(),
            rules.data().rules[0].rule.origin(),
        )
    });
    assert_eq!(count, 15);
    assert_eq!(
        first,
        Some(core_rule::RuleOrigin::Edit),
        "a new rule goes first"
    );
    assert_eq!(
        scratch.category(scratch.seeded.vinted).as_deref(),
        Some("shopping")
    );
}

#[gpui_kit::test]
fn names_and_history_open_with_their_inspectors(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (_, rules, cx) = open(cx, &scratch);
    click("rules-section-names", cx);
    let (section, selected, names) = cx.update(|_, cx| {
        let rules = rules.read(cx);
        (
            rules.section(),
            rules.selected_name(),
            rules.data().names.len(),
        )
    });
    assert_eq!(section, Section::Names);
    assert_eq!(names, 8);
    assert!(matches!(selected, Some(NameSelection::Stored(_))));
    let amazon = cx.update(|_, cx| rules.read(cx).data().names[0].clone());
    assert_eq!(
        amazon.alias.definition().name().as_str(),
        "Amazon Marketplace"
    );
    assert_eq!(amazon.matches, 3);
    assert!(shows("rules-delete-name", cx));

    // A new name applies at once once both fields are filled.
    click("rules-new-name", cx);
    cx.update(|_, cx| {
        rules.update(cx, |rules, cx| {
            rules.edit_name(
                true,
                |form| {
                    form.pattern = "VINTED".into();
                    form.name = "Vinted".into();
                },
                cx,
            )
        })
    });
    let names = cx.update(|_, cx| rules.read(cx).data().names.len());
    assert_eq!(names, 9);

    click("rules-section-history", cx);
    let (entries, items) = cx.update(|_, cx| {
        let rules = rules.read(cx);
        (rules.data().history.len(), rules.batch_items().len())
    });
    assert!(entries >= 5, "the applies, the undone one and the edits");
    assert!(items > 0, "the newest change opens with what it changed");
    assert!(shows("rules-undo-change", cx));
}

#[gpui_kit::test]
fn typing_then_selecting_another_rule_keeps_each_rules_text(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, rules, cx) = open(cx, &scratch);
    let (pret, uber) = (scratch.seeded.rules[3], scratch.seeded.rules[1]);
    cx.update(|_, cx| rules.update(cx, |rules, cx| rules.select_rule(Some(pret), cx)));
    redraw(cx);
    let view = cx.update(|_, cx| shell.read(cx).rules_view().clone());
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.type_into(Input::Pattern, "PRET", window, cx)
        })
    });
    // Another row is chosen before the field is left.
    click(&format!("rule-{uber}"), cx);
    let stored = |id| {
        core_rule::get(&scratch.db(), id)
            .unwrap()
            .definition()
            .pattern()
            .as_str()
            .to_owned()
    };
    assert_eq!(stored(pret), "PRET", "what was typed is kept");
    assert_eq!(stored(uber), "UBER", "and never lands on the next rule");
    let shown = cx.update(|_, cx| rules.read(cx).form().unwrap().text.clone());
    assert_eq!(shown, "UBER");
}

// Categories -------------------------------------------------------------------------

fn category_key(text: &str) -> coffer_core::category::CategoryKey {
    coffer_core::category::CategoryKey::new(text).unwrap()
}

/// The stored category with `key` in `scratch`'s database.
fn stored_category(scratch: &Scratch, key: &str) -> Option<coffer_core::category::Category> {
    scratch
        .db()
        .categories()
        .unwrap()
        .get(&category_key(key))
        .cloned()
}

#[gpui_kit::test]
fn the_categories_section_lists_and_filters_every_category(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let seeded = fixture::seed_categories(&mut scratch.db());
    let (_, rules, cx) = open(cx, &scratch);
    click("rules-section-categories", cx);
    let (section, selected, rows, all) = cx.update(|_, cx| {
        let rules = rules.read(cx);
        (
            rules.section(),
            rules.selected_category(),
            rules.category_rows().len(),
            rules.data().categories.len(),
        )
    });
    assert_eq!(section, Section::Categories);
    assert_eq!(rows, all, "every category, hidden ones too");
    assert!(
        matches!(selected, Some(CategorySelection::Stored(_))),
        "the first row opens"
    );
    assert!(shows(&format!("category-{}", seeded.coffee), cx));
    assert!(shows(&format!("category-{}", seeded.side_projects), cx));
    assert!(shows("rules-category-hide", cx));

    // KIND keeps a kind's groups and subcategories; a second click clears.
    click("rules-category-kind-income", cx);
    let kinds = cx.update(|_, cx| {
        rules
            .read(cx)
            .category_rows()
            .iter()
            .map(|row| row.summary.category.kind())
            .collect::<Vec<_>>()
    });
    assert!(!kinds.is_empty());
    assert!(
        kinds
            .iter()
            .all(|kind| *kind == coffer_core::category::CategoryKind::Income)
    );
    click("rules-category-kind-income", cx);
    assert_eq!(cx.update(|_, cx| rules.read(cx).category_rows().len()), all);

    // Hidden: Mortgage, and Travel with its subcategories.
    click("rules-category-hidden", cx);
    let hidden: Vec<String> = cx.update(|_, cx| {
        rules
            .read(cx)
            .category_rows()
            .iter()
            .map(|row| row.summary.category.key().as_str().to_owned())
            .collect()
    });
    assert_eq!(
        hidden,
        [
            "home.mortgage",
            "travel",
            "travel.flights",
            "travel.accommodation"
        ]
    );

    // ↓ moves the selection through the listed rows.
    cx.update(|_, cx| rules.update(cx, |rules, cx| rules.select_category(None, cx)));
    cx.update(|_, cx| rules.update(cx, |rules, cx| rules.select_next(1, cx)));
    let first = cx.update(|_, cx| rules.read(cx).selected_category());
    let mortgage = stored_category(&scratch, "home.mortgage").unwrap().id();
    assert_eq!(first, Some(CategorySelection::Stored(mortgage)));

    // Another section and back keeps the Categories selection.
    click("rules-section-rules", cx);
    assert_eq!(cx.update(|_, cx| rules.read(cx).section()), Section::Rules);
    click("rules-section-categories", cx);
    assert_eq!(cx.update(|_, cx| rules.read(cx).selected_category()), first);
}

#[gpui_kit::test]
fn a_new_category_is_checked_then_created(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let (shell, rules, cx) = open(cx, &scratch);
    click("rules-section-categories", cx);
    click("rules-new-category", cx);
    assert_eq!(
        cx.update(|_, cx| rules.read(cx).selected_category()),
        Some(CategorySelection::New)
    );
    // Nothing typed: Create waits, and says nothing yet.
    assert_eq!(
        cx.update(|_, cx| rules.read(cx).category_error()),
        Some(form::CategoryError::Blank)
    );
    let view = cx.update(|_, cx| shell.read(cx).rules_view().clone());

    // A name a sibling has is refused, ignoring case.
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.type_into(Input::CategoryName, "groceries", window, cx)
        })
    });
    cx.update(|_, cx| {
        rules.update(cx, |rules, cx| {
            rules.edit_category(false, |form| form.group = Some(category_key("food")), cx)
        })
    });
    assert_eq!(
        cx.update(|_, cx| rules.read(cx).category_error()),
        Some(form::CategoryError::Taken)
    );
    click("rules-create-category", cx);
    assert!(stored_category(&scratch, "food.groceries_2").is_none());

    // Another name under Food is created with Food's kind, and selected.
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.type_into(Input::CategoryName, "Coffee beans", window, cx)
        })
    });
    assert_eq!(cx.update(|_, cx| rules.read(cx).category_error()), None);
    click("rules-create-category", cx);
    let created = stored_category(&scratch, "food.coffee_beans").expect("created");
    assert_eq!(created.label(), "Coffee beans");
    assert_eq!(
        created.kind(),
        coffer_core::category::CategoryKind::Spending
    );
    assert_eq!(
        cx.update(|_, cx| rules.read(cx).selected_category()),
        Some(CategorySelection::Stored(created.id()))
    );

    // A new group takes the kind chosen for it.
    click("rules-new-category", cx);
    cx.update(|_, cx| {
        rules.update(cx, |rules, cx| {
            rules.edit_category(
                false,
                |form| {
                    form.name = "Side income".into();
                    form.kind = coffer_core::category::CategoryKind::Income;
                },
                cx,
            )
        })
    });
    cx.update(|_, cx| rules.update(cx, |rules, cx| rules.create_category(cx)));
    let group = stored_category(&scratch, "side_income").expect("a new group");
    assert!(group.is_group());
    assert_eq!(group.kind(), coffer_core::category::CategoryKind::Income);
}

#[gpui_kit::test]
fn renaming_hiding_and_deleting_a_category_go_to_the_database(cx: &mut TestAppContext) {
    let scratch = Scratch::new();
    let seeded = fixture::seed_categories(&mut scratch.db());
    let (shell, rules, cx) = open(cx, &scratch);
    let view = cx.update(|_, cx| shell.read(cx).rules_view().clone());
    let groceries = stored_category(&scratch, "food.groceries").unwrap().id();
    cx.update(|_, cx| {
        rules.update(cx, |rules, cx| {
            rules.select_category(Some(CategorySelection::Stored(groceries)), cx)
        })
    });
    redraw(cx);

    // A rename saves on leaving the field; the key stays.
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.type_into(Input::CategoryName, "Food shop", window, cx)
        })
    });
    assert_eq!(
        stored_category(&scratch, "food.groceries").unwrap().label(),
        "Groceries",
        "not yet"
    );
    cx.update(|_, cx| rules.update(cx, |rules, cx| rules.save_category_name(cx)));
    assert_eq!(
        stored_category(&scratch, "food.groceries").unwrap().label(),
        "Food shop"
    );
    // A name its group already has isn't saved.
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.type_into(Input::CategoryName, "Takeaways", window, cx)
        })
    });
    cx.update(|_, cx| rules.update(cx, |rules, cx| rules.save_category_name(cx)));
    assert_eq!(
        cx.update(|_, cx| rules.read(cx).category_error()),
        Some(form::CategoryError::Taken)
    );
    assert_eq!(
        stored_category(&scratch, "food.groceries").unwrap().label(),
        "Food shop"
    );

    // Groceries is used, so it can be hidden but not deleted.
    redraw(cx);
    assert!(!shows("rules-delete-category", cx));
    click("rules-category-hide", cx);
    assert!(
        stored_category(&scratch, "food.groceries")
            .unwrap()
            .is_hidden()
    );
    // The rule editor's category menu leaves it out, unless it's chosen.
    let data = cx.update(|_, cx| rules.read(cx).data().clone());
    let offered = |chosen: Option<&coffer_core::category::CategoryKey>| {
        view::category_groups(&data, chosen)
            .iter()
            .flat_map(|group| group.options.iter())
            .any(|option| option.key.as_ref() == "food.groceries")
    };
    assert!(!offered(None));
    assert!(offered(Some(&category_key("food.groceries"))));
    // Travel is hidden as a group: none of it is offered.
    let travel = view::category_groups(&data, None)
        .iter()
        .flat_map(|group| group.options.iter())
        .any(|option| option.key.as_ref().starts_with("travel"));
    assert!(!travel);
    click("rules-category-hide", cx);
    assert!(
        !stored_category(&scratch, "food.groceries")
            .unwrap()
            .is_hidden()
    );

    // Coffee is unused: Delete category… opens the dialog, which deletes it.
    click(&format!("category-{}", seeded.coffee), cx);
    assert!(shows("rules-delete-category", cx));
    click("rules-delete-category", cx);
    let dialog = cx.update(|_, cx| view.read(cx).delete_category_dialog().cloned());
    let dialog = dialog.expect("the dialog opens");
    cx.update(|_, cx| dialog.update(cx, |dialog, cx| dialog.submit(cx)));
    redraw(cx);
    assert!(stored_category(&scratch, "food.coffee").is_none());
    assert!(
        cx.update(|_, cx| view.read(cx).delete_category_dialog().is_none()),
        "the dialog closes"
    );
    assert!(matches!(
        cx.update(|_, cx| rules.read(cx).selected_category()),
        Some(CategorySelection::Stored(_))
    ));
}
