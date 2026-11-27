{
  # Orre as a Nix package (M5.6). The dev shell stays devenv (devenv.nix);
  # this flake is for installing and running Orre:
  #
  #   nix run github:TheSlimeCompany/orre               # open the app
  #   nix run github:TheSlimeCompany/orre -- --help     # the headless commands
  #   nix build && ./result/bin/orre --help
  #   nix profile install github:TheSlimeCompany/orre
  #   nix flake check                               # builds it and runs --help
  description = "Orre: a local-first personal finance dashboard (GPUI desktop app, CLI and MCP server)";

  inputs = {
    # A release channel with rustc >= the workspace MSRV (rust-version in
    # Cargo.toml, 1.95). nixos-26.11 has rustc 1.99.
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.11";
  };

  outputs =
    { self, nixpkgs }:
    let
      # Linux only for now: the macOS download is the unsigned .dmg
      # (packaging/macos); a Darwin package is out of v0.1's scope.
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
      workspace = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).workspace.package;
    in
    {
      packages = forAllSystems (pkgs: {
        orre = pkgs.callPackage ./packaging/nix/package.nix {
          version = workspace.version;
          src = ./.;
        };
        default = self.packages.${pkgs.stdenv.hostPlatform.system}.orre;
      });

      apps = forAllSystems (pkgs: {
        default = {
          type = "app";
          program = "${self.packages.${pkgs.stdenv.hostPlatform.system}.orre}/bin/orre";
          meta.description = "Open Orre";
        };
      });

      checks = forAllSystems (pkgs: {
        # The package builds, and the binary starts and prints its help
        # without LD_LIBRARY_PATH (the sandbox has no display, so the
        # window isn't opened).
        orre-help = pkgs.runCommand "orre-help" { } ''
          ${self.packages.${pkgs.stdenv.hostPlatform.system}.orre}/bin/orre --help > $out
          grep -q 'local-first personal finance dashboard' $out
          if grep -q -- '--showcase' $out; then
            echo "--showcase is in the help: dev-samples is in the package" >&2
            exit 1
          fi
        '';
      });

      formatter = forAllSystems (pkgs: pkgs.nixfmt);
    };
}
