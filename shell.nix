let
  pkgs = import <nixpkgs> {};
in
pkgs.mkShell {
  nativeBuildInputs = with pkgs; [
    rustc
    cargo
    gcc
  ];
  buildInputs = with pkgs; [
    rustfmt
    clippy
  ];

  RUST_SRC_PATH = "${pkgs.rust.packages.stable.rustPlatform.rustLibSrc}";
}
