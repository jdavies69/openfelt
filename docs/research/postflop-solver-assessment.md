# postflop-solver integration assessment

Investigated 2026-09-23. This document records the pre-integration investigation. The subsequent implementation is documented in [river practice](../solver/README.md); timing results below describe the isolated investigation, not a release benchmark.

## Recommendation

A promising engine for an optional, local heads-up postflop practice mode. Start with prescribed river scenarios and known range assumptions, then turn scenarios. It is not a replacement for OpenFelt's full 2–9-seat engine or all current coaching. Do not advertise general GTO or blunder detection for unsupported states.

## Verified upstream scope

Repository: https://github.com/b-inary/postflop-solver
Inspected commit: `9d1509fe5077d019825f833eed04b16d342dfda1` (2023-10-01).

- Rust 2021 library; OpenFelt is also Rust 2021.
- Discounted CFR with two players: OOP and IP. Postflop board states, not preflop solving.
- Inputs: weighted ranges for both players, board, starting pot, effective stack, rake, and explicit betting/raising tree.
- Outputs include per-hand strategy frequencies and action expected values; the solver reports exploitability in its configured game.
- `solve_step` permits iteration-by-iteration control; `memory_usage` estimates tree storage before allocation. A worker can impose time, iteration, and memory limits.
- Optional serialization/compression allows reuse of solved trees. Cache reuse must require matching state, range assumptions, bet tree, solver version, and convergence settings.
- Folded-player bunching support is not a multiway active-player solver. It adds computation and requires folded ranges.
- The maintainer suspended open-source development in October 2023. The README warns that library APIs historically changed without version changes. Pin a commit and maintain a tested integration boundary.

Sources: [README](https://github.com/b-inary/postflop-solver), [basic example](https://github.com/b-inary/postflop-solver/blob/main/examples/basic.rs), [API](https://b-inary.github.io/postflop_solver/postflop_solver/struct.PostFlopGame.html), [solver implementation](https://github.com/b-inary/postflop-solver/blob/main/src/solver.rs).

## OpenFelt integration gaps

OpenFelt already has a frozen, player-authorized `Observation` containing public cards, own cards, pot, stacks, legal actions, and history. That is the appropriate boundary; never pass the full private engine state to the solver adapter.

Missing pieces:

1. **Range model.** Current bot heuristics are not weighted posterior hand ranges. Supply explicit scenario ranges initially. A generic reviewer would need ranges conditioned on public action history. Solve both players' ranges, then look up the hero's actual combination; do not let the opponent strategy assume knowledge of that actual holding.
2. **Street-aware public history.** Engine action records include phase and wager-after; the current coaching observation flattens them to seat/action pairs. A solver adapter needs a versioned public history sufficient to reconstruct its root and traverse the action tree, without future cards or hidden holdings.
3. **Action mapping.** OpenFelt allows legal custom bet/raise totals. A solver only evaluates actions in its configured tree. Reject unsupported sizing or rebuild a matching tree; silently snapping to the nearest size would misstate the quality of the user's actual decision.
4. **Eligibility.** Initially require exactly two eligible active players, supported postflop street, compatible pot/stack structure, valid nonempty ranges, and an exact action mapping. Exclude multiway pots and complicated side pots rather than force them into a heads-up model.
5. **EV interpretation.** Compare actions for the same hero combination at the same decision node and in consistent chip units. A low-frequency action can still have nearly equal EV. Exploitability of the overall solution is not a guaranteed per-hand error bound. Only label mistakes when the result is sufficiently converged and robust to the declared assumptions.
6. **UI and lifecycle.** Keep game input responsive. Run bounded work outside the renderer; support cancellation and stale-hand/revision rejection. Show source, assumptions, solve quality, and unsupported states. Keep LLM explanations optional and downstream of numerical results.

## License decision before shipping

Upstream declares **AGPL-3.0-or-later**; OpenFelt currently declares **MIT**. This is not an ordinary permissive dependency that can simply be incorporated and distributed under MIT alone. Plan an AGPL-compliant combined distribution or obtain suitable separate permission before shipping an integrated binary. A subprocess is not an automatic answer to the licensing question; the actual separation and distribution model need review. This investigation does not change OpenFelt's license or incorporate solver code into its build.

Source: [upstream license](https://github.com/b-inary/postflop-solver/blob/main/LICENSE) and Cargo manifests inspected locally.

## Proposed first experiment

- A dedicated, clearly labeled heads-up river drill with a fixed board, range pair, pot, stacks, and limited legal bet sizes.
- Precompute or solve before the exercise starts, so feedback after an action is an immediate lookup.
- Display strategy mix and action EV difference under the stated model, plus deterministic plain-language explanations.
- Validate selected fixtures against an independent reference before calling the advice GTO-quality.
- Expand to turn/flop and public-history range modeling only after measured accuracy, latency, and memory meet product targets.

## Local build and timing experiment

The experiment is isolated under `.git/investigations/postflop-solver`; it does not use API keys, network model requests, or the user's saved hands.

Machine: Apple M5 Pro, aarch64 macOS; Rust/Cargo 1.98.1 (Homebrew). Upstream commit pinned above. Four Rayon threads; two explicit five-combination ranges, pot 100 chips, effective stack 100, 50%-pot/all-in bets and 2.5x raises. Board `2s3h4d6c`, plus `7s` for river. Maximum 200 iterations and a 30-second process timeout per case; exploitability target 0.1 chips (0.1% of starting pot), checked every ten iterations.

| Case | Solve time | Iterations | Reported exploitability (chips) | Estimated uncompressed tree | Process peak RSS |
| --- | ---: | ---: | ---: | ---: | ---: |
| River | 0.551 ms | 170 | 0.027075 | 146,556 bytes | 4,472,832 bytes |
| Turn | 14.383 ms | 170 | 0.090657 | 354,264 bytes | 5,111,808 bytes |

Single-run measurements, not latency percentiles. Solve timing includes initial/periodic exploitability computation and finalization, excludes tree construction/allocation. RSS measured with macOS `/usr/bin/time`. These deliberately tiny ranges establish local feasibility only; realistic ranges, flop trees, UI latency, action EV accuracy, and independent reference agreement remain untested.

Compatibility findings:

- Default `cargo check --all-targets` fails: `bincode = "2.0.0-rc.3"` resolves to 2.0.1, whose `Decode`/`BorrowDecode` require a context type.
- Disabling serialization still produces three `dangerous_implicit_autorefs` errors on the installed Rust compiler.
- The isolated benchmark builds with serialization disabled and that lint allowed. This is an investigation workaround, not a production fix; audit and correct the pointer operations and pin compatible dependencies in a maintained fork.

Reproduction within the isolated clone:

```sh
RUSTFLAGS='-A dangerous_implicit_autorefs' cargo build --release --example bounded_bench --no-default-features --features rayon
RAYON_NUM_THREADS=4 target/release/examples/bounded_bench river
RAYON_NUM_THREADS=4 target/release/examples/bounded_bench turn
```

Benchmark source: `examples/bounded_bench.rs`, SHA-256 `09d019ef9c81184b698cd150eae40e95e5a95e7ecddd7787fddb61cfdd1f5287`. Explicit ranges: `AsAh,QsQh,JsJh,AcKc,AdKd` versus `KsKh,QcJc,QdJd,Ts9s,Th9h`. No solver dependency or runtime change was added to OpenFelt.
