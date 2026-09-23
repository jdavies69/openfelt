# Licensing and corresponding source

OpenFelt with its integrated postflop solver is distributed under **GNU Affero
General Public License version 3 or later**. See [LICENSE-AGPL](LICENSE-AGPL).
The original MIT-licensed code remains available under its original terms;
[LICENSE-MIT](LICENSE-MIT) preserves the original copyright and permission notice.
The combined program must be distributed in compliance with the AGPL.

## Included solver

- Project: [b-inary/postflop-solver](https://github.com/b-inary/postflop-solver)
- Upstream revision: `9d1509fe5077d019825f833eed04b16d342dfda1`
- License: AGPL-3.0-or-later
- Local source: `vendor/postflop-solver/`
- Modifications and maintenance notes: `vendor/postflop-solver/OPENFELT.md`

## Obtaining and rebuilding the source

The full maintained source is published at
https://github.com/jdavies69/openfelt. For a binary release, use the matching
release's `source.tar.gz` asset; it includes the vendored solver, lockfile,
license notices, application source and build scripts. Git tags identify releases.

With a current stable Rust toolchain installed, unpack that source archive and run:

```sh
cargo build --locked --release --bin openfelt
```

The resulting executable is `target/release/openfelt` (`openfelt.exe` on Windows).
Dependencies are fetched from the registries identified by `Cargo.lock`.
No API credentials are needed to build or use solver practice.

Anyone redistributing a modified combined program must provide the corresponding
source and retain the applicable notices. Network deployment of a modified AGPL
program must also satisfy the license's remote-user source requirements. This
repository provides the source; it does not grant an exception to those terms.

Other dependencies retain their own licenses, as recorded in their Cargo manifests.
