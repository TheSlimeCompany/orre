//! A keyboard cursor for a left panel whose rows are mostly filters
//! (`docs/DESIGN.md` → "Keyboard paths"; ROADMAP U.9): the Rules panel, and
//! the Inbox's.
//!
//! The panel is a Tab stop, like the Accounts and Settings panels. There ↑↓
//! move a keyboard cursor through every row that does something, across
//! sections (a section label isn't a row, so it's skipped), and Enter or
//! Space act on the cursor row as a click does: open a section or a queue,
//! turn a filter on or off. Unlike the Accounts panel, ↑↓ don't change the
//! selection, because most of these rows are filters; the open row keeps
//! its `molten-soft` fill while the cursor is elsewhere.
//!
//! - [`PanelKeys`] binds the keys in the panel's key context: ↑↓ through
//!   [`list::Keys`], Enter and Space to [`Confirm`].
//! - [`PanelCursor`] is where the cursor is, with the panel's focus handle.
//!   [`PanelCursor::keyed`] keeps one with the panel's element, for a panel
//!   drawn by a function with no view of its own to keep it in.
//! - [`PanelCursorExt::panel_cursor`] puts the key context, the focus and
//!   the handlers on the panel's element.
//! - [`Frame::on`] says which row draws the cursor in this frame: that row
//!   takes [`ListRow::current`] and [`ListRow::focus_visible`]
//!   ([`Frame::mark`]).
//!
//! The panel keeps its rows (any `Clone + PartialEq` value naming what a
//! row does), their order, the row the cursor starts on (its "home": the
//! open row) and what acting on a row does. Movement is [`list::step`]'s,
//! so it stops at the ends like every list.
//!
//! [`ListRow::current`]: crate::ui::list_row::ListRow::current
//! [`ListRow::focus_visible`]: crate::ui::list_row::ListRow::focus_visible

use std::rc::Rc;

use gpui_kit::base::actions::Confirm;
use gpui_kit::{App, ElementId, Entity, FocusHandle, InteractiveElement, KeyBinding, Window};

use crate::ui::list::{self, Keys, ListKeysExt as _, Move};
use crate::ui::list_row::ListRow;

/// The keys a panel with a cursor answers to in its key context: ↑↓ move
/// the cursor, Enter and Space act on its row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PanelKeys {
    keys: Keys,
}

impl PanelKeys {
    /// ↑↓, Enter and Space in `context`.
    pub const fn new(context: &'static str) -> Self {
        Self {
            keys: Keys::new(context),
        }
    }

    pub const fn context(&self) -> &'static str {
        self.keys.context()
    }

    /// The movement keys, as [`list`] binds them.
    pub const fn keys(&self) -> &Keys {
        &self.keys
    }

    /// Binds the keys in the panel's context. Call once, from the view's
    /// `init`.
    pub fn bind(&self, cx: &mut App) {
        self.keys.bind(cx);
        let context = Some(self.context());
        cx.bind_keys(
            ACT_KEYS.map(|key| KeyBinding::new(key, Confirm { secondary: false }, context)),
        );
    }
}

/// The keys that act on the cursor's row, as a click does: Enter and
/// Space.
pub const ACT_KEYS: [&str; 2] = ["enter", "space"];

/// Where a panel's keyboard cursor is, and the panel's focus (a Tab stop).
pub struct PanelCursor<T> {
    focus: FocusHandle,
    /// The row ↑↓ last reached; `None` until they're pressed.
    at: Option<T>,
}

impl<T: Clone + PartialEq + 'static> PanelCursor<T> {
    /// A cursor on no row yet, with a new focus handle that's a Tab stop.
    pub fn new(cx: &mut App) -> Self {
        Self {
            focus: cx.focus_handle().tab_stop(true),
            at: None,
        }
    }

    /// The cursor kept with the element drawing the panel: there for as
    /// long as the panel is drawn in consecutive frames, so it starts again
    /// when the panel is next shown. Moving it redraws the view drawing the
    /// panel. Call while drawing, with a `key` unique in that view.
    pub fn keyed(key: impl Into<ElementId>, window: &mut Window, cx: &mut App) -> Entity<Self> {
        window.use_keyed_state(key, cx, |_, cx| Self::new(cx))
    }

    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus
    }

    /// The row the cursor is on, of `rows`: the one ↑↓ last reached while
    /// it's still listed, else `home` (the open row) if that's listed.
    pub fn current(&self, rows: &[T], home: Option<&T>) -> Option<T> {
        [self.at.as_ref(), home]
            .into_iter()
            .flatten()
            .find(|row| rows.contains(row))
            .cloned()
    }

    /// Moves the cursor from [`Self::current`] as [`list::step`] says.
    pub fn step(&mut self, rows: &[T], home: Option<&T>, to: Move) {
        let from = self.current(rows, home);
        if let Some(row) = list::step(rows, from.as_ref(), to) {
            self.at = Some(row);
        }
    }

    /// The cursor as this frame draws it: on [`Self::current`] while the
    /// panel has keyboard focus (focused, and the last input was a key
    /// press), else on no row.
    pub fn frame(&self, rows: &[T], home: Option<&T>, window: &Window) -> Frame<T> {
        let shown = self.focus.is_focused(window) && window.last_input_was_keyboard();
        Frame {
            at: shown.then(|| self.current(rows, home)).flatten(),
        }
    }
}

/// Which row draws the cursor in one frame ([`PanelCursor::frame`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame<T> {
    at: Option<T>,
}

impl<T: PartialEq> Frame<T> {
    /// Whether `row` draws the cursor.
    pub fn on(&self, row: &T) -> bool {
        self.at.as_ref() == Some(row)
    }

    /// `list_row` drawn for `row`: the list row's current look (`hover`
    /// fill, `text` label) and the inset focus ring when the cursor is on
    /// it. A selected row keeps its own look under the ring.
    pub fn mark(&self, row: &T, list_row: ListRow) -> ListRow {
        let on = self.on(row);
        list_row.current(on).focus_visible(on)
    }
}

/// Puts a panel's cursor on the element drawing the panel.
pub trait PanelCursorExt: InteractiveElement + Sized {
    /// Gives the element `keys`' context and `cursor`'s focus (a Tab stop),
    /// moves the cursor through `rows` on ↑↓ (from `home`, the open row,
    /// until moved) and calls `act` with the cursor row on Enter or Space.
    /// The keys act only while the panel itself has focus, so Enter on a
    /// button inside it stays the button's.
    fn panel_cursor<T: Clone + PartialEq + 'static>(
        self,
        keys: &PanelKeys,
        cursor: &Entity<PanelCursor<T>>,
        rows: Vec<T>,
        home: Option<T>,
        act: impl Fn(&T, &mut Window, &mut App) + 'static,
        cx: &App,
    ) -> Self {
        let focus = cursor.read(cx).focus.clone();
        let rows = Rc::new((rows, home));
        let moved = {
            let (cursor, rows, focus) = (cursor.clone(), rows.clone(), focus.clone());
            move |to: &Move, window: &mut Window, cx: &mut App| {
                if !focus.is_focused(window) {
                    cx.propagate();
                    return;
                }
                cursor.update(cx, |cursor, cx| {
                    cursor.step(&rows.0, rows.1.as_ref(), *to);
                    cx.notify();
                });
            }
        };
        let confirmed = {
            let (cursor, focus) = (cursor.clone(), focus.clone());
            move |_: &Confirm, window: &mut Window, cx: &mut App| {
                if !focus.is_focused(window) {
                    cx.propagate();
                    return;
                }
                if let Some(row) = cursor.read(cx).current(&rows.0, rows.1.as_ref()) {
                    act(&row, window, cx);
                }
            }
        };
        self.key_context(keys.context())
            .track_focus(&focus)
            .on_list_move(keys.keys(), moved)
            .on_action(confirmed)
    }
}

impl<E: InteractiveElement> PanelCursorExt for E {}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{
        AppContext as _, Context, IntoElement, ParentElement as _, Render, Styled as _,
        TestAppContext, VisualTestContext, WindowOptions, div,
    };

    use super::*;
    use crate::theme;
    use crate::theme::focus::RING_SELECTOR;
    use crate::theme::tokens::size;
    use crate::ui::section_label::SectionLabelRow;

    const ROWS: [&str; 5] = ["a", "b", "c", "x", "y"];

    fn cursor(at: Option<&'static str>, cx: &mut TestAppContext) -> PanelCursor<&'static str> {
        let focus = cx.update(|cx| cx.focus_handle());
        PanelCursor { focus, at }
    }

    #[gpui_kit::test]
    fn the_cursor_starts_on_home_and_stops_at_the_ends(cx: &mut TestAppContext) {
        let mut on = cursor(None, cx);
        assert_eq!(on.current(&ROWS, Some(&"b")), Some("b"));
        assert_eq!(on.current(&ROWS, None), None);
        on.step(&ROWS, Some(&"b"), Move::DOWN);
        assert_eq!(on.current(&ROWS, Some(&"b")), Some("c"));
        on.step(&ROWS, Some(&"b"), Move::By(10));
        assert_eq!(on.current(&ROWS, Some(&"b")), Some("y"));
        // With no home and no cursor, ↓ reaches the first row.
        let mut fresh = cursor(None, cx);
        fresh.step(&ROWS, None, Move::DOWN);
        assert_eq!(fresh.at, Some("a"));
    }

    #[gpui_kit::test]
    fn a_row_that_has_gone_falls_back_to_home(cx: &mut TestAppContext) {
        // The filters changed under the cursor.
        let on = cursor(Some("y"), cx);
        assert_eq!(on.current(&["a", "b", "z"], Some(&"a")), Some("a"));
        assert_eq!(on.current(&["a", "b", "z"], Some(&"q")), None);
    }

    const CONTEXT: &str = "TestPanel";
    const KEYS: PanelKeys = PanelKeys::new(CONTEXT);

    /// Two sections, each a label and rows; acting on a row selects it.
    /// Inside a focusable region, where Tab starts.
    struct Panel {
        region: FocusHandle,
        selected: &'static str,
        acted: Rc<RefCell<Vec<&'static str>>>,
    }

    impl Render for Panel {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let cursor = PanelCursor::<&'static str>::keyed("cursor", window, cx);
            let home = Some(self.selected);
            let frame = cursor.read(cx).frame(&ROWS, home.as_ref(), window);
            let row = |id: &'static str| {
                frame.mark(&id, ListRow::new(id, id).selected(id == self.selected))
            };
            let view = cx.entity();
            let panel = div()
                .id("panel")
                .panel_cursor(
                    &KEYS,
                    &cursor,
                    ROWS.to_vec(),
                    home,
                    move |row, _, cx| {
                        let row = *row;
                        view.update(cx, |panel, cx| {
                            panel.acted.borrow_mut().push(row);
                            panel.selected = row;
                            cx.notify();
                        })
                    },
                    cx,
                )
                .flex()
                .flex_col()
                .w(size::PANEL)
                .child(SectionLabelRow::new("First"))
                .children(["a", "b", "c"].map(row))
                .child(SectionLabelRow::new("Second"))
                .children(["x", "y"].map(row));
            div().track_focus(&self.region).size_full().child(panel)
        }
    }

    /// Opens a real window with `Root` (which binds Tab) and the region
    /// focused.
    fn open(cx: &mut TestAppContext) -> (Entity<Panel>, VisualTestContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            theme::component::install(cx);
            KEYS.bind(cx);
        });
        let (window, view) = cx
            .update(|cx| {
                gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| {
                    cx.new(|cx| Panel {
                        region: cx.focus_handle(),
                        selected: "b",
                        acted: Rc::new(RefCell::new(Vec::new())),
                    })
                })
            })
            .expect("open test window");
        let mut cx = VisualTestContext::from_window(window, cx);
        cx.update(|window, cx| {
            let region = view.read(cx).region.clone();
            window.focus(&region, cx);
            window.draw(cx).clear(cx);
        });
        (view, cx)
    }

    fn press(keys: &str, cx: &mut VisualTestContext) {
        cx.simulate_keystrokes(keys);
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }

    /// The row drawing the ring, if any.
    fn ring_on(cx: &mut VisualTestContext) -> Option<&'static str> {
        let ring = cx.debug_bounds(RING_SELECTOR)?;
        ROWS.into_iter()
            .find(|id| cx.update(|window, _| window.find(*id).bounds()) == ring)
    }

    #[gpui_kit::test]
    fn tab_reaches_the_panel_and_the_keys_move_the_cursor_not_the_selection(
        cx: &mut TestAppContext,
    ) {
        let (view, mut cx) = open(cx);
        let cx = &mut cx;
        assert_eq!(ring_on(cx), None, "unfocused: no cursor");

        press("tab", cx);
        assert_eq!(ring_on(cx), Some("b"), "the cursor starts on the open row");
        // Down across the second section's label to its first row.
        press("down down", cx);
        assert_eq!(ring_on(cx), Some("x"));
        press("down down", cx);
        assert_eq!(ring_on(cx), Some("y"), "it stops at the end");
        assert_eq!(view.read_with(cx, |panel, _| panel.selected), "b");

        press("enter", cx);
        assert_eq!(view.read_with(cx, |panel, _| panel.selected), "y");
        press("up space", cx);
        let acted = view.read_with(cx, |panel, _| panel.acted.borrow().clone());
        assert_eq!(acted, ["y", "x"]);
        assert_eq!(ring_on(cx), Some("x"));
    }
}
