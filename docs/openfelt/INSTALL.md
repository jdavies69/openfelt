# Install and upgrade OpenFelt

Tagged releases produce archives, shell and PowerShell installers, and checksums from the tagged source through the repository's `release.yml` workflow. Supported build targets are Linux x86_64, Windows x86_64, macOS Apple Silicon, and macOS Intel.

## Install a release

1. Open the matching entry on [GitHub Releases](https://github.com/jdavies69/openfelt/releases).
2. Confirm the Release workflow succeeded for the tag and download the archive or installer for your platform plus its checksum file.
3. Verify the checksum using the command printed on the release page. On macOS or Linux this normally uses `shasum -a 256`; on Windows use `Get-FileHash -Algorithm SHA256`.
4. Extract the archive and place `openfelt` (or `openfelt.exe`) somewhere on `PATH`, or run the generated installer.
5. Run `openfelt --version`, then `openfelt`. The first launch opens local setup; no API key or paid request is required.

macOS may require an explicit first-launch approval for an unsigned downloaded binary. Linux desktop key storage requires an available Secret Service/keyring session. When native credential storage is unavailable, local play still works and cloud credentials can be supplied for one session through the provider environment variable.

Use a terminal of at least 80 columns by 30 rows. Complete one local hand before enabling a cloud provider. A release is only described as clean-install verified after the platform walkthrough records launch, setup, one completed hand, key save/forget where available, and terminal restoration after quit.

## Upgrade or remove

Download and verify the newer tagged artifact, then replace the prior executable. Saved settings and learning history remain in the platform local-data directory and are not bundled with the program. Stored API keys remain in the operating-system credential store under service `dev.openfelt.coaching` until you use **Forget key** in Settings or remove them with the platform credential manager.

Removing the executable does not remove local history or keychain entries. Back up or delete the local `openfelt` data directory separately if that is your intent.

## Build from source

With a current stable Rust toolchain:

```sh
git clone https://github.com/jdavies69/openfelt.git
cd openfelt
cargo install --locked --path . --bin openfelt
openfelt
```

Source builds are distinct from the tagged binary provenance above. CI build success is evidence of compilation and automated tests; it is not a claim that a clean-install walkthrough was performed on that machine.
