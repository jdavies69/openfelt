# Trainer implementation sequence

1. Add the dedicated OpenFelt controller over MultiwayHand, default six-max and fixed 100BB starting stacks. Keep legacy binaries intact.
2. Capture an immutable allowlisted pre-decision view, validate/accept the action, freeze the visible hand and bot processing until Enter. Add F/C/R/A controls and deliberate all-in confirmation.
3. Add local teaching, configurable heuristic opponent profiles with independent RNGs, exact legal call costs and contestable-pot facts. Regression-test short stacks, side pots, information isolation and session cash flows.
4. Save nonsecret settings, private decisions and learning progress in an OpenFelt namespace. Support cash top-ups, rebuys and explicit withdrawals between hands.
5. Add optional OpenAI BYOK coaching with current structured-output contract, bounded asynchronous requests, cancellation, scoped credentials, response validation, usage tracking and request/budget safeguards. Test with mocks; live paid quality evaluation is a separate explicit action.
6. Verify the complete terminal flow, add reproducible tests and public installation guidance. Additional native Anthropic, compatible-service and local-model adapters remain subsequent milestones until their contracts are tested.

The smallest first increment is steps 1–2 with deterministic local teaching and no network access.

## Implementation status

Steps 1–5 are now implemented, including a standalone local trainer, policy profiles, paused teaching, cash ledger and optional OpenAI Responses adapter. Step 6 has local automated/PTY coverage, public setup documentation and the CI matrix. See [actual validation and limits](VALIDATION.md). Live paid coaching evaluation and additional provider adapters remain separate future work; no unsupported provider is advertised as available.
