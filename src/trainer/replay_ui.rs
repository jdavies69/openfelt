//! In-app completed-hand browser. Replay decisions remain pre-decision snapshots.
use super::{
    drills::{self, Answer, DrillTopic, Question},
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
    practice: Option<ReplayPractice>,
}

struct ReplayPractice {
    topic: DrillTopic,
    questions: Vec<Question>,
    question: usize,
    choice: usize,
    answered: Option<Answer>,
    results: Vec<bool>,
    complete: bool,
}

impl ReplayUi {
    pub fn open(store: &Store) -> Result<Self, String> {
        let archive = Archive::load(&store.root)?;
        let hand = archive.hands.len().saturating_sub(1);
        Ok(Self {
            archive,
            hand,
            decision: 0,
            detail: false,
            outcome: false,
            practice: None,
        })
    }

    /// Reloads amended background feedback while keeping the reader on the
    /// same hand and decision whenever that snapshot still exists.
    pub fn refresh(&mut self, store: &Store) -> Result<(), String> {
        let selected_hand = self
            .archive
            .hands
            .get(self.hand)
            .map(|hand| hand.hand_id.clone());
        let archive = Archive::load(&store.root)?;
        self.hand = selected_hand
            .as_deref()
            .and_then(|id| archive.hands.iter().position(|hand| hand.hand_id == id))
            .unwrap_or_else(|| archive.hands.len().saturating_sub(1));
        self.decision = self.decision.min(
            archive
                .hands
                .get(self.hand)
                .map(|hand| hand.decisions.len().saturating_sub(1))
                .unwrap_or(0),
        );
        self.archive = archive;
        Ok(())
    }

    /// Returns true when the browser should close.
    pub fn key(&mut self, key: KeyCode, store: &Store) -> Result<bool, String> {
        if matches!(key, KeyCode::Char('q' | 'Q')) {
            return Ok(true);
        }
        if self.archive.hands.is_empty() {
            return Ok(matches!(key, KeyCode::Esc | KeyCode::Char('r' | 'R')));
        }
        if self.practice.is_some() {
            return self.practice_key(key, store);
        }
        if !self.detail {
            match key {
                KeyCode::Esc | KeyCode::Char('r' | 'R') => return Ok(true),
                KeyCode::Up => self.hand = self.hand.saturating_sub(1),
                KeyCode::Down => self.hand = (self.hand + 1).min(self.archive.hands.len() - 1),
                KeyCode::Enter => {
                    self.detail = true;
                    self.decision = first_questionable(&self.archive.hands[self.hand]);
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
            KeyCode::Char('p' | 'P') if !self.outcome && decisions > 0 => {
                let item = &self.archive.hands[self.hand].decisions[self.decision];
                let topic = practice_topic(item);
                let seed = item.decision.observation.hand_id
                    ^ item.decision.observation.revision.rotate_left(17);
                self.practice = Some(ReplayPractice {
                    topic,
                    questions: drills::selected(topic, seed, 3),
                    question: 0,
                    choice: 0,
                    answered: None,
                    results: Vec::new(),
                    complete: false,
                });
            }
            _ => {}
        }
        Ok(false)
    }

    fn practice_key(&mut self, key: KeyCode, store: &Store) -> Result<bool, String> {
        let Some(practice) = &mut self.practice else {
            return Ok(false);
        };
        if matches!(key, KeyCode::Esc | KeyCode::Char('p' | 'P')) {
            self.practice = None;
            return Ok(false);
        }
        if practice.complete {
            if key == KeyCode::Enter {
                self.practice = None;
            }
            return Ok(false);
        }
        let question = &practice.questions[practice.question];
        match key {
            KeyCode::Up if practice.answered.is_none() => {
                practice.choice = practice.choice.saturating_sub(1)
            }
            KeyCode::Down if practice.answered.is_none() => {
                practice.choice = (practice.choice + 1).min(question.choices.len() - 1)
            }
            KeyCode::Char(choice @ '1'..='9') if practice.answered.is_none() => {
                let index = (choice as usize) - ('1' as usize);
                if index < question.choices.len() {
                    practice.choice = index;
                    practice.answered = Some(drills::grade(question, index));
                }
            }
            KeyCode::Enter if practice.answered.is_none() => {
                practice.answered = Some(drills::grade(question, practice.choice));
            }
            KeyCode::Enter => {
                let accepted = practice
                    .answered
                    .as_ref()
                    .is_some_and(|answer| answer.accepted);
                practice.results.push(accepted);
                if practice.question + 1 < practice.questions.len() {
                    practice.question += 1;
                    practice.choice = 0;
                    practice.answered = None;
                } else {
                    let mut progress = store.progress()?;
                    drills::record(&mut progress, practice.topic, &practice.results);
                    store.save("progress.json", &progress)?;
                    practice.complete = true;
                }
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
        if let Some(practice) = &self.practice {
            draw_practice(frame, inner, practice);
            return;
        }
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
        let assessment = match item.feedback.assessment.as_str() {
            "reconsider" => "QUESTIONABLE DECISION",
            "reasonable" => "REASONABLE DECISION",
            _ => "UNCERTAIN DECISION",
        };
        let alternative = item
            .feedback
            .alternative_action
            .as_deref()
            .unwrap_or("No specific alternative recorded");
        let text = format!(
            "Decision {} of {} {}\n←/→ step · B bookmark · P similar practice · O outcome · Esc hand list\n\n{assessment}\nACCEPTED ACTION: {}\nALTERNATIVE TO COMPARE: {}\nConcept: {}\n\nStreet: {:?}\nYour cards: {}\nBoard then: {}\nPot: {} · wager: {} · call: {}\nStacks: {}\nPublic actions: {}\nLegal options: {}\n\nWHY: {}\n\nOnly information saved before this choice appears here.",
            self.decision + 1, hand.decisions.len(),
            if self.archive.bookmarks.contains(&bookmark) { "★ BOOKMARKED" } else { "" },
            item.decision.accepted_action.description(), alternative, item.feedback.concept,
            o.phase, cards(&o.hole_cards), cards(&o.board), o.pot, o.wager, o.call_cost(),
            o.seats.iter().map(|s|format!("S{}:{}",s.seat.as_u8(),s.stack)).collect::<Vec<_>>().join("  "),
            o.history.iter().map(|(s,a)|format!("S{} {}",s.as_u8(),a.description())).collect::<Vec<_>>().join(" · "),
            serde_json::to_string(&o.legal).unwrap_or_else(|_| "unavailable".into()),
            item.feedback.explanation,
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

fn first_questionable(hand: &super::replay::CompletedHand) -> usize {
    hand.decisions
        .iter()
        .position(|item| item.feedback.assessment == "reconsider")
        .unwrap_or(0)
}

fn practice_topic(item: &super::replay::ReplayDecision) -> DrillTopic {
    drills::topic_for_concept(&item.feedback.concept).unwrap_or_else(|| {
        if item.decision.observation.phase == crate::game::multiway::MultiwayPhase::Preflop {
            DrillTopic::StartingHands
        } else if item.decision.observation.call_cost() > 0 {
            DrillTopic::CallingPrices
        } else {
            DrillTopic::ValueBetting
        }
    })
}

fn draw_practice(
    frame: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    practice: &ReplayPractice,
) {
    let text = if practice.complete {
        format!(
            "Set complete: {}/{} accepted.\n\nThese are reviewed {} questions related to the saved concept. They do not recreate the hand or claim a solver action.\n\nEnter, P, or Esc returns to the same replay decision.",
            practice.results.iter().filter(|accepted| **accepted).count(),
            practice.results.len(),
            practice.topic.title(),
        )
    } else {
        let question = &practice.questions[practice.question];
        let mut lines = vec![
            format!(
                "Related {} practice · question {} of {}",
                practice.topic.title(),
                practice.question + 1,
                practice.questions.len()
            ),
            "Reviewed categorical heuristic; this is not a replayed solver result.".into(),
            String::new(),
            question.prompt.into(),
        ];
        for (index, choice) in question.choices.iter().enumerate() {
            lines.push(format!(
                "{} {}. {}",
                if practice.choice == index { "›" } else { " " },
                index + 1,
                choice
            ));
        }
        if let Some(answer) = &practice.answered {
            lines.push(String::new());
            lines.push(format!(
                "{} · {}",
                if answer.accepted {
                    "ACCEPTED"
                } else {
                    "REVIEW"
                },
                answer.explanation
            ));
            lines.push("Enter continues · P or Esc returns to replay".into());
        } else {
            lines.push(String::new());
            lines.push("↑/↓ select · 1–9 or Enter answer · P or Esc returns to replay".into());
        }
        lines.join("\n")
    };
    frame.render_widget(
        Paragraph::new(text).wrap(Wrap { trim: false }).block(
            Block::default()
                .borders(Borders::ALL)
                .title(" SIMILAR SITUATIONS · OFFLINE PRACTICE "),
        ),
        area,
    );
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
        let mut hand = completed_hand();
        for item in &mut hand.decisions {
            item.feedback.assessment = "reasonable".into();
        }
        let questionable = hand.decisions.len().saturating_sub(1);
        hand.decisions[questionable].feedback.assessment = "reconsider".into();
        hand.decisions[questionable].feedback.alternative_action = Some("check instead".into());
        hand.decisions[questionable].feedback.concept = "Postflop: betting purpose".into();
        store.append("completed-hands.jsonl", &hand).unwrap();
        let mut ui = ReplayUi::open(&store).unwrap();
        assert!(rendered(&ui).contains("COMPLETED HANDS"));
        assert!(!ui.key(KeyCode::Enter, &store).unwrap());
        let first = rendered(&ui);
        assert!(first.contains("Only information saved before this choice appears here"));
        assert!(first.contains("QUESTIONABLE DECISION"));
        assert!(first.contains("ACCEPTED ACTION:"));
        assert!(first.contains("ALTERNATIVE TO COMPARE:"));
        assert!(first.contains("check instead"));
        assert_eq!(ui.decision, questionable);
        ui.key(KeyCode::Char('p'), &store).unwrap();
        assert!(rendered(&ui).contains("SIMILAR SITUATIONS"));
        for _ in 0..3 {
            let accepted = ui.practice.as_ref().unwrap().questions
                [ui.practice.as_ref().unwrap().question]
                .accepted[0];
            let key = char::from_digit((accepted + 1) as u32, 10).unwrap();
            ui.key(KeyCode::Char(key), &store).unwrap();
            ui.key(KeyCode::Enter, &store).unwrap();
        }
        assert!(rendered(&ui).contains("Set complete: 3/3 accepted"));
        assert_eq!(store.progress().unwrap().drills["valuebetting"].attempts, 3);
        ui.key(KeyCode::Esc, &store).unwrap();
        assert_eq!(ui.decision, questionable);
        assert!(rendered(&ui).contains("QUESTIONABLE DECISION"));
        ui.key(KeyCode::Right, &store).unwrap();
        ui.key(KeyCode::Char('b'), &store).unwrap();
        assert!(rendered(&ui).contains("BOOKMARKED"));
        assert_eq!(ReplayUi::open(&store).unwrap().archive.bookmarks.len(), 1);
        let selected = ui.decision;
        let mut amended = ui.archive.hands[ui.hand].clone();
        amended.decisions[selected].feedback.explanation = "Late background review".into();
        store.append("completed-hands.jsonl", &amended).unwrap();
        ui.refresh(&store).unwrap();
        assert_eq!(ui.decision, selected);
        assert!(rendered(&ui).contains("Late background review"));
        assert!(rendered(&ui).contains("BOOKMARKED"));
        ui.key(KeyCode::Char('o'), &store).unwrap();
        assert!(rendered(&ui).contains("separated from every coaching input"));
        ui.key(KeyCode::Esc, &store).unwrap();
        assert!(rendered(&ui).contains("COMPLETED HANDS"));
        assert!(ui.key(KeyCode::Char('q'), &store).unwrap());
        fs::remove_dir_all(root).unwrap();
    }
}
