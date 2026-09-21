//! OpenFelt's local controller. Engine, policy and coaching have separate contracts.
pub mod facts;
pub mod policy;
pub mod provider;
pub mod storage;
pub mod tui;

use crate::game::{
    actions::Action,
    command::SeatCommand,
    multiway::{BlindValues, MultiwayHand, MultiwayPhase},
    seat::{SeatId, TableSize},
};
use crate::protocol::{project_hand, HandId, ProjectionAudience, TableProjection};
use facts::{Decision, Facts, Observation};
use policy::Opponent;
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
}
impl Session {
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
        })
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
    pub fn observation(&self, actor: SeatId) -> Result<Observation, String> {
        let projection = project_hand(
            &self.hand,
            HandId(self.hand_id),
            ProjectionAudience::Player(actor),
        )
        .map_err(|_| "Invalid player")?;
        Observation::from_projection(
            &projection,
            self.hand.action_history.len() as u64,
            self.hand
                .action_history
                .iter()
                .map(|a| (a.seat, a.action))
                .collect(),
        )
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
        self.frozen_view = Some(old_view);
        Ok(self.coaching.as_ref().expect("accepted decision"))
    }
    pub fn continue_hand(&mut self) {
        self.coaching = None;
        self.frozen_view = None;
        self.settle();
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
        self.settled = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
