# OpenFelt development-build validation

September 21, 2026. Local host: macOS 27.0 arm64, Homebrew Rust/Cargo 1.98.1. See [baseline provenance](BASELINE.md) for the unmodified upstream results.

## Final implementation checks

| Executed check | Result |
| --- | --- |
| `cargo fmt --all -- --check` | Passed |
| `cargo clippy --locked --all-targets --all-features -- -D warnings` | Passed |
| `cargo test --locked --all-targets --all-features` | 336 passed, 0 failed, 3 inherited ignores across 36 summaries |
| Trainer tests within that suite | 16 passed, including real localhost HTTP mocks and renderer checks |
| Release build: openfelt, sneakyblinders, poker-server | Passed |
| `python3 scripts/test_openfelt_pty.py target/release/openfelt` | All terminal smoke tests passed |
| `target/release/openfelt --version` | Launch passed; inherited package version 1.0.1 (development build, not a published release) |

The three inherited ignored integration/capacity tests remain explicitly unexecuted. The pre-existing Cargo warning about `poker` and `terminal-poker` sharing one source file remains. No model key or paid request was used.

## Important regression coverage

- Six occupied seats and exactly 100BB initial chips; configurable blinds and table sizes.
- Accepted decisions pause the visible player projection, reject a second decision, and stop bot stepping until explicit continuation. Fold, call and all-in paths are covered.
- Hidden opponent cards and a different deck cannot change the serialized player observation; identical permitted observations plus policy RNG seeds produce identical choices.
- Actual engine fixture for the handoff's short-stack example: 20 chips to call, 100 chips contestable, 80 returned unmatched. Multiway side-pot fixture includes folded money while excluding ineligible layers.
- Cash sessions spanning 2/6/9 seats and all three profiles preserve chip totals except recorded additions/withdrawals. Rebuy/top-up changes do not alter profit.
- Invalid raises leave the hand unchanged. A big blind's option has a raise-to minimum of two big blinds. Short all-ins do not reopen a prior actor's raise, including an attempted all-in raise. Existing policy masks and seeded invariant campaigns use the same eligibility rule.
- Provider request shape, authentication header isolation, invalid credentials, rate/credit errors, refused redirects, malformed responses, stale hand/revision IDs, token usage, timeout, cancellation, and conservative request/budget reservations.
- Canary credentials are rejected in raw or JSON-decoded provider text. Numeric and obvious solver claims are rejected, without claiming those checks can prove strategic accuracy.
- Saved settings/progress round-trip without credential fields. Unix private files are owner-only.
- Renderer tests show every seat and continuation controls at 80×30, including nine seats, and a safe resize message below the minimum.

## Interactive terminal checks

`scripts/test_openfelt_pty.py` uses real Unix pseudo-terminals with isolated temporary data and removes `OPENAI_API_KEY` from the child environment. It exercises both cases of F/C/R/A; one accepted decision per panel; all-in cancellation; invalid raise rejection; details; quit/terminal restoration; resizing down and back up; rebuy/withdrawal accounting; persisted stats; and missing-key play after explicit cloud consent. No paid request is made.

An additional manual PTY session at 100×36 confirmed that R1 is rejected, uppercase C records a legal call and pauses the pre-decision table, and ? expands the teaching facts. Native Terminal.app automation was unavailable, so no native-app screenshot or font/emulator-wide visual certification is claimed.

## Verification boundaries

The OpenAI adapter follows the current official Responses structured-output contract and is tested with controlled HTTP fixtures. No live model, provider billing, coaching latency or poker-teaching quality has been evaluated. Other provider/local-model adapters, equilibrium strategy, equity simulation, rake and prebuilt cross-platform releases are not included in this development build.

Upstream's unsafe aggregate pot-odds observation fields remain available for legacy training compatibility. OpenFelt coaching does not use them: its separate allowlisted facts layer calculates legal call cost and contestable pots and makes no universal equity/EV claim.

The initial baseline and documentation-only branch passed the historical four-platform GitHub Quality matrix, which included Intel macOS. Intel macOS is now deprecated; the supported matrix is Linux x86_64, Windows x86_64, and macOS Apple Silicon. Final implementation CI results should be read from the repository's corresponding commit/run; local results alone are not a claim of remote CI success.
