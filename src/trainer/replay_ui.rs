//! In-app completed-hand browser. Replay decisions remain pre-decision snapshots.
use super::{
    replay::{Archive, Bookmark},
    storage::Store,
};
use crossterm::event::KeyCode;
use ratatui::{
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Paragraph, Wrap},
};

pub struct ReplayUi {
    pub archive: Archive,
    hand: usize,
    decision: usize,
    detail: bool,
    outcome: bool,
}

impl ReplayUi {
    pub fn open(store: &Store) -> Result<Self, String> {
        Ok(Self {
            archive: Archive::load(&store.root)?,
            hand: 0,
            decision: 0,
            detail: false,
            outcome: false,
        })
    }

    /// Returns true when the browser should close.
    pub fn key(&mut self, key: KeyCode, store: &Store) -> Result<bool, String> {
        if self.archive.hands.is_empty() {
            return Ok(matches!(key, KeyCode::Esc | KeyCode::Char('r' | 'R')));
        }
        if !self.detail {
            match key {
                KeyCode::Esc | KeyCode::Char('r' | 'R') => return Ok(true),
                KeyCode::Up => self.hand = self.hand.saturating_sub(1),
                KeyCode::Down => self.hand = (self.hand + 1).min(self.archive.hands.len() - 1),
                KeyCode::Enter => {
                    self.detail = true;
                    self.decision = 0;
                    self.outcome = false;
                }
                _ => {}
            }
            return Ok(false);
        }
        let decisions = self.archive.hands[self.hand].decisions.len();
        match key {
            KeyCode::Esc => {
                self.detail = false;
                self.outcome = false;
            }
            KeyCode::Left if !self.outcome => self.decision = self.decision.saturating_sub(1),
            KeyCode::Right if !self.outcome && decisions > 0 => {
                self.decision = (self.decision + 1).min(decisions - 1)
            }
            KeyCode::Char('o' | 'O') => self.outcome = !self.outcome,
            KeyCode::Char('b' | 'B') if !self.outcome && decisions > 0 => {
                let bookmark = Bookmark {
                    hand_id: self.archive.hands[self.hand].hand_id.clone(),
                    decision: self.decision,
                };
                if !self.archive.bookmarks.remove(&bookmark) {
                    self.archive.bookmarks.insert(bookmark);
                }
                store.save("bookmarks.json", &self.archive.bookmarks)?;
            }
            _ => {}
        }
        Ok(false)
    }

    pub fn draw(&self, frame: &mut ratatui::Frame) {
        let area = frame.area();
        let base = Style::default()
            .fg(Color::Rgb(224, 232, 224))
            .bg(Color::Rgb(12, 25, 25));
        frame.render_widget(Block::default().style(base), area);
        let inner = area.inner(ratatui::layout::Margin {
            horizontal: 2,
            vertical: 1,
        });
        if self.archive.hands.is_empty() {
            let skipped = if self.archive.skipped_records > 0 {
                format!(
                    "\nSkipped {} invalid or unsupported record(s).",
                    self.archive.skipped_records
                )
            } else {
                String::new()
            };
            frame.render_widget(
                Paragraph::new(format!(
                    "No completed hands saved yet.{skipped}\n\nEsc or R returns to the table."
                ))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(" HAND REPLAY "),
                ),
                inner,
            );
            return;
        }
        if !self.detail {
            let mut lines = vec![Line::from(
                "↑/↓ choose · Enter opens · Esc or R returns · ★ has bookmarks",
            )];
            for (index, hand) in self.archive.hands.iter().enumerate() {
                let marked = self
                    .archive
                    .bookmarks
                    .iter()
                    .any(|bookmark| bookmark.hand_id == hand.hand_id);
                lines.push(Line::styled(
                    format!(
                        "{} {}  Hand {}  · {} decision(s) {}",
                        if index == self.hand { "›" } else { " " },
                        hand.session_id.chars().take(8).collect::<String>(),
                        hand.sequence,
                        hand.decisions.len(),
                        if marked { "★" } else { "" }
                    ),
                    if index == self.hand {
                        Style::default()
                            .fg(Color::Rgb(225, 190, 105))
                            .add_modifier(Modifier::BOLD)
                    } else {
                        base
                    },
                ));
            }
            if self.archive.skipped_records > 0 {
                lines.push(Line::from(format!(
                    "Skipped {} invalid or unsupported record(s); valid history remains available.",
                    self.archive.skipped_records
                )));
            }
            frame.render_widget(
                Paragraph::new(lines).wrap(Wrap { trim: false }).block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(" COMPLETED HANDS "),
                ),
                inner,
            );
            return;
        }
        let hand = &self.archive.hands[self.hand];
        if self.outcome {
            let text = format!(
                "Final outcome is separated from every coaching input.\n\nStreet: {:?}\nBoard: {}\nPot: {}\nAwards: {}\n\nO returns to the saved decision · Esc returns to hand list",
                hand.outcome.phase,
                cards(&hand.outcome.board),
                hand.outcome.pot_total,
                serde_json::to_string(&hand.outcome.awards).unwrap_or_else(|_| "unavailable".into())
            );
            frame.render_widget(
                Paragraph::new(text).wrap(Wrap { trim: false }).block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(" FINAL OUTCOME "),
                ),
                inner,
            );
            return;
        }
        let Some(item) = hand.decisions.get(self.decision) else {
            frame.render_widget(
                Paragraph::new(
                    "This completed hand has no player decisions.\nEsc returns to the list.",
                )
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(" HAND REPLAY "),
                ),
                inner,
            );
            return;
        };
        let o = &item.decision.observation;
        let bookmark = Bookmark {
            hand_id: hand.hand_id.clone(),
            decision: self.decision,
        };
        let text = format!(
            "Decision {} of {} {}\n←/→ step · B bookmark/unbookmark · O final outcome · Esc hand list\n\nStreet: {:?}\nYour cards: {}\nBoard then: {}\nPot: {} · wager: {} · call: {}\nStacks: {}\nPublic actions: {}\nLegal options: {}\n\nChosen: {}\nFeedback: {}\n\nOnly information saved before this choice appears here.",
            self.decision + 1, hand.decisions.len(),
            if self.archive.bookmarks.contains(&bookmark) { "★ BOOKMARKED" } else { "" },
            o.phase, cards(&o.hole_cards), cards(&o.board), o.pot, o.wager, o.call_cost(),
            o.seats.iter().map(|s|format!("S{}:{}",s.seat.as_u8(),s.stack)).collect::<Vec<_>>().join("  "),
            o.history.iter().map(|(s,a)|format!("S{} {}",s.as_u8(),a.description())).collect::<Vec<_>>().join(" · "),
            serde_json::to_string(&o.legal).unwrap_or_else(|_| "unavailable".into()),
            item.decision.accepted_action.description(), item.feedback.explanation,
        );
        frame.render_widget(
            Paragraph::new(text).wrap(Wrap { trim: false }).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" DECISION REPLAY "),
            ),
            inner,
        );
    }
}

fn cards(cards: &[crate::game::deck::Card]) -> String {
    if cards.is_empty() {
        "—".into()
    } else {
        cards
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trainer::{hero, Session};
    use ratatui::{backend::TestBackend, Terminal};
    use std::fs;

    fn root(label: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "openfelt-replay-ui-{label}-{}-{:016x}",
            std::process::id(),
            rand::random::<u64>()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }
    fn completed_hand() -> super::super::replay::CompletedHand {
        let mut session = Session::new_seeded_for_evaluation(Default::default(), 91).unwrap();
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
    fn rendered(ui: &ReplayUi) -> String {
        let mut terminal = Terminal::new(TestBackend::new(100, 36)).unwrap();
        terminal.draw(|frame| ui.draw(frame)).unwrap();
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    #[test]
    fn empty_and_corrupt_archive_is_clear_and_exits() {
        let root = root("empty");
        fs::write(root.join("completed-hands.jsonl"), "bad record\n").unwrap();
        let store = Store { root: root.clone() };
        let mut ui = ReplayUi::open(&store).unwrap();
        let text = rendered(&ui);
        assert!(text.contains("No completed hands saved yet"));
        assert!(text.contains("Skipped 1 invalid"));
        assert!(ui.key(KeyCode::Esc, &store).unwrap());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn list_opens_steps_renders_predecision_and_persists_bookmark() {
        let root = root("browse");
        let store = Store { root: root.clone() };
        let hand = completed_hand();
        store.append("completed-hands.jsonl", &hand).unwrap();
        let mut ui = ReplayUi::open(&store).unwrap();
        assert!(rendered(&ui).contains("COMPLETED HANDS"));
        assert!(!ui.key(KeyCode::Enter, &store).unwrap());
        let first = rendered(&ui);
        assert!(first.contains("Only information saved before this choice appears here"));
        assert!(first.contains("Chosen:"));
        ui.key(KeyCode::Right, &store).unwrap();
        ui.key(KeyCode::Char('b'), &store).unwrap();
        assert!(rendered(&ui).contains("BOOKMARKED"));
        assert_eq!(ReplayUi::open(&store).unwrap().archive.bookmarks.len(), 1);
        ui.key(KeyCode::Char('o'), &store).unwrap();
        assert!(rendered(&ui).contains("separated from every coaching input"));
        ui.key(KeyCode::Esc, &store).unwrap();
        assert!(rendered(&ui).contains("COMPLETED HANDS"));
        fs::remove_dir_all(root).unwrap();
    }
}
