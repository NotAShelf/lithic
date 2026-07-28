{
  lib,
  craneLib,
  clang,
  libclang,
  mold,
  pkg-config,
  openssl,
  libxkbcommon,
  wayland,
  vulkan-loader,
  makeWrapper,
}: let
  cargoTOML = (lib.importTOML ../Cargo.toml).workspace.package;
  pname = "lithic";
  version = cargoTOML.version;

  runtimeInputs = [
    libxkbcommon
    vulkan-loader
    wayland
  ];

  buildInputs = runtimeInputs ++ [openssl.dev];
  nativeBuildInputs = [
    clang
    mold
    pkg-config
    makeWrapper
  ];

  depsSrc = craneLib.cleanCargoSource ../.;
  commonArgs = {
    inherit pname version buildInputs nativeBuildInputs;
    strictDeps = true;
    doCheck = false;
    src = depsSrc;

    env = {
      LIBCLANG_PATH = lib.makeLibraryPath [libclang.lib];
    };
  };

  # Pre-build all external deps, this derivation is cached across source changes
  cargoArtifacts = craneLib.buildDepsOnly commonArgs;

  # Build source that includes locale '.ftl' files required by lithic-locale's
  # include_str! macros. We keep them out of depsSrc so that touching a
  # translation does not invalidate the cargoArtifacts cache.
  buildSrc = let
    fs = lib.fileset;
    s = ../.;
  in
    fs.toSource {
      root = s;
      fileset = fs.unions [
        (fs.fileFilter (file: builtins.any file.hasExt ["ftl"]) (s + /crates))
        (s + /crates)
        (s + /packages)
        (s + /Cargo.toml)
        (s + /Cargo.lock)
      ];
    };
in
  craneLib.buildPackage (
    commonArgs
    // {
      inherit cargoArtifacts;
      src = buildSrc;
      useNextest = true;

      postFixup = ''
        for bin in $out/bin/*; do
          wrapProgram "$bin" \
            --prefix LD_LIBRARY_PATH : ${lib.makeLibraryPath runtimeInputs}
        done
      '';

      meta = {
        description = "Fast, cross-platform mod manager for Vintage Story";
        homepage = "https://github.com/notashelf/lithic";
        license = lib.licenses.mpl20;
        maintainers = [lib.maintainers.NotAShelf];
        platforms = lib.platforms.linux ++ lib.platforms.darwin;
        mainProgram = "lithic";
      };
    }
  )
