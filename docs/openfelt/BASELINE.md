# Verified baseline — September 21, 2026

OpenFelt derives from [ashxudev/terminal-poker](https://github.com/ashxudev/terminal-poker).
The independent public repository is [jdavies69/openfelt](https://github.com/jdavies69/openfelt).
Its initial main is exactly `2e6cfe7454476ecd292797bb1801e4a6b2f70fb2`, with upstream history and the original MIT copyright retained.
No affiliation or endorsement is implied. `upstream` points to the author; `origin` points to OpenFelt.

## Executed checks

macOS 27.0, arm64; Homebrew rustc 1.98.1 (48a229cea, 2026-09-01), Cargo 1.98.1 (797e8a9bc, 2026-08-05).

| Check | Actual result |
| --- | --- |
| Setup shell syntax + seven isolated mock scenarios | Passed |
| GitHub visibility / independence | isPrivate=false, isFork=false |
| Remote main SHA | Exact match to pinned baseline |
| cargo fmt --all -- --check | Passed |
| cargo clippy --locked --all-targets --all-features -- -D warnings | Passed |
| cargo test --locked --all-targets --all-features | 319 passed, 0 failed, 3 ignored across 35 test summaries |
| cargo build --locked --release --bin sneakyblinders --bin poker-server --bin poker | Passed |

An initial sandboxed dependency fetch failed DNS resolution and was interrupted; the completed lint and test checks ran with authorized network/localhost access. The manifest emits an upstream warning because poker and terminal-poker share src/main.rs.

## Terminal inspection

Executed `cargo run --locked --release --bin sneakyblinders` in an interactive macOS PTY at 120 columns × 40 rows. Enter opened local Quick Practice, displayed the nine-seat table, 1/2 blinds and passive bot calls. No host/join or author LAN connection was selected. Executed the original release `poker` in an 80×24 PTY: its heads-up table and single-letter action controls appeared, but the compact layout leaves very little vertical room for cards. These were PTY output inspections, not screenshots or native Terminal.app visual validation.

Source minimums: shell home 40×20; other shell routes 80×24. The nine-seat table benefits from a larger terminal. Real rendering across terminal fonts/emulators remains a release check.

## Re-verified source findings

- `Cargo.toml`: poker/terminal-poker use the old main; sneakyblinders is the newer shell. No OpenFelt executable exists in baseline main.
- `src/local_practice.rs`: generic table construction, nine-handed menu default, passive_action bot selection. Busts remove seats and there is no cash rebuy ledger.
- `src/game/multiway.rs`: two-to-nine-seat authoritative engine, configurable blinds, legal actions, separate side pots and unmatched returns.
- `src/training/observation.rs`: own-player observation checks acting seat and history revision; amount_to_call is uncapped, and pot odds use the entire pot. Do not use that ratio as general short-stack or side-pot coaching truth.
- `src/ui/input.rs` and newer shell: reusable action concepts, but the old controller accepts the heads-up state and is not a multiway trainer.
- Historical baseline: `.github/workflows/ci.yml` then declared a four-platform matrix including Intel macOS; only the local macOS results above were claimed. Intel macOS has since been deprecated. The current supported matrix is Linux x86_64, Windows x86_64, and macOS Apple Silicon. Release workflow is tag driven (also plans on PR); publish-crates is callable and expects a registry secret. No release tag or registry publishing was performed.

This is a focused integration audit, not a complete security or poker-strategy certification. Three ignored upstream tests remain ignored.
