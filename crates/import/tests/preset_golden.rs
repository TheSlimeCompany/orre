//! Freezes each bundled preset's dedupe identity: the external ID and the
//! fingerprint of every row its fixture imports.
//!
//! A row's identity is its source ID, else its fingerprint, and the
//! fingerprint depends on the columns a preset joins into the description,
//! its date format and sign handling. Editing a preset can therefore move
//! it, and the next import of a file that overlaps an earlier one would
//! duplicate rows. The golden files in `tests/golden/<region>/<bank>.txt`
//! record the identities; see `presets/README.md` → "Changing a preset".
//!
//! The rows are read back from a scratch database after the real preview and
//! commit, so the fingerprint stays private to `orre-core`.
//!
//! To create the golden file of a new preset, run this test once with
//! `ORRE_BLESS_GOLDEN=1`: it writes the files that are missing and never
//! overwrites one that exists.

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use orre_core::account::{self, AccountKind, NewAccount};
use orre_core::db::Database;
use orre_core::import::{self, Committed};
use orre_import::preset::{Preset, Presets};
use orre_import::{AnyImporter, SourceFile};
use rusqlite::{Connection, OpenFlags};
use tempfile::TempDir;

const BLESS: &str = "ORRE_BLESS_GOLDEN";

const HEADER: &str = "\
# Golden file: the dedupe identity of every row in the preset's fixture, in file
# order: `row <n>`, the row's external ID (or `-`), and its fingerprint, tab
# separated. The test in crates/import/tests/preset_golden.rs checks them.
#
# Rule: a preset change that moves a row's external ID or fingerprint needs a
# new preset ID (a new preset), not an edited golden file. Imports already in
# users' databases were deduplicated under the old identity, and the next
# overlapping import would duplicate rows. See presets/README.md, \"Changing a preset\".
";

fn manifest() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

/// Every `presets/<region>/<bank>.toml`, as (region, bank, path), sorted.
fn preset_files() -> Vec<(String, String, PathBuf)> {
    let mut files = Vec::new();
    for region in fs::read_dir(manifest().join("../../presets")).unwrap() {
        let region = region.unwrap().path();
        if !region.is_dir() {
            continue;
        }
        for file in fs::read_dir(&region).unwrap() {
            let path = file.unwrap().path();
            if path
                .extension()
                .is_some_and(|extension| extension == "toml")
            {
                files.push((
                    region.file_name().unwrap().to_str().unwrap().to_owned(),
                    path.file_stem().unwrap().to_str().unwrap().to_owned(),
                    path,
                ));
            }
        }
    }
    files.sort();
    files
}

/// The fixtures for `region/bank`, any extension.
fn fixtures_of(region: &str, bank: &str) -> Vec<PathBuf> {
    let mut found: Vec<_> = fs::read_dir(manifest().join("../../fixtures/presets").join(region))
        .into_iter()
        .flatten()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.file_stem().is_some_and(|stem| stem == bank))
        .collect();
    found.sort();
    found
}

/// Imports `fixture` with `preset` into a scratch database and returns one
/// golden line per committed imported row, in file order.
fn identities(preset: &Preset, fixture: &Path) -> Vec<String> {
    let dir = TempDir::new().unwrap();
    let mut db = Database::open(&dir.path().join("orre.db")).unwrap();
    let account = account::create(
        &mut db,
        &NewAccount::new("Golden", AccountKind::Current, preset.mapping().currency),
    )
    .unwrap();
    let statement = preset
        .importer()
        .import(&SourceFile::read(fixture).unwrap())
        .unwrap_or_else(|error| panic!("{}: {error}", fixture.display()));
    let preview = import::preview(&db, account, "golden", statement).unwrap();
    let confirm = preview.needs_confirmation();
    match import::commit(&mut db, &preview, confirm).unwrap() {
        Committed::Batch(_) => {}
        Committed::NothingNew => panic!("{}: nothing was imported", fixture.display()),
    }

    let reader = Connection::open_with_flags(db.path(), OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let mut query = reader
        .prepare(
            "SELECT external_id, fingerprint FROM \"transaction\" \
             WHERE origin = 'imported' ORDER BY id",
        )
        .unwrap();
    query
        .query_map([], |row| {
            Ok((row.get::<_, Option<String>>(0)?, row.get::<_, String>(1)?))
        })
        .unwrap()
        .enumerate()
        .map(|(index, row)| {
            let (external_id, fingerprint) = row.unwrap();
            format!(
                "row {}\t{}\t{fingerprint}",
                index + 1,
                external_id.as_deref().unwrap_or("-")
            )
        })
        .collect()
}

/// The golden file's text, header included.
fn render(lines: &[String]) -> String {
    let mut text = HEADER.to_owned();
    for line in lines {
        writeln!(text, "{line}").unwrap();
    }
    text
}

/// The data lines of a golden file: everything but blank and `#` lines.
fn data_lines(text: &str) -> Vec<&str> {
    text.lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect()
}

fn moved_message(id: &str, golden: &Path, expected: &[&str], actual: &[String]) -> String {
    let mut message = format!(
        "preset {id}: the external IDs or fingerprints its fixture imports moved from {}.\n",
        golden.display()
    );
    if expected.len() != actual.len() {
        writeln!(
            message,
            "  rows: {} in the golden file, {} imported now",
            expected.len(),
            actual.len()
        )
        .unwrap();
    }
    for (was, now) in expected.iter().zip(actual) {
        if was != now {
            writeln!(message, "  was: {was}\n  now: {now}").unwrap();
        }
    }
    message.push_str(
        "Rule: a preset change that moves a row's external ID or fingerprint needs a new preset \
         ID (a new preset), not an edited golden file: imports already in users' databases were \
         deduplicated under the old identity, and the next overlapping import would duplicate \
         rows. See presets/README.md, \"Changing a preset\".",
    );
    message
}

#[test]
fn every_bundled_preset_keeps_its_dedupe_identity() {
    let presets = Presets::bundled().unwrap_or_else(|error| panic!("{error}"));
    let bless = std::env::var_os(BLESS).is_some();
    let files = preset_files();
    assert_eq!(files.len(), presets.len(), "every preset file is bundled");

    for (region, bank, path) in files {
        let id = Preset::from_toml(&fs::read_to_string(&path).unwrap())
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()))
            .id()
            .to_owned();
        let preset = presets.get(&id).expect("bundled");

        let fixtures = fixtures_of(&region, &bank);
        assert!(
            !fixtures.is_empty(),
            "{id} needs a fixture in fixtures/presets/{region}/ (presets/README.md, \"Where a preset goes\")"
        );
        let golden = manifest()
            .join("tests/golden")
            .join(&region)
            .join(format!("{bank}.txt"));
        assert_eq!(
            fixtures.len(),
            1,
            "{id}: one golden file per preset, but {} fixtures share its name",
            fixtures.len()
        );
        let actual = identities(preset, &fixtures[0]);
        assert!(!actual.is_empty(), "{id}: its fixture imports no rows");

        if !golden.exists() {
            assert!(
                bless,
                "{id} has no golden file at {}. Create it with \
                 `{BLESS}=1 cargo test -p orre-import --test preset_golden`, \
                 check it, and commit it (presets/README.md, \"Changing a preset\").",
                golden.display()
            );
            fs::create_dir_all(golden.parent().unwrap()).unwrap();
            fs::write(&golden, render(&actual)).unwrap();
            eprintln!("wrote {}", golden.display());
            continue;
        }

        let expected_text = fs::read_to_string(&golden).unwrap();
        let expected = data_lines(&expected_text);
        if expected != actual.iter().map(String::as_str).collect::<Vec<_>>() {
            panic!("{}", moved_message(&id, &golden, &expected, &actual));
        }
    }
}
