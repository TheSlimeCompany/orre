//! The inspector's edits to one transaction (ROADMAP M3.9; `docs/DESIGN.md`
//! → "Transactions additions"; Paper `2Q3-0`, `3WT-0`, `4XF-0`;
//! `ARCHITECTURE.md` → "Decision record: splits, tags and notes in the
//! inspector").
//!
//! [`Editor`] holds what the inspector types into: the Merchant and Note
//! fields and the split being edited. The view fills it from the selected
//! row ([`TransactionsView::sync_editor`]) when the selection changes or
//! the register is read again, and every change goes through the edit
//! pipeline (`transactions::write`, [`Policy::edit`]) to core's `edit`, one
//! undoable change batch each, then reads the register again so the table
//! shows it.
//!
//! - **Merchant and Note** save when the field loses focus or on Enter, and
//!   when the selection moves on (what was typed is kept, as the account
//!   inspector does). A field being typed in isn't overwritten by a reload.
//! - **The split** is core's [`SplitDraft`]: the remainder, the sign, "Add
//!   line" filling in the rest and whether it can be saved are all core's.
//!   It saves whenever it is balanced and differs from what's stored (an
//!   amount field left or Enter, a category chosen, a line added or
//!   removed). An unbalanced draft isn't saved and stays through reloads;
//!   leaving the row drops it, so the last balanced split stays.
//! - **Tags** are added and removed from the tag menu and a pill's ✕.
//! - **A manual row's Date and Amount** (M3.10) save like the text fields,
//!   through core's `manual::update`; its Account select saves on a choice
//!   (`transactions::manual_edit`).

use coffer_core::edit;
use coffer_core::id::{TagId, TransactionId};
use coffer_core::money::Money;
use coffer_core::reporting::register::{RegisterRow, RowCategory};
use coffer_core::split::SplitLine;
use coffer_core::split::draft::SplitDraft;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::{
    AppContext as _, Context, Entity, FocusHandle, Focusable as _, SharedString, Subscription,
    Window,
};

use crate::format::{self, Sign};
use crate::ui::menu::SelectState;

use super::TransactionsView;
use super::write::Policy;

/// Where an edit's error is said.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditPlace {
    /// Under the Details fields (merchant, tags, note).
    Details,
    /// Under the split editor.
    Split,
    /// Under the Share section (M3.11).
    Share,
    /// Under the Links section (M3.11).
    Links,
    /// Under the Source section (M3.10).
    Source,
    /// Inside the exclusion card (M3.10).
    Exclusion,
    /// Under the Transfer section (M3.8).
    Transfer,
}

/// One line's fields in the split editor.
pub(super) struct LineFields {
    /// Stable while the line exists: its menu's and elements' identity.
    pub(super) key: u64,
    pub(super) amount: Entity<InputState>,
    pub(super) select: SelectState,
    _subscription: Subscription,
}

/// The split being edited and its lines' fields, in the draft's order.
pub(super) struct SplitEditor {
    pub(super) draft: SplitDraft,
    pub(super) lines: Vec<LineFields>,
}

impl SplitEditor {
    fn index(&self, key: u64) -> Option<usize> {
        self.lines.iter().position(|line| line.key == key)
    }
}

/// The inspector's fields for the selected transaction.
pub(super) struct Editor {
    /// The transaction the fields were last filled from.
    pub(super) shown: Option<TransactionId>,
    /// The register read they were filled from.
    generation: u64,
    pub(super) merchant: Entity<InputState>,
    pub(super) note: Entity<InputState>,
    /// What the Merchant, Note, Date and Amount fields were last filled
    /// with, as they read it back: only text the user changed is saved, so
    /// a value the field can't hold as stored (a note over several lines,
    /// in a one-line field) is never rewritten behind their back.
    pub(super) filled: [SharedString; 4],
    /// A manual row's Date field (M3.10).
    pub(super) date: Entity<InputState>,
    /// A manual row's Amount field, signed, with the currency as its unit.
    pub(super) amount: Entity<InputState>,
    /// A manual row's Account select.
    pub(super) account_select: SelectState,
    pub(super) split: Option<SplitEditor>,
    /// The "+ Tag" pill, where focus goes back when its menu closes.
    pub(super) tag_focus: FocusHandle,
    /// The split editor's "Add line".
    pub(super) add_line_focus: FocusHandle,
    /// Why the last edit wasn't saved, and where to say so.
    pub(super) error: Option<(EditPlace, SharedString)>,
    /// An error to say once the editor shows that row again: the failure
    /// bar's Show, for an edit that didn't save as the user left the row.
    pub(super) restore_error: Option<(TransactionId, EditPlace, SharedString)>,
    /// "Create a rule from this edit?" for the row just categorised (M3.13).
    pub(super) rule_prompt: Option<super::rule_prompt::Prompt>,
    /// The Share section's Yours: the user's part, unsigned (M3.11).
    pub(super) yours: Entity<InputState>,
    /// "Shared" was chosen and waits for a person before it's saved.
    pub(super) shared_draft: bool,
    /// The Share section's Owed by select.
    pub(super) person_select: SelectState,
    next_key: u64,
}

impl Editor {
    /// The fields, empty, and their subscriptions: each saves on blur and
    /// Enter.
    pub(super) fn new(
        window: &mut Window,
        cx: &mut Context<TransactionsView>,
    ) -> (Self, Vec<Subscription>) {
        let merchant = cx.new(|cx| InputState::new(window, cx));
        let note = cx.new(|cx| InputState::new(window, cx).placeholder("Add a note"));
        let yours = cx.new(|cx| InputState::new(window, cx));
        let date = cx.new(|cx| InputState::new(window, cx));
        let amount = cx.new(|cx| InputState::new(window, cx));
        let subscriptions = vec![
            cx.subscribe_in(&date, window, |this, _, event, window, cx| {
                if matches!(event, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                    this.commit_manual_date(window, cx);
                }
            }),
            cx.subscribe_in(&amount, window, |this, _, event, window, cx| {
                if matches!(event, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                    this.commit_manual_amount(window, cx);
                }
            }),
            cx.subscribe_in(&merchant, window, |this, _, event, _, cx| {
                if matches!(event, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                    this.commit_merchant(cx);
                }
            }),
            cx.subscribe_in(&note, window, |this, _, event, _, cx| {
                if matches!(event, InputEvent::Blur | InputEvent::PressEnter { .. }) {
                    this.commit_note(cx);
                }
            }),
            cx.subscribe_in(&yours, window, |this, _, event, _, cx| match event {
                InputEvent::Blur | InputEvent::PressEnter { .. } => this.commit_yours(cx),
                // The caption follows what's typed.
                InputEvent::Change => cx.notify(),
                InputEvent::Focus => {}
            }),
        ];
        let editor = Self {
            shown: None,
            generation: 0,
            merchant,
            note,
            filled: Default::default(),
            date,
            amount,
            account_select: SelectState::new(cx),
            split: None,
            tag_focus: cx.focus_handle().tab_stop(true),
            add_line_focus: cx.focus_handle(),
            error: None,
            restore_error: None,
            rule_prompt: None,
            yours,
            shared_draft: false,
            person_select: SelectState::new(cx),
            next_key: 0,
        };
        (editor, subscriptions)
    }

    /// The select state of split line `key`.
    pub(super) fn line_select(&mut self, key: u64) -> Option<&mut SelectState> {
        let split = self.split.as_mut()?;
        let index = split.index(key)?;
        Some(&mut split.lines[index].select)
    }

    /// The error to show at `place`.
    pub(super) fn error_at(&self, place: EditPlace) -> Option<SharedString> {
        self.error
            .as_ref()
            .filter(|(at, _)| *at == place)
            .map(|(_, error)| error.clone())
    }
}

/// `row`'s stored split as lines to save, its categories by key; empty
/// when it isn't split.
fn stored_lines(view: &TransactionsView, row: &RegisterRow) -> Vec<SplitLine> {
    let RowCategory::Split(parts) = &row.category else {
        return Vec::new();
    };
    parts
        .iter()
        .map(|part| SplitLine {
            amount: part.amount,
            category: part
                .category
                .and_then(|id| view.register.category(id))
                .map(|category| category.key.clone()),
            my_share: part.my_share,
            share_label: part.share_label.clone(),
            user_note: part.user_note.clone(),
        })
        .collect()
}

/// What an amount field shows for `amount`: "−£38.66"; empty for none.
fn amount_text(amount: Option<Money>) -> SharedString {
    amount.map_or_else(SharedString::default, |amount| {
        format::money(amount, Sign::Always).into()
    })
}

impl TransactionsView {
    /// The one selected row, if exactly one is selected.
    pub(super) fn single_row(&self) -> Option<&RegisterRow> {
        if self.selection.len() != 1 {
            return None;
        }
        self.selection
            .current()
            .and_then(|id| self.register.row(id))
            .or_else(|| self.selected().into_iter().next())
    }

    /// Fills the inspector's fields from the selected row when it changes,
    /// and refreshes them after the register is read again. Called before
    /// each render.
    pub(super) fn sync_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let row = self.single_row().cloned();
        let shown = row.as_ref().map(|row| row.id);
        if shown != self.editor.shown {
            // Keep what was typed for the row being left: its fields' blur
            // arrives after this render, when they show the new row. A
            // balanced split typed but not yet left is saved too. What
            // doesn't save can't be said under the fields, which show the
            // new row next: the failure bar says it (and an error already
            // said there is said again if saving still fails).
            let left = self.editor.shown;
            self.editor.error = None;
            self.commit_merchant(cx);
            self.commit_note(cx);
            self.commit_yours(cx);
            self.commit_manual_date(window, cx);
            self.commit_manual_amount(window, cx);
            self.save_split(cx);
            if let Some(left) = left {
                self.say_on_failure_bar(left);
            }
            self.editor.shown = shown;
            self.editor.shared_draft = false;
            self.editor.generation = self.generation;
            self.editor.error = self
                .editor
                .restore_error
                .take()
                .filter(|(row, _, _)| Some(*row) == shown)
                .map(|(_, place, reason)| (place, reason));
            self.editor.split = None;
            // The prompt goes away when the selection changes (D6.3).
            if self
                .editor
                .rule_prompt
                .as_ref()
                .is_some_and(|prompt| Some(prompt.proposal.transaction) != shown)
            {
                self.editor.rule_prompt = None;
            }
            // A menu opened for the row being left closes without moving
            // focus, as the account inspector's selects do.
            match self.menu {
                Some(super::Menu::Inspector) => {
                    self.inspector_select.dismiss();
                    self.menu = None;
                }
                Some(super::Menu::Tags) => {
                    self.menu_state.dismiss();
                    self.menu = None;
                }
                // The line's select goes with the split editor below.
                Some(super::Menu::SplitLine(_)) => self.menu = None,
                Some(super::Menu::Person) => {
                    self.editor.person_select.dismiss();
                    self.menu = None;
                }
                Some(super::Menu::ManualAccount) => {
                    self.editor.account_select.dismiss();
                    self.menu = None;
                }
                _ => {}
            }
            if let Some(row) = &row {
                self.fill_text(row, true, window, cx);
                self.editor.split = self.stored_split(row, window, cx);
            }
            return;
        }
        if self.editor.generation == self.generation {
            return;
        }
        self.editor.generation = self.generation;
        let Some(row) = row else {
            return;
        };
        self.fill_text(&row, false, window, cx);
        // An unsaved draft stays; otherwise the split follows the database.
        let editing = self
            .editor
            .split
            .as_ref()
            .is_some_and(|split| !split.draft.status().is_balanced());
        if editing {
            return;
        }
        let stored = stored_lines(self, &row);
        let current = self
            .editor
            .split
            .as_ref()
            .and_then(|split| split.draft.to_lines())
            .unwrap_or_default();
        if stored != current {
            self.editor.split = self.stored_split(&row, window, cx);
        }
    }

    /// Puts `row`'s merchant name and note in their fields; unless `force`,
    /// only into a field that isn't being typed in.
    fn fill_text(
        &mut self,
        row: &RegisterRow,
        force: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A name the user set is theirs to edit; otherwise the field is
        // empty and shows the name the table uses as its placeholder.
        let (merchant, placeholder) = if row.merchant_set {
            (row.merchant.clone(), String::new())
        } else {
            (String::new(), row.merchant.clone())
        };
        let note = row.note.clone().unwrap_or_default();
        // A share waiting for its person keeps what was typed.
        if !self.editor.shared_draft {
            let yours = self.editor.yours.clone();
            let typing = yours.read(cx).focus_handle(cx).is_focused(window);
            if force || !typing {
                let value = row
                    .share
                    .my_share
                    .map(|mine| {
                        coffer_core::money::Money::new(
                            mine.minor().saturating_abs(),
                            mine.currency(),
                        )
                        .to_decimal_string()
                    })
                    .unwrap_or_default();
                yours.update(cx, |input, cx| {
                    if input.value().as_ref() != value {
                        input.set_value(SharedString::from(value), window, cx);
                    }
                });
            }
        }
        for (slot, input, value, placeholder) in [
            (0, self.editor.merchant.clone(), merchant, Some(placeholder)),
            (1, self.editor.note.clone(), note, None),
        ] {
            let typing = input.read(cx).focus_handle(cx).is_focused(window);
            if !force && typing {
                continue;
            }
            let filled = input.update(cx, |input, cx| {
                if let Some(placeholder) = placeholder {
                    input.set_placeholder(placeholder, window, cx);
                }
                if input.value().as_ref() != value {
                    input.set_value(SharedString::from(value), window, cx);
                }
                input.value()
            });
            self.editor.filled[slot] = filled;
        }
        self.fill_manual(row, force, window, cx);
    }

    /// The split editor for `row`'s stored split, if it has one.
    fn stored_split(
        &mut self,
        row: &RegisterRow,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<SplitEditor> {
        let lines = stored_lines(self, row);
        if lines.is_empty() {
            return None;
        }
        Some(self.split_editor(SplitDraft::from_lines(row.amount, &lines), window, cx))
    }

    /// The editor for `draft`, a field per line.
    fn split_editor(
        &mut self,
        draft: SplitDraft,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> SplitEditor {
        let lines = draft
            .lines()
            .iter()
            .map(|line| self.line_fields(line.amount, window, cx))
            .collect();
        SplitEditor { draft, lines }
    }

    /// A new line's fields, its amount field showing `amount`.
    fn line_fields(
        &mut self,
        amount: Option<Money>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> LineFields {
        let key = self.editor.next_key;
        self.editor.next_key += 1;
        let input = cx.new(|cx| {
            let mut input = InputState::new(window, cx);
            input.set_value(amount_text(amount), window, cx);
            input
        });
        let subscription =
            cx.subscribe_in(&input, window, move |this, input, event, window, cx| {
                this.line_amount_event(key, input, event, window, cx);
            });
        LineFields {
            key,
            amount: input,
            select: SelectState::new(cx),
            _subscription: subscription,
        }
    }

    /// Typing in line `key`'s amount updates the remainder; leaving the
    /// field or Enter shows the amount as stored and saves a balanced split.
    pub(super) fn line_amount_event(
        &mut self,
        key: u64,
        input: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(split) = self.editor.split.as_mut() else {
            return;
        };
        let Some(index) = split.index(key) else {
            return;
        };
        match event {
            InputEvent::Change => {
                let text = input.read(cx).value();
                let currency = split.draft.parent().currency();
                let amount = format::parse_amount(&text, currency).ok();
                split.draft.set_amount(index, amount);
                cx.notify();
            }
            InputEvent::Blur | InputEvent::PressEnter { .. } => {
                if let Some(amount) = split.draft.lines()[index].amount {
                    let text = amount_text(Some(amount));
                    if input.read(cx).value() != text {
                        input.update(cx, |input, cx| input.set_value(text, window, cx));
                    }
                }
                self.save_split(cx);
            }
            InputEvent::Focus => {}
        }
    }

    /// Starts a split of the selected row: one line with the whole amount,
    /// its amount field focused.
    pub(super) fn start_split(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(row) = self.single_row().cloned() else {
            return;
        };
        if !row.splittable || matches!(row.category, RowCategory::Split(_)) {
            return;
        }
        let category = match row.category {
            RowCategory::Category(id) => self.register.category(id).map(|c| c.key.clone()),
            _ => None,
        };
        let split = self.split_editor(SplitDraft::start(row.amount, category), window, cx);
        if let Some(first) = split.lines.first() {
            first.amount.update(cx, |input, cx| input.focus(window, cx));
        }
        self.editor.split = Some(split);
        self.editor.error = None;
        cx.notify();
    }

    /// Adds a line holding the remainder (`DESIGN.md`: "Add line" prefills
    /// it), focuses its amount and saves the split if that balances it.
    pub(super) fn add_split_line(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(mut split) = self.editor.split.take() else {
            return;
        };
        let index = split.draft.add_line();
        let fields = self.line_fields(split.draft.lines()[index].amount, window, cx);
        fields
            .amount
            .update(cx, |input, cx| input.focus(window, cx));
        split.lines.push(fields);
        self.editor.split = Some(split);
        self.save_split(cx);
        cx.notify();
    }

    /// Removes line `key` and saves the split if it's still balanced.
    pub(super) fn remove_split_line(&mut self, key: u64, cx: &mut Context<Self>) {
        let Some(split) = self.editor.split.as_mut() else {
            return;
        };
        let Some(index) = split.index(key) else {
            return;
        };
        split.draft.remove_line(index);
        split.lines.remove(index);
        if self.menu == Some(super::Menu::SplitLine(key)) {
            self.menu = None;
        }
        self.save_split(cx);
        cx.notify();
    }

    /// Sets line `key`'s category to the category with ID `category`.
    pub(super) fn set_line_category(&mut self, key: u64, category: i64, cx: &mut Context<Self>) {
        let Some(category) = self.register.category(category).map(|c| c.key.clone()) else {
            return;
        };
        let Some(split) = self.editor.split.as_mut() else {
            return;
        };
        let Some(index) = split.index(key) else {
            return;
        };
        split.draft.set_category(index, Some(category));
        self.save_split(cx);
        cx.notify();
    }

    /// The category ID of line `key`, for its select.
    pub(super) fn line_category(&self, key: u64) -> Option<i64> {
        let split = self.editor.split.as_ref()?;
        let category = split.draft.lines()[split.index(key)?].category.as_ref()?;
        self.register
            .categories()
            .iter()
            .find(|candidate| &candidate.key == category)
            .map(|candidate| candidate.id)
    }

    /// Saves the draft when it's balanced and differs from what's stored,
    /// on the row the editor shows (which may be the one being left).
    fn save_split(&mut self, cx: &mut Context<Self>) {
        let Some(row) = self
            .editor
            .shown
            .and_then(|id| self.register.row(id))
            .cloned()
        else {
            return;
        };
        let Some(lines) = self
            .editor
            .split
            .as_ref()
            .and_then(|split| split.draft.to_lines())
        else {
            return;
        };
        if lines == stored_lines(self, &row) {
            return;
        }
        self.write(Policy::edit(EditPlace::Split), cx, |db| {
            edit::set_split(db, row.id, &lines)
        })
        .ok();
    }

    /// "Remove split": takes the stored split off (the row is left
    /// uncategorised), or drops a split not saved yet.
    pub(super) fn remove_split(&mut self, cx: &mut Context<Self>) {
        let Some(row) = self.single_row().cloned() else {
            return;
        };
        self.editor.split = None;
        self.editor.error = None;
        if matches!(row.category, RowCategory::Split(_)) {
            self.write(Policy::edit(EditPlace::Split), cx, |db| {
                edit::remove_split(db, row.id)
            })
            .ok();
        }
        cx.notify();
    }

    /// Adds `tag` to the selected row, or takes it off when it has it.
    pub(super) fn toggle_tag(&mut self, tag: TagId, cx: &mut Context<Self>) {
        let Some(row) = self.single_row().cloned() else {
            return;
        };
        let policy = Policy::edit(EditPlace::Details);
        if row.tags.contains(&tag) {
            self.write(policy, cx, |db| edit::remove_tag(db, row.id, tag))
                .ok();
        } else {
            self.write(policy, cx, |db| edit::add_tag(db, row.id, tag))
                .ok();
        }
    }

    /// Takes `tag` off the selected row (a pill's ✕).
    pub(super) fn remove_tag(&mut self, tag: TagId, cx: &mut Context<Self>) {
        let Some(row) = self.single_row().cloned() else {
            return;
        };
        self.write(Policy::edit(EditPlace::Details), cx, |db| {
            edit::remove_tag(db, row.id, tag)
        })
        .ok();
    }

    /// Tags the selected row with the tag named `name`, creating it.
    pub(super) fn create_tag(&mut self, name: &str, private: bool, cx: &mut Context<Self>) {
        let Some(row) = self.single_row().cloned() else {
            return;
        };
        self.write(Policy::edit(EditPlace::Details), cx, |db| {
            edit::add_new_tag(db, row.id, name, private).map(|(_, batch)| batch)
        })
        .ok();
    }

    /// Saves the Merchant field when it differs from the stored name.
    fn commit_merchant(&mut self, cx: &mut Context<Self>) {
        let Some(row) = self
            .editor
            .shown
            .and_then(|id| self.register.row(id))
            .cloned()
        else {
            return;
        };
        let typed = self.editor.merchant.read(cx).value();
        if typed == self.editor.filled[0] {
            return;
        }
        let typed = typed.trim().to_owned();
        let stored = if row.merchant_set {
            row.merchant.trim()
        } else {
            ""
        };
        if typed != stored {
            self.write(Policy::edit(EditPlace::Details), cx, |db| {
                edit::set_merchant(db, row.id, &typed)
            })
            .ok();
        }
    }

    /// Saves the Note field when it differs from the stored note.
    fn commit_note(&mut self, cx: &mut Context<Self>) {
        let Some(row) = self
            .editor
            .shown
            .and_then(|id| self.register.row(id))
            .cloned()
        else {
            return;
        };
        let typed = self.editor.note.read(cx).value();
        if typed == self.editor.filled[1] {
            return;
        }
        let typed = typed.trim().to_owned();
        if typed != row.note.as_deref().unwrap_or_default().trim() {
            self.write(Policy::edit(EditPlace::Details), cx, |db| {
                edit::set_note(db, row.id, &typed)
            })
            .ok();
        }
    }
}
