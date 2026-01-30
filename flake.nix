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
  };

  outputs = inputs @ {
    self,
    nixpkgs,
    crane,
    flake-parts,
    rust-overlay,
    advisory-db,
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

        # Include Python files alongside Rust sources for include_str! macro
        src = lib.cleanSourceWith {
          src = craneLib.path ./.;
          filter = path: type:
            (craneLib.filterCargoSources path type)
            || (lib.hasSuffix ".py" path);
        };

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

          # VSCode extension VSIX package
          beancount-vscode-vsix = let
            vscodeWithLicense = pkgs.runCommand "vscode-src" {} ''
              cp -r ${./vscode} $out
              chmod -R +w $out
              rm -f $out/LICENSE
              cp ${./LICENSE} $out/LICENSE
            '';
          in pkgs.stdenv.mkDerivation rec {
            pname = "beancount-vscode-vsix";
            version = (builtins.fromJSON (builtins.readFile ./vscode/package.json)).version;

            src = vscodeWithLicense;

            nativeBuildInputs = with pkgs; [
              nodejs
              pnpm
              pnpmConfigHook
            ];

            pnpmDeps = pkgs.fetchPnpmDeps {
              inherit pname version src;
              hash = "sha256-n0qfM51winZCu2kv9pqJmQE4OVKl4+DFC/2wgJ/hYZs=";
              fetcherVersion = 3; # lockfileVersion 9.0 uses fetcher v3
            };

            buildPhase = ''
              runHook preBuild

              # Create server directory with the locally built binary
              # Map Nix system to rust triplet for VSCode extension compatibility
              triplet=""
              case "${system}" in
                x86_64-linux) triplet="x86_64-unknown-linux-gnu" ;;
                aarch64-linux) triplet="aarch64-unknown-linux-gnu" ;;
                x86_64-darwin) triplet="x86_64-apple-darwin" ;;
                aarch64-darwin) triplet="aarch64-apple-darwin" ;;
                *) echo "Unsupported system: ${system}"; exit 1 ;;
              esac

              mkdir -p server/$triplet
              cp ${beancount-language-server}/bin/beancount-language-server server/$triplet/
              chmod +x server/$triplet/beancount-language-server

              # Build the extension (compiles TypeScript)
              pnpm run build-base

              # Package the VSIX
              mkdir -p dist
              pnpm exec vsce package --no-dependencies -o dist/beancount-language-server-${system}.vsix

              runHook postBuild
            '';

            installPhase = ''
              runHook preInstall

              mkdir -p $out
              cp dist/*.vsix $out/

              runHook postInstall
            '';

            meta = with lib; {
              description = "Beancount language server VSCode extension";
              homepage = "https://github.com/polarmutex/beancount-language-server";
              license = licenses.mit;
              platforms = platforms.all;
            };
          };
        };

        devShells.default = craneLib.devShell {
          # Inherit inputs from checks
          checks = self'.checks;

          # Additional dev dependencies
          packages = with pkgs;
            [
              git-cliff
              cargo-edit
              beancount
              cargo-llvm-cov
              cargo-hack
              just
              rustToolchain
              nodejs
              nodePackages.pnpm
            ]
            ++ lib.optionals stdenv.isLinux [systemd];

          # Environment variables
          GIT_HASH = self.shortRev or (self.dirtyShortRev or "dirty");
        };
      };
    };
}
