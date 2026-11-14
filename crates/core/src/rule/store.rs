//! Storing rules: `rule` rows with their `rule_action` rows, in priority
//! order.
//!
//! Every write is one database transaction. A rule is validated as a
//! [`RuleDefinition`] before it is stored, and again when it is read back,
//! so a row changed outside core can't reach the engine unchecked.

use rusqlite::{Connection, OptionalExtension, Row, params};

use super::{
    ActionKind, AmountBounds, AmountBoundsError, MatchField, MatchKind, Pattern, PatternError,
    RuleAction, RuleDefinition, RuleError, RuleOrigin,
};
use crate::category::CategoryKey;
use crate::db::Database;
use crate::id::RuleId;

/// A stored rule: its ID, its priority and its definition.
///
/// Rules are evaluated by priority, lowest first, then by ID. Priorities
/// aren't unique and their values mean nothing beyond that order: use a
/// rule's position in [`list`] for the Position select.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredRule {
    id: RuleId,
    priority: i64,
    definition: RuleDefinition,
    origin: Option<RuleOrigin>,
    created_at: String,
}

impl StoredRule {
    /// The rule's row ID.
    pub const fn id(&self) -> RuleId {
        self.id
    }

    /// The stored priority (`rule.priority`): lower runs first.
    pub const fn priority(&self) -> i64 {
        self.priority
    }

    /// Where the rule came from (`rule.origin`, V16); `None` for a rule
    /// stored before that was recorded.
    pub const fn origin(&self) -> Option<RuleOrigin> {
        self.origin
    }

    /// When the rule was stored (`rule.created_at`, UTC, RFC 3339 with
    /// milliseconds).
    pub fn created_at(&self) -> &str {
        &self.created_at
    }

    /// What the rule matches and does.
    pub const fn definition(&self) -> &RuleDefinition {
        &self.definition
    }
}

/// Why storing, reading or applying rules failed.
///
/// Messages never contain a rule's pattern or action text, or transaction
/// text. Category keys are named: they are validated keys, not free text.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// There is no rule with this ID.
    #[error("there is no rule {0}")]
    NotFound(RuleId),

    /// A position past the end of the rule list.
    #[error("there is no position {position} in the rule list: the last is {last}")]
    NoPosition {
        /// The position asked for, 0-based.
        position: usize,
        /// The last position there is, 0-based.
        last: usize,
    },

    /// A new rule names a category that doesn't exist.
    #[error("there is no category {0}")]
    UnknownCategory(CategoryKey),

    /// A stored rule names a category that no longer exists.
    #[error("rule {rule} sets category {key}, which doesn't exist")]
    RuleCategoryMissing {
        /// The rule.
        rule: RuleId,
        /// The key it names.
        key: CategoryKey,
    },

    /// A stored rule no longer validates.
    #[error("rule {id} is stored with {problem}")]
    Invalid {
        /// The rule.
        id: RuleId,
        /// What is wrong with it.
        problem: InvalidRule,
    },

    /// Writing what a rule sets failed (`change::Recorder`): a value the
    /// row can't take.
    #[error("applying a rule: {0}")]
    Change(#[from] crate::change::Error),

    /// The database refused the operation.
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
}

/// What is wrong with a stored rule ([`StoreError::Invalid`]).
///
/// Never carries the rule's text: an action value's own error can quote it,
/// so only the action's kind is kept.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidRule {
    /// `match_field` isn't a [`MatchField`] key.
    #[error("an unknown match field")]
    MatchField,

    /// `match_kind` isn't a [`MatchKind`] key.
    #[error("an unknown match kind")]
    MatchKind,

    /// The pattern doesn't validate for its kind.
    #[error("an invalid pattern: {0}")]
    Pattern(PatternError),

    /// The amount bounds are the wrong way round.
    #[error("invalid amount bounds: {0}")]
    Amount(AmountBoundsError),

    /// A `rule_action.kind` isn't an [`ActionKind`] key.
    #[error("an unknown action kind")]
    ActionKind,

    /// An action's `value_json` doesn't decode for its kind.
    #[error("an invalid {0} action")]
    Action(ActionKind),

    /// The rule as a whole doesn't validate (no actions, or a category with
    /// a split).
    #[error("an invalid definition: {0}")]
    Definition(RuleError),
}

/// Stores a new rule first in the list, above every existing rule
/// (decision record "rules", point 2: a new rule is usually narrower than
/// the rules it corrects).
///
/// Every category the rule names must exist.
pub fn create(db: &mut Database, rule: &RuleDefinition) -> Result<RuleId, StoreError> {
    create_at(db, rule, 0)
}

/// Stores a new rule at `position`, as [`create_at`], recording where it
/// came from ([`RuleOrigin`]).
pub fn create_from(
    db: &mut Database,
    rule: &RuleDefinition,
    position: usize,
    origin: RuleOrigin,
) -> Result<RuleId, StoreError> {
    let tx = db.conn_mut().transaction()?;
    let id = create_at_in(&tx, rule, position, origin)?;
    tx.commit()?;
    Ok(id)
}

/// Stores a new rule at `position` in the list (0 is first, the current
/// number of rules is last) and renumbers the priorities.
///
/// Every category the rule names must exist.
pub fn create_at(
    db: &mut Database,
    rule: &RuleDefinition,
    position: usize,
) -> Result<RuleId, StoreError> {
    create_from(db, rule, position, RuleOrigin::New)
}

/// [`create_from`] inside the caller's DB transaction.
pub(crate) fn create_at_in(
    tx: &Connection,
    rule: &RuleDefinition,
    position: usize,
    origin: RuleOrigin,
) -> Result<RuleId, StoreError> {
    let mut order = ordered_ids(tx)?;
    if position > order.len() {
        return Err(StoreError::NoPosition {
            position,
            last: order.len(),
        });
    }
    check_categories(tx, rule)?;
    let pattern = rule.pattern();
    tx.execute(
        "INSERT INTO rule (priority, match_field, match_kind, pattern, amount_min, amount_max, \
         account_id, origin) VALUES (0, ?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            rule.field().as_str(),
            pattern.kind().as_str(),
            pattern.as_str(),
            rule.amount().min(),
            rule.amount().max(),
            rule.account_id(),
            origin.as_str(),
        ],
    )?;
    let id = RuleId::new(tx.last_insert_rowid());
    insert_actions(tx, id, rule)?;
    order.insert(position, id);
    renumber(tx, &order)?;
    Ok(id)
}

/// Every category `rule` names must exist.
fn check_categories(conn: &Connection, rule: &RuleDefinition) -> Result<(), StoreError> {
    for key in category_keys(rule) {
        let exists = conn
            .prepare_cached("SELECT 1 FROM category WHERE key = ?1")?
            .exists([key])?;
        if !exists {
            return Err(StoreError::UnknownCategory(key.clone()));
        }
    }
    Ok(())
}

fn insert_actions(conn: &Connection, id: RuleId, rule: &RuleDefinition) -> rusqlite::Result<()> {
    for action in rule.actions() {
        conn.execute(
            "INSERT INTO rule_action (rule_id, kind, value_json) VALUES (?1, ?2, ?3)",
            params![id, action.kind().as_str(), action.value_json()],
        )?;
    }
    Ok(())
}

/// Replaces what rule `id` matches and does with `rule`, keeping its
/// place, origin and creation date.
///
/// Rule edits don't re-run on history (decision record "rules", point 4):
/// the transactions it set keep their values until an Apply. Every
/// category the rule names must exist. The database refuses moving a rule
/// with amount bounds to an account in another currency (V1's
/// `rule_account_currency`).
pub fn update(db: &mut Database, id: RuleId, rule: &RuleDefinition) -> Result<(), StoreError> {
    let tx = db.conn_mut().transaction()?;
    update_in(&tx, id, rule)?;
    tx.commit()?;
    Ok(())
}

/// [`update`] inside the caller's DB transaction.
fn update_in(conn: &Connection, id: RuleId, rule: &RuleDefinition) -> Result<(), StoreError> {
    check_categories(conn, rule)?;
    let pattern = rule.pattern();
    let changed = conn.execute(
        "UPDATE rule SET match_field = ?2, match_kind = ?3, pattern = ?4, amount_min = ?5, \
         amount_max = ?6, account_id = ?7 WHERE id = ?1",
        params![
            id,
            rule.field().as_str(),
            pattern.kind().as_str(),
            pattern.as_str(),
            rule.amount().min(),
            rule.amount().max(),
            rule.account_id(),
        ],
    )?;
    if changed == 0 {
        return Err(StoreError::NotFound(id));
    }
    conn.execute("DELETE FROM rule_action WHERE rule_id = ?1", [id])?;
    insert_actions(conn, id, rule)?;
    Ok(())
}

/// The rule with this ID, validated.
pub fn get(db: &Database, id: RuleId) -> Result<StoredRule, StoreError> {
    let conn = db.conn();
    let raw = conn
        .query_row(
            &format!("SELECT {COLUMNS} FROM rule WHERE id = ?1"),
            [id],
            RawRule::from_row,
        )
        .optional()?
        .ok_or(StoreError::NotFound(id))?;
    raw.validate(conn)
}

/// Every rule in evaluation order (priority, then ID), validated. A rule's
/// index here is its position.
pub fn list(db: &Database) -> Result<Vec<StoredRule>, StoreError> {
    load_all(db.conn())
}

/// Moves a rule to `position` in the list (0 is first, the last rule's
/// index is last) and renumbers the priorities.
pub fn move_to(db: &mut Database, id: RuleId, position: usize) -> Result<(), StoreError> {
    let tx = db.conn_mut().transaction()?;
    let mut order = ordered_ids(&tx)?;
    let from = order
        .iter()
        .position(|rule| *rule == id)
        .ok_or(StoreError::NotFound(id))?;
    if position >= order.len() {
        return Err(StoreError::NoPosition {
            position,
            last: order.len() - 1,
        });
    }
    order.remove(from);
    order.insert(position, id);
    renumber(&tx, &order)?;
    tx.commit()?;
    Ok(())
}

/// Deletes a rule and its actions.
///
/// The transactions it set keep their values: their `rule_id` is cleared,
/// and `category_id` and `category_source` stay as they are (decision
/// record "rules", point 7), so a later rule or Apply may change them.
pub fn delete(db: &mut Database, id: RuleId) -> Result<(), StoreError> {
    let tx = db.conn_mut().transaction()?;
    tx.execute(
        "UPDATE \"transaction\" SET rule_id = NULL WHERE rule_id = ?1",
        [id],
    )?;
    tx.execute("DELETE FROM rule_action WHERE rule_id = ?1", [id])?;
    if tx.execute("DELETE FROM rule WHERE id = ?1", [id])? == 0 {
        return Err(StoreError::NotFound(id));
    }
    tx.commit()?;
    Ok(())
}

/// Every rule in evaluation order, validated.
pub(crate) fn load_all(conn: &Connection) -> Result<Vec<StoredRule>, StoreError> {
    let mut statement =
        conn.prepare(&format!("SELECT {COLUMNS} FROM rule ORDER BY priority, id"))?;
    let raw = statement
        .query_map([], RawRule::from_row)?
        .collect::<Result<Vec<_>, _>>()?;
    raw.into_iter().map(|rule| rule.validate(conn)).collect()
}

/// The category keys a rule names: its `set_category` key and its split
/// lines' keys.
fn category_keys(rule: &RuleDefinition) -> Vec<&CategoryKey> {
    rule.actions()
        .iter()
        .flat_map(|action| match action {
            RuleAction::SetCategory(key) => vec![key],
            RuleAction::SplitTemplate(template) => template
                .lines()
                .iter()
                .map(|line| line.category())
                .collect(),
            _ => vec![],
        })
        .collect()
}

/// Rule IDs in evaluation order.
fn ordered_ids(conn: &Connection) -> rusqlite::Result<Vec<RuleId>> {
    let mut statement = conn.prepare("SELECT id FROM rule ORDER BY priority, id")?;
    statement.query_map([], |row| row.get(0))?.collect()
}

/// Gives the rules in `order` priorities 0, 1, 2, …, writing only the rows
/// whose priority changes.
fn renumber(conn: &Connection, order: &[RuleId]) -> rusqlite::Result<()> {
    let mut statement =
        conn.prepare("UPDATE rule SET priority = ?1 WHERE id = ?2 AND priority IS NOT ?1")?;
    for (priority, id) in order.iter().enumerate() {
        statement.execute(params![priority as i64, id])?;
    }
    Ok(())
}

const COLUMNS: &str = "id, priority, match_field, match_kind, pattern, amount_min, amount_max, \
     account_id, origin, created_at";

/// A `rule` row before validation.
struct RawRule {
    id: RuleId,
    priority: i64,
    field: String,
    kind: String,
    pattern: String,
    amount_min: Option<i64>,
    amount_max: Option<i64>,
    account_id: Option<i64>,
    origin: Option<String>,
    created_at: String,
}

impl RawRule {
    fn from_row(row: &Row<'_>) -> rusqlite::Result<RawRule> {
        Ok(RawRule {
            id: row.get("id")?,
            priority: row.get("priority")?,
            field: row.get("match_field")?,
            kind: row.get("match_kind")?,
            pattern: row.get("pattern")?,
            amount_min: row.get("amount_min")?,
            amount_max: row.get("amount_max")?,
            account_id: row.get("account_id")?,
            origin: row.get("origin")?,
            created_at: row.get("created_at")?,
        })
    }

    /// Reads the rule's actions and validates the whole rule.
    fn validate(self, conn: &Connection) -> Result<StoredRule, StoreError> {
        let id = self.id;
        let invalid = |problem| StoreError::Invalid { id, problem };
        let field: MatchField = self
            .field
            .parse()
            .map_err(|_| invalid(InvalidRule::MatchField))?;
        let kind: MatchKind = self
            .kind
            .parse()
            .map_err(|_| invalid(InvalidRule::MatchKind))?;
        let pattern =
            Pattern::new(kind, &self.pattern).map_err(|err| invalid(InvalidRule::Pattern(err)))?;
        let amount = AmountBounds::new(self.amount_min, self.amount_max)
            .map_err(|err| invalid(InvalidRule::Amount(err)))?;

        let mut statement = conn.prepare_cached(
            "SELECT kind, value_json FROM rule_action WHERE rule_id = ?1 ORDER BY id",
        )?;
        let rows = statement
            .query_map([id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        let actions = rows
            .iter()
            .map(|(kind, value_json)| {
                let kind: ActionKind =
                    kind.parse().map_err(|_| invalid(InvalidRule::ActionKind))?;
                RuleAction::decode(kind, value_json).map_err(|_| invalid(InvalidRule::Action(kind)))
            })
            .collect::<Result<Vec<_>, _>>()?;

        let definition = RuleDefinition::new(field, pattern, amount, self.account_id, actions)
            .map_err(|err| invalid(InvalidRule::Definition(err)))?;
        // The schema's CHECK keeps it a key; anything else reads as unknown.
        let origin = self.origin.and_then(|key| key.parse().ok());
        Ok(StoredRule {
            id,
            priority: self.priority,
            definition,
            origin,
            created_at: self.created_at,
        })
    }
}

impl StoredRule {
    /// A definition as a rule, for matching it without storing it.
    pub(crate) fn unsaved(id: RuleId, definition: RuleDefinition) -> StoredRule {
        StoredRule {
            id,
            priority: 0,
            definition,
            origin: None,
            created_at: String::new(),
        }
    }
}

#[cfg(test)]
impl StoredRule {
    /// A rule as if stored, for engine tests without a database.
    pub(super) fn new(id: RuleId, priority: i64, definition: RuleDefinition) -> StoredRule {
        StoredRule {
            id,
            priority,
            definition,
            origin: None,
            created_at: String::new(),
        }
    }
}
