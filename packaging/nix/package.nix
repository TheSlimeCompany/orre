# The `orre` package for flake.nix: the release build of crates/app with no
# features (no `dev-samples` sample data or `--showcase`, no `dev-driver`),
# from Cargo.lock as it is (buildRustPackage vendors it and builds offline,
# which is `--locked` and more).
{
  lib,
  rustPlatform,
  pkg-config,
  patchelf,
  openssl,
  zstd,
  alsa-lib,
  vulkan-headers,
  wayland,
  libxkbcommon,
  libxcb,
  libx11,
  vulkan-loader,
  fontconfig,
  freetype,
  version,
  src,
}:

let
  # Libraries GPUI loads at run time (windowing, GPU, fonts): the same list
  # as `gpuiLibs` in devenv.nix. They go on the binary's rpath, so it runs
  # without LD_LIBRARY_PATH.
  gpuiLibs = [
    wayland
    libxkbcommon
    libxcb
    libx11
    vulkan-loader
    fontconfig
    freetype
  ];
in
rustPlatform.buildRustPackage {
  pname = "orre";
  inherit version;

  # Only what the build reads; editing docs or CI doesn't rebuild Orre.
  src = lib.fileset.toSource {
    root = src;
    fileset = lib.fileset.unions [
      (src + "/Cargo.toml")
      (src + "/Cargo.lock")
      (src + "/.cargo")
      (src + "/crates")
      (src + "/assets")
      (src + "/presets")
      (src + "/design")
      (src + "/fixtures")
    ];
  };

  cargoLock.lockFile = src + "/Cargo.lock";

  cargoBuildFlags = [
    "--package"
    "orre-app"
    "--bin"
    "orre"
  ];

  # nixpkgs' cargo setup hook exports LIBSQLITE3_SYS_USE_PKG_CONFIG=1, which
  # makes libsqlite3-sys link the system SQLite despite rusqlite's `bundled`
  # feature. Turn it off so this binary has the same bundled SQLite (and the
  # LIBSQLITE3_FLAGS in .cargo/config.toml) as every other build of Orre.
  preBuild = ''
    export LIBSQLITE3_SYS_USE_PKG_CONFIG=0
  '';

  # The tests need a display and the whole dev setup; CI and the pre-push
  # hook run them. `nix flake check` runs `orre --help` instead.
  doCheck = false;

  nativeBuildInputs = [
    pkg-config
    patchelf
    rustPlatform.bindgenHook
  ];

  buildInputs = [
    openssl
    zstd
    alsa-lib
    vulkan-headers
  ]
  ++ gpuiLibs;

  # buildRustPackage overrides the release profile's `strip = true` and
  # leaves stripping to stdenv, which only strips debug info (`strip -S`).
  # Strip the symbol table too, like the tarball's binary; then add the GPUI
  # libraries to its rpath.
  stripAllList = [ "bin" ];
  postFixup = ''
    patchelf --add-rpath ${lib.makeLibraryPath gpuiLibs} $out/bin/orre
  '';

  meta = {
    description = "Local-first personal finance dashboard: bank exports into SQLite, charts, and an MCP server";
    homepage = "https://github.com/TheSlimeCompany/orre";
    license = with lib.licenses; [
      mit
      asl20
    ];
    mainProgram = "orre";
    platforms = lib.platforms.linux;
  };
}
