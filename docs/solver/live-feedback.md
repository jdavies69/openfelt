# Solver feedback during normal play

Open Settings with **S**, set **Solver feedback** to **On**, then **Save and return**.
The choice persists between launches. It defaults to Off for existing and new
installations. This setting is independent of the optional cloud provider.

After an accepted, supported decision, the normal hand review starts a local
solver job while showing the existing immediate teaching. A successful result
appears in that same review, labeled **Solver · modeled ranges**. Press **?** for
both separately labeled solver analysis and the existing local or cloud coaching
explanation. Cloud coaching follows the saved provider selection and request budget;
it cannot replace the solver's numerical grade. Continue whenever you want; leaving the
review cancels the job. Turning the setting Off cancels pending solver work and
restores ordinary coaching. No API key or network request is required by the solver.

## What is modeled

The initial live adapter supports heads-up river decisions, including in-position
and out-of-position play and exact observed river bet sizes. It uses the frozen
pre-decision player observation, public street-by-street action history and your
accepted action. It never reads the opponent's actual cards, the undealt deck or
the outcome of the hand.

Both players begin with broad, weighted possible holdings. Public actions before
the river adjust those weights using explicit heuristic likelihoods. Each such
update uses only the board that was visible on that action's street. The river
board removes impossible combinations; the solver then conditions on the river
action sequence. These ranges are assumptions, not measured opponent tendencies
or equilibrium preflop ranges. Changing the range model can change the advice.

The accepted action is evaluated at its exact size in the modeled tree. Unsupported
states, inconsistent histories, side pots, unreachable decisions, exceeded limits
or insufficient convergence fall back to labeled heuristic coaching rather than
receiving a solver badge. Preflop, flop, turn and multiway decisions are not
solver-graded in this version.

A converged solution is an approximation for the stated ranges and action tree.
EV loss compares actions for your actual combination at the same decision node.
Global exploitability is not a per-combination error guarantee. Review the
assumptions alongside the number; a good decision can still lose the hand.

The separate [river practice](README.md) remains available for inspecting custom
range scenarios, but is not required to receive feedback at the normal table.

## Help for settings

Highlight any setting and press **?** to read what it changes and when it takes
effect. Press **?** again or **Esc** to return to that same row, preserving
unsaved edits. Opening help does not reveal credentials or request cloud access.
