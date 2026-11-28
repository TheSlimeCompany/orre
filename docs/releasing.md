# Releasing Orre

How a version is tagged and what the release workflow,
[`.github/workflows/release.yml`](../.github/workflows/release.yml), does with the tag.
This page describes only what that workflow does today. What changed in each version is
in [`CHANGELOG.md`](../CHANGELOG.md); how users install the builds is in
[`install.md`](install.md).

## What the workflow does not do (yet)

Today a tag run produces **workflow artifacts only**. The workflow's token has
`contents: read`, and no step creates a GitHub Release: the tarball, the `.dmg` and their
checksums are on the run's page under **Artifacts**, not on a release page.

Creating the GitHub Release from a tag is coming in a follow-up ticket. Havoc will add
it to `release.yml` and update this page then.

## Triggers

- **A pushed `v*` tag** (`push: tags: ["v*"]`), for example `v0.1.0`.
- **`workflow_dispatch`**, to prove a branch builds without tagging it:
  `gh workflow run release.yml --ref <branch>`.

Runs for the same ref queue rather than cancel each other (`concurrency:
release-<ref>`, `cancel-in-progress: false`).

## Jobs

| Job | Runner | Builds and checks | Uploads (workflow artifact) |
|---|---|---|---|
| `Linux tarball` | `ubuntu-22.04` | `packaging/linux/build-tarball.sh` with `ORRE_MAX_GLIBC=2.35` (fails if the binary needs a newer glibc), then `packaging/linux/smoke-test.sh` under Xvfb | `orre-linux-x86_64`: `dist/*.tar.gz` and `dist/*.tar.gz.sha256` |
| `macOS dmg (unsigned)` | `macos-latest` (Apple Silicon) | `packaging/macos/build-dmg.sh` (ad-hoc signed, not notarised), then `packaging/macos/smoke-test.sh` | `orre-macos-arm64-unsigned`: `dist/*.dmg` and `dist/*.dmg.sha256` |
| `Nix flake` | `ubuntu-latest` | `nix build -L`, `nix flake check -L`, then `./result/bin/orre --version` | nothing: Nix users build from the flake |

- **Checksums.** Each build script writes a SHA-256 file beside its archive, and both are
  uploaded together (`if-no-files-found: error`, so a missing file fails the job). The
  Linux and macOS jobs also print the checksum in the run's summary, with the glibc
  floor (Linux) or the signing note (macOS).
- **File names** come from the `orre-app` crate's version (`cargo pkgid`):
  `orre-<version>-linux-x86_64.tar.gz` and `orre-<version>-macos-arm64.dmg`. The
  workflow doesn't compare the tag with that version.

## The tag path

1. On `main`, check that `version` in the workspace `Cargo.toml` is the version being
   tagged and that `CHANGELOG.md` has its entry, then give the entry its date.
2. Optionally, prove the commit builds first with a `workflow_dispatch` run. Run
   [46087614468](https://github.com/TheSlimeCompany/orre/actions/runs/46087614468) was
   such a run: all three jobs green on `main` at `07e4ea2`.
3. Push the tag, for example `git tag -a v0.1.0 -m "Orre 0.1.0"` then
   `git push origin v0.1.0`. The push starts the workflow.
4. When the three jobs are green, the tarball, the `.dmg` and their `.sha256` files are
   on that run's page under **Artifacts**. The workflow doesn't create a GitHub Release
   from them yet (see above).
