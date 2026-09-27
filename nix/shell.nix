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
  cairo,
  gtk3,
  alsa-lib,
  libpulseaudio,
  pipewire,
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
    cairo
    gtk3
    alsa-lib
    libpulseaudio
    pipewire
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
