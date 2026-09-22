# OpenFelt

> Play the hand. Learn the game.

A public, MIT-licensed, local play-money No-Limit Texas Hold’em trainer. Start with six seats, 100 big blinds, heuristic opponents and short teaching after each decision. No account, subscription, API key or hosted service is required to play.

## Run locally

Install a current stable Rust toolchain, then:

```sh
git clone https://github.com/jdavies69/openfelt.git
cd openfelt
cargo run --locked --release --bin openfelt
```

This is the first OpenFelt development build; the original audited baseline is preserved in Git history. Use a terminal at least **80 columns × 30 rows**; 100×36 gives more room. Tested locally on macOS arm64. Other platforms have a CI matrix; check the actual workflow results before relying on them. No prebuilt release is claimed.

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

The four drill topics each draw three questions from a larger local set, explain every answer, accept listed defensible alternatives, and persist attempts and accepted answers separately from ordinary hand/concept exposure. A recommendation appears only after at least two completed sets supply evidence; these exercises teach reviewed categorical rules rather than universal strategy.

`fundamentals`, `recreational`, and `competent` remain **heuristic profiles**, not independently rated skill levels or GTO opponents. `--opponent-style tight|balanced|loose` changes range width and bluff/call tendencies, while `--opponent-difficulty beginner|practiced` changes decision consistency independently. Ranges account for position and prior raises; postflop policy recognizes made hands and basic flush/open-ended draws and varies sizing. Opponents receive only their own cards and public data with separate random generators. Behavioral fixtures verify reproducibility and legal completion, but no claim of human-equivalent skill is made.

## Optional OpenAI coaching (BYOK)

Local teaching is the default. The OpenAI Responses adapter is implemented and tested against local HTTP fixtures for authentication, request shape, structured output, errors, redirects, stale responses and timeouts. **Live model coaching quality has not been evaluated and no paid provider test has been run.** Choose a model that supports Responses structured outputs; unsupported models fail visibly and leave play available.

Set `OPENAI_API_KEY` privately in your environment, then:

```sh
openfelt --coaching openai --model YOUR_MODEL_ID --max-requests 30 --max-output-tokens 600
```

The app displays `https://api.openai.com/v1/responses` and asks you to enable cloud coaching for the current session. It sends only the allowlisted pre-decision player view, accepted action and calculated facts. The key goes only in the authentication header, never in the prompt, files or logs. No validation request is sent merely because a key is present. Enter at the coaching pause cancels the request and continues; T explicitly retries and may incur another charge. Q cancels and quits. There are no automatic retries, provider fallbacks, telemetry or project proxy.

The first adapter accepts environment credentials only. Custom endpoints, native Anthropic, OpenAI-compatible services, local-model adapters, masked key entry and OS keychain integration are not implemented. OpenAI credentials cannot be redirected or automatically reused with another endpoint.

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

OpenFelt uses the platform local-data folder plus `openfelt` (on macOS, normally `~/Library/Application Support/openfelt`). Nonsecret settings are saved only with `--save-settings`; decisions, concept counts, completed-hand stats, cash events and provider usage save locally. `--data-dir PATH` selects an isolated folder. Unix files are created with owner-only permissions. Keep private histories out of public bug reports. There is no automatic upload of history. Only new explicitly enabled cloud decisions are sent to the selected provider, whose privacy/billing terms apply.

## Development and provenance

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets --all-features
cargo build --locked --release --bin openfelt --bin sneakyblinders --bin poker-server
```

On macOS/Linux, also run `python3 scripts/test_openfelt_pty.py target/release/openfelt` for real terminal input/resize/restart checks.

No model key is needed for tests. Provider tests run artificial localhost servers; some environments need localhost permission. New trainer code lives under `src/trainer/`; the engine and original binary targets remain available. Package registry publication is disabled until OpenFelt distribution metadata is deliberately prepared. Do not reuse inherited release tags/workflows without reviewing their upstream-specific distribution settings.

The provider contract follows the [official OpenAI structured-output documentation](https://developers.openai.com/api/docs/guides/structured-outputs), checked September 21, 2026. Responses use `text.format` with a strict JSON schema, and the app validates the result again locally.

Derived from [ashxudev/terminal-poker](https://github.com/ashxudev/terminal-poker) at `2e6cfe7454476ecd292797bb1801e4a6b2f70fb2`, preserving Git history and the original MIT license/copyright. No endorsement is implied. [Original README](docs/openfelt/UPSTREAM_README.md) is retained as historical upstream documentation; its installation and LAN instructions are not OpenFelt setup instructions.

See [verified baseline](docs/openfelt/BASELINE.md), [trainer validation](docs/openfelt/VALIDATION.md), [requirements](docs/openfelt/REQUIREMENTS.md), [implementation plan](docs/openfelt/PLAN.md), [contribution terms](CONTRIBUTING.md) and [security reporting](SECURITY.md).
