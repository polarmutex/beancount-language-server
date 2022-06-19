{
  inputs = {
    flake-utils.url = "github:numtide/flake-utils";
    naersk.url = "github:nix-community/naersk";
  };

  outputs = { self, nixpkgs, flake-utils, naersk }:
    {
      overlay = final: prev: {
        inherit (self.packages.${final.system})
          beancount-language-server-git;
      };
    } //
    flake-utils.lib.eachDefaultSystem
      (
        system:
        let
          pkgs = import nixpkgs {
            inherit system;
            allowBroken = true;
            allowUnfree = true;
            overlays = [ ];
          };

          naersk-lib = naersk.lib."${system}";
        in
        {
          # `nix build`
          packages = {
            beancount-language-server-git = naersk-lib.buildPackage {
              pname = "beancount-language-server-git";
              version = "master";
              root = ./.;
            };
          };

          #defaultPackage = packages.beancount-language-server-git;

          # `nix run`
          #apps.beancount-language-server = flake-utils.lib.mkApp {
          #  drv = packages.beancount-language-server-git;
          #};

          #defaultApp = apps.beancount-language-server;

          # `nix develop`
          devShell = pkgs.mkShell {
            nativeBuildInputs = with pkgs; [
              rustc
              cargo
              rustfmt
              clippy
              nodejs-16_x
              python310
            ];
          };
        }
      );
}
