# Local river practice

OpenFelt includes a maintained copy of `b-inary/postflop-solver` for a separate,
local heads-up river decision exercise. It evaluates a prescribed model: both
players' weighted ranges, the complete board, pot, effective stack and allowed
bet sizes. It does not infer hidden opponent cards or grade regular table hands.

## Open the exercise

```sh
openfelt --solver-practice
openfelt --solver-scenario docs/solver/river-example.json
```

At the regular trainer table, press **G** to open practice and **Esc** to return.
The current table hand stays in place. Direct CLI practice needs no saved settings,
API key, Keychain access or model request.

Use arrows to choose an action, Enter or its number to answer, left/right to
choose another hero combination, and **N** for the next scenario. Feedback is
hidden until you answer. Press **?** for the full scrollable range and sizing
assumptions. Solving runs on a background worker; Escape cancels it.

## Define possible hands and betting options

Start with [river-example.json](river-example.json). The two ranges are hypotheses
for the exercise, not the actual secret holdings of a table opponent. Both ranges
are solved together; selecting a hero combination only selects which result to
view. It does not reveal that combination to the opponent's strategy.

- Cards use rank and suit, e.g. `As` = ace of spades, `Th` = ten of hearts.
- Ranges accept explicit combinations and weights, e.g. `AsAh,AcKc:0.5`.
  Weight 0.5 gives half the relative mass of weight 1 before card removal.
- `pot` and `effective_stack` are chips at the start of the river.
- `oop` is `true`: this initial mode asks for the first player's opening decision.
- `bet_sizes` are fractions of the pot: `0.5` is a half-pot bet. All-in is also
  available; the same betting options are modeled for both players.
- `raise_sizes` are multiples of the previous bet, e.g. `2.5` means raise to 2.5x.
- The tree may consolidate duplicate sizes or cap a bet at the remaining stack.
  Choices shown in practice are the actual tree actions; no arbitrary table bet
  is silently mapped to a nearby size.

Invalid boards, empty/incompatible ranges and configurations outside the bounded
practice limits are rejected. This first version intentionally supports small
river exercises; it does not accept preflop, multiway, side-pot or arbitrary
in-progress table states.

Current limits: 64 board-compatible combinations per player, 1–2 opening sizes
from 25% to 200% pot, at most one raise size from 2x to 5x, and effective stack
no greater than three times the pot. Solving uses two worker threads, a 64 MiB
estimated tree-storage limit, at most 2,000 iterations and a three-second soft
time budget. Cancellation/time checks occur between bounded solver operations;
the memory estimate is not a total-process RSS cap.

## Read the feedback

EV loss is the best action's expected value minus your action's expected value
for **the same hero combination at the same node**. Frequencies show the solver's
strategy mix. An infrequent action can still lose almost no EV. These values are
estimates under the displayed ranges and betting tree, not a promise of winning
this hand or a measured edge against OpenFelt's heuristic bots.

The solver reports convergence to its configured overall exploitability target.
This is not a per-hand accuracy guarantee. Unconverged results must remain
ungraded. The colored decision labels are teaching thresholds on estimated EV
loss; the numerical loss and assumptions are the primary information.

The labels use EV loss as a percentage of the starting pot: up to 0.5% is a
“Near best,” up to 2% is “Small EV loss,” and above 2% is “Large EV loss.” These are
OpenFelt teaching thresholds, not standardized solver definitions.

The first validation includes simple analytically checkable river fixtures.
Agreement with an independent commercial solver across realistic ranges has not
been established. No full-range or full-game GTO claim is made.

## Maintain the integration

The source is vendored under `vendor/postflop-solver`; `OPENFELT.md` records the
upstream revision and compatibility patches. The adapter and limits live in
`src/trainer/solver.rs`; the cancellable practice screen is in `solver_ui.rs`.
Run the normal repository checks plus the solver PTY script after changes.
Update the vendor only with a reviewed diff and numerical regression checks.

The combined application uses AGPL-3.0-or-later. Original MIT notices are retained.
See [third-party notices](../../THIRD_PARTY_NOTICES.md) for source and licensing.
