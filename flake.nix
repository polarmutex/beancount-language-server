{
  inputs = {
    flake-utils.url = "github:numtide/flake-utils";
    naersk.url = "github:nix-community/naersk";
  };

  outputs = { self, nixpkgs, flake-utils, naersk }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = nixpkgs.legacyPackages."${system}";
        naersk-lib = naersk.lib."${system}";
      in
      rec {
        # `nix build`
        packages.beancount-language-server = naersk-lib.buildPackage {
          pname = "beancount-language-server";
          root = ./.;
        };
        defaultPackage = packages.beancount-language-server;

        # `nix run`
        apps.beancount-language-server = flake-utils.lib.mkApp {
          drv = packages.beancount-language-server;
        };
        defaultApp = apps.beancount-language-server;

        # `nix develop`
        devShell = pkgs.mkShell {
          nativeBuildInputs = with pkgs; [
            rustc
            cargo
            rustfmt
            clippy
          ];
        };
      }
    );
}
