{
  inputs = {
    nixpkgs.url = "https://channels.nixos.org/nixos-unstable/nixexprs.tar.zst";
    crane.url = "github:ipetkov/crane";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = {
    self,
    nixpkgs,
    rust-overlay,
    crane,
  }: let
    systems = [
      "x86_64-linux"
      "aarch64-linux"
      "aarch64-darwin"
    ];
    forAllSystems = nixpkgs.lib.genAttrs systems;
    pkgsForEach = system: nixpkgs.legacyPackages.${system}.extend rust-overlay.overlays.default;
    mkCraneLib = pkgs:
      (crane.mkLib pkgs).overrideToolchain (
        p:
        # We use the rust-overlay to get the stable Rust toolchain for various targets.
        # This is not exactly necessary, but it allows for compiling for various targets
        # with the least amount of friction. Using a rust-toolchain.toml also allows us
        # to provide a stable toolchain for non-NixOS users as well.
        (p.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml)
      );
  in {
    devShells = forAllSystems (
      system: let
        pkgs = pkgsForEach system;
      in {
        default = pkgs.callPackage ./nix/shell.nix {inherit self;};
      }
    );

    packages = forAllSystems (
      system: let
        pkgs = pkgsForEach system;
        craneLib = mkCraneLib pkgs;
      in {
        lithic = pkgs.callPackage ./nix/package.nix {inherit craneLib;};
        default = self.packages.${system}.lithic;
      }
    );

    checks = forAllSystems (system: let
      pkgs = pkgsForEach system;
      craneLib = mkCraneLib pkgs;
      src = craneLib.cleanCargoSource ./.;
    in {
      default = self.packages.${system}.default;

      # Check formatting
      my-crate-fmt = craneLib.cargoFmt {
        inherit src;
      };

      # Check TOML formatting
      my-crate-toml-fmt = craneLib.taploFmt {
        src = pkgs.lib.sources.sourceFilesBySuffices src [".toml"];
      };

      # Audit licenses
      my-crate-deny = craneLib.cargoDeny {
        inherit src;
      };
    });
  };
}
