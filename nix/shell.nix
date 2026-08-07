{
  self,
  lib,
  stdenv,
  mkShell,
  clang,
  libclang,
  mold,
  pkg-config,
  taplo,
  libxkbcommon,
  vulkan-loader,
  wayland,
  libx11,
  libxcursor,
  libxi,
  libxrandr,
}: let
  lithicPkg = self.packages.${stdenv.hostPlatform.system}.lithic;
  runtimeInputs = lib.makeLibraryPath [
    libxkbcommon
    vulkan-loader
    wayland
    libx11
    libxcursor
    libxi
    libxrandr
  ];
in
  mkShell {
    name = "lithic-dev";
    inputsFrom = [lithicPkg];

    nativeBuildInputs = [
      clang
      mold
      pkg-config
      taplo
    ];

    env = {
      LIBCLANG_PATH = "${libclang.lib}/lib";
      LD_LIBRARY_PATH = "$LD_LIBRARY_PATH:${runtimeInputs}";
    };
  }
