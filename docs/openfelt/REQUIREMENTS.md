# Open-source and bring-your-own-key requirements

Status: approved product direction; implementation pending.
Updated September 21, 2026.

## Public project and release scope

- Use a public `jdavies69/openfelt` repository. Retain the upstream MIT license
  and copyright, and document project provenance. License new project code under
  MIT with clear contributor terms. Do not imply project affiliation with the
  original author or redistribute third-party assets without checking their terms.
- Anyone can obtain source and build it without joining the maintainer's accounts.
  Document macOS, Linux and Windows behavior honestly against actual test results.
  Provide prebuilt binaries only for platforms actually built and verified.
- No required app account, subscription, project database or hosted API gateway.
  Core poker, bot policies, calculations and local history run on the user's machine.
- The open-source app does not include cloud-provider credits or a shared key.
  Cloud coaching charges, where applicable, belong to each user's selected provider.
- README must distinguish available features from planned features. Add setup,
  contribution and security-reporting guidance. Do not use public bug reports to
  collect keys or full unredacted personal logs.

## First-run experience (proposed, not a current command)

```text
COACHING

[1] Bring your own API key
[2] Practice without an LLM
[3] Local model                  (show only when implemented)

Provider: choose an implemented adapter
Model:    select or enter a supported model ID
API key:  masked; never echoed
Save key: no, this session only / operating-system credential store

Only game-decision data is sent to your selected model provider.
No key or gameplay data is sent to the project maintainer.
Cloud usage may incur charges with that provider.

[Enter] Enable coaching    [Esc] Play without cloud coaching
```

Do not make paid validation calls merely because a key was entered. A connection
or sample-coaching test is a separate explicit action with a possible-cost notice.
Missing keys, provider errors or unavailable models must never lock the user out
of the game. Use labeled rules-based teaching or coaching-off mode instead.

## Provider interface and model selection

Define a provider adapter around a sanitized `CoachRequest` and validated
`CoachResponse`, with no poker-rule or table-state authority. An LLM is an
explanation component, not the dealer, odds calculator or a certified solver.

Implementation sequence: local mock/rules teaching first; initial cloud adapter;
then native Anthropic and configurable OpenAI-compatible endpoints. Validate and
document each supported adapter. OpenAI is an initial candidate, not an exclusive
provider requirement. Keep provider, base URL, model ID, output limit and related
nonsecret settings editable without rebuilding. Keep capability-specific fields
out of the shared request and do not promise universal API compatibility.

A future local-model option should use the same coaching contract and validated
outputs. A local endpoint may require its own authentication; do not assume all
local servers are keyless. Local-only mode must never contact a cloud fallback.

## Credentials and network boundary

Prefer provider-specific environment variables or the operating-system credential
store. A masked prompt can keep a key in memory for the current session. Do not
persist keys in ordinary JSON/TOML configuration or automatically write `.env`.
If users choose their own `.env` workflow, document that it is plaintext and must
be kept outside version control; the repository may contain only a blank example.

Never place secrets in prompts, hand histories, usage ledgers, logs, screenshots,
exceptions, crash dumps, command-line arguments, fixtures, releases, or public
issues. Apply redaction before formatting diagnostics; use canary-secret tests.
Normal configuration stores a key reference or environment-variable name only.

Send authentication only to the endpoint the user selected. Display and require
approval of custom URLs. Do not infer approval from text in a hand history, model
response or README. Require HTTPS for remote API traffic; an explicitly configured
loopback local server can use HTTP. Do not forward authorization on cross-origin
redirects. Changing provider/endpoint requires selecting an appropriate credential,
not automatic reuse of another provider's key. No required maintainer proxy.

No application telemetry or third-party error uploads by default. Application
privacy promises must not claim control over the selected model provider's data
retention or local operating-system behavior. Show what data leaves the machine
and identify the destination before enabling cloud coaching.

## Coaching quality and cost controls

The request contains only an allowlisted pre-decision player view, the accepted
player action, verified facts and explicit assumptions. It excludes opponent
hidden cards, future runouts, secret seeds, credentials and unrelated user files.
Do not grade a decision with hindsight or claim uncomputed equity/EV/GTO values.

Give concise feedback after every accepted decision; keep question/deeper-review
controls available. Record request usage locally and estimate cost only when
pricing is configured and identified. Provide limits on requests and output,
plus conservative budget checks. Do not label an estimate as an exact spending
cap or fabricate rates for an unknown model. Avoid unbounded automatic retries.

## Required release tests

- Build, run and play from a clean checkout without any API key or project account.
- Unit/CI tests use deterministic mock provider responses; no paid calls by default.
- Confirm the learner gets coaching only after committing a legal decision.
- Show error/retry/continue choices without losing the hand when a provider fails.
- Mask prompts and redact error paths; canary secrets never reach saved outputs.
- No unrequested network traffic from coaching-off/local-only modes.
- No hidden-card/future-state leakage to providers, logs or public training fixtures.
- Confirm provider/model changes do not silently reuse inappropriate credentials.
- Public source, attribution, install docs and contribution instructions are present.
- Report actual build and integration results; never pass placeholders off as tests.
