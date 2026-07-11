# Usage

## CLI Usage

> [!TIP]
> You can also use `lithic-gui` from the command line to launch the graphical
> interface.

<!--markdownlint-disable MD013-->

```text
An extremely fast mod manager for Vintage Story, written in Rust.

Usage: lithic-cli [OPTIONS] <COMMAND>

Commands:
  sync          Checks with the VintageStory mods website for any updates to mods you have installed. Run update after this command to update your mods
  list          List installed mods and their versions and any missing dependencies. Running sync first will show any available updates to your mods
  update        Updates a specific mod OR all mods installed. Runs sync after completion
  install       Install a specific mod. Must use the mod_id, Example: ./Lithic install alchemy
  search        Search the mod website for new mods, Example: ./Lithic search -q magic
  config        Manage config options for Lithic
  misc          Miscellaneous items for Lithic, like shell auto-completion and 1-click mod installation
  download      Download a Vintage Story executable
  info          Get more information about the mod specified
  modpack       Create, download, update modpacks for VintageStory
  self          Manage the Lithic binary; Check for updates, perform updates.
  delete        Remove mods and backups
  instance      Manage game instances
  game-version  Manage installed game versions
  launch        Launch Vintage Story using the selected instance
  help          Print this message or the help of the given subcommand(s)

Options:
  -v, --verbose
  -d, --debug                Shows all logging messages. This is EXTREMELY noisy. Only run this if you have to
  -m, --mods-dir <MODS_DIR>  Specify the directory to manage mods. This takes priority over any other directory setting, including from the config file
  -w, --with-mpk <WITH_MPK>  This command will set the working mod directory to be that of the modpack specified, INCLUDING modpacks you create. If you use this to work on a custom m
odpack, you will need to run Lithic modpack create again to update your modpack file, just set the --mpk-id to the same one you used before to overwrite the old one
  -h, --help                 Print help
  -V, --version              Print version
```

<!--markdownlint-enable MD013-->

### Common workflows

```bash
# Sync your mod list with the mod database
$ lithic-cli sync

# List installed mods
$ lithic-cli list

# Search for mods
$ lithic-cli search -q magic

# Install a mod
$ lithic-cli install alchemy

# Update all mods
$ lithic-cli update

# Get info about a mod
$ lithic-cli info alchemy

# Launch the game
$ lithic-cli launch

# Generate shell completions
$ lithic-cli misc --gen-auto-complete bash
```

## GUI Usage

Launch the graphical interface from your application menu or terminal:

```bash
lithic-gui
```

The GUI provides a point-and-click interface for:

- **Browse** – Search and discover mods from the Vintage Story mod database.
- **Manage** – Enable, disable, and remove installed mods.
- **Update** – See which mods have updates available and apply them.
- **Modpacks** – Create and manage modpack collections.
- **Instances** – Switch between different game instances and configurations.
