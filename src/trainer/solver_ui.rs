//! Local river decision practice. The worker owns only public scenario inputs.
use super::solver::{self, RiverScenario, SolvedDrill};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Terminal,
};
use std::{
    io,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
        Arc, Mutex,
    },
    thread,
    time::Duration,
};

struct Worker {
    cancel: Arc<AtomicBool>,
    result: Receiver<Result<SolvedDrill, String>>,
}

// A user can leave and reopen practice while cancellation is winding down.
// This keeps solver memory bounded to one active calculation per process.
static SOLVER_GATE: Mutex<()> = Mutex::new(());

fn card_span(code: &str) -> Span<'static> {
    let suit = code.chars().last().unwrap_or('?').to_ascii_lowercase();
    let mark = match suit {
        's' => '♠',
        'h' => '♥',
        'd' => '♦',
        'c' => '♣',
        _ => '?',
    };
    let rank = match code.chars().next().unwrap_or('?') {
        'T' => "10".to_string(),
        rank => rank.to_string(),
    };
    let ink = if matches!(suit, 'h' | 'd') {
        Color::Rgb(174, 42, 53)
    } else {
        Color::Rgb(25, 35, 43)
    };
    Span::styled(
        format!(" {rank}{mark} "),
        Style::default().fg(ink).bg(Color::Rgb(238, 236, 226)),
    )
}

fn render_cards(frame: &mut ratatui::Frame, area: Rect, title: &str, cards: &[String]) {
    let mut spans = Vec::new();
    for card in cards {
        spans.push(card_span(card));
        spans.push(Span::raw(" "));
    }
    frame.render_widget(
        Paragraph::new(Line::from(spans))
            .block(Block::default().title(title).borders(Borders::ALL)),
        area,
    );
}

fn preview(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let mut result = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        result.push('…');
    }
    result
}

fn bet_sizes(scenario: &RiverScenario) -> String {
    let mut sizes = scenario
        .bet_sizes
        .iter()
        .map(|size| format!("{:.0}% pot", size * 100.0))
        .collect::<Vec<_>>();
    sizes.push("all-in".into());
    sizes.join(", ")
}

fn raise_sizes(scenario: &RiverScenario) -> String {
    if scenario.raise_sizes.is_empty() {
        "none".into()
    } else {
        scenario
            .raise_sizes
            .iter()
            .map(|size| format!("{size:.1}x previous bet"))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn assumptions_text(scenario: &RiverScenario) -> String {
    format!(
        "{}\n\nHeads-up river; hero acts first (out of position).\nBoard: {}\nPot: {} chips\nEffective stack: {} chips\n\nHero possible hands / range:\n{}\n\nVillain possible hands / range:\n{}\n\nHero opening bet options:\n{}\n\nRaise options after a bet:\n{}\n\nAll-in is included automatically. Sizes and ranges are fixed assumptions for this exercise; solver frequencies and pot-share EV apply only to them.",
        scenario.description, scenario.board.join(" "), scenario.pot, scenario.effective_stack,
        scenario.hero_range, scenario.villain_range, bet_sizes(scenario), raise_sizes(scenario),
    )
}

/// A cancellable, self-contained practice screen. `key` returns true to leave it.
pub struct SolverUi {
    scenarios: Vec<RiverScenario>,
    scenario_index: usize,
    worker: Option<Worker>,
    solution: Option<SolvedDrill>,
    error: Option<String>,
    combo_index: usize,
    action_index: usize,
    answered: Option<usize>,
    queued_restart: bool,
    show_assumptions: bool,
    assumptions_scroll: u16,
}

impl SolverUi {
    pub fn new(custom: Option<RiverScenario>) -> Result<Self, String> {
        let scenarios = custom.map(|s| vec![s]).unwrap_or_else(solver::presets);
        if scenarios.is_empty() {
            return Err("No river practice scenarios are available".into());
        }
        for scenario in &scenarios {
            scenario.validate()?;
        }
        let mut ui = Self {
            scenarios,
            scenario_index: 0,
            worker: None,
            solution: None,
            error: None,
            combo_index: 0,
            action_index: 0,
            answered: None,
            queued_restart: false,
            show_assumptions: false,
            assumptions_scroll: 0,
        };
        ui.start();
        Ok(ui)
    }

    fn start(&mut self) {
        self.cancel();
        self.queued_restart = false;
        self.solution = None;
        self.error = None;
        self.combo_index = 0;
        self.action_index = 0;
        self.answered = None;
        let scenario = self.scenarios[self.scenario_index].clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = Arc::clone(&cancel);
        let (sender, result) = mpsc::channel();
        thread::spawn(move || {
            let _gate = SOLVER_GATE
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let solved = if worker_cancel.load(Ordering::Relaxed) {
                Err("Solver cancelled".into())
            } else {
                solver::solve(&scenario, &worker_cancel)
            };
            let _ = sender.send(solved);
        });
        self.worker = Some(Worker { cancel, result });
    }

    fn restart_when_ready(&mut self) {
        self.solution = None;
        self.error = None;
        self.answered = None;
        if let Some(worker) = &self.worker {
            worker.cancel.store(true, Ordering::Relaxed);
            self.queued_restart = true;
        } else {
            self.start();
        }
    }

    fn cancel(&mut self) {
        if let Some(worker) = self.worker.take() {
            worker.cancel.store(true, Ordering::Relaxed);
        }
    }

    pub fn tick(&mut self) -> bool {
        let Some(worker) = &self.worker else {
            return false;
        };
        match worker.result.try_recv() {
            Ok(result) => {
                self.worker = None;
                if self.queued_restart {
                    self.start();
                    return true;
                }
                match result {
                    Ok(solution) if !solution.hero_combos.is_empty() => {
                        self.solution = Some(solution)
                    }
                    Ok(_) => self.error = Some("The selected ranges produced no hero hands".into()),
                    Err(error) => self.error = Some(error),
                }
                true
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.worker = None;
                if self.queued_restart {
                    self.start();
                    return true;
                }
                self.error = Some("The solver worker stopped unexpectedly".into());
                true
            }
            Err(mpsc::TryRecvError::Empty) => false,
        }
    }

    pub fn key(&mut self, code: KeyCode) -> bool {
        if self.show_assumptions {
            match code {
                KeyCode::Esc | KeyCode::Char('?') => self.show_assumptions = false,
                KeyCode::Char('q' | 'Q') => return true,
                KeyCode::Up => self.assumptions_scroll = self.assumptions_scroll.saturating_sub(1),
                KeyCode::Down => {
                    self.assumptions_scroll = self.assumptions_scroll.saturating_add(1)
                }
                KeyCode::PageUp => {
                    self.assumptions_scroll = self.assumptions_scroll.saturating_sub(8)
                }
                KeyCode::PageDown => {
                    self.assumptions_scroll = self.assumptions_scroll.saturating_add(8)
                }
                _ => {}
            }
            return false;
        }
        match code {
            KeyCode::Esc | KeyCode::Char('q' | 'Q') => return true,
            KeyCode::Char('?') => {
                self.show_assumptions = true;
                self.assumptions_scroll = 0;
                return false;
            }
            KeyCode::Char('n' | 'N') => {
                self.scenario_index = (self.scenario_index + 1) % self.scenarios.len();
                self.restart_when_ready();
                return false;
            }
            KeyCode::Char('r' | 'R') if self.error.is_some() => {
                self.restart_when_ready();
                return false;
            }
            _ => {}
        }
        let Some(solution) = &self.solution else {
            return false;
        };
        match code {
            KeyCode::Left | KeyCode::Char('h' | 'H') => {
                self.combo_index = self.combo_index.saturating_sub(1);
                self.action_index = 0;
                self.answered = None;
            }
            KeyCode::Right | KeyCode::Char('l' | 'L') => {
                self.combo_index = (self.combo_index + 1).min(solution.hero_combos.len() - 1);
                self.action_index = 0;
                self.answered = None;
            }
            KeyCode::Up => self.action_index = self.action_index.saturating_sub(1),
            KeyCode::Down => {
                let count = solution.hero_combos[self.combo_index].actions.len();
                self.action_index = (self.action_index + 1).min(count.saturating_sub(1));
            }
            KeyCode::Char(c @ '1'..='9') => {
                let index = (c as usize) - ('1' as usize);
                if index < solution.hero_combos[self.combo_index].actions.len() {
                    self.action_index = index;
                    self.answered = Some(index);
                }
            }
            KeyCode::Enter
                if self.action_index < solution.hero_combos[self.combo_index].actions.len() =>
            {
                self.answered = Some(self.action_index);
            }
            _ => {}
        }
        false
    }

    pub fn draw(&self, frame: &mut ratatui::Frame) {
        let area = frame.area();
        frame.render_widget(
            Block::default().style(Style::default().bg(Color::Rgb(12, 25, 25))),
            area,
        );
        let scenario = &self.scenarios[self.scenario_index];
        if area.width < 80 || area.height < 30 {
            frame.render_widget(
                Paragraph::new(
                    "River practice needs 80 × 30 or larger. Resize to continue. Esc returns.",
                )
                .wrap(Wrap { trim: true }),
                area,
            );
            return;
        }
        if self.show_assumptions {
            frame.render_widget(
                Paragraph::new(assumptions_text(scenario))
                    .wrap(Wrap { trim: true })
                    .scroll((self.assumptions_scroll, 0))
                    .block(
                        Block::default()
                            .title("SCENARIO ASSUMPTIONS · ↑/↓ PgUp/PgDn scroll · ?/Esc close")
                            .borders(Borders::ALL),
                    ),
                area,
            );
            return;
        }
        let parts = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Length(8),
                Constraint::Min(6),
                Constraint::Length(3),
            ])
            .split(area);
        frame.render_widget(
            Paragraph::new(format!(
                "LOCAL RIVER PRACTICE  ·  {} ({}/{})",
                scenario.title,
                self.scenario_index + 1,
                self.scenarios.len()
            ))
            .block(Block::default().borders(Borders::ALL)),
            parts[0],
        );
        render_cards(frame, parts[1], "BOARD", &scenario.board);
        let assumptions = format!(
            "Heads-up river · Hero first (OOP) · Pot {} · Stack {}\nHero range: {}\nVillain range: {}\nOpening: {}\nRaises: {} · ? full assumptions",
            scenario.pot, scenario.effective_stack,
            preview(&scenario.hero_range, 55), preview(&scenario.villain_range, 53),
            preview(&bet_sizes(scenario), 66), preview(&raise_sizes(scenario), 49),
        );
        frame.render_widget(
            Paragraph::new(assumptions).wrap(Wrap { trim: true }).block(
                Block::default()
                    .title("RANGES AND BETTING OPTIONS")
                    .borders(Borders::ALL),
            ),
            parts[2],
        );
        let details = if let Some(solution) = &self.solution {
            let combo = &solution.hero_combos[self.combo_index];
            let mut rows = Vec::new();
            if let Some(chosen) = self.answered.and_then(|index| combo.actions.get(index)) {
                let best = combo
                    .actions
                    .iter()
                    .map(|a| a.ev)
                    .fold(f64::NEG_INFINITY, f64::max);
                if solution.converged {
                    let loss = (best - chosen.ev).max(0.0);
                    let grade = if loss <= f64::from(scenario.pot) * 0.005 {
                        "Near best"
                    } else if loss <= f64::from(scenario.pot) * 0.02 {
                        "Small EV loss"
                    } else {
                        "Large EV loss"
                    };
                    rows.push(format!(
                        "{grade} · EV loss {:.3} chips · played {:.1}%",
                        loss,
                        chosen.frequency * 100.0
                    ));
                    rows.push("Grade bands: ≤0.5% pot near best; ≤2% small; >2% large".into());
                } else {
                    rows.push(format!(
                        "Estimated EV gap {:.3} chips · no grade (tolerance not reached)",
                        (best - chosen.ev).max(0.0)
                    ));
                }
                rows.push(format!(
                    "{} iterations · {} · exploitability {:.3}/{:.3} chips",
                    solution.iterations,
                    if solution.converged {
                        "tolerance reached"
                    } else {
                        "approximate"
                    },
                    solution.exploitability,
                    solution.target_exploitability
                ));
                rows.push("Actions · solver frequency · pot-share EV (chips):".into());
            } else {
                rows.push("Choose an action before seeing frequencies and EVs.".into());
            }
            for (index, action) in combo.actions.iter().enumerate() {
                let prefix = if self.action_index == index { ">" } else { " " };
                let detail = if self.answered.is_some() {
                    format!(" · {:.1}% · {:+.3}", action.frequency * 100.0, action.ev)
                } else {
                    String::new()
                };
                rows.push(format!("{prefix} {}. {}{detail}", index + 1, action.label));
            }
            if self.answered.is_some() {
                rows.push("Estimates apply only to the shown ranges and sizes. ? details".into());
            }
            rows.join("\n")
        } else if let Some(error) = &self.error {
            format!("Solver unavailable: {error}\nR retries this scenario.")
        } else {
            "Calculating locally… Esc cancels and returns.\nNo API key or network request is used."
                .into()
        };
        let detail_area = parts[3];
        frame.render_widget(
            Block::default()
                .title("DECISION AND FEEDBACK")
                .borders(Borders::ALL),
            detail_area,
        );
        let inner = Rect {
            x: detail_area.x + 1,
            y: detail_area.y + 1,
            width: detail_area.width.saturating_sub(2),
            height: detail_area.height.saturating_sub(2),
        };
        let text_area = if let Some(solution) = &self.solution {
            let combo = &solution.hero_combos[self.combo_index];
            let mut spans = vec![Span::raw(format!(
                "Your hand ({}/{}): ",
                self.combo_index + 1,
                solution.hero_combos.len()
            ))];
            spans.push(card_span(&combo.cards[0]));
            spans.push(Span::raw(" "));
            spans.push(card_span(&combo.cards[1]));
            spans.push(Span::raw("  [←/→]"));
            frame.render_widget(
                Paragraph::new(Line::from(spans)),
                Rect { height: 1, ..inner },
            );
            Rect {
                y: inner.y + 1,
                height: inner.height.saturating_sub(1),
                ..inner
            }
        } else {
            inner
        };
        let feedback_color = self.solution.as_ref().and_then(|solution| {
            if !solution.converged {
                return None;
            }
            let combo = &solution.hero_combos[self.combo_index];
            let chosen = combo.actions.get(self.answered?)?;
            let best = combo
                .actions
                .iter()
                .map(|a| a.ev)
                .fold(f64::NEG_INFINITY, f64::max);
            let loss = (best - chosen.ev).max(0.0);
            Some(if loss <= f64::from(scenario.pot) * 0.005 {
                Color::Green
            } else if loss <= f64::from(scenario.pot) * 0.02 {
                Color::Yellow
            } else {
                Color::Red
            })
        });
        frame.render_widget(
            Paragraph::new(details)
                .wrap(Wrap { trim: true })
                .style(Style::default().fg(feedback_color.unwrap_or(Color::Rgb(224, 232, 224)))),
            text_area,
        );
        frame.render_widget(
            Paragraph::new(
                "↑/↓ select · 1–9/Enter choose · ←/→ hand · ? assumptions · N next · Esc/Q return",
            )
            .block(Block::default().borders(Borders::ALL)),
            parts[4],
        );
    }
}

impl Drop for SolverUi {
    fn drop(&mut self) {
        self.cancel();
    }
}

struct TerminalGuard;
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, crossterm::cursor::Show);
    }
}

pub fn run(custom: Option<RiverScenario>) -> Result<(), Box<dyn std::error::Error>> {
    let mut ui = SolverUi::new(custom)?;
    enable_raw_mode()?;
    let _guard = TerminalGuard;
    execute!(io::stdout(), EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let mut redraw = true;
    loop {
        redraw |= ui.tick();
        if redraw {
            terminal.draw(|frame| ui.draw(frame))?;
            redraw = false;
        }
        if event::poll(Duration::from_millis(40))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }
                if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    break;
                }
                if ui.key(key.code) {
                    break;
                }
                redraw = true;
            } else {
                redraw = true;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use std::time::Instant;

    fn rendered(ui: &SolverUi, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| ui.draw(frame)).unwrap();
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<Vec<_>>()
            .join("")
    }

    #[test]
    fn local_practice_shows_cards_choices_and_feedback() {
        let mut ui = SolverUi::new(None).unwrap();
        let loading = rendered(&ui, 80, 30);
        assert!(loading.contains("BOARD"));
        assert!(loading.contains("♠"));
        assert!(loading.contains("Hero range"));
        assert!(loading.contains("Opening:"));
        assert!(loading.contains("all-in"));
        assert!(loading.contains("? full assumptions"));
        let deadline = Instant::now() + Duration::from_secs(6);
        while ui.solution.is_none() && ui.error.is_none() && Instant::now() < deadline {
            ui.tick();
            thread::sleep(Duration::from_millis(10));
        }
        assert!(ui.solution.is_some(), "solver error: {:?}", ui.error);
        let question = rendered(&ui, 80, 30);
        assert!(question.contains("Your hand"));
        assert!(question.contains("Choose an action"));
        ui.key(KeyCode::Char('1'));
        let answer = rendered(&ui, 80, 30);
        assert!(answer.contains("EV loss") || answer.contains("Estimated EV gap"));
        assert!(answer.contains("pot-share EV"));
        assert!(answer.contains("exploitability"));
        assert!(answer.contains("Actions"));
        assert!(ui.key(KeyCode::Esc));
    }

    #[test]
    fn assumptions_overlay_scrolls_long_ranges_without_exiting() {
        let mut ui = SolverUi::new(None).unwrap();
        ui.scenarios[0].hero_range = format!("{}TAIL_HERO", "AsAh, ".repeat(300));
        ui.scenarios[0].villain_range = "TAIL_VILLAIN".into();
        ui.key(KeyCode::Char('?'));
        let first = rendered(&ui, 80, 30);
        assert!(first.contains("SCENARIO ASSUMPTIONS"));
        assert!(first.contains("Heads-up river"));
        for _ in 0..4 {
            ui.key(KeyCode::PageDown);
        }
        let later = rendered(&ui, 80, 30);
        assert!(later.contains("TAIL_HERO"));
        assert!(later.contains("TAIL_VILLAIN"));
        assert!(!ui.key(KeyCode::Esc));
        assert!(!ui.show_assumptions);
        assert!(ui.key(KeyCode::Esc));
    }

    #[test]
    fn rapid_next_requests_queue_one_replacement() {
        let mut ui = SolverUi::new(None).unwrap();
        for _ in 0..5 {
            assert!(!ui.key(KeyCode::Char('n')));
        }
        assert!(ui.queued_restart);
        let deadline = Instant::now() + Duration::from_secs(6);
        while ui.solution.is_none() && ui.error.is_none() && Instant::now() < deadline {
            ui.tick();
            thread::sleep(Duration::from_millis(10));
        }
        assert!(ui.solution.is_some(), "solver error: {:?}", ui.error);
        assert!(!ui.queued_restart);
    }
}
