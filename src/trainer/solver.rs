//! Bounded, local heads-up river practice. Ranges are explicit assumptions.

use postflop_solver::{
    card_from_str, card_to_string, compute_exploitability, finalize, solve_step, Action,
    ActionTree, BetSize, BetSizeOptions, BoardState, CardConfig, PostFlopGame, Range, TreeConfig,
};
use rayon::ThreadPoolBuilder;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

const MAX_COMBOS_PER_PLAYER: usize = 64;
const MAX_MEMORY_BYTES: u64 = 64 * 1024 * 1024;
const MAX_ITERATIONS: u32 = 2_000;
const MAX_DURATION: Duration = Duration::from_secs(3);
const CHECK_INTERVAL: u32 = 20;
const MAX_THREADS: usize = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiverScenario {
    pub id: String,
    pub title: String,
    pub description: String,
    /// Five distinct cards in rank+suit form, e.g. `As`.
    pub board: [String; 5],
    /// Upstream range syntax; explicit combinations may have `:weight`, e.g. `AsAh:0.5`.
    pub hero_range: String,
    pub villain_range: String,
    pub pot: u32,
    pub effective_stack: u32,
    /// The initial implementation presents the first decision to OOP.
    pub oop: bool,
    /// Opening bets as fractions of the pot. All-in is included automatically.
    pub bet_sizes: Vec<f64>,
    /// Raises as multiples of the previous bet.
    pub raise_sizes: Vec<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ActionResult {
    pub label: String,
    pub frequency: f64,
    /// Chip EV for the specified hero combination and this action.
    pub ev: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ComboResult {
    pub cards: [String; 2],
    pub weight: f64,
    pub actions: Vec<ActionResult>,
    pub best_ev: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SolvedDrill {
    pub scenario: RiverScenario,
    pub hero_combos: Vec<ComboResult>,
    pub exploitability: f32,
    pub iterations: u32,
    /// Only converged results may be used to grade a decision.
    pub converged: bool,
    pub target_exploitability: f32,
    pub estimated_memory_bytes: u64,
    pub elapsed_ms: u128,
}

impl RiverScenario {
    pub fn validate(&self) -> Result<(), String> {
        if self.id.trim().is_empty() || self.title.trim().is_empty() {
            return Err("Scenario id and title are required".into());
        }
        if !self.oop {
            return Err("The first river drill decision must be out of position".into());
        }
        if self.pot == 0
            || self.effective_stack == 0
            || self.pot > 100_000
            || self.effective_stack > 100_000
            || self.effective_stack > self.pot.saturating_mul(3)
        {
            return Err(
                "Pot and stack must be 1–100,000 chips, with stack at most 3 times the pot".into(),
            );
        }
        if self.bet_sizes.is_empty() || self.bet_sizes.len() > 2 || self.raise_sizes.len() > 1 {
            return Err("Use 1–2 opening sizes and at most 1 raise size".into());
        }
        if self.hero_range.len() > 1_000 || self.villain_range.len() > 1_000 {
            return Err("Range strings are limited to 1,000 characters".into());
        }
        if self
            .bet_sizes
            .iter()
            .any(|x| !x.is_finite() || *x < 0.25 || *x > 2.0)
            || self
                .raise_sizes
                .iter()
                .any(|x| !x.is_finite() || *x < 2.0 || *x > 5.0)
        {
            return Err("Bet sizes must be 25–200% pot; raises must be 2–5x".into());
        }
        let mut board_mask = 0u64;
        for card in &self.board {
            let parsed =
                card_from_str(card).map_err(|e| format!("Invalid board card {card}: {e}"))?;
            let bit = 1u64 << parsed;
            if board_mask & bit != 0 {
                return Err("Board cards must be distinct".into());
            }
            board_mask |= bit;
        }
        let hero: Range = self
            .hero_range
            .parse()
            .map_err(|e| format!("Invalid hero range: {e}"))?;
        let villain: Range = self
            .villain_range
            .parse()
            .map_err(|e| format!("Invalid villain range: {e}"))?;
        let (hero_hands, _) = hero.get_hands_weights(board_mask);
        let (villain_hands, _) = villain.get_hands_weights(board_mask);
        if hero_hands.is_empty() || villain_hands.is_empty() {
            return Err("Both ranges need a combination clear of the board".into());
        }
        if hero_hands.len() > MAX_COMBOS_PER_PLAYER || villain_hands.len() > MAX_COMBOS_PER_PLAYER {
            return Err(format!(
                "Each range is limited to {MAX_COMBOS_PER_PLAYER} combinations"
            ));
        }
        if !hero_hands.iter().any(|&(a, b)| {
            villain_hands
                .iter()
                .any(|&(c, d)| a != c && a != d && b != c && b != d)
        }) {
            return Err("Ranges have no compatible matchup".into());
        }
        Ok(())
    }
}

pub fn presets() -> Vec<RiverScenario> {
    vec![RiverScenario {
        id: "river_overpairs".into(),
        title: "River overpairs and draws".into(),
        description: "Heads-up river, OOP first. Explicit five-combination ranges; explore checking, a half-pot bet, and all-in.".into(),
        board: ["2s".into(), "3h".into(), "4d".into(), "6c".into(), "7s".into()],
        hero_range: "AsAh,QsQh,JsJh,AcKc,AdKd".into(),
        villain_range: "KsKh,QcJc,QdJd,Ts9s,Th9h".into(),
        pot: 100,
        effective_stack: 100,
        oop: true,
        bet_sizes: vec![0.5],
        raise_sizes: vec![2.5],
    }, RiverScenario {
        id: "river_set_vs_overpair".into(),
        title: "Set versus overpairs".into(),
        description: "A dry river with strong made hands and missed draws. Compare checks, a two-thirds pot bet, and all-in.".into(),
        board: ["2s".into(), "3h".into(), "4d".into(), "9c".into(), "Ks".into()],
        hero_range: "AsAh,QsQh,JsJh,AcQc,AdQd".into(),
        villain_range: "KhKc,2c2d,QcJc,Tc8c,8c7c".into(),
        pot: 120,
        effective_stack: 120,
        oop: true,
        bet_sizes: vec![0.67],
        raise_sizes: vec![2.5],
    }]
}

fn action_label(action: Action, pot: u32, stack: u32) -> String {
    match action {
        Action::Check => "Check".into(),
        Action::Fold => "Fold".into(),
        Action::Call => "Call".into(),
        Action::Bet(amount) | Action::AllIn(amount) if amount as u32 >= stack => {
            "Bet all-in".into()
        }
        Action::Bet(amount) | Action::AllIn(amount) => {
            format!("Bet {amount} ({}% pot)", amount as u32 * 100 / pot)
        }
        Action::Raise(amount) => format!("Raise to {amount}"),
        other => format!("{other:?}"),
    }
}

pub fn solve(scenario: &RiverScenario, cancelled: &AtomicBool) -> Result<SolvedDrill, String> {
    scenario.validate()?;
    if cancelled.load(Ordering::Relaxed) {
        return Err("Solver cancelled".into());
    }
    let started = Instant::now();
    let board = scenario
        .board
        .iter()
        .map(|s| card_from_str(s))
        .collect::<Result<Vec<_>, _>>()?;
    let card_config = CardConfig {
        range: [
            scenario.hero_range.parse()?,
            scenario.villain_range.parse()?,
        ],
        flop: [board[0], board[1], board[2]],
        turn: board[3],
        river: board[4],
    };
    let options = BetSizeOptions {
        bet: scenario
            .bet_sizes
            .iter()
            .copied()
            .map(BetSize::PotRelative)
            .chain(std::iter::once(BetSize::AllIn))
            .collect(),
        raise: scenario
            .raise_sizes
            .iter()
            .copied()
            .map(BetSize::PrevBetRelative)
            .collect(),
    };
    let pool = ThreadPoolBuilder::new()
        .num_threads(MAX_THREADS)
        .build()
        .map_err(|e| e.to_string())?;
    let tree = pool.install(|| {
        ActionTree::new(TreeConfig {
            initial_state: BoardState::River,
            starting_pot: scenario.pot as i32,
            effective_stack: scenario.effective_stack as i32,
            river_bet_sizes: [options.clone(), options],
            // Avoid inserting extra action sizes beyond the declared options.
            add_allin_threshold: 0.0,
            force_allin_threshold: 0.0,
            merging_threshold: 0.0,
            ..Default::default()
        })
    })?;
    if cancelled.load(Ordering::Relaxed) {
        return Err("Solver cancelled".into());
    }
    let mut game = pool.install(|| PostFlopGame::with_config(card_config, tree))?;
    let (estimated_memory_bytes, _) = game.memory_usage();
    if estimated_memory_bytes > MAX_MEMORY_BYTES {
        return Err(format!(
            "Scenario needs {estimated_memory_bytes} bytes; limit is {MAX_MEMORY_BYTES}"
        ));
    }
    if started.elapsed() > MAX_DURATION {
        return Err("Scenario construction exceeded time limit".into());
    }
    game.allocate_memory(false);
    let target_exploitability = scenario.pot as f32 * 0.001;
    let mut exploitability = f32::INFINITY;
    let mut iterations = 0;
    pool.install(|| {
        for i in 0..MAX_ITERATIONS {
            if cancelled.load(Ordering::Relaxed) || started.elapsed() >= MAX_DURATION {
                break;
            }
            solve_step(&game, i);
            iterations = i + 1;
            if iterations % CHECK_INTERVAL == 0 {
                exploitability = compute_exploitability(&game);
                if exploitability <= target_exploitability {
                    break;
                }
            }
        }
    });
    if cancelled.load(Ordering::Relaxed) {
        return Err("Solver cancelled".into());
    }
    if iterations == 0 {
        return Err("Solver time limit reached before an iteration".into());
    }
    if iterations % CHECK_INTERVAL != 0 || !exploitability.is_finite() {
        exploitability = pool.install(|| compute_exploitability(&game));
    }
    pool.install(|| finalize(&mut game));
    game.cache_normalized_weights();
    let actions = game.available_actions();
    let strategy = game.strategy();
    let values = game.expected_values_detail(0);
    let hands = game.private_cards(0);
    let hand_count = hands.len();
    let mut hero_combos = Vec::new();
    for (hand_idx, &(a, b)) in hands.iter().enumerate() {
        let weight = game.card_config().range[0].get_weight_by_cards(a, b);
        if weight <= 0.0 {
            continue;
        }
        let compatible = game.private_cards(1).iter().any(|&(c, d)| {
            game.card_config().range[1].get_weight_by_cards(c, d) > 0.0
                && a != c
                && a != d
                && b != c
                && b != d
        });
        if !compatible {
            continue;
        }
        let mut combo_actions = Vec::with_capacity(actions.len());
        for (action_idx, action) in actions.iter().enumerate() {
            let index = action_idx * hand_count + hand_idx;
            if !strategy[index].is_finite() || !values[index].is_finite() {
                return Err(
                    "The scenario produced non-finite numerical results; no grade is available"
                        .into(),
                );
            }
            combo_actions.push(ActionResult {
                label: action_label(*action, scenario.pot, scenario.effective_stack),
                frequency: strategy[index] as f64,
                ev: values[index] as f64,
            });
        }
        let best_ev = combo_actions
            .iter()
            .map(|a| a.ev)
            .fold(f64::NEG_INFINITY, f64::max);
        hero_combos.push(ComboResult {
            cards: [card_to_string(a)?, card_to_string(b)?],
            weight: weight as f64,
            actions: combo_actions,
            best_ev,
        });
    }
    if hero_combos.is_empty() {
        return Err("No hero combination has a compatible opponent hand".into());
    }
    if !exploitability.is_finite() {
        return Err("The solver could not measure convergence; no grade is available".into());
    }
    Ok(SolvedDrill {
        scenario: scenario.clone(),
        hero_combos,
        exploitability,
        iterations,
        converged: exploitability <= target_exploitability,
        target_exploitability,
        estimated_memory_bytes,
        elapsed_ms: started.elapsed().as_millis(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_overlap_and_unbounded_ranges() {
        let mut scenario = presets().remove(0);
        scenario.board[0] = scenario.board[1].clone();
        assert!(scenario.validate().is_err());
        scenario = presets().remove(0);
        scenario.hero_range = "random".into();
        assert!(scenario.validate().is_err());
    }

    #[test]
    fn certain_showdown_check_ev_matches_pot() {
        let scenario = RiverScenario {
            id: "known_win".into(),
            title: "Known win".into(),
            description: "Analytic fixture".into(),
            board: [
                "2s".into(),
                "3h".into(),
                "4d".into(),
                "6c".into(),
                "7s".into(),
            ],
            hero_range: "AsAh".into(),
            villain_range: "KsKh".into(),
            pot: 100,
            effective_stack: 100,
            oop: true,
            bet_sizes: vec![0.5],
            raise_sizes: vec![],
        };
        let result = solve(&scenario, &AtomicBool::new(false)).unwrap();
        assert!(result.converged, "exploitability {}", result.exploitability);
        let check = result.hero_combos[0]
            .actions
            .iter()
            .find(|a| a.label == "Check")
            .unwrap();
        assert!((check.ev - 100.0).abs() < 0.5, "check EV {}", check.ev);
    }

    #[test]
    fn presets_have_action_values_and_mixes() {
        for scenario in presets() {
            let result = solve(&scenario, &AtomicBool::new(false)).unwrap();
            assert!(
                result.converged,
                "{}: exploitability {}",
                scenario.id, result.exploitability
            );
            assert!(
                result.hero_combos.iter().any(|combo| {
                    combo
                        .actions
                        .iter()
                        .any(|action| combo.best_ev - action.ev > 0.5)
                }),
                "{} lacks a meaningful action EV difference",
                scenario.id
            );
            for combo in &result.hero_combos {
                let total: f64 = combo.actions.iter().map(|action| action.frequency).sum();
                assert!((total - 1.0).abs() < 0.001, "{}: {total}", scenario.id);
                assert!(combo.actions.iter().all(|action| action.ev.is_finite()));
            }
        }
    }
}
