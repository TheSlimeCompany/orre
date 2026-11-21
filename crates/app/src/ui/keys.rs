//! A view's keys as data (ROADMAP U.7): each key declared once, with what
//! it does and the hint the toolbar shows for it, so the binding, the hint
//! and a test-only `describe()` all read the same entry and can't drift.
//!
//! - [`Key`] is one decision key ("a" approves, hinted "A Approve"), or a
//!   key the toolbar doesn't hint (Escape clears the selection). A pair of
//!   keys with one hint is one [`Key`] too ([`Key::and`]: ⌥↑ and ⌥↓,
//!   hinted "⌥↑↓ Reorder").
//! - [`ViewKeys`] is a view's whole set: its list's movement keys
//!   ([`Keys`], hinted "↑↓ Select") and its [`Key`]s, in one key context.
//!   [`ViewKeys::bind`] binds them all, [`ViewKeys::hints`] lists the hints
//!   in the toolbar's order (the hinted keys, then ↑↓, unless the view puts
//!   ↑↓ first with [`ViewKeys::select_first`]), and [`hints`] draws them.
//!
//! A view declares its keys as a constant next to its key context, calls
//! `KEYS.bind(cx)` from its `init`, and draws its toolbar's hints with
//! `hints(KEYS.hints())`. Each view has a test that the keymap holds
//! exactly the declared keys (`ViewKeys::assert_bound`, test-only), so a key bound
//! some other way can't go unhinted.

use gpui_kit::{Action, App, IntoElement, KeyBinding, ParentElement as _, Styled as _, div};

use crate::theme::tokens::{color, spacing, typography};
use crate::ui::kbd::Kbd;
use crate::ui::list::Keys;
use crate::ui::text::TextRoleExt as _;

/// What a toolbar hint shows: the key cap ("A", "←→", "↑↓") and what the
/// key does ("Approve").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hint {
    pub cap: &'static str,
    pub label: &'static str,
}

/// One key of a view: the keystrokes that do it, the action they dispatch
/// and what it's called.
#[derive(Clone, Copy)]
pub struct Key {
    /// The keystrokes, in GPUI's syntax ("a"; "left" and "right" for ←→).
    keys: &'static [&'static str],
    /// The key cap the hint shows ("A", "←→").
    cap: &'static str,
    /// What it does ("Approve", "Clear the selection").
    label: &'static str,
    /// Whether the toolbar hints it.
    hinted: bool,
    /// Binds one of `keys` to the action in a context.
    binding: Binding,
    /// The keystrokes of a second action under the same hint, and their
    /// binding ([`Key::and`]); none for most keys.
    and: Option<(&'static [&'static str], Binding)>,
}

/// Binds a keystroke to a key's action in a context.
type Binding = fn(&'static str, &'static str) -> KeyBinding;

/// A binding of `key` to a fresh `A` in `context`: what [`Key`] keeps of
/// its action, so a table of keys with different actions can be a
/// constant.
fn binding<A: Action + Default>(key: &'static str, context: &'static str) -> KeyBinding {
    KeyBinding::new(key, A::default(), Some(context))
}

impl Key {
    /// `keys` dispatch `A`, hinted in the toolbar as `cap` and `label`.
    pub const fn new<A: Action + Default>(
        keys: &'static [&'static str],
        cap: &'static str,
        label: &'static str,
    ) -> Self {
        Self {
            keys,
            cap,
            label,
            hinted: true,
            binding: binding::<A>,
            and: None,
        }
    }

    /// `keys` dispatch `B` too, under the same hint: the other half of a
    /// pair ("alt-up" moves a rule up, "alt-down" down, both hinted "⌥↑↓
    /// Reorder").
    pub const fn and<B: Action + Default>(mut self, keys: &'static [&'static str]) -> Self {
        self.and = Some((keys, binding::<B>));
        self
    }

    /// Not hinted in the toolbar (Escape, which clears the selection).
    pub const fn unhinted(mut self) -> Self {
        self.hinted = false;
        self
    }

    /// The keystrokes, in GPUI's syntax: of the first action, for a pair.
    pub const fn keys(&self) -> &'static [&'static str] {
        self.keys
    }

    /// The keystrokes of a pair's second action ([`Key::and`]); none for a
    /// single key.
    pub const fn and_keys(&self) -> &'static [&'static str] {
        match self.and {
            Some((keys, _)) => keys,
            None => &[],
        }
    }

    /// The bindings of every keystroke, in `context`.
    fn bindings(&self, context: &'static str) -> impl Iterator<Item = KeyBinding> + '_ {
        let first = self.keys.iter().map(move |k| (self.binding)(k, context));
        let second = self
            .and
            .into_iter()
            .flat_map(move |(keys, binding)| keys.iter().map(move |k| binding(k, context)));
        first.chain(second)
    }

    pub const fn label(&self) -> &'static str {
        self.label
    }

    /// The toolbar's hint for it, if it's hinted.
    pub const fn hint(&self) -> Option<Hint> {
        if self.hinted {
            Some(Hint {
                cap: self.cap,
                label: self.label,
            })
        } else {
            None
        }
    }
}

impl std::fmt::Debug for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Key")
            .field("keys", &self.keys)
            .field("and_keys", &self.and_keys())
            .field("cap", &self.cap)
            .field("label", &self.label)
            .field("hinted", &self.hinted)
            .finish_non_exhaustive()
    }
}

/// A view's keys: its list's movement keys and its own [`Key`]s, all in
/// the list's key context.
#[derive(Clone, Copy, Debug)]
pub struct ViewKeys {
    list: Keys,
    keys: &'static [Key],
    /// Whether the list's "↑↓ Select" leads the hints.
    select_first: bool,
}

impl ViewKeys {
    /// `list`'s movement keys and `keys`, in `list`'s context.
    pub const fn new(list: Keys, keys: &'static [Key]) -> Self {
        Self {
            list,
            keys,
            select_first: false,
        }
    }

    /// The list's "↑↓ Select" leads the hints (Rules: ↑↓ Select, then ⌥↑↓
    /// Reorder).
    pub const fn select_first(mut self) -> Self {
        self.select_first = true;
        self
    }

    /// The list's movement keys, for [`crate::ui::list::ListKeysExt::on_list_move`].
    pub const fn list(&self) -> &Keys {
        &self.list
    }

    /// The view's own keys, in the order they're declared.
    pub const fn keys(&self) -> &'static [Key] {
        self.keys
    }

    pub const fn context(&self) -> &'static str {
        self.list.context()
    }

    /// Binds every key in the view's context. Call once, from the view's
    /// `init`.
    pub fn bind(&self, cx: &mut App) {
        self.list.bind(cx);
        cx.bind_keys(self.own_bindings());
    }

    /// The bindings of the view's own keys (not the list's).
    fn own_bindings(&self) -> Vec<KeyBinding> {
        let context = self.context();
        self.keys
            .iter()
            .flat_map(|key| key.bindings(context))
            .collect()
    }

    /// The toolbar's hints, in order: the hinted keys, then the list's ↑↓
    /// (first, with [`ViewKeys::select_first`]).
    pub fn hints(&self) -> Vec<Hint> {
        let own = self.keys.iter().filter_map(Key::hint);
        let select = [self.list.hint()];
        if self.select_first {
            select.into_iter().chain(own).collect()
        } else {
            own.chain(select).collect()
        }
    }
}

/// A toolbar's key hints: each a [`Kbd`] and its label in caption type,
/// `text-muted`, 6px apart; hints 12px apart.
pub fn hints(hints: impl IntoIterator<Item = Hint>) -> gpui_kit::Div {
    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(spacing::S3)
        .children(hints.into_iter().map(hint))
}

fn hint(Hint { cap, label }: Hint) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap(spacing::S1_5)
        .child(Kbd::new(cap))
        .child(
            div()
                .text_role(typography::CAPTION)
                .text_color(color::TEXT_MUTED)
                .child(label),
        )
}

#[cfg(any(test, feature = "dev-driver"))]
impl ViewKeys {
    /// Asserts that the keymap binds exactly these keys in the view's
    /// context ([`ViewKeys::drift`] is empty).
    pub fn assert_bound(&self, cx: &App) {
        let drift = self.drift(cx);
        assert!(drift.is_empty(), "{}", drift.join("\n"));
    }

    /// Where the keymap and these keys disagree in the view's context: a
    /// declared key that isn't bound to its action, or a key bound there
    /// that isn't declared (so it has no hint and no entry in a
    /// `describe()`).
    pub fn drift(&self, cx: &App) -> Vec<String> {
        let context = gpui_kit::KeyBindingContextPredicate::parse(self.context())
            .expect("a key context parses");
        let declared: Vec<KeyBinding> = self
            .list
            .bindings()
            .into_iter()
            .chain(self.own_bindings())
            .collect();
        let same = |a: &KeyBinding, b: &KeyBinding| {
            a.keystrokes() == b.keystrokes() && a.action().partial_eq(b.action())
        };
        let keymap = cx.key_bindings();
        let keymap = keymap.borrow();
        let bound: Vec<&KeyBinding> = keymap
            .bindings()
            .filter(|binding| binding.predicate().as_deref() == Some(&context))
            .collect();
        let unbound = declared
            .iter()
            .filter(|binding| !bound.iter().any(|b| same(b, binding)))
            .map(|binding| format!("declared but not bound: {binding:?}"));
        let undeclared = bound
            .iter()
            .filter(|binding| !declared.iter().any(|d| same(d, binding)))
            .map(|binding| format!("bound but not declared, so not hinted: {binding:?}"));
        unbound.chain(undeclared).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    gpui_kit::actions!(orre_keys_test, [Yes, No, Up, Down]);

    const KEYS: ViewKeys = ViewKeys::new(
        Keys::new("KeysTest"),
        &[
            Key::new::<Yes>(&["y"], "Y", "Yes"),
            Key::new::<No>(&["left", "right"], "←→", "No"),
            Key::new::<Yes>(&["escape"], "Esc", "Never mind").unhinted(),
        ],
    );

    /// A pair under one hint, with ↑↓ hinted first.
    const PAIRED: ViewKeys = ViewKeys::new(
        Keys::new("PairTest"),
        &[Key::new::<Up>(&["alt-up"], "⌥↑↓", "Reorder").and::<Down>(&["alt-down"])],
    )
    .select_first();

    #[test]
    fn a_pair_is_one_hint_after_select_and_binds_both_actions() {
        assert_eq!(
            PAIRED.hints(),
            [
                Hint {
                    cap: "↑↓",
                    label: "Select"
                },
                Hint {
                    cap: "⌥↑↓",
                    label: "Reorder"
                },
            ]
        );
        let pair = &PAIRED.keys()[0];
        assert_eq!(
            (pair.keys(), pair.and_keys()),
            (&["alt-up"][..], &["alt-down"][..])
        );
        let bindings = PAIRED.own_bindings();
        assert_eq!(bindings.len(), 2);
        assert!(bindings[0].action().partial_eq(&Up));
        assert!(bindings[1].action().partial_eq(&Down));
        assert!(KEYS.keys()[0].and_keys().is_empty());
    }

    #[gpui_kit::test]
    fn both_keys_of_a_pair_are_bound(cx: &mut gpui_kit::TestAppContext) {
        cx.update(|cx| {
            PAIRED.bind(cx);
            PAIRED.assert_bound(cx);
        });
    }

    #[test]
    fn hints_list_the_hinted_keys_then_the_list() {
        assert_eq!(
            KEYS.hints(),
            [
                Hint {
                    cap: "Y",
                    label: "Yes"
                },
                Hint {
                    cap: "←→",
                    label: "No"
                },
                Hint {
                    cap: "↑↓",
                    label: "Select"
                },
            ]
        );
    }

    #[test]
    fn every_keystroke_is_bound() {
        assert_eq!(KEYS.own_bindings().len(), 4);
    }

    #[gpui_kit::test]
    fn bound_keys_are_the_declared_ones(cx: &mut gpui_kit::TestAppContext) {
        cx.update(|cx| {
            KEYS.bind(cx);
            KEYS.assert_bound(cx);
        });
    }

    #[gpui_kit::test]
    fn a_key_bound_outside_the_table_is_caught(cx: &mut gpui_kit::TestAppContext) {
        cx.update(|cx| {
            KEYS.bind(cx);
            cx.bind_keys([KeyBinding::new("n", No, Some("KeysTest"))]);
            let drift = KEYS.drift(cx);
            assert_eq!(drift.len(), 1, "{drift:?}");
            assert!(drift[0].starts_with("bound but not declared"), "{drift:?}");
        });
    }

    #[gpui_kit::test]
    fn a_declared_key_left_unbound_is_caught(cx: &mut gpui_kit::TestAppContext) {
        cx.update(|cx| {
            // Only the list's keys, not the view's own.
            KEYS.list().bind(cx);
            let drift = KEYS.drift(cx);
            assert_eq!(drift.len(), 4, "{drift:?}");
            assert!(
                drift
                    .iter()
                    .all(|line| line.starts_with("declared but not bound")),
                "{drift:?}"
            );
        });
    }
}
