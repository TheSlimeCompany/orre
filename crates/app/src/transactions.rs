//! The Transactions screen (ROADMAP M3.3; `docs/DESIGN.md` → "Transactions"
//! and "Data table (Transactions)"; Paper `18E-0`, `1NP-0`; `ARCHITECTURE.md`
//! → "Decision record: the Transactions view").
//!
//! The view draws the canvas (toolbar, column header, the day-grouped table,
//! and the bulk-action bar or the undo bar) and the inspector. The app shell
//! ([`crate::shell`]) puts the title bar, the accounts panel and the status
//! bar around it.
//!
//! - **Data.** The [`Register`] (core's read model: every transaction, with
//!   its accounts and categories) is read from the database on a background
//!   thread ([`store::TransactionStore`]), at start and after each change.
//!   Filtering, sorting and grouping are core's [`Register::select`], pure
//!   and in memory, so a keystroke in the search field never reads the
//!   database. Without a database (snapshots, tests) the view shows the
//!   Paper fixture ([`fixture::register`]), which can't change.
//! - **Filters.** The accounts panel's selection is the account filter (the
//!   Account chip mirrors it), and hides the Account column. The Category
//!   and Date chips open menus; Uncategorised toggles. Search matches the
//!   merchant, the bank's merchant and the raw description.
//! - **Sort.** Clicking a column title sorts each day's rows by it, then
//!   reverses, then goes back to newest first. Days never move.
//! - **Category edits.** Enter or a double-click on a row's category opens
//!   the category menu in place; the inspector's Category select and the
//!   bulk-action bar's Categorise set it for every selected row. Each goes
//!   through core as one undoable change batch; after a change to several
//!   rows the undo bar offers to revert it.
//! - **Splits, tags, notes and names** (M3.9) are edited in the inspector
//!   (`transactions::editor`): each change is one undoable change batch too, and the
//!   register is read again so the table shows it.
//! - **Excluded rows, the Source section and manual rows** (M3.10): the
//!   Excluded chip shows excluded rows in their days (they never count in
//!   a day's net); the inspector's Source section ([`source`]) excludes,
//!   undoes the import or deletes a manual row through dialogs the shell
//!   opens ([`exclude_dialog`], [`confirm`]); ⌘N and a cash account's "New
//!   transaction" open [`new_transaction`]; a manual row's Date, Amount and
//!   Account are fields (`transactions::manual_edit`). After excluding, the
//!   undo bar offers to include the row again.
//! - **Every change** goes through the edit pipeline (`transactions::write`):
//!   sample data refused, the error said where the change was made, the
//!   undo bar, the register read again and the shell told, in one place.
//!
//! The table is a GPUI `list`: virtualised, with rows of their own heights
//! (day headers carry 4px more than a row when they draw a divider). The
//! view owns the selection (by transaction id, so it survives filtering)
//! and the list's scroll state; rows only present (`ui::transaction_row`).

pub mod confirm;
mod editor;
pub mod exclude_dialog;
pub mod fixture;
mod inspector;
pub mod link_dialog;
pub mod link_fixture;
pub mod linking;
pub mod links;
mod manual_edit;
pub mod new_transaction;
pub mod present;
pub mod rule_prompt;
pub mod selection;
pub mod source;
pub mod store;
pub mod transfer;
mod write;

use chrono::NaiveDate;
use coffer_core::account::AccountKind;
use coffer_core::change;
use coffer_core::exclusion::{self, ExclusionReason};
use coffer_core::id::{AccountId, BatchId, ChangeBatchId, TransactionId};
use coffer_core::import::{self, UndoPreview};
use coffer_core::inbox::own;
use coffer_core::manual::{self, ManualTransaction};
use coffer_core::note::UserNote;
use coffer_core::reporting::register::{
    CategoryCounts, DatePreset, Filter, Listing, Register, RegisterRow, Sort, SortColumn,
};
use coffer_core::transfer as core_transfer;
use gpui_kit::base::actions::Cancel;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    AnyElement, App, AppContext as _, ClickEvent, Context, Div, Entity, FocusHandle,
    InteractiveElement as _, IntoElement, KeyBinding, ListAlignment, ListState, MouseButton,
    MouseDownEvent, ParentElement as _, Render, SharedString, Styled as _, Subscription, Task,
    Window, div, list,
};

use crate::accounts::CategoryPick;
use crate::accounts::inspector::AccountInspector;
use crate::accounts::panel::{PanelCategories, PanelCategory};
use crate::accounts::store::StoreError;
use crate::accounts::{Accounts, AccountsEvent};
use crate::format::{self, Sign};
use crate::theme::tokens::{color, size as token_size, spacing};
use crate::ui::bulk_action_bar::BulkActionBar;
use crate::ui::button::{Button, ButtonVariant, Chip};
use crate::ui::empty_state::EmptyState;
use crate::ui::field::search_input;
use crate::ui::list::{Keys, ListKeysExt as _, Move, Reveal as _};
use crate::ui::menu::{self, MenuGroup, MenuList, SelectState, filter_groups};
use crate::ui::notification::NotificationBar;
use crate::ui::transaction_row::{
    CategoryCell, DayHeader, TransactionColumn, TransactionHeader, TransactionRow,
};
use crate::ui::undo_bar::UndoBar;

use selection::{ClickKind, Selection};
use store::TransactionStore;
use write::{Offer, Policy, Said};

/// The key context of the canvas, where the arrow keys move the selection.
const TABLE_CONTEXT: &str = "CofferTransactionsTable";

/// The table's movement keys: ↑↓, ⇧↑⇧↓ to extend the selection, Home and
/// End.
const TABLE_KEYS: Keys = Keys::new(TABLE_CONTEXT).with_extend().with_ends();

/// Menus taller than eight options scroll (`DESIGN.md` → "Accounts panel" →
/// "Menus").
const MENU_ROWS: f32 = 8.;

gpui_kit::actions!(
    coffer_transactions,
    [
        /// Enter: open the category menu on the keyboard row.
        EditCategory,
    ]
);

/// Binds the table's and the menus' keys. Call once, before the first view
/// is created.
pub fn init(cx: &mut App) {
    TABLE_KEYS.bind(cx);
    cx.bind_keys([
        KeyBinding::new("enter", EditCategory, Some(TABLE_CONTEXT)),
        KeyBinding::new("escape", Cancel, Some(TABLE_CONTEXT)),
    ]);
    menu::init(cx);
}

/// Where the transactions come from.
pub enum Source {
    /// The database: what `coffer` opens.
    Database(TransactionStore),
    /// The Paper fixture, which can't change.
    Fixture,
}

/// What the Transactions screen asks its owner (the shell) to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TransactionsEvent {
    /// "Create rule…": open Rules on the preview with this rule, not saved
    /// (M3.13).
    CreateRule(
        coffer_core::rule::RuleDefinition,
        coffer_core::rule::RuleOrigin,
    ),
    /// "Open rule": show this rule in Rules (M3.13).
    OpenRule(coffer_core::id::RuleId),
    /// "Link…" or "Link a payment…": open the link dialog.
    LinkRequested(links::LinkFrom),
    /// Links or shares changed: Owed to you reads them again.
    Changed,
    /// "Exclude…" or "Edit reason…": open the Exclude dialog (M3.10).
    ExcludeRequested(TransactionId),
    /// "Undo import…": open the undo confirmation for the batch (M3.10).
    UndoImportRequested(BatchId),
    /// "Delete transaction…" on a manual row: open its confirmation
    /// (M3.10).
    DeleteRequested(TransactionId),
    /// Rows were added, removed or moved, or left or rejoined a balance
    /// (M3.10): the accounts panel and the checks read again.
    LedgerChanged,
}

impl gpui_kit::EventEmitter<TransactionsEvent> for TransactionsView {}

/// Which menu is open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Menu {
    /// The Account chip's.
    Account,
    /// The Category chip's.
    Category,
    /// The Date chip's.
    Date,
    /// The category select in place on a row.
    Row(TransactionId),
    /// The inspector's Category select.
    Inspector,
    /// The bulk-action bar's Categorise.
    Bulk,
    /// The inspector's tag menu, from "+ Tag".
    Tags,
    /// A split line's category select, by the line's key.
    SplitLine(u64),
    /// The Share section's Owed by select (M3.11).
    Person,
    /// A manual row's Account select (M3.10).
    ManualAccount,
}

impl Menu {
    /// Whether it has the filter field: the category and tag menus.
    fn has_filter(self) -> bool {
        !matches!(self, Menu::Account | Menu::Date | Menu::ManualAccount)
    }

    /// Whether it's a select field's menu, with a state of its own.
    fn is_field_select(self) -> bool {
        matches!(
            self,
            Menu::Inspector | Menu::SplitLine(_) | Menu::Person | Menu::ManualAccount
        )
    }
}

/// One entry of the table's list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TableItem {
    /// A day's header, by index into [`Listing::days`]; `first` has no
    /// divider above it.
    Day { day: usize, first: bool },
    /// A transaction, by index into [`Register::rows`].
    Row(usize),
}

/// The list's entries: each day's header, then its rows.
fn table_items(listing: &Listing) -> Vec<TableItem> {
    let mut items = Vec::with_capacity(listing.rows.len() + listing.days.len());
    for (day, entry) in listing.days.iter().enumerate() {
        items.push(TableItem::Day {
            day,
            first: day == 0,
        });
        items.extend(
            listing.rows[entry.rows.clone()]
                .iter()
                .map(|&row| TableItem::Row(row)),
        );
    }
    items
}

fn sort_column(column: TransactionColumn) -> SortColumn {
    match column {
        TransactionColumn::Merchant => SortColumn::Merchant,
        TransactionColumn::Category => SortColumn::Category,
        TransactionColumn::Account => SortColumn::Account,
        TransactionColumn::Amount => SortColumn::Amount,
    }
}

fn table_column(column: SortColumn) -> TransactionColumn {
    match column {
        SortColumn::Merchant => TransactionColumn::Merchant,
        SortColumn::Category => TransactionColumn::Category,
        SortColumn::Account => TransactionColumn::Account,
        SortColumn::Amount => TransactionColumn::Amount,
    }
}

/// What the undo bar's Undo does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UndoAction {
    /// Reverts a change batch.
    Batch(ChangeBatchId),
    /// Includes an excluded row again: exclusion isn't a change batch
    /// ("Decision record: Transactions additions", point 1).
    Include(TransactionId),
    /// Reverses a transfer-pair decision: a status change, not a change
    /// batch (M3.8).
    Transfer(core_transfer::Decision),
}

/// A change the undo bar can revert.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Undo {
    action: UndoAction,
    message: SharedString,
}

/// An edit that didn't save on a row the user had already left (its
/// fields were saved as the selection moved): the inspector no longer shows
/// that row, so the failure bar says so, and its Show goes back to the row
/// with the reason said where the inspector says it.
#[derive(Clone, Debug, PartialEq, Eq)]
struct EditFailure {
    row: TransactionId,
    place: editor::EditPlace,
    reason: SharedString,
}

/// What the status bar counts (M3.10): "328 transactions · 2 excluded".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    /// Transactions that count: every row but the excluded ones.
    pub transactions: usize,
    pub excluded: usize,
    /// The sample data: the status bar keeps the artboard's figure.
    pub sample: bool,
}

/// The Transactions screen.
pub struct TransactionsView {
    source: Source,
    register: Register,
    /// What each category group holds, counted when the register is read:
    /// the accounts panel's CATEGORIES.
    category_counts: CategoryCounts,
    /// Whether the register has been read once: until then the table is
    /// blank rather than "No transactions yet".
    loaded: bool,
    load_error: Option<SharedString>,
    /// The read in flight; replacing it drops a stale one.
    loading: Option<Task<()>>,
    /// How many times the register has been read: the editor refreshes
    /// its fields when it moves.
    generation: u64,
    today: NaiveDate,
    filter: Filter,
    date: DatePreset,
    sort: Sort,
    listing: Listing,
    items: Vec<TableItem>,
    /// Transaction ids in display order, for ranges and keyboard moves.
    order: Vec<TransactionId>,
    list: ListState,
    selection: Selection<TransactionId>,
    focus: FocusHandle,
    search: Entity<InputState>,
    menu: Option<Menu>,
    /// The popup menus' state (chips, the row, the bulk-action bar).
    menu_state: SelectState,
    /// The inspector's Category select.
    inspector_select: SelectState,
    menu_filter: Entity<InputState>,
    chip_focus: [FocusHandle; 3],
    /// Why the last change wasn't saved, said under the inspector's Category.
    edit_error: Option<SharedString>,
    /// The inspector's fields for the selected transaction (M3.9).
    editor: editor::Editor,
    undo: Option<Undo>,
    /// The accounts panel's state and the account inspector, shown while an
    /// account is selected and no transaction is. Set by the shell.
    accounts: Option<(Entity<Accounts>, Entity<AccountInspector>)>,
    /// A row just added: selected and scrolled into view once the register
    /// has it, the filters that hide it cleared.
    reveal: Option<TransactionId>,
    /// The filters were changed without a window to set the search field
    /// with: it's set from `filter.text` at the next render.
    search_stale: bool,
    /// An edit typed on a row the user then left, which didn't save: the
    /// failure bar says so until it's dismissed or the selection moves.
    edit_failure: Option<EditFailure>,
    _subscriptions: Vec<Subscription>,
}

impl TransactionsView {
    /// The screen over `source`. The fixture opens with the Paper
    /// artboard's selection, the Amazon payment; the database is read in
    /// the background.
    pub fn new(source: Source, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search transactions"));
        let menu_filter = cx.new(|cx| InputState::new(window, cx).placeholder("Find a category"));
        let (editor, editor_subscriptions) = editor::Editor::new(window, cx);
        let mut subscriptions = vec![
            cx.subscribe_in(&search, window, |this, input, event, _, cx| {
                if matches!(event, InputEvent::Change) {
                    this.filter.text = input.read(cx).value().to_string();
                    this.refilter(cx);
                }
            }),
            cx.subscribe_in(&menu_filter, window, |this, _, event, _, cx| {
                if matches!(event, InputEvent::Change) {
                    let groups = this.visible_groups(cx);
                    if let Some(menu) = this.menu
                        && let Some(state) = this.select_state(menu)
                    {
                        state.highlight_first(&groups);
                    }
                    cx.notify();
                }
            }),
        ];
        subscriptions.extend(editor_subscriptions);
        let fixture = matches!(source, Source::Fixture);
        let today = if fixture {
            crate::accounts::fixture::today()
        } else {
            coffer_core::schedule::local_today()
                .unwrap_or_else(|_| crate::accounts::fixture::today())
        };
        let mut view = Self {
            source,
            register: Register::default(),
            category_counts: CategoryCounts::default(),
            loaded: false,
            load_error: None,
            loading: None,
            generation: 0,
            today,
            filter: Filter::default(),
            date: DatePreset::AllTime,
            sort: Sort::Default,
            listing: Listing::default(),
            items: Vec::new(),
            order: Vec::new(),
            // Ten rows past each edge, so a fast scroll doesn't show blanks.
            list: ListState::new(0, ListAlignment::Top, token_size::CONTROL * 10.),
            selection: Selection::default(),
            menu_state: SelectState::with_trigger(focus.clone(), cx),
            inspector_select: SelectState::new(cx),
            chip_focus: [cx.focus_handle(), cx.focus_handle(), cx.focus_handle()],
            focus,
            search,
            menu: None,
            menu_filter,
            edit_error: None,
            editor,
            undo: None,
            accounts: None,
            reveal: None,
            search_stale: false,
            edit_failure: None,
            _subscriptions: subscriptions,
        };
        if fixture {
            view.apply(Ok(fixture::register()), cx);
            view.selection.set([fixture::id("txn-amazon-09")]);
        } else {
            view.reload(cx);
        }
        view
    }

    /// Filters by `accounts`' selection, and shows its selected account in
    /// the inspector while no transaction is selected (`DESIGN.md` →
    /// "Account inspector").
    pub fn show_accounts(
        &mut self,
        accounts: Entity<Accounts>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let inspector = cx.new(|cx| AccountInspector::new(accounts.clone(), window, cx));
        self._subscriptions
            .push(cx.subscribe(&accounts, |this, _, event, cx| {
                if let AccountsEvent::Selected(account) = event {
                    this.set_account(*account, cx);
                }
            }));
        self.filter.account = accounts.read(cx).selected();
        self.accounts = Some((accounts, inspector));
        self.refilter(cx);
    }

    /// The account inspector to show instead of an empty one: set when an
    /// account is selected.
    pub fn account_inspector(&self, cx: &App) -> Option<Entity<AccountInspector>> {
        let (accounts, inspector) = self.accounts.as_ref()?;
        accounts.read(cx).selected().map(|_| inspector.clone())
    }

    /// The valuations canvas to show instead of the table: set while an
    /// asset or liability account is selected, which holds values, not
    /// transactions (M3.17, `DESIGN.md` → "Accounts you keep yourself").
    fn valuations_canvas(&self, cx: &App) -> Option<AnyElement> {
        let (_, inspector) = self.accounts.as_ref()?;
        let canvas = inspector.read(cx).valuations(cx)?;
        Some(canvas.into_any_element())
    }

    /// Clears the selection, so the inspector shows the selected account.
    pub fn clear_selection(&mut self, cx: &mut Context<Self>) {
        self.selection.clear();
        self.undo = None;
        self.edit_failure = None;
        cx.notify();
    }

    /// Selects exactly `ids`, as if clicked in turn with ⌘ held.
    pub fn select(&mut self, ids: impl IntoIterator<Item = TransactionId>, cx: &mut Context<Self>) {
        self.selection.set(ids);
        cx.notify();
    }

    /// The register the table shows.
    /// Today, as the screen dates things.
    pub fn today(&self) -> NaiveDate {
        self.today
    }

    pub fn register(&self) -> &Register {
        &self.register
    }

    /// What the filters and sort pick, as the table shows it.
    pub fn listing(&self) -> &Listing {
        &self.listing
    }

    pub fn filter(&self) -> &Filter {
        &self.filter
    }

    /// The menu that's open, if any.
    pub fn open_menu(&self) -> Option<Menu> {
        self.menu
    }

    /// The undo bar's message, while it shows.
    pub fn undo_message(&self) -> Option<&SharedString> {
        self.undo.as_ref().map(|undo| &undo.message)
    }

    /// Why the last change wasn't saved.
    pub fn edit_error(&self) -> Option<&SharedString> {
        self.edit_error.as_ref()
    }

    /// The selected rows, in display order.
    pub fn selected(&self) -> Vec<&RegisterRow> {
        self.selection
            .in_order(&self.order)
            .filter_map(|id| self.register.row(id))
            .collect()
    }

    /// Reads the register again, in the background.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        let Source::Database(store) = &self.source else {
            return;
        };
        let store = store.clone();
        let today = self.today;
        self.loading = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_spawn(async move {
                    store
                        .open()
                        .and_then(|db| Ok(Register::load(&db.reader()?, today)?))
                })
                .await
                .map_err(|error| SharedString::from(error.to_string()));
            this.update(cx, |this, cx| this.apply(result, cx)).ok();
        }));
    }

    fn apply(&mut self, result: Result<Register, SharedString>, cx: &mut Context<Self>) {
        self.loading = None;
        self.loaded = true;
        self.generation += 1;
        match result {
            Ok(register) => {
                self.category_counts = register.category_counts();
                self.register = register;
                self.load_error = None;
            }
            Err(error) => {
                tracing::warn!(%error, "couldn't read the transactions");
                self.load_error = Some(error);
            }
        }
        self.refilter(cx);
        if let Some(id) = self.reveal.take() {
            self.show_row(id, cx);
        }
    }

    /// Selects row `id` and scrolls it into view, first clearing the
    /// filters that hide it (core's `Register::filter_showing`), so a row
    /// the user just added or asked to see is never left out of sight. The
    /// others stay as they were.
    fn show_row(&mut self, id: TransactionId, cx: &mut Context<Self>) {
        let Some(shown) = self.register.filter_showing(&self.filter, id) else {
            return;
        };
        if shown != self.filter {
            if shown.text != self.filter.text {
                self.search_stale = true;
            }
            if shown.dates.is_none() {
                self.date = DatePreset::AllTime;
            }
            if shown.account.is_none()
                && self.filter.account.is_some()
                && let Some((accounts, _)) = &self.accounts
            {
                accounts.update(cx, |accounts, cx| accounts.select(None, cx));
            }
            self.filter = shown;
            self.refilter(cx);
        }
        if self.order.contains(&id) {
            self.selection.set([id]);
            self.reveal_current();
        }
        cx.notify();
    }

    /// Sets the search field to `filter.text` after [`Self::show_row`]
    /// cleared it.
    fn sync_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !std::mem::take(&mut self.search_stale) {
            return;
        }
        let text = self.filter.text.clone();
        self.search
            .update(cx, |search, cx| search.set_value(text, window, cx));
    }

    /// The failure bar's message, while it shows.
    pub fn edit_failure_message(&self) -> Option<SharedString> {
        let failure = self.edit_failure.as_ref()?;
        let merchant = self
            .register
            .row(failure.row)
            .map(|row| row.merchant.as_str());
        Some(present::unsaved_message(merchant).into())
    }

    /// The failure bar's Show: back to the row, with why the edit didn't
    /// save said where the inspector says it.
    fn show_failed_edit(&mut self, cx: &mut Context<Self>) {
        let Some(failure) = self.edit_failure.take() else {
            return;
        };
        if self.register.row(failure.row).is_none() {
            cx.notify();
            return;
        }
        self.show_row(failure.row, cx);
        self.editor.restore_error = Some((failure.row, failure.place, failure.reason));
        cx.notify();
    }

    /// Picks the rows again from the register: after a filter, sort or
    /// data change. Selected rows the filters now hide are deselected.
    fn refilter(&mut self, cx: &mut Context<Self>) {
        self.listing = self.register.select(&self.filter, self.sort);
        self.items = table_items(&self.listing);
        self.order = self
            .listing
            .rows
            .iter()
            .map(|&index| self.register.rows()[index].id)
            .collect();
        let kept: Vec<TransactionId> = self.selection.in_order(&self.order).collect();
        if kept.len() != self.selection.len() {
            self.selection.set(kept);
        }
        self.list.reset(self.items.len());
        cx.notify();
    }

    fn set_account(&mut self, account: Option<AccountId>, cx: &mut Context<Self>) {
        if self.filter.account != account {
            self.filter.account = account;
            self.refilter(cx);
        }
    }

    /// Clears the search and every filter, the panel's account included.
    fn clear_filters(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search
            .update(cx, |search, cx| search.set_value("", window, cx));
        self.filter.text.clear();
        self.filter.category = None;
        self.filter.uncategorised = false;
        self.filter.dates = None;
        self.filter.include_excluded = false;
        self.date = DatePreset::AllTime;
        match &self.accounts {
            Some((accounts, _)) => accounts.update(cx, |accounts, cx| accounts.select(None, cx)),
            None => self.filter.account = None,
        }
        self.refilter(cx);
    }

    fn has_filters(&self) -> bool {
        self.filter.account.is_some()
            || self.filter.category.is_some()
            || self.filter.dates.is_some()
            || self.filter.uncategorised
            || self.filter.include_excluded
            || !self.filter.text.trim().is_empty()
    }

    fn toggle_uncategorised(&mut self, cx: &mut Context<Self>) {
        self.set_uncategorised(!self.filter.uncategorised, cx);
    }

    /// Shows excluded rows in their days, or hides them (the Excluded chip,
    /// off by default).
    pub fn set_excluded(&mut self, on: bool, cx: &mut Context<Self>) {
        self.filter.include_excluded = on;
        self.refilter(cx);
    }

    /// What the status bar counts.
    pub fn counts(&self) -> Counts {
        let excluded = self
            .register
            .rows()
            .iter()
            .filter(|row| row.excluded.is_some())
            .count();
        Counts {
            transactions: self.register.rows().len() - excluded,
            excluded,
            sample: matches!(self.source, Source::Fixture),
        }
    }

    /// What the accounts panel lists under CATEGORIES, and which row the
    /// table is filtered to.
    pub fn panel_categories(&self) -> PanelCategories {
        let groups = self
            .category_counts
            .groups
            .iter()
            .filter_map(|count| {
                let category = self.register.category(count.category)?;
                Some(PanelCategory {
                    id: category.id,
                    label: category.label.clone().into(),
                    marker: present::marker(category.colour),
                    rows: count.rows,
                })
            })
            .collect();
        PanelCategories {
            groups,
            uncategorised: self.category_counts.uncategorised,
            picked: self.picked_category(),
        }
    }

    /// The panel row the table is filtered to: Uncategorised wins, as it
    /// narrows any category further.
    fn picked_category(&self) -> Option<CategoryPick> {
        if self.filter.uncategorised {
            Some(CategoryPick::Uncategorised)
        } else {
            self.filter.category.map(CategoryPick::Category)
        }
    }

    /// Filters by a row of the panel's CATEGORIES: a group, its
    /// subcategories included, or the rows that need a category; the two
    /// replace each other. With `toggle`, picking the row the table is
    /// already filtered to clears it, as clicking a selected account does.
    pub fn pick_category(&mut self, pick: CategoryPick, toggle: bool, cx: &mut Context<Self>) {
        if toggle && self.picked_category() == Some(pick) {
            self.filter.category = None;
            self.filter.uncategorised = false;
        } else {
            match pick {
                CategoryPick::Category(id) => {
                    self.filter.category = Some(id);
                    self.filter.uncategorised = false;
                }
                CategoryPick::Uncategorised => {
                    self.filter.category = None;
                    self.filter.uncategorised = true;
                }
            }
        }
        self.refilter(cx);
    }

    /// The account the accounts panel filters by, if any.
    pub fn selected_account(&self) -> Option<AccountId> {
        self.filter.account
    }

    /// Whether the selected account is one the user keeps by hand (cash):
    /// the toolbar offers "New transaction" in place of Import.
    fn on_manual_account(&self) -> bool {
        self.filter
            .account
            .and_then(|id| self.register.account(id))
            .is_some_and(|account| account.kind == AccountKind::Cash)
    }

    fn sort_by(&mut self, column: TransactionColumn, cx: &mut Context<Self>) {
        self.sort = self.sort.next(sort_column(column));
        self.refilter(cx);
    }

    // Menus ---------------------------------------------------------------

    /// The open menu's options before the filter field narrows them.
    fn menu_groups(&self, menu: Menu, cx: &App) -> Vec<MenuGroup> {
        match menu {
            Menu::Account => present::account_groups(&self.account_choices(cx)),
            Menu::Category => present::category_filter_groups(&self.register),
            Menu::Date => present::date_groups(),
            Menu::Row(_) | Menu::Inspector | Menu::Bulk | Menu::SplitLine(_) => {
                present::category_groups(&self.register)
            }
            Menu::Tags => present::tag_groups(&self.register, self.row_tags(), ""),
            Menu::Person => links::person_groups(self, ""),
            Menu::ManualAccount => manual_edit::account_groups(&self.register),
        }
    }

    /// The tags of the inspector's transaction.
    fn row_tags(&self) -> &[coffer_core::id::TagId] {
        self.single_row().map_or(&[], |row| row.tags.as_slice())
    }

    /// The state of `menu`: its select field's, or the popup menus'.
    fn select_state(&mut self, menu: Menu) -> Option<&mut SelectState> {
        match menu {
            Menu::Inspector => Some(&mut self.inspector_select),
            Menu::SplitLine(key) => self.editor.line_select(key),
            Menu::Person => Some(&mut self.editor.person_select),
            Menu::ManualAccount => Some(&mut self.editor.account_select),
            _ => Some(&mut self.menu_state),
        }
    }

    /// The open menu's options as shown.
    fn visible_groups(&self, cx: &App) -> Vec<MenuGroup> {
        let Some(menu) = self.menu else {
            return Vec::new();
        };
        if menu == Menu::Tags {
            let query = self.menu_filter.read(cx).value();
            return present::tag_groups(&self.register, self.row_tags(), &query);
        }
        if menu == Menu::Person {
            let query = self.menu_filter.read(cx).value();
            return links::person_groups(self, &query);
        }
        let groups = self.menu_groups(menu, cx);
        if menu.has_filter() {
            filter_groups(&groups, &self.menu_filter.read(cx).value())
        } else {
            groups
        }
    }

    /// The accounts the Account menu offers: the panel's (each pot after its
    /// parent), or the register's without a panel.
    fn account_choices(&self, cx: &App) -> Vec<(AccountId, String)> {
        match &self.accounts {
            Some((accounts, _)) => accounts
                .read(cx)
                .data()
                .tree
                .rows()
                .map(|node| (node.account.id, node.account.name.clone()))
                .collect(),
            None => self
                .register
                .accounts()
                .iter()
                .map(|account| (account.id, account.name.clone()))
                .collect(),
        }
    }

    /// The key of the option the menu ticks.
    fn chosen_key(&self, menu: Menu) -> Option<SharedString> {
        let category = |id: Option<i64>| id.map(|id| SharedString::from(id.to_string()));
        match menu {
            Menu::Account => Some(
                self.filter
                    .account
                    .map_or(present::ALL.into(), |id| id.to_string().into()),
            ),
            Menu::Category => Some(category(self.filter.category).unwrap_or(present::ALL.into())),
            Menu::Date => Some(present::date_key(self.date).into()),
            Menu::Row(id) => self.register.row(id).and_then(|row| match row.category {
                coffer_core::reporting::register::RowCategory::Category(id) => category(Some(id)),
                _ => None,
            }),
            Menu::Inspector | Menu::Bulk => {
                let selected = self.selected();
                let mut categories = selected.iter().map(|row| match row.category {
                    coffer_core::reporting::register::RowCategory::Category(id) => Some(id),
                    _ => None,
                });
                let first = categories.next().flatten();
                categories
                    .all(|other| other == first)
                    .then(|| category(first))
                    .flatten()
            }
            Menu::SplitLine(key) => category(self.line_category(key)),
            Menu::Tags => None,
            Menu::Person => self
                .chosen_person()
                .map(|label| format!("person-{label}").into()),
            Menu::ManualAccount => self.single_row().map(|row| row.account.to_string().into()),
        }
    }

    /// Opens `menu`, as its chip, row or button would (snapshots).
    pub fn show_menu(&mut self, menu: Menu, window: &mut Window, cx: &mut Context<Self>) {
        // The inspector's fields follow a selection made just before.
        self.sync_editor(window, cx);
        if self.menu != Some(menu) {
            self.open(menu, window, cx);
        }
    }

    /// Shows `register` instead of the Paper fixture: another artboard's
    /// rows (snapshots). Does nothing over the database.
    pub fn show_fixture(&mut self, register: Register, cx: &mut Context<Self>) {
        if matches!(self.source, Source::Fixture) {
            self.apply(Ok(register), cx);
        }
    }

    /// Splits the selected row as its Split button does (snapshots).
    pub fn split_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.sync_editor(window, cx);
        self.start_split(window, cx);
    }

    /// Types `text` into split line `line`'s amount field, as the user
    /// would (snapshots).
    pub fn type_split_amount(
        &mut self,
        line: usize,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sync_editor(window, cx);
        let Some(fields) = self
            .editor
            .split
            .as_ref()
            .and_then(|split| split.lines.get(line))
        else {
            return;
        };
        let (key, input) = (fields.key, fields.amount.clone());
        input.update(cx, |input, cx| {
            input.set_value(SharedString::from(text.to_owned()), window, cx);
            input.focus(window, cx);
        });
        self.line_amount_event(key, &input, &InputEvent::Change, window, cx);
    }

    /// Types `text` into the selected row's Merchant field, as the user
    /// would, without leaving it (snapshots).
    pub fn type_merchant(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.sync_editor(window, cx);
        let text = SharedString::from(text.to_owned());
        self.editor
            .merchant
            .update(cx, |input, cx| input.set_value(text, window, cx));
    }

    /// Removes split line `line` as its ✕ does (snapshots).
    pub fn remove_split_line_at(
        &mut self,
        line: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sync_editor(window, cx);
        let key = self
            .editor
            .split
            .as_ref()
            .and_then(|split| split.lines.get(line))
            .map(|fields| fields.key);
        if let Some(key) = key {
            self.remove_split_line(key, cx);
        }
    }

    /// Types `text` into the open menu's filter field (snapshots).
    pub fn type_in_menu(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.menu_filter.update(cx, |filter, cx| {
            filter.set_value(SharedString::from(text.to_owned()), window, cx)
        });
        let groups = self.visible_groups(cx);
        if let Some(menu) = self.menu
            && let Some(state) = self.select_state(menu)
        {
            state.highlight_first(&groups);
        }
        cx.notify();
    }

    /// Sorts each day's rows (snapshots; the header's clicks in the app).
    pub fn set_sort(&mut self, sort: Sort, cx: &mut Context<Self>) {
        self.sort = sort;
        self.refilter(cx);
    }

    /// Shows only the rows that need a category, or every row.
    pub fn set_uncategorised(&mut self, on: bool, cx: &mut Context<Self>) {
        self.filter.uncategorised = on;
        self.refilter(cx);
    }

    /// Types `text` into the search field (snapshots; the field itself in
    /// the app).
    pub fn set_search(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.search
            .update(cx, |search, cx| search.set_value(text, window, cx));
        self.filter.text = text.to_owned();
        self.refilter(cx);
    }

    fn open(&mut self, menu: Menu, window: &mut Window, cx: &mut Context<Self>) {
        if self.menu == Some(menu) {
            self.close(window, cx);
            return;
        }
        if self.menu.is_some() {
            self.close(window, cx);
        }
        self.menu = Some(menu);
        let placeholder = match menu {
            Menu::Tags => "Find or create a tag",
            Menu::Person => "Find or add a person",
            _ => "Find a category",
        };
        self.menu_filter.update(cx, |filter, cx| {
            filter.set_placeholder(placeholder, window, cx);
            filter.set_value("", window, cx);
        });
        let chosen = self.chosen_key(menu);
        let Some(state) = self.select_state(menu) else {
            self.menu = None;
            return;
        };
        state.set_open(true, chosen.as_ref(), window, cx);
        if menu.has_filter() {
            self.menu_filter
                .update(cx, |filter, cx| filter.focus(window, cx));
        }
        cx.notify();
    }

    /// Closes the open menu and puts focus back on what opened it.
    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(menu) = self.menu.take() else {
            return;
        };
        if menu.is_field_select() {
            if let Some(state) = self.select_state(menu) {
                state.set_open(false, None, window, cx);
            }
        } else {
            self.menu_state.dismiss();
            let back = match menu {
                Menu::Account => &self.chip_focus[0],
                Menu::Category => &self.chip_focus[1],
                Menu::Date => &self.chip_focus[2],
                Menu::Tags => &self.editor.tag_focus,
                _ => &self.focus,
            };
            back.focus(window, cx);
        }
        cx.notify();
    }

    fn move_highlight(&mut self, step: isize, cx: &mut Context<Self>) {
        let groups = self.visible_groups(cx);
        let Some(menu) = self.menu else {
            return;
        };
        if let Some(state) = self.select_state(menu) {
            state.move_highlight(&groups, step);
        }
        cx.notify();
    }

    fn choose(&mut self, key: &SharedString, window: &mut Window, cx: &mut Context<Self>) {
        let Some(menu) = self.menu else {
            return;
        };
        // What was typed into the tag menu names a new tag.
        let typed = self.menu_filter.read(cx).value().to_string();
        self.close(window, cx);
        match menu {
            Menu::Account => {
                let account = key.parse::<i64>().ok().map(AccountId::new);
                match &self.accounts {
                    Some((accounts, _)) => {
                        let accounts = accounts.clone();
                        accounts.update(cx, |accounts, cx| accounts.select(account, cx));
                    }
                    None => self.set_account(account, cx),
                }
            }
            Menu::Category => {
                self.filter.category = key.parse::<i64>().ok();
                self.refilter(cx);
            }
            Menu::Date => {
                if let Some(preset) = present::date_preset(key) {
                    self.date = preset;
                    self.filter.dates = preset.range(self.today);
                    self.refilter(cx);
                }
            }
            Menu::Row(id) => {
                if let Ok(category) = key.parse() {
                    self.categorise(vec![id], category, cx);
                }
            }
            Menu::Inspector | Menu::Bulk => {
                if let Ok(category) = key.parse() {
                    let ids = self.selection.in_order(&self.order).collect();
                    self.categorise(ids, category, cx);
                }
            }
            Menu::SplitLine(line) => {
                if let Ok(category) = key.parse() {
                    self.set_line_category(line, category, cx);
                }
            }
            Menu::Person => match key.as_ref() {
                links::NEW_PERSON => self.choose_person(&typed, cx),
                key => {
                    if let Some(person) = links::person_from_key(key) {
                        let person = person.to_owned();
                        self.choose_person(&person, cx);
                    }
                }
            },
            Menu::ManualAccount => {
                if let Ok(id) = key.parse::<i64>() {
                    self.set_manual_account(AccountId::new(id), cx);
                }
            }
            Menu::Tags => match key.as_ref() {
                present::NEW_TAG => self.create_tag(&typed, false, cx),
                present::NEW_PRIVATE_TAG => self.create_tag(&typed, true, cx),
                key => {
                    if let Some(tag) = present::tag_from_key(key) {
                        self.toggle_tag(tag, cx);
                    }
                }
            },
        }
    }

    /// The menu list for `menu`, wired to this view.
    fn menu_list(&self, menu: Menu, cx: &mut Context<Self>) -> MenuList {
        let groups = self.visible_groups(cx);
        let state = if menu == Menu::Inspector {
            &self.inspector_select
        } else {
            &self.menu_state
        };
        let label = match menu {
            Menu::Account | Menu::ManualAccount => "Account",
            Menu::Date => "Date",
            Menu::Tags => "Tags",
            _ => "Category",
        };
        let rows = groups
            .iter()
            .map(|group| group.options.len() + usize::from(group.label.is_some()))
            .sum::<usize>();
        let (moved, chosen, dismissed) = (
            cx.entity().downgrade(),
            cx.entity().downgrade(),
            cx.entity().downgrade(),
        );
        let mut list = MenuList::new(label, groups, self.chosen_key(menu), state)
            .own_keys()
            .on_move(move |step, _, cx| {
                moved
                    .update(cx, |this, cx| this.move_highlight(step, cx))
                    .ok();
            })
            .on_choose(move |key, window, cx| {
                chosen
                    .update(cx, |this, cx| this.choose(key, window, cx))
                    .ok();
            })
            .on_dismiss(move |window, cx| {
                dismissed.update(cx, |this, cx| this.close(window, cx)).ok();
            });
        if menu.has_filter() {
            list = list.filter(&self.menu_filter);
        }
        if menu.has_filter() || rows as f32 > MENU_ROWS {
            list = list.max_height(token_size::CONTROL * MENU_ROWS);
        }
        list
    }

    // Changes -------------------------------------------------------------

    /// Sets `category` on each of `ids` that can take it, through core: one
    /// undoable change batch. After a change to several rows, the undo bar
    /// shows.
    fn categorise(&mut self, ids: Vec<TransactionId>, category: i64, cx: &mut Context<Self>) {
        let Some(target) = self.register.category(category) else {
            return;
        };
        let (key, label) = (target.key.clone(), target.label.clone());
        let items: Vec<_> = ids
            .into_iter()
            .filter(|id| {
                self.register
                    .row(*id)
                    .is_some_and(RegisterRow::takes_category)
            })
            .map(|id| (id, key.clone()))
            .collect();
        if items.is_empty() {
            return;
        }
        let count = items.len();
        let message = present::categorised_message(count, &label);
        let policy = Policy::new(Said::Category)
            .log("couldn't change the category")
            .undo(
                move |batch: &Option<ChangeBatchId>| match batch.filter(|_| count > 1) {
                    Some(batch) => Offer::Show(UndoAction::Batch(batch), message),
                    None => Offer::Clear,
                },
            );
        if self
            .write(policy, cx, |db| own::recategorize(db, &items))
            .is_ok()
            // One row's category set by hand may become a rule (D6.3).
            && let [(id, _)] = items.as_slice()
        {
            self.propose_rule(*id, cx);
        }
    }

    fn undo_change(&mut self, cx: &mut Context<Self>) {
        let Some(undo) = self.undo.take() else {
            return;
        };
        match undo.action {
            UndoAction::Batch(batch) => {
                let policy = Policy::new(Said::Category)
                    .log("couldn't undo the change")
                    .emit(TransactionsEvent::Changed)
                    .always();
                self.write(policy, cx, |db| change::undo(db, batch)).ok();
            }
            UndoAction::Include(id) => {
                // The row comes back selected, where the user left it.
                self.reveal = Some(id);
                self.include(id, true, cx);
            }
            UndoAction::Transfer(decision) => self.undo_transfer(decision, cx),
        }
        cx.notify();
    }

    // Exclusions, imports and manual rows (M3.10) --------------------------

    /// Excludes `id` for `reason` (or changes the reason and details of an
    /// excluded row) through core. Newly excluded, the undo bar offers to
    /// include it again: with the Excluded chip off, the row leaves the
    /// table.
    pub fn exclude(
        &mut self,
        id: TransactionId,
        reason: ExclusionReason,
        note: Option<&str>,
        cx: &mut Context<Self>,
    ) -> Result<(), SharedString> {
        let was_excluded = self
            .register
            .row(id)
            .is_some_and(|row| row.excluded.is_some());
        let message = format!(
            "Excluded as {}",
            source::reason_label(reason).to_lowercase()
        );
        let policy = Policy::ledger().undo(move |_| {
            if was_excluded {
                Offer::Keep
            } else {
                Offer::Show(UndoAction::Include(id), message.into())
            }
        });
        self.write(policy, cx, |db| {
            exclusion::exclude(db, id, reason, note).map(drop)
        })
    }

    /// "Include again": the undo of excluding. A refusal (a lock) is said
    /// in the exclusion card.
    pub fn include_again(&mut self, id: TransactionId, cx: &mut Context<Self>) {
        self.include(id, false, cx);
    }

    /// Includes `id` again; `from_bar` when it's the undo bar's Undo, which
    /// offers it again if it fails.
    fn include(&mut self, id: TransactionId, from_bar: bool, cx: &mut Context<Self>) {
        let action = UndoAction::Include(id);
        let mut policy = Policy::new(Said::Inspector(editor::EditPlace::Exclusion))
            .sentence()
            .log("couldn't include a transaction again")
            .undo(move |_| Offer::ClearIf(action))
            .emit(TransactionsEvent::LedgerChanged);
        if from_bar {
            policy = policy.retry(action);
        }
        if self
            .write(policy, cx, |db| exclusion::include_again(db, id))
            .is_err()
        {
            self.reveal = None;
        }
    }

    /// What undoing import `batch` would remove.
    pub fn undo_import_preview(&self, batch: BatchId) -> Result<UndoPreview, SharedString> {
        self.query(|db| import::undo_preview(db, batch))
    }

    /// Undoes import `batch`; the table and the accounts panel read again.
    pub fn undo_import(
        &mut self,
        batch: BatchId,
        cx: &mut Context<Self>,
    ) -> Result<(), SharedString> {
        let policy = Policy::ledger().undo(|_| Offer::Clear);
        self.write(policy, cx, |db| import::undo(db, batch))
            .map(drop)
    }

    /// Adds a manual transaction with the user's `note`; once the register
    /// has it, the new row is selected and scrolled into view.
    pub fn create_manual(
        &mut self,
        entry: &ManualTransaction,
        note: &str,
        cx: &mut Context<Self>,
    ) -> Result<TransactionId, SharedString> {
        let policy = Policy::ledger().undo(|_| Offer::Clear);
        let id = self.write(policy, cx, |db| -> Result<_, StoreError> {
            // Checked before the row is added: stored with it in one DB
            // transaction, so a failure adds nothing (and trying again can't
            // add the row twice).
            let note = UserNote::new(note)?;
            Ok(manual::create_with_note(db, entry, note.as_ref())?)
        })?;
        // Selected once the register, read again in the background, has it.
        self.reveal = Some(id);
        Ok(id)
    }

    /// Deletes manual transaction `id`.
    pub fn delete_manual(
        &mut self,
        id: TransactionId,
        cx: &mut Context<Self>,
    ) -> Result<(), SharedString> {
        let policy = Policy::ledger().undo(|_| Offer::Clear);
        self.write(policy, cx, |db| manual::delete(db, id).map(drop))
    }

    // Selection -----------------------------------------------------------

    fn click_row(
        &mut self,
        id: TransactionId,
        event: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let modifiers = event.modifiers();
        let kind = if modifiers.shift {
            ClickKind::Extend
        } else if modifiers.secondary(cx) {
            ClickKind::Toggle
        } else {
            ClickKind::Replace
        };
        self.selection.click(id, kind, &self.order);
        self.undo = None;
        self.edit_failure = None;
        self.focus.clone().focus(window, cx);
        cx.notify();
    }

    fn move_selection(&mut self, to: Move, cx: &mut Context<Self>) {
        self.selection.move_by(to, &self.order);
        self.undo = None;
        self.edit_failure = None;
        self.reveal_current();
        cx.notify();
    }

    /// Opens the category menu on the keyboard row, if it can take one.
    fn edit_current(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.selection.current() else {
            return;
        };
        if self
            .register
            .row(id)
            .is_some_and(|row| row.takes_category() && row.excluded.is_none())
        {
            self.open(Menu::Row(id), window, cx);
        }
    }

    /// Escape: clears the selection, or with nothing selected, the filters.
    fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.selection.is_empty() {
            if self.has_filters() {
                self.clear_filters(window, cx);
            }
        } else {
            self.clear_selection(cx);
        }
    }

    /// Scrolls the keyboard row into view.
    fn reveal_current(&self) {
        let Some(current) = self.selection.current() else {
            return;
        };
        let index = self.items.iter().position(|item| match item {
            TableItem::Row(row) => self.register.rows()[*row].id == current,
            TableItem::Day { .. } => false,
        });
        if let Some(index) = index {
            self.list.reveal(index);
        }
    }

    // Rendering -----------------------------------------------------------

    fn render_item(
        &mut self,
        index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // A list item is laid out on its own, at the list's width, and
        // ignores the list's padding: the wrapper fills the width and
        // carries the table body's 8px inline padding.
        let item = match self.items[index] {
            TableItem::Day { day, first } => {
                let day = &self.listing.days[day];
                DayHeader::new(
                    present::day_heading(day.date, self.today),
                    present::net(&day.net),
                )
                .divided(!first)
                .into_any_element()
            }
            TableItem::Row(row) => self.render_row(row, window, cx),
        };
        div()
            .flex()
            .flex_col()
            .w_full()
            .px(spacing::S2)
            .child(item)
            .into_any_element()
    }

    fn render_row(&self, index: usize, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let row = &self.register.rows()[index];
        let id = row.id;
        let focus_visible = self.selection.current() == Some(id)
            && self.focus.is_focused(window)
            && window.last_input_was_keyboard();
        // An excluded row's category cell says why; its category is set
        // in the inspector.
        let editable = row.takes_category() && row.excluded.is_none();
        let cell = match row.excluded {
            Some(reason) => CategoryCell::excluded(source::reason_label(reason)),
            None => linking::cell(&self.register, row),
        };
        TransactionRow::new(
            format!("txn-{id}"),
            row.merchant.clone(),
            cell,
            format::money(row.amount, Sign::Always),
        )
        .when(self.filter.account.is_none(), |this| {
            this.account(
                self.register
                    .account(row.account)
                    .map_or(String::new(), |account| account.name.clone()),
            )
        })
        .when_some(row.original, |this, original| {
            let unsigned = original.checked_abs().unwrap_or(original);
            this.original_amount(format::money(unsigned, Sign::Negative))
        })
        .selected(self.selection.contains(id))
        .focus_visible(focus_visible)
        .on_click(cx.listener(move |this, event, window, cx| this.click_row(id, event, window, cx)))
        .when(editable, |this| {
            this.on_edit_category(cx.listener(move |this, _, window, cx| {
                this.selection.set([id]);
                this.open(Menu::Row(id), window, cx);
            }))
        })
        .when(self.menu == Some(Menu::Row(id)), |this| {
            this.category_menu(self.menu_list(Menu::Row(id), cx))
        })
        .into_any_element()
    }

    /// `trigger` as the opener of `menu`: a press toggles it, judged by
    /// whether it was open when drawn, as the field select does. The press
    /// outside the menu that closes it is the same press, so it mustn't
    /// open it again. A keyboard click on the trigger toggles it too.
    fn menu_trigger(&self, menu: Menu, trigger: impl IntoElement, cx: &mut Context<Self>) -> Div {
        let was_open = self.menu == Some(menu);
        div()
            .flex()
            .flex_none()
            .capture_any_mouse_down(
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    if event.button != MouseButton::Left {
                        return;
                    }
                    if was_open {
                        this.close(window, cx);
                    } else if this.menu != Some(menu) {
                        this.open(menu, window, cx);
                        // The "+ Tag" pill is a focusable div: its own mouse
                        // down would take focus back from the menu's filter.
                        if menu == Menu::Tags {
                            window.prevent_default();
                        }
                    }
                }),
            )
            .child(trigger)
    }

    /// What a keyboard activation of a menu's trigger does; a mouse press
    /// is [`Self::menu_trigger`]'s.
    fn keyboard_toggle(
        menu: Menu,
    ) -> impl Fn(&mut Self, &ClickEvent, &mut Window, &mut Context<Self>) + 'static {
        move |this, event, window, cx| {
            if matches!(event, ClickEvent::Keyboard(_)) {
                this.open(menu, window, cx);
            }
        }
    }

    /// A filter chip that opens `menu`, with the menu below it while open.
    fn menu_chip(&self, menu: Menu, chip: Chip, cx: &mut Context<Self>) -> impl IntoElement {
        let focus = match menu {
            Menu::Account => &self.chip_focus[0],
            Menu::Category => &self.chip_focus[1],
            _ => &self.chip_focus[2],
        };
        let chip = chip
            .menu(true)
            .track_focus(focus)
            .on_click(cx.listener(Self::keyboard_toggle(menu)));
        div()
            .relative()
            .flex_none()
            .child(self.menu_trigger(menu, chip, cx))
            .when(self.menu == Some(menu), |this| {
                this.child(menu::below(
                    token_size::CONTROL + spacing::S1,
                    self.menu_list(menu, cx),
                ))
            })
    }

    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let account = self
            .filter
            .account
            .and_then(|id| self.register.account(id))
            .map(|account| account.name.clone());
        let category = self
            .filter
            .category
            .and_then(|id| self.register.category(id))
            .map(|category| category.label.clone());
        let date = (self.date != DatePreset::AllTime).then(|| present::date_label(self.date));
        div()
            .flex()
            .flex_none()
            .items_center()
            .gap(spacing::S2)
            .py(spacing::S3)
            .px(spacing::S4)
            // With "New transaction" (wider than Import) the toolbar
            // overflows 1440 by a few pixels: the search field gives them up.
            .child(
                search_input(&self.search)
                    .id("search")
                    .flex_shrink(1.)
                    .min_w_0(),
            )
            .child(
                self.menu_chip(
                    Menu::Account,
                    Chip::new(
                        "filter-account",
                        account.clone().unwrap_or("Account".into()),
                    )
                    .active(account.is_some()),
                    cx,
                ),
            )
            .child(
                self.menu_chip(
                    Menu::Category,
                    Chip::new(
                        "filter-category",
                        category.clone().unwrap_or("Category".into()),
                    )
                    .active(category.is_some()),
                    cx,
                ),
            )
            .child(self.menu_chip(
                Menu::Date,
                Chip::new("filter-date", date.unwrap_or("Date")).active(date.is_some()),
                cx,
            ))
            .child(
                Chip::new("filter-uncategorised", "Uncategorised")
                    .count(self.listing.uncategorised.to_string())
                    .active(self.filter.uncategorised)
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_uncategorised(cx))),
            )
            // No count: with one it overflowed the toolbar at 1440 (D6.3);
            // the status bar counts the excluded rows.
            .child(
                Chip::new("filter-excluded", "Excluded")
                    .active(self.filter.include_excluded)
                    .on_click(cx.listener(|this, _, _, cx| {
                        let on = !this.filter.include_excluded;
                        this.set_excluded(on, cx)
                    })),
            )
            .child(div().flex_1())
            .map(|this| {
                if self.on_manual_account() {
                    // A cash account takes rows by hand (D6.3, `4D7-0`).
                    this.child(
                        Button::new("new-transaction", "New transaction")
                            .with_variant(ButtonVariant::Outline)
                            .shortcut("⌘N")
                            .on_click(|_, window, cx| {
                                window.dispatch_action(Box::new(crate::shell::NewTransaction), cx)
                            }),
                    )
                } else {
                    this.child(
                        Button::new("import", "Import")
                            .with_variant(ButtonVariant::Outline)
                            .shortcut("⌘I")
                            // The shell opens the Import dialog (M3.4).
                            .on_click(|_, window, cx| {
                                window.dispatch_action(Box::new(crate::shell::ImportStatement), cx)
                            }),
                    )
                }
            })
    }

    fn render_header(&self, cx: &mut Context<Self>) -> TransactionHeader {
        let view = cx.entity().downgrade();
        let header = TransactionHeader::new()
            .full_bleed(true)
            .show_account(self.filter.account.is_none())
            .on_sort(move |column, _, cx| {
                view.update(cx, |this, cx| this.sort_by(column, cx)).ok();
            });
        match self.sort {
            Sort::Default => header,
            Sort::Ascending(column) => header.sorted(table_column(column), false),
            Sort::Descending(column) => header.sorted(table_column(column), true),
        }
    }

    /// What the table area shows when it has no rows.
    fn render_empty(&self) -> Option<AnyElement> {
        if !self.loaded || !self.listing.is_empty() {
            return None;
        }
        let state = if let Some(error) = &self.load_error {
            EmptyState::new("Couldn't read the transactions").body(error.clone())
        } else if self.register.rows().is_empty() {
            EmptyState::new("No transactions yet").shortcut("⌘I", "Import a statement")
        } else {
            EmptyState::new("No transactions match")
                .body("Nothing matches the search and filters.")
                .shortcut("Esc", "Clear filters")
        };
        Some(
            div()
                .flex()
                .flex_1()
                .items_center()
                .justify_center()
                .child(state)
                .into_any_element(),
        )
    }

    fn render_table(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let body = match self.render_empty() {
            Some(empty) => empty,
            None => list(self.list.clone(), cx.processor(Self::render_item))
                .flex_1()
                .into_any_element(),
        };
        div()
            .track_focus(&self.focus)
            .flex()
            .flex_col()
            .flex_1()
            .min_h_0()
            .child(self.render_header(cx))
            .child(body)
    }

    fn render_floating(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let bar = if let Some(message) = self.edit_failure_message() {
            NotificationBar::new(message)
                .failed(true)
                .action(
                    Button::new("edit-failure-show", "Show")
                        .with_variant(ButtonVariant::Outline)
                        .on_click(cx.listener(|this, _, _, cx| this.show_failed_edit(cx))),
                )
                .on_dismiss(cx.listener(|this, _, _, cx| {
                    this.edit_failure = None;
                    cx.notify();
                }))
                .into_any_element()
        } else if let Some(undo) = &self.undo {
            UndoBar::new(undo.message.clone())
                .on_undo(cx.listener(|this, _, _, cx| this.undo_change(cx)))
                .on_dismiss(cx.listener(|this, _, _, cx| {
                    this.undo = None;
                    cx.notify();
                }))
                .into_any_element()
        } else if self.selection.len() > 1 {
            div()
                .relative()
                .child(
                    BulkActionBar::new(self.selection.len().to_string())
                        .action(
                            self.menu_trigger(
                                Menu::Bulk,
                                Button::new("bulk-categorise", "Categorise")
                                    .with_variant(ButtonVariant::Toolbar)
                                    .on_click(cx.listener(Self::keyboard_toggle(Menu::Bulk))),
                                cx,
                            ),
                        )
                        .action(
                            Button::new("bulk-transfer", "Mark as transfer")
                                .with_variant(ButtonVariant::Toolbar),
                        )
                        .action(
                            Button::new("bulk-rule", "Create rule")
                                .with_variant(ButtonVariant::Toolbar),
                        )
                        .on_clear(cx.listener(|this, _, _, cx| this.clear_selection(cx))),
                )
                .when(self.menu == Some(Menu::Bulk), |this| {
                    this.child(menu::above(spacing::S1, self.menu_list(Menu::Bulk, cx)))
                })
                .into_any_element()
        } else {
            return None;
        };
        Some(
            div()
                .absolute()
                .left_0()
                .right_0()
                .bottom(spacing::S6)
                .flex()
                .justify_center()
                .child(bar)
                .into_any_element(),
        )
    }

    fn render_canvas(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .key_context(TABLE_CONTEXT)
            .on_list_move(
                &TABLE_KEYS,
                cx.listener(|this, to: &Move, _, cx| this.move_selection(*to, cx)),
            )
            .on_action(
                cx.listener(|this, _: &EditCategory, window, cx| this.edit_current(window, cx)),
            )
            .on_action(cx.listener(|this, _: &Cancel, window, cx| this.cancel(window, cx)))
            .relative()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            .bg(color::CANVAS)
            .child(self.render_toolbar(cx))
            .child(self.render_table(cx))
            .children(self.render_floating(cx))
    }
}

impl Render for TransactionsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_search(window, cx);
        self.sync_editor(window, cx);
        div()
            .flex()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .map(|this| match self.valuations_canvas(cx) {
                Some(canvas) => this.child(canvas),
                None => this.child(self.render_canvas(cx)),
            })
            .child(inspector::render(self, window, cx))
    }
}

#[cfg(test)]
mod link_tests;
#[cfg(test)]
mod source_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod transfer_tests;
#[cfg(test)]
mod write_tests;
