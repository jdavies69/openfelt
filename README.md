# OpenFelt

> Play the hand. Learn the game.

A public, MIT-licensed, local play-money No-Limit Texas Hold’em trainer. Start with six seats, 100 big blinds, heuristic opponents and short teaching after each decision. No account, subscription, API key or hosted service is required to play.

## Run locally

Install a published release without Rust. On macOS or Linux:

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/jdavies69/openfelt/releases/latest/download/openfelt-installer.sh | sh
```

On Windows PowerShell:

```powershell
irm https://github.com/jdavies69/openfelt/releases/latest/download/openfelt-installer.ps1 | iex
```

Open a new terminal and run `openfelt`. The command is placed on `PATH` in `~/.local/bin` (or `%USERPROFILE%\.local\bin`). Upgrade, uninstall, and checksum details are in [installation and upgrade instructions](docs/openfelt/INSTALL.md). Between hands, Settings → Updates can check for a newer stable release and install it for that same directory. A public release tag is required before those installer URLs succeed; packaging in CI is not a clean-install test.

To build from source, install a current stable Rust toolchain, then:

```sh
git clone https://github.com/jdavies69/openfelt.git
cd openfelt
cargo run --locked --release --bin openfelt
```

Use a terminal at least **80 columns × 30 rows**; 100×36 gives more room. Tagged releases are built by GitHub Actions for Linux x86_64, Windows x86_64, and macOS Apple Silicon. Intel macOS is deprecated and receives no release artifacts or CI coverage. Treat a supported platform as verified only when its release and Quality workflows are green; native clean-install walkthroughs remain a manual release check.

To install the command from this checkout:

```sh
cargo install --locked --path . --bin openfelt
openfelt
```

## Play and learn

| Key | Action |
| --- | --- |
| F | Fold when facing a wager |
| C | Check or call the legal amount; an all-in call requires confirmation |
| R | Enter a bet/raise **TO** a total street contribution in whole chips |
| A | Review an all-in; Enter confirms, Esc cancels |
| Enter | Submit an amount, continue after teaching, or deal the next hand |
| ? | Expand teaching details, or show help outside the coaching pause |
| V | Between hands: browse saved hands and bookmark replay decisions |
| S | Open settings when the table is not paused for coaching |
| B | Between hands: rebuy/top up to 100BB |
| W | Between hands: withdraw chips with confirmation |
| Q | Quit at any time |

Letters work in either case. Arrow keys adjust raise entry by one chip. Rejected actions do not count as decisions. Every accepted hero decision freezes the visible pre-decision table and pauses bots until Enter, including folds and all-ins. There is no learner action timer. The hand authority may resolve an immediate runout internally after accepting an action, but no future state reaches the frozen display or coach request.

Blinds stay fixed and rake is off. Busted bots rebuy to preserve the selected seat count. Top-ups and withdrawals are logged separately from completed-hand profit. Quitting mid-hand records decisions already made but does not count the unfinished hand as completed profit; restarting starts a new cash session.

```sh
openfelt --seats 6 --small-blind 1 --big-blind 2 --opponents fundamentals
openfelt --opponents recreational --aggression 0.6 --bluff-rate 0.08 --mistake-rate 0.15
openfelt --opponent-style loose --opponent-difficulty practiced
openfelt --coaching off
openfelt --drill starting-hands
openfelt --drill position
openfelt --drill calling-prices
openfelt --drill value-betting
openfelt --save-settings
openfelt --stats
```

The four drill topics each draw three questions from a larger local set, explain every answer, accept listed defensible alternatives, and persist attempts and accepted answers separately from ordinary hand/concept exposure. A recommendation appears after at least two completed sets, after three local "reconsider" assessments of the same supported concept, or from a bookmarked decision whose saved concept maps to a drill. One uncertain decision is not treated as a weakness. These exercises teach reviewed categorical rules rather than universal strategy.

`fundamentals`, `recreational`, and `competent` remain **heuristic profiles**, not independently rated skill levels or GTO opponents. `--opponent-style tight|balanced|loose` changes range width and bluff/call tendencies, while `--opponent-difficulty beginner|practiced` changes decision consistency independently. Ranges account for position and prior raises; postflop policy recognizes made hands and basic flush/open-ended draws and varies sizing. Opponents receive only their own cards and public data with separate random generators. Behavioral fixtures verify reproducibility and legal completion, but no claim of human-equivalent skill is made.

## Optional OpenAI or Anthropic coaching (BYOK)

Local teaching is the default. Press `S` to choose Local, OpenAI, or Anthropic, select a curated model, set a session request limit, and optionally save a key in the operating-system credential store. The first launch without CLI setup options opens this screen. Settings also cover seat count, opponent profile, and an explicit update check between hands. Table changes apply to the next session. Checking for an update does not install it, and the app does not replace Cargo, source, or package-manager copies.

The OpenAI Responses and Anthropic Messages adapters use the same allowlisted decision data and local response checks. Their request shapes are covered by local fixtures. **Live model coaching quality and paid credentials have not been evaluated.** Unsupported or retired models fail visibly and leave local play available.

Model IDs are a **curated, hardcoded list** in the app (`Provider::models()`), not a live fetch from `/v1/models` or equivalent. Curated OpenAI entries: `gpt-5.4-nano` (default), `gpt-4o-mini`, `gpt-4o`, `gpt-4.1`, `gpt-5`. Curated Anthropic entries: `claude-haiku-4-5-20251001`, `claude-sonnet-5`, `claude-opus-5`. Use ←/→ in Settings to cycle the list, `E` to type a custom model ID, or pass `--model`. Only models that support the adapter's structured-output path are useful; the app does not auto-select from a provider catalog.

You may instead set `OPENAI_API_KEY` or `ANTHROPIC_API_KEY` privately in your environment:

```sh
openfelt --coaching openai --model YOUR_MODEL_ID --max-requests 30 --max-output-tokens 600
openfelt --coaching anthropic --model YOUR_MODEL_ID --max-requests 30 --max-output-tokens 600
```

The app displays the selected provider and asks you to enable cloud coaching for the current session. It sends only the allowlisted pre-decision player view, accepted action and calculated facts. The key goes only in the authentication header, never in the prompt, settings, history, or logs. No validation request is sent merely because a key is present or saved. Enter at the coaching pause cancels the request and continues; T explicitly retries and may incur another charge. Q cancels and quits. There are no automatic retries, provider fallbacks, telemetry or project proxy.

Credential precedence is the selected provider's environment variable, its keychain entry, then no cloud credential. Nonsecret CLI options override saved settings. OpenAI and Anthropic use separate credential accounts. On macOS, Save prefers a **local** Data Protection internet password (`kSecUseDataProtectionKeychain`, synchronizable = false — no iCloud sync) for `api.openai.com` / `api.anthropic.com`. CLI and unsigned builds often lack that entitlement, so Save falls back to the login keychain generic item under service `dev.openfelt.coaching` (Keychain Access). Older generic items are still read and removed when a Data Protection save succeeds. On Windows and Linux the OS credential manager / Secret Service is unchanged. Forgetting one provider never exposes or reuses it for the other. API keys are deliberately not accepted as command-line arguments because process listings and shell history can expose them. Custom endpoints, OpenAI-compatible services, and local-model adapters are outside this feature.

Opening the app or browsing Settings does not read saved API keys. OpenFelt reads a key when you explicitly test it or use enabled cloud coaching, then reuses it in memory for the session. On macOS, **Allow** authorizes one Keychain access; **Always Allow** remembers access for the app ([Apple support](https://support.apple.com/guide/mac-help/kychn002/mac)). Locally built, ad-hoc-signed executables may need authorization again after an update. Local coaching needs no Keychain access.


Limits are per session. Optional estimated budget safeguards require explicit current prices and their date/source:

```sh
openfelt --coaching openai --model YOUR_MODEL_ID \
  --input-usd-per-million YOUR_INPUT_PRICE \
  --output-usd-per-million YOUR_OUTPUT_PRICE \
  --pricing-as-of YOUR_PRICE_DATE_AND_SOURCE --budget-usd YOUR_BUDGET
```

Use numeric values for prices/budget. Request reservations conservatively use input bytes plus overhead and the output limit. Cancelled, failed or timed-out requests keep their reservation because provider billing can be uncertain. This is a local estimate, **not a guaranteed provider billing cap**. Raw token usage and cumulative reservations are recorded locally. No default model or prices are advertised as current.

Cloud responses are schema-checked and strategic feedback is labeled heuristic. Numeric model claims and obvious solver claims are rejected; these checks cannot establish strategic correctness. Exact call cost and pot eligibility come from code. No equity simulation, exact EV, solver frequency or luck-based grade is claimed. The contestable-pot figure assumes a call and no further contributions; it excludes uncalled excess and ineligible side pots, and is not a universal equity threshold.

## Private local data

OpenFelt uses the platform local-data folder plus `openfelt` (on macOS, normally `~/Library/Application Support/openfelt`). The in-app Save action or `--save-settings` writes nonsecret settings; decisions, concept counts, completed-hand stats, cash events and provider usage save locally. API keys are excluded from this file. On macOS they prefer local Data Protection internet passwords (no iCloud sync); CLI builds fall back to the login keychain under `dev.openfelt.coaching`. Check Passwords or Keychain Access. On Linux they use Secret Service/keyring; on Windows, Credential Manager. `--data-dir PATH` selects an isolated folder for nonsecret state only. Unix files are created with owner-only permissions. Keep private histories out of public bug reports. There is no automatic upload of history.

Completed hands can be browsed after a restart with `openfelt-replay list`, then `openfelt-replay show HAND_ID --decision 0`. Move forward or backward by changing the zero-based decision number. `openfelt-replay outcome HAND_ID` shows the separately stored final state. `openfelt-replay bookmark HAND_ID DECISION` saves a direct review target; `openfelt-replay bookmarks` lists them. Invalid history lines are skipped and reported without hiding valid hands. Replay decision views contain only the saved pre-decision information.

Run `openfelt-eval` for the committed, reproducible offline coaching corpus and rubric report. It makes no provider calls. `--live` requires an API key plus explicit model, request count, and budget, but live adapter execution remains disabled until reviewed scenario decisions and dated price inputs are supplied. Offline fixture success does not establish live teaching quality.

## Development and provenance

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets --all-features
cargo build --locked --release --bin openfelt --bin sneakyblinders --bin poker-server
```

On macOS/Linux, also run `python3 scripts/test_openfelt_pty.py target/release/openfelt` for real terminal input/resize/restart checks.

No model key is needed for tests. Provider tests run artificial localhost servers; some environments need localhost permission. New trainer code lives under `src/trainer/`; the engine and original binary targets remain available. Package registry publication is disabled until OpenFelt distribution metadata is deliberately prepared. Do not reuse inherited release tags/workflows without reviewing their upstream-specific distribution settings.

The provider contracts follow the official [OpenAI structured-output documentation](https://developers.openai.com/api/docs/guides/structured-outputs) and [Anthropic Messages/model documentation](https://platform.claude.com/docs/en/models/overview), checked September 22, 2026. Both adapters request schema-constrained JSON and validate it again locally.

Derived from [ashxudev/terminal-poker](https://github.com/ashxudev/terminal-poker) at `2e6cfe7454476ecd292797bb1801e4a6b2f70fb2`, preserving Git history and the original MIT license/copyright. No endorsement is implied. [Original README](docs/openfelt/UPSTREAM_README.md) is retained as historical upstream documentation; its installation and LAN instructions are not OpenFelt setup instructions.

See [verified baseline](docs/openfelt/BASELINE.md), [trainer validation](docs/openfelt/VALIDATION.md), [requirements](docs/openfelt/REQUIREMENTS.md), [implementation plan](docs/openfelt/PLAN.md), [contribution terms](CONTRIBUTING.md) and [security reporting](SECURITY.md).
