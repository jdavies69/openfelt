# Install and upgrade OpenFelt

Tagged releases produce archives, shell and PowerShell installers, and checksums from the tagged source through the repository's `release.yml` workflow. Supported build targets are Linux x86_64, Windows x86_64, and macOS Apple Silicon. Intel macOS is deprecated and receives no release artifact. A release install does not need Rust, Cargo, or a source checkout.

## Install a release

These commands download the installer for the latest stable release, copy `openfelt` into a user-writable directory, and add that directory to `PATH` if it is not already there. Existing `PATH` entries are left in place. Running the installer again replaces the executable and does not add a second `PATH` line.

macOS and Linux:

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/jdavies69/openfelt/releases/latest/download/openfelt-installer.sh | sh
```

Windows PowerShell:

```powershell
irm https://github.com/jdavies69/openfelt/releases/latest/download/openfelt-installer.ps1 | iex
```

The default directory is `~/.local/bin` on macOS and Linux, and `%USERPROFILE%\.local\bin` on Windows. Set `OPENFELT_HOME` before running the installer to use `$OPENFELT_HOME/bin` instead. These locations do not need an administrator account. If the directory cannot be created, the installer stops and leaves any existing copy alone.

The installer cannot change the `PATH` of the terminal that is already open. Close it, open a new terminal, and run both commands from any directory:

```sh
openfelt --version
openfelt
```

`openfelt --version` prints the installed release. The first launch opens local setup. No API key or paid request is required.

On macOS, Terminal starts zsh and the installer records the directory in `.zshrc` and `.zshenv`. On Linux bash, it records the directory in the bash startup files that already exist, plus `.profile`. On Windows, the PowerShell installer adds the directory to the user `Path`. If automatic setup cannot edit those files, the installer prints the `source` command, or the directory to add by hand.

macOS may ask you to approve an unsigned downloaded binary the first time it runs. Linux cloud-key storage needs an available Secret Service session. Local play still works when that store is unavailable.

Use a terminal of at least 80 columns by 30 rows.

## Upgrade

Between hands, press `S` and move to **Updates**. The row shows the installed version. Enter checks stable releases on `jdavies69/openfelt`. Prereleases and older versions are ignored. Checking does not download the app or install anything.

When a newer release exists, Settings shows the version and the release-notes link. Enter again downloads that platform's `.tar.gz` archive and its published `.sha256` file, checks the checksum, and only then replaces the executable. Quit, open a new terminal, and run `openfelt --version`. The new binary should report the release you installed.

The check sends the installed version's user agent and downloads the release list, the matching archive, and the checksum. It does not send coaching keys, settings, or hand history.

Settings replaces only a copy installed in `~/.local/bin` or under `$OPENFELT_HOME/bin` (a path ending in `.openfelt/bin`). Other copies stay where they are and Settings explains how to upgrade them:

- Cargo: `cargo install --locked --git https://github.com/jdavies69/openfelt.git --bin openfelt`
- This repository: `cargo install --locked --path . --bin openfelt` after updating the checkout
- A package manager: use that package manager

If the download, checksum, permissions, or replacement fails, the previous executable is restored or left untouched. Saved settings, learning progress, hand history, and bookmarks stay in the local data directory. On macOS, API keys prefer the Data Protection keychain as local internet passwords for `api.openai.com` / `api.anthropic.com` (no iCloud sync); CLI builds may fall back to the login keychain under service `dev.openfelt.coaching`. On other platforms they remain in the OS credential store under service `dev.openfelt.coaching`.

You can also rerun the installer from this page. It follows the same user directory and does not remove personal data.

## Uninstall

Removing the program does not remove personal data.

1. Delete the executable: `~/.local/bin/openfelt`, `%USERPROFILE%\.local\bin\openfelt.exe`, or `$OPENFELT_HOME/bin/openfelt` if you set that variable.
2. Leave `~/.local/bin` on `PATH` when other tools use it. To remove only the installer hook, delete the single line that sources `~/.local/bin/env` from `.zshrc`, `.zshenv`, `.bashrc`, or `.profile`, and delete `~/.local/bin/env` only when no other program shares that file. On Windows, remove that one directory from the user `Path` if you added it and nothing else needs it.
3. Optional: `openfelt --stats` prints the data directory. Delete that folder to remove settings, history, and bookmarks. Use **Forget key** in Settings, or the operating-system credential manager, to remove API keys. Uninstall does not do either of those.

## Build from source

With a current stable Rust toolchain:

```sh
git clone https://github.com/jdavies69/openfelt.git
cd openfelt
cargo install --locked --path . --bin openfelt
openfelt
```

A source or Cargo install is not replaced by Settings. CI compilation is not a clean-install walkthrough. Treat a platform as install-verified only after a new terminal on a clean machine can run `openfelt` and `openfelt --version` for that published release.
