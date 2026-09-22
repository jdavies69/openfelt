use super::{
    facts::{local_feedback, Feedback},
    hero,
    provider::{self, Credential, Pending, Usage},
    storage::{CoachingMode, Progress, Settings, Store},
    Session,
};
use crate::game::actions::Action;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Terminal,
};
use std::{
    io,
    time::{Duration, Instant},
};

enum Input {
    Play,
    Raise(String),
    AllIn,
    Withdraw(String),
}
struct TerminalGuard;
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, crossterm::cursor::Show);
    }
}
struct Ui {
    session: Session,
    input: Input,
    feedback: Option<Feedback>,
    pending: Option<Pending>,
    status: String,
    deep: bool,
    help: bool,
    consent: bool,
    cloud_enabled: bool,
    provider_feedback: bool,
    usage: Usage,
    progress: Progress,
    accounted_hands: u64,
    accounted_profit: i64,
    cash_saved: usize,
    replay_saved: usize,
    replay: Option<super::replay_ui::ReplayUi>,
    saved_progress: Vec<u8>,
}
pub fn run(settings: Settings, store: Store) -> Result<(), Box<dyn std::error::Error>> {
    let progress = store.progress()?;
    let consent = settings.coaching == CoachingMode::Openai;
    let mut ui = Ui {
        session: Session::new(settings)?,
        input: Input::Play,
        feedback: None,
        pending: None,
        status: "F fold · C check/call · R raise · A all-in · ? help".into(),
        deep: false,
        help: false,
        consent,
        cloud_enabled: false,
        provider_feedback: false,
        usage: Usage::default(),
        progress,
        accounted_hands: 0,
        accounted_profit: 0,
        cash_saved: 0,
        replay_saved: 0,
        replay: None,
        saved_progress: Vec::new(),
    };
    enable_raw_mode()?;
    let _guard = TerminalGuard;
    execute!(io::stdout(), EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let mut next_bot = Instant::now();
    let mut redraw = true;
    let mut last_size = None;
    loop {
        if let Some(result) = ui.pending.as_ref().and_then(Pending::poll) {
            redraw = true;
            ui.pending = None;
            match result {
                Ok(result) => {
                    if ui
                        .session
                        .coaching
                        .as_ref()
                        .is_some_and(|d| provider::validate_feedback(&result.feedback, d).is_ok())
                    {
                        ui.usage.input_tokens += result.input_tokens;
                        ui.usage.output_tokens += result.output_tokens;
                        if let Err(e) = store.append("cloud-feedback.jsonl", &result.feedback) {
                            ui.status = e;
                        } else {
                            ui.status =
                                "Provider heuristic received · Enter continues · ? details".into();
                        }
                        ui.feedback = Some(result.feedback);
                        ui.provider_feedback = true;
                    }
                }
                Err(e) => ui.status = format!("{e} · Enter continues · T retries (paid)"),
            }
            if let Err(e) = store.append("usage.jsonl", &ui.usage) {
                ui.status = e;
            }
        }
        let size = terminal.size()?;
        if last_size != Some(size) {
            redraw = true;
            last_size = Some(size);
        }
        if !ui.consent
            && !ui.help
            && size.width >= 80
            && size.height >= 30
            && Instant::now() >= next_bot
        {
            match ui.session.step_bot() {
                Ok(changed) => redraw |= changed,
                Err(e) => {
                    ui.status = e;
                    redraw = true;
                }
            }
            next_bot = Instant::now() + Duration::from_millis(450);
        }
        ui.save_progress(&store);
        if redraw {
            terminal.draw(|frame| draw(frame, &ui))?;
            redraw = false;
        }
        if !event::poll(Duration::from_millis(40))? {
            continue;
        }
        let Event::Key(key) = event::read()? else {
            redraw = true;
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        redraw = true;
        if key.code == KeyCode::Char('q')
            || key.code == KeyCode::Char('Q')
            || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL))
        {
            break;
        }
        if size.width < 80 || size.height < 30 {
            continue;
        }
        let code = match key.code {
            KeyCode::Char(c) => KeyCode::Char(c.to_ascii_lowercase()),
            other => other,
        };
        if let Some(mut replay) = ui.replay.take() {
            match replay.key(key.code, &store) {
                Ok(true) => ui.status = "Returned to table".into(),
                Ok(false) => ui.replay = Some(replay),
                Err(e) => {
                    ui.status = e;
                    ui.replay = Some(replay);
                }
            }
            continue;
        }
        if code == KeyCode::Char('r') && ui.session.coaching.is_none() {
            match super::replay_ui::ReplayUi::open(&store) {
                Ok(replay) => ui.replay = Some(replay),
                Err(e) => ui.status = e,
            }
            continue;
        }
        if ui.consent {
            match code {
                KeyCode::Enter => {
                    ui.consent = false;
                    ui.cloud_enabled = true;
                    ui.status="OpenAI enabled for this session · optional calls may incur provider charges".into();
                }
                KeyCode::Esc | KeyCode::Char('l') => {
                    ui.consent = false;
                    ui.session.settings.coaching = CoachingMode::Local;
                    ui.status = "Local teaching · no network requests".into();
                }
                _ => {}
            }
            continue;
        }
        if code == KeyCode::Char('?') {
            if ui.session.coaching.is_some() {
                ui.deep = !ui.deep;
            } else {
                ui.help = !ui.help;
            }
            continue;
        }
        if ui.help {
            if matches!(code, KeyCode::Esc | KeyCode::Enter) {
                ui.help = false;
            }
            continue;
        }
        if ui.session.coaching.is_some() {
            match code {
                KeyCode::Enter => {
                    ui.pending = None;
                    ui.session.continue_hand();
                    ui.feedback = None;
                    ui.provider_feedback = false;
                    ui.deep = false;
                    ui.status = "Hand resumed".into();
                    next_bot = Instant::now() + Duration::from_millis(450);
                }
                KeyCode::Char('t') if ui.cloud_enabled && ui.pending.is_none() => {
                    ui.request(&store)
                }
                _ => {}
            }
            continue;
        }
        if ui.session.finished() && matches!(ui.input, Input::Play) {
            match code {
                KeyCode::Enter => match ui.session.next_hand() {
                    Ok(()) => ui.status = "Next hand · fixed blinds · no rake".into(),
                    Err(e) => ui.status = e,
                },
                KeyCode::Char('b') => match ui.session.top_up() {
                    Ok(n) => ui.status = format!("Added {n} chips; tracked separately from profit"),
                    Err(e) => ui.status = e,
                },
                KeyCode::Char('w') => ui.input = Input::Withdraw(String::new()),
                _ => {}
            }
            continue;
        }
        let mut action = None;
        match &mut ui.input {
            Input::Withdraw(text) => match code {
                KeyCode::Esc => ui.input = Input::Play,
                KeyCode::Backspace => {
                    text.pop();
                }
                KeyCode::Char(c) if c.is_ascii_digit() && text.len() < 9 => text.push(c),
                KeyCode::Enter => {
                    match text
                        .parse::<u32>()
                        .map_err(|_| "Enter a chip amount".into())
                        .and_then(|n| ui.session.withdraw(n))
                    {
                        Ok(()) => ui.status = "Withdrawal recorded separately from profit".into(),
                        Err(e) => ui.status = e,
                    };
                    ui.input = Input::Play;
                }
                _ => {}
            },
            Input::AllIn => match code {
                KeyCode::Enter => {
                    if let Ok(o) = ui.session.observation(hero()) {
                        action = Some(Action::AllIn(o.legal.all_in_to));
                    }
                    ui.input = Input::Play;
                }
                KeyCode::Esc => ui.input = Input::Play,
                _ => {}
            },
            Input::Raise(text) => match code {
                KeyCode::Esc => ui.input = Input::Play,
                KeyCode::Backspace => {
                    text.pop();
                }
                KeyCode::Char(c) if c.is_ascii_digit() && text.len() < 9 => text.push(c),
                KeyCode::Up => {
                    *text = text
                        .parse::<u32>()
                        .unwrap_or(0)
                        .saturating_add(1)
                        .to_string();
                }
                KeyCode::Down => {
                    *text = text
                        .parse::<u32>()
                        .unwrap_or(0)
                        .saturating_sub(1)
                        .to_string();
                }
                KeyCode::Enter => {
                    if let Ok(o) = ui.session.observation(hero()) {
                        match text.parse::<u32>() {
                            Ok(n) if n == o.legal.all_in_to => ui.input = Input::AllIn,
                            Ok(n) => {
                                action = Some(if o.legal.min_raise_to.is_some() {
                                    Action::Raise(n)
                                } else {
                                    Action::Bet(n)
                                });
                                ui.input = Input::Play;
                            }
                            Err(_) => ui.status = "Enter a whole number of chips".into(),
                        }
                    }
                }
                _ => {}
            },
            Input::Play => {
                if let Ok(o) = ui.session.observation(hero()) {
                    match code {
                        KeyCode::Char('f') => action = Some(Action::Fold),
                        KeyCode::Char('c') => {
                            let a = o.check_call();
                            if matches!(a, Action::AllIn(_)) {
                                ui.input = Input::AllIn;
                            } else {
                                action = Some(a);
                            }
                        }
                        KeyCode::Char('a') => ui.input = Input::AllIn,
                        KeyCode::Char('r') => {
                            if o.legal.min_raise_to.or(o.legal.min_bet_to).is_some() {
                                ui.input = Input::Raise(String::new());
                            } else {
                                ui.status =
                                    "No regular raise available; A reviews an all-in".into();
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        if let Some(action) = action {
            match ui.session.submit(action) {
                Ok(d) => {
                    let f = local_feedback(d);
                    ui.progress.decisions += 1;
                    *ui.progress.concepts.entry(f.concept.clone()).or_default() += 1;
                    if let Err(e) = store.decision(d, &f) {
                        ui.status = e;
                    } else {
                        ui.status =
                            "Decision accepted · hand paused · Enter continues · ? details".into();
                    }
                    ui.feedback = Some(f);
                    if ui.cloud_enabled {
                        ui.request(&store);
                    }
                }
                Err(e) => ui.status = format!("Not accepted: {e}"),
            }
        }
    }
    ui.pending = None;
    ui.save_progress(&store);
    Ok(())
}
impl Ui {
    fn request(&mut self, store: &Store) {
        let Some(d) = self.session.coaching.clone() else {
            return;
        };
        let result = Credential::from_environment().and_then(|key| {
            provider::start(d, self.session.settings.cloud.clone(), key, &mut self.usage)
        });
        match result {
            Ok(p) => {
                self.pending = Some(p);
                self.status = "Requesting coaching… Enter cancels and continues · Q quits".into();
            }
            Err(e) => self.status = format!("Coaching unavailable: {e}"),
        }
        if let Err(e) = store.append("usage.jsonl", &self.usage) {
            self.status = e;
        }
    }
    fn save_progress(&mut self, store: &Store) {
        let hands = self.session.completed_hands;
        self.progress.hands += hands - self.accounted_hands;
        self.accounted_hands = hands;
        self.progress.profit_chips += self.session.session_profit - self.accounted_profit;
        self.accounted_profit = self.session.session_profit;
        // Writing only changed content avoids constant disk activity from the render loop.
        let state = serde_json::to_vec(&self.progress).unwrap_or_default();
        if state != self.saved_progress {
            match store.save("progress.json", &self.progress) {
                Ok(()) => self.saved_progress = state,
                Err(e) => self.status = e,
            }
        }
        while self.cash_saved < self.session.cash_events.len() {
            if let Err(e) = store.append(
                "cash-events.jsonl",
                &self.session.cash_events[self.cash_saved],
            ) {
                self.status = e;
                break;
            }
            self.cash_saved += 1;
        }
        while self.replay_saved < self.session.replay_ready.len() {
            if let Err(e) = store.append(
                "completed-hands.jsonl",
                &self.session.replay_ready[self.replay_saved],
            ) {
                self.status = e;
                break;
            }
            self.replay_saved += 1;
        }
    }
}
fn cards(cards: &[crate::game::deck::Card]) -> String {
    cards
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("  ")
}
fn draw(frame: &mut ratatui::Frame, ui: &Ui) {
    if let Some(replay) = &ui.replay {
        replay.draw(frame);
        return;
    }
    let area = frame.area();
    let base = Style::default()
        .fg(Color::Rgb(224, 232, 224))
        .bg(Color::Rgb(12, 25, 25));
    frame.render_widget(Block::default().style(base), area);
    if area.width < 80 || area.height < 30 {
        frame.render_widget(Paragraph::new(format!("OpenFelt needs 80 × 30 or larger. Current: {} × {}.\nResize to continue. Q quits safely.",area.width,area.height)).style(base),area);
        return;
    }
    let regions = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(7),
        Constraint::Length(17 - u16::from(ui.session.settings.seats)),
        Constraint::Length(3),
    ])
    .margin(1)
    .split(area);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled(
                    "OPENFELT",
                    Style::default()
                        .fg(Color::Rgb(225, 190, 105))
                        .add_modifier(Modifier::BOLD),
                ),
                Span::raw("   Play the hand. Learn the game."),
            ]),
            Line::from(format!(
                "Local play money · {}/{} blinds · {} seats · Rake OFF · {:?}/{:?} opponents",
                ui.session.settings.small_blind,
                ui.session.settings.big_blind,
                ui.session.settings.seats,
                ui.session.settings.opponents.style,
                ui.session.settings.opponents.difficulty
            )),
        ]),
        regions[0],
    );
    let p = ui.session.view();
    let bb = f64::from(ui.session.settings.big_blind);
    let own = p.seats.iter().find(|s| s.seat == hero()).expect("hero");
    let mut lines = vec![
        Line::from(format!(
            "Hand {} · {} · Pot {} chips / {:.1} BB",
            ui.session.hand_id,
            p.phase.name(),
            p.pot_total,
            f64::from(p.pot_total) / bb
        )),
        Line::from(format!(
            "Board  {}",
            if p.board.is_empty() {
                "—".into()
            } else {
                cards(&p.board)
            }
        )),
        Line::from(format!(
            "Your cards  {}",
            own.hole_cards
                .as_ref()
                .map(|c| cards(c))
                .unwrap_or_default()
        )),
    ];
    for seat in &p.seats {
        let name = if seat.seat == hero() {
            "YOU".into()
        } else {
            format!("Bot {}", seat.seat.as_u8())
        };
        let position = if seat.seat == p.button {
            "D"
        } else if seat.seat == p.small_blind {
            "SB"
        } else if seat.seat == p.big_blind {
            "BB"
        } else {
            ""
        };
        lines.push(Line::from(format!(
            "{} {:<6} {:<2} {:>7} chips  {:>6.1} BB   in {:>5}   {:?} {}",
            if p.to_act == Some(seat.seat) {
                "›"
            } else {
                " "
            },
            name,
            position,
            seat.stack,
            f64::from(seat.stack) / bb,
            seat.street_contribution,
            seat.participation,
            if seat.seat != hero() {
                seat.hole_cards
                    .as_ref()
                    .map(|c| cards(c))
                    .unwrap_or_else(|| "[hidden]".into())
            } else {
                String::new()
            }
        )));
    }
    frame.render_widget(
        Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title(
            if ui.session.coaching.is_some() {
                " TABLE PAUSED "
            } else {
                " TABLE "
            },
        )),
        regions[1],
    );
    let (title, body) = if ui.consent {
        (" ENABLE OPTIONAL CLOUD COACHING ",format!("Destination: https://api.openai.com/v1/responses\nModel: {} · credential: OPENAI_API_KEY\nOnly your pre-decision cards, public table/action data and teaching facts leave this device.\nProvider charges and data terms apply. Limit: {} requests / session.\nEnter enables paid coaching this session. Esc or L plays with local teaching.\nNo request is made until you accept a poker decision.",ui.session.settings.cloud.model,ui.session.settings.cloud.max_requests))
    } else if ui.help {
        (" HOW TO PLAY ","F folds · C checks or calls · R opens bet/raise TO entry in chips.\nEnter submits an amount; arrows adjust by one chip. A asks for all-in confirmation.\nAfter every accepted decision the table pauses: Enter continues, ? expands teaching.\nBetween hands: B tops up/rebuys to 100BB; W withdraws chips; Enter deals.\nRun openfelt --drill <topic> for a short offline practice set.\nBots use reviewed heuristic ranges, style, and difficulty—not solver strategies.\nSettings and private learning history live in your local OpenFelt data folder.\nQ quits at any time. No timer acts for you.".into())
    } else if let Some(d) = &ui.session.coaching {
        let explanation = if ui.session.settings.coaching == CoachingMode::Off {
            "Coaching off. Your decision is accepted.".into()
        } else if let Some(f) = &ui.feedback {
            format!(
                "{} · {} ({})\n{}",
                f.concept,
                f.assessment,
                if ui.provider_feedback {
                    "provider heuristic"
                } else {
                    "local heuristic"
                },
                f.explanation
            )
        } else {
            "Coaching unavailable".into()
        };
        let details = if ui.deep {
            format!(
                "\n{} · {}\nLegal call: {} chips. Contestable pot after call: {} chips.\n{}",
                d.facts.position,
                d.facts.hand_classification,
                d.facts.call_cost,
                d.facts.contestable_pot_after_call,
                d.facts.assumptions[0]
            )
        } else {
            String::new()
        };
        (
            " AFTER YOUR DECISION ",
            format!(
                "You {}. The visible hand is frozen.\n{}{}\nEnter continues · ? {} · Q quits",
                d.accepted_action.description(),
                explanation,
                details,
                if ui.deep { "less" } else { "details" }
            ),
        )
    } else if ui.session.finished() {
        (" HAND COMPLETE ",format!("Session profit: {:+} chips (excludes top-ups and withdrawals).\nLifetime: {} completed hands · {} decisions reviewed.\nEnter next hand · B top up/rebuy to 100BB · W withdraw chips\nBot busts rebuy automatically and are recorded as external chip additions.",ui.session.session_profit,ui.progress.hands,ui.progress.decisions))
    } else {
        (
            " YOUR NEXT DECISION ",
            if p.to_act == Some(hero()) {
                format!("Your turn. {}\nF fold · C check/call · R bet/raise TO · A all-in\nTake your time. Coaching appears after your choice. ? opens help.",ui.session.observation(hero()).map(|o|format!("Call costs {} chips / {:.1} BB.",o.call_cost(),f64::from(o.call_cost())/bb)).unwrap_or_default())
            } else {
                "Opponents are acting…\nOnly each opponent's own cards and public information inform its choice.".into()
            },
        )
    };
    let panel = if ui.deep || ui.help || ui.consent {
        ratatui::layout::Rect {
            x: regions[1].x,
            y: regions[1].y,
            width: regions[1].width,
            height: regions[1].height + regions[2].height,
        }
    } else {
        regions[2]
    };
    frame.render_widget(ratatui::widgets::Clear, panel);
    frame.render_widget(
        Paragraph::new(body)
            .style(base)
            .wrap(Wrap { trim: true })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(title)
                    .border_style(Style::default().fg(Color::Rgb(78, 151, 131))),
            ),
        panel,
    );
    let input = match &ui.input {
        Input::Play => ui.status.clone(),
        Input::AllIn => "ALL-IN: Enter commits your remaining stack. Esc cancels.".into(),
        Input::Withdraw(s) => format!("Withdraw chips: {s} · Enter confirms · Esc cancels"),
        Input::Raise(s) => {
            let bounds = ui
                .session
                .observation(hero())
                .ok()
                .map(|o| {
                    format!(
                        "{}–{}",
                        o.legal.min_raise_to.or(o.legal.min_bet_to).unwrap_or(0),
                        o.legal.all_in_to
                    )
                })
                .unwrap_or_default();
            format!("Bet/raise TO (total street chips) [{bounds}]: {s} · Enter · Esc")
        }
    };
    frame.render_widget(
        Paragraph::new(input)
            .wrap(Wrap { trim: true })
            .style(Style::default().fg(Color::Rgb(225, 190, 105))),
        regions[3],
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    fn ui(seats: u8) -> Ui {
        Ui {
            session: Session::new(Settings {
                seats,
                ..Default::default()
            })
            .unwrap(),
            input: Input::Play,
            feedback: None,
            pending: None,
            status: "Ready".into(),
            deep: false,
            help: false,
            consent: false,
            cloud_enabled: false,
            provider_feedback: false,
            usage: Usage::default(),
            progress: Progress::default(),
            accounted_hands: 0,
            accounted_profit: 0,
            cash_saved: 0,
            replay_saved: 0,
            replay: None,
            saved_progress: vec![],
        }
    }
    fn rendered(ui: &Ui, width: u16, height: u16) -> String {
        let mut terminal =
            Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| draw(frame, ui)).unwrap();
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect::<Vec<_>>()
            .join("")
    }
    #[test]
    fn minimum_size_shows_all_seats_and_resize_guidance() {
        for seats in [2, 6, 9] {
            let ui = ui(seats);
            let text = rendered(&ui, 80, 30);
            assert!(text.contains(&format!("Bot {}", seats - 1)));
            assert!(text.contains("YOUR NEXT DECISION"));
            assert!(text.contains("Ready"));
        }
        let text = rendered(&ui(6), 60, 20);
        assert!(text.contains("needs 80"));
        assert!(text.contains("Q quits safely"));
    }
    #[test]
    fn coaching_and_deep_view_keep_continuation_visible() {
        let mut ui = ui(6);
        while ui.session.view().to_act != Some(hero()) {
            ui.session.step_bot().unwrap();
        }
        let action = ui.session.observation(hero()).unwrap().check_call();
        let d = ui.session.submit(action).unwrap();
        ui.feedback = Some(local_feedback(d));
        let text = rendered(&ui, 80, 30);
        assert!(text.contains("TABLE PAUSED"));
        assert!(text.contains("Enter continues"));
        ui.deep = true;
        let text = rendered(&ui, 80, 30);
        assert!(text.contains("Legal call:"));
        assert!(text.contains("Enter continues"));
        assert!(!text.contains("[hidden]"));
    }
}
