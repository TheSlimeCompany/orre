//! Moving through a list from the keyboard (ROADMAP U.6): the one behaviour
//! every list shares, whatever it looks like.
//!
//! - [`Keys`] says which keys a list answers to in its key context (↑↓
//!   always; Home/End and ⇧↑⇧↓ where the list takes them) and binds them.
//!   Its [`Keys::moves`] lists them as data, for a test-only `describe()`.
//! - [`Move`] is one press of those keys. [`step`] (by id) and [`target`]
//!   (by index) find the row it reaches, with the rule every list uses: stop
//!   at the ends; with no row selected, or one that has gone, down and Home
//!   reach the first row and up and End the last.
//! - [`ListKeysExt::on_list_move`] puts the handlers for those keys on the
//!   list's element, so a view handles one [`Move`] instead of an action
//!   per key.
//! - [`Reveal`] scrolls the row reached into view, for a list that tracks
//!   its scrolling (a [`ScrollHandle`] by child index, or a [`ListState`]).
//!
//! The view keeps its own selection (an id, usually, so it survives a
//! reload), its rows' order and how they look, and decides what selecting
//! a row does. Nothing here knows what the rows are.

use std::rc::Rc;

use gpui_kit::base::actions::{SelectDown, SelectFirst, SelectLast, SelectUp};
use gpui_kit::{App, InteractiveElement, KeyBinding, ListState, ScrollHandle, Window};

gpui_kit::actions!(
    coffer_list,
    [
        /// ⇧↑: extend the selection one row up.
        ExtendSelectionUp,
        /// ⇧↓: extend the selection one row down.
        ExtendSelectionDown,
    ]
);

/// One press of a list's movement keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Move {
    /// `n` rows down, up when negative: ↑↓ are ±1.
    By(isize),
    /// `n` rows down (up when negative), extending a multiple selection
    /// from its anchor: ⇧↑⇧↓ are ±1.
    Extend(isize),
    /// Home: the first row.
    First,
    /// End: the last row.
    Last,
}

impl Move {
    /// ↑
    pub const UP: Self = Self::By(-1);
    /// ↓
    pub const DOWN: Self = Self::By(1);

    /// Whether the move extends a multiple selection (⇧↑⇧↓) rather than
    /// replacing it.
    pub fn extends(self) -> bool {
        matches!(self, Self::Extend(_))
    }
}

impl From<isize> for Move {
    fn from(step: isize) -> Self {
        Self::By(step)
    }
}

/// The index `to` reaches in a list of `len` rows from row `current`:
/// stopping at the ends; with no current row, down and Home reach the
/// first row and up and End the last. `None` when the list is empty.
pub fn target(len: usize, current: Option<usize>, to: Move) -> Option<usize> {
    let last = len.checked_sub(1)?;
    Some(match (to, current) {
        (Move::First, _) => 0,
        (Move::Last, _) => last,
        (Move::By(step) | Move::Extend(step), Some(at)) => {
            at.min(last).saturating_add_signed(step).min(last)
        }
        (Move::By(step) | Move::Extend(step), None) if step < 0 => last,
        (Move::By(_) | Move::Extend(_), None) => 0,
    })
}

/// Where `current` is in `order`, if it's there.
pub fn position<T: PartialEq>(order: &[T], current: Option<&T>) -> Option<usize> {
    let current = current?;
    order.iter().position(|row| row == current)
}

/// The row `to` reaches in `order` from `current`, by id: a row that has
/// gone from `order` (a reload dropped it) counts as none, so the move
/// starts again from an end ([`target`]). `None` when `order` is empty.
pub fn step<T: Clone + PartialEq>(order: &[T], current: Option<&T>, to: Move) -> Option<T> {
    let index = target(order.len(), position(order, current), to)?;
    Some(order[index].clone())
}

/// The keys a list answers to in its key context: ↑↓, and optionally
/// Home/End and ⇧↑⇧↓.
///
/// A view declares its list's keys once, as a constant, binds them in its
/// `init` ([`Keys::bind`]) and handles them with
/// [`ListKeysExt::on_list_move`] on the element that has the context.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Keys {
    context: &'static str,
    ends: bool,
    extend: bool,
}

impl Keys {
    /// ↑↓ in `context`.
    pub const fn new(context: &'static str) -> Self {
        Self {
            context,
            ends: false,
            extend: false,
        }
    }

    /// Home and End go to the first and last rows too.
    pub const fn with_ends(mut self) -> Self {
        self.ends = true;
        self
    }

    /// ⇧↑⇧↓ extend a multiple selection too.
    pub const fn with_extend(mut self) -> Self {
        self.extend = true;
        self
    }

    pub const fn context(&self) -> &'static str {
        self.context
    }

    /// Each key (in GPUI's keystroke syntax) with the move it makes.
    pub fn moves(&self) -> Vec<(&'static str, Move)> {
        let mut moves = vec![("up", Move::UP), ("down", Move::DOWN)];
        if self.extend {
            moves.extend([
                ("shift-up", Move::Extend(-1)),
                ("shift-down", Move::Extend(1)),
            ]);
        }
        if self.ends {
            moves.extend([("home", Move::First), ("end", Move::Last)]);
        }
        moves
    }

    /// Binds the keys in the list's context. Call once, from the view's
    /// `init`.
    pub fn bind(&self, cx: &mut App) {
        let context = Some(self.context);
        let bindings: Vec<KeyBinding> = self
            .moves()
            .into_iter()
            .map(|(key, to)| match to {
                Move::By(step) if step < 0 => KeyBinding::new(key, SelectUp, context),
                Move::By(_) => KeyBinding::new(key, SelectDown, context),
                Move::Extend(step) if step < 0 => KeyBinding::new(key, ExtendSelectionUp, context),
                Move::Extend(_) => KeyBinding::new(key, ExtendSelectionDown, context),
                Move::First => KeyBinding::new(key, SelectFirst, context),
                Move::Last => KeyBinding::new(key, SelectLast, context),
            })
            .collect();
        cx.bind_keys(bindings);
    }
}

/// Handles a list's movement keys on the element that has its context.
pub trait ListKeysExt: InteractiveElement + Sized {
    /// Calls `handler` with the [`Move`] each of `keys` makes. Only the
    /// moves `keys` binds are handled, so any other key (Home in a list
    /// without it) goes on to whoever handles it further out. To keep a
    /// key from reaching a list further out (a menu inside a table), call
    /// `cx.stop_propagation()` in `handler`.
    fn on_list_move(
        self,
        keys: &Keys,
        handler: impl Fn(&Move, &mut Window, &mut App) + 'static,
    ) -> Self {
        let handler = Rc::new(handler);
        keys.moves().into_iter().fold(self, |this, (_, to)| {
            let handler = handler.clone();
            let run = move |window: &mut Window, cx: &mut App| handler(&to, window, cx);
            match to {
                Move::By(step) if step < 0 => {
                    this.on_action(move |_: &SelectUp, window, cx| run(window, cx))
                }
                Move::By(_) => this.on_action(move |_: &SelectDown, window, cx| run(window, cx)),
                Move::Extend(step) if step < 0 => {
                    this.on_action(move |_: &ExtendSelectionUp, window, cx| run(window, cx))
                }
                Move::Extend(_) => {
                    this.on_action(move |_: &ExtendSelectionDown, window, cx| run(window, cx))
                }
                Move::First => this.on_action(move |_: &SelectFirst, window, cx| run(window, cx)),
                Move::Last => this.on_action(move |_: &SelectLast, window, cx| run(window, cx)),
            }
        })
    }
}

impl<E: InteractiveElement> ListKeysExt for E {}

/// Something that scrolls a list's row into view.
pub trait Reveal {
    /// Scrolls the least needed to show row `index` (a child of the
    /// scrolling element, counting any headers between the rows) on the
    /// next frame.
    fn reveal(&self, index: usize);
}

impl Reveal for ScrollHandle {
    fn reveal(&self, index: usize) {
        self.scroll_to_item(index);
    }
}

impl Reveal for ListState {
    fn reveal(&self, index: usize) {
        self.scroll_to_reveal_item(index);
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use gpui_kit::{
        Context, FocusHandle, IntoElement, ParentElement as _, Render,
        StatefulInteractiveElement as _, Styled as _, TestAppContext, div, px,
    };

    use super::*;

    const ORDER: [u8; 4] = [10, 20, 30, 40];

    #[test]
    fn moves_stop_at_the_ends() {
        assert_eq!(target(4, Some(0), Move::UP), Some(0));
        assert_eq!(target(4, Some(3), Move::DOWN), Some(3));
        assert_eq!(target(4, Some(1), Move::By(10)), Some(3));
        assert_eq!(target(4, Some(2), Move::By(-10)), Some(0));
        assert_eq!(target(4, Some(1), Move::Extend(1)), Some(2));
        assert_eq!(target(4, Some(2), Move::First), Some(0));
        assert_eq!(target(4, Some(1), Move::Last), Some(3));
    }

    #[test]
    fn with_no_row_down_and_home_start_at_the_top_and_up_and_end_at_the_bottom() {
        assert_eq!(target(4, None, Move::DOWN), Some(0));
        assert_eq!(target(4, None, Move::First), Some(0));
        assert_eq!(target(4, None, Move::UP), Some(3));
        assert_eq!(target(4, None, Move::Last), Some(3));
        assert_eq!(target(4, None, Move::Extend(-1)), Some(3));
    }

    #[test]
    fn an_empty_list_reaches_nothing() {
        for to in [Move::UP, Move::DOWN, Move::First, Move::Last] {
            assert_eq!(target(0, None, to), None);
            assert_eq!(target(0, Some(2), to), None);
            assert_eq!(step::<u8>(&[], Some(&10), to), None);
        }
    }

    #[test]
    fn a_current_row_past_the_end_counts_from_the_last() {
        // A list that shrank under a remembered index.
        assert_eq!(target(4, Some(9), Move::UP), Some(2));
        assert_eq!(target(4, Some(9), Move::DOWN), Some(3));
    }

    #[test]
    fn steps_by_id() {
        assert_eq!(step(&ORDER, Some(&20), Move::DOWN), Some(30));
        assert_eq!(step(&ORDER, Some(&20), Move::UP), Some(10));
        assert_eq!(step(&ORDER, None, Move::DOWN), Some(10));
        assert_eq!(step(&ORDER, None, Move::UP), Some(40));
        assert_eq!(position(&ORDER, Some(&30)), Some(2));
        assert_eq!(position(&ORDER, None), None);
    }

    #[test]
    fn the_selection_keeps_its_id_across_a_reload() {
        // 30 moved to the top: ↓ goes on from where it is now.
        let reloaded = [30, 10, 20, 40];
        assert_eq!(step(&reloaded, Some(&30), Move::DOWN), Some(10));
        // 20 has gone: the move starts again from an end.
        let shrunk = [10, 30, 40];
        assert_eq!(step(&shrunk, Some(&20), Move::DOWN), Some(10));
        assert_eq!(step(&shrunk, Some(&20), Move::UP), Some(40));
    }

    #[test]
    fn keys_list_their_moves() {
        let keys = Keys::new("List");
        assert_eq!(keys.moves(), [("up", Move::UP), ("down", Move::DOWN)]);
        let all = keys.with_ends().with_extend();
        assert_eq!(
            all.moves(),
            [
                ("up", Move::UP),
                ("down", Move::DOWN),
                ("shift-up", Move::Extend(-1)),
                ("shift-down", Move::Extend(1)),
                ("home", Move::First),
                ("end", Move::Last),
            ]
        );
        assert!(Move::Extend(1).extends() && !Move::DOWN.extends());
    }

    const CONTEXT: &str = "TestList";
    const KEYS: Keys = Keys::new(CONTEXT).with_ends();
    const ROWS: usize = 20;
    const ROW: f32 = 10.;

    /// A focused list of 20 10px rows in a 50px box, moved with its keys.
    struct TestList {
        focus: FocusHandle,
        scroll: ScrollHandle,
        current: Option<usize>,
        moves: Rc<RefCell<Vec<Move>>>,
    }

    impl Render for TestList {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .id("list")
                .key_context(CONTEXT)
                .track_focus(&self.focus)
                .track_scroll(&self.scroll)
                .overflow_y_scroll()
                .h(px(ROW * 5.))
                .flex()
                .flex_col()
                .on_list_move(
                    &KEYS,
                    cx.listener(|this, to: &Move, _, cx| {
                        this.moves.borrow_mut().push(*to);
                        this.current = target(ROWS, this.current, *to);
                        if let Some(index) = this.current {
                            this.scroll.reveal(index);
                        }
                        cx.notify();
                    }),
                )
                .children((0..ROWS).map(|_| div().flex_none().h(px(ROW))))
        }
    }

    #[gpui_kit::test]
    fn the_keys_move_the_list_and_scroll_the_row_into_view(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            KEYS.bind(cx);
        });
        let moves = Rc::new(RefCell::new(Vec::new()));
        let (view, cx) = cx.add_window_view(|_, cx| TestList {
            focus: cx.focus_handle(),
            scroll: ScrollHandle::new(),
            current: None,
            moves: moves.clone(),
        });
        let focus = view.read_with(cx, |view, _| view.focus.clone());
        cx.update(|window, cx| {
            focus.focus(window, cx);
            window.draw(cx).clear(cx);
        });

        cx.simulate_keystrokes("down down up end");
        assert_eq!(
            *moves.borrow(),
            [Move::DOWN, Move::DOWN, Move::UP, Move::Last]
        );
        cx.update(|window, cx| window.draw(cx).clear(cx));
        view.read_with(cx, |view, _| {
            assert_eq!(view.current, Some(ROWS - 1));
            // The last row's bottom is at the box's bottom.
            assert_eq!(view.scroll.offset().y, px(-(ROW * ROWS as f32 - ROW * 5.)));
        });

        // ⇧↓ isn't bound here, so it moves nothing.
        cx.simulate_keystrokes("shift-down home");
        assert_eq!(moves.borrow().last(), Some(&Move::First));
        assert!(!moves.borrow().contains(&Move::Extend(1)));
        cx.update(|window, cx| window.draw(cx).clear(cx));
        view.read_with(cx, |view, _| {
            assert_eq!(view.current, Some(0));
            assert_eq!(view.scroll.offset().y, px(0.));
        });
    }
}
