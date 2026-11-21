//! One line from `orre dev`, parsed.

use std::fmt;
use std::str::FromStr;

use crate::shell::View;

/// What `orre dev` asks the driver to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// The window as plain data.
    Describe,
    /// Open a view, as clicking its tab does.
    Show(View),
    /// Keystrokes, in order, as the tests write them ("down", "secondary-i").
    Press(Vec<String>),
    /// Text into the focused field.
    Type(String),
    /// Click the element with this id.
    Click(String),
    /// Click the visible, enabled control with this accessible label.
    Run(String),
}

/// What's wrong with a line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unparsed(String);

impl fmt::Display for Unparsed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The usage line, for errors.
pub const USAGE: &str = "commands: describe | show <view> | press <keys…> | type <text> | \
                         click <element id> | run <label>";

impl FromStr for Command {
    type Err = Unparsed;

    fn from_str(line: &str) -> Result<Self, Self::Err> {
        let line = line.trim();
        let (verb, rest) = line.split_once(' ').unwrap_or((line, ""));
        let rest = rest.trim();
        let needs = |what: &str| Unparsed(format!("`{verb}` needs {what}; {USAGE}"));
        match verb {
            "describe" if rest.is_empty() => Ok(Command::Describe),
            "describe" => Err(Unparsed(format!("`describe` takes nothing; {USAGE}"))),
            "show" if rest.is_empty() => Err(needs("a view")),
            "show" => View::from_key(rest).map(Command::Show).ok_or_else(|| {
                let views: Vec<_> = View::ALL.iter().map(|view| view.key()).collect();
                Unparsed(format!(
                    "no view `{rest}`; the views are {}",
                    views.join(", ")
                ))
            }),
            "press" if rest.is_empty() => Err(needs("keys")),
            "press" => Ok(Command::Press(
                rest.split_whitespace().map(str::to_owned).collect(),
            )),
            "type" if rest.is_empty() => Err(needs("text")),
            "type" => Ok(Command::Type(rest.to_owned())),
            "click" if rest.is_empty() => Err(needs("an element id")),
            "click" => Ok(Command::Click(rest.to_owned())),
            "run" if rest.is_empty() => Err(needs("a label")),
            "run" => Ok(Command::Run(rest.to_owned())),
            "" => Err(Unparsed(USAGE.to_owned())),
            _ => Err(Unparsed(format!("no command `{verb}`; {USAGE}"))),
        }
    }
}
