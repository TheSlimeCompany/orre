//! W1-3: a build without `dev-samples` (the release build, and
//! `cargo test` without features) has no sample data, no `--showcase` and
//! no tab or shortcut for an unbuilt screen. Integration tests link the
//! library as that build compiles it, not under `cfg(test)`, so these see
//! what a release `orre` ships. With `--all-features` they check the
//! inverse: a `dev-samples` build keeps all three.

use std::process::Command;

use orre_app::shell::window_state::restored_view;
use orre_app::shell::{View, tab_shortcuts};
use orre_core::setting::{WindowMode, WindowState};

const UNBUILT: [View; 4] = [View::Trends, View::Budgets, View::NetWorth, View::Reviews];

fn samples() -> bool {
    cfg!(feature = "dev-samples")
}

#[test]
fn the_tabs_are_the_shipped_screens() {
    if samples() {
        assert_eq!(View::TABS.to_vec(), View::ALL_TABS.to_vec());
        return;
    }
    assert_eq!(View::TABS.to_vec(), View::SHIPPED_TABS.to_vec());
    for view in UNBUILT {
        assert!(!View::TABS.contains(&view), "{view:?} has a tab");
        assert!(!view.is_shown(), "{view:?} is shown");
    }
}

#[test]
fn shortcuts_are_numbered_without_holes_and_skip_unbuilt_screens() {
    let shortcuts = tab_shortcuts(&View::TABS);
    for (index, (keys, view)) in shortcuts.iter().enumerate() {
        assert_eq!(*keys, format!("secondary-{}", index + 1));
        assert!(samples() || !UNBUILT.contains(view), "{view:?} has {keys}");
    }
    if !samples() {
        let views: Vec<_> = shortcuts.into_iter().map(|(_, view)| view).collect();
        assert_eq!(
            views,
            [View::Overview, View::Transactions, View::Inbox, View::Rules]
        );
    }
}

/// A window saved on an unbuilt view (by a dev build) reopens on the
/// default; the keys still parse, so the saved state isn't refused.
#[test]
fn a_saved_unbuilt_view_reopens_on_the_default() {
    for view in UNBUILT {
        let saved = WindowState {
            mode: WindowMode::Windowed,
            x: 0,
            y: 0,
            width: 1440,
            height: 900,
            view: Some(view.key().to_owned()),
        };
        assert_eq!(View::from_key(view.key()), Some(view));
        let expected = if samples() { view } else { View::default() };
        assert_eq!(restored_view(Some(&saved)), expected, "{view:?}");
    }
}

fn orre(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_orre"))
        .args(args)
        // Never the real data dir, though neither command opens it.
        .env(
            "ORRE_DATA_DIR",
            std::env::temp_dir().join("orre-release-gating"),
        )
        .output()
        .expect("orre runs")
}

#[test]
fn showcase_is_only_in_a_dev_samples_build() {
    let help = orre(&["--help"]);
    assert!(help.status.success());
    let help = String::from_utf8_lossy(&help.stdout);
    assert_eq!(help.contains("--showcase"), samples(), "{help}");

    if !samples() {
        let showcase = orre(&["--showcase"]);
        assert!(!showcase.status.success(), "--showcase parsed");
        let stderr = String::from_utf8_lossy(&showcase.stderr);
        assert!(stderr.contains("--showcase"), "{stderr}");
    }
}

/// The parser itself, as the release binary builds it.
#[test]
fn the_parser_refuses_showcase_without_dev_samples() {
    use clap::Parser as _;
    let parsed = orre_app::cli::Cli::try_parse_from(["orre", "--showcase"]);
    assert_eq!(parsed.is_ok(), samples(), "{parsed:?}");
}
