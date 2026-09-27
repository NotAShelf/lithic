# Usage

Run `lithic` with no arguments to open the app. Run it with a command, such as
`lithic list`, to use it from a terminal. Both work on the same data, so an
instance you create in one shows up in the other.

## How lithic organises things

An **instance** is one way of playing: a name, a game version, a data folder and
the mods in it. The data folder is what the game calls its data path. It holds
your worlds, settings, mod configs and the `Mods` folder. Lithic starts the game
with `--dataPath` pointing at it, so instances never see each other's mods or
saves.

A **game version** is an installed copy of Vintage Story. Several instances can
share one. Lithic downloads official builds, or you can register a copy you
installed yourself.

One instance is **selected**. Mod commands act on it unless you name another,
and the app uses it as the default place to install mods.

## The app

<!--markdownlint-disable MD033-->

<p align="center">
  <img alt="Browsing the ModDB in lithic" src="./assets/browse.png" width="850px">
</p>

<!--markdownlint-enable MD033-->

**Instances** lists your instances with their game version, mod count and play
time. Press Play to start one, or open it to see three tabs:

- _Mods_ shows what is installed. Untick a mod to turn it off without deleting
  it. "Check for updates" asks the ModDB for newer releases that fit the
  instance's game version, and "Update all" installs them. Pin a mod to keep it
  on its current version. A yellow panel appears when a mod needs something that
  is missing or too old, with a button to install it.
- _Logs_ shows the game's output from each launch and the game's own log files.
  While the game runs, "Follow" keeps the view at the end.
- _Settings_ changes the name, game version, account, game arguments,
  environment variables, a program to start the game through (such as
  `gamemoderun`) and an extra mods folder.

The buttons under the instance name open its folder, copy it, export it as a
pack, or delete it.

**Browse mods** searches the ModDB. By default it only shows mods with releases
for the game version of the instance you are installing into; turn that off to
see everything. The download button installs a mod into the selected instance.
Use Details to read its description, save it as a favourite, or install a
specific release. Installing a specific release pins the mod to it.

**Game versions** installs official builds, shows which instances use each
version, and registers copies you installed yourself.

**Accounts** signs you in. With an account, the game starts already logged in.
Each instance can use its own account; otherwise it uses the active one.

Account sessions go to the system keyring (Secret Service on Linux, Keychain on
macOS, Credential Manager on Windows). If none is available, lithic stores them
in a file only your user can read.

## Commands

Everything below also has a `--json` form for scripts. Commands that delete
things ask first; pass `--yes` to skip the question.

Set up an instance and a game version:

```sh
lithic game install latest
lithic instance create "Survival" --game latest --select
```

Or keep using the folder the official launcher created:

```sh
lithic instance adopt --game 1.21.5
```

Find and install mods. Dependencies come along automatically:

```sh
lithic search carry on
lithic info carryon
lithic install carryon expandedfoods
lithic install carryon@1.13.0        # this exact version, pinned
lithic list
lithic mods check                    # missing or outdated dependencies
```

Keep them current:

```sh
lithic update --check    # what would change
lithic update            # update everything
lithic update carryon    # just one mod
lithic mods pin carryon  # stay on the installed version
lithic mods disable carryon
```

Work on an instance other than the selected one with `-i`:

```sh
lithic -i creative list
lithic instance select creative
```

Play:

```sh
lithic launch            # waits for the game to close and records play time
lithic launch --dry-run  # prints the command without running it
lithic logs              # output of the last launch
lithic logs --game       # the game's own client-main.log
```

Accounts and packs:

```sh
lithic account login
lithic instance edit --account "YourPlayerName"
lithic pack export -o survival.zip --config
lithic pack import survival.zip --name "Friend's pack"
```

`lithic settings show` lists the settings you can change with
`lithic settings set <key> <value>`, for example turning on backups of replaced
mods with `lithic settings set backups.enabled true`. The colours of the `list`
and `search` tables can be changed with `lithic settings table`.

Shell completions come from `lithic completions bash` (or `zsh`, `fish`,
`powershell`, `elvish`).

## Where files live

| What                                 | Linux                   |
| ------------------------------------ | ----------------------- |
| `settings.toml`, `accounts.toml`     | `~/.config/lithic`      |
| Instances and downloaded game builds | `~/.local/share/lithic` |
| Mod list cache and downloads         | `~/.cache/lithic`       |

On macOS these are under `~/Library/Application Support` and `~/Library/Caches`;
on Windows under `%APPDATA%` and `%LOCALAPPDATA%`. `lithic settings paths`
prints the exact locations. You can move each one with the `LITHIC_CONFIG_DIR`,
`LITHIC_DATA_DIR` and `LITHIC_CACHE_DIR` environment variables, which also makes
a portable setup possible.

Inside an instance folder:

```text
instance.toml    the instance's settings
data/            the game's data path, unless you chose another folder
data/Mods/       the mods the game loads
disabled-mods/   mods you turned off
mods.json        where each mod came from, and pins
logs/            output of the last ten launches
```

Lithic never deletes a data folder you chose yourself, such as an adopted
`VintagestoryData`. Deleting that instance only removes lithic's record of it.

## Coming from lithic 1.x

The first time lithic 2 starts, it converts your old `config.toml`. Instances
keep their folders, play time, account, game version, launch options and pinned
mods. Registered game builds, accounts, backup settings, table colours, theme
and favourites carry over, and logins keep working. Mods that 1.x linked in from
a modpack are replaced with real copies. The app shows a summary of what
changed; the command line prints it.

The old file stays next to the new ones as `config.toml.v1`. Settings that no
longer exist are listed in the summary. Modpacks built with 1.x can be imported
with `lithic pack import`.

If the old file cannot be read, lithic leaves it alone and says so. Fix or move
it and start lithic again.

## When something goes wrong

If the game closes with an error, lithic shows the last lines of its output and
the path to the crash report if the game wrote one. The Logs tab and
`lithic logs` show the full output.

If moving a mod or migrating an instance fails because the destination refuses
the rename, lithic leaves the source in place. It only copies files when the
source and destination are on different filesystems.

`LITHIC_LOG=debug lithic ...` prints detailed logs to the terminal, for both the
app and commands. The app writes nothing to a log file of its own.
