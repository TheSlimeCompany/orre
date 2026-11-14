//! What the Rules view writes (`docs/DESIGN.md` → "Rules"): field and test
//! names, a rule's When and Then as the table and inspector say them, the
//! change history's descriptions, and the faults a pattern can have. The
//! wording the artboards don't show is recorded in `docs/ARCHITECTURE.md` →
//! "Decision record: the Rules view (built)".
//!
//! Patterns, merchant names, tag names, notes and bank text are untrusted
//! (an agent's suggestion may have written them): they go through
//! [`plain`] before they're drawn, and are never acted on.

use chrono::{NaiveDate, NaiveDateTime};
use coffer_core::category::CategoryKey;
use coffer_core::change::{ChangeKind, Field, HistoryEntry, Shown, Summary};
use coffer_core::merchant::AliasKind;
use coffer_core::money::{Currency, Money};
use coffer_core::rule::{
    ActionKind, AmountBounds, ExcludeReason, MatchField, MatchKind, PatternError, RuleAction,
    RuleDefinition,
};

use crate::format::{self, Sign};
use crate::ui::icon::Glyph;

use super::store::RulesData;

pub use crate::inbox::text::{NAME_SHOWN, Part, count, plain, rich};

/// The longest pattern shown in a sentence or a row.
pub const PATTERN_SHOWN: usize = 80;

/// A date in a list or a sentence: `9 Nov`, or `2 Oct 25` in another year.
pub fn date(date: NaiveDate, today: NaiveDate) -> String {
    format::list_date(date, today)
}

/// A change's time in the history: `9 Nov 14:05`.
pub fn when(at: NaiveDateTime, today: NaiveDate) -> String {
    format!("{} {}", date(at.date(), today), at.format("%H:%M"))
}

/// `9 Nov at 14:05`.
pub fn when_at(at: NaiveDateTime, today: NaiveDate) -> String {
    format!("{} at {}", date(at.date(), today), at.format("%H:%M"))
}

/// `text` as a sentence: a capital first letter and a full stop (core's
/// errors are lower case and unpunctuated).
pub fn sentence(text: &str) -> String {
    let mut chars = text.trim().chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    let mut out: String = first.to_uppercase().chain(chars).collect();
    if !out.ends_with(['.', '!', '?']) {
        out.push('.');
    }
    out
}

/// `parts` as [`rich`] draws them, struck through in `color`: an undone
/// change in the history (Paper `H9N-0`).
pub fn struck(
    window: &gpui_kit::Window,
    role: crate::theme::tokens::typography::TextRole,
    color: gpui_kit::Rgba,
    parts: Vec<Part>,
) -> gpui_kit::StyledText {
    let base = window.text_style().font();
    let mut text = String::new();
    let mut runs = Vec::with_capacity(parts.len());
    for part in parts.into_iter().filter(|part| !part.text.is_empty()) {
        let font = gpui_kit::Font {
            family: if part.mono {
                crate::theme::tokens::font::MONO.into()
            } else {
                role.family.into()
            },
            weight: part.weight.unwrap_or(role.weight),
            ..base.clone()
        };
        let tone = part.color.unwrap_or(color);
        runs.push(gpui_kit::TextRun {
            len: part.text.len(),
            font,
            color: tone.into(),
            background_color: None,
            underline: None,
            strikethrough: Some(gpui_kit::StrikethroughStyle {
                thickness: crate::theme::tokens::size::HAIRLINE,
                color: Some(tone.into()),
            }),
        });
        text.push_str(&part.text);
    }
    gpui_kit::StyledText::new(text).with_runs(runs)
}

// When ---------------------------------------------------------------------------

/// The Field select's options, in its order.
pub const FIELDS: [MatchField; 4] = [
    MatchField::Merchant,
    MatchField::Description,
    MatchField::SourceCategory,
    MatchField::SourceType,
];

/// The Test select's options, in its order.
pub const TESTS: [MatchKind; 4] = [
    MatchKind::Is,
    MatchKind::StartsWith,
    MatchKind::Contains,
    MatchKind::Regex,
];

/// "Merchant", "Bank text", "Bank category", "Bank type".
pub fn field(field: MatchField) -> &'static str {
    match field {
        MatchField::Merchant => "Merchant",
        MatchField::Description => "Bank text",
        MatchField::SourceCategory => "Bank category",
        MatchField::SourceType => "Bank type",
    }
}

/// The Test select's words: "is", "starts with", "contains", "matches
/// pattern".
pub fn test(kind: MatchKind) -> &'static str {
    match kind {
        MatchKind::Is => "is",
        MatchKind::StartsWith => "starts with",
        MatchKind::Contains => "contains",
        MatchKind::Regex => "matches pattern",
    }
}

/// The test in a sentence: "matches" for a pattern.
fn test_in_sentence(kind: MatchKind) -> &'static str {
    match kind {
        MatchKind::Regex => "matches",
        kind => test(kind),
    }
}

/// "Merchant contains": a rule's field and test, before its pattern.
pub fn condition(rule: &RuleDefinition) -> String {
    format!(
        "{} {}",
        field(rule.field()),
        test_in_sentence(rule.pattern().kind())
    )
}

/// A rule's pattern, plain.
pub fn pattern(rule: &RuleDefinition) -> String {
    plain(rule.pattern().as_str(), PATTERN_SHOWN)
}

/// An amount the way a condition says it, without a sign: `£1,000`.
fn amount(minor: i64, currency: Currency) -> String {
    let text = format::money(Money::new(minor.saturating_abs(), currency), Sign::Negative);
    // Whole amounts lose their pence, as Paper writes "over £1,000".
    text.strip_suffix(".00").map_or(text.clone(), str::to_owned)
}

/// The amount condition as a row's extra: ("over", "£1,000"), ("between",
/// "£5 and £10"), with " in" after money in. `None` for any amount.
pub fn amount_words(bounds: AmountBounds, currency: Currency) -> Option<(String, String)> {
    let money = |minor| amount(minor, currency);
    let (lead, value, incoming) = match (bounds.min(), bounds.max()) {
        (None, None) => return None,
        (None, Some(max)) if max < 0 => ("over", money(max), false),
        (Some(min), None) if min > 0 => ("over", money(min), true),
        (Some(min), Some(0)) if min < 0 => ("under", money(min), false),
        (Some(0), Some(max)) if max > 0 => ("under", money(max), true),
        (Some(min), Some(max)) if max < 0 => (
            "between",
            format!("{} and {}", money(max), money(min)),
            false,
        ),
        (Some(min), Some(max)) if min > 0 => (
            "between",
            format!("{} and {}", money(min), money(max)),
            true,
        ),
        (min, max) => {
            let signed = |minor: i64| format::money(Money::new(minor, currency), Sign::Always);
            return Some((
                "amount".to_owned(),
                match (min, max) {
                    (Some(min), Some(max)) => format!("{} to {}", signed(min), signed(max)),
                    (Some(min), None) => format!("from {}", signed(min)),
                    (None, Some(max)) => format!("to {}", signed(max)),
                    (None, None) => String::new(),
                },
            ));
        }
    };
    let value = if incoming {
        format!("{value} in")
    } else {
        value
    };
    Some((lead.to_owned(), value))
}

/// The rule's When as parts for one line: the condition, the pattern in
/// mono, and the amount after a "·".
pub fn when_parts(
    rule: &RuleDefinition,
    currency: Currency,
    pattern_color: gpui_kit::Rgba,
    muted: gpui_kit::Rgba,
) -> Vec<Part> {
    let mut parts = vec![
        Part::text(format!("{} ", condition(rule))),
        Part::mono(pattern(rule)).color(pattern_color),
    ];
    if let Some((lead, value)) = amount_words(rule.amount(), currency) {
        parts.push(Part::text(format!(" · {lead} ")).color(muted));
        parts.push(Part::mono(value).color(pattern_color));
    }
    parts
}

// Then ---------------------------------------------------------------------------

/// The Then rows' labels, by kind.
pub fn action_label(kind: ActionKind) -> &'static str {
    match kind {
        ActionKind::SetCategory => "Category",
        ActionKind::SetMerchant => "Merchant",
        ActionKind::AddTag => "Tag",
        ActionKind::SetNote => "Note",
        ActionKind::Exclude => "Exclude",
        ActionKind::SplitTemplate => "Split",
        ActionKind::MyShare => "My share",
    }
}

/// The ACTION filter's labels.
pub fn action_filter_label(kind: ActionKind) -> &'static str {
    match kind {
        ActionKind::SetCategory => "Set category",
        ActionKind::SetMerchant => "Set merchant",
        ActionKind::AddTag => "Add tag",
        ActionKind::SetNote => "Set note",
        ActionKind::Exclude => "Exclude",
        ActionKind::SplitTemplate => "Split",
        ActionKind::MyShare => "My share",
    }
}

/// The add-action menu's order and labels (Paper `HFH-0`): Exclude… last,
/// under a hairline.
pub const MENU_ACTIONS: [ActionKind; 6] = [
    ActionKind::SetCategory,
    ActionKind::SetMerchant,
    ActionKind::AddTag,
    ActionKind::SetNote,
    ActionKind::SplitTemplate,
    ActionKind::MyShare,
];

pub fn menu_label(kind: ActionKind) -> &'static str {
    match kind {
        ActionKind::SetMerchant => "Merchant name",
        ActionKind::Exclude => "Exclude…",
        kind => action_label(kind),
    }
}

/// The ACTION filter's order: the panel's (Paper `BRM-0`).
pub const PANEL_ACTIONS: [ActionKind; 7] = [
    ActionKind::SetCategory,
    ActionKind::SetMerchant,
    ActionKind::AddTag,
    ActionKind::SetNote,
    ActionKind::Exclude,
    ActionKind::SplitTemplate,
    ActionKind::MyShare,
];

pub fn action_glyph(kind: ActionKind) -> Glyph {
    match kind {
        ActionKind::SetCategory => Glyph::SetCategory,
        ActionKind::SetMerchant => Glyph::NameTag,
        ActionKind::AddTag => Glyph::Hash,
        ActionKind::SetNote => Glyph::Note,
        ActionKind::Exclude => Glyph::Exclude,
        ActionKind::SplitTemplate => Glyph::Split,
        ActionKind::MyShare => Glyph::Share,
    }
}

/// A rule action as the THEN lane's suffix: "+ tag", "+ name".
fn suffix(kind: ActionKind) -> &'static str {
    match kind {
        ActionKind::SetCategory => "category",
        ActionKind::SetMerchant => "name",
        ActionKind::AddTag => "tag",
        ActionKind::SetNote => "note",
        ActionKind::Exclude => "exclude",
        ActionKind::SplitTemplate => "split",
        ActionKind::MyShare => "my share",
    }
}

/// The exclusion reasons a rule can use (never Other: it needs a note).
pub const REASONS: [ExcludeReason; 3] = [
    ExcludeReason::Duplicate,
    ExcludeReason::BankError,
    ExcludeReason::Disputed,
];

pub fn reason(reason: ExcludeReason) -> &'static str {
    match reason {
        ExcludeReason::Duplicate => "Duplicate",
        ExcludeReason::BankError => "Bank error",
        ExcludeReason::Disputed => "Disputed",
        ExcludeReason::Other => "Other",
    }
}

/// How the THEN lane leads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ThenLead {
    /// A category: its marker and label.
    Category(CategoryKey),
    /// A rule without a category: its first action's icon, kind word and
    /// value ("Tag Gift Aid").
    Action {
        kind: ActionKind,
        word: &'static str,
        value: String,
    },
}

/// A rule's THEN: what leads, and the other actions as suffixes.
pub fn then(rule: &RuleDefinition, data: &RulesData) -> (ThenLead, Vec<&'static str>) {
    let lead_action = rule
        .action(ActionKind::SetCategory)
        .or_else(|| rule.actions().first());
    let lead = match lead_action {
        Some(RuleAction::SetCategory(key)) => ThenLead::Category(key.clone()),
        Some(action) => ThenLead::Action {
            kind: action.kind(),
            word: action_label(action.kind()),
            value: action_value(action, data),
        },
        None => ThenLead::Action {
            kind: ActionKind::SetCategory,
            word: "",
            value: String::new(),
        },
    };
    let lead_kind = lead_action.map(RuleAction::kind);
    let suffixes = rule
        .actions()
        .iter()
        .map(RuleAction::kind)
        .filter(|kind| Some(*kind) != lead_kind)
        .map(suffix)
        .collect();
    (lead, suffixes)
}

/// An action's value in words: "Gift Aid", "Bills, Groceries", "50%".
pub fn action_value(action: &RuleAction, data: &RulesData) -> String {
    let label = |key: &CategoryKey| {
        data.category(key)
            .map_or_else(|| key.as_str().to_owned(), |option| option.label.clone())
    };
    match action {
        RuleAction::SetCategory(key) => label(key),
        RuleAction::SetMerchant(text) | RuleAction::SetNote(text) => {
            plain(text.as_str(), NAME_SHOWN)
        }
        RuleAction::AddTag(tags) => plain(
            &tags
                .tags()
                .iter()
                .map(|tag| tag.as_str())
                .collect::<Vec<_>>()
                .join(", "),
            NAME_SHOWN,
        ),
        RuleAction::Exclude(why) => reason(*why).to_owned(),
        RuleAction::SplitTemplate(template) => template
            .lines()
            .iter()
            .map(|line| label(line.category()))
            .collect::<Vec<_>>()
            .join(", "),
        RuleAction::MyShare(share) => format!("{}%", super::form::percent_text(*share)),
    }
}

/// What the rule does, after "→": the category, else the lead action
/// ("Tag Gift Aid").
pub fn then_words(rule: &RuleDefinition, data: &RulesData) -> String {
    match then(rule, data).0 {
        ThenLead::Category(key) => data
            .category(&key)
            .map_or_else(|| key.as_str().to_owned(), |option| option.label.clone()),
        ThenLead::Action { word, value, .. } => format!("{word} {value}"),
    }
}

// Patterns -----------------------------------------------------------------------

/// What's wrong with a pattern, as the field's caption says it (Paper
/// `HFH-0`: "A bracket is opened but never closed.").
pub fn pattern_error(error: &PatternError) -> String {
    match error {
        PatternError::Empty => "Type the text to match.".to_owned(),
        PatternError::TooLong { max } => {
            format!("The text is longer than {max} characters.")
        }
        PatternError::RegexTooBig => {
            "This pattern is too complex: simplify it or use fewer repeats.".to_owned()
        }
        PatternError::InvalidRegex { reason } => {
            let known = [
                ("unclosed group", "A bracket is opened but never closed."),
                ("unopened group", "A bracket is closed but never opened."),
                (
                    "unclosed character class",
                    "A [ is opened but never closed.",
                ),
                (
                    "repetition operator missing expression",
                    "A *, + or ? has nothing before it to repeat.",
                ),
                ("incomplete escape", "A \\ ends the pattern."),
            ];
            known
                .iter()
                .find(|(key, _)| reason.contains(key))
                .map_or_else(
                    || sentence(&format!("this pattern can't be used: {reason}")),
                    |(_, words)| (*words).to_owned(),
                )
        }
    }
}

// Merchant names -----------------------------------------------------------------

/// The MATCH filter's order.
pub const ALIAS_KINDS: [AliasKind; 3] = [AliasKind::Exact, AliasKind::Prefix, AliasKind::Regex];

/// "Is", "Starts with", "Pattern": the row's lead and the MATCH filter.
pub fn alias_kind(kind: AliasKind) -> &'static str {
    match kind {
        AliasKind::Exact => "Is",
        AliasKind::Prefix => "Starts with",
        AliasKind::Regex => "Pattern",
    }
}

/// The name inspector's Test select: "is", "starts with", "matches
/// pattern".
pub fn alias_test(kind: AliasKind) -> &'static str {
    match kind {
        AliasKind::Exact => "is",
        AliasKind::Prefix => "starts with",
        AliasKind::Regex => "matches pattern",
    }
}

pub fn alias_glyph(kind: AliasKind) -> Glyph {
    match kind {
        AliasKind::Exact => Glyph::Equals,
        AliasKind::Prefix => Glyph::StartsWith,
        AliasKind::Regex => Glyph::Asterisk,
    }
}

// Change history -----------------------------------------------------------------

/// The KIND filter's order.
pub const CHANGE_KINDS: [ChangeKind; 3] = [
    ChangeKind::RuleApply,
    ChangeKind::BulkEdit,
    ChangeKind::DuplicateMerge,
];

pub fn change_kind(kind: ChangeKind) -> &'static str {
    match kind {
        ChangeKind::RuleApply => "Rule applied",
        ChangeKind::BulkEdit => "Bulk edit",
        ChangeKind::DuplicateMerge => "Duplicate merge",
    }
}

pub fn change_glyph(kind: ChangeKind) -> Glyph {
    match kind {
        ChangeKind::RuleApply => Glyph::Arrow,
        ChangeKind::BulkEdit => Glyph::Lines,
        ChangeKind::DuplicateMerge => Glyph::Duplicate,
    }
}

fn field_word(field: Field) -> &'static str {
    match field {
        Field::Category => "category",
        Field::Merchant => "name",
        Field::UserNote => "note",
        Field::Tags => "tags",
        Field::Exclusion => "exclusion",
        Field::Split => "split",
        Field::Share => "share",
        Field::Reimbursable => "claim",
    }
}

/// A change's description in pieces, each with whether it's mono.
fn change_pieces(entry: &HistoryEntry, data: &RulesData) -> Vec<(String, bool)> {
    let rows = count(entry.batch.transactions, "transaction", "transactions");
    let label = |key: &CategoryKey| {
        data.category(key)
            .map_or_else(|| key.as_str().to_owned(), |option| option.label.clone())
    };
    match &entry.summary {
        Summary::Rule {
            definition: Some(rule),
            ..
        } => vec![
            ("Applied rule ".to_owned(), false),
            (pattern(rule), true),
            (format!(" → {}", then_words(rule, data)), false),
        ],
        Summary::Rule {
            definition: None, ..
        } => vec![("Applied a rule since deleted".to_owned(), false)],
        Summary::Category(Some(key)) => {
            vec![(format!("Categorised {rows} as {}", label(key)), false)]
        }
        Summary::Category(None) => vec![(format!("Uncategorised {rows}"), false)],
        Summary::Tags(tags) => vec![(
            format!("Tagged {rows} {}", plain(&tags.join(", "), NAME_SHOWN)),
            false,
        )],
        Summary::Merged { name } => vec![(
            format!(
                "Merged {}: one excluded as a duplicate",
                plain(name, NAME_SHOWN)
            ),
            false,
        )],
        Summary::Fields(fields) => {
            let words: Vec<&str> = fields.iter().copied().map(field_word).collect();
            vec![(
                format!(
                    "Changed the {} of {rows}",
                    crate::checks::text::join(&words)
                ),
                false,
            )]
        }
    }
}

/// A change's description as parts: "Applied rule `VINTED` → Shopping",
/// "Categorised 3 transactions as Shopping".
pub fn change_parts(entry: &HistoryEntry, data: &RulesData) -> Vec<Part> {
    change_pieces(entry, data)
        .into_iter()
        .map(|(text, mono)| {
            if mono {
                Part::mono(text)
            } else {
                Part::text(text)
            }
        })
        .collect()
}

/// A change's description as one plain string, for a dialog's title.
pub fn change_words(entry: &HistoryEntry, data: &RulesData) -> String {
    change_pieces(entry, data)
        .into_iter()
        .map(|(text, _)| text)
        .collect()
}

/// A value as "What changed" says it: "Uncategorised", "Shopping".
pub fn shown(value: &Shown, data: &RulesData, currency: Currency) -> String {
    match value {
        Shown::Category(None) => "Uncategorised".to_owned(),
        Shown::Category(Some(key)) => data
            .category(key)
            .map_or_else(|| key.as_str().to_owned(), |option| option.label.clone()),
        Shown::Split(0) => "Not split".to_owned(),
        Shown::Split(lines) => format!("Split in {lines}"),
        Shown::Merchant(None) => "Bank’s name".to_owned(),
        Shown::Merchant(Some(name)) => plain(name, NAME_SHOWN),
        Shown::Note(false) => "No note".to_owned(),
        Shown::Note(true) => "A note".to_owned(),
        Shown::Tags(tags) if tags.is_empty() => "No tags".to_owned(),
        Shown::Tags(tags) => plain(&tags.join(", "), NAME_SHOWN),
        Shown::Excluded(None) => "Counted".to_owned(),
        Shown::Excluded(Some(why)) => format!("Excluded · {}", exclusion(*why)),
        Shown::Share(None) => "All mine".to_owned(),
        Shown::Share(Some(minor)) => format!(
            "My share {}",
            format::money(Money::new(minor.saturating_abs(), currency), Sign::Negative)
        ),
        Shown::Reimbursable(true) => "Claimed back".to_owned(),
        Shown::Reimbursable(false) => "Just me".to_owned(),
    }
}

fn exclusion(reason: coffer_core::exclusion::ExclusionReason) -> &'static str {
    use coffer_core::exclusion::ExclusionReason;
    match reason {
        ExclusionReason::Duplicate => "Duplicate",
        ExclusionReason::BankError => "Bank error",
        ExclusionReason::Disputed => "Disputed",
        ExclusionReason::Other => "Other",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn amounts_read_as_the_row_says_them() {
        let gbp = Currency::GBP;
        let words = |min, max| {
            amount_words(AmountBounds::new(min, max).unwrap(), gbp)
                .map(|(lead, value)| format!("{lead} {value}"))
        };
        assert_eq!(words(None, None), None);
        assert_eq!(words(None, Some(-100_000)).as_deref(), Some("over £1,000"));
        assert_eq!(
            words(Some(200_000), None).as_deref(),
            Some("over £2,000 in")
        );
        assert_eq!(words(Some(-550), Some(0)).as_deref(), Some("under £5.50"));
        assert_eq!(
            words(Some(-1_000), Some(-500)).as_deref(),
            Some("between £5 and £10")
        );
        assert_eq!(
            words(Some(-500), Some(500)).as_deref(),
            Some("amount −£5.00 to +£5.00")
        );
    }

    #[test]
    fn a_pattern_fault_is_said_plainly() {
        let error = coffer_core::rule::Pattern::new(MatchKind::Regex, "^(ASOS|ZARA").unwrap_err();
        assert_eq!(
            pattern_error(&error),
            "A bracket is opened but never closed."
        );
        assert_eq!(
            pattern_error(&PatternError::Empty),
            "Type the text to match."
        );
    }

    #[test]
    fn errors_become_sentences() {
        assert_eq!(sentence("the pattern is empty"), "The pattern is empty.");
        assert_eq!(sentence("Done."), "Done.");
        assert_eq!(sentence(""), "");
    }
}
