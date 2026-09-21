//! Allowlisted decision data. No authoritative hand, opponent cards or seed crosses this boundary.
use crate::game::{
    actions::Action,
    deck::Card,
    hand::evaluate_hand,
    multiway::{build_pots, Contribution, MultiwayLegalActions, MultiwayPhase},
    seat::SeatId,
    table::HandParticipation,
};
use crate::protocol::{ProjectionKind, TableProjection};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PublicSeat {
    pub seat: SeatId,
    pub stack: u32,
    pub street_contribution: u32,
    pub hand_contribution: u32,
    pub participation: HandParticipation,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    pub hand_id: u64,
    pub revision: u64,
    pub actor: SeatId,
    pub button: SeatId,
    pub small_blind: SeatId,
    pub big_blind: SeatId,
    pub big_blind_chips: u32,
    pub phase: MultiwayPhase,
    pub hole_cards: Vec<Card>,
    pub board: Vec<Card>,
    pub seats: Vec<PublicSeat>,
    pub pot: u32,
    pub wager: u32,
    pub legal: MultiwayLegalActions,
    pub history: Vec<(SeatId, Action)>,
}
impl Observation {
    pub fn from_projection(
        p: &TableProjection,
        revision: u64,
        history: Vec<(SeatId, Action)>,
    ) -> Result<Self, String> {
        let ProjectionKind::Player { seat: actor } = p.audience else {
            return Err("A player observation is required".into());
        };
        if p.to_act != Some(actor) {
            return Err("The player must be acting".into());
        }
        let cards = p
            .seats
            .iter()
            .find(|s| s.seat == actor)
            .and_then(|s| s.hole_cards.clone())
            .ok_or("Missing own cards")?;
        if cards.len() != 2 {
            return Err("Expected two own cards".into());
        }
        Ok(Self {
            hand_id: p.hand_id.0,
            revision,
            actor,
            button: p.button,
            small_blind: p.small_blind,
            big_blind: p.big_blind,
            big_blind_chips: p.big_blind_amount,
            phase: p.phase,
            hole_cards: cards,
            board: p.board.clone(),
            seats: p
                .seats
                .iter()
                .map(|s| PublicSeat {
                    seat: s.seat,
                    stack: s.stack,
                    street_contribution: s.street_contribution,
                    hand_contribution: s.hand_contribution,
                    participation: s.participation,
                })
                .collect(),
            pot: p.pot_total,
            wager: p.current_wager,
            legal: p.legal_actions.clone().ok_or("Missing legal actions")?,
            history,
        })
    }
    pub fn own(&self) -> &PublicSeat {
        self.seats
            .iter()
            .find(|s| s.seat == self.actor)
            .expect("validated observation")
    }
    pub fn call_cost(&self) -> u32 {
        self.wager
            .saturating_sub(self.own().street_contribution)
            .min(self.own().stack)
    }
    pub fn check_call(&self) -> Action {
        if self.legal.can_check {
            Action::Check
        } else if let Some(cost) = self.legal.call_amount {
            Action::Call(cost)
        } else {
            Action::AllIn(self.legal.all_in_to)
        }
    }
    pub fn position(&self) -> String {
        if self.actor == self.button {
            "Button (last to act postflop while still in the hand)".into()
        } else if self.actor == self.small_blind {
            "Small blind (early postflop)".into()
        } else if self.actor == self.big_blind {
            "Big blind".into()
        } else {
            let n = self.seats.len() as u8;
            let distance = (self.actor.as_u8() + n - self.big_blind.as_u8()) % n;
            format!("Seat {} after the big blind", distance)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Facts {
    pub call_cost: u32,
    pub contestable_pot_after_call: u32,
    pub hand_classification: String,
    pub position: String,
    pub effective_remaining_by_opponent: Vec<(SeatId, u32)>,
    pub assumptions: Vec<String>,
}
impl Facts {
    pub fn calculate(o: &Observation) -> Self {
        let call_cost = o.call_cost();
        let contributions: Vec<_> = o
            .seats
            .iter()
            .map(|s| Contribution {
                seat: s.seat,
                amount: s.hand_contribution + if s.seat == o.actor { call_cost } else { 0 },
                eligible: matches!(
                    s.participation,
                    HandParticipation::Live | HandParticipation::AllIn
                ),
            })
            .collect();
        let pots = build_pots(&contributions);
        let contestable_pot_after_call = pots
            .pots
            .iter()
            .filter(|p| p.eligible.contains(&o.actor))
            .map(|p| p.amount)
            .sum();
        Self { call_cost, contestable_pot_after_call, hand_classification:evaluate_hand(&o.hole_cards,&o.board).description, position:o.position(), effective_remaining_by_opponent:o.seats.iter().filter(|s|s.seat!=o.actor && matches!(s.participation,HandParticipation::Live|HandParticipation::AllIn)).map(|s|(s.seat,o.own().stack.saturating_sub(call_cost).min(s.stack))).collect(), assumptions:vec!["Contestable pot assumes this call and no further contributions; excludes unmatched excess and pots this player cannot win.".into(), "No equity or EV calculated. Different side pots can require different winning probabilities. Earlier streets can involve future betting.".into(), "Effective remaining stacks are measured after this hypothetical call, separately for each opponent; zero for an all-in opponent.".into()] }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Decision {
    pub version: u16,
    pub observation: Observation,
    pub accepted_action: Action,
    pub facts: Facts,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Feedback {
    pub version: u16,
    pub hand_id: u64,
    pub revision: u64,
    pub assessment: String,
    pub explanation: String,
    pub concept: String,
    pub assumptions: Vec<String>,
    pub evidence_basis: String,
    pub alternative_action: Option<String>,
}
pub fn local_feedback(d: &Decision) -> Feedback {
    let (concept, explanation)=match d.accepted_action {
        Action::Fold => ("Folding", "Folding gives up this pot and preserves your remaining chips. Judge the choice using what you knew now, not the cards that come later."),
        Action::Check => ("Position", "Checking costs no additional chips and keeps you in the hand. Notice who still acts after you: later position gives you more information."),
        Action::Call(_) => ("Calling", "Calling matches the wager without raising. Before calling, ask which worse hands can continue and how your hand can improve. A cheap call alone does not make it profitable."),
        Action::Bet(_) | Action::Raise(_) => ("Value betting", "A value bet aims to get called by worse hands. Name a plausible worse hand before betting; a bluff instead aims to make better hands fold."),
        Action::AllIn(_) => ("Stack commitment", "An all-in commits your remaining stack. You can win only the pots you are eligible for; chips above a matched contribution are returned. Winning this hand does not prove the choice was good."),
    };
    Feedback {
        version: 1,
        hand_id: d.observation.hand_id,
        revision: d.observation.revision,
        assessment: "uncertain".into(),
        explanation: explanation.into(),
        concept: concept.into(),
        assumptions: vec!["Local teaching heuristic; no opponent range evaluated.".into()],
        evidence_basis: "heuristic".into(),
        alternative_action: None,
    }
}
