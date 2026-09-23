//! Explainable public-history range assumptions for live river practice.
//!
//! These weights are a bounded teaching heuristic. They are not calibrated
//! posteriors and never inspect an opponent's cards or condition an opponent
//! range on the hero's private cards.

use super::facts::{Decision, Observation, PublicAction};
use crate::game::{
    actions::Action,
    deck::{Card, Rank, Suit},
    hand::evaluate_hand,
    multiway::MultiwayPhase,
    seat::SeatId,
    table::HandParticipation,
};

pub const RANGE_MODEL_VERSION: &str = "public-action-heuristic-v1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveRanges {
    pub oop: String,
    pub ip: String,
    pub oop_seat: SeatId,
    pub ip_seat: SeatId,
    pub model_version: String,
    pub assumptions: Vec<String>,
}

/// Builds full, positive weighted ranges in `[OOP, IP]` seat order using only
/// public positions, the public board, and actions before the river.
pub fn ranges(decision: &Decision) -> Result<LiveRanges, String> {
    ranges_from_observation(&decision.observation)
}

fn ranges_from_observation(observation: &Observation) -> Result<LiveRanges, String> {
    if observation.phase != MultiwayPhase::River || observation.board.len() != 5 {
        return Err("Live solver ranges require a five-card river decision".into());
    }
    if observation.public_history.is_empty() {
        return Err("Live solver ranges require street-aware public action history".into());
    }
    let active = observation
        .seats
        .iter()
        .filter(|seat| {
            matches!(
                seat.participation,
                HandParticipation::Live | HandParticipation::AllIn
            )
        })
        .map(|seat| seat.seat)
        .collect::<Vec<_>>();
    if active.len() != 2 {
        return Err("Live solver ranges require exactly two active players".into());
    }
    let table_size = observation.seats.len() as u8;
    let distance_after_button = |seat: SeatId| {
        let distance = (seat.as_u8() + table_size - observation.button.as_u8()) % table_size;
        if distance == 0 {
            table_size
        } else {
            distance
        }
    };
    // Postflop action begins clockwise after the button. With two players left,
    // the later of those two positions is IP even when the button has folded.
    let ip_seat = *active
        .iter()
        .max_by_key(|seat| distance_after_button(**seat))
        .expect("two active seats");
    let oop_seat = active
        .iter()
        .copied()
        .find(|seat| *seat != ip_seat)
        .expect("two distinct active seats");

    let (oop, oop_examples) = weighted_range(observation, oop_seat, false);
    let (ip, ip_examples) = weighted_range(observation, ip_seat, true);
    Ok(LiveRanges {
        oop,
        ip,
        oop_seat,
        ip_seat,
        model_version: RANGE_MODEL_VERSION.into(),
        assumptions: vec![
            format!("Range model: {RANGE_MODEL_VERSION}."),
            "Ranges use a broad positive preflop position prior and public actions through the turn only.".into(),
            "Action likelihood uses only cards public on that action's street; river action is replayed by the solver.".into(),
            "Weights are an explainable teaching heuristic, not a calibrated posterior or GTO range.".into(),
            "Opponent weights do not use the hero's private cards; the final public river board removes impossible combinations.".into(),
            format!("OOP highest-weight examples: {}.", oop_examples.join(", ")),
            format!("IP highest-weight examples: {}.", ip_examples.join(", ")),
        ],
    })
}

fn weighted_range(
    observation: &Observation,
    seat: SeatId,
    in_position: bool,
) -> (String, Vec<String>) {
    let mut combos = candidate_combos(&observation.board)
        .into_iter()
        .map(|cards| {
            let mut weight = preflop_prior(cards, in_position);
            for action in observation
                .public_history
                .iter()
                .filter(|action| action.seat == seat && action.phase != MultiwayPhase::River)
            {
                weight *= action_likelihood(
                    cards,
                    action,
                    &observation.board,
                    observation.big_blind_chips,
                );
            }
            (cards, weight.max(0.000_1))
        })
        .collect::<Vec<_>>();
    let maximum = combos
        .iter()
        .map(|(_, weight)| *weight)
        .fold(0.0_f64, f64::max)
        .max(0.000_1);
    let mut examples = combos.clone();
    examples.sort_by(|left, right| right.1.total_cmp(&left.1));
    let examples = examples
        .into_iter()
        .take(8)
        .map(|(cards, weight)| {
            format!(
                "{}{} ({:.0}%)",
                card_code(cards[0]),
                card_code(cards[1]),
                100.0 * weight / maximum
            )
        })
        .collect();
    let serialized = combos
        .drain(..)
        .map(|(cards, weight)| {
            format!(
                "{}{}:{:.4}",
                card_code(cards[0]),
                card_code(cards[1]),
                (weight / maximum).clamp(0.000_1, 1.0)
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    (serialized, examples)
}

fn candidate_combos(board: &[Card]) -> Vec<[Card; 2]> {
    let suits = [Suit::Spades, Suit::Hearts, Suit::Diamonds, Suit::Clubs];
    let deck = Rank::ALL
        .iter()
        .rev()
        .flat_map(|rank| suits.map(|suit| Card::new(*rank, suit)))
        .filter(|card| !board.contains(card))
        .collect::<Vec<_>>();
    let mut result = Vec::with_capacity(deck.len() * deck.len() / 2);
    for first in 0..deck.len() {
        for second in first + 1..deck.len() {
            result.push([deck[first], deck[second]]);
        }
    }
    result
}

fn preflop_prior(cards: [Card; 2], in_position: bool) -> f64 {
    let high = cards[0].rank.max(cards[1].rank) as u8 as f64;
    let low = cards[0].rank.min(cards[1].rank) as u8 as f64;
    let pair = cards[0].rank == cards[1].rank;
    let suited = cards[0].suit == cards[1].suit;
    let gap = (high - low).abs();
    let strength = if pair {
        0.45 + high / 28.0
    } else {
        0.08 + (high + low) / 42.0 + if suited { 0.10 } else { 0.0 } + (4.0 - gap).max(0.0) * 0.025
    }
    .clamp(0.05, 1.0);
    if in_position {
        0.28 + 0.72 * strength
    } else {
        0.16 + 0.84 * strength
    }
}

fn action_likelihood(
    cards: [Card; 2],
    public_action: &PublicAction,
    river_board: &[Card],
    big_blind: u32,
) -> f64 {
    let visible = match public_action.phase {
        MultiwayPhase::Preflop => 0,
        MultiwayPhase::Flop => 3,
        MultiwayPhase::Turn => 4,
        MultiwayPhase::River | MultiwayPhase::Showdown | MultiwayPhase::HandComplete => 5,
    };
    let strength = evaluate_hand(&cards, &river_board[..visible]).strength();
    let base = match public_action.action {
        Action::Fold => 0.08 + 0.82 * (1.0 - strength),
        Action::Check => 0.55 + 0.35 * (1.0 - strength),
        Action::Call(_) => 0.25 + 0.75 * strength,
        Action::Bet(_) | Action::Raise(_) | Action::AllIn(_) => 0.12 + 0.88 * strength,
    };
    let wager_bb = f64::from(public_action.wager_after) / f64::from(big_blind.max(1));
    let sizing_tilt = 1.0 + wager_bb.ln_1p().min(5.0) * 0.08 * (strength - 0.5);
    (base * sizing_tilt).max(0.01)
}

fn card_code(card: Card) -> String {
    let rank = match card.rank {
        Rank::Ten => "T",
        _ => card.rank.symbol(),
    };
    let suit = match card.suit {
        Suit::Spades => 's',
        Suit::Hearts => 'h',
        Suit::Diamonds => 'd',
        Suit::Clubs => 'c',
    };
    format!("{rank}{suit}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::{multiway::MultiwayLegalActions, table::HandParticipation};
    use crate::trainer::facts::PublicSeat;

    fn seat(index: u8) -> SeatId {
        SeatId::new(index).unwrap()
    }

    fn card(rank: Rank, suit: Suit) -> Card {
        Card::new(rank, suit)
    }

    fn observation() -> Observation {
        Observation {
            hand_id: 1,
            revision: 8,
            actor: seat(0),
            button: seat(1),
            small_blind: seat(1),
            big_blind: seat(0),
            big_blind_chips: 2,
            phase: MultiwayPhase::River,
            hole_cards: vec![card(Rank::Ace, Suit::Spades), card(Rank::Ace, Suit::Hearts)],
            board: vec![
                card(Rank::Two, Suit::Spades),
                card(Rank::Three, Suit::Hearts),
                card(Rank::Four, Suit::Diamonds),
                card(Rank::Six, Suit::Clubs),
                card(Rank::Seven, Suit::Spades),
            ],
            seats: vec![
                PublicSeat {
                    seat: seat(0),
                    stack: 100,
                    street_contribution: 0,
                    hand_contribution: 20,
                    participation: HandParticipation::Live,
                },
                PublicSeat {
                    seat: seat(1),
                    stack: 100,
                    street_contribution: 0,
                    hand_contribution: 20,
                    participation: HandParticipation::Live,
                },
            ],
            pot: 40,
            wager: 0,
            legal: MultiwayLegalActions {
                can_fold: false,
                can_check: true,
                call_amount: None,
                min_bet_to: Some(2),
                min_raise_to: None,
                all_in_to: 100,
                raise_reopened: true,
            },
            history: vec![],
            public_history: vec![
                PublicAction {
                    phase: MultiwayPhase::Preflop,
                    seat: seat(1),
                    action: Action::Raise(6),
                    wager_after: 6,
                },
                PublicAction {
                    phase: MultiwayPhase::Flop,
                    seat: seat(0),
                    action: Action::Check,
                    wager_after: 0,
                },
                PublicAction {
                    phase: MultiwayPhase::Turn,
                    seat: seat(1),
                    action: Action::Bet(12),
                    wager_after: 12,
                },
            ],
        }
    }

    #[test]
    fn full_ranges_and_opponent_weights_ignore_hero_private_cards() {
        let original = ranges_from_observation(&observation()).unwrap();
        let mut changed = observation();
        changed.hole_cards = vec![
            card(Rank::King, Suit::Spades),
            card(Rank::Queen, Suit::Spades),
        ];
        let changed = ranges_from_observation(&changed).unwrap();
        assert_eq!(original, changed);
        assert_eq!(original.oop.split(',').count(), 1_081);
        assert_eq!(original.ip.split(',').count(), 1_081);
        let _: postflop_solver::Range = original.oop.parse().unwrap();
        let _: postflop_solver::Range = original.ip.parse().unwrap();
        assert_eq!((original.oop_seat, original.ip_seat), (seat(0), seat(1)));
    }

    #[test]
    fn action_strength_uses_only_the_board_known_on_that_street() {
        let action = PublicAction {
            phase: MultiwayPhase::Flop,
            seat: seat(0),
            action: Action::Bet(8),
            wager_after: 8,
        };
        let cards = [card(Rank::Ace, Suit::Clubs), card(Rank::King, Suit::Clubs)];
        let first = observation().board;
        let mut different_future = first.clone();
        different_future[3] = card(Rank::Ace, Suit::Diamonds);
        different_future[4] = card(Rank::King, Suit::Diamonds);
        assert_eq!(
            action_likelihood(cards, &action, &first, 2),
            action_likelihood(cards, &action, &different_future, 2)
        );
    }

    #[test]
    fn river_actions_do_not_feed_the_root_range_model() {
        let baseline = ranges_from_observation(&observation()).unwrap();
        let mut with_river_action = observation();
        with_river_action.public_history.push(PublicAction {
            phase: MultiwayPhase::River,
            seat: seat(1),
            action: Action::AllIn(100),
            wager_after: 100,
        });
        assert_eq!(
            baseline,
            ranges_from_observation(&with_river_action).unwrap()
        );
    }

    #[test]
    fn position_uses_live_order_when_the_button_folded() {
        let mut three_handed = observation();
        three_handed.button = seat(0);
        three_handed.actor = seat(1);
        three_handed.seats = vec![
            PublicSeat {
                seat: seat(0),
                stack: 90,
                street_contribution: 0,
                hand_contribution: 10,
                participation: HandParticipation::Folded,
            },
            PublicSeat {
                seat: seat(1),
                stack: 100,
                street_contribution: 0,
                hand_contribution: 20,
                participation: HandParticipation::Live,
            },
            PublicSeat {
                seat: seat(2),
                stack: 100,
                street_contribution: 0,
                hand_contribution: 20,
                participation: HandParticipation::Live,
            },
        ];
        let inferred = ranges_from_observation(&three_handed).unwrap();
        assert_eq!((inferred.oop_seat, inferred.ip_seat), (seat(1), seat(2)));
    }

    #[test]
    fn legacy_river_without_street_history_fails_closed() {
        let mut legacy = observation();
        legacy.public_history.clear();
        assert!(ranges_from_observation(&legacy)
            .unwrap_err()
            .contains("street-aware public action history"));
    }
}
