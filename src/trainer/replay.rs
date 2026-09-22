//! Durable, private completed-hand replay records.
use super::facts::{Decision, Feedback};
use crate::protocol::TableProjection;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};

pub const REPLAY_VERSION: u16 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplayDecision {
    /// This is the immutable pre-decision player projection. Outcome cards live only in `outcome`.
    pub decision: Decision,
    pub feedback: Feedback,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompletedHand {
    pub version: u16,
    pub session_id: String,
    pub hand_id: String,
    pub sequence: u64,
    pub decisions: Vec<ReplayDecision>,
    /// Post-hand display data, intentionally separated from every coaching input.
    pub outcome: TableProjection,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bookmark {
    pub hand_id: String,
    pub decision: usize,
}

#[derive(Debug, Default)]
pub struct Archive {
    pub hands: Vec<CompletedHand>,
    pub bookmarks: BTreeSet<Bookmark>,
    pub skipped_records: usize,
}

impl Archive {
    pub fn load(root: &Path) -> Result<Self, String> {
        let mut archive = Self::default();
        let path = root.join("completed-hands.jsonl");
        match fs::read_to_string(path) {
            Ok(text) => {
                for line in text.lines().filter(|line| !line.trim().is_empty()) {
                    match serde_json::from_str::<CompletedHand>(line) {
                        Ok(hand)
                            if hand.version == REPLAY_VERSION
                                && !hand.session_id.is_empty()
                                && !hand.hand_id.is_empty()
                                && hand
                                    .decisions
                                    .iter()
                                    .all(|d| d.decision.observation.hand_id == hand.sequence) =>
                        {
                            archive.hands.push(hand)
                        }
                        _ => archive.skipped_records += 1,
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("Cannot read completed-hand history".into()),
        }
        let bookmark_path = root.join("bookmarks.json");
        match fs::read(&bookmark_path) {
            Ok(bytes) => match serde_json::from_slice::<BTreeSet<Bookmark>>(&bytes) {
                Ok(bookmarks) => archive.bookmarks = bookmarks,
                Err(_) => archive.skipped_records += 1,
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("Cannot read bookmarks".into()),
        }
        Ok(archive)
    }

    /// Counts bookmarked decisions by their saved concept. Unresolved bookmarks are skipped.
    pub fn bookmarked_concepts(&self) -> BTreeMap<String, u64> {
        let mut counts = BTreeMap::new();
        for bookmark in &self.bookmarks {
            if let Ok(decision) = self.resolve(&bookmark.hand_id, bookmark.decision) {
                *counts.entry(decision.feedback.concept.clone()).or_default() += 1;
            }
        }
        counts
    }

    pub fn resolve(&self, hand_id: &str, decision: usize) -> Result<&ReplayDecision, String> {
        self.hands
            .iter()
            .find(|h| h.hand_id == hand_id)
            .ok_or_else(|| "Hand not found".to_string())?
            .decisions
            .get(decision)
            .ok_or_else(|| "Decision not found".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        game::{
            actions::Action,
            multiway::{BlindValues, MultiwayHand, MultiwayPhase},
            seat::{SeatId, TableSize},
        },
        protocol::{project_hand, HandId, ProjectionAudience},
        trainer::{
            facts::{local_feedback, Decision, Facts, Observation},
            hero, Session,
        },
    };
    use std::fs;

    fn temp_root(label: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "openfelt-replay-{label}-{}-{:016x}",
            std::process::id(),
            rand::random::<u64>()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn play_seeded(seed: u64) -> CompletedHand {
        let mut session = Session::new_seeded_for_evaluation(Default::default(), seed).unwrap();
        for _ in 0..500 {
            if session.finished() {
                break;
            }
            if session.view().to_act == Some(hero()) {
                let action = session.observation(hero()).unwrap().check_call();
                session.submit(action).unwrap();
                session.continue_hand();
            } else {
                session.step_bot().unwrap();
            }
        }
        assert!(session.finished());
        session.replay_ready.remove(0)
    }

    #[test]
    fn multi_street_hand_reloads_with_predecision_only_and_matching_engine_actions() {
        let hand = (0..64)
            .map(play_seeded)
            .find(|hand| {
                hand.decisions.len() >= 2
                    && hand
                        .decisions
                        .iter()
                        .any(|d| d.decision.observation.phase == MultiwayPhase::Preflop)
                    && hand.decisions.iter().any(|d| {
                        matches!(
                            d.decision.observation.phase,
                            MultiwayPhase::Flop | MultiwayPhase::Turn | MultiwayPhase::River
                        )
                    })
            })
            .expect("seeded sessions should produce a multi-street hero hand");
        assert!(hand.outcome.board.len() >= 3);
        for (index, recorded) in hand.decisions.iter().enumerate() {
            assert_eq!(
                recorded.decision.observation.board.len(),
                match recorded.decision.observation.phase {
                    MultiwayPhase::Preflop => 0,
                    MultiwayPhase::Flop => 3,
                    MultiwayPhase::Turn => 4,
                    MultiwayPhase::River => 5,
                    _ => recorded.decision.observation.board.len(),
                }
            );
            if !matches!(
                recorded.decision.observation.phase,
                MultiwayPhase::River | MultiwayPhase::Showdown | MultiwayPhase::HandComplete
            ) {
                assert!(
                    recorded.decision.observation.board.len() < hand.outcome.board.len(),
                    "earlier-street decision {index} must not include later board cards"
                );
            }
            assert_eq!(
                local_feedback(&recorded.decision).assessment,
                recorded.feedback.assessment
            );
            assert_eq!(
                Facts::calculate(&recorded.decision.observation),
                recorded.decision.facts
            );
        }
        let root = temp_root("multi-street");
        let mut lines = String::new();
        lines.push_str(&serde_json::to_string(&hand).unwrap());
        lines.push('\n');
        lines.push_str("not-json\n");
        fs::write(root.join("completed-hands.jsonl"), lines).unwrap();
        let archive = Archive::load(&root).unwrap();
        assert_eq!(archive.skipped_records, 1);
        let reloaded = archive.resolve(&hand.hand_id, 0).unwrap();
        assert_eq!(
            reloaded.decision.accepted_action,
            hand.decisions[0].decision.accepted_action
        );
        assert_eq!(
            reloaded.decision.observation.phase,
            hand.decisions[0].decision.observation.phase
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn all_in_side_pot_hand_preserves_engine_actions_and_eligible_pots() {
        let table = TableSize::new(3).unwrap();
        let stacks = [
            (SeatId::new(0).unwrap(), 40),
            (SeatId::new(1).unwrap(), 100),
            (SeatId::new(2).unwrap(), 200),
        ];
        let mut hand = MultiwayHand::new_seeded_with_blinds(
            table,
            SeatId::new(0).unwrap(),
            &stacks,
            &[],
            BlindValues::new(1, 2, 0).unwrap(),
            17,
        )
        .unwrap();
        let mut decisions = Vec::new();
        for sequence in 0..200u64 {
            if matches!(
                hand.phase,
                MultiwayPhase::Showdown | MultiwayPhase::HandComplete
            ) {
                break;
            }
            let Some(actor) = hand.to_act else {
                break;
            };
            let projection =
                project_hand(&hand, HandId(1), ProjectionAudience::Player(actor)).unwrap();
            let observation = Observation::from_projection(
                &projection,
                sequence,
                hand.action_history
                    .iter()
                    .map(|a| (a.seat, a.action))
                    .collect(),
            )
            .unwrap();
            let action = if observation.legal.all_in_to > 0
                && observation.own().stack <= observation.big_blind_chips.saturating_mul(20)
                && !observation.legal.can_check
            {
                Action::AllIn(observation.legal.all_in_to)
            } else {
                observation.check_call()
            };
            if actor == hero() {
                let decision = Decision {
                    version: 1,
                    observation: observation.clone(),
                    accepted_action: action,
                    facts: Facts::calculate(&observation),
                };
                decisions.push(ReplayDecision {
                    feedback: local_feedback(&decision),
                    decision,
                });
            }
            hand.apply_command(crate::game::command::SeatCommand::new(actor, action))
                .unwrap();
        }
        assert!(
            hand.pots.len() >= 2 || hand.settled_contributions.iter().any(|c| c.amount > 0),
            "unequal stacks should create side-pot structure or settled layers"
        );
        assert!(!decisions.is_empty());
        for recorded in &decisions {
            assert_eq!(
                Facts::calculate(&recorded.decision.observation),
                recorded.decision.facts
            );
            assert_eq!(
                local_feedback(&recorded.decision).concept,
                recorded.feedback.concept
            );
            assert_eq!(recorded.decision.observation.actor, hero());
        }
        let completed = CompletedHand {
            version: REPLAY_VERSION,
            session_id: "side-pot-review".into(),
            hand_id: "side-pot-1".into(),
            sequence: 1,
            decisions: decisions
                .into_iter()
                .map(|mut d| {
                    d.decision.observation.hand_id = 1;
                    d.feedback.hand_id = 1;
                    d
                })
                .collect(),
            outcome: project_hand(&hand, HandId(1), ProjectionAudience::Player(hero())).unwrap(),
        };
        assert!(!completed.outcome.pots.is_empty());
        let root = temp_root("side-pot");
        fs::write(
            root.join("completed-hands.jsonl"),
            serde_json::to_string(&completed).unwrap(),
        )
        .unwrap();
        let archive = Archive::load(&root).unwrap();
        let first = archive.resolve("side-pot-1", 0).unwrap();
        assert_eq!(
            first.decision.accepted_action,
            completed.decisions[0].decision.accepted_action
        );
        assert_eq!(first.decision.facts, completed.decisions[0].decision.facts);
        fs::remove_dir_all(root).unwrap();
    }
}
