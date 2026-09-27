# Installation

Lithic ships as three programs:

- `lithic` opens the app when started without arguments and runs commands
  otherwise. This is the one to use.
- `lithic-gui` only opens the app. On Windows it starts without a console
  window.
- `lithic-cli` only runs CLI commands.

> [!NOTE]
> Outside the Nix package, Lithic does not install what the game itself needs
> to run, such as the .NET runtime on Linux. You are encouraged to install
> this as a part per-distribution packaging steps.

There are two ways to install Lithic on your system:

## With Nix

Nix is the recommended way of downloading (and developing!) Lithic. You can
install it using Nix flakes using `nix profile add` if on non-nixos or add
Lithic as a flake input if you are on NixOS or Darwin:

```nix
{
  # Add Lithic to your inputs like so
  inputs.lithic.url = "github:NotAShelf/lithic";

  outputs = { /* ... */ };
}
```

Then you can get the package from your flake input, and add it to your packages
to make `lithic` available in your system.

```nix
{inputs, pkgs, ...}: let
  lithicPkg = inputs.lithic.packages.${pkgs.stdenv.hostPlatform.system}.lithic;
in {
  environment.systemPackages = [lithicPkg];
}
```

The Nix package wraps Lithic with .NET 8 and 10, so games launched through it
can find the runtime needed by Vintage Story 1.21 and 1.22. Older game versions
need other runtimes; see [Vintage Story's Linux installation guide](https://wiki.vintagestory.at/Installing_the_game_on_Linux).

If you want to give Lithic a try before you switch to it, you may also run it
one time with `nix run`.

```sh
# Run directly from the git repository; will be garbage collected
$ nix run github:NotAShelf/lithic # run
```

## Without Nix

The package includes a desktop entry, so lithic shows up in your application
menu and handles the install buttons on the ModDB website.

To try it without installing:

```sh
nix run github:NotAShelf/lithic
```

### Release archives

Tagged releases on [GitHub](https://github.com/notashelf/lithic/releases) have

[GitHub Releases]: https://github.com/notashelf/lithic/releases

You can also install Lithic on any of your systems _without_ using Nix. New
releases are made when a version gets tagged, and are available under
[GitHub Releases]. To install Lithic on your system without Nix, either:

- Download a tagged release from [GitHub Releases] for your platform and place
  the binary in your `$PATH`. Instructions may differ based on your distribution
  and operating system, but generally you want to download the built binary from
  releases and put it somewhere like `/usr/bin` or `~/.local/bin` depending on
  your distribution.
- Build and install from source with Cargo:

  ```bash
  # Lithic packages are available on crates.io, and can be installed with
  # `cargo install`
  $ cargo install lithic-cli --locked
  $ cargo install lithic-gui --locked
  ```

Additionally, you may get Lithic from source via `cargo install` using
`cargo install --git https://github.com/notashelf/lithic --locked -p lithic-cli`
or you may check out to the repository, and use Cargo to build it before
`install`ing the files to a directory part of your `PATH`. You'll need Rust
1.91.0 or above. Most distributions should package this version already. You
may, of course, prefer to package the built releases if you'd like.

On Windows, start `lithic-gui.exe` for the app. For commands, open a terminal in
the extracted folder:

```powershell
# Use the .exe on Windows.
$ .\lithic.exe --help
```

## Post-Installation

On Linux, run this once so the app appears in your menu and install buttons on
the ModDB open lithic:

```sh
# Installs the desktop file.
$ lithic desktop-entry
```

### From source

You need Rust 1.95 or newer.

```sh
# Clone the repository and navigate to it
$ git clone https://github.com/notashelf/lithic; cd lithic

# Install it from the crate source epath
$ cargo install --path packages/lithic --locked
```

> [!TIP]
> On Linux, building needs the development files for `libxkbcommon` and
> `wayland`, which most distributions package as `libxkbcommon-dev` and
> `libwayland-dev` or similar. At runtime the app loads the Wayland or X11
> libraries your desktop already has.
