//! Local solver review for supported heads-up river decisions in normal play.
//! The only private card input is the acting player's own two cards.

use super::facts::{Decision, Feedback, Observation, PublicAction};
use super::ranges::{ranges, LiveRanges};
use crate::game::{
    actions::Action as GameAction,
    deck::{Card, Rank, Suit},
    multiway::MultiwayPhase,
    table::HandParticipation,
};
use postflop_solver::{
    card_from_str, compute_exploitability, finalize, solve_step, Action, ActionTree, BetSize,
    BetSizeOptions, BoardState, CardConfig, PostFlopGame, Range, TreeConfig,
};
use rayon::ThreadPoolBuilder;
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

const MAX_HISTORY: usize = 6;
const MAX_RANGE_CHARS: usize = 32_000;
const MAX_MEMORY_BYTES: u64 = 128 * 1024 * 1024;
const MAX_ITERATIONS: u32 = 1_000;
const MAX_DURATION: Duration = Duration::from_secs(5);
const CHECK_INTERVAL: u32 = 25;
const THREADS: usize = 2;

/// A cheap gate for the UI. `solve_decision` repeats this before doing work.
pub fn eligibility(d: &Decision) -> Result<(), String> {
    let o = &d.observation;
    if o.phase != MultiwayPhase::River || o.board.len() != 5 || o.hole_cards.len() != 2 {
        return Err("Solver review supports river decisions with five board cards".into());
    }
    let live: Vec<_> = o
        .seats
        .iter()
        .filter(|s| s.participation == HandParticipation::Live)
        .collect();
    if live.len() != 2
        || o.seats
            .iter()
            .any(|s| s.participation == HandParticipation::AllIn)
    {
        return Err("Solver review requires two live players and no all-in side pot".into());
    }
    if !live.iter().any(|s| s.seat == o.actor) || live.iter().any(|s| s.stack == 0) {
        return Err("The acting player and opponent must both have chips".into());
    }
    if o.public_history
        .iter()
        .filter(|a| a.phase == MultiwayPhase::River)
        .count()
        > MAX_HISTORY
    {
        return Err("River betting history exceeds the supported depth".into());
    }
    if o.public_history.iter().any(|a| {
        a.phase == MultiwayPhase::River
            && (matches!(a.action, GameAction::Fold | GameAction::AllIn(_))
                || !live.iter().any(|s| s.seat == a.seat))
    }) {
        return Err("River history includes a fold, all-in, or third player".into());
    }
    if o.seats
        .iter()
        .any(|s| s.participation != HandParticipation::Live && s.street_contribution != 0)
    {
        return Err("River side contributions are unsupported".into());
    }
    let prior = [
        live[0]
            .hand_contribution
            .checked_sub(live[0].street_contribution),
        live[1]
            .hand_contribution
            .checked_sub(live[1].street_contribution),
    ];
    if prior[0].is_none() || prior[0] != prior[1] {
        return Err("Unequal earlier contributions need a side-pot model".into());
    }
    let street_total = live[0]
        .street_contribution
        .checked_add(live[1].street_contribution)
        .ok_or("River contribution overflow")?;
    if o.pot == 0 || o.pot > 1_000_000 || o.pot < street_total {
        return Err("River pot or contributions are inconsistent".into());
    }
    if o.wager
        != live
            .iter()
            .map(|s| s.street_contribution)
            .max()
            .unwrap_or(0)
    {
        return Err("Current wager is inconsistent with river contributions".into());
    }
    if o.public_history.is_empty()
        || o.public_history.len() != o.history.len()
        || usize::try_from(o.revision).ok() != Some(o.public_history.len())
        || !o
            .public_history
            .iter()
            .zip(&o.history)
            .all(|(public, legacy)| public.seat == legacy.0 && public.action == legacy.1)
        || o.public_history
            .windows(2)
            .any(|pair| phase_index(pair[0].phase) > phase_index(pair[1].phase))
        || o.public_history
            .iter()
            .any(|action| phase_index(action.phase) > 3)
    {
        return Err("Street-aware public history is required for solver review".into());
    }
    validate_accepted(o, d.accepted_action)?;
    Ok(())
}

fn phase_index(phase: MultiwayPhase) -> u8 {
    match phase {
        MultiwayPhase::Preflop => 0,
        MultiwayPhase::Flop => 1,
        MultiwayPhase::Turn => 2,
        MultiwayPhase::River => 3,
        MultiwayPhase::Showdown => 4,
        MultiwayPhase::HandComplete => 5,
    }
}

fn validate_accepted(o: &Observation, action: GameAction) -> Result<(), String> {
    let l = &o.legal;
    let accepted = match action {
        GameAction::Fold => l.can_fold,
        GameAction::Check => l.can_check,
        GameAction::Call(v) => l.call_amount == Some(v),
        GameAction::Bet(v) => l.min_bet_to.is_some_and(|min| v >= min && v < l.all_in_to),
        GameAction::Raise(v) => {
            l.raise_reopened
                && l.min_raise_to
                    .is_some_and(|min| v >= min && v < l.all_in_to)
        }
        GameAction::AllIn(v) => {
            l.can_all_in() && v == l.all_in_to && v > o.own().street_contribution
        }
    };
    if accepted {
        Ok(())
    } else {
        Err("The recorded action is not legal at this decision".into())
    }
}

fn card_ascii(card: Card) -> String {
    let suit = match card.suit {
        Suit::Spades => 's',
        Suit::Hearts => 'h',
        Suit::Diamonds => 'd',
        Suit::Clubs => 'c',
    };
    let rank = if card.rank == Rank::Ten {
        "T"
    } else {
        card.rank.symbol()
    };
    format!("{rank}{suit}")
}

#[derive(Debug)]
struct Context {
    root_pot: i32,
    effective_stack: i32,
    hero_player: usize,
    board: [u8; 5],
    hero_cards: (u8, u8),
    history: Vec<Action>,
    accepted: Action,
    ranges: [Range; 2],
    assumptions: Vec<String>,
}

fn map_action(
    action: GameAction,
    current_wager: u32,
    effective_stack: u32,
) -> Result<Action, String> {
    let mapped = match action {
        GameAction::Fold => Action::Fold,
        GameAction::Check => Action::Check,
        GameAction::Call(_) => Action::Call,
        GameAction::Bet(v) if current_wager == 0 && v == effective_stack => Action::AllIn(v as i32),
        GameAction::Bet(v) if current_wager == 0 && v < effective_stack => Action::Bet(v as i32),
        GameAction::Raise(v) if current_wager > 0 && v == effective_stack => {
            Action::AllIn(v as i32)
        }
        GameAction::Raise(v) if current_wager > 0 && v < effective_stack => Action::Raise(v as i32),
        GameAction::AllIn(v) if v == effective_stack && v > current_wager => {
            Action::AllIn(v as i32)
        }
        _ => {
            return Err(
                "Action size cannot be represented exactly in the effective-stack tree".into(),
            )
        }
    };
    Ok(mapped)
}

fn replay_record(
    record: &PublicAction,
    actor: usize,
    contributions: &mut [u32; 2],
    wager: &mut u32,
    effective_stack: u32,
) -> Result<Action, String> {
    let before = *wager;
    let own = contributions[actor];
    let action = match record.action {
        GameAction::Check if own == before => Action::Check,
        GameAction::Call(cost) if before > own && cost == before - own => {
            contributions[actor] = before;
            Action::Call
        }
        GameAction::Bet(v) if before == 0 && own == 0 && v > 0 && v <= effective_stack => {
            contributions[actor] = v;
            *wager = v;
            map_action(record.action, before, effective_stack)?
        }
        GameAction::Raise(v) if before > own && v > before && v <= effective_stack => {
            contributions[actor] = v;
            *wager = v;
            map_action(record.action, before, effective_stack)?
        }
        _ => return Err("River history contains an unsupported action or amount".into()),
    };
    if record.wager_after != *wager {
        return Err("River history wager does not match the action".into());
    }
    Ok(action)
}

fn context(d: &Decision, live_ranges: LiveRanges) -> Result<Context, String> {
    let o = &d.observation;
    if live_ranges.oop.len() > MAX_RANGE_CHARS || live_ranges.ip.len() > MAX_RANGE_CHARS {
        return Err("Inferred range is too large for local review".into());
    }
    let oop = o
        .seats
        .iter()
        .find(|s| s.seat == live_ranges.oop_seat)
        .ok_or("Missing OOP seat")?;
    let ip = o
        .seats
        .iter()
        .find(|s| s.seat == live_ranges.ip_seat)
        .ok_or("Missing IP seat")?;
    if oop.seat == ip.seat
        || oop.participation != HandParticipation::Live
        || ip.participation != HandParticipation::Live
    {
        return Err("Range positions do not match the two live players".into());
    }
    let root_pot = o.pot - oop.street_contribution - ip.street_contribution;
    if root_pot == 0 {
        return Err("River starting pot must be positive".into());
    }
    let total_oop = oop
        .stack
        .checked_add(oop.street_contribution)
        .ok_or("OOP stack overflow")?;
    let total_ip = ip
        .stack
        .checked_add(ip.street_contribution)
        .ok_or("IP stack overflow")?;
    let effective_stack = total_oop.min(total_ip);
    if effective_stack == 0 || effective_stack > 100_000 || root_pot > 1_000_000 {
        return Err("River pot or effective stack exceeds review limits".into());
    }
    let board = o
        .board
        .iter()
        .map(|c| card_from_str(&card_ascii(*c)))
        .collect::<Result<Vec<_>, _>>()?;
    let board: [u8; 5] = board.try_into().map_err(|_| "Expected five board cards")?;
    let hero_cards = (
        card_from_str(&card_ascii(o.hole_cards[0]))?,
        card_from_str(&card_ascii(o.hole_cards[1]))?,
    );
    if hero_cards.0 == hero_cards.1
        || board.contains(&hero_cards.0)
        || board.contains(&hero_cards.1)
    {
        return Err("Hero cards overlap the board".into());
    }
    let ranges: [Range; 2] = [live_ranges.oop.parse()?, live_ranges.ip.parse()?];
    let hero_player = if o.actor == oop.seat { 0 } else { 1 };
    if ranges[hero_player].get_weight_by_cards(hero_cards.0, hero_cards.1) <= 0.0 {
        return Err("The actual hand has no weight in the inferred range".into());
    }
    let board_mask = board.iter().fold(0u64, |mask, &c| mask | (1u64 << c));
    if board_mask.count_ones() != 5 {
        return Err("Board cards must be distinct".into());
    }
    let (hero_hands, _) = ranges[hero_player].get_hands_weights(board_mask);
    let (opponent_hands, _) = ranges[hero_player ^ 1].get_hands_weights(board_mask);
    if hero_hands.is_empty() || opponent_hands.is_empty() {
        return Err("Inferred ranges have no live combinations".into());
    }
    if !opponent_hands.iter().any(|&(a, b)| {
        a != hero_cards.0 && a != hero_cards.1 && b != hero_cards.0 && b != hero_cards.1
    }) {
        return Err("The actual hand has zero compatible opponent reach".into());
    }
    let river: Vec<_> = o
        .public_history
        .iter()
        .filter(|a| a.phase == MultiwayPhase::River)
        .collect();
    let mut contributions = [0, 0];
    let mut wager = 0;
    let mut history = Vec::with_capacity(river.len());
    for (index, record) in river.iter().enumerate() {
        let expected = if index % 2 == 0 { oop.seat } else { ip.seat };
        if record.seat != expected {
            return Err("River action order does not match heads-up positions".into());
        }
        let action = replay_record(
            record,
            index % 2,
            &mut contributions,
            &mut wager,
            effective_stack,
        )?;
        history.push(action);
    }
    if contributions != [oop.street_contribution, ip.street_contribution] || wager != o.wager {
        return Err("River history does not reproduce current contributions".into());
    }
    let expected_actor = if river.len() % 2 == 0 {
        oop.seat
    } else {
        ip.seat
    };
    if o.actor != expected_actor {
        return Err("Current actor does not match river history".into());
    }
    let accepted = map_action(d.accepted_action, wager, effective_stack)?;
    Ok(Context {
        root_pot: root_pot as i32,
        effective_stack: effective_stack as i32,
        hero_player,
        board,
        hero_cards,
        history,
        accepted,
        ranges,
        assumptions: live_ranges.assumptions,
    })
}

fn ensure_action(tree: &mut ActionTree, prefix: &[Action], action: Action) -> Result<(), String> {
    tree.apply_history(prefix)?;
    if !tree.available_actions().contains(&action) {
        if !matches!(action, Action::Bet(_) | Action::Raise(_) | Action::AllIn(_)) {
            return Err("Observed passive action is absent from the solver tree".into());
        }
        let mut line = prefix.to_vec();
        line.push(action);
        tree.add_line(&line)?;
        tree.apply_history(prefix)?;
        if !tree.available_actions().contains(&action) {
            return Err("Exact action size could not be added to the solver tree".into());
        }
    }
    Ok(())
}

fn action_label(action: Action) -> String {
    match action {
        Action::Fold => "fold".into(),
        Action::Check => "check".into(),
        Action::Call => "call".into(),
        Action::Bet(v) => format!("bet {v}"),
        Action::Raise(v) => format!("raise to {v}"),
        Action::AllIn(v) => format!("all-in to {v}"),
        other => format!("{other:?}"),
    }
}

pub fn solve_decision(d: &Decision, cancelled: &AtomicBool) -> Result<Feedback, String> {
    eligibility(d)?;
    if cancelled.load(Ordering::Relaxed) {
        return Err("Solver review cancelled".into());
    }
    let live_ranges = ranges(d)?;
    solve_with_ranges(d, live_ranges, cancelled)
}

fn solve_with_ranges(
    d: &Decision,
    live_ranges: LiveRanges,
    cancelled: &AtomicBool,
) -> Result<Feedback, String> {
    let started = Instant::now();
    let ctx = context(d, live_ranges)?;
    let pool = ThreadPoolBuilder::new()
        .num_threads(THREADS)
        .build()
        .map_err(|e| e.to_string())?;
    let options = BetSizeOptions {
        bet: vec![
            BetSize::PotRelative(0.5),
            BetSize::PotRelative(1.0),
            BetSize::AllIn,
        ],
        raise: vec![BetSize::AllIn],
    };
    let mut tree = pool.install(|| {
        ActionTree::new(TreeConfig {
            initial_state: BoardState::River,
            starting_pot: ctx.root_pot,
            effective_stack: ctx.effective_stack,
            river_bet_sizes: [options.clone(), options],
            add_allin_threshold: 0.0,
            force_allin_threshold: 0.0,
            merging_threshold: 0.0,
            ..Default::default()
        })
    })?;
    for (index, action) in ctx.history.iter().copied().enumerate() {
        ensure_action(&mut tree, &ctx.history[..index], action)?;
    }
    ensure_action(&mut tree, &ctx.history, ctx.accepted)?;
    tree.apply_history(&ctx.history)?;
    let card_config = CardConfig {
        range: ctx.ranges,
        flop: [ctx.board[0], ctx.board[1], ctx.board[2]],
        turn: ctx.board[3],
        river: ctx.board[4],
    };
    if cancelled.load(Ordering::Relaxed) {
        return Err("Solver review cancelled".into());
    }
    let mut game = pool.install(|| PostFlopGame::with_config(card_config, tree))?;
    let (memory, _) = game.memory_usage();
    if memory > MAX_MEMORY_BYTES {
        return Err("Solver tree exceeds the local memory limit".into());
    }
    if started.elapsed() >= MAX_DURATION {
        return Err("Solver review time limit reached while building".into());
    }
    game.allocate_memory(false);
    let target = ctx.root_pot as f32 * 0.005;
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
                if exploitability <= target {
                    break;
                }
            }
        }
    });
    if cancelled.load(Ordering::Relaxed) {
        return Err("Solver review cancelled".into());
    }
    if iterations == 0 {
        return Err("Solver review time limit reached before solving".into());
    }
    if iterations % CHECK_INTERVAL != 0 || !exploitability.is_finite() {
        exploitability = pool.install(|| compute_exploitability(&game));
    }
    if !exploitability.is_finite() || exploitability > target {
        return Err(format!("Solver did not converge within limits (exploitability {exploitability:.2} chips, target {target:.2})"));
    }
    pool.install(|| finalize(&mut game));
    for action in &ctx.history {
        let index = game
            .available_actions()
            .iter()
            .position(|a| a == action)
            .ok_or("Solved tree lost an exact historical action")?;
        game.play(index);
    }
    if game.current_player() != ctx.hero_player || game.is_terminal_node() || game.is_chance_node()
    {
        return Err("Solved node is not the player's river decision".into());
    }
    game.cache_normalized_weights();
    let hero_index = game
        .private_cards(ctx.hero_player)
        .iter()
        .position(|&(a, b)| {
            (a == ctx.hero_cards.0 && b == ctx.hero_cards.1)
                || (a == ctx.hero_cards.1 && b == ctx.hero_cards.0)
        })
        .ok_or("Hero combination is absent from solved range")?;
    if game.normalized_weights(ctx.hero_player)[hero_index] <= 0.0 {
        return Err("Hero combination has zero opponent reach at this decision".into());
    }
    let actions = game.available_actions();
    let selected = actions
        .iter()
        .position(|a| *a == ctx.accepted)
        .ok_or("Accepted action has no exact solver action")?;
    let hand_count = game.private_cards(ctx.hero_player).len();
    let evs = game.expected_values_detail(ctx.hero_player);
    let own_ev = evs[selected * hand_count + hero_index] as f64;
    let (best_index, best_ev) = actions
        .iter()
        .enumerate()
        .map(|(i, _)| (i, evs[i * hand_count + hero_index] as f64))
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .ok_or("No solver actions")?;
    if !own_ev.is_finite() || !best_ev.is_finite() {
        return Err("Nonfinite action value".into());
    }
    let loss = (best_ev - own_ev).max(0.0);
    let gap_for_reconsider = (ctx.root_pot as f64 * 0.02).max(1.0);
    let assessment = if loss >= gap_for_reconsider {
        "reconsider"
    } else {
        "reasonable"
    };
    let alternative = if loss >= gap_for_reconsider {
        Some(action_label(actions[best_index]))
    } else {
        None
    };
    Ok(Feedback {
        version: 1, hand_id: d.observation.hand_id, revision: d.observation.revision,
        assessment: assessment.into(),
        explanation: format!("Under these modeled ranges, {} has {:.1} chips of action EV, {:.1} below the best available modeled action ({}). The whole-tree exploitability is {:.2} chips after {} iterations; this is a model estimate, not a per-hand error bound.",
            action_label(ctx.accepted), own_ev, loss, action_label(actions[best_index]), exploitability, iterations),
        concept: "Heads-up river action EV".into(),
        assumptions: ctx.assumptions.into_iter().chain([format!(
            "Only public history and your cards were used. The action tree has half-pot, pot, and all-in opening bets, all-in raises, and exact observed and accepted sizes. Convergence target: {:.2} chips; limits: {} iterations, {} seconds, {} MiB.",
            target, MAX_ITERATIONS, MAX_DURATION.as_secs(), MAX_MEMORY_BYTES / 1024 / 1024
        )]).collect(),
        evidence_basis: "solver".into(), alternative_action: alternative,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::{
        deck::{Rank, Suit},
        seat::SeatId,
    };
    use crate::trainer::{hero, storage::Settings, Session};

    #[test]
    fn ten_card_uses_solver_notation() {
        assert_eq!(card_ascii(Card::new(Rank::Ten, Suit::Spades)), "Ts");
        assert!(card_from_str(&card_ascii(Card::new(Rank::Ten, Suit::Spades))).is_ok());
    }

    #[test]
    fn replays_exact_bet_and_rejects_mismatched_wager() {
        let seat = SeatId::new(0).unwrap();
        let mut contributions = [0, 0];
        let mut wager = 0;
        let record = PublicAction {
            phase: MultiwayPhase::River,
            seat,
            action: GameAction::Bet(37),
            wager_after: 37,
        };
        assert_eq!(
            replay_record(&record, 0, &mut contributions, &mut wager, 200).unwrap(),
            Action::Bet(37)
        );
        assert_eq!(contributions, [37, 0]);
        let mut wrong = record.clone();
        wrong.wager_after = 38;
        assert!(replay_record(&wrong, 0, &mut [0, 0], &mut 0, 200).is_err());
    }

    #[test]
    fn live_session_reaches_supported_river() {
        let mut found = None;
        for seed in 0..100 {
            let settings = Settings {
                seats: 2,
                ..Settings::default()
            };
            let mut session = Session::new_seeded_for_evaluation(settings, seed).unwrap();
            for _ in 0..100 {
                if session.finished() {
                    break;
                }
                if session.view().to_act == Some(hero()) {
                    let action = session.observation(hero()).unwrap().check_call();
                    let decision = session.submit(action).unwrap().clone();
                    if decision.observation.phase == MultiwayPhase::River
                        && eligibility(&decision).is_ok()
                    {
                        found = Some((seed, decision));
                        break;
                    }
                    session.continue_hand();
                } else {
                    session.step_bot().unwrap();
                }
            }
            if found.is_some() {
                break;
            }
        }
        let (seed, decision) =
            found.expect("a normal seeded session should have a supported river");
        let mut inferred = ranges(&decision).unwrap();
        context(&decision, inferred.clone()).unwrap();
        let hero_cards = &decision.observation.hole_cards;
        let hero_combo = format!("{}{}", card_ascii(hero_cards[0]), card_ascii(hero_cards[1]));
        let available = Rank::ALL
            .iter()
            .flat_map(|rank| {
                [Suit::Spades, Suit::Hearts, Suit::Diamonds, Suit::Clubs]
                    .map(|suit| Card::new(*rank, suit))
            })
            .filter(|card| !decision.observation.board.contains(card) && !hero_cards.contains(card))
            .take(2)
            .collect::<Vec<_>>();
        let opponent_combo = format!("{}{}", card_ascii(available[0]), card_ascii(available[1]));
        if inferred.oop_seat == hero() {
            inferred.oop = hero_combo;
            inferred.ip = opponent_combo;
        } else {
            inferred.ip = hero_combo;
            inferred.oop = opponent_combo;
        }
        let result = solve_with_ranges(&decision, inferred, &AtomicBool::new(false));
        assert!(result.is_ok(), "seed {seed}: {result:?}");
        assert_eq!(result.unwrap().evidence_basis, "solver");
    }
}
