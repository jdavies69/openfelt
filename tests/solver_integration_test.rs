use std::sync::atomic::AtomicBool;
use terminal_poker::trainer::solver::{solve, RiverScenario};

fn scenario(hero_range: &str, villain_range: &str) -> RiverScenario {
    RiverScenario {
        id: "integration_matrix".into(),
        title: "Integration matrix".into(),
        description: "Checks per-hand solver projection".into(),
        board: [
            "2s".into(),
            "3h".into(),
            "4d".into(),
            "6c".into(),
            "7s".into(),
        ],
        hero_range: hero_range.into(),
        villain_range: villain_range.into(),
        pot: 100,
        effective_stack: 100,
        oop: true,
        bet_sizes: vec![0.5],
        raise_sizes: vec![2.5],
    }
}

#[test]
fn action_values_are_indexed_by_action_then_hand() {
    // 5c5d completes a straight on this board; KsKh loses to AsAh. The two
    // check EVs therefore sit at opposite ends and catch a transposed action/
    // hand index without relying on one-hand fixtures.
    let result = solve(&scenario("KsKh,5c5d", "AsAh"), &AtomicBool::new(false)).unwrap();
    assert!(result.converged, "exploitability {}", result.exploitability);
    assert_eq!(result.hero_combos.len(), 2);

    for combo in &result.hero_combos {
        assert_eq!(combo.actions.len(), 3);
        let frequency_sum: f64 = combo.actions.iter().map(|action| action.frequency).sum();
        assert!((frequency_sum - 1.0).abs() < 1e-5, "{combo:?}");
        assert!(combo.actions.iter().all(|action| action.ev.is_finite()));
        let calculated_best = combo
            .actions
            .iter()
            .map(|action| action.ev)
            .fold(f64::NEG_INFINITY, f64::max);
        assert!((combo.best_ev - calculated_best).abs() < 1e-6);
    }

    let winning = result
        .hero_combos
        .iter()
        .find(|combo| combo.cards.iter().any(|card| card.starts_with('5')))
        .unwrap();
    let losing = result
        .hero_combos
        .iter()
        .find(|combo| combo.cards.iter().any(|card| card.starts_with('K')))
        .unwrap();
    let check = |combo: &terminal_poker::trainer::solver::ComboResult| {
        combo
            .actions
            .iter()
            .find(|action| action.label == "Check")
            .unwrap()
            .ev
    };
    assert!((check(winning) - 100.0).abs() < 0.5, "{winning:?}");
    assert!(check(losing).abs() < 0.5, "{losing:?}");
}

#[test]
fn combinations_with_zero_compatible_opponent_reach_are_omitted() {
    // AsAh conflicts with villain AsKh. QsQh remains a valid matchup, so the
    // scenario as a whole is valid but the unreachable hero combo must not be
    // shown with the solver's uniform fallback strategy.
    let result = solve(&scenario("AsAh,QsQh", "AsKh"), &AtomicBool::new(false)).unwrap();
    assert_eq!(result.hero_combos.len(), 1, "{:?}", result.hero_combos);
    assert!(result.hero_combos[0]
        .cards
        .iter()
        .all(|card| !card.starts_with('A')));
}
