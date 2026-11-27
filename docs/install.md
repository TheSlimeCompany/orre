# Installing Orre

v0.1 is not released yet: this page describes the downloads the release
workflow (`.github/workflows/release.yml`) builds. Until a `v0.1.0` tag
exists, build from source (README → "Development").

## Linux (tarball)

`orre-<version>-linux-x86_64.tar.gz` holds the stripped release binary,
a `.desktop` file, the icon, the licences and a short `README`. No Rust
toolchain, Nix or devenv is needed to run it.

```sh
tar xzf orre-0.1.0-linux-x86_64.tar.gz
cd orre-0.1.0-linux-x86_64
./orre            # opens the app
./orre --help     # the headless commands
```

Check the download against the `.sha256` file next to it:
`sha256sum -c orre-0.1.0-linux-x86_64.tar.gz.sha256`.

To add it to your desktop's launcher (for your user only):

```sh
install -Dm755 orre ~/.local/bin/orre
install -Dm644 orre.desktop ~/.local/share/applications/orre.desktop
install -Dm644 orre.png ~/.local/share/icons/hicolor/1024x1024/apps/orre.png
```

`~/.local/bin` has to be on your `PATH`, since the launcher runs `orre`.

### Requirements

- **glibc.** The release job builds on Ubuntu 22.04, so the tarball needs
  glibc 2.35 or newer (Ubuntu 22.04+, Debian 12+, Fedora 36+), to be confirmed
  by the first run of the job. The
  floor is read from the binary at build time and printed in the
  tarball's `README` and the workflow summary; that number is the one that
  counts, and the release job fails if it is above 2.35. A tarball built
  elsewhere needs a newer glibc: one built on Debian 13 (glibc 2.41) has a
  floor of 2.39.
- **A Wayland or X11 session and a Vulkan driver.** Mesa's drivers are
  fine, including its software one.
- **Shared libraries.** The binary links `libxkbcommon`,
  `libxkbcommon-x11` and `libxcb` directly; the windowing and GPU code
  loads `libwayland-client`, `libX11-xcb`, `libvulkan` (or `libEGL`) and
  the Mesa driver when it starts. fontconfig and freetype are in GPUI's
  library list (`devenv.nix`) but weren't loaded in an X11 test run;
  install them if Orre won't start. Most desktop installs already have all
  of these:

| Library | Debian / Ubuntu | Fedora |
| --- | --- | --- |
| wayland-client | `libwayland-client0` | `libwayland-client` |
| xkbcommon | `libxkbcommon0`, `libxkbcommon-x11-0` | `libxkbcommon`, `libxkbcommon-x11` |
| xcb, X11 | `libxcb1`, `libx11-6`, `libx11-xcb1` | `libxcb`, `libX11`, `libX11-xcb` |
| Vulkan | `libvulkan1`, `mesa-vulkan-drivers` | `vulkan-loader`, `mesa-vulkan-drivers` |
| fontconfig | `libfontconfig1` | `fontconfig` |
| freetype | `libfreetype6` | `freetype` |

Fonts (Inter and Geist Mono) are compiled into the binary.

Your data lives in `~/.local/share/orre` unless `ORRE_DATA_DIR` names
another folder.

There is no AppImage in v0.1 (planned for v0.1.1).

## How the tarball is built

`bash packaging/linux/build-tarball.sh` builds
`cargo build --release --locked -p orre-app` with the workspace release
profile and no features (so no sample data, `--showcase` or dev driver),
refuses a binary that still has symbols or debug info, and writes
`dist/orre-<version>-linux-<arch>.tar.gz` and its `.sha256`. With
`ORRE_MAX_GLIBC` set (the release job sets `2.35`), it also fails if the
binary's glibc floor is above that version; unset, there is no limit.
`bash packaging/linux/smoke-test.sh dist/<archive>` unpacks it and checks
the contents, the linked libraries (each must be in the table above), that
`--showcase` is rejected, `--help` and `--version` under Xvfb, a headless
command on a scratch data dir, and that the window opens and stays up.
The release workflow runs both on `workflow_dispatch` and on `v*` tags,
and uploads the tarball as a workflow artifact.
