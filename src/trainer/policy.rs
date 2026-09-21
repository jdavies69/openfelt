//! Heuristic opponents use only their own observation and an independent RNG.
use super::facts::Observation;
use crate::game::{
    actions::Action,
    hand::{evaluate_hand, HandRank},
};
use rand::{rngs::StdRng, Rng, SeedableRng};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum, Default)]
#[serde(rename_all = "snake_case")]
pub enum Profile {
    #[default]
    Fundamentals,
    Recreational,
    Competent,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicySettings {
    pub profile: Profile,
    pub aggression: f64,
    pub bluff_rate: f64,
    pub mistake_rate: f64,
}
impl Default for PolicySettings {
    fn default() -> Self {
        Self {
            profile: Profile::Fundamentals,
            aggression: 0.55,
            bluff_rate: 0.03,
            mistake_rate: 0.05,
        }
    }
}
pub struct Opponent {
    rng: StdRng,
    settings: PolicySettings,
}
impl Opponent {
    pub fn new(seed: u64, settings: PolicySettings) -> Self {
        Self {
            rng: StdRng::seed_from_u64(seed),
            settings,
        }
    }
    pub fn choose(&mut self, o: &Observation) -> Action {
        let a = o.hole_cards[0];
        let b = o.hole_cards[1];
        let high = (a.rank as u8).max(b.rank as u8);
        let low = (a.rank as u8).min(b.rank as u8);
        // Explicit starting ranges: pairs, broadway, suited aces; recreational adds suited connectors.
        let premium = (a.rank == b.rank && high >= 10) || (high == 14 && low >= 12);
        let range = a.rank == b.rank
            || (high >= 11 && low >= 10)
            || (high == 14 && a.suit == b.suit)
            || (self.settings.profile == Profile::Recreational
                && a.suit == b.suit
                && high - low == 1);
        let rank = evaluate_hand(&o.hole_cards, &o.board).rank;
        let strong = if o.board.is_empty() {
            premium
        } else {
            rank >= HandRank::TwoPair
        };
        let playable = if o.board.is_empty() {
            range
        } else {
            rank >= HandRank::Pair
        };
        let mistake = self.rng.gen_bool(self.settings.mistake_rate);
        let bluff = self.rng.gen_bool(self.settings.bluff_rate);
        if (strong || bluff) && self.rng.gen_bool(self.settings.aggression) {
            if let Some(min) = o.legal.min_raise_to.or(o.legal.min_bet_to) {
                let desired = if o.board.is_empty() {
                    o.big_blind_chips.saturating_mul(3)
                } else {
                    o.wager.saturating_add((o.pot / 2).max(o.big_blind_chips))
                };
                let to = desired.max(min).min(o.legal.all_in_to.saturating_sub(1));
                return if o.legal.min_raise_to.is_some() {
                    Action::Raise(to)
                } else {
                    Action::Bet(to)
                };
            }
        }
        let price_limit = match self.settings.profile {
            Profile::Fundamentals => 4,
            Profile::Recreational => 10,
            Profile::Competent => 3,
        };
        if o.legal.can_check
            || strong
            || (playable && o.call_cost() <= o.big_blind_chips * price_limit)
            || mistake
        {
            o.check_call()
        } else {
            Action::Fold
        }
    }
}
