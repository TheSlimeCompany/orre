//! The rule editor's model (`docs/DESIGN.md` → "Rules", the rule
//! inspector): what the When and Then fields hold while the user edits, and
//! the [`RuleDefinition`] they make once every field is valid.
//!
//! Pure data, so it is tested without a window. The view keeps text fields
//! in step with it; core validates what comes out again when it's stored.
//!
//! Composed, not drawn (user, 2026-11-14; `DESIGN.md` → "Rules" → "Built in
//! M3.13"):
//! - **Amount**: Any amount, Over, Under or Between, then Out or In and the
//!   amount(s) typed without a sign, as the new-transaction dialog's Out/In
//!   (D6.3). Bounds are inclusive and signed in core: Over £1,000 out is
//!   `amount ≤ −100000`, Under £5 out is `−500 ≤ amount ≤ 0`, Between £5 and
//!   £10 out is `−1000 ≤ amount ≤ −500`; in is the same above zero. Bounds
//!   no choice can say (an agent's rule across zero) read as "Custom" and
//!   stay as they are until another choice replaces them.
//! - **Split**: D6.3's lines without amounts, each a category and a share
//!   in percent (two decimals at most), adding up to 100%.
//! - **Exclude**: D6.3's reason select without Other (a rule carries no
//!   note).

use coffer_core::category::CategoryKey;
use coffer_core::id::AccountId;
use coffer_core::merchant::{AliasDefinition, AliasError, AliasKind, DisplayName};
use coffer_core::money::Currency;
use coffer_core::rule::{
    ActionKind, ActionText, AmountBounds, ExcludeReason, MatchField, MatchKind, Pattern,
    PatternError, RuleAction, RuleDefinition, RuleError, Share, SplitLine, SplitTemplate, TagList,
};
use coffer_core::tag::TagName;

use crate::format;

/// What the Amount select says.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum AmountChoice {
    #[default]
    Any,
    Over,
    Under,
    Between,
    /// Stored bounds the other choices can't say (module docs).
    Custom,
}

/// Money out or money in: the sign is never typed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Direction {
    #[default]
    Out,
    In,
}

/// The Amount condition as the inspector holds it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AmountForm {
    pub choice: AmountChoice,
    pub direction: Direction,
    /// Over's or Under's amount, Between's lower one; unsigned text.
    pub low: String,
    /// Between's higher amount.
    pub high: String,
    /// The stored bounds, kept while the choice is Custom.
    pub custom: AmountBounds,
}

/// One split line of a Split action: a category and a share in percent.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SplitLineForm {
    pub category: Option<CategoryKey>,
    pub percent: String,
}

/// One action as the inspector holds it. A new action starts empty, and
/// the rule can't be saved until it's filled in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActionForm {
    Category(Option<CategoryKey>),
    Merchant(String),
    Tags(Vec<String>),
    Note(String),
    Exclude(Option<ExcludeReason>),
    Split(Vec<SplitLineForm>),
    /// My share, in percent.
    Share(String),
}

impl ActionForm {
    pub fn kind(&self) -> ActionKind {
        match self {
            ActionForm::Category(_) => ActionKind::SetCategory,
            ActionForm::Merchant(_) => ActionKind::SetMerchant,
            ActionForm::Tags(_) => ActionKind::AddTag,
            ActionForm::Note(_) => ActionKind::SetNote,
            ActionForm::Exclude(_) => ActionKind::Exclude,
            ActionForm::Split(_) => ActionKind::SplitTemplate,
            ActionForm::Share(_) => ActionKind::MyShare,
        }
    }

    /// A new, empty action of `kind`. A split starts with two lines.
    pub fn empty(kind: ActionKind) -> ActionForm {
        match kind {
            ActionKind::SetCategory => ActionForm::Category(None),
            ActionKind::SetMerchant => ActionForm::Merchant(String::new()),
            ActionKind::AddTag => ActionForm::Tags(Vec::new()),
            ActionKind::SetNote => ActionForm::Note(String::new()),
            ActionKind::Exclude => ActionForm::Exclude(None),
            ActionKind::SplitTemplate => {
                ActionForm::Split(vec![SplitLineForm::default(), SplitLineForm::default()])
            }
            ActionKind::MyShare => ActionForm::Share(String::new()),
        }
    }

    fn of(action: &RuleAction) -> ActionForm {
        match action {
            RuleAction::SetCategory(key) => ActionForm::Category(Some(key.clone())),
            RuleAction::SetMerchant(text) => ActionForm::Merchant(text.as_str().to_owned()),
            RuleAction::AddTag(tags) => ActionForm::Tags(
                tags.tags()
                    .iter()
                    .map(|tag| tag.as_str().to_owned())
                    .collect(),
            ),
            RuleAction::SetNote(text) => ActionForm::Note(text.as_str().to_owned()),
            RuleAction::Exclude(reason) => ActionForm::Exclude(Some(*reason)),
            RuleAction::SplitTemplate(template) => ActionForm::Split(
                template
                    .lines()
                    .iter()
                    .map(|line| SplitLineForm {
                        category: Some(line.category().clone()),
                        percent: percent_text(line.share()),
                    })
                    .collect(),
            ),
            RuleAction::MyShare(share) => ActionForm::Share(percent_text(*share)),
        }
    }
}

/// Why a kind can't be added now: it's in the rule, or it can't go with
/// one that is (Category and Split exclude each other).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unavailable {
    InUse,
    NotWith(ActionKind),
}

/// The rule's fields as the inspector holds them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuleForm {
    pub field: MatchField,
    pub kind: MatchKind,
    pub text: String,
    pub account: Option<AccountId>,
    pub amount: AmountForm,
    /// One per kind, in [`ActionKind`] order.
    pub actions: Vec<ActionForm>,
}

impl Default for RuleForm {
    /// New rule from scratch: Merchant contains, nothing typed, no actions.
    fn default() -> Self {
        RuleForm {
            field: MatchField::Merchant,
            kind: MatchKind::Contains,
            text: String::new(),
            account: None,
            amount: AmountForm::default(),
            actions: Vec::new(),
        }
    }
}

/// What's wrong with the form, and so why the rule can't be saved yet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FormError {
    /// Nothing typed to match.
    NoText,
    Pattern(PatternError),
    /// An amount that doesn't read, or Between the wrong way round.
    Amount(String),
    NoActions,
    /// An action not filled in, or a value it can't take.
    Action {
        kind: ActionKind,
        message: String,
    },
}

impl FormError {
    /// Whether it belongs under the When section's Text field.
    pub fn is_pattern(&self) -> bool {
        matches!(self, FormError::NoText | FormError::Pattern(_))
    }
}

impl RuleForm {
    /// The form for a stored rule.
    pub fn of(rule: &RuleDefinition) -> RuleForm {
        RuleForm {
            field: rule.field(),
            kind: rule.pattern().kind(),
            text: rule.pattern().as_str().to_owned(),
            account: rule.account_id().map(AccountId::new),
            amount: AmountForm::of(rule.amount()),
            actions: rule.actions().iter().map(ActionForm::of).collect(),
        }
    }

    /// Whether `kind` can be added, and if not, why.
    pub fn availability(&self, kind: ActionKind) -> Option<Unavailable> {
        let has = |kind| self.actions.iter().any(|action| action.kind() == kind);
        if has(kind) {
            return Some(Unavailable::InUse);
        }
        match kind {
            ActionKind::SetCategory if has(ActionKind::SplitTemplate) => {
                Some(Unavailable::NotWith(ActionKind::SplitTemplate))
            }
            ActionKind::SplitTemplate if has(ActionKind::SetCategory) => {
                Some(Unavailable::NotWith(ActionKind::SetCategory))
            }
            _ => None,
        }
    }

    /// Adds an empty action of `kind` in its place, if it can be added.
    pub fn add_action(&mut self, kind: ActionKind) -> bool {
        if self.availability(kind).is_some() {
            return false;
        }
        self.actions.push(ActionForm::empty(kind));
        self.actions.sort_by_key(ActionForm::kind);
        true
    }

    pub fn remove_action(&mut self, kind: ActionKind) {
        self.actions.retain(|action| action.kind() != kind);
    }

    pub fn action_mut(&mut self, kind: ActionKind) -> Option<&mut ActionForm> {
        self.actions.iter_mut().find(|action| action.kind() == kind)
    }

    pub fn action(&self, kind: ActionKind) -> Option<&ActionForm> {
        self.actions.iter().find(|action| action.kind() == kind)
    }

    /// The rule the form makes, with amounts read in `currency` (the
    /// rule's account's, or the home currency for any account).
    pub fn definition(&self, currency: Currency) -> Result<RuleDefinition, FormError> {
        if self.text.is_empty() {
            return Err(FormError::NoText);
        }
        let pattern = Pattern::new(self.kind, &self.text).map_err(FormError::Pattern)?;
        let amount = self.amount.bounds(currency)?;
        if self.actions.is_empty() {
            return Err(FormError::NoActions);
        }
        let actions = self
            .actions
            .iter()
            .map(action)
            .collect::<Result<Vec<_>, _>>()?;
        RuleDefinition::new(
            self.field,
            pattern,
            amount,
            self.account.map(|account| account.get()),
            actions,
        )
        .map_err(|error| match error {
            RuleError::NoActions => FormError::NoActions,
            RuleError::DuplicateAction(kind) => FormError::Action {
                kind,
                message: "Each kind of action once per rule.".to_owned(),
            },
            RuleError::CategoryWithSplit => FormError::Action {
                kind: ActionKind::SplitTemplate,
                message: "A rule sets either Category or Split, not both.".to_owned(),
            },
        })
    }
}

fn action_error(kind: ActionKind, message: &str) -> FormError {
    FormError::Action {
        kind,
        message: message.to_owned(),
    }
}

fn action(form: &ActionForm) -> Result<RuleAction, FormError> {
    let kind = form.kind();
    Ok(match form {
        ActionForm::Category(Some(key)) => RuleAction::SetCategory(key.clone()),
        ActionForm::Category(None) => return Err(action_error(kind, "Choose a category.")),
        ActionForm::Merchant(text) => RuleAction::SetMerchant(
            ActionText::new(text.trim())
                .map_err(|_| action_error(kind, "Type the name to show."))?,
        ),
        ActionForm::Note(text) => RuleAction::SetNote(
            ActionText::new(text.trim()).map_err(|_| action_error(kind, "Type the note."))?,
        ),
        ActionForm::Tags(names) => {
            let tags = names
                .iter()
                .map(|name| TagName::new(name))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|_| action_error(kind, "A tag name can't be used."))?;
            RuleAction::AddTag(
                TagList::new(tags).map_err(|_| action_error(kind, "Add at least one tag."))?,
            )
        }
        ActionForm::Exclude(Some(reason)) => RuleAction::exclude(*reason)
            .map_err(|_| action_error(kind, "Choose a reason other than Other."))?,
        ActionForm::Exclude(None) => return Err(action_error(kind, "Choose a reason.")),
        ActionForm::Share(text) => {
            let basis_points = parse_percent(text)
                .ok_or_else(|| action_error(kind, "Type your share from 0 to 100%."))?;
            RuleAction::MyShare(
                Share::new(basis_points)
                    .map_err(|_| action_error(kind, "Type your share from 0 to 100%."))?,
            )
        }
        ActionForm::Split(lines) => {
            let mut template = Vec::with_capacity(lines.len());
            for (index, line) in lines.iter().enumerate() {
                let number = index + 1;
                let category = line.category.clone().ok_or_else(|| {
                    action_error(kind, &format!("Choose line {number}’s category."))
                })?;
                let share = parse_percent(&line.percent)
                    .filter(|bp| *bp > 0)
                    .and_then(|bp| Share::new(bp).ok())
                    .ok_or_else(|| {
                        action_error(kind, &format!("Give line {number} a share above 0%."))
                    })?;
                template.push(SplitLine::new(category, share));
            }
            let total: u64 = template
                .iter()
                .map(|line| u64::from(line.share().basis_points()))
                .sum();
            if template.len() < 2 {
                return Err(action_error(kind, "A split needs at least two lines."));
            }
            if total != 10_000 {
                return Err(action_error(
                    kind,
                    &format!("The lines add up to {}, not 100%.", percent_words(total)),
                ));
            }
            RuleAction::SplitTemplate(
                SplitTemplate::new(template)
                    .map_err(|_| action_error(kind, "Each line needs a different category."))?,
            )
        }
    })
}

/// `total` basis points as "75%" or "62.5%".
fn percent_words(total: u64) -> String {
    let whole = total / 100;
    let rest = total % 100;
    if rest == 0 {
        format!("{whole}%")
    } else if rest.is_multiple_of(10) {
        format!("{whole}.{}%", rest / 10)
    } else {
        format!("{whole}.{rest:02}%")
    }
}

/// A share as the field shows it: "50", "33.33", "12.5".
pub fn percent_text(share: Share) -> String {
    percent_words(u64::from(share.basis_points()))
        .trim_end_matches('%')
        .to_owned()
}

/// Reads a percentage, "50", "33.33" or "12.5%", as basis points. At
/// most two decimals; `None` for anything else.
pub fn parse_percent(text: &str) -> Option<u64> {
    let text = text.trim().trim_end_matches('%').trim();
    if text.is_empty() {
        return None;
    }
    let (whole, fraction) = match text.split_once('.') {
        Some((whole, fraction)) => (whole, fraction),
        None => (text, ""),
    };
    if fraction.len() > 2
        || !whole.chars().all(|c| c.is_ascii_digit())
        || !fraction.chars().all(|c| c.is_ascii_digit())
        || (whole.is_empty() && fraction.is_empty())
    {
        return None;
    }
    let whole: u64 = if whole.is_empty() {
        0
    } else {
        whole.parse().ok()?
    };
    let fraction: u64 = format!("{fraction:0<2}").parse().ok()?;
    whole.checked_mul(100)?.checked_add(fraction)
}

impl AmountForm {
    /// The form for stored bounds (module docs).
    pub fn of(bounds: AmountBounds) -> AmountForm {
        let unsigned = |minor: i64| minor.unsigned_abs();
        let (choice, direction, low, high) = match (bounds.min(), bounds.max()) {
            (None, None) => (AmountChoice::Any, Direction::Out, None, None),
            (None, Some(max)) if max < 0 => (
                AmountChoice::Over,
                Direction::Out,
                Some(unsigned(max)),
                None,
            ),
            (Some(min), None) if min > 0 => {
                (AmountChoice::Over, Direction::In, Some(unsigned(min)), None)
            }
            (Some(min), Some(0)) if min < 0 => (
                AmountChoice::Under,
                Direction::Out,
                Some(unsigned(min)),
                None,
            ),
            (Some(0), Some(max)) if max > 0 => (
                AmountChoice::Under,
                Direction::In,
                Some(unsigned(max)),
                None,
            ),
            (Some(min), Some(max)) if max < 0 => (
                AmountChoice::Between,
                Direction::Out,
                Some(unsigned(max)),
                Some(unsigned(min)),
            ),
            (Some(min), Some(max)) if min > 0 => (
                AmountChoice::Between,
                Direction::In,
                Some(unsigned(min)),
                Some(unsigned(max)),
            ),
            _ => (AmountChoice::Custom, Direction::Out, None, None),
        };
        // Minor units as the field shows them: two decimals, the home
        // currency's (a rule's amounts are in its account's units).
        let text = |minor: Option<u64>| {
            minor.map_or(String::new(), |minor| {
                if minor.is_multiple_of(100) {
                    (minor / 100).to_string()
                } else {
                    format!("{}.{:02}", minor / 100, minor % 100)
                }
            })
        };
        AmountForm {
            choice,
            direction,
            low: text(low),
            high: text(high),
            custom: bounds,
        }
    }

    /// The bounds the form says, in `currency`'s minor units.
    pub fn bounds(&self, currency: Currency) -> Result<AmountBounds, FormError> {
        let read = |text: &str| -> Result<i64, FormError> {
            let amount = format::parse_amount(text, currency)
                .map_err(|_| FormError::Amount("Type the amount without a sign.".to_owned()))?;
            if amount.minor() <= 0 {
                return Err(FormError::Amount(
                    "Type an amount above zero; Out or In gives the sign.".to_owned(),
                ));
            }
            Ok(amount.minor())
        };
        let sign = match self.direction {
            Direction::Out => -1,
            Direction::In => 1,
        };
        let (min, max) = match self.choice {
            AmountChoice::Any => (None, None),
            AmountChoice::Custom => return Ok(self.custom),
            AmountChoice::Over => {
                let value = read(&self.low)?;
                match self.direction {
                    Direction::Out => (None, Some(-value)),
                    Direction::In => (Some(value), None),
                }
            }
            AmountChoice::Under => {
                let value = read(&self.low)?;
                match self.direction {
                    Direction::Out => (Some(-value), Some(0)),
                    Direction::In => (Some(0), Some(value)),
                }
            }
            AmountChoice::Between => {
                let (low, high) = (read(&self.low)?, read(&self.high)?);
                if low > high {
                    return Err(FormError::Amount(
                        "The first amount is above the second.".to_owned(),
                    ));
                }
                let (a, b) = (sign * low, sign * high);
                (Some(a.min(b)), Some(a.max(b)))
            }
        };
        AmountBounds::new(min, max)
            .map_err(|_| FormError::Amount("The first amount is above the second.".to_owned()))
    }
}

/// A merchant name's fields as the inspector holds them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NameForm {
    pub kind: AliasKind,
    pub pattern: String,
    pub name: String,
}

impl Default for NameForm {
    /// New name from scratch: Starts with, nothing typed.
    fn default() -> Self {
        NameForm {
            kind: AliasKind::Prefix,
            pattern: String::new(),
            name: String::new(),
        }
    }
}

/// What's wrong with a name's fields.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NameError {
    /// Nothing typed to match, or a pattern that can't be used.
    Pattern(Option<PatternError>),
    /// Nothing typed to show.
    Name,
}

impl NameForm {
    pub fn of(alias: &AliasDefinition) -> NameForm {
        NameForm {
            kind: alias.kind(),
            pattern: alias.pattern().to_owned(),
            name: alias.name().as_str().to_owned(),
        }
    }

    pub fn definition(&self) -> Result<AliasDefinition, NameError> {
        let name = DisplayName::new(&self.name).map_err(|_| NameError::Name)?;
        AliasDefinition::new(self.kind, &self.pattern, name).map_err(|error| match error {
            AliasError::Pattern(error) => NameError::Pattern(Some(error)),
            AliasError::BlankPattern => NameError::Pattern(None),
            AliasError::BlankName => NameError::Name,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(text: &str) -> CategoryKey {
        CategoryKey::new(text).unwrap()
    }

    fn form(text: &str) -> RuleForm {
        RuleForm {
            text: text.to_owned(),
            actions: vec![ActionForm::Category(Some(key("transport")))],
            ..RuleForm::default()
        }
    }

    #[test]
    fn a_stored_rule_round_trips_through_the_form() {
        let rule = RuleDefinition::new(
            MatchField::Description,
            Pattern::new(MatchKind::Contains, "FLAT 2B RENT").unwrap(),
            AmountBounds::new(None, Some(-100_000)).unwrap(),
            Some(3),
            vec![
                RuleAction::SetCategory(key("bills")),
                RuleAction::MyShare(Share::new(5_000).unwrap()),
                RuleAction::AddTag(TagList::new(vec![TagName::new("Household").unwrap()]).unwrap()),
            ],
        )
        .unwrap();
        let form = RuleForm::of(&rule);
        assert_eq!(form.amount.choice, AmountChoice::Over);
        assert_eq!(form.amount.direction, Direction::Out);
        assert_eq!(form.amount.low, "1000");
        assert_eq!(
            form.action(ActionKind::MyShare),
            Some(&ActionForm::Share("50".into()))
        );
        assert_eq!(form.definition(Currency::GBP), Ok(rule));
    }

    #[test]
    fn amounts_are_typed_unsigned_with_out_or_in() {
        let gbp = Currency::GBP;
        let bounds = |choice, direction, low: &str, high: &str| {
            AmountForm {
                choice,
                direction,
                low: low.into(),
                high: high.into(),
                custom: AmountBounds::ANY,
            }
            .bounds(gbp)
        };
        let b = |min, max| Ok(AmountBounds::new(min, max).unwrap());
        assert_eq!(
            bounds(AmountChoice::Over, Direction::Out, "1,000", ""),
            b(None, Some(-100_000))
        );
        assert_eq!(
            bounds(AmountChoice::Over, Direction::In, "2000", ""),
            b(Some(200_000), None)
        );
        assert_eq!(
            bounds(AmountChoice::Under, Direction::Out, "5", ""),
            b(Some(-500), Some(0))
        );
        assert_eq!(
            bounds(AmountChoice::Under, Direction::In, "5", ""),
            b(Some(0), Some(500))
        );
        assert_eq!(
            bounds(AmountChoice::Between, Direction::Out, "5", "10.50"),
            b(Some(-1_050), Some(-500))
        );
        assert_eq!(
            bounds(AmountChoice::Between, Direction::In, "5", "10"),
            b(Some(500), Some(1_000))
        );
        assert!(bounds(AmountChoice::Between, Direction::In, "10", "5").is_err());
        assert!(bounds(AmountChoice::Over, Direction::Out, "-5", "").is_err());
        assert!(bounds(AmountChoice::Over, Direction::Out, "0", "").is_err());
        assert!(bounds(AmountChoice::Over, Direction::Out, "abc", "").is_err());
        // Every choice reads back as itself.
        for (choice, direction) in [
            (AmountChoice::Over, Direction::Out),
            (AmountChoice::Over, Direction::In),
            (AmountChoice::Under, Direction::Out),
            (AmountChoice::Under, Direction::In),
            (AmountChoice::Between, Direction::Out),
            (AmountChoice::Between, Direction::In),
        ] {
            let stored = bounds(choice, direction, "5", "10").unwrap();
            let back = AmountForm::of(stored);
            assert_eq!((back.choice, back.direction), (choice, direction));
            assert_eq!(back.bounds(gbp), Ok(stored));
        }
        // Across zero: Custom, kept as it is.
        let across = AmountBounds::new(Some(-500), Some(500)).unwrap();
        let form = AmountForm::of(across);
        assert_eq!(form.choice, AmountChoice::Custom);
        assert_eq!(form.bounds(gbp), Ok(across));
    }

    #[test]
    fn percentages_have_two_decimals_at_most() {
        assert_eq!(parse_percent("50"), Some(5_000));
        assert_eq!(parse_percent("33.33"), Some(3_333));
        assert_eq!(parse_percent("12.5%"), Some(1_250));
        assert_eq!(parse_percent(" .5 "), Some(50));
        assert_eq!(parse_percent("100"), Some(10_000));
        assert_eq!(parse_percent("1.234"), None);
        assert_eq!(parse_percent("-5"), None);
        assert_eq!(parse_percent(""), None);
        assert_eq!(parse_percent("."), None);
        assert_eq!(percent_text(Share::new(1_250).unwrap()), "12.5");
        assert_eq!(percent_text(Share::new(3_333).unwrap()), "33.33");
    }

    #[test]
    fn the_form_says_what_stops_it_saving() {
        assert_eq!(form("").definition(Currency::GBP), Err(FormError::NoText));
        let mut regex = form("^(ASOS|ZARA");
        regex.kind = MatchKind::Regex;
        assert!(matches!(
            regex.definition(Currency::GBP),
            Err(FormError::Pattern(PatternError::InvalidRegex { .. }))
        ));
        let mut none = form("UBER");
        none.actions.clear();
        assert_eq!(none.definition(Currency::GBP), Err(FormError::NoActions));
        let mut empty = form("UBER");
        empty.add_action(ActionKind::SetNote);
        assert!(matches!(
            empty.definition(Currency::GBP),
            Err(FormError::Action {
                kind: ActionKind::SetNote,
                ..
            })
        ));
    }

    #[test]
    fn category_and_split_exclude_each_other() {
        let mut form = form("UBER");
        assert_eq!(
            form.availability(ActionKind::SetCategory),
            Some(Unavailable::InUse)
        );
        assert_eq!(
            form.availability(ActionKind::SplitTemplate),
            Some(Unavailable::NotWith(ActionKind::SetCategory))
        );
        assert!(!form.add_action(ActionKind::SplitTemplate));
        assert!(form.add_action(ActionKind::MyShare));
        assert!(form.add_action(ActionKind::SetMerchant));
        let kinds: Vec<_> = form.actions.iter().map(ActionForm::kind).collect();
        assert_eq!(
            kinds,
            [
                ActionKind::SetCategory,
                ActionKind::SetMerchant,
                ActionKind::MyShare
            ]
        );
        form.remove_action(ActionKind::SetCategory);
        assert_eq!(form.availability(ActionKind::SplitTemplate), None);
    }

    #[test]
    fn a_split_adds_up_to_a_hundred() {
        let mut form = form("RENT");
        form.actions = vec![ActionForm::Split(vec![
            SplitLineForm {
                category: Some(key("bills")),
                percent: "60".into(),
            },
            SplitLineForm {
                category: Some(key("food")),
                percent: "30".into(),
            },
        ])];
        let Err(FormError::Action { message, .. }) = form.definition(Currency::GBP) else {
            panic!("a split short of 100% isn't saved");
        };
        assert_eq!(message, "The lines add up to 90%, not 100%.");
        if let Some(ActionForm::Split(lines)) = form.action_mut(ActionKind::SplitTemplate) {
            lines[1].percent = "40".into();
        }
        assert!(form.definition(Currency::GBP).is_ok());
    }
}
