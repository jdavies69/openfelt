//! OpenFelt's local controller. Engine, policy and coaching have separate contracts.
pub mod coaching;
pub mod drills;
pub mod evaluation;
pub mod facts;
pub mod live_solver;
#[cfg(target_os = "macos")]
pub mod macos_credentials;
pub mod policy;
pub mod provider;
pub mod ranges;
pub mod replay;
pub mod replay_ui;
pub mod solver;
pub mod solver_ui;
pub mod storage;
pub mod table_ui;
pub mod tui;
pub mod update;

use crate::game::{
    actions::Action,
    command::SeatCommand,
    multiway::{BlindValues, MultiwayHand, MultiwayPhase},
    seat::{SeatId, TableSize},
};
use crate::protocol::{project_hand, HandId, ProjectionAudience, TableProjection};
use facts::{Decision, Facts, Observation};
use policy::Opponent;
use replay::{CompletedHand, ReplayDecision};
use serde::{Deserialize, Serialize};
use storage::Settings;

pub fn hero() -> SeatId {
    SeatId::new(0).expect("valid hero seat")
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CashEvent {
    pub hand_id: u64,
    pub seat: SeatId,
    pub added: u32,
    pub withdrawn: u32,
}
pub struct Session {
    hand: MultiwayHand,
    pub hand_id: u64,
    pub settings: Settings,
    bots: Vec<Opponent>,
    pub coaching: Option<Decision>,
    frozen_view: Option<TableProjection>,
    pub cash_events: Vec<CashEvent>,
    pub opening_hero_stack: u32,
    pub session_profit: i64,
    settled: bool,
    pub completed_hands: u64,
    pub session_id: String,
    replay_decisions: Vec<ReplayDecision>,
    pub replay_ready: Vec<CompletedHand>,
}
impl Session {
    pub fn result_summary(&self) -> Option<String> {
        if !self.finished() || self.hand.awards.is_empty() {
            return None;
        }
        let payouts = self
            .hand
            .awards
            .iter()
            .flat_map(|award| award.payouts.iter())
            .map(|payout| {
                let name = if payout.seat == hero() {
                    "You".to_string()
                } else {
                    format!("Bot {}", payout.seat.as_u8())
                };
                format!("{name} won {}", payout.amount)
            })
            .collect::<Vec<_>>();
        (!payouts.is_empty()).then(|| payouts.join(" · "))
    }
    pub fn new(settings: Settings) -> Result<Self, String> {
        settings.validate()?;
        let table = TableSize::new(settings.seats).map_err(|e| e.to_string())?;
        let chips = settings.big_blind * 100;
        let stacks: Vec<_> = table.seats().map(|s| (s, chips)).collect();
        let hand = MultiwayHand::new_with_blinds(
            table,
            hero(),
            &stacks,
            &[],
            BlindValues::new(settings.small_blind, settings.big_blind, 0)
                .ok_or("Invalid blinds")?,
        )
        .map_err(|e| e.to_string())?;
        let bots = table
            .seats()
            .map(|_| Opponent::new(rand::random(), settings.opponents.clone()))
            .collect();
        let session_id = format!("{:032x}", rand::random::<u128>());
        Ok(Self {
            hand,
            hand_id: 1,
            settings,
            bots,
            coaching: None,
            frozen_view: None,
            cash_events: Vec::new(),
            opening_hero_stack: chips,
            session_profit: 0,
            settled: false,
            completed_hands: 0,
            session_id,
            replay_decisions: Vec::new(),
            replay_ready: Vec::new(),
        })
    }
    /// Deterministic construction for evaluation scenarios only.
    pub fn new_seeded_for_evaluation(settings: Settings, seed: u64) -> Result<Self, String> {
        let mut session = Self::new(settings)?;
        let table = session.hand.table_size;
        let chips = session.settings.big_blind * 100;
        let stacks: Vec<_> = table.seats().map(|seat| (seat, chips)).collect();
        session.hand = MultiwayHand::new_seeded_for_review(table, hero(), &stacks, seed)
            .map_err(|e| e.to_string())?;
        Ok(session)
    }
    pub fn view(&self) -> TableProjection {
        self.frozen_view.clone().unwrap_or_else(|| {
            project_hand(
                &self.hand,
                HandId(self.hand_id),
                ProjectionAudience::Player(hero()),
            )
            .expect("hero occupies table")
        })
    }

    /// Public, player-facing action copy for the table rail. The renderer gets
    /// descriptions only; it never receives an authoritative deck or hidden
    /// opponent cards.
    pub fn recent_actions(&self) -> Vec<String> {
        self.hand
            .action_history
            .iter()
            .rev()
            .take(8)
            .rev()
            .map(|record| {
                let actor = if record.seat == hero() {
                    "you".to_string()
                } else {
                    format!("bot {}", record.seat.as_u8())
                };
                format!("{actor} {}", record.action.description())
            })
            .collect()
    }
    pub fn observation(&self, actor: SeatId) -> Result<Observation, String> {
        let projection = project_hand(
            &self.hand,
            HandId(self.hand_id),
            ProjectionAudience::Player(actor),
        )
        .map_err(|_| "Invalid player")?;
        let mut observation = Observation::from_projection(
            &projection,
            self.hand.action_history.len() as u64,
            self.hand
                .action_history
                .iter()
                .map(|a| (a.seat, a.action))
                .collect(),
        )?;
        observation.public_history = self
            .hand
            .action_history
            .iter()
            .map(|record| facts::PublicAction {
                phase: record.phase,
                seat: record.seat,
                action: record.action,
                wager_after: record.wager_after,
            })
            .collect();
        Ok(observation)
    }
    pub fn submit(&mut self, action: Action) -> Result<&Decision, String> {
        if self.coaching.is_some() {
            return Err("Press Enter to continue the hand first".into());
        }
        let observation = self.observation(hero())?;
        let old_view = self.view();
        self.hand
            .apply_command(SeatCommand::new(hero(), action))
            .map_err(|e| e.to_string())?;
        let facts = Facts::calculate(&observation);
        self.coaching = Some(Decision {
            version: 1,
            observation,
            accepted_action: action,
            facts,
        });
        let decision = self.coaching.as_ref().expect("accepted decision").clone();
        self.replay_decisions.push(ReplayDecision {
            decision: decision.clone(),
            feedback: facts::local_feedback(&decision),
        });
        self.frozen_view = Some(old_view);
        Ok(self.coaching.as_ref().expect("accepted decision"))
    }
    pub fn continue_hand(&mut self) {
        self.coaching = None;
        self.frozen_view = None;
        self.settle();
    }
    /// Replace only the still-paused decision's replay feedback, never another hand.
    pub fn record_solver_feedback(&mut self, feedback: &facts::Feedback) -> bool {
        let Some(decision) = &self.coaching else {
            return false;
        };
        if feedback.hand_id != decision.observation.hand_id
            || feedback.revision != decision.observation.revision
        {
            return false;
        }
        let Some(record) = self.replay_decisions.last_mut() else {
            return false;
        };
        if record.decision != *decision {
            return false;
        }
        record.feedback = feedback.clone();
        true
    }
    pub fn step_bot(&mut self) -> Result<bool, String> {
        if self.coaching.is_some() || self.finished() {
            return Ok(false);
        }
        let Some(actor) = self.hand.to_act else {
            return Ok(false);
        };
        if actor == hero() {
            return Ok(false);
        }
        let o = self.observation(actor)?;
        let action = self.bots[actor.index()].choose(&o);
        self.hand
            .apply_command(SeatCommand::new(actor, action))
            .map_err(|e| e.to_string())?;
        self.settle();
        Ok(true)
    }
    pub fn finished(&self) -> bool {
        self.coaching.is_none()
            && matches!(
                self.hand.phase,
                MultiwayPhase::Showdown | MultiwayPhase::HandComplete
            )
    }
    fn settle(&mut self) {
        if self.finished() && !self.settled {
            self.session_profit +=
                i64::from(self.hand.seat(hero()).stack) - i64::from(self.opening_hero_stack);
            self.completed_hands += 1;
            self.replay_ready.push(CompletedHand {
                version: replay::REPLAY_VERSION,
                session_id: self.session_id.clone(),
                hand_id: format!("{}-{}", self.session_id, self.hand_id),
                sequence: self.hand_id,
                decisions: std::mem::take(&mut self.replay_decisions),
                outcome: project_hand(
                    &self.hand,
                    HandId(self.hand_id),
                    ProjectionAudience::Player(hero()),
                )
                .expect("hero occupies completed table"),
            });
            self.settled = true;
        }
    }
    pub fn top_up(&mut self) -> Result<u32, String> {
        if !self.finished() {
            return Err("Top up only between hands".into());
        }
        let target = self.settings.big_blind * 100;
        let current = self.hand.seat(hero()).stack;
        let added = target.saturating_sub(current);
        // No active hand is mutated; the completed stack is the next hand's bankroll.
        self.hand.seats[hero().index()]
            .as_mut()
            .expect("hero")
            .stack += added;
        if added > 0 {
            self.cash_events.push(CashEvent {
                hand_id: self.hand_id,
                seat: hero(),
                added,
                withdrawn: 0,
            });
        }
        Ok(added)
    }
    pub fn withdraw(&mut self, amount: u32) -> Result<(), String> {
        if !self.finished() || amount > self.hand.seat(hero()).stack {
            return Err("Withdraw only available chips between hands".into());
        }
        self.hand.seats[hero().index()]
            .as_mut()
            .expect("hero")
            .stack -= amount;
        self.cash_events.push(CashEvent {
            hand_id: self.hand_id,
            seat: hero(),
            added: 0,
            withdrawn: amount,
        });
        Ok(())
    }
    pub fn next_hand(&mut self) -> Result<(), String> {
        if !self.finished() {
            return Err("Finish this hand first".into());
        }
        if self.hand.seat(hero()).stack == 0 {
            return Err("Press B to rebuy before the next hand".into());
        }
        let stacks: Vec<_> = self
            .hand
            .occupied_seats()
            .map(|seat| {
                let stack = self.hand.seat(seat).stack;
                let added = if seat != hero() && stack == 0 {
                    self.settings.big_blind * 100
                } else {
                    0
                };
                (seat, stack + added, added)
            })
            .collect();
        for &(seat, _, added) in &stacks {
            if added > 0 {
                self.cash_events.push(CashEvent {
                    hand_id: self.hand_id,
                    seat,
                    added,
                    withdrawn: 0,
                });
            }
        }
        let button =
            SeatId::new((self.hand.button.as_u8() + 1) % self.settings.seats).expect("table seat");
        self.opening_hero_stack = self.hand.seat(hero()).stack;
        self.hand = MultiwayHand::new_with_blinds(
            self.hand.table_size,
            button,
            &stacks.iter().map(|&(s, c, _)| (s, c)).collect::<Vec<_>>(),
            &[],
            self.hand.blind_values,
        )
        .map_err(|e| e.to_string())?;
        self.hand_id += 1;
        self.replay_decisions.clear();
        self.settled = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
