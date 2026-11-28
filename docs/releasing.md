# Releasing Orre

How a version is tagged and what the release workflow,
[`.github/workflows/release.yml`](../.github/workflows/release.yml), does with the tag.
This page describes only what that workflow does today. What changed in each version is
in [`CHANGELOG.md`](../CHANGELOG.md); how users install the builds is in
[`install.md`](install.md).

## What the publish job does

A pushed `v*` tag builds the downloads, then the `publish` job creates a **published**
GitHub Release for the tag (not a draft). It runs only when all three build jobs pass,
and only on a tag push. It is the only job whose token has `contents: write`; the
workflow's default stays `contents: read`. Before it creates anything,
[`packaging/release/prepare-release.sh`](../packaging/release/prepare-release.sh)
checks the tag and writes the notes, and the job fails if:

- the tag is not exactly `v<version>`, where `<version>` is the `orre-app` crate's
  version (the workspace `version` in `Cargo.toml`): `v0.1.0` for `0.1.0`, and not
  `v0.1`, `0.1.0` or `v0.1.0-rc1`;
- `CHANGELOG.md` has no `## [<version>]` section (a ` - <date>` after the heading is
  fine), or the section is empty;
- the downloaded artifacts don't hold exactly one each of `*.tar.gz`,
  `*.tar.gz.sha256`, `*.dmg` and `*.dmg.sha256`.

Otherwise it runs `gh release create <tag> --verify-tag --title "Orre <version>"` with
those four files attached. The Release's body is that version's `CHANGELOG.md` section,
verbatim: from below its heading up to the next `## ` heading or the link references at
the end of the file, with blank lines trimmed at either end.

The Release, its notes and its files are **public** as soon as the job finishes. A
throwaway tag such as `v0.0.0-test` fails the version check, but any tag that matches
the crate version publishes a real Release, so test the workflow on a fork, not here.

It does **not** sign or notarise anything, publish the Nix flake anywhere (Nix users
build from the flake), or touch an existing Release: `gh release create` fails if the
tag already has one.

## Triggers

- **A pushed `v*` tag** (`push: tags: ["v*"]`), for example `v0.1.0`.
- **`workflow_dispatch`**, to prove a branch builds without tagging it:
  `gh workflow run release.yml --ref <branch>`. It runs the three build jobs and
  uploads their workflow artifacts, but never publishes: the `publish` job is skipped,
  even when the run is dispatched on a tag.

Runs for the same ref queue rather than cancel each other (`concurrency:
release-<ref>`, `cancel-in-progress: false`).

## Jobs

| Job | Runner | Builds and checks | Uploads (workflow artifact) |
|---|---|---|---|
| `Linux tarball` | `ubuntu-22.04` | `packaging/linux/build-tarball.sh` with `ORRE_MAX_GLIBC=2.35` (fails if the binary needs a newer glibc), then `packaging/linux/smoke-test.sh` under Xvfb | `orre-linux-x86_64`: `dist/*.tar.gz` and `dist/*.tar.gz.sha256` |
| `macOS dmg (unsigned)` | `macos-latest` (Apple Silicon) | `packaging/macos/build-dmg.sh` (ad-hoc signed, not notarised), then `packaging/macos/smoke-test.sh` | `orre-macos-arm64-unsigned`: `dist/*.dmg` and `dist/*.dmg.sha256` |
| `Nix flake` | `ubuntu-latest` | `nix build -L`, `nix flake check -L`, then `./result/bin/orre --version` | nothing: Nix users build from the flake |
| `Publish GitHub Release` | `ubuntu-latest` | tag pushes only, after the three jobs above pass: `packaging/release/prepare-release.sh`, then the four files (see "What the publish job does") | nothing: it downloads the two artifacts above and attaches them to the Release |

- **Checksums.** Each build script writes a SHA-256 file beside its archive, and both are
  uploaded together (`if-no-files-found: error`, so a missing file fails the job). The
  Linux and macOS jobs also print the checksum in the run's summary, with the glibc
  floor (Linux) or the signing note (macOS).
- **File names** come from the `orre-app` crate's version (`cargo pkgid`):
  `orre-<version>-linux-x86_64.tar.gz` and `orre-<version>-macos-arm64.dmg`.

## The tag path

1. On `main`, check that `version` in the workspace `Cargo.toml` is the version being
   tagged. The tag must be exactly `v` followed by that version, or the `publish` job
   fails.
2. Finish its `CHANGELOG.md` entry: give the heading its date
   (`## [0.1.0] - 2026-12-15`) and remove the "Draft:" note. The section becomes the
   Release's body verbatim, so it should read as release notes.
3. Optionally, prove the commit builds first with a `workflow_dispatch` run; it
   publishes nothing. Run
   [46087614468](https://github.com/TheSlimeCompany/orre/actions/runs/46087614468) was
   such a run: all three build jobs green on `main` at `07e4ea2`.
4. Push the tag, for example `git tag -a v0.1.0 -m "Orre 0.1.0"` then
   `git push origin v0.1.0`. The push starts the workflow.
5. When the three build jobs are green, the `publish` job creates the Release for the
   tag, with the tarball, the `.dmg` and their `.sha256` files attached and the
   changelog section as its notes. The files are also on the run's page under
   **Artifacts**.

If a run fails before `gh release create`, nothing was published. If the build jobs and
the tagged commit are fine (a flaky runner, say), re-run the failed jobs from the run's
page. If the tag or the commit needs fixing (a wrong version, a missing changelog
section), fix it on `main`, delete the tag locally and on GitHub
(`git tag -d v0.1.0` and `git push origin :refs/tags/v0.1.0`), then tag the fixed
commit and push the tag again.
