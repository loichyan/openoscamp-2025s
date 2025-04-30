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
        inherit (pkgs)
          fetchurl
          lib
          mkShell
          rust-bin
          stdenv
          ;

        rust-dev = rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
        qemu-7_0_0 = nixpkgs-qemu-7_0_0.legacyPackages.${system}.qemu;
        # Use a prebuilt gnu toolchain, since nixpkgs does not support riscv64-embedded.
        riscv-embedded-toolchain = lib.optionals (system == "x86_64-linux") (
          stdenv.mkDerivation rec {
            pname = "riscv-embedded-toolchain";
            version = "14.2.0-3";
            src = fetchurl {
              url = "https://github.com/xpack-dev-tools/riscv-none-elf-gcc-xpack/releases/download/v${version}/xpack-riscv-none-elf-gcc-${version}-linux-x64.tar.gz";
              hash = "sha256-9XRBW2PxKwm900dSI6tJKkZdI4EGRskME6TDtnbINQM=";
            };
            nativeBuildInputs = [ pkgs.autoPatchelfHook ];
            sourceRoot = "xpack-riscv-none-elf-gcc-${version}";
            installPhase = ''
              runHook preInstall
              cp -r . $out
              runHook postInstall
            '';
          }
        );
      in
      {
        devShells.default = mkShell {
          packages = [
            pkgs.cargo-binutils
            qemu-7_0_0
            riscv-embedded-toolchain
            (rust-dev.override (prev: {
              extensions = prev.extensions ++ [ "rust-analyzer" ];
            }))
          ];
        };
        devShells.minimal = mkShell { packages = [ rust-dev ]; };
      }
    );
}
