# Installation

There are two ways to install Lithic on your system.

## With Nix

Nix is the recommended way of downloading (and developing!) Lithic. You can
install it using Nix flakes using `nix profile add` if on non-nixos or add
Lithic as a flake input if you are on NixOS or Darwin.

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

If you want to give Lithic a try before you switch to it, you may also run it
one time with `nix run`.

```sh
# Run directly from the git repository; will be garbage collected
$ nix run github:NotAShelf/lithic # start the watch daemon
```

## Without Nix

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

### Windows

For Windows, download the `lithic-windows-x86_64.zip` archive from
[GitHub Releases], extract it with File Explorer, and run `lithic-gui.exe` for
the graphical interface. To use the CLI, open a terminal in the extracted folder
and run:

```powershell
.\lithic-cli.exe --help
```

You do not need to add the folder to `PATH` unless you want to run Lithic from
other directories.
