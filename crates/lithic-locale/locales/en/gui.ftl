## Window and navigation

window-title = Lithic
window-title-instance = { $name } - Lithic
nav-instances = Instances
nav-browse = Browse mods
nav-games = Game versions
nav-accounts = Accounts
nav-settings = Settings
sidebar-signed-in = Signed in as { $name }
sidebar-signed-out = Not signed in
sidebar-running = { $count ->
    [one] 1 game running
   *[other] { $count } games running
}
loading = Loading
load-failed = Could not read lithic's data

## Shared words

common-back = Back
common-cancel = Cancel
common-change = Change
common-choose-folder = Choose folder
common-close = Close
common-ok = OK
common-open = Open
common-open-folder = Open folder
common-refresh = Refresh
common-remove = Remove
common-reset = Reset
common-retry = Try again
common-revert = Revert
common-save = Save
common-saving = Saving

toast-dismiss = Dismiss
toast-show-details = Details
toast-hide-details = Hide

## Progress

step-starting = Starting
step-resolving = Looking up mods
step-downloading = Downloading
step-downloading-item = Downloading { $item }
step-verifying = Checking the download
step-extracting = Unpacking
step-installing = Installing
step-cleaning = Tidying up
op-already-running = Something is already running for this. Wait for it to finish.
op-cancelled = Cancelled
op-failed = Something went wrong with { $name }
open-failed = Could not open that

## Launching

launch-failed = The game could not be started
launch-exited = { $name } closed with error code { $code }
launch-lost = Lost track of { $name } while it was running
launch-crash-report = Crash report: { $path }

quit-title = Quit while games are running?
quit-body = Games you started keep running, but lithic will not record how long you play them. Downloads in progress are abandoned.
quit-confirm = Quit

link-title = Install from the ModDB
link-mod = Install { $mod }?
link-mod-version = Install { $mod } { $version }?
link-into = Into
link-no-instances = Create an instance first, then use the install button on the ModDB again.

## Migration from 1.x

migrated-title = Your lithic setup was moved
migrated-body = This version of lithic stores things differently, so your old settings were converted. Here is what happened:
migrated-backup = Your old configuration is kept at { $path }
migration-failed-title = Your old settings could not be converted
migration-failed-body = Lithic will keep working, but your instances from the previous version are not here yet. Fix or move the file below and restart lithic.

## Instances

instances-new = New instance
instances-import = Import pack
instances-import-pick = Choose a pack to import
instances-importing = Importing pack:
instances-adopt = Use my existing game data
instances-empty-title = No instances yet
instances-empty-body = An instance is a separate game setup with its own mods, settings and worlds. Create one to get started, or use the game data you already have.
instances-broken = The instance { $id } could not be read: { $error }
instances-game = Vintage Story { $version }
instances-no-game = No game version set
instances-game-missing = Game not installed
instances-played = Played { $total }, last on { $when }
instances-never-played = Not played yet
instances-selected = Selected
instances-running = Running
instances-play = Play
instances-starting = Starting
instances-stop = Stop
instances-create-title = New instance
instances-create = Create
instances-creating = Creating
instances-name = Name
instances-name-placeholder = My modded world
instances-game-version = Game version
instances-pick-game = Choose a version
instances-version-not-installed = { $version } (will be installed)
instances-will-install = Vintage Story { $version } will be downloaded after the instance is created.
instances-releases-unavailable = The list of game releases could not be loaded. You can set the version later.
instances-data-folder = Game data folder
instances-data-default = A new folder inside the instance
instances-data-hint = Pick an existing folder, such as VintagestoryData, to keep using its worlds and settings. Lithic never deletes a folder you chose.
instances-pick-data = Choose the game data folder

## One instance

instance-back = Back to instances
instance-tab-mods = Mods
instance-tab-mods-count = Mods ({ $count })
instance-tab-logs = Logs
instance-tab-settings = Settings
instance-open-folder = Open folder
instance-clone = Copy
instance-export = Export pack
instance-delete = Delete
instance-select = Make selected
instance-game-not-installed = Vintage Story { $version } (not installed)
instance-no-game-warning = This instance has no game version, so mods are not checked for compatibility and it cannot be started. Set one in the Settings tab.
instance-filter = Filter mods
instance-add-mods = Add mods
instance-open-mods = Open mods folder
instance-check-updates = Check for updates
instance-checking = Checking
instance-update-all = Update all ({ $count })
instance-updating = Updating
instance-up-to-date = Every mod is up to date
instance-update-check-failed = Could not check for updates
instance-update = Update
instance-update-to = { $version } available
instance-pin = Pin
instance-unpin = Unpin
instance-pinned = Pinned at { $version }
instance-dependency = Dependency
instance-no-metadata = No mod info
instance-unreadable = Unreadable
instance-problems = Some mods may not load
instance-fix-install = Install it
instance-no-mods-title = No mods installed
instance-no-mods-body = Browse the ModDB to find mods for this instance. Missing dependencies are installed for you.
instance-change-failed = That change could not be made
instance-remove-title = Remove mod
instance-remove-body = Remove { $name }? Dependencies that nothing else needs are removed with it.
instance-remove-body-deps = Remove { $name }? { $dependents } will stop working without it.
instance-removed-mods = Removed { $names }
instance-remove-failed = Could not remove the mod
instance-delete-title = Delete { $name }?
instance-delete-body = Everything in { $path } is deleted, including saved worlds. This cannot be undone.
instance-delete-body-external = The instance is removed from lithic. Its game data in { $path } is kept.
instance-delete-busy = Stop the game and wait for downloads to finish first.
instance-delete-failed = Could not delete the instance
instance-clone-title = Copy instance
instance-copy-name = { $name } (copy)
instance-clone-saves = Copy saved worlds too
instance-cloned = Created { $name }
instance-clone-failed = Could not copy the instance
instance-export-title = Export pack
instance-export-body = A pack lists this instance's mods so others can recreate it. Mods on the ModDB are downloaded when the pack is imported; others are put in the pack itself.
instance-export-config = Include mod settings
instance-export-bundle = Put every mod file in the pack (larger, but works offline)
instance-export-save = Choose where to save
pack-exported = Pack saved to { $path }
instance-no-logs = No logs yet
instance-no-logs-body = Logs appear here once the game has been started.
instance-follow-log = Follow
instance-launch-log = Game output, { $when }
instance-log-failed = Could not read the log
instance-account = Account
instance-account-active = Active account ({ $name })
instance-account-none = No account (the game will ask)
instance-account-hint = The account the game signs in with.
instance-args = Game arguments
instance-args-hint = Passed to the game when it starts. Quote values that contain spaces.
instance-bad-args = The game arguments could not be read: { $error }
instance-env = Environment variables
instance-env-hint = One KEY=value per line.
instance-bad-env = "{ $line }" is not KEY=value
instance-wrapper = Start through
instance-wrapper-hint = A program the game is started with, such as gamemoderun or prime-run.
instance-bad-wrapper = The command could not be read: { $error }
instance-mods-dir = Extra mods folder
instance-mods-dir-default = None, mods live in the data folder
instance-mods-dir-hint = Lithic installs mods here and the game also loads them from here.
instance-pick-mods = Choose a mods folder
instance-data-dir = Game data folder
instance-data-internal = Kept inside the instance and deleted with it.
instance-data-external = A folder you chose. Deleting the instance leaves it alone.
instance-name-required = The instance needs a name
instance-saved = Saved

## Mods

mods-count = { $count ->
    [one] 1 mod
   *[other] { $count } mods
}
mods-installed = { $count ->
    [one] Installed 1 mod in { $name }
   *[other] Installed { $count } mods in { $name }
}
mods-updated = { $count ->
    [one] Updated 1 mod in { $name }
   *[other] Updated { $count } mods in { $name }
}
mods-already-installed = Already installed
mods-some-failed = { $count ->
    [one] 1 mod could not be installed
   *[other] { $count } mods could not be installed
}
mods-needed-by = needed by { $name }

problem-missing = { $mod } needs { $dep }, which is not installed
problem-missing-version = { $mod } needs { $dep } { $version } or newer, which is not installed
problem-outdated = { $mod } needs { $dep } { $version } or newer, but { $installed } is installed
problem-disabled = { $mod } needs { $dep }, which is turned off
problem-duplicate = { $mod } is installed more than once: { $files }
problem-unreadable = { $file } could not be read: { $error }

## Browse

browse-search = Search mods
browse-install-into = Install into
browse-pick-instance = Choose an instance
browse-compatible = Only compatible mods
browse-compatible-with = Only mods for { $version }
browse-favorites-only = Favourites only
browse-hide-installed = Hide installed mods
browse-loading = Loading mods from the ModDB
browse-failed = The ModDB could not be reached
browse-no-results = No mods found
browse-no-results-body = Try other words, or turn off the filters.
browse-no-instances = Create an instance first; mods are installed into one.
browse-no-instance = Choose an instance to install into
browse-installed-failed = Could not read the installed mods
browse-favorite-failed = Could not save your favourites
browse-result-count = { $count ->
    [one] 1 mod
   *[other] { $count } mods
}
browse-more = Show more ({ $count } left)
browse-by = by { $author }
browse-mod-heading = Mod
browse-downloads-heading = Downloads
browse-downloads = { $count } downloads
browse-side = { $side } only
browse-install = Install
browse-installed = Installed { $version }
browse-update-available = Update available
browse-update-to = Update to { $version }
browse-favorite = Favourite
browse-unfavorite = Unfavourite
browse-details = Details
browse-open-page = Open on the ModDB
browse-source = Source code
browse-issues = Issue tracker
browse-wiki = Wiki
browse-homepage = Homepage
browse-would-install = Version { $version } fits this instance
browse-releases = Releases
browse-install-version = Install this version
browse-pin-hint = Installing a specific version pins the mod to it, so updates leave it alone until you unpin it.
browse-no-game-versions = No game versions listed
browse-version-range = { $from } to { $to }
browse-sort-relevance = Best match
browse-sort-downloads = Most downloaded
browse-sort-trending = Trending
browse-sort-updated = Recently updated
browse-sort-follows = Most followed
browse-sort-name = Name

## Game versions

games-installed = Installed
games-none = No game versions installed yet.
games-by-lithic = Installed by lithic
games-by-you = Added by you
games-missing = Files missing
games-unused = Not used by any instance
games-used-by = Used by { $names }
games-remove-title = Remove Vintage Story { $version }?
games-remove-managed = Its files in { $path } are deleted.
games-remove-external = Lithic forgets this install. Its files in { $path } are kept.
games-remove-failed = Could not remove the game version
games-install-title = Install a version
games-install-hint = Downloads the official build for this computer and checks it before unpacking.
games-pick-version = Choose a version
games-install = Install
games-installing-short = Installing
games-installing = Installing Vintage Story { $version }
games-working = Working
games-show-unstable = Show pre-releases
games-download-size = Download size: { $size }
games-list-unavailable = The list of game releases could not be loaded.
games-unsupported-os = There are no Vintage Story builds for this operating system.
game-installed = Vintage Story { $version } is installed
games-add-title = Use an existing install
games-add-hint = Point lithic at a Vintage Story folder you installed some other way.
games-add-no-folder = No folder chosen
games-add-pick = Choose the Vintage Story folder
games-add-version = Version, such as 1.21.5
games-add = Add
games-adding = Adding
games-added = Added Vintage Story { $version }
games-add-failed = Could not add that install

## Accounts

accounts-none = No accounts yet. Sign in below so the game starts already logged in.
accounts-active = Active
accounts-session-missing = Sign in again
accounts-make-active = Make active
accounts-logout = Sign out
accounts-logout-title = Sign out?
accounts-logout-body = { $name } is removed from lithic and its stored session is deleted.
accounts-login-title = Sign in to Vintage Story
accounts-login-hint = Use the email and password of your account at vintagestory.at.
accounts-email = Email
accounts-password = Password
accounts-login = Sign in
accounts-logging-in = Signing in
accounts-code-title = Two-factor code
accounts-code-hint = Enter the code from your authenticator app.
accounts-verify = Verify
accounts-logged-in = Signed in as { $name }
accounts-change-failed = That change could not be made

## Settings

settings-appearance = Appearance
settings-theme = Theme
settings-theme-preset = Preset
settings-theme-preset-pick = Choose a preset
settings-start-page = Open on
settings-mods = Mods
settings-prerelease = Offer pre-release mod versions
settings-prerelease-hint = When off, versions like 2.0.0-dev.3 are only used if a mod has nothing else for your game version.
settings-concurrency = Parallel downloads
settings-backups = Keep a copy of mods before replacing or removing them
settings-backups-hint = Useful if an update breaks something and you want the old file back.
settings-backups-keep = Copies to keep per mod
settings-backups-dir = Backup folder
settings-storage = Storage
settings-game-dir = Game versions folder
settings-game-dir-hint = Where downloaded game versions are unpacked. Existing installs stay where they are.
settings-path-config = Settings and accounts
settings-path-data = Instances and games
settings-path-cache = Downloads and caches
settings-save-failed = Could not save settings

theme-system = Follow the system
theme-light = Light
theme-dark = Dark
theme-preset = Preset
