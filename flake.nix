{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    nixpkgs-qemu-7_0_0.url = "github:nixos/nixpkgs/7cf5ccf1cdb2ba5f08f0ac29fc3d04b0b59a07e4";
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
      nixpkgs-qemu-7_0_0,
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
        inherit (pkgs) mkShell rust-bin;
        qemu-7_0_0 = nixpkgs-qemu-7_0_0.legacyPackages.${system}.qemu;
        rust-dev = rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
      in
      {
        devShells.default = mkShell {
          packages = [
            pkgs.cargo-binutils
            qemu-7_0_0
            (rust-dev.override (prev: {
              extensions = prev.extensions ++ [ "rust-analyzer" ];
            }))
          ];
        };
        devShells.minimal = mkShell { packages = [ rust-dev ]; };
      }
    );
}
