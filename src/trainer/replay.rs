//! Durable, private completed-hand replay records.
use super::facts::{Decision, Feedback};
use crate::protocol::TableProjection;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, fs, path::Path};

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
