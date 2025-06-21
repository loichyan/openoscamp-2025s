{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
      rust-overlay,
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ rust-overlay.overlays.default ];
        };
        inherit (pkgs) lib mkShellNoCC rust-bin;

        # Rust toolchain for development
        rustupToolchain = (lib.importTOML ./rust-toolchain.toml).toolchain;
        rust-dev = rust-bin.fromRustupToolchain rustupToolchain;
        rust-dev-with-rust-analyzer = rust-dev.override (prev: {
          extensions = prev.extensions ++ [
            "rust-src"
            "rust-analyzer"
          ];
        });

        libPath = lib.makeLibraryPath (
          with pkgs;
          [
            # Required by numpy
            stdenv.cc.cc
            zlib
          ]
        );
      in
      {
        devShells.default = mkShellNoCC {
          packages = with pkgs; [
            rust-dev-with-rust-analyzer
            gnuplot
            python312
            uv
          ];
          shellHook = ''
            export LD_LIBRARY_PATH=${libPath}
          '';
        };
      }
    );
}
