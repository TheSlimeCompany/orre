# Installing Orre

v0.1 is not released yet: this page describes the Linux tarball the
release workflow (`.github/workflows/release.yml`) builds, and the Nix
flake (`flake.nix`) that workflow builds and checks. Until a `v0.1.0` tag
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

### How the tarball is built

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

## Nix (Linux)

The flake (`flake.nix`) builds the same release binary, with no features,
from the repository. It is Linux only: `x86_64-linux` is tested (the
release workflow's `Nix flake` job builds and checks it); `aarch64-linux`
is listed in the flake but untested, as there is no aarch64 build in CI
for v0.1. On macOS use the `.dmg`.

```sh
nix run github:TheSlimeCompany/orre               # opens the app
nix run github:TheSlimeCompany/orre -- --help     # the headless commands
nix profile install github:TheSlimeCompany/orre   # puts `orre` on your PATH
```

From a checkout, `nix build` leaves the binary in `./result/bin/orre`, and
`nix flake check` builds it and runs `orre --help`. The windowing, Vulkan
and font libraries are on the binary's rpath, so you don't set
`LD_LIBRARY_PATH`.

**Known limitation in v0.1: Nix outside NixOS.** On NixOS the GPU driver
comes from the system (`hardware.graphics`). On other distributions,
Nix's Vulkan loader doesn't load the distribution's Mesa drivers, and the
window fails to open with "Failed to create surface".
The headless commands still work. To open the window, point the loader at a
Mesa from nixpkgs, for example its software driver (lavapipe):

```sh
mesa=$(nix build --no-link --print-out-paths nixpkgs#mesa)
VK_ICD_FILENAMES=$mesa/share/vulkan/icd.d/lvp_icd.x86_64.json orre
```

(Checked on Debian 13 under Xvfb.) [nixGL](https://github.com/nix-community/nixGL)
automates this, hardware drivers included; it hasn't been tested with Orre.
Its Mesa Vulkan wrapper, with nixGL's nixpkgs set to the flake's
(`nixos-26.11`) as its README advises, is:

```sh
nix run --override-input nixpkgs nixpkgs/nixos-26.11 github:nix-community/nixGL#nixVulkanIntel -- orre
```

The tarball above uses the distribution's drivers and doesn't have this
problem.
