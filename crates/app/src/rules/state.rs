//! What each section of the Rules view holds: its panel filter and its
//! selection, with the form that selection edits, as one value. A form
//! exists exactly when a row is selected (or being made), a preview belongs
//! to the rule it previews, and the suggestions over a rule belong to the
//! rule selected, so none of them can disagree with the selection.
//!
//! Each section keeps its own value while another is open, so coming back
//! finds the same row selected. Leaving Rules is the exception for what is
//! only a draft: [`RulesPane::leave`].

use orre_core::change::{ChangeKind, HistoryItem};
use orre_core::id::{ChangeBatchId, MerchantAliasId, RuleId};
use orre_core::inbox::Suggestion;
use orre_core::merchant::AliasKind;
use orre_core::rule::{ActionKind, Draft, RuleOrigin};

use super::form::{CategoryForm, NameForm, RuleForm};
use super::store::RulesData;
use super::{Apply, CategoryFilter, CategorySelection, NameSelection};

/// The Rules section: the ACTION filter and the rule being edited.
#[derive(Default)]
pub(super) struct RulesPane {
    pub(super) filter: Option<ActionKind>,
    pub(super) editing: Option<RuleEdit>,
}

/// The rule in the inspector.
pub(super) enum RuleEdit {
    /// A stored rule: its fields, the pending agent rules over its rows and
    /// the preview of applying it, when open.
    Stored {
        id: RuleId,
        form: RuleForm,
        touching: Vec<Suggestion>,
        apply: Option<Apply>,
    },
    /// The rule being made, not stored yet: its place and origin, and its
    /// preview, which is always open.
    New {
        form: RuleForm,
        position: usize,
        origin: RuleOrigin,
        apply: Apply,
    },
}

impl RuleEdit {
    pub(super) fn form(&self) -> &RuleForm {
        match self {
            RuleEdit::Stored { form, .. } | RuleEdit::New { form, .. } => form,
        }
    }

    pub(super) fn form_mut(&mut self) -> &mut RuleForm {
        match self {
            RuleEdit::Stored { form, .. } | RuleEdit::New { form, .. } => form,
        }
    }

    pub(super) fn is_new(&self) -> bool {
        matches!(self, RuleEdit::New { .. })
    }

    pub(super) fn stored_id(&self) -> Option<RuleId> {
        match self {
            RuleEdit::Stored { id, .. } => Some(*id),
            RuleEdit::New { .. } => None,
        }
    }

    pub(super) fn apply(&self) -> Option<&Apply> {
        match self {
            RuleEdit::Stored { apply, .. } => apply.as_ref(),
            RuleEdit::New { apply, .. } => Some(apply),
        }
    }

    pub(super) fn apply_mut(&mut self) -> Option<&mut Apply> {
        match self {
            RuleEdit::Stored { apply, .. } => apply.as_mut(),
            RuleEdit::New { apply, .. } => Some(apply),
        }
    }

    /// The new rule as core takes it, once its fields are valid.
    pub(super) fn draft(&self, data: &RulesData) -> Option<Draft> {
        let RuleEdit::New {
            form,
            position,
            origin,
            ..
        } = self
        else {
            return None;
        };
        let definition = form.definition(data.currency(form.account)).ok()?;
        Some(Draft {
            definition,
            position: *position,
            origin: *origin,
        })
    }
}

impl RulesPane {
    /// Leaving the section: a new rule is dropped, a stored rule's preview
    /// closes.
    pub(super) fn leave(&mut self) {
        match &mut self.editing {
            Some(RuleEdit::New { .. }) => self.editing = None,
            Some(RuleEdit::Stored { apply, .. }) => *apply = None,
            None => {}
        }
    }
}

/// The Merchant names section: the MATCH filter and the name being edited.
#[derive(Default)]
pub(super) struct NamesPane {
    pub(super) filter: Option<AliasKind>,
    pub(super) editing: Option<NameEdit>,
}

/// The name in the inspector.
pub(super) enum NameEdit {
    Stored {
        id: MerchantAliasId,
        form: NameForm,
    },
    /// "+ New name", not stored yet.
    New {
        form: NameForm,
    },
}

impl NameEdit {
    pub(super) fn form(&self) -> &NameForm {
        match self {
            NameEdit::Stored { form, .. } | NameEdit::New { form } => form,
        }
    }

    pub(super) fn form_mut(&mut self) -> &mut NameForm {
        match self {
            NameEdit::Stored { form, .. } | NameEdit::New { form } => form,
        }
    }
}

impl NamesPane {
    pub(super) fn selection(&self) -> Option<NameSelection> {
        match self.editing.as_ref()? {
            NameEdit::Stored { id, .. } => Some(NameSelection::Stored(*id)),
            NameEdit::New { .. } => Some(NameSelection::New),
        }
    }
}

/// The Change history section: the KIND filter and the change selected,
/// with its items.
#[derive(Default)]
pub(super) struct HistoryPane {
    pub(super) filter: Option<ChangeKind>,
    pub(super) selected: Option<(ChangeBatchId, Vec<HistoryItem>)>,
}

impl HistoryPane {
    pub(super) fn selected_id(&self) -> Option<ChangeBatchId> {
        self.selected.as_ref().map(|(id, _)| *id)
    }
}

/// The Categories section: the KIND filter and the category being edited.
#[derive(Default)]
pub(super) struct CategoriesPane {
    pub(super) filter: Option<CategoryFilter>,
    pub(super) editing: Option<CategoryEdit>,
}

/// The category in the inspector.
pub(super) enum CategoryEdit {
    Stored {
        id: i64,
        form: CategoryForm,
    },
    /// "+ New category", not stored yet.
    New {
        form: CategoryForm,
    },
}

impl CategoryEdit {
    pub(super) fn form(&self) -> &CategoryForm {
        match self {
            CategoryEdit::Stored { form, .. } | CategoryEdit::New { form } => form,
        }
    }

    pub(super) fn form_mut(&mut self) -> &mut CategoryForm {
        match self {
            CategoryEdit::Stored { form, .. } | CategoryEdit::New { form } => form,
        }
    }
}

impl CategoriesPane {
    pub(super) fn selection(&self) -> Option<CategorySelection> {
        match self.editing.as_ref()? {
            CategoryEdit::Stored { id, .. } => Some(CategorySelection::Stored(*id)),
            CategoryEdit::New { .. } => Some(CategorySelection::New),
        }
    }
}
