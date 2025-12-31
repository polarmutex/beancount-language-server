{
  description = "Beancount Language Server";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
    crane = {
      url = "github:ipetkov/crane";
    };
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    advisory-db = {
      url = "github:rustsec/advisory-db";
      flake = false;
    };
    cargo-dist-src = {
      url = "github:axodotdev/cargo-dist/v0.28.1";
      flake = false;
    };
  };

  outputs = inputs @ {
    self,
    nixpkgs,
    crane,
    flake-parts,
    rust-overlay,
    advisory-db,
    cargo-dist-src,
    ...
  }:
    flake-parts.lib.mkFlake {inherit inputs;} {
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
        "x86_64-darwin"
      ];

      perSystem = {
        config,
        self',
        inputs',
        pkgs,
        system,
        ...
      }: let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [(import rust-overlay)];
        };
        inherit (pkgs) lib;

        # Use latest stable Rust instead of pinning to specific version
        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = ["rust-src" "rust-analyzer" "llvm-tools-preview"];
          targets = [
            "aarch64-apple-darwin"
            "aarch64-pc-windows-msvc"
            "aarch64-unknown-linux-gnu"
            "x86_64-pc-windows-msvc"
            "x86_64-unknown-linux-gnu"
          ];
        };
        craneLib = (crane.mkLib pkgs).overrideToolchain rustToolchain;

        src = craneLib.cleanCargoSource ./.;

        commonArgs = {
          inherit src;
          strictDeps = true;
          buildInputs = with pkgs;
            []
            ++ lib.optionals stdenv.isDarwin [libiconv];

          # Pass git hash as environment variable
          GIT_HASH = self.shortRev or (self.dirtyShortRev or "dirty");
        };

        cargoArtifacts = craneLib.buildDepsOnly (commonArgs
          // {
            pname = "beancount-language-server-deps";
          });

        beancount-language-server = craneLib.buildPackage (commonArgs
          // {
            inherit cargoArtifacts;
            inherit (craneLib.crateNameFromCargoToml {cargoToml = ./Cargo.toml;}) version;
            pname = "beancount-language-server";
          });
      in {
        checks = {
          # Build the crate as part of `nix flake check` for convenience
          inherit beancount-language-server;

          # Run clippy (and deny all warnings) on the crate source,
          # again, reusing the dependency artifacts from above.
          beancount-language-server-clippy = craneLib.cargoClippy (commonArgs
            // {
              inherit cargoArtifacts;
              cargoClippyExtraArgs = "--all-targets -- --deny warnings";
            });

          beancount-language-server-doc = craneLib.cargoDoc (commonArgs
            // {
              inherit cargoArtifacts;
            });

          # Check formatting
          beancount-language-server-fmt = craneLib.cargoFmt {
            inherit src;
          };

          # Audit dependencies
          beancount-language-server-audit = craneLib.cargoAudit (commonArgs
            // {
              inherit advisory-db;
            });

          # Run tests with cargo-nextest
          beancount-language-server-nextest = craneLib.cargoNextest (commonArgs
            // {
              inherit cargoArtifacts;
              partitions = 1;
              partitionType = "count";
            });
        };

        packages = {
          inherit beancount-language-server;
          default = beancount-language-server;
        };

        devShells.default = craneLib.devShell {
          # Inherit inputs from checks
          checks = self'.checks;

          # Additional dev dependencies
          packages = with pkgs;
            [
              git-cliff
              beancount
              cargo-llvm-cov
              (pkgs.cargo-dist.overrideAttrs (oldAttrs: {
                version = "0.28.1";
                src = cargo-dist-src;
                cargoDeps = pkgs.rustPlatform.importCargoLock {
                  lockFile = "${cargo-dist-src}/Cargo.lock";
                };
              }))
              rustToolchain
            ]
            ++ lib.optionals stdenv.isLinux [systemd];

          # Environment variables
          GIT_HASH = self.shortRev or (self.dirtyShortRev or "dirty");
        };
      };
    };
}
