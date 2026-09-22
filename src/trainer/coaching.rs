//! Deterministic teaching rules built only from a frozen pre-decision observation.
//! These are reviewed heuristics, not equity, EV, or solver output.
use super::facts::{Decision, Feedback, Observation};
use crate::game::{actions::Action, deck::Rank, multiway::MultiwayPhase, table::HandParticipation};

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
    let action = action_kind(&d.accepted_action);
    let (concept, explanation)=match action {
        "check" => ("Postflop: free continuation", format!("Checking adds no chips. Your shown hand is {}. Reassess after later players act; this rule does not estimate the chance of winning.",d.facts.hand_classification)),
        "call" => ("Postflop calling price",format!("The legal call price and contestable pot are exact engine facts shown below, but whether {} is strong enough depends on unknown ranges and future action.",d.facts.hand_classification)),
        "raise" => ("Postflop: betting purpose",format!("With {}, name the purpose: value expects worse hands to continue; a bluff expects better hands to fold. This heuristic cannot prove either without opponent ranges.",d.facts.hand_classification)),
        "all-in" => ("Postflop: stack commitment",format!("All-in commits the remaining stack with {}. Eligible side pots are exact engine facts; strategic quality remains uncertain without ranges.",d.facts.hand_classification)),
        _ => ("Postflop: folding",format!("Folding preserves the remaining stack but gives up this pot with {}. Future cards are deliberately excluded from this review.",d.facts.hand_classification)),
    };
    Feedback {
        version: 1,
        hand_id: o.hand_id,
        revision: o.revision,
        assessment: "uncertain".into(),
        explanation,
        concept: concept.into(),
        assumptions: vec!["No opponent range or future card evaluated.".into()],
        evidence_basis: "heuristic".into(),
        alternative_action: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        game::{
            deck::{Card, Suit},
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
}
