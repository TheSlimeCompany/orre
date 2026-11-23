//! What the Transactions screen shows, as plain data, for tests (ROADMAP
//! U.2): the toolbar, the column header's sort and focused title, the days
//! and their rows, the selection and the keyboard row, the open menu, the
//! inspector, the bar over the table's foot (failure, undo or bulk actions)
//! and the keys with what they do.
//!
//! Compiled for tests and the dev driver only (the `dev-driver` feature,
//! debug builds): it isn't in a release build, and nothing but the tests and
//! the driver can reach it, `orre mcp`
//! included (`docs/architecture/app.md` → "Decision record: views that
//! describe themselves stay out of the release binary").
//!
//! Every string comes from the code the view draws with: the canvas's
//! `…Text` types ([`super::RowText`], [`super::ToolbarText`], …), the
//! inspector's ([`super::inspector`]) and [`super::present`]. Merchant names
//! and payment methods (the bank's text) are made plain there, through
//! [`super::linking::name`] ([`orre_core::envelope::shown`]), for both.
//! Styling (colours, markers, icons, the focus ring) is left out.
//!
//! What it leaves out: the edit fields' contents (Merchant and Note hold
//! the text as stored, raw, for editing), the bank note and the Source
//! section's raw description (drawn as imported), what the Share, Links,
//! Transfer, Source and Rule sections and the rule prompt hold (only which
//! sections show, and an error said in one), the account inspector and the
//! valuations canvas (the Accounts screen's), and the dialogs, which the
//! shell opens in windows of their own.

use gpui_kit::App;
use orre_core::id::TransactionId;
use orre_core::reporting::register::Sort;

pub use crate::ui::describe::Action;
use crate::ui::menu::SelectState;

use super::editor::EditPlace;
use super::inspector::{
    CHOOSE_CATEGORY, CategoryField, HeaderText, LINE_PLACEHOLDER, NOTHING_SELECTED, Section,
    SeveralText,
};
use super::present::{self, CellKind};
use super::{
    BarText, CATEGORISE, CREATE_RULE, CURSOR_MOVES, ChipText, EmptyText, MARK_AS_TRANSFER, Menu,
    RowText, SHOW, SORT_BY_TITLE, TABLE_KEYS, TOGGLE_ROW, TableItem, ToolbarText,
    TransactionColumn, TransactionsView,
};

/// What the Transactions screen shows ([`TransactionsView::describe`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransactionsDescription {
    pub toolbar: ToolbarDescription,
    /// Whether the table has the Account column (not while filtered to one
    /// account).
    pub account_column: bool,
    /// The column the header marks, and which way.
    pub sort: Sort,
    /// The column title with focus, where Enter and Space sort, as last
    /// drawn.
    pub focused_column: Option<TransactionColumn>,
    /// What the table area says with no rows.
    pub empty: Option<EmptyDescription>,
    pub days: Vec<DayDescription>,
    /// The rows when a column sort lists them as one flat list: no days.
    pub flat: Vec<RowDescription>,
    /// The menu that's open: a chip's, a row's, the inspector's or the bulk
    /// bar's.
    pub menu: Option<MenuDescription>,
    pub inspector: InspectorDescription,
    /// The bar over the table's foot.
    pub bar: Option<BarDescription>,
    /// The table's keys, and whether each does anything now.
    pub actions: Vec<Action>,
}

/// The toolbar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolbarDescription {
    /// What the search field holds.
    pub search: String,
    /// Account, Category, Date, Uncategorised, Excluded.
    pub chips: Vec<ChipDescription>,
    /// "Import" or "New transaction", and the shortcut on it.
    pub button: (&'static str, &'static str),
}

/// A filter chip.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChipDescription {
    /// The filter's value ("Current account"), or its name ("Account").
    pub label: String,
    pub active: bool,
    pub count: Option<String>,
}

/// The table area's empty state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmptyDescription {
    pub title: &'static str,
    pub body: Option<String>,
    /// The key cap and what it does: ("Esc", "Clear filters").
    pub shortcut: Option<(&'static str, &'static str)>,
}

/// One day: its header and rows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DayDescription {
    /// "Mon 9 Nov".
    pub heading: String,
    /// The day's net: "−£37.74".
    pub net: String,
    pub rows: Vec<RowDescription>,
}

/// A row of the table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowDescription {
    pub id: TransactionId,
    pub merchant: String,
    /// The category cell's words: "Eating out", "Uncategorised", "Transfer
    /// · Rainy day pot", "Excluded · Bank error", "Repays Sam".
    pub category: String,
    /// How it counts, after the category: "refund", "shared".
    pub counts_as: Option<String>,
    /// A transfer: the row dims, the merchant has ⇄.
    pub transfer: bool,
    /// A leg of a suggested pair: ⇄ in `warning` beside the merchant, the
    /// row not dimmed, because it counts until the pair is confirmed.
    pub suggested_transfer: bool,
    /// Excluded: the row is muted and its amount struck through.
    pub excluded: bool,
    /// The Account column, when it shows.
    pub account: Option<String>,
    pub amount: String,
    /// The amount in the original currency, under it.
    pub original: Option<String>,
    pub selected: bool,
    /// The keyboard row: ↑↓ move from it, Enter opens its category menu,
    /// ⌘↑/⌘↓ move it without changing the selection and Space adds or
    /// removes it.
    pub keyboard: bool,
    /// Wears the row mark: it arrived or changed in a read from another
    /// process (LU.5). Drawn only on listed rows, so only listed rows say so.
    pub marked: bool,
}

/// An open menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuDescription {
    pub menu: Menu,
    /// What's typed in its filter field, if it has one.
    pub filter: Option<String>,
    pub groups: Vec<MenuGroupDescription>,
}

/// Options under an optional caps label.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuGroupDescription {
    pub label: Option<String>,
    pub options: Vec<OptionDescription>,
}

/// One option of a menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OptionDescription {
    pub label: String,
    pub hint: Option<String>,
    /// Ticked: the value chosen now, or one of a row's tags.
    pub chosen: bool,
    /// Where Enter chooses.
    pub highlighted: bool,
    pub disabled: bool,
}

/// The inspector.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InspectorDescription {
    /// Nothing is selected: what it says instead.
    Nothing(&'static str),
    /// Nothing is selected and an account is: the account inspector.
    Account,
    /// Every selected row went in a read from another process: the empty
    /// state's title and body, until the next selection (LU.5).
    Removed {
        title: String,
        body: String,
    },
    One(Box<OneDescription>),
    Several(SeveralDescription),
}

/// The inspector for one transaction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OneDescription {
    /// The merchant.
    pub title: String,
    pub figure: String,
    /// Excluded: the figure is struck through.
    pub struck: bool,
    /// "Card payment · 12:41".
    pub caption: Option<String>,
    /// The Details rows with a value to read, in order: Date, Amount (a
    /// manual row's), Account, Category, Tags.
    pub fields: Vec<FieldDescription>,
    /// Why the last category change didn't save, under the Category row.
    pub category_error: Option<String>,
    /// Why the last edit didn't save, and where it's said: only a place the
    /// inspector draws now.
    pub error: Option<(EditPlace, String)>,
    /// The sections after Details: "Transfer", "Share", "Links", "Split",
    /// "Source", "Rule".
    pub sections: Vec<&'static str>,
    /// The split editor, while the row is split or being split.
    pub split: Option<SplitDescription>,
}

/// A Details row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldDescription {
    pub label: &'static str,
    pub value: String,
    /// Read-only (a lock, or a category that follows another row's).
    pub locked: bool,
}

/// The split editor.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SplitDescription {
    /// Each line's category and what its amount field holds.
    pub lines: Vec<(String, String)>,
    /// The signed remainder: "−£4.00".
    pub remaining: String,
    pub caption: Option<String>,
}

/// The inspector for several transactions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SeveralDescription {
    /// "3 transactions".
    pub title: String,
    /// The total, when they share a currency.
    pub figure: Option<String>,
    /// What they share: "Uncategorised · Current account".
    pub caption: Option<String>,
    /// The Category select's value, while any of them can take one.
    pub category: Option<String>,
    /// The error said under the Category select (or where it would be):
    /// Categorise's, Mark as transfer's or Create rule's.
    pub category_error: Option<String>,
    /// Each one's date, merchant and amount.
    pub rows: Vec<(String, String, String)>,
}

/// The bar over the table's foot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BarDescription {
    /// An edit on a row already left didn't save.
    Failed {
        message: String,
        actions: Vec<Action>,
    },
    /// After a change to several rows, excluding, a link or a transfer
    /// decision; or an undo that failed.
    Undo {
        message: String,
        actions: Vec<Action>,
    },
    /// Several rows are selected.
    Bulk { count: String, actions: Vec<Action> },
}

/// What a movement key does, in words.
fn chip(chip: ChipText) -> ChipDescription {
    ChipDescription {
        label: chip.label,
        active: chip.active,
        count: chip.count,
    }
}

fn row(
    view: &TransactionsView,
    row: &orre_core::reporting::register::RegisterRow,
) -> RowDescription {
    let RowText {
        merchant,
        cell,
        account,
        amount,
        original,
        editable: _,
        suggested_transfer,
    } = RowText::of(view, row);
    let (category, transfer, excluded) = match cell.kind {
        CellKind::Category { name, .. } => (name, false, false),
        CellKind::Repays(words) => (words, false, false),
        CellKind::Uncategorised => ("Uncategorised".to_owned(), false, false),
        CellKind::Transfer(label) => (label, true, false),
        // As `ui::transaction_row` words it.
        CellKind::Excluded(reason) => (format!("Excluded · {reason}"), false, true),
    };
    RowDescription {
        id: row.id,
        merchant,
        category,
        counts_as: cell.suffix,
        transfer,
        suggested_transfer,
        excluded,
        account,
        amount,
        original,
        selected: view.selection.contains(row.id),
        keyboard: view.selection.current() == Some(row.id),
        marked: view.marks.contains_key(&row.id),
    }
}

fn flat(view: &TransactionsView) -> Vec<RowDescription> {
    if !view.listing.flat {
        return Vec::new();
    }
    view.items
        .iter()
        .filter_map(|item| match *item {
            TableItem::Row(index) => Some(row(view, &view.register.rows()[index])),
            TableItem::Day { .. } => None,
        })
        .collect()
}

fn days(view: &TransactionsView) -> Vec<DayDescription> {
    let mut days: Vec<DayDescription> = Vec::new();
    for item in &view.items {
        match *item {
            TableItem::Day { day, .. } => {
                let day = &view.listing.days[day];
                days.push(DayDescription {
                    heading: present::day_heading(day.date, view.today),
                    net: present::net(&day.net),
                    rows: Vec::new(),
                });
            }
            TableItem::Row(index) => {
                if let Some(day) = days.last_mut() {
                    day.rows.push(row(view, &view.register.rows()[index]));
                }
            }
        }
    }
    days
}

impl TransactionsView {
    /// The state of `menu`'s list: [`TransactionsView::select_state`],
    /// read only.
    fn menu_state_of(&self, menu: Menu) -> Option<&SelectState> {
        match menu {
            Menu::Inspector => Some(&self.inspector_select),
            Menu::SplitLine(key) => self
                .editor
                .split
                .as_ref()?
                .lines
                .iter()
                .find(|line| line.key == key)
                .map(|line| &line.select),
            Menu::Person => Some(&self.editor.person_select),
            Menu::ManualAccount => Some(&self.editor.account_select),
            _ => Some(&self.menu_state),
        }
    }

    fn describe_menu(&self, cx: &App) -> Option<MenuDescription> {
        let menu = self.menu?;
        let chosen = self.chosen_key(menu);
        let highlighted = self
            .menu_state_of(menu)
            .and_then(|state| state.highlighted().cloned());
        let groups = self
            .visible_groups(cx)
            .into_iter()
            .map(|group| MenuGroupDescription {
                label: group.label.map(|label| label.to_string()),
                options: group
                    .options
                    .into_iter()
                    .map(|option| OptionDescription {
                        chosen: option.checked || chosen.as_ref() == Some(&option.key),
                        highlighted: highlighted.as_ref() == Some(&option.key),
                        label: option.label.to_string(),
                        hint: option.hint.map(|hint| hint.to_string()),
                        disabled: option.disabled,
                    })
                    .collect(),
            })
            .collect();
        Some(MenuDescription {
            menu,
            filter: menu
                .has_filter()
                .then(|| self.menu_filter.read(cx).value().to_string()),
            groups,
        })
    }

    fn describe_one(
        &self,
        row: &orre_core::reporting::register::RegisterRow,
        cx: &App,
    ) -> OneDescription {
        let HeaderText {
            title,
            figure,
            struck,
            method,
            time,
        } = HeaderText::of(row);
        let caption = match (method, time) {
            (None, None) => None,
            (Some(method), Some(time)) => Some(format!("{method} · {time}")),
            (method, time) => method.or(time),
        };
        let field = |label, value: String, locked| FieldDescription {
            label,
            value,
            locked,
        };
        let account = self
            .register
            .account(row.account)
            .map_or(String::new(), |account| account.name.clone());
        let mut fields = if row.source.is_manual() {
            vec![
                field("Date", self.editor.date.read(cx).value().to_string(), false),
                field(
                    "Amount",
                    self.editor.amount.read(cx).value().to_string(),
                    false,
                ),
                field("Account", account, false),
            ]
        } else {
            vec![
                field("Date", crate::format::field_date(row.date), true),
                field("Account", account, true),
            ]
        };
        let category = CategoryField::of(self, row);
        let category_drawn = matches!(
            category,
            CategoryField::Select(_) | CategoryField::Locked(_)
        );
        match category {
            CategoryField::Select(value) => fields.push(field(
                "Category",
                value.map_or_else(|| CHOOSE_CATEGORY.to_owned(), |(label, _)| label),
                false,
            )),
            CategoryField::Locked(label) => fields.push(field("Category", label, true)),
            CategoryField::Follows => {
                let (label, _) = super::links::following_label(self, row);
                fields.push(field("Category", label, true));
            }
            CategoryField::Lines => {}
        }
        let tags: Vec<String> = row
            .tags
            .iter()
            .filter_map(|id| self.register.tag(*id))
            .map(|tag| tag.name.clone())
            .collect();
        fields.push(field("Tags", tags.join(", "), false));

        let sections = Section::of(self, row);
        let mut places = vec![EditPlace::Details];
        if struck {
            places.push(EditPlace::Exclusion);
        }
        places.extend(sections.iter().filter_map(|section| section.place()));
        let split = self
            .editor
            .split
            .as_ref()
            .filter(|_| sections.contains(&Section::Split))
            .map(|split| {
                let footer = present::split_footer(&split.draft);
                SplitDescription {
                    lines: split
                        .lines
                        .iter()
                        .map(|line| {
                            let category = self
                                .line_category(line.key)
                                .and_then(|id| self.register.category(id))
                                .map_or_else(
                                    || LINE_PLACEHOLDER.to_owned(),
                                    |category| category.label.clone(),
                                );
                            (category, line.amount.read(cx).value().to_string())
                        })
                        .collect(),
                    remaining: footer.remaining,
                    caption: footer.caption,
                }
            });
        OneDescription {
            title,
            figure,
            struck,
            caption,
            fields,
            category_error: self
                .edit_error
                .as_ref()
                .filter(|_| category_drawn)
                .map(|error| error.to_string()),
            error: self
                .editor
                .error
                .as_ref()
                .filter(|(place, _)| places.contains(place))
                .map(|(place, error)| (*place, error.to_string())),
            sections: sections.into_iter().map(Section::title).collect(),
            split,
        }
    }

    fn describe_inspector(&self, cx: &App) -> InspectorDescription {
        let selected = self.selected();
        match selected.as_slice() {
            [] => match (&self.removed, self.account_inspector(cx)) {
                (Some(note), _) => InspectorDescription::Removed {
                    title: note.title.clone(),
                    body: note.body.clone(),
                },
                (None, Some(_)) => InspectorDescription::Account,
                (None, None) => InspectorDescription::Nothing(NOTHING_SELECTED),
            },
            [row] => InspectorDescription::One(Box::new(self.describe_one(row, cx))),
            many => {
                let SeveralText {
                    title,
                    figure,
                    caption,
                    category,
                    rows,
                } = SeveralText::of(self, many);
                InspectorDescription::Several(SeveralDescription {
                    title,
                    figure,
                    caption,
                    category_error: self.edit_error.as_ref().map(|error| error.to_string()),
                    category: category.map(|value| {
                        value.map_or_else(|| CHOOSE_CATEGORY.to_owned(), |(label, _)| label)
                    }),
                    rows: rows
                        .into_iter()
                        .map(|row| (row.date, row.merchant, row.amount))
                        .collect(),
                })
            }
        }
    }

    fn describe_bar(&self) -> Option<BarDescription> {
        let dismiss = || Action::button("Dismiss", true);
        Some(match self.bar_text()? {
            BarText::Failed(message) => BarDescription::Failed {
                message: message.to_string(),
                actions: vec![Action::button(SHOW, true), dismiss()],
            },
            BarText::Undo(message) => BarDescription::Undo {
                message: message.to_string(),
                actions: vec![Action::button(crate::ui::undo_bar::UNDO, true), dismiss()],
            },
            BarText::Bulk(count) => BarDescription::Bulk {
                count,
                actions: vec![
                    Action::button(CATEGORISE, true),
                    Action::button(MARK_AS_TRANSFER, self.markable_selection().is_some()),
                    Action::button(CREATE_RULE, true),
                    Action::button("Clear selection", true),
                ],
            },
        })
    }

    /// What the Transactions screen shows now, as plain data (module docs).
    pub fn describe(&self, cx: &App) -> TransactionsDescription {
        let ToolbarText {
            account,
            category,
            date,
            uncategorised,
            excluded,
            manual: _,
            button,
        } = ToolbarText::of(self);
        let has_rows = !self.order.is_empty();
        let mut actions: Vec<Action> = TABLE_KEYS
            .moves()
            .into_iter()
            .map(|(key, to)| Action::keyed(to.label(), key, has_rows))
            .collect();
        actions.extend(
            CURSOR_MOVES
                .into_iter()
                .map(|(key, _, label)| Action::keyed(label, key, has_rows)),
        );
        let cursor_listed = self
            .selection
            .current()
            .is_some_and(|id| self.order.contains(&id));
        let title = self.focused_column.is_some();
        actions.extend([
            Action::keyed(TOGGLE_ROW, "space", cursor_listed),
            Action::keyed(
                "Set the category",
                "enter",
                self.editable_current().is_some(),
            ),
            Action::keyed("Clear the selection", "escape", !self.selection.is_empty()),
            Action::keyed(
                "Clear the filters",
                "escape",
                self.selection.is_empty() && self.has_filters(),
            ),
            Action::keyed(SORT_BY_TITLE, "enter", title),
            Action::keyed(SORT_BY_TITLE, "space", title),
        ]);
        TransactionsDescription {
            toolbar: ToolbarDescription {
                search: self.search.read(cx).value().to_string(),
                chips: [account, category, date, uncategorised, excluded]
                    .into_iter()
                    .map(chip)
                    .collect(),
                button,
            },
            account_column: self.filter.account.is_none(),
            sort: self.sort,
            focused_column: self.focused_column,
            empty: self.empty_text().map(
                |EmptyText {
                     title,
                     body,
                     shortcut,
                 }| EmptyDescription {
                    title,
                    body: body.map(|body| body.to_string()),
                    shortcut,
                },
            ),
            days: days(self),
            flat: flat(self),
            menu: self.describe_menu(cx),
            inspector: self.describe_inspector(cx),
            bar: self.describe_bar(),
            actions,
        }
    }
}

impl TransactionsDescription {
    /// Every row, in the table's order.
    pub fn rows(&self) -> impl Iterator<Item = &RowDescription> {
        self.days.iter().flat_map(|day| &day.rows).chain(&self.flat)
    }

    /// The rows the table lists, in its order.
    pub fn listed(&self) -> Vec<TransactionId> {
        self.rows().map(|row| row.id).collect()
    }

    /// The listed rows that wear the row mark, in the table's order.
    pub fn marked(&self) -> Vec<TransactionId> {
        self.rows()
            .filter(|row| row.marked)
            .map(|row| row.id)
            .collect()
    }

    /// The keyboard row, if listed.
    pub fn keyboard_row(&self) -> Option<TransactionId> {
        self.rows().find(|row| row.keyboard).map(|row| row.id)
    }

    /// The selected rows, in the table's order.
    pub fn selected(&self) -> Vec<TransactionId> {
        self.rows()
            .filter(|row| row.selected)
            .map(|row| row.id)
            .collect()
    }

    /// The row `id`, if listed.
    pub fn row(&self, id: TransactionId) -> Option<&RowDescription> {
        self.rows().find(|row| row.id == id)
    }

    /// The inspector for one transaction, if that's what it shows.
    pub fn one(&self) -> Option<&OneDescription> {
        match &self.inspector {
            InspectorDescription::One(one) => Some(one),
            _ => None,
        }
    }

    /// What the undo bar says, while it shows.
    pub fn undo_message(&self) -> Option<&str> {
        match &self.bar {
            Some(BarDescription::Undo { message, .. }) => Some(message),
            _ => None,
        }
    }

    /// What the failure bar says, while it shows.
    pub fn failure_message(&self) -> Option<&str> {
        match &self.bar {
            Some(BarDescription::Failed { message, .. }) => Some(message),
            _ => None,
        }
    }
}
