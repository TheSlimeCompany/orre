//! What the Rules view shows, as plain data, for tests (ROADMAP U.3): the
//! panel (sections, filters, counts, the keyboard cursor), the toolbar, the
//! open section's table or the apply preview, the selection, the
//! inspector's header and buttons, the line under the toolbar, the undo
//! bar, the dialog open over the window, and the canvas's keys with whether
//! each does anything now.
//!
//! Compiled for tests only (`#[cfg(test)]` on the module): it isn't in a
//! release build and nothing outside the tests can reach it, `orre mcp`
//! included (`docs/architecture/app.md` → "Decision record: views that
//! describe themselves stay out of the release binary").
//!
//! Every string comes from the code the view draws with
//! ([`super::table`]'s and [`super::inspector`]'s `…Text` types and
//! functions, [`super::panel::PanelRow`], [`super::text`]), so untrusted
//! text (patterns, merchant names, bank text, an agent's rule) is described
//! exactly as it's shown: through [`super::text::plain`]. Styling (colours,
//! fonts, markers, icons, the focus ring on the table) is left out; mixed
//! runs are joined into one string. Change history's WHEN and the change
//! inspector's caption are wall-clock times, described as shown.
//!
//! [`Rules::describe`] reads the [`Rules`] entity alone. The panel's
//! keyboard cursor lives in the window and the dialogs belong to the shell
//! (Delete category to [`super::view::RulesView`]), so [`describe_window`]
//! adds them.
//!
//! What it leaves out: the inspector's fields (When, Then, Order, Match,
//! Category, Used by and their errors: tests read the form), the menus, the
//! tables' captions and lane labels, the change inspector's "What changed"
//! lines and a name's "Bank text it catches", and the panel's own keys
//! (`panel::KEYS`).

use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{App, Entity, SharedString, VisualTestContext};

pub use crate::ui::describe::Action;

use super::dialogs::{
    self, DeleteCategoryDialog, DeleteNameDialog, DeleteRuleDialog, UndoChangeDialog,
};
use super::inspector::{self, HeaderText, NeverReachedText};
use super::panel::{self, PanelRow};
use super::table::{
    self, ANY_ACCOUNT, ApplyText, ChangeRowText, NameRowText, PreviewRowText, RuleRowText,
    ToolbarCaption, ToolbarText,
};
use super::text::{Part, ThenLead};
use super::view::{self, KEYS};
use super::{CategorySelection, NameSelection, Outcome, Rules, Section};
use crate::shell::ShellView;
use crate::theme::focus::RING_SELECTOR;
use crate::theme::tokens::color;

/// What the Rules view shows ([`Rules::describe`], [`describe_window`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RulesDescription {
    pub panel: PanelDescription,
    pub toolbar: ToolbarDescription,
    /// What core refused, under the toolbar.
    pub refused: Option<String>,
    pub canvas: CanvasDescription,
    /// The selected row's first lane: a rule's WHEN, a name's BANK TEXT, a
    /// change, a category's label.
    pub selected: Option<String>,
    pub inspector: InspectorDescription,
    /// The undo bar at the foot of the canvas.
    pub bar: Option<BarDescription>,
    /// The dialog open over the window ([`describe_window`] only).
    pub dialog: Option<DialogDescription>,
    /// The canvas's keys ([`view::KEYS`]: its own, then ↑↓), and whether
    /// each does anything now. ⌥↑⌥↓ also need the table to have focus,
    /// which this leaves out.
    pub actions: Vec<Action>,
}

/// The left panel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PanelDescription {
    /// Rules, Merchant names, Change history, Categories: the open one is
    /// selected.
    pub sections: Vec<PanelRowDescription>,
    /// The open section's filters' title ("Action") and rows: one that's
    /// on is selected.
    pub filter_title: &'static str,
    pub filters: Vec<PanelRowDescription>,
    /// The row the keyboard cursor is on while the panel has keyboard
    /// focus ([`describe_window`] only).
    pub cursor: Option<&'static str>,
    /// Why nothing could be read, under the filters.
    pub load_error: Option<String>,
}

/// A row of the panel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PanelRowDescription {
    pub label: &'static str,
    pub count: usize,
    pub selected: bool,
}

/// The toolbar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolbarDescription {
    pub title: &'static str,
    pub caption: String,
    /// The key hints, cap and label ([`view::KEYS`]).
    pub keys: Vec<(&'static str, &'static str)>,
    /// "Back to rules" in the preview, the section's New button.
    pub buttons: Vec<Action>,
}

/// The canvas under the toolbar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CanvasDescription {
    /// The open section is empty: the title and caption.
    Empty {
        title: &'static str,
        caption: &'static str,
    },
    Rules(Vec<RuleRowDescription>),
    /// Apply to past transactions.
    Preview(PreviewDescription),
    Names(Vec<NameRowDescription>),
    History(Vec<ChangeRowDescription>),
    Categories(Vec<CategoryRowDescription>),
}

/// A rule's row, lane by lane.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuleRowDescription {
    pub number: usize,
    /// "Merchant contains UBER · over £10".
    pub when: String,
    pub account: String,
    /// "Eating out + Tag", "Tag Gift Aid".
    pub then: String,
    pub matches: String,
    /// MATCHES says "Never reached" before its 0.
    pub never_reached: bool,
    pub selected: bool,
}

/// The apply preview.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreviewDescription {
    /// "Will change".
    pub will_change: Vec<PreviewRowDescription>,
    /// "Category you set", when there are any, and whether "Change these
    /// too" is on.
    pub user_set: Option<(Vec<PreviewRowDescription>, bool)>,
    /// The line in place of rows.
    pub message: Option<&'static str>,
    /// "Apply to 4 transactions", or "Create rule and change 4" and "Create
    /// rule only".
    pub actions: Vec<Action>,
}

/// A preview row, lane by lane.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreviewRowDescription {
    pub merchant: String,
    pub account: String,
    pub now: String,
    pub after: String,
    pub date: String,
    pub amount: String,
}

/// A merchant name's row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NameRowDescription {
    /// "Starts with AMZN MKTP".
    pub bank_text: String,
    pub shows_as: String,
    pub last_seen: String,
    pub matches: String,
    pub selected: bool,
}

/// A change's row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangeRowDescription {
    /// "9 Nov 14:05": wall-clock time.
    pub when: String,
    pub change: String,
    /// The kind, or "Undone 28 Oct".
    pub kind: String,
    pub transactions: String,
    /// Struck through.
    pub undone: bool,
    pub selected: bool,
}

/// A category's row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CategoryRowDescription {
    pub label: String,
    /// Indented under its group.
    pub subcategory: bool,
    /// Muted, with "Hidden" after the label.
    pub hidden: bool,
    pub kind: &'static str,
    pub rules: usize,
    pub transactions: usize,
    pub selected: bool,
}

/// The inspector.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InspectorDescription {
    /// Nothing is selected: what it says instead.
    Nothing(&'static str),
    Item(Box<ItemDescription>),
}

/// The inspector for the selected rule, name, change or category.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ItemDescription {
    pub title: String,
    pub figure: Option<String>,
    pub caption: Option<String>,
    /// A never-reached rule's card: the rule that catches its matches.
    pub never_reached: Option<String>,
    /// An agent's pending rule that would take over this rule's rows.
    pub suggestion: Option<String>,
    /// The footer's line: "Created on 12 Mar", "Key food.coffee".
    pub footer: Option<String>,
    /// Its buttons, in the order drawn.
    pub actions: Vec<Action>,
}

/// The undo bar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BarDescription {
    /// "Changed 1 transaction to Eating out", "Moved “Merchant contains
    /// UBER TRIP” to rule 2 of 14".
    pub message: String,
    /// Whether it offers Undo.
    pub undo: bool,
}

/// A dialog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DialogDescription {
    pub title: String,
    pub body: String,
    /// What core refused, under the body.
    pub error: Option<String>,
    /// Cancel, then the confirming button.
    pub actions: Vec<Action>,
}

impl DialogDescription {
    /// A Rules dialog: Cancel and `confirm` ([`super::dialogs`]).
    pub fn new(title: &str, body: &str, error: Option<&SharedString>, confirm: &str) -> Self {
        Self {
            title: title.to_owned(),
            body: body.to_owned(),
            error: error.map(ToString::to_string),
            actions: vec![
                Action::button(dialogs::CANCEL, true),
                Action::button(confirm, true),
            ],
        }
    }
}

/// Runs joined into the line they're drawn as.
fn joined(parts: Vec<Part>) -> String {
    parts.into_iter().map(|part| part.text).collect()
}

fn panel(rules: &Rules) -> PanelDescription {
    let row = |row: PanelRow| PanelRowDescription {
        label: row.label(),
        count: row.count(rules),
        selected: row.selected(rules),
    };
    let open = rules.section();
    let (sections, filters): (Vec<PanelRow>, Vec<PanelRow>) = PanelRow::all(open)
        .into_iter()
        .partition(|row| matches!(row, PanelRow::Section(_)));
    PanelDescription {
        sections: sections.into_iter().map(row).collect(),
        filter_title: panel::filter_title(open),
        filters: filters.into_iter().map(row).collect(),
        cursor: None,
        load_error: rules.load_error().map(ToString::to_string),
    }
}

fn toolbar(rules: &Rules) -> ToolbarDescription {
    let ToolbarText {
        back,
        title,
        caption,
        hints,
        button,
    } = ToolbarText::of(rules);
    ToolbarDescription {
        title,
        caption: match caption {
            ToolbarCaption::Plain(words) => words,
            ToolbarCaption::Rich(parts) => joined(parts),
        },
        keys: if hints {
            KEYS.hints()
                .into_iter()
                .map(|hint| (hint.cap, hint.label))
                .collect()
        } else {
            Vec::new()
        },
        buttons: back
            .then_some(table::BACK_TO_RULES)
            .into_iter()
            .chain(button)
            .map(|label| Action::button(label, true))
            .collect(),
    }
}

fn rule_row(rules: &Rules, summary: &orre_core::rule::RuleSummary) -> RuleRowDescription {
    let data = rules.data();
    let RuleRowText {
        number,
        when,
        account,
        then,
        then_suffix,
        never_reached,
        matches,
    } = RuleRowText::of(data, summary, color::TEXT);
    let lead = match then {
        ThenLead::Category(key) => table::category_label(data, &key),
        ThenLead::Action { word, value, .. } => format!("{word} {value}"),
    };
    RuleRowDescription {
        number,
        when: joined(when),
        account: account.unwrap_or_else(|| ANY_ACCOUNT.to_owned()),
        then: lead + &then_suffix,
        matches,
        never_reached,
        selected: rules.selected_rule_id() == Some(summary.rule.id()),
    }
}

fn preview(rules: &Rules, apply: &super::Apply) -> PreviewDescription {
    let data = rules.data();
    let today = rules.today();
    let row = |row: &orre_core::rule::PreviewRow| {
        let text = PreviewRowText::of(data, row, today, apply.include);
        PreviewRowDescription {
            merchant: text.merchant,
            account: text.account,
            now: text.now.1,
            after: text.after.1,
            date: text.date,
            amount: text.amount,
        }
    };
    let user_set: Vec<PreviewRowDescription> = apply.user_set().map(row).collect();
    let ApplyText {
        message,
        commit: (commit, can_commit),
        create_only,
    } = ApplyText::of(rules, apply);
    PreviewDescription {
        will_change: apply.will_change().map(row).collect(),
        user_set: (!user_set.is_empty()).then_some((user_set, apply.include)),
        message,
        actions: [Action::button(commit, can_commit)]
            .into_iter()
            .chain(create_only.map(|(label, enabled)| Action::button(label, enabled)))
            .collect(),
    }
}

fn canvas(rules: &Rules) -> CanvasDescription {
    if let Some((_, title, caption)) = table::empty_text(rules) {
        return CanvasDescription::Empty { title, caption };
    }
    let data = rules.data();
    let today = rules.today();
    match (rules.section(), rules.apply()) {
        (Section::Rules, Some(apply)) => CanvasDescription::Preview(preview(rules, apply)),
        (Section::Rules, None) => CanvasDescription::Rules(
            rules
                .rules()
                .into_iter()
                .map(|summary| rule_row(rules, summary))
                .collect(),
        ),
        (Section::Names, _) => CanvasDescription::Names(
            rules
                .names()
                .into_iter()
                .map(|summary| {
                    let text = NameRowText::of(summary, today, color::TEXT);
                    NameRowDescription {
                        bank_text: joined(text.bank),
                        shows_as: text.shows_as,
                        last_seen: text.last_seen,
                        matches: text.matches,
                        selected: rules.selected_name()
                            == Some(NameSelection::Stored(summary.alias.id())),
                    }
                })
                .collect(),
        ),
        (Section::History, _) => CanvasDescription::History(
            rules
                .history()
                .into_iter()
                .map(|entry| {
                    let text = ChangeRowText::of(data, entry, today);
                    ChangeRowDescription {
                        when: text.when,
                        change: joined(text.change),
                        kind: text.kind,
                        transactions: text.transactions,
                        undone: text.undone,
                        selected: rules.selected_batch().map(|e| e.batch.id)
                            == Some(entry.batch.id),
                    }
                })
                .collect(),
        ),
        (Section::Categories, _) => CanvasDescription::Categories(
            rules
                .category_rows()
                .iter()
                .map(|row| {
                    let category = &row.summary.category;
                    CategoryRowDescription {
                        label: category.label().to_owned(),
                        subcategory: row.child,
                        hidden: row.hidden,
                        kind: super::text::category_kind(category.kind()),
                        rules: row.summary.usage.rules,
                        transactions: table::filed(&row.summary.usage),
                        selected: rules.selected_category()
                            == Some(CategorySelection::Stored(category.id())),
                    }
                })
                .collect(),
        ),
    }
}

/// The selected row's first lane, as [`canvas`] describes it.
fn selected(canvas: &CanvasDescription) -> Option<String> {
    match canvas {
        CanvasDescription::Rules(rows) => rows.iter().find(|r| r.selected).map(|r| r.when.clone()),
        CanvasDescription::Names(rows) => rows
            .iter()
            .find(|r| r.selected)
            .map(|r| r.bank_text.clone()),
        CanvasDescription::History(rows) => {
            rows.iter().find(|r| r.selected).map(|r| r.change.clone())
        }
        CanvasDescription::Categories(rows) => {
            rows.iter().find(|r| r.selected).map(|r| r.label.clone())
        }
        CanvasDescription::Empty { .. } | CanvasDescription::Preview(_) => None,
    }
}

fn item(header: HeaderText) -> ItemDescription {
    let HeaderText {
        title,
        figure,
        caption,
    } = header;
    ItemDescription {
        title: joined(title),
        figure: figure.map(|(figure, _)| figure),
        caption,
        never_reached: None,
        suggestion: None,
        footer: None,
        actions: Vec::new(),
    }
}

fn footer(lead: &str, date: Option<String>) -> Option<String> {
    date.map(|date| format!("{lead}{date}"))
}

fn rule_inspector(rules: &Rules) -> InspectorDescription {
    if rules.form().is_none() {
        return InspectorDescription::Nothing(inspector::NO_RULE);
    }
    let Some(header) = inspector::rule_header(rules) else {
        return InspectorDescription::Nothing(inspector::NO_RULE);
    };
    let mut item = item(header);
    let data = rules.data();
    if let Some(summary) = rules.selected_rule() {
        if let Some(card) = NeverReachedText::of(data, summary) {
            item.never_reached = Some(joined(card.body));
            item.actions.extend([
                Action::button(inspector::move_above(card.position), true),
                Action::button(inspector::show_rule(card.position), true),
            ]);
        }
        item.actions.push(Action::button(
            inspector::APPLY_TO_PAST,
            rules.apply().is_none(),
        ));
        if let Some(suggestion) = rules.touching().first() {
            item.suggestion = Some(joined(inspector::touching_parts(suggestion)));
            item.actions
                .push(Action::button("Review in the Inbox", true));
        }
        let (lead, date) = inspector::rule_created(summary, data, rules.today());
        item.footer = footer(lead, date);
        item.actions
            .push(Action::button(inspector::DELETE_RULE, true));
    }
    InspectorDescription::Item(Box::new(item))
}

fn name_inspector(rules: &Rules) -> InspectorDescription {
    let Some(header) = inspector::name_header(rules) else {
        return InspectorDescription::Nothing(inspector::NO_NAME);
    };
    let mut item = item(header);
    if let Some(NameSelection::Stored(id)) = rules.selected_name()
        && let Some(summary) = rules.data().name(id)
    {
        item.footer = footer(
            "Added on ",
            inspector::name_added(rules.data(), summary, rules.today()),
        );
        item.actions
            .push(Action::button(inspector::DELETE_NAME, true));
    }
    InspectorDescription::Item(Box::new(item))
}

fn change_inspector(rules: &Rules) -> InspectorDescription {
    let (Some(entry), Some(header)) = (rules.selected_batch(), inspector::change_header(rules))
    else {
        return InspectorDescription::Nothing(inspector::NO_CHANGE);
    };
    let mut item = item(header);
    if entry.batch.status != orre_core::change::BatchStatus::Undone {
        item.actions
            .push(Action::button(inspector::UNDO_CHANGE, true));
    }
    InspectorDescription::Item(Box::new(item))
}

fn category_inspector(rules: &Rules) -> InspectorDescription {
    match rules.selected_category() {
        None => InspectorDescription::Nothing(inspector::NO_CATEGORY),
        Some(CategorySelection::New) => {
            let Some(form) = rules.category_form() else {
                return InspectorDescription::Item(Box::default());
            };
            let mut item = item(inspector::new_category_header(form));
            item.actions.push(Action::button(
                inspector::CREATE_CATEGORY,
                rules.category_error().is_none(),
            ));
            InspectorDescription::Item(Box::new(item))
        }
        Some(CategorySelection::Stored(_)) => {
            let (Some((summary, _)), Some(header)) = (
                rules.selected_category_entry(),
                inspector::stored_category_header(rules),
            ) else {
                return InspectorDescription::Nothing(inspector::CATEGORY_GONE);
            };
            let mut item = item(header);
            item.footer = Some(format!("Key {}", summary.category.key().as_str()));
            item.actions.push(Action::button(
                inspector::hide_label(&summary.category),
                true,
            ));
            if inspector::deletable(rules, summary) {
                item.actions
                    .push(Action::button(inspector::DELETE_CATEGORY, true));
            }
            InspectorDescription::Item(Box::new(item))
        }
    }
}

fn bar(rules: &Rules) -> Option<BarDescription> {
    match rules.outcome()? {
        Outcome::Done { message, undo } => Some(BarDescription {
            message: message.to_string(),
            undo: undo.is_some(),
        }),
        Outcome::Moved(_) => Some(BarDescription {
            message: joined(rules.moved_parts()?),
            undo: true,
        }),
        Outcome::Refused(_) => None,
    }
}

/// Whether ⌥↑ and ⌥↓ would move the selected rule: a stored rule selected
/// in the table (not the preview), with a row shown above or below it.
fn reorder_room(rules: &Rules) -> (bool, bool) {
    if rules.section() != Section::Rules || rules.apply().is_some() {
        return (false, false);
    }
    let Some(id) = rules.selected_rule_id() else {
        return (false, false);
    };
    let shown: Vec<_> = rules.rules().iter().map(|s| s.rule.id()).collect();
    match shown.iter().position(|rule| *rule == id) {
        Some(at) => (at > 0, at + 1 < shown.len()),
        None => (false, false),
    }
}

/// The canvas's keys, from [`view::KEYS`]: ⌥↑⌥↓ while the selected rule
/// has room to move, Escape in the preview or while a row is selected, ↑↓
/// while the open section lists rows (not in the preview).
fn actions(rules: &Rules) -> Vec<Action> {
    let preview = rules.apply().is_some();
    let (has_rows, has_selection) = match rules.section() {
        Section::Rules => (
            !rules.rules().is_empty(),
            rules.selected_rule_id().is_some(),
        ),
        Section::Names => (!rules.names().is_empty(), rules.selected_name().is_some()),
        Section::History => (
            !rules.history().is_empty(),
            rules.selected_batch().is_some(),
        ),
        Section::Categories => (
            !rules.category_rows().is_empty(),
            rules.selected_category().is_some(),
        ),
    };
    let (up, down) = reorder_room(rules);
    let own = KEYS.keys().iter().flat_map(|key| {
        let (first, second) = if key.label() == view::REORDER.label() {
            (up, down)
        } else {
            assert_eq!(
                key.label(),
                view::BACK.label(),
                "say when the Rules view's new key works"
            );
            let back = preview || has_selection;
            (back, back)
        };
        let first = key
            .keys()
            .iter()
            .map(move |stroke| Action::keyed(key.label(), stroke, first));
        let second = key
            .and_keys()
            .iter()
            .map(move |stroke| Action::keyed(key.label(), stroke, second));
        first.chain(second)
    });
    let moves = KEYS
        .list()
        .moves()
        .into_iter()
        .map(|(stroke, to)| Action::keyed(to.label(), stroke, has_rows && !preview));
    own.chain(moves).collect()
}

impl Rules {
    /// What the Rules view shows now, as plain data (module docs). The
    /// panel's cursor and any dialog are [`describe_window`]'s.
    pub fn describe(&self) -> RulesDescription {
        let canvas = canvas(self);
        RulesDescription {
            panel: panel(self),
            toolbar: toolbar(self),
            refused: match self.outcome() {
                Some(Outcome::Refused(message)) => Some(message.to_string()),
                _ => None,
            },
            selected: selected(&canvas),
            canvas,
            inspector: match self.section() {
                Section::Rules => rule_inspector(self),
                Section::Names => name_inspector(self),
                Section::History => change_inspector(self),
                Section::Categories => category_inspector(self),
            },
            bar: bar(self),
            dialog: None,
            actions: actions(self),
        }
    }
}

/// [`Rules::describe`] as `shell`'s window shows it now: with the panel's
/// keyboard cursor (the panel row drawing the focus ring) and the dialog
/// open over the window, the shell's or the Rules view's own.
pub fn describe_window(shell: &Entity<ShellView>, cx: &mut VisualTestContext) -> RulesDescription {
    let (mut described, open) = cx.update(|_, cx| {
        let shell = shell.read(cx);
        let rules = shell.rules().read(cx);
        let mut described = rules.describe();
        described.dialog = dialog(shell, cx);
        (described, rules.section())
    });
    described.panel.cursor = cursor(open, cx);
    described
}

fn dialog(shell: &ShellView, cx: &App) -> Option<DialogDescription> {
    if let Some(open) = shell.dialog() {
        let open = open.clone();
        let open = match open.downcast::<DeleteRuleDialog>() {
            Ok(dialog) => return Some(dialog.read(cx).describe()),
            Err(open) => open,
        };
        let open = match open.downcast::<DeleteNameDialog>() {
            Ok(dialog) => return Some(dialog.read(cx).describe()),
            Err(open) => open,
        };
        if let Ok(dialog) = open.downcast::<UndoChangeDialog>() {
            return Some(dialog.read(cx).describe());
        }
    }
    let view = shell.rules_view().read(cx);
    view.delete_category_dialog()
        .map(|dialog: &Entity<DeleteCategoryDialog>| dialog.read(cx).describe())
}

/// The panel row drawing the focus ring: the keyboard cursor, drawn while
/// the panel has keyboard focus.
fn cursor(open: Section, cx: &mut VisualTestContext) -> Option<&'static str> {
    let ring = cx.debug_bounds(RING_SELECTOR)?;
    PanelRow::all(open)
        .into_iter()
        .find(|row| {
            let id = row.id();
            cx.update(move |window, _| window.try_find(id).map(|row| row.bounds())) == Some(ring)
        })
        .map(PanelRow::label)
}
