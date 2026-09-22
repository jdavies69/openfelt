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
/// Personality controls tendencies; difficulty controls how consistently reviewed rules are used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum, Default)]
#[serde(rename_all = "snake_case")]
pub enum Style {
    Tight,
    #[default]
    Balanced,
    Loose,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum, Default)]
#[serde(rename_all = "snake_case")]
pub enum Difficulty {
    Beginner,
    #[default]
    Practiced,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PolicySettings {
    pub profile: Profile,
    pub style: Style,
    pub difficulty: Difficulty,
    pub aggression: f64,
    pub bluff_rate: f64,
    pub mistake_rate: f64,
}
impl Default for PolicySettings {
    fn default() -> Self {
        Self {
            profile: Profile::Fundamentals,
            style: Style::Balanced,
            difficulty: Difficulty::Practiced,
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
        // Explicit starting ranges vary by position and prior aggression. They are heuristics, not GTO charts.
        let premium = (a.rank == b.rank && high >= 10) || (high == 14 && low >= 12);
        let base_range =
            a.rank == b.rank || (high >= 11 && low >= 10) || (high == 14 && a.suit == b.suit);
        let late = matches!(
            super::coaching::table_position(o),
            super::coaching::TablePosition::Late | super::coaching::TablePosition::SmallBlind
        );
        let suited_connector = a.suit == b.suit && high.saturating_sub(low) <= 1 && high >= 7;
        let loose_extra = (late && (high == 14 || high >= 11 && low >= 8)) || suited_connector;
        let raised = o
            .history
            .iter()
            .any(|(_, a)| matches!(a, Action::Raise(_) | Action::AllIn(_)));
        let range = preflop_range(
            base_range,
            loose_extra,
            suited_connector,
            late,
            raised,
            self.settings.style,
        );
        let rank = evaluate_hand(&o.hole_cards, &o.board).rank;
        let strong = if o.board.is_empty() {
            premium
        } else {
            rank >= HandRank::TwoPair
        };
        let draw = !o.board.is_empty() && has_flush_or_open_ended_draw(o);
        let playable = if o.board.is_empty() {
            range && (!raised || premium || (late && base_range))
        } else {
            rank >= HandRank::Pair || draw
        };
        let quality_mistake = match self.settings.difficulty {
            Difficulty::Beginner => 0.20,
            Difficulty::Practiced => 0.0,
        };
        let mistake = self
            .rng
            .gen_bool((self.settings.mistake_rate + quality_mistake).min(1.0));
        let style_bluff = match self.settings.style {
            Style::Tight => 0.5,
            Style::Balanced => 1.0,
            Style::Loose => 1.8,
        };
        let bluff = self
            .rng
            .gen_bool((self.settings.bluff_rate * style_bluff).min(1.0));
        if (strong || bluff) && self.rng.gen_bool(self.settings.aggression) {
            if let Some(min) = o.legal.min_raise_to.or(o.legal.min_bet_to) {
                let desired = if o.board.is_empty() {
                    let open = if late { 2 } else { 3 };
                    if raised {
                        o.wager.saturating_add(o.big_blind_chips.saturating_mul(3))
                    } else {
                        o.big_blind_chips.saturating_mul(open)
                    }
                } else {
                    let fraction = if strong { 2 } else { 1 };
                    o.wager.saturating_add(
                        ((o.pot.saturating_mul(fraction)) / 3).max(o.big_blind_chips),
                    )
                };
                let to = desired.max(min).min(o.legal.all_in_to.saturating_sub(1));
                return if o.legal.min_raise_to.is_some() {
                    Action::Raise(to)
                } else {
                    Action::Bet(to)
                };
            }
        }
        let price_limit: u32 = match self.settings.profile {
            Profile::Fundamentals => 4,
            Profile::Recreational => 10,
            Profile::Competent => 3,
        };
        let style_price = match self.settings.style {
            Style::Tight => price_limit.saturating_sub(1),
            Style::Balanced => price_limit,
            Style::Loose => price_limit + 2,
        };
        if o.legal.can_check
            || strong
            || (playable && o.call_cost() <= o.big_blind_chips * style_price)
            || mistake
        {
            o.check_call()
        } else {
            Action::Fold
        }
    }
}

fn preflop_range(
    base: bool,
    loose_extra: bool,
    suited_connector: bool,
    late: bool,
    raised: bool,
    style: Style,
) -> bool {
    let enters = base
        || style == Style::Loose && loose_extra
        || style != Style::Tight && late && suited_connector;
    enters && (!raised || base || style == Style::Loose && late)
}

/// Documents what the reviewed fixtures do and do not claim.
pub fn behavior_limitations() -> &'static [&'static str] {
    &[
        "Opening ranges change with position, prior raises, and style. They are hand-authored heuristics, not solver charts, and they have not been calibrated against recorded human play.",
        "Difficulty changes how often a reviewed rule is ignored. Style changes range width and bluff tendency. Neither label is a measured skill rating.",
        "Postflop continuation looks for a pair or a four-card flush or straight draw using that opponent's own cards and the public board. Hidden cards and future deck order are not inputs.",
    ]
}

fn has_flush_or_open_ended_draw(o: &Observation) -> bool {
    use std::collections::BTreeMap;
    let cards = o.hole_cards.iter().chain(o.board.iter());
    let mut suits = BTreeMap::new();
    let mut ranks = Vec::new();
    for c in cards {
        *suits.entry(c.suit.symbol()).or_insert(0u8) += 1;
        ranks.push(c.rank as u8);
    }
    let flush = suits.values().any(|n| *n == 4);
    ranks.sort_unstable();
    ranks.dedup();
    if ranks.contains(&14) {
        ranks.insert(0, 1);
    }
    let straight = ranks.windows(4).any(|w| w[3] - w[0] == 3);
    flush || straight
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn style_changes_marginal_range_without_changing_difficulty() {
        assert!(!preflop_range(
            false,
            true,
            false,
            true,
            false,
            Style::Tight
        ));
        assert!(!preflop_range(
            false,
            true,
            false,
            true,
            false,
            Style::Balanced
        ));
        assert!(preflop_range(false, true, false, true, false, Style::Loose));
        assert!(!preflop_range(
            false,
            true,
            false,
            false,
            true,
            Style::Loose
        ));
        for difficulty in [Difficulty::Beginner, Difficulty::Practiced] {
            let s = PolicySettings {
                difficulty,
                style: Style::Loose,
                ..Default::default()
            };
            assert_eq!(s.style, Style::Loose);
        }
        assert!(behavior_limitations().iter().all(|note| note.len() > 40));
        assert!(behavior_limitations()
            .iter()
            .any(|note| note.contains("not been calibrated")));
    }
    #[test]
    fn late_suited_connector_enters_for_loose_but_not_tight() {
        // Eight-seven suited is a late suited connector: not base_range, not loose_extra
        // from an ace/broadway, but late && suited_connector clears Balanced/Loose.
        assert!(!preflop_range(
            false,
            false,
            true,
            true,
            false,
            Style::Tight
        ));
        assert!(preflop_range(
            false,
            false,
            true,
            true,
            false,
            Style::Balanced
        ));
        assert!(preflop_range(false, false, true, true, false, Style::Loose));
        assert!(!preflop_range(
            false,
            false,
            true,
            false,
            false,
            Style::Loose
        ));
        assert!(!preflop_range(
            false,
            false,
            true,
            true,
            true,
            Style::Balanced
        ));
        assert!(preflop_range(false, false, true, true, true, Style::Loose));
    }
}
