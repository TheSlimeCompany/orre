//! The Rules view's canvas and inspector (`docs/DESIGN.md` → "Rules";
//! Paper `B1J-0`, `EFT-0`, `G2O-0`, `GZT-0`, `HFH-0`).
//!
//! The view owns what needs a window: the text fields, the selects' states
//! and which menu is open. It keeps the fields in step with the
//! [`Rules`] entity's form, and sends every edit back to it: a keystroke
//! changes the form (a new rule's preview follows at once), leaving a field
//! or Enter saves a stored rule or name, and a select saves at once.
//!
//! The canvas is [`super::table`]; the inspector is [`super::inspector`].
//! After an apply, or a rule moved with ⌥↑⌥↓, the D6.3 undo bar floats at
//! the foot of the canvas; a change core refuses is said under the toolbar
//! in caption type, `danger`. ⌥↑⌥↓ work only while the table itself has
//! focus, not in the inspector's fields.

use std::collections::HashMap;

use coffer_core::category::{CategoryKey, CategoryKind};
use coffer_core::id::AccountId;
use coffer_core::merchant::AliasKind;
use coffer_core::rule::{ActionKind, ExcludeReason, MatchField, MatchKind};
use gpui_kit::base::actions::Cancel;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::{
    AnyElement, App, AppContext as _, ClickEvent, Context, Entity, FocusHandle, Focusable as _,
    InteractiveElement as _, IntoElement, MouseButton, MouseDownEvent, ParentElement as _, Render,
    SharedString, Styled as _, Subscription, Window, div,
};

use crate::theme::tokens::{color, size, spacing, typography};
use crate::ui::icon::Glyph;
use crate::ui::keys::{Key, ViewKeys};
use crate::ui::list::{Keys, ListKeysExt as _, Move};
use crate::ui::menu::{MenuGroup, MenuList, MenuOption, SelectState, filter_groups};
use crate::ui::text::TextRoleExt as _;
use crate::ui::undo_bar::UndoBar;

use crate::accounts::DialogClosed;

use super::dialogs::DeleteCategoryDialog;
use super::form::{ActionForm, AmountChoice, Direction, SplitLineForm, Unavailable};
use super::store::RulesData;
use super::text;
use super::{MoveRuleDown, MoveRuleUp, Outcome, Rules};

/// The key context of the Rules canvas, where ↑↓, ⌥↑⌥↓ and Escape work.
pub const CONTEXT: &str = "CofferRules";

/// ⌥↑ and ⌥↓ (Alt on Linux and Windows): move the selected rule one place
/// (`DESIGN.md` → "Rules" → "Keyboard reorder"), hinted as one "⌥↑↓
/// Reorder".
pub const REORDER: Key =
    Key::new::<MoveRuleUp>(&["alt-up"], "⌥↑↓", "Reorder").and::<MoveRuleDown>(&["alt-down"]);

/// Escape: back from the preview, else clears the selection. Not hinted.
pub const BACK: Key =
    Key::new::<Cancel>(&["escape"], "Esc", "Back, or clear the selection").unhinted();

/// The canvas's keys, declared once (ROADMAP U.7): ↑↓ through the open
/// section's rows, [`REORDER`] and [`BACK`]. `init` binds them, the
/// toolbar hints them ("↑↓ Select", then "⌥↑↓ Reorder") and the test-only
/// `describe()` lists them.
pub const KEYS: ViewKeys = ViewKeys::new(Keys::new(CONTEXT), &[REORDER, BACK]).select_first();

/// The most rows a menu shows before it scrolls.
pub const MENU_ROWS: f32 = 12.;

/// Option keys that aren't IDs.
const ANY: &str = "any";
const NEW_TAG: &str = "new-tag";
/// The Group select's "New group" (no category key has a `-`).
const NEW_GROUP: &str = "new-group";

/// Which select or menu is open.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Menu {
    Field,
    Test,
    Account,
    Amount,
    Position,
    Category,
    Exclude,
    SplitLine(usize),
    NameTest,
    /// A new category's Group.
    CategoryGroup,
    /// "+ Add action".
    AddAction,
    /// The Tag action's "+ Tag".
    Tags,
}

impl Menu {
    /// Whether it has the filter field: the category and tag menus.
    fn has_filter(self) -> bool {
        matches!(self, Menu::Category | Menu::SplitLine(_) | Menu::Tags)
    }

    /// Whether it opens from a button (its own keys) rather than a select.
    fn is_popup(self) -> bool {
        matches!(self, Menu::AddAction | Menu::Tags)
    }
}

/// A field the view keeps in step with the form.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Input {
    Pattern,
    Low,
    High,
    Merchant,
    Note,
    Share,
    SplitPercent(usize),
    NamePattern,
    NameShows,
    /// A category's Name: a stored one's (a rename) or the new one's.
    CategoryName,
}

/// The Rules canvas and inspector.
pub struct RulesView {
    rules: Entity<Rules>,
    focus: FocusHandle,
    pub(super) inputs: HashMap<Input, Entity<InputState>>,
    pub(super) selects: HashMap<Menu, SelectState>,
    /// The popup menus' state ("+ Add action", "+ Tag").
    pub(super) popup: SelectState,
    pub(super) menu: Option<Menu>,
    pub(super) menu_filter: Entity<InputState>,
    pub(super) add_action_focus: FocusHandle,
    pub(super) tag_focus: FocusHandle,
    /// What the fields were last filled from.
    filled_for: Option<super::FormKey>,
    /// The Delete category dialog while it's open, and its subscription.
    delete_category: Option<(Entity<DeleteCategoryDialog>, Subscription)>,
    _subscriptions: Vec<Subscription>,
}

impl RulesView {
    pub fn new(rules: Entity<Rules>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let menu_filter = cx.new(|cx| InputState::new(window, cx).placeholder("Find a category"));
        let mut subscriptions = vec![
            cx.observe_in(&rules, window, |this, _, window, cx| this.sync(window, cx)),
            cx.subscribe_in(&menu_filter, window, |this, _, event, _, cx| {
                if matches!(event, InputEvent::Change) {
                    let groups = this.visible_groups(cx);
                    if let Some(menu) = this.menu
                        && let Some(state) = this.state_mut(menu)
                    {
                        state.highlight_first(&groups);
                    }
                    cx.notify();
                }
            }),
        ];
        let mut view = Self {
            rules,
            focus: cx.focus_handle(),
            inputs: HashMap::new(),
            selects: HashMap::new(),
            popup: SelectState::new(cx),
            menu: None,
            menu_filter,
            add_action_focus: cx.focus_handle(),
            tag_focus: cx.focus_handle(),
            filled_for: None,
            delete_category: None,
            _subscriptions: Vec::new(),
        };
        for input in [
            Input::Pattern,
            Input::Low,
            Input::High,
            Input::Merchant,
            Input::Note,
            Input::Share,
            Input::NamePattern,
            Input::NameShows,
            Input::CategoryName,
        ] {
            subscriptions.push(view.add_input(input, window, cx));
        }
        for menu in [
            Menu::Field,
            Menu::Test,
            Menu::Account,
            Menu::Amount,
            Menu::Position,
            Menu::Category,
            Menu::Exclude,
            Menu::NameTest,
            Menu::CategoryGroup,
        ] {
            view.selects.insert(menu, SelectState::new(cx));
        }
        view._subscriptions = subscriptions;
        view.sync(window, cx);
        view
    }

    pub fn rules(&self) -> &Entity<Rules> {
        &self.rules
    }

    /// The canvas's focus: where ↑↓ and Escape work.
    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus
    }

    /// ⌥↑ or ⌥↓: moves the selected rule, but only while the table itself
    /// has focus. The binding's context takes in the inspector too, where
    /// the keys stay with the field, so anywhere else they go on.
    fn reorder(&mut self, down: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !self.focus.is_focused(window) {
            cx.propagate();
            return;
        }
        self.rules
            .update(cx, |rules, cx| rules.reorder_selected(down, cx));
    }

    fn add_input(
        &mut self,
        input: Input,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Subscription {
        let placeholder = match input {
            Input::Pattern | Input::NamePattern => "Text to match",
            Input::Low | Input::High => "0.00",
            Input::Merchant | Input::NameShows => "Name to show",
            Input::Note => "Note to add",
            Input::Share | Input::SplitPercent(_) => "0",
            Input::CategoryName => "Category name",
        };
        let state = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
        let subscription = cx.subscribe_in(&state, window, move |this, state, event, _, cx| {
            let save = match event {
                InputEvent::Change => false,
                InputEvent::Blur | InputEvent::PressEnter { .. } => true,
                _ => return,
            };
            let value = state.read(cx).value().to_string();
            this.typed(input, value, save, cx);
        });
        self.inputs.insert(input, state);
        subscription
    }

    pub(super) fn input(&self, input: Input) -> Option<&Entity<InputState>> {
        self.inputs.get(&input)
    }

    /// Makes sure every split line has its fields.
    fn ensure_split_inputs(&mut self, lines: usize, window: &mut Window, cx: &mut Context<Self>) {
        for line in 0..lines {
            if !self.inputs.contains_key(&Input::SplitPercent(line)) {
                let subscription = self.add_input(Input::SplitPercent(line), window, cx);
                self._subscriptions.push(subscription);
            }
            self.selects
                .entry(Menu::SplitLine(line))
                .or_insert_with(|| SelectState::new(cx));
        }
    }

    /// What the form holds for `input`.
    fn form_value(&self, input: Input, cx: &App) -> Option<String> {
        let rules = self.rules.read(cx);
        if input == Input::CategoryName {
            return rules.category_form().map(|form| form.name.clone());
        }
        if matches!(input, Input::NamePattern | Input::NameShows) {
            let form = rules.name_form()?;
            return Some(match input {
                Input::NamePattern => form.pattern.clone(),
                _ => form.name.clone(),
            });
        }
        let form = rules.form()?;
        Some(match input {
            Input::Pattern => form.text.clone(),
            Input::Low => form.amount.low.clone(),
            Input::High => form.amount.high.clone(),
            Input::Merchant => match form.action(ActionKind::SetMerchant)? {
                ActionForm::Merchant(text) => text.clone(),
                _ => return None,
            },
            Input::Note => match form.action(ActionKind::SetNote)? {
                ActionForm::Note(text) => text.clone(),
                _ => return None,
            },
            Input::Share => match form.action(ActionKind::MyShare)? {
                ActionForm::Share(text) => text.clone(),
                _ => return None,
            },
            Input::SplitPercent(line) => match form.action(ActionKind::SplitTemplate)? {
                ActionForm::Split(lines) => lines.get(line)?.percent.clone(),
                _ => return None,
            },
            Input::NamePattern | Input::NameShows | Input::CategoryName => return None,
        })
    }

    /// A field changed: the form follows, and a stored rule or name saves
    /// when `save` (blur, Enter).
    fn typed(&mut self, input: Input, value: String, save: bool, cx: &mut Context<Self>) {
        // A late blur from fields filled for a rule or name since left.
        if self.filled_for != Some(self.rules.read(cx).form_key()) {
            return;
        }
        if !save && self.form_value(input, cx).as_deref() == Some(value.as_str()) {
            return;
        }
        self.rules.update(cx, |rules, cx| match input {
            Input::NamePattern => rules.edit_name(save, |form| form.pattern = value, cx),
            Input::NameShows => rules.edit_name(save, |form| form.name = value, cx),
            Input::CategoryName => rules.edit_category(save, |form| form.name = value, cx),
            _ => rules.edit(
                save,
                |form| match input {
                    Input::Pattern => form.text = value,
                    Input::Low => form.amount.low = value,
                    Input::High => form.amount.high = value,
                    Input::Merchant => {
                        if let Some(ActionForm::Merchant(text)) =
                            form.action_mut(ActionKind::SetMerchant)
                        {
                            *text = value;
                        }
                    }
                    Input::Note => {
                        if let Some(ActionForm::Note(text)) = form.action_mut(ActionKind::SetNote) {
                            *text = value;
                        }
                    }
                    Input::Share => {
                        if let Some(ActionForm::Share(text)) = form.action_mut(ActionKind::MyShare)
                        {
                            *text = value;
                        }
                    }
                    Input::SplitPercent(line) => {
                        if let Some(ActionForm::Split(lines)) =
                            form.action_mut(ActionKind::SplitTemplate)
                            && let Some(line) = lines.get_mut(line)
                        {
                            line.percent = value;
                        }
                    }
                    Input::NamePattern | Input::NameShows | Input::CategoryName => {}
                },
                cx,
            ),
        });
    }

    /// Puts the form's values into fields that aren't being typed in.
    fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let lines = match self
            .rules
            .read(cx)
            .form()
            .and_then(|form| form.action(ActionKind::SplitTemplate))
        {
            Some(ActionForm::Split(lines)) => lines.len(),
            _ => 0,
        };
        self.ensure_split_inputs(lines, window, cx);
        // Another rule or name: every field takes its values, even the one
        // focused.
        let key = self.rules.read(cx).form_key();
        let moved = self.filled_for != Some(key);
        self.filled_for = Some(key);
        let inputs: Vec<(Input, Entity<InputState>)> = self
            .inputs
            .iter()
            .map(|(input, state)| (*input, state.clone()))
            .collect();
        for (input, state) in inputs {
            let value = self.form_value(input, cx).unwrap_or_default();
            let (focused, now) = {
                let state = state.read(cx);
                (
                    state.focus_handle(cx).is_focused(window),
                    state.value().to_string(),
                )
            };
            if (moved || !focused) && now != value {
                state.update(cx, |state, cx| state.set_value(value, window, cx));
            }
        }
        cx.notify();
    }

    // Menus --------------------------------------------------------------------

    pub(super) fn state(&self, menu: Menu) -> &SelectState {
        if menu.is_popup() {
            &self.popup
        } else {
            self.selects.get(&menu).unwrap_or(&self.popup)
        }
    }

    fn state_mut(&mut self, menu: Menu) -> Option<&mut SelectState> {
        if menu.is_popup() {
            Some(&mut self.popup)
        } else {
            self.selects.get_mut(&menu)
        }
    }

    /// The menu's options before the filter narrows them.
    pub(super) fn menu_groups(&self, menu: Menu, cx: &App) -> Vec<MenuGroup> {
        let rules = self.rules.read(cx);
        let data = rules.data();
        match menu {
            Menu::Field => vec![MenuGroup::new(
                None,
                text::FIELDS
                    .iter()
                    .map(|field| MenuOption::new(field.as_str(), text::field(*field)))
                    .collect(),
            )],
            Menu::Test => vec![MenuGroup::new(
                None,
                text::TESTS
                    .iter()
                    .map(|kind| MenuOption::new(kind.as_str(), text::test(*kind)))
                    .collect(),
            )],
            Menu::Account => {
                let mut options = vec![MenuOption::new(ANY, "Any account")];
                options.extend(data.accounts.iter().map(|account| {
                    MenuOption::new(
                        account.id.to_string(),
                        text::plain(&account.name, text::NAME_SHOWN),
                    )
                }));
                vec![MenuGroup::new(None, options)]
            }
            Menu::Amount => {
                let custom = rules
                    .form()
                    .is_some_and(|form| form.amount.choice == AmountChoice::Custom);
                let mut options: Vec<MenuOption> = [
                    AmountChoice::Any,
                    AmountChoice::Over,
                    AmountChoice::Under,
                    AmountChoice::Between,
                ]
                .into_iter()
                .map(|choice| MenuOption::new(amount_key(choice), amount_label(choice)))
                .collect();
                if custom {
                    options.push(
                        MenuOption::new(amount_key(AmountChoice::Custom), "Custom").disabled(true),
                    );
                }
                vec![MenuGroup::new(None, options)]
            }
            Menu::Position => {
                let count = data.rules.len() + 1;
                let options = (0..count)
                    .map(|at| {
                        let label = match at {
                            0 => "First".to_owned(),
                            at if at + 1 == count => "Last".to_owned(),
                            at => format!("After rule {at}"),
                        };
                        MenuOption::new(at.to_string(), label)
                            .hint(format!("rule {} of {count}", at + 1))
                    })
                    .collect();
                vec![MenuGroup::new(None, options)]
            }
            Menu::Category | Menu::SplitLine(_) => {
                let chosen = self
                    .chosen_key(menu, cx)
                    .and_then(|key| CategoryKey::new(&key).ok());
                category_groups(data, chosen.as_ref())
            }
            Menu::CategoryGroup => {
                let mut options = vec![MenuOption::new(NEW_GROUP, "New group").icon(Glyph::Plus)];
                options.extend(data.category_overview.iter().map(|group| {
                    let category = &group.group.category;
                    let option =
                        MenuOption::new(category.key().as_str().to_owned(), category.label())
                            .marker(super::table::marker(data, category.key()));
                    if category.is_hidden() {
                        option.hint("Hidden")
                    } else {
                        option
                    }
                }));
                vec![MenuGroup::new(None, options)]
            }
            Menu::Exclude => vec![MenuGroup::new(
                None,
                text::REASONS
                    .iter()
                    .map(|reason| MenuOption::new(reason.as_str(), text::reason(*reason)))
                    .collect(),
            )],
            Menu::NameTest => vec![MenuGroup::new(
                None,
                text::ALIAS_KINDS
                    .iter()
                    .map(|kind| MenuOption::new(kind.as_str(), text::alias_test(*kind)))
                    .collect(),
            )],
            Menu::AddAction => {
                let option = |kind: ActionKind| {
                    let option = MenuOption::new(kind.as_str(), text::menu_label(kind))
                        .icon(text::action_glyph(kind));
                    match rules.form().and_then(|form| form.availability(kind)) {
                        None => option,
                        Some(Unavailable::InUse) => option.hint("In use").disabled(true),
                        Some(Unavailable::NotWith(other)) => option
                            .hint(format!("Not with {}", text::action_label(other)))
                            .disabled(true),
                    }
                };
                vec![
                    MenuGroup::new(
                        None,
                        text::MENU_ACTIONS.iter().map(|k| option(*k)).collect(),
                    ),
                    MenuGroup::new(None, vec![option(ActionKind::Exclude)]),
                ]
            }
            Menu::Tags => {
                let chosen: Vec<String> = match rules
                    .form()
                    .and_then(|form| form.action(ActionKind::AddTag))
                {
                    Some(ActionForm::Tags(tags)) => tags.clone(),
                    _ => Vec::new(),
                };
                let options = data
                    .tags
                    .iter()
                    .map(|tag| {
                        let option = MenuOption::new(
                            tag_key(&tag.name),
                            text::plain(&tag.name, text::NAME_SHOWN),
                        )
                        .checked(
                            chosen
                                .iter()
                                .any(|name| name.eq_ignore_ascii_case(&tag.name)),
                        );
                        if tag.private {
                            option.icon(Glyph::Lock)
                        } else {
                            option
                        }
                    })
                    .collect();
                vec![MenuGroup::new(None, options)]
            }
        }
    }

    /// The open menu's options as shown.
    pub(super) fn visible_groups(&self, cx: &App) -> Vec<MenuGroup> {
        let Some(menu) = self.menu else {
            return Vec::new();
        };
        let groups = self.menu_groups(menu, cx);
        if !menu.has_filter() {
            return groups;
        }
        let query = self.menu_filter.read(cx).value().to_string();
        let mut groups = filter_groups(&groups, &query);
        if menu == Menu::Tags {
            let typed = query.trim();
            let known = self
                .rules
                .read(cx)
                .data()
                .tags
                .iter()
                .any(|tag| tag.name.eq_ignore_ascii_case(typed));
            if !typed.is_empty() && !known {
                let option = match coffer_core::tag::TagName::new(typed) {
                    Ok(name) => MenuOption::new(NEW_TAG, format!("Create “{}”", name.as_str()))
                        .icon(Glyph::Plus),
                    Err(_) => MenuOption::new(NEW_TAG, "Create a tag")
                        .icon(Glyph::Plus)
                        .hint("Not a name a tag can have")
                        .disabled(true),
                };
                groups.push(MenuGroup::new(None, vec![option]));
            }
        }
        groups
    }

    /// The key of the option the menu ticks.
    pub(super) fn chosen_key(&self, menu: Menu, cx: &App) -> Option<SharedString> {
        let rules = self.rules.read(cx);
        if menu == Menu::NameTest {
            return rules.name_form().map(|form| form.kind.as_str().into());
        }
        if menu == Menu::Position {
            return Some(rules.new_position().to_string().into());
        }
        if menu == Menu::CategoryGroup {
            return rules.category_form().map(|form| match &form.group {
                Some(group) => group.as_str().to_owned().into(),
                None => NEW_GROUP.into(),
            });
        }
        let form = rules.form()?;
        Some(match menu {
            Menu::Field => form.field.as_str().into(),
            Menu::Test => form.kind.as_str().into(),
            Menu::Account => form
                .account
                .map_or(ANY.into(), |account| account.to_string().into()),
            Menu::Amount => amount_key(form.amount.choice).into(),
            Menu::Category => match form.action(ActionKind::SetCategory)? {
                ActionForm::Category(Some(key)) => key.as_str().to_owned().into(),
                _ => return None,
            },
            Menu::Exclude => match form.action(ActionKind::Exclude)? {
                ActionForm::Exclude(Some(reason)) => reason.as_str().into(),
                _ => return None,
            },
            Menu::SplitLine(line) => match form.action(ActionKind::SplitTemplate)? {
                ActionForm::Split(lines) => lines
                    .get(line)?
                    .category
                    .as_ref()?
                    .as_str()
                    .to_owned()
                    .into(),
                _ => return None,
            },
            _ => return None,
        })
    }

    pub(super) fn open(&mut self, menu: Menu, window: &mut Window, cx: &mut Context<Self>) {
        if self.menu == Some(menu) {
            self.close(window, cx);
            return;
        }
        if self.menu.is_some() {
            self.close(window, cx);
        }
        self.menu = Some(menu);
        let placeholder = if menu == Menu::Tags {
            "Find or create a tag"
        } else {
            "Find a category"
        };
        self.menu_filter.update(cx, |filter, cx| {
            filter.set_placeholder(placeholder, window, cx);
            filter.set_value("", window, cx);
        });
        let chosen = self.chosen_key(menu, cx);
        if let Some(state) = self.state_mut(menu) {
            state.set_open(true, chosen.as_ref(), window, cx);
        }
        if menu.has_filter() {
            self.menu_filter
                .update(cx, |filter, cx| filter.focus(window, cx));
        }
        cx.notify();
    }

    pub(super) fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(menu) = self.menu.take() else {
            return;
        };
        if menu.is_popup() {
            self.popup.dismiss();
            let back = match menu {
                Menu::Tags => &self.tag_focus,
                _ => &self.add_action_focus,
            };
            back.focus(window, cx);
        } else if let Some(state) = self.state_mut(menu) {
            state.set_open(false, None, window, cx);
        }
        cx.notify();
    }

    pub(super) fn move_highlight(&mut self, step: isize, cx: &mut Context<Self>) {
        let groups = self.visible_groups(cx);
        if let Some(menu) = self.menu
            && let Some(state) = self.state_mut(menu)
        {
            state.move_highlight(&groups, step);
        }
        cx.notify();
    }

    pub(super) fn choose(
        &mut self,
        key: &SharedString,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(menu) = self.menu else {
            return;
        };
        let typed = self.menu_filter.read(cx).value().trim().to_owned();
        self.close(window, cx);
        let key = key.to_string();
        self.rules.update(cx, |rules, cx| match menu {
            Menu::Field => {
                if let Ok(field) = key.parse::<MatchField>() {
                    rules.edit(true, |form| form.field = field, cx);
                }
            }
            Menu::Test => {
                if let Ok(kind) = key.parse::<MatchKind>() {
                    rules.edit(true, |form| form.kind = kind, cx);
                }
            }
            Menu::Account => {
                let account = key.parse::<i64>().ok().map(AccountId::new);
                rules.edit(true, |form| form.account = account, cx);
            }
            Menu::Amount => {
                if let Some(choice) = amount_choice(&key) {
                    rules.edit(true, |form| form.amount.choice = choice, cx);
                }
            }
            Menu::Position => {
                if let Ok(position) = key.parse::<usize>() {
                    rules.set_new_position(position, cx);
                }
            }
            Menu::Category => {
                if let Ok(category) = CategoryKey::new(&key) {
                    rules.edit(
                        true,
                        |form| {
                            if let Some(ActionForm::Category(value)) =
                                form.action_mut(ActionKind::SetCategory)
                            {
                                *value = Some(category);
                            }
                        },
                        cx,
                    );
                }
            }
            Menu::SplitLine(line) => {
                if let Ok(category) = CategoryKey::new(&key) {
                    rules.edit(
                        true,
                        |form| {
                            if let Some(ActionForm::Split(lines)) =
                                form.action_mut(ActionKind::SplitTemplate)
                                && let Some(line) = lines.get_mut(line)
                            {
                                line.category = Some(category);
                            }
                        },
                        cx,
                    );
                }
            }
            Menu::Exclude => {
                if let Ok(reason) = key.parse::<ExcludeReason>() {
                    rules.edit(
                        true,
                        |form| {
                            if let Some(ActionForm::Exclude(value)) =
                                form.action_mut(ActionKind::Exclude)
                            {
                                *value = Some(reason);
                            }
                        },
                        cx,
                    );
                }
            }
            Menu::NameTest => {
                if let Ok(kind) = key.parse::<AliasKind>() {
                    rules.edit_name(true, |form| form.kind = kind, cx);
                }
            }
            Menu::CategoryGroup => {
                let group = CategoryKey::new(&key).ok();
                let kind = group
                    .as_ref()
                    .and_then(|group| rules.data().category_group(group))
                    .map(|group| group.group.category.kind());
                rules.edit_category(
                    false,
                    |form| {
                        form.group = group;
                        if let Some(kind) = kind {
                            form.kind = kind;
                        }
                    },
                    cx,
                );
            }
            Menu::AddAction => {
                if let Ok(kind) = key.parse::<ActionKind>() {
                    rules.edit(
                        true,
                        |form| {
                            form.add_action(kind);
                        },
                        cx,
                    );
                }
            }
            Menu::Tags => {
                let name = if key == NEW_TAG {
                    typed
                } else {
                    key.strip_prefix("tag:").unwrap_or(&key).to_owned()
                };
                if name.is_empty() {
                    return;
                }
                rules.edit(
                    true,
                    |form| {
                        if let Some(ActionForm::Tags(tags)) = form.action_mut(ActionKind::AddTag) {
                            match tags.iter().position(|tag| tag.eq_ignore_ascii_case(&name)) {
                                Some(at) => drop(tags.remove(at)),
                                None => tags.push(name),
                            }
                        }
                    },
                    cx,
                );
            }
        });
    }

    /// A popup menu's list (its own keys), for "+ Add action" and "+ Tag".
    pub(super) fn popup_list(&self, menu: Menu, cx: &mut Context<Self>) -> MenuList {
        let groups = self.visible_groups(cx);
        let label = match menu {
            Menu::Tags => "Tags",
            _ => "Add action",
        };
        let (moved, chosen, dismissed) = (
            cx.entity().downgrade(),
            cx.entity().downgrade(),
            cx.entity().downgrade(),
        );
        let mut list = MenuList::new(label, groups, None, &self.popup)
            .own_keys()
            .width(size::SEARCH_FIELD)
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
            list = list
                .filter(&self.menu_filter)
                .max_height(size::CONTROL * MENU_ROWS);
        }
        list
    }

    /// A button that opens a popup menu on press (the Transactions view's
    /// chips work the same way).
    pub(super) fn popup_trigger(
        &self,
        menu: Menu,
        trigger: impl IntoElement,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Div {
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
                        window.prevent_default();
                    }
                }),
            )
            .child(trigger)
    }

    /// What a keyboard activation of a popup's trigger does.
    pub(super) fn keyboard_open(
        menu: Menu,
    ) -> impl Fn(&mut Self, &ClickEvent, &mut Window, &mut Context<Self>) + 'static {
        move |this, event, window, cx| {
            if matches!(event, ClickEvent::Keyboard(_)) {
                this.open(menu, window, cx);
            }
        }
    }

    /// Opens `menu` as its control would (snapshots).
    pub fn show_menu(&mut self, menu: Menu, window: &mut Window, cx: &mut Context<Self>) {
        if self.menu != Some(menu) {
            self.open(menu, window, cx);
        }
    }

    /// Types `text` into `input` as the user would (snapshots, tests).
    pub fn type_into(
        &mut self,
        input: Input,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = self.inputs.get(&input).cloned() else {
            return;
        };
        state.update(cx, |state, cx| state.set_value(text.to_owned(), window, cx));
        self.typed(input, text.to_owned(), false, cx);
    }

    /// Removes the action of `kind`, as its ✕ does.
    pub(super) fn remove_action(&mut self, kind: ActionKind, cx: &mut Context<Self>) {
        self.rules.update(cx, |rules, cx| {
            rules.edit(true, |form| form.remove_action(kind), cx)
        });
    }

    /// Sets a new group's kind.
    pub(super) fn set_category_kind(&mut self, kind: CategoryKind, cx: &mut Context<Self>) {
        self.rules.update(cx, |rules, cx| {
            rules.edit_category(false, |form| form.kind = kind, cx)
        });
    }

    /// "Delete category…": the dialog over the window. Composed: the view
    /// opens it itself (`gpui-base` draws a dialog over the whole window
    /// from wherever it's rendered) and puts focus back on the canvas when
    /// it closes.
    pub fn request_delete_category(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self
            .rules
            .read(cx)
            .selected_category_entry()
            .map(|(summary, _)| summary.category.id())
        else {
            return;
        };
        let rules = self.rules.clone();
        let dialog = cx.new(|cx| DeleteCategoryDialog::new(rules, id, window, cx));
        let subscription =
            cx.subscribe_in(&dialog, window, |this, _, _: &DialogClosed, window, cx| {
                this.delete_category = None;
                this.focus.focus(window, cx);
                cx.notify();
            });
        self.delete_category = Some((dialog, subscription));
        cx.notify();
    }

    /// The Delete category dialog, while it's open.
    pub fn delete_category_dialog(&self) -> Option<&Entity<DeleteCategoryDialog>> {
        self.delete_category.as_ref().map(|(dialog, _)| dialog)
    }

    /// Sets the amount condition's direction.
    pub(super) fn set_direction(&mut self, direction: Direction, cx: &mut Context<Self>) {
        self.rules.update(cx, |rules, cx| {
            rules.edit(true, |form| form.amount.direction = direction, cx)
        });
    }

    /// Adds a split line ("+ Add line").
    pub(super) fn add_split_line(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.rules.update(cx, |rules, cx| {
            rules.edit(
                false,
                |form| {
                    if let Some(ActionForm::Split(lines)) =
                        form.action_mut(ActionKind::SplitTemplate)
                    {
                        lines.push(SplitLineForm::default());
                    }
                },
                cx,
            )
        });
        self.sync(window, cx);
    }

    pub(super) fn remove_split_line(
        &mut self,
        line: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.rules.update(cx, |rules, cx| {
            rules.edit(
                true,
                |form| {
                    if let Some(ActionForm::Split(lines)) =
                        form.action_mut(ActionKind::SplitTemplate)
                        && line < lines.len()
                        && lines.len() > 2
                    {
                        lines.remove(line);
                    }
                },
                cx,
            )
        });
        // The fields below the removed line shift up: fill every field again.
        self.filled_for = None;
        self.sync(window, cx);
    }

    fn render_bar(&self, rules: &Rules, window: &Window) -> Option<AnyElement> {
        let (bar, undoable) = match rules.outcome()? {
            Outcome::Done { message, undo } => (UndoBar::new(message.clone()), undo.is_some()),
            Outcome::Moved(_) => (
                UndoBar::rich(text::rich(
                    window,
                    typography::BODY,
                    color::TEXT,
                    rules.moved_parts()?,
                )),
                true,
            ),
            Outcome::Refused(_) => return None,
        };
        let (undo_on, dismiss_on) = (self.rules.clone(), self.rules.clone());
        let mut bar = bar.on_dismiss(move |_, _, cx| {
            dismiss_on.update(cx, |rules, cx| rules.dismiss_outcome(cx))
        });
        if undoable {
            bar = bar.on_undo(move |_, _, cx| undo_on.update(cx, |rules, cx| rules.undo(cx)));
        }
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
}

/// The category menu: each group under its caps label, its own category
/// first, with its marker (the Transactions view's menu). Hidden categories
/// are left out, except `chosen`, which the field already shows.
pub fn category_groups(data: &RulesData, chosen: Option<&CategoryKey>) -> Vec<MenuGroup> {
    let mut groups: Vec<MenuGroup> = Vec::new();
    // Whether the last group pushed is the current category's group.
    let mut in_group = false;
    for category in &data.categories {
        let offered = !category.hidden || chosen == Some(&category.key);
        let option = MenuOption::new(category.key.as_str().to_owned(), category.label.clone())
            .marker(super::table::marker(data, &category.key));
        match &category.group {
            None => {
                in_group = offered;
                if offered {
                    groups.push(MenuGroup::named(category.label.clone(), vec![option]));
                }
            }
            Some(_) if !offered => {}
            Some(label) => match groups.last_mut() {
                Some(group) if in_group => group.options.push(option),
                // A chosen subcategory of a hidden group: under its group's
                // label, without the group itself.
                _ => {
                    in_group = true;
                    groups.push(MenuGroup::named(label.clone(), vec![option]));
                }
            },
        }
    }
    groups
}

fn tag_key(name: &str) -> String {
    format!("tag:{name}")
}

pub fn amount_label(choice: AmountChoice) -> &'static str {
    match choice {
        AmountChoice::Any => "Any amount",
        AmountChoice::Over => "Over",
        AmountChoice::Under => "Under",
        AmountChoice::Between => "Between",
        AmountChoice::Custom => "Custom",
    }
}

fn amount_key(choice: AmountChoice) -> &'static str {
    match choice {
        AmountChoice::Any => "any",
        AmountChoice::Over => "over",
        AmountChoice::Under => "under",
        AmountChoice::Between => "between",
        AmountChoice::Custom => "custom",
    }
}

fn amount_choice(key: &str) -> Option<AmountChoice> {
    [
        AmountChoice::Any,
        AmountChoice::Over,
        AmountChoice::Under,
        AmountChoice::Between,
    ]
    .into_iter()
    .find(|choice| amount_key(*choice) == key)
}

/// A refusal under the toolbar: caption type, `danger`.
fn refused(message: SharedString) -> impl IntoElement {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(spacing::S1_5)
        .pt(spacing::S2)
        .px(spacing::S4)
        .child(Glyph::Alert.render().text_color(color::DANGER))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .text_role(typography::CAPTION)
                .text_color(color::DANGER)
                .child(message),
        )
}

impl Render for RulesView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let root = div()
            .key_context(CONTEXT)
            .track_focus(&self.focus)
            .on_list_move(
                KEYS.list(),
                cx.listener(|this, to: &Move, _, cx| {
                    this.rules
                        .update(cx, |rules, cx| rules.select_next(*to, cx))
                }),
            )
            .on_action(cx.listener(|this, _: &Cancel, _, cx| {
                this.rules.update(cx, |rules, cx| rules.cancel(cx))
            }))
            .on_action(
                cx.listener(|this, _: &MoveRuleUp, window, cx| this.reorder(false, window, cx)),
            )
            .on_action(
                cx.listener(|this, _: &MoveRuleDown, window, cx| this.reorder(true, window, cx)),
            )
            .flex()
            .flex_1()
            .min_w_0()
            .min_h_0();
        let entity = self.rules.clone();
        let (refusal, bar) = {
            let rules = entity.read(cx);
            let refusal = match rules.outcome() {
                Some(Outcome::Refused(message)) => Some(message.clone()),
                _ => None,
            };
            (refusal, self.render_bar(rules, window))
        };
        // The selected row takes the inset focus ring while the table has
        // keyboard focus, so it shows where ↑↓ and ⌥↑⌥↓ act.
        let ring = self.focus.is_focused(window) && window.last_input_was_keyboard();
        let toolbar = super::table::toolbar(&entity, window, cx);
        let canvas_body = super::table::render(&entity, ring, window, cx);
        let inspector = super::inspector::render(self, &entity, window, cx);
        let canvas = div()
            .relative()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .overflow_hidden()
            .bg(color::CANVAS)
            .child(toolbar)
            .children(refusal.map(refused))
            .child(canvas_body)
            .children(bar);
        let dialog = self
            .delete_category
            .as_ref()
            .map(|(dialog, _)| dialog.clone());
        root.child(canvas).child(inspector).children(dialog)
    }
}
