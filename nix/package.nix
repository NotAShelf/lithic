{
  lib,
  craneLib,
  pkg-config,
  openssl,
  libxkbcommon,
  wayland,
  vulkan-loader,
}: let
  cargoTOML = (lib.importTOML ../Cargo.toml).workspace.package;
  pname = "lithic";
  version = cargoTOML.version;

  nativeBuildInputs = [pkg-config];

  buildInputs = [
    openssl
    libxkbcommon
    wayland
    vulkan-loader
  ];

  commonArgs = {
    inherit pname version buildInputs nativeBuildInputs;
    strictDeps = true;
    doCheck = false;

    src = let
      fs = lib.fileset;
      s = ../.;
    in
      fs.toSource {
        root = s;
        fileset = fs.intersection (fs.fromSource (craneLib.cleanCargoSource s)) (
          fs.unions [
            (s + /crates)
            (s + /packages)

            (s + /Cargo.toml)
            (s + /Cargo.lock)
          ]
        );
      };
  };

  # Pre-build all external deps, this derivation is cached across source changes
  cargoArtifacts = craneLib.buildDepsOnly commonArgs;
in
  craneLib.buildPackage (
    commonArgs
    // {
      inherit cargoArtifacts;
      useNextest = true;

      meta = {
        description = "Fast, cross-platform mod manager for Vintage Story";
        homepage = "https://github.com/notashelf/lithic";
        license = lib.licenses.mit;
        maintainers = with lib.maintainers; [NotAShelf];
        platforms = lib.platforms.linux ++ lib.platforms.darwin;
        mainProgram = "lithic";
      };
    }
  )
