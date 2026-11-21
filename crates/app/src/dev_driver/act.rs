//! Doing a [`Command`] in the window.
//!
//! Nothing here may panic on what the client sent: gpui-kit's test calls
//! panic on a bad keystroke or an element that isn't there, so each is
//! checked first and refused with an error.

use gpui_kit::test::ElementSnapshot;
use gpui_kit::test::{TestEventExt as _, TestQueryExt as _, TestWindowExt as _};
use gpui_kit::{App, ElementId, Entity, Keystroke, MouseButton, Window, point};

use crate::shell::ShellView;

use super::Command;

/// The reply to a command that did what it was asked.
const DONE: &str = "ok";

/// Does `command` in `window`, `shell`'s, and says what came of it: the
/// description for `describe`, else "ok", or why it couldn't.
pub fn act(
    command: &Command,
    shell: &Entity<ShellView>,
    window: &mut Window,
    cx: &mut App,
) -> Result<String, String> {
    window.render_frame(cx);
    match command {
        Command::Describe => Ok(format!("{:#?}", shell.read(cx).describe(window, cx))),
        Command::Show(view) => click_id(view.element_id(), window, cx),
        Command::Press(keys) => {
            for key in keys {
                Keystroke::parse_with_os(key, cx.platform_info().os)
                    .map_err(|error| format!("`{key}` isn't a keystroke: {error}"))?;
            }
            for key in keys {
                window.press(key, cx);
            }
            Ok(DONE.to_owned())
        }
        Command::Type(text) => {
            if window.focused(cx).is_none() {
                return Err("nothing has keyboard focus to type into".to_owned());
            }
            window.input(text, cx);
            window.render_frame(cx);
            Ok(DONE.to_owned())
        }
        Command::Click(id) => click_id(id, window, cx),
        Command::Run(label) => run(label, shell, window, cx),
    }
}

/// Clicks the one visible element whose own id is `id`.
fn click_id(id: &str, window: &mut Window, cx: &mut App) -> Result<String, String> {
    let wanted = ElementId::from(gpui_kit::SharedString::from(id.to_owned()));
    let found: Vec<_> = window
        .elements()
        .into_iter()
        .filter(|element| element.visible() && element.path().last() == Some(&wanted))
        .collect();
    match found.as_slice() {
        [] => Err(format!("no visible element `{id}`")),
        [element] => {
            click_at(element.bounds().center(), window, cx);
            Ok(DONE.to_owned())
        }
        _ => Err(format!(
            "{} visible elements are `{id}`; use `run <label>` or `press`",
            found.len()
        )),
    }
}

/// Clicks the one visible, enabled control labelled `label` ([`control`]).
fn run(
    label: &str,
    shell: &Entity<ShellView>,
    window: &mut Window,
    cx: &mut App,
) -> Result<String, String> {
    let control = control(label, shell, window, cx)?;
    click_at(control.bounds().center(), window, cx);
    Ok(DONE.to_owned())
}

/// The one visible, enabled control labelled `label`: inside the open
/// dialog when there is one, since a dialog takes the clicks. `run` clicks
/// it; the tests check every described button has one.
pub(super) fn control(
    label: &str,
    shell: &Entity<ShellView>,
    window: &Window,
    cx: &App,
) -> Result<ElementSnapshot, String> {
    let dialog = shell.read(cx).open_dialog_id(cx).map(ElementId::View);
    let (controls, hidden): (Vec<_>, Vec<_>) = window
        .elements()
        .into_iter()
        .filter(|element| {
            dialog
                .as_ref()
                .is_none_or(|dialog| element.path().contains(dialog))
        })
        .partition(|element| element.visible());
    let matching: Vec<_> = controls
        .iter()
        .filter(|element| element.label() == Some(label))
        .collect();
    let mut enabled: Vec<_> = matching
        .iter()
        .filter(|element| element.disabled() != Some(true))
        .collect();
    let place = if dialog.is_some() {
        " in the dialog"
    } else {
        ""
    };
    match enabled.len() {
        1 => Ok((*enabled.remove(0)).clone()),
        0 if !matching.is_empty() => Err(format!("`{label}` is disabled")),
        0 if hidden.iter().any(|element| element.label() == Some(label)) => Err(format!(
            "`{label}` is off screen; move the selection to it, or scroll"
        )),
        0 => {
            let mut labels: Vec<_> = controls
                .iter()
                .filter(|element| element.disabled() != Some(true))
                .filter_map(|element| element.label())
                .collect();
            labels.dedup();
            Err(format!(
                "nothing labelled `{label}`{place}; the labelled controls are: {}",
                labels.join(", ")
            ))
        }
        several => Err(format!(
            "{several} controls are labelled `{label}`{place}; select a row first, or use \
             `click <id>`: {}",
            enabled
                .iter()
                .filter_map(|element| element.path().last())
                .map(|id| format!("{id}"))
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

/// A left click at `position`, as a person's: move, press, release.
fn click_at(position: gpui_kit::Point<gpui_kit::Pixels>, window: &mut Window, cx: &mut App) {
    let position = point(position.x, position.y);
    window.pointer_move(position, None, cx);
    window.pointer_down(position, MouseButton::Left, cx);
    window.pointer_up(position, MouseButton::Left, cx);
}
