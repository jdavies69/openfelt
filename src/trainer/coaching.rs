//! Deterministic teaching rules built only from a frozen pre-decision observation.
//! These are reviewed heuristics, not equity, EV, or solver output.
use super::facts::{Decision, Feedback, Observation};
use crate::bot::draws::detect_draws;
use crate::game::{
    actions::Action,
    deck::{Card, Rank},
    hand::{evaluate_hand, HandRank},
    multiway::MultiwayPhase,
    table::HandParticipation,
};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TablePosition {
    Early,
    Middle,
    Late,
    SmallBlind,
    BigBlind,
}

pub fn table_position(o: &Observation) -> TablePosition {
    if o.actor == o.button {
        return TablePosition::Late;
    }
    if o.actor == o.small_blind {
        return TablePosition::SmallBlind;
    }
    if o.actor == o.big_blind {
        return TablePosition::BigBlind;
    }
    let active: Vec<_> = o
        .seats
        .iter()
        // Folded seats retain the position structure from the start of the hand.
        .filter(|s| s.participation != HandParticipation::NotDealt)
        .map(|s| s.seat)
        .collect();
    let button_index = active.iter().position(|s| *s == o.button).unwrap_or(0);
    let actor_index = active.iter().position(|s| *s == o.actor).unwrap_or(0);
    let distance_to_button = (button_index + active.len() - actor_index) % active.len();
    if distance_to_button <= 1 {
        TablePosition::Late
    } else {
        let early_from = match active.len() {
            0..=5 => 2,
            6 => 3,
            7..=8 => 4,
            _ => 5,
        };
        if distance_to_button >= early_from {
            TablePosition::Early
        } else {
            TablePosition::Middle
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PreflopBand {
    Fold,
    LateOpen,
    Open,
    Strong,
    Premium,
}

pub fn preflop_band(o: &Observation) -> PreflopBand {
    let a = o.hole_cards[0];
    let b = o.hole_cards[1];
    let hi = (a.rank as u8).max(b.rank as u8);
    let lo = (a.rank as u8).min(b.rank as u8);
    let pair = a.rank == b.rank;
    let suited = a.suit == b.suit;
    let gap = hi.saturating_sub(lo);
    if pair && hi >= Rank::Ten as u8 || hi == Rank::Ace as u8 && lo >= Rank::Queen as u8 {
        PreflopBand::Premium
    } else if pair && hi >= Rank::Seven as u8
        || hi >= Rank::Queen as u8 && lo >= Rank::Ten as u8
        || hi == Rank::Ace as u8 && suited && lo >= Rank::Ten as u8
    {
        PreflopBand::Strong
    } else if pair
        || hi == Rank::Ace as u8 && lo >= Rank::Ten as u8
        || suited && hi >= Rank::Ten as u8 && lo >= Rank::Nine as u8
    {
        PreflopBand::Open
    } else if hi == Rank::Ace as u8 && suited
        || suited && hi >= Rank::Seven as u8 && gap <= 2
        || hi == Rank::King as u8 && lo >= Rank::Nine as u8
    {
        PreflopBand::LateOpen
    } else {
        PreflopBand::Fold
    }
}

fn action_kind(a: &Action) -> &'static str {
    match a {
        Action::Fold => "fold",
        Action::Check => "check",
        Action::Call(_) => "call",
        Action::Bet(_) | Action::Raise(_) => "raise",
        Action::AllIn(_) => "all-in",
    }
}
fn has_prior_raise(o: &Observation) -> bool {
    o.history
        .iter()
        .any(|(_, a)| matches!(a, Action::Raise(_) | Action::AllIn(_)))
}
fn has_prior_limp(o: &Observation) -> bool {
    o.history.iter().any(|(_, a)| matches!(a, Action::Call(_)))
}
fn price_description(d: &Decision) -> &'static str {
    if d.facts.call_cost == 0 {
        "no additional call"
    } else if d.facts.call_cost.saturating_mul(4) <= d.facts.contestable_pot_after_call {
        "a relatively small call"
    } else {
        "a material call"
    }
}
fn stack_description(d: &Decision) -> &'static str {
    let shortest = d
        .facts
        .effective_remaining_by_opponent
        .iter()
        .map(|(_, s)| *s)
        .min()
        .unwrap_or(0);
    if shortest <= d.observation.big_blind_chips.saturating_mul(12) {
        "short effective stacks"
    } else {
        "room for later betting"
    }
}

pub fn feedback(d: &Decision) -> Feedback {
    if d.observation.phase != MultiwayPhase::Preflop {
        return postflop_feedback(d);
    }
    let o = &d.observation;
    let band = preflop_band(o);
    let pos = table_position(o);
    let raised = has_prior_raise(o);
    let limped = !raised && has_prior_limp(o);
    let action = action_kind(&d.accepted_action);
    let (assessment, concept, explanation, alternative) = if raised {
        match (band, action) {
            (PreflopBand::Premium, "raise" | "all-in") => ("reasonable", "Preflop facing a raise", "This hand is in the trainer's strongest starting band. Re-raising can build the pot with a strong holding; stack depth and the raiser's range still matter.".into(), Some("check_call".into())),
            (PreflopBand::Fold | PreflopBand::LateOpen, "fold") => ("reasonable", "Preflop facing a raise", "This hand is outside the trainer's continue range against prior aggression. Folding avoids paying with a hand that is often dominated.".into(), None),
            (PreflopBand::Fold, "call" | "raise" | "all-in") => ("reconsider", "Preflop facing a raise", "This hand is in the trainer's weakest band. Continuing needs a specific read this local heuristic does not have.".into(), Some("fold".into())),
            _ => ("uncertain", "Preflop facing a raise", format!("This spot can be close against prior aggression with {} and {}. The raiser's range can change the choice.", price_description(d), stack_description(d)), Some("fold".into())),
        }
    } else if limped {
        match (band, action) {
            (PreflopBand::Strong | PreflopBand::Premium, "raise") => ("reasonable", "Preflop after limpers", format!("This strong starting band can raise over limpers for value. {} affects how readily stacks may become committed.",stack_description(d)), Some("check_call".into())),
            (PreflopBand::Fold, "call" | "raise" | "all-in") => ("reconsider", "Preflop after limpers", format!("Limped action does not make the weakest starting band automatically playable, even with {}.",price_description(d)), Some("fold".into())),
            _ => ("uncertain", "Preflop after limpers", format!("Prior callers mean this is not an unopened pot. Weigh position, {}, and {} before adding chips.",price_description(d),stack_description(d)), Some("fold".into())),
        }
    } else {
        let required = match pos {
            TablePosition::Early => PreflopBand::Strong,
            TablePosition::Middle => PreflopBand::Open,
            TablePosition::Late => PreflopBand::LateOpen,
            TablePosition::SmallBlind => PreflopBand::Open,
            TablePosition::BigBlind => PreflopBand::LateOpen,
        };
        match (band >= required, action) {
            (true, "raise") => ("reasonable", "Preflop position-aware opening", format!("This hand clears the trainer's {:?} opening threshold from {:?}. Raising first-in applies pressure and avoids entering passively.", required, pos), None),
            (false, "fold") => ("reasonable", "Preflop position-aware opening", format!("This hand falls below the trainer's {:?} opening threshold from {:?}. Later seats may open more hands because fewer players remain.", required, pos), None),
            (false, "call" | "raise" | "all-in") => ("reconsider", "Preflop position-aware opening", format!("This hand falls below the trainer's {:?} opening threshold from {:?}. Entering needs a table-specific reason this heuristic cannot observe.", required, pos), Some("fold".into())),
            _ => ("uncertain", "Preflop position-aware opening", format!("This hand meets the trainer's {:?} threshold from {:?}, but the accepted {} does not clearly express a first-in value plan.", required, pos, action), if o.legal.min_raise_to.is_some(){Some("raise".into())}else{None}),
        }
    };
    Feedback {
        version: 1,
        hand_id: o.hand_id,
        revision: o.revision,
        assessment: assessment.into(),
        explanation,
        concept: concept.into(),
        assumptions: vec![
            "Agent-reviewed deterministic starting-hand bands; not a numerical strength calculation."
                .into(),
            "Opponent ranges and table-specific reads are unknown.".into(),
        ],
        evidence_basis: "heuristic".into(),
        alternative_action: alternative,
    }
}

fn postflop_feedback(d: &Decision) -> Feedback {
    let o = &d.observation;
    let rank = evaluate_hand(&o.hole_cards, &o.board).rank;
    let draws = detect_draws(&o.hole_cards, &o.board);
    let made = made_label(rank);
    let draw = draw_label(draws.flush_draw, draws.oesd, draws.gutshot);
    let texture = board_texture(&o.board);
    let place = if matches!(table_position(o), TablePosition::Late) {
        "later position"
    } else {
        "with players still to act"
    };
    let price = price_description(d);
    let strong = rank >= HandRank::TwoPair;
    let paired = rank >= HandRank::Pair;
    let live_draw = draws.flush_draw || draws.oesd;
    let action = action_kind(&d.accepted_action);
    let (assessment, concept, explanation, alternative) = match action {
        "raise" | "all-in" if strong => (
            "reasonable",
            "Postflop: value betting",
            format!(
                "Betting {made} can ask worse hands to continue for value. {texture} You are {place}. Unknown ranges can still change whether this is the size or the line to choose."
            ),
            Some("check_call".into()),
        ),
        "raise" | "all-in" if !paired && !live_draw => (
            "reconsider",
            "Postflop: betting purpose",
            format!(
                "Betting {made} with {draw} needs a better hand that can fold. This heuristic cannot see that target. {texture}"
            ),
            Some("check_call".into()),
        ),
        "raise" | "all-in" => (
            "uncertain",
            "Postflop: betting purpose",
            format!(
                "A bet with {made} and {draw} can be value, a bluff, or both. {price} {texture} Name the hands that continue or fold before treating one purpose as established."
            ),
            Some("check_call".into()),
        ),
        "call" if strong => (
            "reasonable",
            "Postflop calling price",
            format!(
                "Calling with {made} continues with a strong shown hand and {price}. A raise is a separate value plan, not required by this rule."
            ),
            alt_raise(o),
        ),
        "call" if !paired && !live_draw && d.facts.call_cost > 0 => (
            "reconsider",
            "Postflop calling price",
            format!(
                "Calling with {made} and {draw} pays {price}. {texture} Continuing needs a reason this heuristic does not have."
            ),
            alt_fold(o),
        ),
        "call" => (
            "uncertain",
            "Postflop calling price",
            format!(
                "The call uses {price} with {made} and {draw}. {texture} Whether that price is attractive depends on ranges and future action, which this rule does not score."
            ),
            alt_fold(o).or_else(|| alt_raise(o)),
        ),
        "check" if strong => (
            "uncertain",
            "Postflop: value betting",
            format!(
                "Checking {made} can trap or miss a value bet. Both can be defensible {place}. {texture}"
            ),
            alt_raise(o),
        ),
        "check" => (
            "reasonable",
            "Postflop: free continuation",
            format!(
                "Checking adds no chips with {made} and {draw}. {texture} Reassess after later players act."
            ),
            alt_raise(o),
        ),
        _ if strong => (
            "reconsider",
            "Postflop: folding",
            format!(
                "Folding {made} gives up this pot. A continue was available. {texture} Future cards stay out of this review."
            ),
            Some("check_call".into()),
        ),
        _ if d.facts.call_cost > 0 && !paired && !live_draw => (
            "reasonable",
            "Postflop: folding",
            format!(
                "Folding {made} with {draw} avoids {price}. {texture} Future cards stay out of this review."
            ),
            Some("check_call".into()),
        ),
        _ => (
            "uncertain",
            "Postflop: folding",
            format!(
                "Folding preserves the remaining stack with {made} and {draw}. {texture} This rule does not prove the fold is required."
            ),
            Some("check_call".into()),
        ),
    };
    Feedback {
        version: 1,
        hand_id: o.hand_id,
        revision: o.revision,
        assessment: assessment.into(),
        explanation,
        concept: concept.into(),
        assumptions: vec![
            "Made-hand category and draw labels use only your cards and the current board.".into(),
            "No opponent range, future card, or numeric chance of winning is evaluated.".into(),
        ],
        evidence_basis: "heuristic".into(),
        alternative_action: alternative,
    }
}

fn made_label(rank: HandRank) -> &'static str {
    match rank {
        HandRank::HighCard => "a high-card hand",
        HandRank::Pair => "one pair",
        HandRank::TwoPair => "two pair",
        HandRank::ThreeOfAKind => "three of a kind",
        HandRank::Straight => "a straight",
        HandRank::Flush => "a flush",
        HandRank::FullHouse => "a full house",
        HandRank::FourOfAKind => "four of a kind",
        HandRank::StraightFlush => "a straight flush",
    }
}

fn draw_label(flush: bool, open_ended: bool, gutshot: bool) -> &'static str {
    match (flush, open_ended, gutshot) {
        (true, true, _) | (true, _, true) => "a flush draw and a straight draw",
        (true, false, false) => "a flush draw",
        (false, true, _) => "an open-ended straight draw",
        (false, false, true) => "a gutshot straight draw",
        (false, false, false) => "no flush or open-ended straight draw",
    }
}

fn board_texture(board: &[Card]) -> &'static str {
    if board.len() < 3 {
        return "The board is not complete.";
    }
    let mut ranks = BTreeMap::new();
    let mut suits = BTreeMap::new();
    for card in board {
        *ranks.entry(card.rank).or_insert(0u8) += 1;
        *suits.entry(card.suit.symbol()).or_insert(0u8) += 1;
    }
    let paired = ranks.values().any(|count| *count >= 2);
    let suited = suits.values().any(|count| *count >= 3);
    match (paired, suited) {
        (true, true) => "The public board is paired and has several cards of one suit.",
        (true, false) => "The public board is paired.",
        (false, true) => "The public board has several cards of one suit.",
        (false, false) => "The public board is unpaired.",
    }
}

fn alt_fold(o: &Observation) -> Option<String> {
    o.legal.can_fold.then(|| "fold".into())
}

fn alt_raise(o: &Observation) -> Option<String> {
    (o.legal.min_raise_to.is_some() || o.legal.min_bet_to.is_some()).then(|| "raise".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        game::{
            deck::{Card, Rank, Suit},
            seat::SeatId,
        },
        trainer::{hero, storage::Settings, Session},
    };
    fn decision() -> Decision {
        let mut s = Session::new(Settings::default()).unwrap();
        while s.view().to_act != Some(hero()) {
            s.step_bot().unwrap();
        }
        let action = s.observation(hero()).unwrap().check_call();
        s.submit(action).unwrap().clone()
    }
    #[test]
    fn six_max_positions_keep_original_structure_after_folds() {
        let mut d = decision();
        let o = &mut d.observation;
        o.button = SeatId::new(0).unwrap();
        o.small_blind = SeatId::new(1).unwrap();
        o.big_blind = SeatId::new(2).unwrap();
        o.actor = SeatId::new(3).unwrap();
        o.seats[4].participation = HandParticipation::Folded;
        assert_eq!(table_position(o), TablePosition::Early);
        o.actor = SeatId::new(5).unwrap();
        assert_eq!(table_position(o), TablePosition::Late);
    }
    #[test]
    fn heads_up_button_small_blind_uses_late_opening_position() {
        let mut d = decision();
        d.observation.seats.truncate(2);
        d.observation.actor = SeatId::new(0).unwrap();
        d.observation.button = d.observation.actor;
        d.observation.small_blind = d.observation.actor;
        d.observation.big_blind = SeatId::new(1).unwrap();
        assert_eq!(table_position(&d.observation), TablePosition::Late);
    }
    #[test]
    fn guidance_changes_with_hand_position_and_prior_action() {
        let mut d = decision();
        d.observation.history.clear();
        d.observation.actor = d.observation.button;
        d.accepted_action = Action::Raise(d.observation.legal.min_raise_to.unwrap_or(6));
        d.observation.hole_cards = vec![
            Card::new(Rank::Ace, Suit::Spades),
            Card::new(Rank::Eight, Suit::Spades),
        ];
        let late = feedback(&d);
        assert_eq!(late.assessment, "reasonable");
        d.observation.actor = SeatId::new(3).unwrap();
        d.observation.hole_cards = vec![
            Card::new(Rank::Seven, Suit::Clubs),
            Card::new(Rank::Two, Suit::Diamonds),
        ];
        let early = feedback(&d);
        assert_eq!(early.assessment, "reconsider");
        assert_ne!(late.explanation, early.explanation);
        d.observation
            .history
            .push((SeatId::new(4).unwrap(), Action::Raise(8)));
        assert_eq!(feedback(&d).concept, "Preflop facing a raise");
        d.observation.history = vec![(SeatId::new(4).unwrap(), Action::Call(2))];
        assert_eq!(feedback(&d).concept, "Preflop after limpers");
    }
    #[test]
    fn postflop_guidance_uses_made_hand_draw_and_price_without_invented_numbers() {
        use crate::trainer::provider::validate_feedback;
        let mut d = decision();
        d.observation.phase = MultiwayPhase::Flop;
        d.observation.history.clear();
        d.observation.board = vec![
            Card::new(Rank::Ace, Suit::Clubs),
            Card::new(Rank::King, Suit::Diamonds),
            Card::new(Rank::Two, Suit::Hearts),
        ];
        d.observation.hole_cards = vec![
            Card::new(Rank::Ace, Suit::Spades),
            Card::new(Rank::Ace, Suit::Diamonds),
        ];
        d.observation.legal.can_check = true;
        d.observation.legal.can_fold = false;
        d.observation.legal.call_amount = None;
        d.observation.legal.min_bet_to = Some(2);
        d.accepted_action = Action::Bet(6);
        d.facts = super::super::facts::Facts::calculate(&d.observation);
        let value = feedback(&d);
        assert_eq!(value.assessment, "reasonable");
        assert_eq!(value.concept, "Postflop: value betting");
        assert!(value.explanation.contains("three of a kind"));
        validate_feedback(&value, &d).unwrap();

        d.observation.board = vec![
            Card::new(Rank::King, Suit::Diamonds),
            Card::new(Rank::Queen, Suit::Hearts),
            Card::new(Rank::Three, Suit::Clubs),
        ];
        d.observation.hole_cards = vec![
            Card::new(Rank::Seven, Suit::Clubs),
            Card::new(Rank::Two, Suit::Diamonds),
        ];
        d.facts = super::super::facts::Facts::calculate(&d.observation);
        let air = feedback(&d);
        assert_eq!(air.assessment, "reconsider");
        assert_ne!(air.explanation, value.explanation);
        assert!(air.explanation.contains("high-card"));
        validate_feedback(&air, &d).unwrap();

        d.observation.legal.can_check = false;
        d.observation.legal.can_fold = true;
        d.observation.legal.call_amount = Some(20);
        d.observation.legal.min_bet_to = None;
        d.accepted_action = Action::Fold;
        d.facts = super::super::facts::Facts::calculate(&d.observation);
        let folded = feedback(&d);
        assert_eq!(folded.assessment, "reasonable");
        assert!(folded.explanation.contains("no flush"));
        for text in [&value.explanation, &air.explanation, &folded.explanation] {
            assert!(!text.chars().any(|c| c.is_ascii_digit()));
            assert!(!text.to_lowercase().contains("equity"));
        }
    }
}
