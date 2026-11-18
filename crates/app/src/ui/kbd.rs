//! Keyboard shortcut keys (`docs/DESIGN.md` → "Components" → **Kbd**).
//!
//! Geist Mono has neither ⌘ (U+2318) nor ⌥ (U+2325), and a missing glyph
//! comes from whatever fallback font the platform picks (`DESIGN.md` →
//! "Rendering findings"), so [`Kbd`] sets both in Inter, which has them, and
//! the rest in Geist Mono.

use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::{
    App, IntoElement, ParentElement as _, RenderOnce, Rgba, SharedString, Styled as _, Window, div,
};

use crate::theme::tokens::typography::TextRole;
use crate::theme::tokens::{color, font, radius, size, spacing, typography};
use crate::ui::faded;
use crate::ui::text::TextRoleExt as _;

/// The command key, which Geist Mono doesn't have.
const COMMAND: char = '\u{2318}';
/// The option key, which Geist Mono doesn't have either.
const OPTION: char = '\u{2325}';

/// Whether `ch` is set in Inter rather than Geist Mono.
fn in_ui_font(ch: char) -> bool {
    matches!(ch, COMMAND | OPTION)
}

/// How big a [`Kbd`] is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum KbdSize {
    /// 18px, mono 11/14: on its own or in a field (the command bar's ⌘K).
    #[default]
    Standard,
    /// 16px, mono 10/12: inside a 24px button (Import's ⌘I).
    Small,
}

impl KbdSize {
    fn height(self) -> gpui_kit::Pixels {
        match self {
            Self::Standard => size::KBD,
            Self::Small => size::CONTROL_XS,
        }
    }

    fn text(self) -> TextRole {
        match self {
            Self::Standard => typography::MONO_VALUE,
            Self::Small => typography::MONO_MICRO,
        }
    }
}

/// A shortcut ("⌘K"): `hover` fill, radius 4, 4px inline padding,
/// `text-secondary` keys.
///
/// Presentation only: the binding lives in the keymap, and the shortcut is
/// shown where its command is.
#[derive(IntoElement)]
pub struct Kbd {
    keys: SharedString,
    size: KbdSize,
    /// The surface a disabled control around the key cap fades over.
    disabled_surface: Option<Rgba>,
}

impl Kbd {
    /// A standard-size key cap showing `keys` as written ("⌘K").
    pub fn new(keys: impl Into<SharedString>) -> Self {
        Self {
            keys: keys.into(),
            size: KbdSize::Standard,
            disabled_surface: None,
        }
    }

    pub fn with_size(mut self, size: KbdSize) -> Self {
        self.size = size;
        self
    }

    /// Draws the key cap as part of a disabled control that fades as one
    /// group over the opaque `surface` ([`faded`]), for a shortcut inside a
    /// button drawn that way.
    pub fn with_disabled_surface(mut self, surface: Rgba) -> Self {
        self.disabled_surface = Some(surface);
        self
    }

    pub fn keys(&self) -> &SharedString {
        &self.keys
    }
}

/// `keys` split where the font changes: runs of ⌘ and ⌥ and runs of
/// anything else, each with whether it's set in Inter.
fn runs(keys: &str) -> Vec<(bool, &str)> {
    let mut runs = Vec::new();
    let mut start = 0;
    let mut ui = None;
    for (index, ch) in keys.char_indices() {
        let is_ui = in_ui_font(ch);
        if ui.is_some_and(|current| current != is_ui) {
            runs.push((ui.unwrap_or_default(), &keys[start..index]));
            start = index;
        }
        ui = Some(is_ui);
    }
    if let Some(ui) = ui {
        runs.push((ui, &keys[start..]));
    }
    runs
}

impl RenderOnce for Kbd {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let role = self.size.text();
        let paint = |color| {
            self.disabled_surface
                .map_or(color, |surface| faded(color, surface))
        };
        div()
            .flex()
            .flex_none()
            .items_center()
            .h(self.size.height())
            .px(spacing::S1)
            .rounded(radius::SM)
            .bg(paint(color::HOVER))
            .text_role(role)
            .text_color(paint(color::TEXT_SECONDARY))
            .children(runs(&self.keys).into_iter().map(|(ui, text)| {
                div()
                    .when(ui, |this| this.font_family(font::UI))
                    .child(SharedString::from(text.to_owned()))
            }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_is_split_from_the_mono_keys() {
        assert_eq!(runs("⌘K"), vec![(true, "⌘"), (false, "K")]);
        assert_eq!(runs("⇧⌘P"), vec![(false, "⇧"), (true, "⌘"), (false, "P")]);
        assert_eq!(runs("Esc"), vec![(false, "Esc")]);
        assert!(runs("").is_empty());
    }

    #[test]
    fn option_is_split_from_the_mono_keys_like_command() {
        assert_eq!(runs("⌥↑↓"), vec![(true, "⌥"), (false, "↑↓")]);
        assert_eq!(runs("⌥⌘K"), vec![(true, "⌥⌘"), (false, "K")]);
        assert_eq!(runs("⇧⌥P"), vec![(false, "⇧"), (true, "⌥"), (false, "P")]);
    }
}
