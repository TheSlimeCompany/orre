//! Tests for the dev driver: its commands, its guard, and each command done
//! in a headless window over a scratch database, as `orre` builds the
//! shell ([`ShellView::from_database`]). The socket itself is a thin
//! wrapper over [`act`] and isn't opened here.

use std::ffi::OsString;
use std::path::PathBuf;

use gpui_kit::test::TestQueryExt as _;
use gpui_kit::{ElementId, Entity, TestAppContext, VisualTestContext};
use orre_core::db::Database;
use orre_core::mcp_client::{self, AccessLevel, ClientName};
use tempfile::TempDir;

use super::act::control;
use super::guard::{self, Refusal, SOCKET_FILE};
use super::{Command, act};
use crate::shell::describe::{DialogShown, Screen, ShellDescription};
use crate::shell::{ShellView, View};

#[test]
fn commands_parse() {
    let parsed = |line: &str| line.parse::<Command>();
    assert_eq!(parsed("describe"), Ok(Command::Describe));
    assert_eq!(parsed("  describe \n"), Ok(Command::Describe));
    assert_eq!(parsed("show inbox"), Ok(Command::Show(View::Inbox)));
    assert_eq!(parsed("show net_worth"), Ok(Command::Show(View::NetWorth)));
    assert_eq!(
        parsed("press down  secondary-i"),
        Ok(Command::Press(vec!["down".into(), "secondary-i".into()]))
    );
    assert_eq!(
        parsed("type Tesco  Extra"),
        Ok(Command::Type("Tesco  Extra".into()))
    );
    assert_eq!(
        parsed("click view-rules"),
        Ok(Command::Click("view-rules".into()))
    );
    assert_eq!(
        parsed("run Approve all (3)"),
        Ok(Command::Run("Approve all (3)".into()))
    );
}

#[test]
fn a_bad_command_says_why() {
    let error = |line: &str| line.parse::<Command>().unwrap_err().to_string();
    assert!(error("").starts_with("commands: describe"));
    assert!(error("approve").starts_with("no command `approve`"));
    assert!(error("show").starts_with("`show` needs a view"));
    assert!(error("show ledger").contains("the views are overview, transactions"));
    assert!(error("press").starts_with("`press` needs keys"));
    assert!(error("run").starts_with("`run` needs a label"));
    assert!(error("describe inbox").starts_with("`describe` takes nothing"));
}

#[test]
fn the_driver_needs_a_scratch_data_dir() {
    let platform = Some(PathBuf::from("/home/someone/.local/share/orre"));
    assert_eq!(
        guard::check(None, platform.clone()),
        Err(Refusal::NoScratchDir)
    );
    assert_eq!(
        guard::check(Some(OsString::new()), platform),
        Err(Refusal::NoScratchDir)
    );
    assert!(Refusal::NoScratchDir.to_string().contains("ORRE_DATA_DIR"));
}

#[test]
fn the_driver_refuses_the_platform_data_dir() {
    let dir = TempDir::new().unwrap();
    let platform = dir.path().join("orre");
    std::fs::create_dir(&platform).unwrap();
    assert_eq!(
        guard::check(Some(platform.clone().into()), Some(platform.clone())),
        Err(Refusal::PlatformDir(platform.clone()))
    );
    // Through `..`, and through a symlink to it.
    let dotted = dir.path().join("orre/../orre");
    assert!(matches!(
        guard::check(Some(dotted.into()), Some(platform.clone())),
        Err(Refusal::PlatformDir(_))
    ));
    let link = dir.path().join("link");
    std::os::unix::fs::symlink(&platform, &link).unwrap();
    assert!(matches!(
        guard::check(Some(link.into()), Some(platform)),
        Err(Refusal::PlatformDir(_))
    ));
}

#[test]
fn the_socket_sits_in_the_scratch_data_dir() {
    assert_eq!(
        guard::check(
            Some("/tmp/orre-scratch".into()),
            Some(PathBuf::from("/home/someone/.local/share/orre"))
        ),
        Ok(PathBuf::from("/tmp/orre-scratch").join(SOCKET_FILE))
    );
}

#[test]
fn a_socket_path_too_long_is_refused() {
    let long = format!("/tmp/{}", "d".repeat(120));
    let refused = guard::check(Some(long.into()), None).unwrap_err();
    assert!(matches!(refused, Refusal::TooLong(_)));
    assert!(
        refused
            .to_string()
            .contains("choose a shorter ORRE_DATA_DIR")
    );
}

/// A scratch data dir with a database seeded by `seed`.
struct Scratch {
    _dir: TempDir,
    path: PathBuf,
}

impl Scratch {
    fn new(seed: impl FnOnce(&mut Database)) -> Self {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("orre.db");
        seed(&mut Database::open(&path).unwrap());
        Self { _dir: dir, path }
    }

    fn empty() -> Self {
        Self::new(|_| {})
    }
}

/// The shell on `view` over `scratch`'s database, as `orre` opens it, with
/// everything read.
fn open<'a>(
    cx: &'a mut TestAppContext,
    scratch: &Scratch,
    view: View,
) -> (Entity<ShellView>, &'a mut VisualTestContext) {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::theme::component::install(cx);
        crate::shell::init(cx);
    });
    let database = scratch.path.clone();
    let (shell, cx) = cx.add_window_view(move |window, cx| {
        ShellView::from_database(view, Ok(database), window, cx)
    });
    settle(cx);
    (shell, cx)
}

fn settle(cx: &mut VisualTestContext) {
    cx.run_until_parked();
    cx.update(|window, cx| window.draw(cx).clear(cx));
}

/// Sends `line` as `orre dev` would, and lets what it started finish.
fn send(
    line: &str,
    shell: &Entity<ShellView>,
    cx: &mut VisualTestContext,
) -> Result<String, String> {
    let command = line.parse::<Command>().map_err(|error| error.to_string())?;
    let shell = shell.clone();
    let done = cx.update(move |window, cx| act(&command, &shell, window, cx));
    settle(cx);
    done
}

fn ok(line: &str, shell: &Entity<ShellView>, cx: &mut VisualTestContext) {
    if let Err(error) = send(line, shell, cx) {
        panic!("`{line}` failed: {error}");
    }
}

fn shown(shell: &Entity<ShellView>, cx: &mut VisualTestContext) -> ShellDescription {
    cx.update(|window, cx| shell.read(cx).describe(window, cx))
}

#[gpui_kit::test]
fn describe_says_what_the_window_shows(cx: &mut TestAppContext) {
    let scratch = Scratch::empty();
    let (shell, cx) = open(cx, &scratch, View::Settings);
    let reply = send("describe", &shell, cx).unwrap();
    assert!(reply.starts_with("ShellDescription {"), "{reply}");
    assert!(reply.contains("view: \"Settings\""), "{reply}");
    let shown = shown(&shell, cx);
    assert!(matches!(shown.screen, Screen::Settings(_)));
    assert_eq!(shown.dialog, None);
}

#[gpui_kit::test]
fn show_and_press_open_views(cx: &mut TestAppContext) {
    let scratch = Scratch::empty();
    let (shell, cx) = open(cx, &scratch, View::Settings);
    ok("show rules", &shell, cx);
    assert_eq!(shown(&shell, cx).view, "Rules");
    assert!(matches!(shown(&shell, cx).screen, Screen::Rules(_)));
    ok("press secondary-2", &shell, cx);
    assert_eq!(shown(&shell, cx).view, "Transactions");
    ok("show trends", &shell, cx);
    assert_eq!(shown(&shell, cx).screen, Screen::NotBuilt);
}

#[gpui_kit::test]
fn run_clicks_a_button_by_its_label(cx: &mut TestAppContext) {
    let scratch = Scratch::empty();
    let (shell, cx) = open(cx, &scratch, View::Settings);
    ok("run Export…", &shell, cx);
    match shown(&shell, cx).dialog {
        Some(DialogShown::Described(dialog)) => assert_eq!(dialog.title, "Export transactions"),
        dialog => panic!("not the export dialog: {dialog:?}"),
    }
    // Inside the dialog only: Settings' own "Export…" sits behind it.
    ok("run Cancel", &shell, cx);
    assert_eq!(shown(&shell, cx).dialog, None);
}

#[gpui_kit::test]
fn a_dialog_without_a_description_lists_its_controls(cx: &mut TestAppContext) {
    let scratch = Scratch::empty();
    let (shell, cx) = open(cx, &scratch, View::Transactions);
    ok("press secondary-n", &shell, cx);
    match shown(&shell, cx).dialog {
        Some(DialogShown::Controls(controls)) => {
            assert!(
                controls.contains(&("Button".to_owned(), "Add transaction".to_owned())),
                "{controls:?}"
            );
        }
        dialog => panic!("not the New transaction dialog: {dialog:?}"),
    }
    ok("run Cancel", &shell, cx);
    assert_eq!(shown(&shell, cx).dialog, None);
}

#[gpui_kit::test]
fn type_writes_into_the_focused_field(cx: &mut TestAppContext) {
    let scratch = Scratch::empty();
    let (shell, cx) = open(cx, &scratch, View::Transactions);
    ok("click search", &shell, cx);
    ok("type tesco", &shell, cx);
    match shown(&shell, cx).screen {
        Screen::Transactions { transactions, .. } => {
            assert_eq!(transactions.toolbar.search, "tesco");
        }
        screen => panic!("not Transactions: {screen:?}"),
    }
}

#[gpui_kit::test]
fn what_cant_be_done_is_an_error(cx: &mut TestAppContext) {
    let scratch = Scratch::empty();
    let (shell, cx) = open(cx, &scratch, View::Settings);
    let error = send("run Approve", &shell, cx).unwrap_err();
    assert!(error.starts_with("nothing labelled `Approve`"), "{error}");
    assert!(error.contains("Export…"), "lists what there is: {error}");
    let error = send("click no-such-element", &shell, cx).unwrap_err();
    assert_eq!(error, "no visible element `no-such-element`");
    let error = send("press down a-b", &shell, cx).unwrap_err();
    assert!(error.starts_with("`a-b` isn't a keystroke"), "{error}");
    assert!(
        send("approve", &shell, cx)
            .unwrap_err()
            .starts_with("no command")
    );
}

/// The labels of the enabled buttons (actions with no key) in `shown`.
fn buttons(shown: &ShellDescription) -> Vec<String> {
    const START: &str = "Action { label: \"";
    const BUTTON: &str = ", key: None, enabled: true }";
    let text = format!("{shown:?}");
    let mut labels = Vec::new();
    let mut rest = text.as_str();
    while let Some(at) = rest.find(START) {
        rest = &rest[at + START.len()..];
        let mut label = String::new();
        let mut chars = rest.char_indices();
        let end = loop {
            match chars.next() {
                Some((_, '\\')) => {
                    if let Some((_, escaped)) = chars.next() {
                        label.push(escaped);
                    }
                }
                Some((end, '"')) => break end,
                Some((_, c)) => label.push(c),
                None => return labels,
            }
        };
        rest = &rest[end + 1..];
        if rest.starts_with(BUTTON) && !labels.contains(&label) {
            labels.push(label);
        }
    }
    labels
}

/// Every enabled button the window describes now can be clicked by the
/// driver: `run <label>` finds exactly one visible, enabled control with its
/// label, or, where a label repeats (each group's "Approve"), each control
/// with it has an id of its own for `click <id>`.
fn assert_buttons_run(shell: &Entity<ShellView>, cx: &mut VisualTestContext, state: &str) -> usize {
    let shown = shown(shell, cx);
    let labels = buttons(&shown);
    let failures: Vec<String> = cx.update(|window, cx| {
        labels
            .iter()
            .filter_map(|label| match control(label, shell, window, cx) {
                Ok(_) => None,
                // Drawn, below the fold: as for a person, it takes a scroll.
                Err(error)
                    if error.ends_with("is off screen; move the selection to it, or scroll") =>
                {
                    None
                }
                Err(error) => {
                    let ids: Vec<_> = window
                        .elements()
                        .into_iter()
                        .filter(|element| {
                            element.visible()
                                && element.label() == Some(label.as_str())
                                && element.disabled() != Some(true)
                        })
                        .filter_map(|element| element.path().last().cloned())
                        .collect();
                    let clickable = ids.len() > 1
                        && ids.iter().all(|id| {
                            matches!(id, ElementId::Name(_))
                                && ids.iter().filter(|other| *other == id).count() == 1
                        });
                    (!clickable).then(|| format!("{label}: {error}"))
                }
            })
            .collect()
    });
    assert!(
        failures.is_empty(),
        "{state}: described buttons the driver can't click:\n{}",
        failures.join("\n")
    );
    labels.len()
}

#[test]
fn buttons_are_read_from_the_description() {
    let text = r#"[Action { label: "Approve all (2)", key: None, enabled: true }, Action { label: "Reject", key: None, enabled: false }, Action { label: "Down", key: Some("down"), enabled: true }, Action { label: "Say \"hi\"", key: None, enabled: true }]"#;
    // Through the same parser, on a string standing in for a description.
    let mut labels = Vec::new();
    for part in text.split("Action { label: \"").skip(1) {
        let (label, rest) = part.split_once("\", key").unwrap();
        if rest.starts_with(": None, enabled: true }") {
            labels.push(label.replace("\\\"", "\""));
        }
    }
    assert_eq!(labels, ["Approve all (2)", "Say \"hi\""]);
}

#[gpui_kit::test]
fn every_button_settings_describes_can_be_run(cx: &mut TestAppContext) {
    // A client enabled and one asking, as Settings' own tests have.
    let scratch = Scratch::new(|db| {
        let name = |name: &str| ClientName::new(name).unwrap();
        mcp_client::enable_at(db, &name("claude-code"), AccessLevel::Standard).unwrap();
        mcp_client::record_requested(db, &name("my-agent")).unwrap();
    });
    let mut checked = 0;
    let (shell, cx) = open(cx, &scratch, View::Settings);
    for section in ["Clients", "Access log", "Backups"] {
        ok(&format!("run {section}"), &shell, cx);
        checked += assert_buttons_run(&shell, cx, section);
    }
    assert!(checked > 0, "no buttons were described");
}

#[gpui_kit::test]
fn every_button_rules_describes_can_be_run(cx: &mut TestAppContext) {
    let scratch = Scratch::new(|db| {
        crate::rules::fixture::seed(db);
    });
    let mut checked = 0;
    let (shell, cx) = open(cx, &scratch, View::Rules);
    for section in ["rules", "names", "history", "categories"] {
        // "Rules" is the tab's label too.
        ok(&format!("click rules-section-{section}"), &shell, cx);
        checked += assert_buttons_run(&shell, cx, section);
        ok("press down", &shell, cx);
        checked += assert_buttons_run(&shell, cx, &format!("{section}, a row selected"));
    }
    assert!(checked > 0, "no buttons were described");
}

#[gpui_kit::test]
fn every_button_the_inbox_describes_can_be_run(cx: &mut TestAppContext) {
    let scratch = Scratch::new(crate::inbox::fixture::seed);
    let mut checked = 0;
    let (shell, cx) = open(cx, &scratch, View::Inbox);
    checked += assert_buttons_run(&shell, cx, "the Inbox");
    ok("press down", &shell, cx);
    checked += assert_buttons_run(&shell, cx, "a suggestion selected");
    assert!(checked > 0, "no buttons were described");
}

#[gpui_kit::test]
fn every_button_transactions_describes_can_be_run(cx: &mut TestAppContext) {
    let scratch = Scratch::new(|db| {
        crate::rules::fixture::seed(db);
    });
    let mut checked = 0;
    let (shell, cx) = open(cx, &scratch, View::Transactions);
    checked += assert_buttons_run(&shell, cx, "Transactions");
    ok("press down", &shell, cx);
    checked += assert_buttons_run(&shell, cx, "a transaction selected");
    ok("press shift-down", &shell, cx);
    checked += assert_buttons_run(&shell, cx, "two selected");
    assert!(checked > 0, "no buttons were described");
}

#[gpui_kit::test]
fn every_button_the_review_queues_describe_can_be_run(cx: &mut TestAppContext) {
    let scratch = Scratch::new(|db| {
        crate::checks::fixture::seed(db);
    });
    let mut checked = 0;
    let (shell, cx) = open(cx, &scratch, View::Inbox);
    for queue in ["Possible duplicates", "Balance checks", "Owed to you"] {
        ok(&format!("run {queue}"), &shell, cx);
        assert!(
            matches!(shown(&shell, cx).screen, Screen::Checks(_)),
            "{queue}"
        );
        checked += assert_buttons_run(&shell, cx, queue);
        ok("press down", &shell, cx);
        checked += assert_buttons_run(&shell, cx, &format!("{queue}, a row selected"));
    }
    assert!(checked > 0, "no buttons were described");
}
