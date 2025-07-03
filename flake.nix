{
  description = "Build a cargo project";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    crane = { url = "github:ipetkov/crane"; };
    advisory-db = {
      url = "github:rustsec/advisory-db";
      flake = false;
    };
  };

  outputs = inputs@{ self, nixpkgs, crane, flake-parts, advisory-db
    , rust-overlay, ... }:
    let GIT_HASH = self.shortRev or (self.dirtyShortRev or "dirty");
    in flake-parts.lib.mkFlake { inherit inputs; } {
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        # wind
        # mac
      ];
      perSystem = { pkgs, system, ... }:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ (import rust-overlay) ];
          };

          commonArgs = {
            src = craneLib.cleanCargoSource (craneLib.path ./.);
            buildInputs = [ ];

            inherit GIT_HASH;
          } // (craneLib.crateNameFromCargoToml {
            cargoToml = ./crates/lsp/Cargo.toml;
          });

          craneLib = (crane.mkLib pkgs).overrideToolchain
            (pkgs.rust-bin.stable.latest.default.override {
              extensions = [
                "cargo"
                "clippy"
                "rust-src"
                "rust-analyzer"
                "rustc"
                "rustfmt"
              ];
            });

          cargoArtifacts = craneLib.buildDepsOnly
            (commonArgs // { panme = "beancount-language-server-deps"; });

          beancount-language-server =
            craneLib.buildPackage (commonArgs // { inherit cargoArtifacts; });
        in {
          checks = {
            # Build the crate as part of `nix flake check` for convenience
            inherit beancount-language-server;

            # Run clippy (and deny all warnings) on the crate source,
            # again, resuing the dependency artifacts from above.
            #
            # Note that this is done as a separate derivation so that
            # we can block the CI if there are issues here, but not
            # prevent downstream consumers from building our crate by itself.
            beancount-language-server-clippy = craneLib.cargoClippy (commonArgs
              // {
                inherit cargoArtifacts;
                cargoClippyExtraArgs = "--all-targets -- --deny warnings";
              });

            beancount-language-server-doc =
              craneLib.cargoDoc (commonArgs // { inherit cargoArtifacts; });

            # Check formatting
            beancount-language-server-fmt =
              craneLib.cargoFmt (commonArgs // { });

            # Audit dependencies
            beancount-language-server-audit =
              craneLib.cargoAudit (commonArgs // { inherit advisory-db; });

            # Run tests with cargo-nextest
            # Consider setting `doCheck = false` on `my-crate` if you do not want
            # the tests to run twice
            beancount-language-server-nextest = craneLib.cargoNextest
              (commonArgs // {
                inherit cargoArtifacts;
                partitions = 1;
                partitionType = "count";
              });
          };

          packages = {
            inherit beancount-language-server;
            default = beancount-language-server;
          };

          devShells.default = pkgs.mkShell {
            buildInputs = with pkgs;
              [ clang pkg-config systemd ] ++ commonArgs.buildInputs;
            nativeBuildInputs = with pkgs; [
              gnumake
              (rust-bin.stable.latest.default.override {
                extensions = [
                  "cargo"
                  "clippy"
                  "rust-src"
                  "rust-analyzer"
                  "rustc"
                  "rustfmt"
                ];
              })
              git-cliff
              virt-viewer
            ];

            inherit GIT_HASH;
          };
        };
    };
  #packages.default = beancount-language-server-crate;

  #apps.default = flake-utils.lib.mkApp {
  #  drv = beancount-language-server-crate;
  #};

  #devShells.default = pkgs.mkShell {
  #  inputsFrom = builtins.attrValues self.checks;

  #  # Extra inputs can be added here
  #  nativeBuildInputs = with pkgs; [
  #    cargo
  #    cargo-dist
  #    rustc
  #    rustfmt
  #    clippy
  #    git-cliff
  #    #nodejs-16_x
  #    #python310
  #  ];
}
