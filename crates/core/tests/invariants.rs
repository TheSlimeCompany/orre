//! Mechanical checks for invariants in `AGENTS.md` that the compiler can't
//! see. Each one scans the workspace's source text, so a rule that used to
//! live only in prose fails `cargo test` instead of waiting for review.
//!
//! They live in `orre-core` because every crate depends on it, so
//! `cargo test` always runs them; they read the other crates' files by path.
//! The dependency-direction rule needs `cargo tree`, so it's a lefthook job
//! (`.lefthook/pre-push/dependency-direction.sh`), not a test.
//!
//! The scans are deliberately blunt (text, not syntax). If one fires on code
//! that's fine, make the code say so plainly rather than loosening the rule.

use std::fs;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the workspace root exists")
}

/// Every `.rs` file under `dir`, recursively, sorted for stable output.
fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in fs::read_dir(&dir).expect("a source directory is readable") {
            let path = entry.expect("a directory entry is readable").path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

/// `src/` of each named crate.
fn sources(crates: &[&str]) -> Vec<(PathBuf, String)> {
    let root = workspace_root();
    crates
        .iter()
        .flat_map(|name| rust_files(&root.join("crates").join(name).join("src")))
        .map(|path| {
            let text = fs::read_to_string(&path).expect("a source file is readable");
            let shown = path.strip_prefix(&root).unwrap_or(&path).to_path_buf();
            (shown, text)
        })
        .collect()
}

/// The text of a line up to any `//` comment.
fn code(line: &str) -> &str {
    line.find("//").map_or(line, |at| &line[..at])
}

/// Whether `word` appears in `text` as a whole identifier.
fn has_word(text: &str, word: &str) -> bool {
    let is_ident = |c: char| c.is_ascii_alphanumeric() || c == '_';
    text.match_indices(word).any(|(at, _)| {
        let before = text[..at].chars().next_back();
        let after = text[at + word.len()..].chars().next();
        !before.is_some_and(is_ident) && !after.is_some_and(is_ident)
    })
}

fn fail_if_any(rule: &str, found: Vec<String>) {
    assert!(
        found.is_empty(),
        "{rule}\n\n{}\n",
        found
            .iter()
            .map(|line| format!("  {line}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// AGENTS.md → "Money is integer minor units + ISO 4217 currency. Never
/// `f32`/`f64` for amounts." Outside the GUI there is no other honest use of
/// a float, so the crates that hold money have none at all. (`crates/app`
/// uses floats for layout and chart geometry, which a text scan can't tell
/// from amounts.)
#[test]
fn no_floats_where_money_lives() {
    let mut found = Vec::new();
    for (path, text) in sources(&["core", "import", "mcp"]) {
        for (n, line) in text.lines().enumerate() {
            let code = code(line);
            if has_word(code, "f32") || has_word(code, "f64") {
                found.push(format!("{}:{}: {}", path.display(), n + 1, line.trim()));
            }
        }
    }
    fail_if_any(
        "Floats in a crate that handles money. Amounts are integer minor units \
         (`orre_core::money`); convert currencies only at reporting time.",
        found,
    );
}

/// Field names and variables that carry transaction text or amounts.
const TRANSACTION_CONTENT: &[&str] = &[
    "amount",
    "description",
    "raw_description",
    "merchant",
    "display_merchant",
    "payee",
    "counterparty",
    "memo",
    "note",
    "notes",
    "iban",
    "sort_code",
    "account_number",
];

/// The source text of each `info!`, `warn!` and `error!` call: from the macro
/// name to its closing parenthesis, which may be several lines on.
fn loud_log_calls(text: &str) -> Vec<(usize, String)> {
    let mut calls = Vec::new();
    for macro_name in ["info!(", "warn!(", "error!("] {
        for (at, _) in text.match_indices(macro_name) {
            let before = text[..at].chars().next_back();
            if before.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_') {
                continue; // e.g. `log_info!(`
            }
            let open = at + macro_name.len() - 1;
            let mut depth = 0usize;
            let mut end = text.len();
            for (offset, c) in text[open..].char_indices() {
                match c {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            end = open + offset + 1;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            let line = text[..at].lines().count().max(1);
            calls.push((line, text[at..end].to_owned()));
        }
    }
    calls
}

/// AGENTS.md → "Never log transaction contents at `info` level or above."
/// Log lines at `info`, `warn` and `error` name rows by ID only; contents go
/// to `debug`/`trace` if anywhere.
#[test]
fn loud_logs_carry_no_transaction_content() {
    let mut found = Vec::new();
    for (path, text) in sources(&["core", "import", "mcp", "app"]) {
        for (line, call) in loud_log_calls(&text) {
            let code: String = call.lines().map(code).collect::<Vec<_>>().join(" ");
            for word in TRANSACTION_CONTENT {
                if has_word(&code, word) {
                    found.push(format!("{}:{line}: `{word}` in {}", path.display(), {
                        let first = call.lines().next().unwrap_or_default();
                        first.trim()
                    }));
                }
            }
        }
    }
    fail_if_any(
        "Transaction content in an info/warn/error log line. Log the row's ID, \
         or move the detail to `debug!`/`trace!`.",
        found,
    );
}

/// The cfg that may gate a `describe` module or the dev driver: tests, or
/// the `dev-driver` feature (decision record "the dev driver", which can't
/// be on in a release build). Any other feature would put them in one.
const DESCRIBE_GATES: [&str; 2] = [
    "#[cfg(test)]",
    "#[cfg(any(test, feature = \"dev-driver\"))]",
];

/// AGENTS.md → "A view's `describe()` and anything that runs its actions are
/// test-only or dev-feature only." The views' descriptions live in
/// `describe` modules built on `ui::describe`; as long as every such module
/// is gated, release code that reaches one doesn't compile. So the rule to
/// keep is that each `mod describe` declaration stays under one of
/// [`DESCRIBE_GATES`].
#[test]
fn describe_modules_are_test_only() {
    let mut found = Vec::new();
    let mut declarations = 0;
    for (path, text) in sources(&["app"]) {
        let lines: Vec<&str> = text.lines().collect();
        for (n, line) in lines.iter().enumerate() {
            let code = code(line).trim();
            let declares = [
                "mod describe",
                "pub mod describe",
                "pub(crate) mod describe",
            ]
            .iter()
            .any(|decl| code.starts_with(decl) && code[decl.len()..].starts_with([';', ' ', '{']));
            if !declares {
                continue;
            }
            declarations += 1;
            let gated = lines[..n]
                .iter()
                .rev()
                .map(|line| line.trim())
                .take_while(|line| line.starts_with("#[") || line.starts_with("///"))
                .any(|attr| DESCRIBE_GATES.contains(&attr));
            if !gated {
                found.push(format!("{}:{}: {}", path.display(), n + 1, line.trim()));
            }
        }
    }
    assert!(
        declarations > 0,
        "found no `mod describe` declarations; did they move?"
    );
    fail_if_any(
        "A `describe` module reachable from a release build. Declare it under \
         `#[cfg(test)]` or `#[cfg(any(test, feature = \"dev-driver\"))]` (decision \
         records 2026-11-18 and 2026-11-21 in docs/ARCHITECTURE.md).",
        found,
    );
}

/// Decision record "the dev driver" (2026-11-21): the `dev-driver` feature
/// is off by default and enabled by no other feature, its module refuses
/// to compile in a release build, and `orre mcp`'s code never names it.
#[test]
fn the_dev_driver_stays_out_of_release_and_mcp() {
    let root = workspace_root();
    let manifest = fs::read_to_string(root.join("crates/app/Cargo.toml")).unwrap();
    let mut found = Vec::new();
    let mut in_features = false;
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_features = line == "[features]";
            continue;
        }
        if in_features && line.contains("dev-driver") && !line.starts_with("dev-driver =") {
            found.push(format!("crates/app/Cargo.toml: {line}"));
        }
    }
    assert!(
        manifest.contains("\ndev-driver = ["),
        "the dev-driver feature moved; update this check"
    );
    for (path, text) in sources(&["mcp"]).into_iter().chain(
        sources(&["app"])
            .into_iter()
            .filter(|(path, _)| path.to_string_lossy().contains("/cli/mcp")),
    ) {
        if text.contains("dev_driver") || text.contains(".describe(") {
            found.push(format!(
                "{}: names the dev driver or describe()",
                path.display()
            ));
        }
    }
    let driver = fs::read_to_string(root.join("crates/app/src/dev_driver.rs")).unwrap();
    if !driver.contains("#[cfg(not(debug_assertions))]\ncompile_error!(") {
        found.push("crates/app/src/dev_driver.rs: lost its release-build compile_error!".into());
    }
    let lib = fs::read_to_string(root.join("crates/app/src/lib.rs")).unwrap();
    if !lib.contains("#[cfg(feature = \"dev-driver\")]\npub mod dev_driver;") {
        found.push("crates/app/src/lib.rs: dev_driver isn't gated on its feature".into());
    }
    fail_if_any(
        "The dev driver reachable outside a dev build (decision record 2026-11-21 in \
         docs/ARCHITECTURE.md).",
        found,
    );
}

/// Decision record "full-text search over bank text", point 9: orre-core's
/// `bench` feature exists only so `ledger_bench --only search` can time
/// `LIKE` alone. No crate enables it, and nothing outside the ledger and its
/// tests names `like_only`, so the app and `orre mcp` always read through
/// the index.
#[test]
fn the_bench_feature_stays_out_of_the_app_and_mcp() {
    let root = workspace_root();
    let core = fs::read_to_string(root.join("crates/core/Cargo.toml")).unwrap();
    assert!(
        core.contains("\nbench = []"),
        "orre-core's bench feature moved; update this check"
    );
    let mut found = Vec::new();
    // In core's own manifest the definition is the only line that may name
    // it: `default = ["bench"]` or another feature listing it turns it on.
    for line in core
        .lines()
        .filter(|line| line.contains("\"bench\"") || line.contains("/bench\""))
    {
        found.push(format!("crates/core/Cargo.toml: {}", line.trim()));
    }
    // Elsewhere, neither `features = ["bench"]` on the dependency nor a
    // feature forwarding `orre-core/bench`.
    for name in ["app", "import", "mcp"] {
        let manifest =
            fs::read_to_string(root.join("crates").join(name).join("Cargo.toml")).unwrap();
        for line in manifest
            .lines()
            .filter(|line| line.contains("\"bench\"") || line.contains("/bench\""))
        {
            found.push(format!("crates/{name}/Cargo.toml: {}", line.trim()));
        }
    }
    let workspace = fs::read_to_string(root.join("Cargo.toml")).unwrap();
    for line in workspace
        .lines()
        .filter(|line| line.contains("\"bench\"") || line.contains("/bench\""))
    {
        found.push(format!("Cargo.toml: {}", line.trim()));
    }
    for (path, text) in sources(&["core", "import", "mcp", "app"]) {
        let allowed = path.ends_with("crates/core/src/reporting/ledger.rs")
            || path.ends_with("crates/core/src/reporting/ledger/tests.rs");
        if !allowed && text.contains("like_only") {
            found.push(format!("{}: names like_only", path.display()));
        }
    }
    fail_if_any(
        "orre-core's bench feature or `Ledger::like_only` reachable outside the bench \
         (decision record \"full-text search over bank text\", point 9).",
        found,
    );
}

#[test]
fn the_scans_see_what_they_look_for() {
    // Guard against a scan that silently matches nothing (a moved directory,
    // a renamed macro): each fixture below must trip its rule.
    assert!(has_word("let x: f64 = 1.0;", "f64"));
    assert!(!has_word("let x = buf64;", "f64"));
    let calls = loud_log_calls("tracing::info!(\n    id = %id,\n    amount = a,\n    \"x\"\n);");
    assert_eq!(calls.len(), 1);
    assert!(has_word(&calls[0].1, "amount"));
    assert!(loud_log_calls("log_info!(amount)").is_empty());
    assert!(!sources(&["core", "import", "mcp", "app"]).is_empty());
}
