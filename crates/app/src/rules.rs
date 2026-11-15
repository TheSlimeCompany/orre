//! The Rules view (ROADMAP M3.13 and the rules half of M3.7;
//! `docs/DESIGN.md` → "Rules", D6.7; `docs/ARCHITECTURE.md` → "Decision
//! record: rules" and "Decision record: the Rules view (built)").
//!
//! - **Rules** (`B1J-0`): every rule in priority order with what it decides
//!   today, the selected one edited in the inspector (When, Then, Past
//!   transactions). Edits save as they're made, once valid, and never re-run
//!   on history; only Apply changes past transactions.
//! - **Apply to past transactions** (`EFT-0`): a stored rule's or a new
//!   rule's changes previewed on the canvas, "Will change" and "Category you
//!   set" (left out unless "Change these too"), committed as one undoable
//!   change batch. A new rule (New rule, or "Create rule…" from
//!   Transactions) is made here and stored only on "Create rule…".
//! - **Merchant names** (`G2O-0`): the aliases, applied at once with no
//!   batch (the display name is derived).
//! - **Change history** (`GZT-0`): every change batch, each undoable.
//! - **Categories** (M3.7's categories editor; composed from the sections
//!   above, not drawn): every group with its subcategories and what uses
//!   each; renamed, hidden, shown, added and (when nothing uses it) deleted
//!   at once, with no batch, since no transaction changes.
//!
//! [`Rules`] holds what the view shows (from [`store::RulesStore`], which
//! opens the database per call), the open section, its filter, the
//! selection, the rule or name being edited ([`form`]) and the last
//! outcome. Every change is core's; this only picks IDs and fields. Rule
//! patterns, names and bank text are untrusted and shown plain ([`text`]).

pub mod dialogs;
pub mod fixture;
pub mod form;
pub mod inspector;
pub mod panel;
pub mod store;
pub mod table;
pub mod text;
pub mod view;

use std::collections::BTreeSet;

use coffer_core::category::{CategoryKind, CategorySummary, GroupOverview};
use coffer_core::change::{ChangeKind, HistoryItem};
use coffer_core::id::{ChangeBatchId, MerchantAliasId, RuleId, TransactionId};
use coffer_core::inbox::{Suggestion, SuggestionId};
use coffer_core::merchant::AliasKind;
use coffer_core::rule::{
    ActionKind, ApplyPreview, Draft, PreviewRow, RuleDefinition, RuleOrigin, RuleSummary,
};
use gpui_kit::{Context, EventEmitter, SharedString};

use form::{CategoryError, CategoryForm, FormError, NameForm, RuleForm};
use store::{RulesData, RulesStore};

/// Binds the view's keys: ↑↓ move the selection, Escape clears it or goes
/// back from the preview. Call once, before the first [`view::RulesView`].
pub fn init(cx: &mut gpui_kit::App) {
    use gpui_kit::KeyBinding;
    use gpui_kit::base::actions::{Cancel, SelectDown, SelectUp};
    cx.bind_keys([
        KeyBinding::new("up", SelectUp, Some(view::CONTEXT)),
        KeyBinding::new("down", SelectDown, Some(view::CONTEXT)),
        KeyBinding::new("escape", Cancel, Some(view::CONTEXT)),
    ]);
}

/// The lists under RULES in the panel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Section {
    #[default]
    Rules,
    Names,
    History,
    Categories,
}

/// What the Categories section's panel filter keeps.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CategoryFilter {
    /// The groups of this kind, with their subcategories.
    Kind(CategoryKind),
    /// The categories pickers leave out: hidden, or in a hidden group.
    Hidden,
}

/// The category selected in Categories.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CategorySelection {
    /// A stored category, by row ID.
    Stored(i64),
    /// "+ New category", not stored yet.
    New,
}

/// A row of the Categories table.
#[derive(Clone, Copy, Debug)]
pub struct CategoryRow<'a> {
    pub summary: &'a CategorySummary,
    /// Whether it's a subcategory (indented under its group).
    pub child: bool,
    /// Hidden itself, or with its group.
    pub hidden: bool,
}

/// What the view asks its owner (the shell) to do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RulesEvent {
    /// "Delete rule…": open the delete dialog.
    DeleteRuleRequested(RuleId),
    /// "Delete name…": open the delete dialog.
    DeleteNameRequested(MerchantAliasId),
    /// "Undo this change…": open the undo dialog.
    UndoRequested(ChangeBatchId),
    /// "Review in the Inbox": open the Inbox on the suggestion.
    ReviewSuggestion(SuggestionId),
    /// Transactions changed (an apply, an undo, a name): what reads them
    /// should read again.
    Changed,
}

/// Which rule the preview applies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// A stored rule.
    Stored(RuleId),
    /// The rule being made, not stored yet.
    New,
}

/// The apply-to-past-transactions preview on the canvas.
#[derive(Clone, Debug, PartialEq)]
pub struct Apply {
    pub target: Target,
    /// Core's plan; `None` while the new rule's fields aren't valid.
    pub preview: Option<ApplyPreview>,
    /// Its rows, newest first.
    pub rows: Vec<PreviewRow>,
    /// A new rule: the transactions it matches, and the rules it overlaps.
    pub matches: usize,
    pub overlaps: Vec<RuleId>,
    /// "Change these too": include the rows the user set.
    pub include: bool,
}

impl Apply {
    fn new(target: Target) -> Apply {
        Apply {
            target,
            preview: None,
            rows: Vec::new(),
            matches: 0,
            overlaps: Vec::new(),
            include: false,
        }
    }

    /// The rows that change without being included.
    pub fn will_change(&self) -> impl Iterator<Item = &PreviewRow> {
        self.rows.iter().filter(|row| !row.user_set)
    }

    /// The rows the user set, changed only with "Change these too".
    pub fn user_set(&self) -> impl Iterator<Item = &PreviewRow> {
        self.rows.iter().filter(|row| row.user_set)
    }

    /// How many transactions the commit changes.
    pub fn changing(&self) -> usize {
        if self.include {
            self.rows.len()
        } else {
            self.will_change().count()
        }
    }

    fn included(&self) -> BTreeSet<TransactionId> {
        if !self.include {
            return BTreeSet::new();
        }
        self.user_set().map(|row| row.transaction).collect()
    }
}

/// What the last change did, shown until the next one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// It went through: the bar's message, and the batch Undo reverts.
    Done {
        message: SharedString,
        undo: Option<ChangeBatchId>,
    },
    /// Core refused it; nothing changed.
    Refused(SharedString),
}

/// Where the view reads from.
enum Source {
    Database(RulesStore),
    /// Nothing to read (no data folder, or a test without a database).
    None,
}

/// The name selected in Merchant names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameSelection {
    Stored(MerchantAliasId),
    /// "+ New name", not stored yet.
    New,
}

/// What the inspector's fields are filled from ([`Rules::form_key`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormKey {
    None,
    Rule(RuleId),
    NewRule,
    Name(NameSelection),
    Category(CategorySelection),
}

/// The Rules view's state (module docs).
pub struct Rules {
    source: Source,
    data: RulesData,
    section: Section,
    action_filter: Option<ActionKind>,
    match_filter: Option<AliasKind>,
    kind_filter: Option<ChangeKind>,
    rule: Option<RuleId>,
    /// The selected rule's fields, or the new rule's.
    form: Option<RuleForm>,
    /// The new rule's place and origin.
    new_position: usize,
    new_origin: RuleOrigin,
    apply: Option<Apply>,
    /// Pending agent rules over the selected rule's rows.
    touching: Vec<Suggestion>,
    name: Option<NameSelection>,
    name_form: Option<NameForm>,
    batch: Option<ChangeBatchId>,
    batch_items: Vec<HistoryItem>,
    category_filter: Option<CategoryFilter>,
    category: Option<CategorySelection>,
    category_form: Option<CategoryForm>,
    outcome: Option<Outcome>,
    load_error: Option<SharedString>,
}

impl EventEmitter<RulesEvent> for Rules {}

fn message(error: impl std::fmt::Display) -> SharedString {
    SharedString::from(text::sentence(&error.to_string()))
}

impl Rules {
    /// The view over `store`, read now.
    pub fn from_store(store: RulesStore) -> Self {
        let mut rules = Self::with_source(Source::Database(store));
        rules.load();
        rules.select_default();
        rules
    }

    /// Nothing to show, because nothing can be read: `reason` says why.
    pub fn unavailable(reason: impl Into<SharedString>) -> Self {
        Self {
            load_error: Some(reason.into()),
            ..Self::with_source(Source::None)
        }
    }

    /// An empty view that reads nothing (tests).
    pub fn empty() -> Self {
        Self::with_source(Source::None)
    }

    fn with_source(source: Source) -> Self {
        Self {
            source,
            data: RulesData::default(),
            section: Section::Rules,
            action_filter: None,
            match_filter: None,
            kind_filter: None,
            rule: None,
            form: None,
            new_position: 0,
            new_origin: RuleOrigin::New,
            apply: None,
            touching: Vec::new(),
            name: None,
            name_form: None,
            batch: None,
            batch_items: Vec::new(),
            category_filter: None,
            category: None,
            category_form: None,
            outcome: None,
            load_error: None,
        }
    }

    pub fn data(&self) -> &RulesData {
        &self.data
    }

    pub fn today(&self) -> chrono::NaiveDate {
        self.data
            .today
            .unwrap_or_else(crate::accounts::fixture::today)
    }

    pub fn section(&self) -> Section {
        self.section
    }

    pub fn outcome(&self) -> Option<&Outcome> {
        self.outcome.as_ref()
    }

    pub fn load_error(&self) -> Option<&SharedString> {
        self.load_error.as_ref()
    }

    pub fn action_filter(&self) -> Option<ActionKind> {
        self.action_filter
    }

    pub fn match_filter(&self) -> Option<AliasKind> {
        self.match_filter
    }

    pub fn kind_filter(&self) -> Option<ChangeKind> {
        self.kind_filter
    }

    pub fn apply(&self) -> Option<&Apply> {
        self.apply.as_ref()
    }

    pub fn touching(&self) -> &[Suggestion] {
        &self.touching
    }

    // Reading ------------------------------------------------------------------

    /// Reads everything again, keeping the selection where it still is.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self.load();
        if let Some(id) = self.rule {
            match self.data.rule(id) {
                // An edit in progress isn't overwritten.
                Some(_) if self.apply.as_ref().is_some_and(|a| a.target == Target::New) => {}
                Some(summary) => {
                    if self.form_error().is_none() {
                        self.form = Some(RuleForm::of(summary.rule.definition()));
                    }
                }
                None => {
                    self.rule = None;
                    self.form = None;
                }
            }
        }
        if let Some(NameSelection::Stored(id)) = self.name
            && self.data.name(id).is_none()
        {
            self.name = None;
            self.name_form = None;
        }
        if let Some(batch) = self.batch {
            if self.data.entry(batch).is_some() {
                self.load_items(batch);
            } else {
                self.batch = None;
                self.batch_items.clear();
            }
        }
        if let Some(CategorySelection::Stored(id)) = self.category {
            match self.data.category_entry(id) {
                // A rename that isn't valid yet isn't overwritten.
                Some((summary, _)) if self.category_error().is_none() => {
                    self.category_form = Some(CategoryForm::of(&summary.category));
                }
                Some(_) => {}
                None => {
                    self.category = None;
                    self.category_form = None;
                }
            }
        }
        cx.notify();
    }

    fn load(&mut self) {
        let Source::Database(store) = &self.source else {
            return;
        };
        match store.load() {
            Ok(data) => {
                self.data = data;
                self.load_error = None;
            }
            Err(error) => {
                tracing::warn!(%error, "couldn't read the rules");
                self.load_error = Some(message(error));
            }
        }
    }

    fn store(&self) -> Result<&RulesStore, SharedString> {
        match &self.source {
            Source::Database(store) => Ok(store),
            Source::None => Err("There is no database to change.".into()),
        }
    }

    /// Selects the first row of the open section when nothing is.
    fn select_default(&mut self) {
        match self.section {
            Section::Rules if self.rule.is_none() && self.apply.is_none() => {
                if let Some(first) = self.rules().first().map(|summary| summary.rule.id()) {
                    self.set_rule(Some(first));
                }
            }
            Section::Names if self.name.is_none() => {
                if let Some(first) = self.names().first().map(|name| name.alias.id()) {
                    self.set_name(Some(NameSelection::Stored(first)));
                }
            }
            Section::History if self.batch.is_none() => {
                if let Some(first) = self.history().first().map(|entry| entry.batch.id) {
                    self.set_batch(Some(first));
                }
            }
            Section::Categories if self.category.is_none() => {
                let first = self
                    .category_rows()
                    .first()
                    .map(|row| row.summary.category.id());
                if let Some(first) = first {
                    self.set_category(Some(CategorySelection::Stored(first)));
                }
            }
            _ => {}
        }
    }

    // Sections and filters -----------------------------------------------------

    pub fn show(&mut self, section: Section, cx: &mut Context<Self>) {
        if self.section != section {
            self.section = section;
            self.outcome = None;
            if section != Section::Rules {
                self.apply = None;
            }
            self.select_default();
        }
        cx.notify();
    }

    pub fn toggle_action_filter(&mut self, kind: ActionKind, cx: &mut Context<Self>) {
        self.section = Section::Rules;
        self.action_filter = (self.action_filter != Some(kind)).then_some(kind);
        cx.notify();
    }

    pub fn toggle_match_filter(&mut self, kind: AliasKind, cx: &mut Context<Self>) {
        self.section = Section::Names;
        self.match_filter = (self.match_filter != Some(kind)).then_some(kind);
        cx.notify();
    }

    pub fn toggle_kind_filter(&mut self, kind: ChangeKind, cx: &mut Context<Self>) {
        self.section = Section::History;
        self.kind_filter = (self.kind_filter != Some(kind)).then_some(kind);
        cx.notify();
    }

    pub fn toggle_category_filter(&mut self, filter: CategoryFilter, cx: &mut Context<Self>) {
        self.section = Section::Categories;
        self.category_filter = (self.category_filter != Some(filter)).then_some(filter);
        cx.notify();
    }

    pub fn category_filter(&self) -> Option<CategoryFilter> {
        self.category_filter
    }

    /// Every category as the table lists it, each group followed by its
    /// subcategories, before the filter.
    fn all_category_rows(&self) -> Vec<CategoryRow<'_>> {
        let mut rows = Vec::new();
        for group in &self.data.category_overview {
            let group_hidden = group.group.category.is_hidden();
            rows.push(CategoryRow {
                summary: &group.group,
                child: false,
                hidden: group_hidden,
            });
            rows.extend(group.children.iter().map(|child| CategoryRow {
                summary: child,
                child: true,
                hidden: group_hidden || child.category.is_hidden(),
            }));
        }
        rows
    }

    fn keeps(filter: CategoryFilter, row: &CategoryRow<'_>) -> bool {
        match filter {
            CategoryFilter::Kind(kind) => row.summary.category.kind() == kind,
            CategoryFilter::Hidden => row.hidden,
        }
    }

    /// The categories the table lists, filtered by KIND or Hidden.
    pub fn category_rows(&self) -> Vec<CategoryRow<'_>> {
        let filter = self.category_filter;
        self.all_category_rows()
            .into_iter()
            .filter(|row| filter.is_none_or(|filter| Self::keeps(filter, row)))
            .collect()
    }

    /// How many categories `filter` keeps (the panel's counts).
    pub fn count_categories(&self, filter: CategoryFilter) -> usize {
        self.all_category_rows()
            .iter()
            .filter(|row| Self::keeps(filter, row))
            .count()
    }

    /// The rules the table lists, filtered by ACTION.
    pub fn rules(&self) -> Vec<&RuleSummary> {
        self.data
            .rules
            .iter()
            .filter(|summary| {
                self.action_filter
                    .is_none_or(|kind| summary.rule.definition().action(kind).is_some())
            })
            .collect()
    }

    /// The names the table lists, filtered by MATCH.
    pub fn names(&self) -> Vec<&coffer_core::merchant::AliasSummary> {
        self.data
            .names
            .iter()
            .filter(|name| {
                self.match_filter
                    .is_none_or(|kind| name.alias.definition().kind() == kind)
            })
            .collect()
    }

    /// The changes the table lists, filtered by KIND.
    pub fn history(&self) -> Vec<&coffer_core::change::HistoryEntry> {
        self.data
            .history
            .iter()
            .filter(|entry| self.kind_filter.is_none_or(|kind| entry.batch.kind == kind))
            .collect()
    }

    /// Rules with an action of `kind` (the ACTION counts).
    pub fn count_with_action(&self, kind: ActionKind) -> usize {
        self.data
            .rules
            .iter()
            .filter(|summary| summary.rule.definition().action(kind).is_some())
            .count()
    }

    // Rules ----------------------------------------------------------------------

    pub fn selected_rule(&self) -> Option<&RuleSummary> {
        self.data.rule(self.rule?)
    }

    pub fn selected_rule_id(&self) -> Option<RuleId> {
        self.rule
    }

    /// The fields in the inspector: the selected rule's or the new rule's.
    pub fn form(&self) -> Option<&RuleForm> {
        self.form.as_ref()
    }

    /// Why the inspector's rule can't be saved yet, if it can't.
    pub fn form_error(&self) -> Option<FormError> {
        let form = self.form.as_ref()?;
        form.definition(self.data.currency(form.account)).err()
    }

    /// What the inspector's fields are for: the view fills them again when
    /// it changes, and ignores a late event from the fields of another.
    pub fn form_key(&self) -> FormKey {
        match self.section {
            Section::Rules if self.is_new() => FormKey::NewRule,
            Section::Rules => self.rule.map_or(FormKey::None, FormKey::Rule),
            Section::Names => self.name.map_or(FormKey::None, FormKey::Name),
            Section::History => FormKey::None,
            Section::Categories => self.category.map_or(FormKey::None, FormKey::Category),
        }
    }

    /// Whether the inspector shows the new rule.
    pub fn is_new(&self) -> bool {
        self.apply
            .as_ref()
            .is_some_and(|apply| apply.target == Target::New)
    }

    pub fn new_position(&self) -> usize {
        self.new_position
    }

    pub fn new_origin(&self) -> RuleOrigin {
        self.new_origin
    }

    fn set_rule(&mut self, id: Option<RuleId>) {
        self.rule = id;
        self.form = id
            .and_then(|id| self.data.rule(id))
            .map(|summary| RuleForm::of(summary.rule.definition()));
        self.touching = match (id, self.store()) {
            (Some(id), Ok(store)) => store.touching(id).unwrap_or_else(|error| {
                tracing::warn!(%error, "couldn't read the suggestions over a rule");
                Vec::new()
            }),
            _ => Vec::new(),
        };
    }

    /// Selects rule `id` in the table (from the preview too: back to it).
    pub fn select_rule(&mut self, id: Option<RuleId>, cx: &mut Context<Self>) {
        // What was typed for the rule being left is kept, as leaving its
        // field would: the field's blur arrives after the selection moved.
        if self.rule != id && self.section == Section::Rules {
            self.save_rule(cx);
        }
        self.section = Section::Rules;
        self.apply = None;
        if self.rule != id || self.form.is_none() {
            self.set_rule(id);
        }
        cx.notify();
    }

    /// Selects `id`, or nothing if it's selected.
    pub fn toggle_rule(&mut self, id: RuleId, cx: &mut Context<Self>) {
        let next = (self.rule != Some(id)).then_some(id);
        self.select_rule(next, cx);
    }

    /// Moves the selection `step` rows in the open section (↑↓).
    pub fn select_next(&mut self, step: isize, cx: &mut Context<Self>) {
        match self.section {
            Section::Rules if self.apply.is_none() => {
                let order: Vec<RuleId> = self.rules().iter().map(|s| s.rule.id()).collect();
                if let Some(next) = step_in(&order, self.rule, step) {
                    self.select_rule(Some(next), cx);
                }
            }
            Section::Names => {
                let order: Vec<MerchantAliasId> =
                    self.names().iter().map(|n| n.alias.id()).collect();
                let current = match self.name {
                    Some(NameSelection::Stored(id)) => Some(id),
                    _ => None,
                };
                if let Some(next) = step_in(&order, current, step) {
                    self.select_name(Some(NameSelection::Stored(next)), cx);
                }
            }
            Section::History => {
                let order: Vec<ChangeBatchId> = self.history().iter().map(|e| e.batch.id).collect();
                if let Some(next) = step_in(&order, self.batch, step) {
                    self.select_batch(Some(next), cx);
                }
            }
            Section::Categories => {
                let order: Vec<i64> = self
                    .category_rows()
                    .iter()
                    .map(|row| row.summary.category.id())
                    .collect();
                let current = match self.category {
                    Some(CategorySelection::Stored(id)) => Some(id),
                    _ => None,
                };
                if let Some(next) = step_in(&order, current, step) {
                    self.select_category(Some(CategorySelection::Stored(next)), cx);
                }
            }
            Section::Rules => {}
        }
    }

    /// Escape: back from the preview, else clears the selection.
    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        if self.apply.is_some() {
            self.back(cx);
            return;
        }
        match self.section {
            Section::Rules => self.select_rule(None, cx),
            Section::Names => self.select_name(None, cx),
            Section::History => self.select_batch(None, cx),
            Section::Categories => self.select_category(None, cx),
        }
    }

    /// Changes the inspector's fields. A new rule's preview follows at
    /// once; a stored rule is saved when `save` and the fields are valid
    /// (text fields save on blur and Enter, selects at once).
    pub fn edit(&mut self, save: bool, change: impl FnOnce(&mut RuleForm), cx: &mut Context<Self>) {
        let Some(form) = self.form.as_mut() else {
            return;
        };
        change(form);
        if self.is_new() {
            self.refresh_preview();
        } else if save {
            self.save_rule(cx);
        }
        cx.notify();
    }

    /// Saves the selected rule's fields when they're valid and changed.
    pub fn save_rule(&mut self, cx: &mut Context<Self>) {
        let (Some(id), Some(form)) = (self.rule, self.form.as_ref()) else {
            return;
        };
        if self.is_new() {
            return;
        }
        let Ok(definition) = form.definition(self.data.currency(form.account)) else {
            return;
        };
        if self
            .data
            .rule(id)
            .is_some_and(|summary| *summary.rule.definition() == definition)
        {
            return;
        }
        let result = self
            .store()
            .and_then(|store| store.update(id, &definition).map_err(message));
        match result {
            Ok(()) => {
                self.outcome = None;
                self.load();
                if self
                    .apply
                    .as_ref()
                    .is_some_and(|a| a.target == Target::Stored(id))
                {
                    self.refresh_preview();
                }
                self.touching = self
                    .store()
                    .ok()
                    .and_then(|store| store.touching(id).ok())
                    .unwrap_or_default();
            }
            Err(message) => self.outcome = Some(Outcome::Refused(message)),
        }
        cx.notify();
    }

    /// Moves rule `id` to `position` (dragging a row, Position, "Move
    /// above rule 2").
    pub fn move_rule(&mut self, id: RuleId, position: usize, cx: &mut Context<Self>) {
        if self.data.position(id) == Some(position) {
            return;
        }
        let result = self
            .store()
            .and_then(|store| store.move_to(id, position).map_err(message));
        match result {
            Ok(()) => {
                self.outcome = None;
                self.load();
            }
            Err(message) => self.outcome = Some(Outcome::Refused(message)),
        }
        cx.notify();
    }

    /// The new rule's Position.
    pub fn set_new_position(&mut self, position: usize, cx: &mut Context<Self>) {
        self.new_position = position.min(self.data.rules.len());
        self.refresh_preview();
        cx.notify();
    }

    /// "+ New rule": the preview with an empty rule (composed: `DESIGN.md`
    /// → "Rules" → "Built in M3.13").
    pub fn new_rule(&mut self, cx: &mut Context<Self>) {
        self.start_new(RuleForm::default(), RuleOrigin::New, cx);
    }

    /// "Create rule…" from Transactions: the preview with `rule` filled in.
    pub fn new_rule_from(
        &mut self,
        rule: &RuleDefinition,
        origin: RuleOrigin,
        cx: &mut Context<Self>,
    ) {
        self.load();
        self.start_new(RuleForm::of(rule), origin, cx);
    }

    fn start_new(&mut self, form: RuleForm, origin: RuleOrigin, cx: &mut Context<Self>) {
        self.section = Section::Rules;
        self.outcome = None;
        self.rule = None;
        self.touching.clear();
        self.form = Some(form);
        self.new_position = 0;
        self.new_origin = origin;
        self.apply = Some(Apply::new(Target::New));
        self.refresh_preview();
        cx.notify();
    }

    /// "Apply to past transactions…": the selected rule's preview.
    pub fn apply_selected(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.rule else {
            return;
        };
        self.outcome = None;
        self.apply = Some(Apply::new(Target::Stored(id)));
        self.refresh_preview();
        cx.notify();
    }

    /// The back button: from the preview to the table.
    pub fn back(&mut self, cx: &mut Context<Self>) {
        if let Some(apply) = self.apply.take()
            && apply.target == Target::New
        {
            self.form = None;
            self.select_default();
        }
        cx.notify();
    }

    /// "Change these too".
    pub fn set_include(&mut self, include: bool, cx: &mut Context<Self>) {
        if let Some(apply) = self.apply.as_mut() {
            apply.include = include;
        }
        cx.notify();
    }

    /// Plans the preview again for the current fields.
    fn refresh_preview(&mut self) {
        let Some(apply) = self.apply.as_ref() else {
            return;
        };
        let target = apply.target;
        let include = apply.include;
        let store = match &self.source {
            Source::Database(store) => store.clone(),
            Source::None => return,
        };
        let mut next = Apply {
            include,
            ..Apply::new(target)
        };
        let result = match target {
            Target::Stored(id) => store.preview_rule(id).map(|(preview, rows)| {
                next.preview = Some(preview);
                next.rows = rows;
            }),
            Target::New => match self.draft() {
                Some(draft) => store.preview_draft(&draft).map(|(preview, rows)| {
                    next.matches = preview.matches;
                    next.overlaps = preview.overlaps;
                    next.preview = Some(preview.preview);
                    next.rows = rows;
                }),
                // Not valid yet: nothing to preview.
                None => Ok(()),
            },
        };
        if let Err(error) = result {
            self.outcome = Some(Outcome::Refused(message(error)));
        }
        self.apply = Some(next);
    }

    /// The new rule as core takes it, once its fields are valid.
    fn draft(&self) -> Option<Draft> {
        let form = self.form.as_ref()?;
        let definition = form.definition(self.data.currency(form.account)).ok()?;
        Some(Draft {
            definition,
            position: self.new_position,
            origin: self.new_origin,
        })
    }

    /// "Create rule and change 4" or "Apply to 4 transactions": applies the
    /// preview as one change batch (storing the new rule first).
    pub fn commit(&mut self, cx: &mut Context<Self>) {
        self.finish(true, cx);
    }

    /// "Create rule only": stores the new rule without touching history.
    pub fn create_only(&mut self, cx: &mut Context<Self>) {
        self.finish(false, cx);
    }

    fn finish(&mut self, apply_it: bool, cx: &mut Context<Self>) {
        let Some(apply) = self.apply.clone() else {
            return;
        };
        let changed_message = |count: usize, category: Option<String>| -> SharedString {
            let rows = text::count(count, "transaction", "transactions");
            match category {
                Some(label) => format!("Changed {rows} to {label}"),
                None => format!("Changed {rows}"),
            }
            .into()
        };
        let category = self
            .form
            .as_ref()
            .and_then(|form| form.action(ActionKind::SetCategory))
            .and_then(|action| match action {
                form::ActionForm::Category(Some(key)) => {
                    self.data.category(key).map(|option| option.label.clone())
                }
                _ => None,
            });
        let included = apply.included();
        let result: Result<(Option<RuleId>, Option<coffer_core::rule::Applied>), SharedString> =
            match apply.target {
                Target::Stored(id) => {
                    let Some(preview) = apply.preview.as_ref() else {
                        return;
                    };
                    self.store().and_then(|store| {
                        store
                            .apply(preview, &included)
                            .map(|applied| (Some(id), Some(applied)))
                            .map_err(message)
                    })
                }
                Target::New => {
                    let Some(draft) = self.draft() else {
                        return;
                    };
                    let preview = apply.preview.as_ref().filter(|_| apply_it);
                    self.store().and_then(|store| {
                        store
                            .save_draft(&draft, preview.map(|preview| (preview, &included)))
                            .map(|saved| (Some(saved.rule), saved.applied))
                            .map_err(message)
                    })
                }
            };
        match result {
            Ok((rule, applied)) => {
                self.apply = None;
                self.load();
                self.set_rule(rule);
                self.outcome = match applied {
                    Some(applied) if applied.transactions > 0 => Some(Outcome::Done {
                        message: changed_message(applied.transactions, category),
                        undo: applied.batch,
                    }),
                    _ => None,
                };
                if applied.is_some_and(|applied| applied.transactions > 0) {
                    cx.emit(RulesEvent::Changed);
                }
            }
            Err(message) => self.outcome = Some(Outcome::Refused(message)),
        }
        cx.notify();
    }

    /// The bar's Undo: undoes the last apply.
    pub fn undo(&mut self, cx: &mut Context<Self>) {
        let Some(Outcome::Done {
            undo: Some(batch), ..
        }) = self.outcome.clone()
        else {
            return;
        };
        self.undo_batch(batch, cx);
    }

    /// Undoes `batch` (the bar, the undo dialog).
    pub fn undo_batch(&mut self, batch: ChangeBatchId, cx: &mut Context<Self>) {
        let result = self
            .store()
            .and_then(|store| store.undo(batch).map_err(message));
        match result {
            Ok(_) => {
                self.outcome = None;
                self.reload(cx);
                cx.emit(RulesEvent::Changed);
            }
            Err(message) => self.outcome = Some(Outcome::Refused(message)),
        }
        cx.notify();
    }

    /// What undoing `batch` would do, for the dialog.
    pub fn undo_preview(
        &self,
        batch: ChangeBatchId,
    ) -> Result<coffer_core::change::UndoSummary, SharedString> {
        self.store()?.undo_preview(batch).map_err(message)
    }

    pub fn dismiss_outcome(&mut self, cx: &mut Context<Self>) {
        if self.outcome.take().is_some() {
            cx.notify();
        }
    }

    pub fn request_delete_rule(&mut self, cx: &mut Context<Self>) {
        if let Some(id) = self.rule {
            cx.emit(RulesEvent::DeleteRuleRequested(id));
        }
    }

    /// Deletes rule `id` (the dialog). Its transactions keep their values.
    pub fn delete_rule(&mut self, id: RuleId, cx: &mut Context<Self>) -> Result<(), SharedString> {
        self.store()?.delete(id).map_err(message)?;
        let next = self.next_rule_after(id);
        self.load();
        self.set_rule(next);
        self.outcome = None;
        cx.notify();
        Ok(())
    }

    fn next_rule_after(&self, id: RuleId) -> Option<RuleId> {
        let order: Vec<RuleId> = self.rules().iter().map(|s| s.rule.id()).collect();
        let at = order.iter().position(|row| *row == id)?;
        order
            .get(at + 1)
            .or_else(|| at.checked_sub(1).and_then(|before| order.get(before)))
            .copied()
    }

    /// "Review in the Inbox".
    pub fn review_suggestion(&mut self, id: SuggestionId, cx: &mut Context<Self>) {
        cx.emit(RulesEvent::ReviewSuggestion(id));
    }

    // Merchant names -------------------------------------------------------------

    pub fn selected_name(&self) -> Option<NameSelection> {
        self.name
    }

    pub fn name_form(&self) -> Option<&NameForm> {
        self.name_form.as_ref()
    }

    pub fn name_error(&self) -> Option<form::NameError> {
        self.name_form.as_ref()?.definition().err()
    }

    fn set_name(&mut self, name: Option<NameSelection>) {
        self.name = name;
        self.name_form = match name {
            Some(NameSelection::Stored(id)) => self
                .data
                .name(id)
                .map(|summary| NameForm::of(summary.alias.definition())),
            Some(NameSelection::New) => Some(NameForm::default()),
            None => None,
        };
    }

    pub fn select_name(&mut self, name: Option<NameSelection>, cx: &mut Context<Self>) {
        if self.name != name && self.section == Section::Names {
            self.save_name(cx);
        }
        self.section = Section::Names;
        if self.name != name {
            self.set_name(name);
        }
        cx.notify();
    }

    pub fn toggle_name(&mut self, id: MerchantAliasId, cx: &mut Context<Self>) {
        let next =
            (self.name != Some(NameSelection::Stored(id))).then_some(NameSelection::Stored(id));
        self.select_name(next, cx);
    }

    /// "+ New name".
    pub fn new_name(&mut self, cx: &mut Context<Self>) {
        self.outcome = None;
        self.select_name(Some(NameSelection::New), cx);
    }

    /// Changes the name's fields; saves when `save` and they're valid. A
    /// name applies everywhere at once, with no preview or batch.
    pub fn edit_name(
        &mut self,
        save: bool,
        change: impl FnOnce(&mut NameForm),
        cx: &mut Context<Self>,
    ) {
        let Some(form) = self.name_form.as_mut() else {
            return;
        };
        change(form);
        if save {
            self.save_name(cx);
        }
        cx.notify();
    }

    fn save_name(&mut self, cx: &mut Context<Self>) {
        let (Some(selection), Some(form)) = (self.name, self.name_form.as_ref()) else {
            return;
        };
        let Ok(definition) = form.definition() else {
            return;
        };
        let result = match selection {
            NameSelection::Stored(id) => {
                if self
                    .data
                    .name(id)
                    .is_some_and(|summary| *summary.alias.definition() == definition)
                {
                    return;
                }
                self.store()
                    .and_then(|store| store.edit_name(id, &definition).map_err(message))
                    .map(|()| id)
            }
            NameSelection::New => self
                .store()
                .and_then(|store| store.add_name(&definition).map_err(message)),
        };
        match result {
            Ok(id) => {
                self.outcome = None;
                self.load();
                self.name = Some(NameSelection::Stored(id));
                cx.emit(RulesEvent::Changed);
            }
            Err(message) => self.outcome = Some(Outcome::Refused(message)),
        }
        cx.notify();
    }

    pub fn request_delete_name(&mut self, cx: &mut Context<Self>) {
        if let Some(NameSelection::Stored(id)) = self.name {
            cx.emit(RulesEvent::DeleteNameRequested(id));
        }
    }

    /// Deletes name `id` (the dialog): its rows show their cleaned text
    /// again, or another name.
    pub fn delete_name(
        &mut self,
        id: MerchantAliasId,
        cx: &mut Context<Self>,
    ) -> Result<(), SharedString> {
        self.store()?.delete_name(id).map_err(message)?;
        self.load();
        self.set_name(None);
        self.select_default();
        cx.emit(RulesEvent::Changed);
        cx.notify();
        Ok(())
    }

    // Change history ---------------------------------------------------------------

    pub fn selected_batch(&self) -> Option<&coffer_core::change::HistoryEntry> {
        self.data.entry(self.batch?)
    }

    pub fn batch_items(&self) -> &[HistoryItem] {
        &self.batch_items
    }

    fn load_items(&mut self, batch: ChangeBatchId) {
        self.batch_items = match self.store() {
            Ok(store) => store.history_items(batch).unwrap_or_else(|error| {
                tracing::warn!(%error, "couldn't read a change's items");
                Vec::new()
            }),
            Err(_) => Vec::new(),
        };
    }

    fn set_batch(&mut self, batch: Option<ChangeBatchId>) {
        self.batch = batch;
        match batch {
            Some(batch) => self.load_items(batch),
            None => self.batch_items.clear(),
        }
    }

    pub fn select_batch(&mut self, batch: Option<ChangeBatchId>, cx: &mut Context<Self>) {
        self.section = Section::History;
        if self.batch != batch {
            self.set_batch(batch);
        }
        cx.notify();
    }

    pub fn toggle_batch(&mut self, batch: ChangeBatchId, cx: &mut Context<Self>) {
        let next = (self.batch != Some(batch)).then_some(batch);
        self.select_batch(next, cx);
    }

    pub fn request_undo(&mut self, cx: &mut Context<Self>) {
        if let Some(batch) = self.batch {
            cx.emit(RulesEvent::UndoRequested(batch));
        }
    }

    // Categories -------------------------------------------------------------------

    pub fn selected_category(&self) -> Option<CategorySelection> {
        self.category
    }

    /// The selected stored category and its group (its own, for a group).
    pub fn selected_category_entry(&self) -> Option<(&CategorySummary, &GroupOverview)> {
        match self.category? {
            CategorySelection::Stored(id) => self.data.category_entry(id),
            CategorySelection::New => None,
        }
    }

    pub fn category_form(&self) -> Option<&CategoryForm> {
        self.category_form.as_ref()
    }

    /// The categories beside the selected one, or beside where the new one
    /// goes: row ID and label.
    fn category_siblings(&self) -> Vec<(i64, &str)> {
        let parent = match self.category {
            Some(CategorySelection::Stored(id)) => match self.data.category_entry(id) {
                Some((summary, group)) if !summary.category.is_group() => {
                    Some(group.group.category.key().clone())
                }
                Some(_) => None,
                None => return Vec::new(),
            },
            Some(CategorySelection::New) => self
                .category_form
                .as_ref()
                .and_then(|form| form.group.clone()),
            None => return Vec::new(),
        };
        let summaries: Vec<&CategorySummary> = match parent {
            Some(parent) => self
                .data
                .category_group(&parent)
                .map(|group| group.children.iter().collect())
                .unwrap_or_default(),
            None => self
                .data
                .category_overview
                .iter()
                .map(|group| &group.group)
                .collect(),
        };
        summaries
            .into_iter()
            .map(|summary| (summary.category.id(), summary.category.label()))
            .collect()
    }

    /// Why the category's name can't be saved, if it can't.
    pub fn category_error(&self) -> Option<CategoryError> {
        let form = self.category_form.as_ref()?;
        let except = match self.category? {
            CategorySelection::Stored(id) => Some(id),
            CategorySelection::New => None,
        };
        form.label(self.category_siblings(), except).err()
    }

    fn set_category(&mut self, selection: Option<CategorySelection>) {
        self.category = selection;
        self.category_form = match selection {
            Some(CategorySelection::Stored(id)) => self
                .data
                .category_entry(id)
                .map(|(summary, _)| CategoryForm::of(&summary.category)),
            Some(CategorySelection::New) => Some(CategoryForm::default()),
            None => None,
        };
    }

    pub fn select_category(
        &mut self,
        selection: Option<CategorySelection>,
        cx: &mut Context<Self>,
    ) {
        // A rename typed before another row is chosen is kept, as leaving
        // the field would: the field's blur arrives after the selection.
        if self.category != selection && self.section == Section::Categories {
            self.save_category_name(cx);
        }
        self.section = Section::Categories;
        if self.category != selection {
            self.set_category(selection);
        }
        cx.notify();
    }

    pub fn toggle_category(&mut self, id: i64, cx: &mut Context<Self>) {
        let stored = CategorySelection::Stored(id);
        let next = (self.category != Some(stored)).then_some(stored);
        self.select_category(next, cx);
    }

    /// "+ New category": the inspector's form, empty.
    pub fn new_category(&mut self, cx: &mut Context<Self>) {
        self.outcome = None;
        self.select_category(Some(CategorySelection::New), cx);
    }

    /// Changes the category's fields. A stored category is renamed when
    /// `save` (blur, Enter) and the name is valid; a new one waits for
    /// "Create category".
    pub fn edit_category(
        &mut self,
        save: bool,
        change: impl FnOnce(&mut CategoryForm),
        cx: &mut Context<Self>,
    ) {
        let Some(form) = self.category_form.as_mut() else {
            return;
        };
        change(form);
        if save {
            self.save_category_name(cx);
        }
        cx.notify();
    }

    /// Renames the selected stored category when its name is valid and
    /// changed.
    pub fn save_category_name(&mut self, cx: &mut Context<Self>) {
        let Some((summary, _)) = self.selected_category_entry() else {
            return;
        };
        let (id, key) = (summary.category.id(), summary.category.key().clone());
        let stored = summary.category.label().to_owned();
        let Some(form) = self.category_form.as_ref() else {
            return;
        };
        let Ok(label) = form.label(self.category_siblings(), Some(id)) else {
            return;
        };
        if label == stored {
            return;
        }
        let result = self
            .store()
            .and_then(|store| store.rename_category(&key, &label).map_err(message));
        self.after_category_change(result, cx);
    }

    /// "Create category": adds the new category and selects it.
    pub fn create_category(&mut self, cx: &mut Context<Self>) {
        let Some(form) = self.category_form.clone() else {
            return;
        };
        if self.category != Some(CategorySelection::New) || self.category_error().is_some() {
            return;
        }
        let result = self
            .store()
            .and_then(|store| store.add_category(&form.new_category()).map_err(message));
        match result {
            Ok(category) => {
                self.outcome = None;
                self.load();
                self.category_filter = None;
                self.set_category(Some(CategorySelection::Stored(category.id())));
                cx.emit(RulesEvent::Changed);
            }
            Err(message) => self.outcome = Some(Outcome::Refused(message)),
        }
        cx.notify();
    }

    /// Hides the selected category from pickers, or shows it again.
    pub fn set_category_hidden(&mut self, hidden: bool, cx: &mut Context<Self>) {
        let Some((summary, _)) = self.selected_category_entry() else {
            return;
        };
        let key = summary.category.key().clone();
        let result = self
            .store()
            .and_then(|store| store.set_category_hidden(&key, hidden).map_err(message));
        self.after_category_change(result, cx);
    }

    fn after_category_change(&mut self, result: Result<(), SharedString>, cx: &mut Context<Self>) {
        match result {
            Ok(()) => {
                self.outcome = None;
                self.load();
                let selection = self.category;
                self.set_category(selection);
                cx.emit(RulesEvent::Changed);
            }
            Err(message) => self.outcome = Some(Outcome::Refused(message)),
        }
        cx.notify();
    }

    /// Deletes category `id` (the dialog) and selects the row after it.
    pub fn delete_category(&mut self, id: i64, cx: &mut Context<Self>) -> Result<(), SharedString> {
        let key = self
            .data
            .category_entry(id)
            .map(|(summary, _)| summary.category.key().clone())
            .ok_or_else(|| SharedString::from("The category is gone."))?;
        self.store()?.delete_category(&key).map_err(message)?;
        let order: Vec<i64> = self
            .category_rows()
            .iter()
            .map(|row| row.summary.category.id())
            .collect();
        let next = step_in(&order, Some(id), 1)
            .filter(|next| *next != id)
            .or_else(|| step_in(&order, Some(id), -1).filter(|before| *before != id));
        self.load();
        self.set_category(next.map(CategorySelection::Stored));
        self.outcome = None;
        cx.emit(RulesEvent::Changed);
        cx.notify();
        Ok(())
    }
}

/// The item `step` rows from `current` in `order`, stopping at the ends;
/// with nothing selected, down gives the first and up the last.
fn step_in<T: Copy + PartialEq>(order: &[T], current: Option<T>, step: isize) -> Option<T> {
    if order.is_empty() {
        return None;
    }
    let next = match current.and_then(|id| order.iter().position(|row| *row == id)) {
        Some(at) => at.saturating_add_signed(step).min(order.len() - 1),
        None if step < 0 => order.len() - 1,
        None => 0,
    };
    Some(order[next])
}

#[cfg(test)]
mod tests;
