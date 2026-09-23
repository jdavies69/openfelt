use super::{
    facts::{local_feedback, Feedback},
    hero, live_solver,
    provider::{self, CredentialTest, Pending, Provider, SystemKeyring, Usage},
    storage::{CoachingMode, PracticePace, Progress, Settings, Store},
    update::{self, AvailableUpdate, UpdateDone, UpdateMessage},
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
    style::{Color, Style},
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
    time::{Duration, Instant},
};

struct PendingSolver {
    hand_id: u64,
    revision: u64,
    cancel: Arc<AtomicBool>,
    result: Receiver<Result<Feedback, String>>,
}
static LIVE_SOLVER_GATE: Mutex<()> = Mutex::new(());
impl PendingSolver {
    fn start(decision: super::facts::Decision) -> Self {
        let hand_id = decision.observation.hand_id;
        let revision = decision.observation.revision;
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = Arc::clone(&cancel);
        let (sender, result) = mpsc::channel();
        thread::spawn(move || {
            let _gate = LIVE_SOLVER_GATE
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let response = if worker_cancel.load(Ordering::Relaxed) {
                Err("Solver cancelled".into())
            } else {
                live_solver::solve_decision(&decision, &worker_cancel)
            };
            let _ = sender.send(response);
        });
        Self {
            hand_id,
            revision,
            cancel,
            result,
        }
    }
    fn poll(&self) -> Option<Result<Feedback, String>> {
        match self.result.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err("Solver worker stopped".into())),
        }
    }
    fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}
impl Drop for PendingSolver {
    fn drop(&mut self) {
        self.cancel();
    }
}

enum Input {
    Play,
    Raise { amount: String, typing: bool },
    AllIn,
    Withdraw(String),
}
struct SettingsEditor {
    draft: Settings,
    field: usize,
    key: String,
    reveal: bool,
    source: String,
    confirm_test: bool,
    custom_model: bool,
    confirm_forget: bool,
    update_note: String,
    update_offer: Option<AvailableUpdate>,
    confirm_install: bool,
    help: bool,
}
impl SettingsEditor {
    fn handle_help_key(&mut self, code: KeyCode) -> bool {
        if self.help {
            if matches!(code, KeyCode::Esc | KeyCode::Char('?')) {
                self.help = false;
            }
            return true;
        }
        if code == KeyCode::Char('?') {
            self.help = true;
            return true;
        }
        false
    }
    fn switch_provider(&mut self, coaching: CoachingMode, source: String) {
        self.draft.coaching = coaching;
        self.source = source;
        self.key.clear();
        self.reveal = false;
        self.confirm_test = false;
        self.confirm_forget = false;
        if let Some(kind) = selected_provider(coaching) {
            self.draft.cloud.model = kind.models()[0].into();
        }
    }
}
struct TerminalGuard;
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, crossterm::cursor::Show);
    }
}
#[derive(Default)]
struct SessionCredentials {
    openai: Option<Result<Option<provider::Credential>, String>>,
    anthropic: Option<Result<Option<provider::Credential>, String>>,
}
impl SessionCredentials {
    fn slot(
        &mut self,
        provider: Provider,
    ) -> &mut Option<Result<Option<provider::Credential>, String>> {
        match provider {
            Provider::Openai => &mut self.openai,
            Provider::Anthropic => &mut self.anthropic,
        }
    }
    fn resolve(
        &mut self,
        provider: Provider,
        store: &dyn provider::CredentialStore,
    ) -> Result<Option<provider::Credential>, String> {
        self.resolve_with(provider, || {
            provider::resolve_credential(provider, None, store).map(|(key, _)| key)
        })
    }
    fn resolve_with(
        &mut self,
        provider: Provider,
        lookup: impl FnOnce() -> Result<Option<provider::Credential>, String>,
    ) -> Result<Option<provider::Credential>, String> {
        let slot = self.slot(provider);
        if slot.is_none() {
            *slot = Some(lookup());
        }
        slot.as_ref().expect("credential lookup cached").clone()
    }
    fn clear(&mut self) {
        self.openai = None;
        self.anthropic = None;
    }
    fn invalidate(&mut self, provider: Provider) {
        *self.slot(provider) = None;
    }
}
struct Ui {
    session: Session,
    input: Input,
    feedback: Option<Feedback>,
    baseline_feedback: Option<Feedback>,
    solver_pending: Option<PendingSolver>,
    background_solvers: Vec<PendingSolver>,
    solver_feedback: bool,
    solver_note: Option<String>,
    pending: Option<Pending>,
    pending_decision: Option<super::facts::Decision>,
    background_pending: Vec<(super::facts::Decision, Pending)>,
    status: String,
    deep: bool,
    deep_scroll: u16,
    help: bool,
    cloud_enabled: bool,
    provider_feedback: bool,
    usage: Usage,
    progress: Progress,
    accounted_hands: u64,
    accounted_profit: i64,
    cash_saved: usize,
    replay_snapshots: Vec<Vec<u8>>,
    review_changed: bool,
    replay: Option<super::replay_ui::ReplayUi>,
    solver: Option<super::solver_ui::SolverUi>,
    saved_progress: Vec<u8>,
    settings: Option<SettingsEditor>,
    credential_test: Option<CredentialTest>,
    credentials: SessionCredentials,
    update_task: Option<update::UpdateTask>,
    practice_note: Option<String>,
    restart_notice: Option<String>,
    first_run: bool,
}
pub fn run(
    settings: Settings,
    store: Store,
    first_run: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let progress = store.progress()?;
    let cloud_enabled = selected_provider(settings.coaching).is_some();
    let mut ui = Ui {
        session: Session::new(settings)?,
        input: Input::Play,
        feedback: None,
        baseline_feedback: None,
        solver_pending: None,
        background_solvers: Vec::new(),
        solver_feedback: false,
        solver_note: None,
        pending: None,
        pending_decision: None,
        background_pending: Vec::new(),
        status: "F fold · C check/call · R raise · A all-in · G river practice · ? help".into(),
        deep: false,
        deep_scroll: 0,
        help: false,
        cloud_enabled,
        provider_feedback: false,
        usage: Usage::default(),
        progress,
        accounted_hands: 0,
        accounted_profit: 0,
        cash_saved: 0,
        replay_snapshots: Vec::new(),
        review_changed: false,
        replay: None,
        solver: None,
        saved_progress: Vec::new(),
        settings: first_run.then(|| settings_editor(ui_settings_placeholder())),
        credential_test: None,
        credentials: SessionCredentials::default(),
        update_task: None,
        practice_note: None,
        restart_notice: None,
        first_run,
    };
    ui.refresh_practice(&store);
    if let Some(editor) = &mut ui.settings {
        editor.draft = ui.session.settings.clone();
    }
    enable_raw_mode()?;
    let _guard = TerminalGuard;
    execute!(io::stdout(), EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    let mut next_bot = Instant::now();
    let mut redraw = true;
    let mut last_size = None;
    loop {
        if let Some(result) = ui.solver_pending.as_ref().and_then(PendingSolver::poll) {
            let pending = ui.solver_pending.take().expect("polled solver pending");
            redraw |= ui.accept_solver_result(pending.hand_id, pending.revision, result, &store);
        }
        if let Some(solver) = &mut ui.solver {
            redraw |= solver.tick();
        }
        if let Some(result) = ui.credential_test.as_ref().and_then(CredentialTest::poll) {
            ui.credential_test = None;
            ui.status = match result {
                Ok(()) => "Key test succeeded; settings are not saved yet".into(),
                Err(e) => e,
            };
            redraw = true;
        }
        let update_messages = ui
            .update_task
            .as_ref()
            .map(update::UpdateTask::drain)
            .unwrap_or_default();
        if !update_messages.is_empty() {
            redraw = true;
        }
        let update_finished = update_messages
            .iter()
            .any(|message| matches!(message, UpdateMessage::Done(_)));
        for message in update_messages {
            match message {
                UpdateMessage::Progress(text) => ui.status = text,
                UpdateMessage::Done(result) => apply_update_result(&mut ui, result),
            }
        }
        if update_finished {
            ui.update_task = None;
        }
        if ui.restart_notice.is_some() {
            break;
        }
        if let Some(result) = ui.pending.as_ref().and_then(Pending::poll) {
            redraw = true;
            ui.pending = None;
            if let Some(decision) = ui.pending_decision.take() {
                ui.accept_provider_result(decision, result, &store);
            }
        }
        let mut background_index = 0;
        while background_index < ui.background_pending.len() {
            if let Some(result) = ui.background_pending[background_index].1.poll() {
                let (decision, _) = ui.background_pending.remove(background_index);
                ui.accept_provider_result(decision, result, &store);
                redraw = true;
            } else {
                background_index += 1;
            }
        }
        let mut solver_index = 0;
        while solver_index < ui.background_solvers.len() {
            if let Some(result) = ui.background_solvers[solver_index].poll() {
                let pending = ui.background_solvers.remove(solver_index);
                redraw |=
                    ui.accept_solver_result(pending.hand_id, pending.revision, result, &store);
            } else {
                solver_index += 1;
            }
        }
        let size = terminal.size()?;
        if last_size != Some(size) {
            redraw = true;
            last_size = Some(size);
        }
        if !ui.help
            && ui.settings.is_none()
            && ui.replay.is_none()
            && ui.solver.is_none()
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
        if ui.review_changed && ui.replay_persisted() {
            if let Some(replay) = &mut ui.replay {
                if let Err(error) = replay.refresh(&store) {
                    ui.status = error;
                }
            }
            ui.review_changed = false;
            redraw = true;
        }
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
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            break;
        }
        if let Some(solver) = &mut ui.solver {
            if solver.key(key.code) {
                ui.solver = None;
                ui.status = "Returned to table".into();
                next_bot = Instant::now() + Duration::from_millis(450);
            }
            continue;
        }
        if size.width < 80 || size.height < 30 {
            continue;
        }
        let code = match key.code {
            KeyCode::Char(c) => KeyCode::Char(c.to_ascii_lowercase()),
            other => other,
        };
        if let Some(mut editor) = ui.settings.take() {
            if editor.handle_help_key(key.code) {
                ui.settings = Some(editor);
                continue;
            }
            let mut close = false;
            match key.code {
                KeyCode::Esc => {
                    ui.credential_test = None;
                    ui.update_task = None;
                    close = true;
                }
                KeyCode::Up => editor.field = editor.field.saturating_sub(1),
                KeyCode::Down => editor.field = (editor.field + 1).min(12),
                _ if editor.field == 2
                    && apply_api_key_field(&mut editor, &mut ui.status, key, &SystemKeyring) =>
                {
                    ui.credentials.clear();
                }
                KeyCode::Left | KeyCode::Right if editor.field == 0 => {
                    ui.credential_test = None;
                    let coaching = match editor.draft.coaching {
                        CoachingMode::Local | CoachingMode::Off => CoachingMode::Openai,
                        CoachingMode::Openai => CoachingMode::Anthropic,
                        CoachingMode::Anthropic => CoachingMode::Local,
                    };
                    ui.credentials.clear();
                    let source = credential_source_label(coaching, false);
                    editor.switch_provider(coaching, source);
                }
                KeyCode::Left | KeyCode::Right if editor.field == 1 => {
                    if let Some(kind) = selected_provider(editor.draft.coaching) {
                        let models = kind.models();
                        let current = models
                            .iter()
                            .position(|m| *m == editor.draft.cloud.model)
                            .unwrap_or(0);
                        let next = if key.code == KeyCode::Left {
                            (current + models.len() - 1) % models.len()
                        } else {
                            (current + 1) % models.len()
                        };
                        editor.draft.cloud.model = models[next].into();
                        editor.custom_model = false;
                    }
                }
                KeyCode::Char('e' | 'E') if editor.field == 1 && !editor.custom_model => {
                    editor.draft.cloud.model.clear();
                    editor.custom_model = true;
                }
                KeyCode::Backspace if editor.field == 1 && editor.custom_model => {
                    editor.draft.cloud.model.pop();
                }
                KeyCode::Char(c)
                    if editor.field == 1
                        && editor.custom_model
                        && editor.draft.cloud.model.len() < 128 =>
                {
                    if c.is_ascii_alphanumeric() || "-_.:/".contains(c) {
                        editor.draft.cloud.model.push(c);
                    }
                }
                KeyCode::Left if editor.field == 3 => {
                    editor.draft.cloud.max_requests =
                        editor.draft.cloud.max_requests.saturating_sub(1).max(1)
                }
                KeyCode::Right if editor.field == 3 => {
                    editor.draft.cloud.max_requests =
                        (editor.draft.cloud.max_requests + 1).min(1000)
                }
                KeyCode::Left if editor.field == 4 => {
                    editor.draft.seats = editor.draft.seats.saturating_sub(1).max(2)
                }
                KeyCode::Right if editor.field == 4 => {
                    editor.draft.seats = (editor.draft.seats + 1).min(9)
                }
                KeyCode::Left | KeyCode::Right if editor.field == 5 => {
                    editor.draft.opponents.profile = match editor.draft.opponents.profile {
                        super::policy::Profile::Fundamentals => {
                            super::policy::Profile::Recreational
                        }
                        super::policy::Profile::Recreational => super::policy::Profile::Competent,
                        super::policy::Profile::Competent => super::policy::Profile::Fundamentals,
                    }
                }
                KeyCode::Left | KeyCode::Right | KeyCode::Enter if editor.field == 6 => {
                    editor.draft.solver_feedback = !editor.draft.solver_feedback;
                    ui.status = format!(
                        "Solver feedback {} · Save and return to apply",
                        if editor.draft.solver_feedback {
                            "On"
                        } else {
                            "Off"
                        }
                    );
                }
                KeyCode::Left | KeyCode::Right | KeyCode::Enter if editor.field == 7 => {
                    editor.draft.practice_pace = match editor.draft.practice_pace {
                        PracticePace::Learn => PracticePace::Play,
                        PracticePace::Play => PracticePace::Learn,
                    };
                    ui.status = format!(
                        "Practice pace {:?} · Save and return to apply",
                        editor.draft.practice_pace
                    );
                }
                KeyCode::Enter if editor.field == 8 => {
                    if !editor.confirm_test {
                        editor.confirm_test = true;
                        ui.status =
                            "Press Enter again to make one potentially billable test request"
                                .into();
                    } else if ui.credential_test.is_none() {
                        let result = selected_provider(editor.draft.coaching)
                            .ok_or_else(|| "Choose OpenAI or Anthropic".to_string())
                            .and_then(|kind| {
                                let key = if editor.key.is_empty() {
                                    let key = ui.credentials.resolve(kind, &SystemKeyring)?;
                                    editor.source = "loaded for explicit test".into();
                                    key.ok_or_else(|| "Enter or configure an API key".to_string())?
                                } else {
                                    editor.source = "Entered".into();
                                    provider::Credential::new(editor.key.clone())?
                                };
                                provider::test_credential(
                                    kind,
                                    editor.draft.cloud.clone(),
                                    key,
                                    &mut ui.usage,
                                )
                            });
                        match result {
                            Ok(test) => {
                                ui.credential_test = Some(test);
                                ui.status =
                                    format!("Testing {} key…", editor.source.to_ascii_lowercase());
                                if let Err(e) = store.append("usage.jsonl", &ui.usage) {
                                    ui.status = e;
                                }
                            }
                            Err(e) => ui.status = e,
                        }
                        editor.confirm_test = false;
                    }
                }
                KeyCode::Enter if editor.field == 10 => {
                    if ui.update_task.is_some() {
                        ui.status = "Update already in progress".into();
                    } else if !updates_allowed(&ui) {
                        ui.status =
                            "Check and install updates between hands. Finish this hand first."
                                .into();
                    } else if let Some(offer) = editor.update_offer.clone() {
                        if !editor.confirm_install {
                            editor.confirm_install = true;
                            ui.status = format!(
                                "Enter again to install {} and quit. Esc cancels. No keys or hand history are sent.",
                                offer.version
                            );
                        } else {
                            match update::start_install(&offer) {
                                Ok(task) => {
                                    ui.update_task = Some(task);
                                    editor.confirm_install = false;
                                    ui.status =
                                        "Downloading update… Esc cancels before replacement."
                                            .into();
                                }
                                Err(e) => ui.status = e,
                            }
                        }
                    } else {
                        match update::start_check() {
                            Ok(task) => {
                                ui.update_task = Some(task);
                                ui.status = "Checking GitHub releases…".into();
                            }
                            Err(e) => ui.status = e,
                        }
                    }
                }
                KeyCode::Enter if editor.field == 9 => {
                    let credentials_changed = !editor.key.is_empty()
                        || editor.draft.coaching != ui.session.settings.coaching;
                    if credentials_changed {
                        if let Some(kind) = selected_provider(editor.draft.coaching) {
                            ui.credentials.invalidate(kind);
                        } else {
                            ui.credentials.clear();
                        }
                    }
                    match editor.draft.validate().and_then(|_| {
                        if !editor.key.is_empty() {
                            let kind = selected_provider(editor.draft.coaching)
                                .ok_or("Choose OpenAI or Anthropic before saving a key")?;
                            let credential = provider::Credential::new(editor.key.clone())?;
                            store.save("settings.json", &editor.draft)?;
                            return provider::CredentialStore::set(
                                &SystemKeyring,
                                kind,
                                credential,
                            )
                            .map_err(|e| {
                                format!("Settings saved, but the API key could not be saved: {e}")
                            });
                        }
                        store.save("settings.json", &editor.draft)
                    }) {
                        Ok(()) => {
                            if ui.first_run {
                                apply_saved_settings(&mut ui, editor.draft.clone(), true)
                                    .expect("validated settings");
                                ui.first_run = false;
                                ui.status = if editor.key.is_empty() {
                                    format!("Setup saved; {} selected", active_coaching_label(&ui))
                                } else {
                                    format!(
                                        "Setup saved with API key. {}",
                                        provider::credential_storage_hint()
                                    )
                                };
                            } else {
                                apply_saved_settings(&mut ui, editor.draft.clone(), false)
                                    .expect("validated settings");
                                ui.status = if editor.key.is_empty() {
                                    format!(
                                        "Settings saved; {} selected",
                                        active_coaching_label(&ui)
                                    )
                                } else {
                                    format!(
                                        "Settings and API key saved. {}",
                                        provider::credential_storage_hint()
                                    )
                                };
                            }
                            close = true;
                        }
                        Err(e) => ui.status = e,
                    }
                }
                _ => {}
            }
            if !close {
                ui.settings = Some(editor);
            }
            continue;
        }
        if matches!(key.code, KeyCode::Char('q' | 'Q')) {
            break;
        }
        if let Some(mut replay) = ui.replay.take() {
            match replay.key(key.code, &store) {
                Ok(true) => {
                    ui.sync_practice_progress(&store);
                    ui.status = "Returned to table".into();
                }
                Ok(false) => ui.replay = Some(replay),
                Err(e) => {
                    ui.status = e;
                    ui.replay = Some(replay);
                }
            }
            continue;
        }
        if code == KeyCode::Char('s') {
            ui.settings = Some(settings_editor(ui.session.settings.clone()));
            continue;
        }
        if matches!(code, KeyCode::Char('v' | 'r')) && replay_available(&ui) {
            match super::replay_ui::ReplayUi::open(&store) {
                Ok(replay) => ui.replay = Some(replay),
                Err(e) => ui.status = e,
            }
            continue;
        }
        if code == KeyCode::Char('?') {
            if ui.session.coaching.is_some() {
                toggle_coaching_details(&mut ui);
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
        if code == KeyCode::Char('g') {
            match super::solver_ui::SolverUi::new(None) {
                Ok(solver) => ui.solver = Some(solver),
                Err(error) => ui.status = error,
            }
            continue;
        }
        if ui.session.coaching.is_some() {
            if ui.deep && scroll_deep(&mut ui.deep_scroll, code) {
                continue;
            }
            match code {
                KeyCode::Esc if ui.deep => {
                    ui.deep = false;
                    ui.deep_scroll = 0;
                }
                KeyCode::Enter => {
                    ui.retire_current_pending();
                    ui.session.continue_hand();
                    ui.feedback = None;
                    ui.baseline_feedback = None;
                    ui.provider_feedback = false;
                    ui.provider_feedback = false;
                    ui.deep = false;
                    ui.deep_scroll = 0;
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
            Input::Raise { amount, typing } => match code {
                KeyCode::Esc => ui.input = Input::Play,
                KeyCode::Backspace => {
                    if *typing {
                        amount.pop();
                    } else {
                        amount.clear();
                        *typing = true;
                    }
                }
                KeyCode::Char('t') => {
                    amount.clear();
                    *typing = true;
                }
                KeyCode::Char(c @ '1'..='5') if !*typing => {
                    if let Ok(observation) = ui.session.observation(hero()) {
                        *amount =
                            raise_presets(&observation)[c as usize - '1' as usize].to_string();
                    }
                }
                KeyCode::Char(c) if c.is_ascii_digit() && amount.len() < 9 => {
                    amount.push(c);
                    *typing = true;
                }
                KeyCode::Up => {
                    let candidate = amount.parse::<u32>().unwrap_or(0).saturating_add(1);
                    if let Ok(observation) = ui.session.observation(hero()) {
                        *amount = clamp_raise(&observation, candidate).to_string();
                    }
                    *typing = false;
                }
                KeyCode::Down => {
                    let candidate = amount.parse::<u32>().unwrap_or(0).saturating_sub(1);
                    if let Ok(observation) = ui.session.observation(hero()) {
                        *amount = clamp_raise(&observation, candidate).to_string();
                    }
                    *typing = false;
                }
                KeyCode::Right => {
                    let candidate = amount.parse::<u32>().unwrap_or(0).saturating_add(10);
                    if let Ok(observation) = ui.session.observation(hero()) {
                        *amount = clamp_raise(&observation, candidate).to_string();
                    }
                    *typing = false;
                }
                KeyCode::Left => {
                    let candidate = amount.parse::<u32>().unwrap_or(0).saturating_sub(10);
                    if let Ok(observation) = ui.session.observation(hero()) {
                        *amount = clamp_raise(&observation, candidate).to_string();
                    }
                    *typing = false;
                }
                KeyCode::Enter => {
                    if let Ok(o) = ui.session.observation(hero()) {
                        match amount.parse::<u32>() {
                            Ok(n) if clamp_raise(&o, n) == o.legal.all_in_to => {
                                ui.input = Input::AllIn
                            }
                            Ok(n) => {
                                let n = clamp_raise(&o, n);
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
                            if let Some(minimum) = o.legal.min_raise_to.or(o.legal.min_bet_to) {
                                ui.input = Input::Raise {
                                    amount: minimum.to_string(),
                                    typing: false,
                                };
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
                    let decision = d.clone();
                    ui.retire_current_pending();
                    let f = local_feedback(&decision);
                    ui.progress.decisions += 1;
                    ui.progress.note_review(&f.concept, &f.assessment);
                    *ui.progress.concepts.entry(f.concept.clone()).or_default() += 1;
                    if let Err(e) = store.decision(&decision, &f) {
                        ui.status = e;
                    } else {
                        ui.status =
                            "Decision accepted · hand paused · Enter continues · ? details".into();
                    }
                    ui.refresh_practice(&store);
                    ui.baseline_feedback = Some(f.clone());
                    ui.feedback = Some(f);
                    ui.begin_solver_feedback(decision);
                    if ui.cloud_enabled {
                        ui.request(&store);
                    }
                    if ui.session.settings.practice_pace == PracticePace::Play {
                        ui.session.continue_hand();
                        ui.feedback = None;
                        ui.baseline_feedback = None;
                        ui.provider_feedback = false;
                        ui.deep = false;
                        ui.deep_scroll = 0;
                        ui.status = if ui.session.finished() {
                            "Hand complete · R reviews decisions · Enter deals next".into()
                        } else {
                            "Playing · review after the hand".into()
                        };
                        next_bot = Instant::now() + Duration::from_millis(450);
                    }
                }
                Err(e) => ui.status = format!("Not accepted: {e}"),
            }
        }
    }
    ui.pending = None;
    ui.cancel_solver_feedback();
    ui.update_task = None;
    ui.save_progress(&store);
    let restart = ui.restart_notice.clone();
    drop(terminal);
    drop(_guard);
    if let Some(version) = restart {
        println!(
            "OpenFelt {version} is installed. Open a new terminal and run `openfelt --version`."
        );
    }
    Ok(())
}

fn apply_saved_settings(
    ui: &mut Ui,
    settings: Settings,
    reset_session: bool,
) -> Result<(), String> {
    let same_cloud_target = ui.session.settings.coaching == settings.coaching
        && ui.session.settings.cloud.model == settings.cloud.model;
    let was_solver_enabled = ui.session.settings.solver_feedback;
    if reset_session {
        ui.session = Session::new(settings)?;
    } else {
        ui.session.settings = settings;
    }
    ui.cloud_enabled = selected_provider(ui.session.settings.coaching).is_some();
    if !same_cloud_target || reset_session {
        ui.pending = None;
        ui.pending_decision = None;
        ui.background_pending.clear();
        ui.provider_feedback = false;
        ui.baseline_feedback = None;
    }
    if !ui.session.settings.solver_feedback || reset_session {
        ui.cancel_solver_feedback();
        ui.background_solvers.clear();
    }
    if let Some(decision) = ui.session.coaching.clone() {
        let baseline = ui
            .baseline_feedback
            .clone()
            .unwrap_or_else(|| local_feedback(&decision));
        ui.baseline_feedback = Some(baseline.clone());
        if !ui.solver_feedback {
            ui.feedback = Some(baseline);
        }
        if ui.session.settings.solver_feedback && !was_solver_enabled {
            ui.begin_solver_feedback(decision);
        }
        if ui.session.settings.practice_pace == PracticePace::Play {
            ui.retire_current_pending();
            ui.session.continue_hand();
            ui.feedback = None;
            ui.baseline_feedback = None;
            ui.provider_feedback = false;
            ui.status = "Playing · review after the hand".into();
        }
    }
    Ok(())
}

fn active_coaching_label(ui: &Ui) -> String {
    let base = match selected_provider(ui.session.settings.coaching) {
        Some(provider) => format!("{} coaching", provider_label(provider)),
        None if ui.session.settings.coaching == CoachingMode::Off => "coaching off".into(),
        None => "local coaching".into(),
    };
    if ui.session.settings.solver_feedback {
        format!("solver on · {base}")
    } else {
        base
    }
}

fn provider_label(provider: Provider) -> &'static str {
    match provider {
        Provider::Openai => "OpenAI",
        Provider::Anthropic => "Anthropic",
    }
}

fn scroll_deep(scroll: &mut u16, code: KeyCode) -> bool {
    match code {
        KeyCode::Up => *scroll = scroll.saturating_sub(1),
        KeyCode::Down => *scroll = scroll.saturating_add(1),
        KeyCode::PageUp => *scroll = scroll.saturating_sub(8),
        KeyCode::PageDown => *scroll = scroll.saturating_add(8),
        _ => return false,
    }
    true
}
impl Ui {
    fn sync_practice_progress(&mut self, store: &Store) {
        match store.progress() {
            Ok(saved) => self.progress.drills = saved.drills,
            Err(error) => self.status = error,
        }
        self.refresh_practice(store);
    }

    fn replay_persisted(&self) -> bool {
        self.replay_snapshots.len() == self.session.replay_ready.len()
            && self
                .session
                .replay_ready
                .iter()
                .enumerate()
                .all(|(index, hand)| {
                    serde_json::to_vec(hand).ok().as_ref() == self.replay_snapshots.get(index)
                })
    }

    fn accept_provider_result(
        &mut self,
        decision: super::facts::Decision,
        result: Result<provider::ProviderResult, String>,
        store: &Store,
    ) {
        match result {
            Ok(result) => {
                if let Err(error) = provider::validate_feedback(&result.feedback, &decision) {
                    self.status = error;
                    return;
                }
                self.usage.input_tokens += result.input_tokens;
                self.usage.output_tokens += result.output_tokens;
                self.review_changed |= self.session.record_review_feedback(&result.feedback);
                if let Err(error) = store.append("cloud-feedback.jsonl", &result.feedback) {
                    self.status = error;
                }
                let paused = self.session.coaching.as_ref().is_some_and(|current| {
                    current.observation.hand_id == decision.observation.hand_id
                        && current.observation.revision == decision.observation.revision
                });
                if paused {
                    self.baseline_feedback = Some(result.feedback.clone());
                    if !self.solver_feedback {
                        self.feedback = Some(result.feedback);
                    }
                    self.provider_feedback = true;
                    self.status = if self.solver_feedback {
                        "Cloud coaching ready too · solver assessment retained · ? both".into()
                    } else {
                        "Coaching ready · ? details · Enter continues".into()
                    };
                } else if self.session.finished() {
                    self.status = "Coaching added to hand review · R opens it".into();
                }
            }
            Err(error) => {
                self.status = format!("Coaching unavailable: {error}");
            }
        }
        if let Err(error) = store.append("usage.jsonl", &self.usage) {
            self.status = error;
        }
    }

    fn retire_current_pending(&mut self) {
        if let (Some(decision), Some(pending)) = (self.pending_decision.take(), self.pending.take())
        {
            self.background_pending.push((decision, pending));
        }
        if let Some(solver) = self.solver_pending.take() {
            self.background_solvers.push(solver);
        }
        self.solver_feedback = false;
        self.solver_note = None;
    }

    fn accept_solver_result(
        &mut self,
        hand_id: u64,
        revision: u64,
        result: Result<Feedback, String>,
        store: &Store,
    ) -> bool {
        let current = self.session.coaching.as_ref().is_some_and(|decision| {
            decision.observation.hand_id == hand_id && decision.observation.revision == revision
        });
        if !self.session.settings.solver_feedback {
            return false;
        }
        if let Ok(feedback) = &result {
            if feedback.hand_id != hand_id || feedback.revision != revision {
                return false;
            }
        }
        match result {
            Ok(feedback)
                if feedback.hand_id == hand_id
                    && feedback.revision == revision
                    && feedback.evidence_basis == "solver" =>
            {
                if !self.session.record_review_feedback(&feedback) {
                    return false;
                }
                self.review_changed = true;
                if let Err(error) = store.append("solver-feedback.jsonl", &feedback) {
                    self.status = error;
                }
                if current {
                    self.solver_feedback = true;
                    self.solver_note = Some("Solver · modeled ranges".into());
                    self.status = "Solver feedback ready · modeled ranges · ? both".into();
                    self.feedback = Some(feedback);
                } else if self.session.finished() {
                    self.status = "Solver analysis added to hand review · R opens it".into();
                }
            }
            Ok(_) if current => {
                self.solver_note = Some("Solver result rejected; coaching shown".into());
                self.status = self.solver_note.clone().unwrap_or_default();
            }
            Err(error) if current => {
                self.solver_note = Some(format!("Solver unavailable: {error}; coaching shown"));
                self.status = self.solver_note.clone().unwrap_or_default();
            }
            Err(error) => self.status = format!("Solver review unavailable: {error}"),
            Ok(_) => {}
        }
        true
    }

    fn cancel_solver_feedback(&mut self) {
        self.solver_pending = None;
        self.solver_feedback = false;
        self.solver_note = None;
    }

    /// Returns true when a supported local solve was started for this exact decision.
    fn begin_solver_feedback(&mut self, decision: super::facts::Decision) -> bool {
        self.cancel_solver_feedback();
        if !self.session.settings.solver_feedback {
            return false;
        }
        match live_solver::eligibility(&decision) {
            Ok(()) => {
                if self.background_solvers.len() >= 8 {
                    self.solver_note = Some("Solver queue full; local review saved".into());
                    return false;
                }
                self.solver_pending = Some(PendingSolver::start(decision));
                self.solver_note =
                    Some("Solver calculating · local heuristic shown meanwhile".into());
                self.status = self.solver_note.clone().unwrap_or_default();
                true
            }
            Err(reason) => {
                self.solver_note = Some(format!("Solver coverage: {reason}"));
                false
            }
        }
    }

    fn request(&mut self, store: &Store) {
        let Some(d) = self.session.coaching.clone() else {
            return;
        };
        let kind = match self.session.settings.coaching {
            CoachingMode::Openai => Provider::Openai,
            CoachingMode::Anthropic => Provider::Anthropic,
            _ => {
                self.status = "Cloud coaching is not selected".into();
                return;
            }
        };
        let result = self
            .credentials
            .resolve(kind, &SystemKeyring)
            .and_then(|key| {
                key.ok_or_else(|| format!("{} or a saved key is required", kind.environment()))
            })
            .and_then(|key| {
                provider::start(
                    d.clone(),
                    kind,
                    self.session.settings.cloud.clone(),
                    key,
                    &mut self.usage,
                )
            });
        match result {
            Ok(p) => {
                self.pending = Some(p);
                self.pending_decision = Some(d);
                if self.session.settings.practice_pace == PracticePace::Learn {
                    self.status = "Requesting coaching… Enter continues · ? details".into();
                }
            }
            Err(e) => {
                self.pending_decision = None;
                self.status = format!("Coaching unavailable: {e}");
            }
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
        if self.review_changed || self.replay_snapshots.len() != self.session.replay_ready.len() {
            for (index, hand) in self.session.replay_ready.iter().enumerate() {
                let snapshot = serde_json::to_vec(hand).unwrap_or_default();
                if self.replay_snapshots.get(index) == Some(&snapshot) {
                    continue;
                }
                if let Err(error) = store.append("completed-hands.jsonl", hand) {
                    self.status = error;
                    break;
                }
                if index < self.replay_snapshots.len() {
                    self.replay_snapshots[index] = snapshot;
                } else {
                    self.replay_snapshots.push(snapshot);
                }
            }
        }
    }
    fn refresh_practice(&mut self, store: &Store) {
        let bookmarks = super::replay::Archive::load(&store.root)
            .map(|archive| archive.bookmarked_concepts())
            .unwrap_or_default();
        self.practice_note = super::drills::recommendation_with(&self.progress, &bookmarks);
    }
}
fn updates_allowed(ui: &Ui) -> bool {
    ui.first_run || ui.session.finished()
}
fn apply_update_result(ui: &mut Ui, result: Result<UpdateDone, String>) {
    match result {
        Ok(UpdateDone::Checked(update::CheckResult::UpToDate { version })) => {
            ui.status = format!("OpenFelt {version} is up to date.");
            if let Some(editor) = &mut ui.settings {
                editor.update_note = format!("installed {version} · up to date");
                editor.update_offer = None;
                editor.confirm_install = false;
            }
        }
        Ok(UpdateDone::Checked(update::CheckResult::NoRelease)) => {
            ui.status = "No stable OpenFelt release is published yet.".into();
            if let Some(editor) = &mut ui.settings {
                editor.update_note = format!(
                    "installed {} · no stable release",
                    update::current_version()
                );
                editor.update_offer = None;
                editor.confirm_install = false;
            }
        }
        Ok(UpdateDone::Checked(update::CheckResult::Available(offer))) => {
            let origin = update::current_origin();
            if !origin.allows_replacement() {
                ui.status = origin.guidance().into();
                if let Some(editor) = &mut ui.settings {
                    editor.update_note = "use the installer’s upgrade command".into();
                    editor.update_offer = None;
                    editor.confirm_install = false;
                }
            } else if !updates_allowed(ui) {
                ui.status = format!(
                    "Update {} is ready. Install it between hands.",
                    offer.version
                );
                if let Some(editor) = &mut ui.settings {
                    editor.update_note = format!("install {} between hands", offer.version);
                    editor.update_offer = Some(offer);
                    editor.confirm_install = false;
                }
            } else {
                ui.status = format!(
                    "Update {} available. Notes: {}. Enter installs and quits.",
                    offer.version, offer.notes_url
                );
                if let Some(editor) = &mut ui.settings {
                    editor.update_note = format!("install {} · Enter confirms", offer.version);
                    editor.update_offer = Some(offer);
                    editor.confirm_install = false;
                }
            }
        }
        Ok(UpdateDone::Installed(version)) => {
            ui.status = format!("OpenFelt {version} installed. Open a new terminal to run it.");
            ui.restart_notice = Some(version);
        }
        Err(error) => ui.status = error,
    }
}
fn replay_available(ui: &Ui) -> bool {
    ui.session.finished() && ui.session.coaching.is_none() && matches!(ui.input, Input::Play)
}
fn ui_settings_placeholder() -> Settings {
    Settings::default()
}
fn settings_help(field: usize) -> (&'static str, &'static str) {
    match field {
        0 => ("Provider", "Select local, OpenAI, or Anthropic coaching. A saved cloud choice stays selected across launches and can request coaching after accepted decisions. Local and Off use no cloud provider. Changes apply when you save."),
        1 => ("Model", "Choose a listed provider model with Left/Right, or press E to type a custom model ID. This matters only for cloud coaching. Changes apply when you save."),
        2 => ("API key", "Type or paste a provider key for optional cloud coaching. Ctrl-V changes visibility and Ctrl-X confirms forgetting a saved key. Help never displays the key. A new key is stored only when you save."),
        3 => ("Session limit", "Maximum optional cloud coaching requests in one session. Left/Right adjusts the limit; it does not affect local solver calculations. Save to apply."),
        4 => ("Table", "Choose 2–9 seats with Left/Right. Blinds are shown here and can be changed with CLI flags. Save to apply the seat count to a new session."),
        5 => ("Opponents", "Choose a heuristic opponent profile with Left/Right. These bots do not use the solver. Save to apply."),
        6 => ("Solver feedback", "Turn on local solver feedback for eligible heads-up river decisions. Existing coaching remains available in the expanded ? review. Unsupported decisions keep their coaching explanation. Off restores that explanation. Save to apply; no key or cloud request is needed for solver work."),
        7 => ("Practice pace", "Learn pauses after each accepted decision to show feedback. Play continues the hand immediately and saves decision review for the hand end. Left/Right or Enter switches the pace; save to apply. Cloud requests never hold the table."),
        8 => ("Test key", "Press Enter twice to make one explicit, potentially billable provider test request. This does not turn on solver feedback or save settings."),
        9 => ("Save and return", "Validate and save these settings. Unsaved edits remain in this screen until saved. Solver feedback and Learn/Play pace apply after saving."),
        10 => ("Updates", "Check for a published release between hands. Installing an offered release requires a second Enter. This does not change solver or coaching settings."),
        11 => ("Cloud limits", "Shows the configured model output limit, budget, and pricing date. Change these with CLI flags, then save settings. Local solver work uses no provider budget."),
        _ => ("Status", "Shows the latest setup or connection result. It does not expose your API key. Use Up/Down to return to an editable row."),
    }
}
fn settings_editor(draft: Settings) -> SettingsEditor {
    let source = credential_source_label(draft.coaching, false);
    SettingsEditor {
        draft,
        field: 0,
        key: String::new(),
        reveal: false,
        source,
        confirm_test: false,
        custom_model: false,
        confirm_forget: false,
        update_note: format!("installed {} · Enter checks", update::current_version()),
        update_offer: None,
        confirm_install: false,
        help: false,
    }
}

fn refresh_credential_source_label(editor: &mut SettingsEditor) {
    editor.source = credential_source_label(editor.draft.coaching, !editor.key.is_empty());
}
fn credential_source_label(mode: CoachingMode, entered: bool) -> String {
    if entered {
        "entered; not saved".into()
    } else if selected_provider(mode).is_some() {
        "key checked when coaching runs".into()
    } else {
        "local; no key needed".into()
    }
}
fn selected_provider(mode: CoachingMode) -> Option<Provider> {
    match mode {
        CoachingMode::Openai => Some(Provider::Openai),
        CoachingMode::Anthropic => Some(Provider::Anthropic),
        _ => None,
    }
}

/// API-key field input. Plain characters (including `x`/`v`) always insert into the
/// draft secret. Reveal / Forget use Ctrl so they cannot collide with paste.
/// Returns true when the event was handled for this field.
fn apply_api_key_field(
    editor: &mut SettingsEditor,
    status: &mut String,
    key: event::KeyEvent,
    store: &dyn provider::CredentialStore,
) -> bool {
    let control = key.modifiers.contains(KeyModifiers::CONTROL);
    let handled = match key.code {
        KeyCode::Backspace if !control => {
            editor.key.pop();
            editor.confirm_forget = false;
            true
        }
        KeyCode::Char('v' | 'V') if control => {
            editor.reveal = !editor.reveal;
            editor.confirm_forget = false;
            true
        }
        KeyCode::Char('x' | 'X') if control => {
            if !editor.confirm_forget {
                editor.confirm_forget = true;
                *status = "Press Ctrl-X again to forget this provider's saved key".into();
            } else if let Some(kind) = selected_provider(editor.draft.coaching) {
                match store.delete(kind) {
                    Ok(()) => {
                        editor.key.clear();
                        *status = format!(
                            "Saved key forgotten; local coaching selected. {}",
                            provider::credential_storage_hint()
                        );
                        editor.draft.coaching = CoachingMode::Local;
                    }
                    Err(e) => *status = e,
                }
                editor.confirm_forget = false;
            } else {
                editor.confirm_forget = false;
                *status = "Choose OpenAI or Anthropic before forgetting a key".into();
            }
            true
        }
        KeyCode::Char(c)
            if !control
                && !key.modifiers.contains(KeyModifiers::ALT)
                && !c.is_control()
                && editor.key.len() < 256 =>
        {
            editor.key.push(c);
            editor.confirm_forget = false;
            true
        }
        _ => false,
    };
    if handled {
        refresh_credential_source_label(editor);
    }
    handled
}
fn expanded_review_copy(ui: &Ui, decision: &super::facts::Decision) -> String {
    let mut body = format!(
        "YOUR ACTION  {}\nTOPIC  {}",
        accepted_action_copy(&decision.accepted_action),
        ui.feedback
            .as_ref()
            .map(|feedback| normalize_paragraph(&feedback.concept))
            .unwrap_or_else(|| "General decision review".into()),
    );
    if ui.session.settings.solver_feedback {
        body.push_str("\n\nSOLVER ANALYSIS — MODELED RANGES\n");
        if ui.solver_feedback {
            if let Some(feedback) = &ui.feedback {
                body.push_str(&coaching_explanation(feedback));
                if let Some(alternative) = &feedback.alternative_action {
                    body.push_str("\nCONSIDER  ");
                    body.push_str(alternative);
                }
                if !feedback.assumptions.is_empty() {
                    body.push('\n');
                    body.push_str(&feedback.assumptions.join("\n"));
                }
            }
        } else {
            body.push_str(ui.solver_note.as_deref().unwrap_or("Solver not ready"));
        }
    }
    body.push_str(if ui.session.settings.coaching == CoachingMode::Off {
        "\n\nCOACHING EXPLANATION — OFF\n"
    } else if ui.provider_feedback {
        "\n\nCOACHING EXPLANATION — CLOUD PROVIDER HEURISTIC\n"
    } else {
        "\n\nCOACHING EXPLANATION — LOCAL HEURISTIC\n"
    });
    if ui.session.settings.coaching == CoachingMode::Off {
        body.push_str("Coaching is off for this session.");
    } else if let Some(feedback) = ui.baseline_feedback.as_ref().or({
        if ui.solver_feedback {
            None
        } else {
            ui.feedback.as_ref()
        }
    }) {
        body.push_str(&coaching_explanation(feedback));
        if let Some(alternative) = &feedback.alternative_action {
            body.push_str("\nCONSIDER  ");
            body.push_str(alternative);
        }
        if !feedback.assumptions.is_empty() {
            body.push('\n');
            body.push_str(&feedback.assumptions.join("\n"));
        }
    } else {
        body.push_str("Coaching explanation unavailable");
    }
    body.push_str(&format!(
        "\n\nDECISION FACTS\n{} · {}\nLegal call: {} chips. Contestable pot after call: {} chips.\n{}",
        decision.facts.position,
        decision.facts.hand_classification,
        decision.facts.call_cost,
        decision.facts.contestable_pot_after_call,
        decision.facts.assumptions[0],
    ));
    body
}

fn draw(frame: &mut ratatui::Frame, ui: &Ui) {
    if let Some(solver) = &ui.solver {
        solver.draw(frame);
        return;
    }
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
    if let Some(editor) = &ui.settings {
        if editor.help {
            let (heading, explanation) = settings_help(editor.field);
            frame.render_widget(
                Paragraph::new(format!("{explanation}\n\n? or Esc returns to the same setting. Unsaved edits are preserved."))
                    .style(base)
                    .wrap(Wrap { trim: true })
                    .block(Block::default().borders(Borders::ALL)
                        .title(format!("SETTINGS HELP · {heading}"))),
                area,
            );
            return;
        }
        let key = if editor.key.is_empty() {
            "(leave unchanged)".into()
        } else {
            provider::mask_credential(&editor.key, editor.reveal)
        };
        let rows = [
            format!("Provider       {:?}", editor.draft.coaching),
            format!(
                "Model          {}  [←/→ curated · E custom]",
                if editor.draft.cloud.model.is_empty() {
                    "(none)"
                } else {
                    &editor.draft.cloud.model
                }
            ),
            format!("API key        {key}   [Ctrl-V reveal · Ctrl-X forget]"),
            format!(
                "Session limit  {} requests",
                editor.draft.cloud.max_requests
            ),
            format!(
                "Table          {} seats · {}/{} blinds",
                editor.draft.seats, editor.draft.small_blind, editor.draft.big_blind
            ),
            format!("Opponents      {:?}", editor.draft.opponents.profile),
            format!(
                "Solver feedback  {} [←/→/Enter] · local HU river only",
                if editor.draft.solver_feedback {
                    "On"
                } else {
                    "Off"
                }
            ),
            format!(
                "Practice pace  {:?} [←/→/Enter] · Learn pauses / Play continues",
                editor.draft.practice_pace
            ),
            if editor.confirm_test {
                "Test key       CONFIRM billable request with Enter".into()
            } else {
                "Test key       explicit one-request check".into()
            },
            "Save and return".into(),
            format!(
                "Updates        {}{}",
                editor.update_note,
                if editor.confirm_install {
                    " · CONFIRM install"
                } else {
                    ""
                }
            ),
            format!(
                "Cloud limits   {} output tokens · budget {} · pricing {}",
                editor.draft.cloud.max_output_tokens,
                editor
                    .draft
                    .cloud
                    .budget_usd
                    .map(|v| format!("${v:.2}"))
                    .unwrap_or_else(|| "unset".into()),
                editor
                    .draft
                    .cloud
                    .pricing_as_of
                    .as_deref()
                    .unwrap_or("unset; configure with CLI")
            ),
            format!("Status         {}", ui.status),
        ];
        super::table_ui::render_settings_panel(
            frame,
            area,
            &format!("SETTINGS · ? help selected · credential: {}", editor.source),
            &rows,
            editor.field,
        );
        return;
    }
    let p = ui.session.view();
    let bb = f64::from(ui.session.settings.big_blind);
    let (title, body) = if ui.help {
        (" HOW TO PLAY ","F folds · C checks or calls · R opens bet/raise TO entry in chips.\nEnter submits an amount; arrows adjust by one chip. A asks for all-in confirmation.\nLearn pauses after each decision: Enter continues, ? expands teaching. Play continues to hand end.\nBetween hands: R or V reviews saved decisions; B tops up; W withdraws; Enter deals.\nIn replay: arrows browse; B bookmarks; O shows outcome; Esc returns.\nRun openfelt --drill <topic> for a short offline practice set.\nG opens local river solver practice; Esc returns to the table.\nS opens settings. A saved cloud provider stays selected across launches; Local or Off stops requests.\nBetween hands, Updates can check for a release; it never installs by itself.\nBots use reviewed heuristic ranges, style, and difficulty—not solver strategies.\nSettings and private learning history live in your local OpenFelt data folder.\nQ quits at any time. No timer acts for you.".into())
    } else if matches!(ui.input, Input::AllIn) {
        (
            " CONFIRM ALL-IN ",
            "Enter commits your remaining stack. Esc cancels.".into(),
        )
    } else if let Input::Withdraw(amount) = &ui.input {
        (
            " WITHDRAW CHIPS ",
            format!("Amount: {amount}\nEnter confirms · Esc cancels"),
        )
    } else if let Some(d) = &ui.session.coaching {
        let explanation =
            if ui.session.settings.coaching == CoachingMode::Off && !ui.solver_feedback {
                if ui.session.settings.solver_feedback {
                    format!(
                        "Coaching is off. {}",
                        ui.solver_note
                            .as_deref()
                            .unwrap_or("Solver review is pending.")
                    )
                } else {
                    "Coaching off. Your decision is accepted.".into()
                }
            } else if let Some(f) = &ui.feedback {
                coaching_explanation(f)
            } else {
                "Coaching unavailable".into()
            };
        (
            if ui.solver_feedback {
                " AFTER YOUR DECISION · SOLVER "
            } else {
                " AFTER YOUR DECISION "
            },
            if ui.deep {
                expanded_review_copy(ui, d)
            } else {
                structured_coaching_copy(
                    d,
                    &explanation,
                    ui.feedback
                        .as_ref()
                        .and_then(|f| f.alternative_action.as_deref()),
                    false,
                )
            },
        )
    } else if ui.session.finished() {
        let practice = ui
            .practice_note
            .as_ref()
            .map(|note| format!("\n{note}"))
            .unwrap_or_default();
        (" HAND COMPLETE ",format!("{}\nSession profit: {:+} chips (excludes top-ups and withdrawals).\nProgress: {} completed hands · {} decisions reviewed.\nR or V review decisions and mistakes · Enter next hand · B top up · W withdraw{practice}",ui.session.result_summary().unwrap_or_else(|| "Hand settled".into()),ui.session.session_profit,ui.progress.hands,ui.progress.decisions))
    } else if p.to_act == Some(hero()) {
        (
            " YOUR NEXT DECISION ",
            format!(
                "{}\n{}",
                ui.session
                    .observation(hero())
                    .map(|o| {
                        format!(
                            "Call costs {} chips / {:.1} BB.\n{}",
                            o.call_cost(),
                            f64::from(o.call_cost()) / bb,
                            if ui.session.settings.practice_pace == PracticePace::Learn {
                                "Learn pauses after your choice for feedback."
                            } else {
                                "Play continues; review this choice after the hand."
                            }
                        )
                    })
                    .unwrap_or_else(|_| ui.status.clone()),
                ui.status
            ),
        )
    } else {
        (
            " TABLE STATUS ",
            format!(
                "Opponents are acting. Hidden cards stay private.\n{}",
                ui.status
            ),
        )
    };
    let mode = if ui.session.coaching.is_some() {
        super::table_ui::TableMode::Paused
    } else if ui.session.finished() {
        super::table_ui::TableMode::Complete
    } else {
        super::table_ui::TableMode::Playing
    };
    let raise = match &ui.input {
        Input::Raise { amount, .. } => ui
            .session
            .observation(hero())
            .ok()
            .map(|observation| raise_view_for(&observation, amount)),
        _ => None,
    };
    let actions = ui.session.recent_actions();
    let hand_label = ui
        .session
        .coaching
        .as_ref()
        .map(|d| present_hand_label(&d.facts.hand_classification))
        .or_else(|| {
            ui.session.observation(hero()).ok().map(|o| {
                present_hand_label(&super::facts::Facts::calculate(&o).hand_classification)
            })
        });
    let (review_tone, guidance_source) = review_presentation(
        ui.feedback.as_ref(),
        ui.provider_feedback,
        ui.pending.is_some(),
        ui.solver_feedback,
        ui.solver_pending.is_some(),
        ui.solver_note.as_deref().is_some_and(|note| {
            note.starts_with("Solver unavailable") || note.starts_with("Solver result rejected")
        }),
        ui.session.settings.coaching == CoachingMode::Off,
    );
    let active_coaching = active_coaching_label(ui);
    super::table_ui::render(
        frame,
        &super::table_ui::TableRenderState {
            projection: &p,
            hero: hero(),
            hand_id: ui.session.hand_id,
            recent_actions: &actions,
            status: &ui.status,
            mode,
            notice_title: Some(title.trim()),
            notice: Some(&body),
            raise,
            hand_label: hand_label.as_deref(),
            review_tone,
            guidance_source,
            active_coaching: &active_coaching,
        },
    );

    if ui.help {
        let panel = ratatui::layout::Rect {
            x: area.x + 5,
            y: area.y + 4,
            width: area.width.saturating_sub(10),
            height: area.height.saturating_sub(8),
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
    } else if ui.deep && ui.session.coaching.is_some() {
        super::table_ui::render_coaching_details(frame, title, &body, ui.deep_scroll);
    }
}

fn review_presentation(
    feedback: Option<&Feedback>,
    provider_feedback: bool,
    pending: bool,
    solver_feedback: bool,
    solver_pending: bool,
    solver_note: bool,
    coaching_off: bool,
) -> (Option<super::table_ui::ReviewTone>, Option<&'static str>) {
    if solver_feedback {
        return (
            feedback.map(|f| super::table_ui::ReviewTone::from_assessment(&f.assessment)),
            Some("Solver · modeled ranges · ? both explanations"),
        );
    }
    if solver_pending {
        return (
            feedback.map(|f| super::table_ui::ReviewTone::from_assessment(&f.assessment)),
            Some(if coaching_off {
                "Solver calculating · coaching off"
            } else if provider_feedback {
                "AI heuristic · solver calculating"
            } else {
                "Local heuristic · solver calculating"
            }),
        );
    }
    if pending {
        return (
            Some(super::table_ui::ReviewTone::Uncertain),
            Some(if solver_note {
                "Solver unavailable · awaiting provider heuristic"
            } else {
                "Awaiting provider heuristic"
            }),
        );
    }
    let tone =
        feedback.map(|feedback| super::table_ui::ReviewTone::from_assessment(&feedback.assessment));
    let source = feedback.map(|_| {
        if coaching_off && solver_note {
            "Solver unavailable · coaching off"
        } else if coaching_off {
            "Coaching off"
        } else if provider_feedback && solver_note {
            "AI heuristic · solver unavailable"
        } else if solver_note {
            "Local heuristic · solver unavailable"
        } else if provider_feedback {
            "AI suggestion · heuristic"
        } else {
            "Local guidance · heuristic"
        }
    });
    (tone, source)
}

pub fn raise_view_for(
    observation: &super::facts::Observation,
    text: &str,
) -> super::table_ui::RaiseView {
    let minimum = observation
        .legal
        .min_raise_to
        .or(observation.legal.min_bet_to)
        .unwrap_or(observation.legal.all_in_to);
    let maximum = observation.legal.all_in_to;
    let amount = text
        .parse::<u32>()
        .unwrap_or(minimum)
        .clamp(minimum, maximum);
    super::table_ui::RaiseView {
        amount,
        minimum,
        maximum,
        presets: raise_presets(observation),
        is_bet: observation.legal.min_bet_to.is_some(),
    }
}

fn clamp_raise(observation: &super::facts::Observation, amount: u32) -> u32 {
    let minimum = observation
        .legal
        .min_raise_to
        .or(observation.legal.min_bet_to)
        .unwrap_or(observation.legal.all_in_to);
    amount.clamp(minimum, observation.legal.all_in_to)
}

fn raise_presets(observation: &super::facts::Observation) -> [u32; 5] {
    let minimum = observation
        .legal
        .min_raise_to
        .or(observation.legal.min_bet_to)
        .unwrap_or(observation.legal.all_in_to);
    let maximum = observation.legal.all_in_to;
    let base = observation.pot.saturating_add(observation.call_cost());
    let wager = observation.wager;
    let target = |numerator: u32, denominator: u32| {
        wager
            .saturating_add(base.saturating_mul(numerator).div_ceil(denominator))
            .clamp(minimum, maximum)
    };
    [minimum, target(1, 2), target(3, 4), target(1, 1), maximum]
}

fn accepted_action_copy(action: &Action) -> String {
    match action {
        Action::Fold => "You folded".into(),
        Action::Check => "You checked".into(),
        Action::Call(amount) => format!("You called {amount}"),
        Action::Bet(amount) => format!("You bet to {amount}"),
        Action::Raise(amount) => format!("You raised to {amount}"),
        Action::AllIn(amount) => format!("You moved all-in to {amount}"),
    }
}

pub fn present_hand_label(label: &str) -> String {
    if let Some(rank) = label.strip_suffix(" high") {
        let lower = rank.to_ascii_lowercase();
        let singular = match lower.as_str() {
            "aces" => "Ace",
            "kings" => "King",
            "queens" => "Queen",
            "jacks" => "Jack",
            "tens" => "Ten",
            "nines" => "Nine",
            "eights" => "Eight",
            "sevens" => "Seven",
            "sixes" => "Six",
            "fives" => "Five",
            "fours" => "Four",
            "threes" => "Three",
            "twos" => "Two",
            other => other,
        };
        return format!("{singular}-high");
    }
    let mut chars = label.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
        .unwrap_or_default()
}

pub fn structured_coaching_copy(
    decision: &super::facts::Decision,
    explanation: &str,
    alternative: Option<&str>,
    _deep: bool,
) -> String {
    format!(
        "BEFORE ACTION  pot {} · stack {}  ·  YOUR ACTION  {}\nWHY  {}\nCONSIDER  {}",
        decision.observation.pot,
        decision.observation.own().stack,
        accepted_action_copy(&decision.accepted_action),
        explanation,
        alternative.unwrap_or("Review the price, position, and remaining stacks."),
    )
}

pub fn humanize_coaching(copy: &str) -> String {
    normalize_paragraph(
        &copy
            .replace("LateOpen", "late-position range")
            .replace("Late", "late-position")
            .replace("Premium", "premium range")
            .replace("Strong", "strong range")
            .replace("Marginal", "marginal range")
            .replace("Fold", "folding range"),
    )
}

pub fn coaching_explanation(feedback: &Feedback) -> String {
    if feedback.evidence_basis == "solver" {
        normalize_paragraph(&feedback.explanation)
    } else {
        humanize_coaching(&feedback.explanation)
    }
}

/// Compatibility alias for render-preview callers. Compact clipping is owned
/// by the renderer so this returns the same full explanation as details.
pub fn collapsed_coaching_explanation(feedback: &Feedback) -> String {
    coaching_explanation(feedback)
}

fn normalize_paragraph(copy: &str) -> String {
    copy.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn toggle_coaching_details(ui: &mut Ui) {
    ui.deep = !ui.deep;
    ui.deep_scroll = 0;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    fn ui(seats: u8) -> Ui {
        Ui {
            session: Session::new(Settings {
                seats,
                ..Default::default()
            })
            .unwrap(),
            input: Input::Play,
            feedback: None,
            baseline_feedback: None,
            solver_pending: None,
            background_solvers: Vec::new(),
            solver_feedback: false,
            solver_note: None,
            pending: None,
            pending_decision: None,
            background_pending: Vec::new(),
            status: "Ready".into(),
            deep: false,
            deep_scroll: 0,
            help: false,
            cloud_enabled: false,
            provider_feedback: false,
            usage: Usage::default(),
            progress: Progress::default(),
            accounted_hands: 0,
            accounted_profit: 0,
            cash_saved: 0,
            replay_snapshots: Vec::new(),
            review_changed: false,
            replay: None,
            solver: None,
            saved_progress: vec![],
            settings: None,
            credential_test: None,
            credentials: SessionCredentials::default(),
            update_task: None,
            practice_note: None,
            restart_notice: None,
            first_run: false,
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
            assert!(text.contains(&format!("bot {}", seats - 1)));
            assert!(text.contains("ACTION"));
            assert!(text.contains("Ready"));
        }
        let text = rendered(&ui(6), 60, 20);
        assert!(text.contains("needs 80"));
        assert!(text.contains("Q quits safely"));
    }
    #[test]
    fn settings_labels_and_provider_navigation_never_read_credentials() {
        let mut editor = settings_editor(Settings::default());
        assert_eq!(editor.source, "local; no key needed");
        for mode in [
            CoachingMode::Openai,
            CoachingMode::Anthropic,
            CoachingMode::Local,
        ] {
            editor.switch_provider(mode, credential_source_label(mode, false));
        }
        assert_eq!(editor.source, "local; no key needed");
        assert_eq!(
            credential_source_label(CoachingMode::Openai, false),
            "key checked when coaching runs"
        );
        let cloud_ui = ui(2);
        assert!(cloud_ui.credentials.openai.is_none());
        assert!(cloud_ui.credentials.anthropic.is_none());
    }

    #[test]
    fn saved_cloud_provider_stays_enabled_until_local_or_off_without_key_access() {
        let mut ui = ui(2);
        let mut saved = ui.session.settings.clone();
        saved.coaching = CoachingMode::Openai;
        saved.cloud.model = "gpt-5.4-nano".into();

        apply_saved_settings(&mut ui, saved.clone(), false).unwrap();
        assert!(ui.cloud_enabled);
        assert_eq!(ui.session.settings.coaching, CoachingMode::Openai);
        assert_eq!(active_coaching_label(&ui), "OpenAI coaching");
        assert!(ui.credentials.openai.is_none());

        let mut unrelated = saved;
        unrelated.seats = 3;
        apply_saved_settings(&mut ui, unrelated, false).unwrap();
        assert!(ui.cloud_enabled);
        assert!(ui.credentials.openai.is_none());

        let mut local = ui.session.settings.clone();
        local.coaching = CoachingMode::Local;
        apply_saved_settings(&mut ui, local, false).unwrap();
        assert!(!ui.cloud_enabled);
        let mut off = ui.session.settings.clone();
        off.coaching = CoachingMode::Off;
        apply_saved_settings(&mut ui, off, false).unwrap();
        assert!(!ui.cloud_enabled);
    }

    #[test]
    fn unrelated_settings_save_roundtrips_active_cloud_provider() {
        let root = std::env::temp_dir().join(format!(
            "openfelt-provider-persistence-{}-{:016x}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let store = Store { root: root.clone() };
        let mut saved = Settings {
            coaching: CoachingMode::Openai,
            ..Settings::default()
        };
        store.save("settings.json", &saved).unwrap();

        let mut current = ui(2);
        apply_saved_settings(&mut current, store.settings().unwrap(), false).unwrap();
        assert!(current.cloud_enabled);
        saved.seats = 3;
        apply_saved_settings(&mut current, saved.clone(), false).unwrap();
        store
            .save("settings.json", &current.session.settings)
            .unwrap();

        let reloaded = store.settings().unwrap();
        assert_eq!(reloaded.coaching, CoachingMode::Openai);
        assert_eq!(reloaded.seats, 3);
        let mut relaunched = ui(2);
        apply_saved_settings(&mut relaunched, reloaded, false).unwrap();
        assert!(relaunched.cloud_enabled);
        assert!(relaunched.credentials.openai.is_none());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn changing_cloud_destination_activates_saved_provider_without_resolving_a_key() {
        let mut ui = ui(2);
        let mut openai = ui.session.settings.clone();
        openai.coaching = CoachingMode::Openai;
        apply_saved_settings(&mut ui, openai, false).unwrap();
        assert!(ui.cloud_enabled);

        let mut anthropic = ui.session.settings.clone();
        anthropic.coaching = CoachingMode::Anthropic;
        anthropic.cloud.model = "claude-sonnet-4-5".into();
        apply_saved_settings(&mut ui, anthropic, false).unwrap();
        assert!(ui.cloud_enabled);
        assert_eq!(active_coaching_label(&ui), "Anthropic coaching");
        assert!(ui.credentials.openai.is_none());
        assert!(ui.credentials.anthropic.is_none());
    }

    #[test]
    fn explicit_credential_lookup_caches_success_missing_and_denial_by_provider() {
        let gets = AtomicUsize::new(0);
        let mut cache = SessionCredentials::default();
        for _ in 0..2 {
            assert!(cache
                .resolve_with(Provider::Openai, || {
                    gets.fetch_add(1, Ordering::SeqCst);
                    Ok(Some(provider::Credential::new("old-key".into()).unwrap()))
                })
                .unwrap()
                .is_some());
        }
        assert_eq!(gets.load(Ordering::SeqCst), 1);

        assert!(cache
            .resolve_with(Provider::Anthropic, || {
                gets.fetch_add(1, Ordering::SeqCst);
                Ok(None)
            })
            .unwrap()
            .is_none());
        assert_eq!(gets.load(Ordering::SeqCst), 2);
        assert!(cache
            .resolve_with(Provider::Anthropic, || panic!(
                "missing result should be cached"
            ))
            .unwrap()
            .is_none());

        cache.invalidate(Provider::Openai);
        assert_eq!(
            cache
                .resolve_with(Provider::Openai, || {
                    gets.fetch_add(1, Ordering::SeqCst);
                    Err("denied".into())
                })
                .err()
                .as_deref(),
            Some("denied")
        );
        assert_eq!(
            cache
                .resolve_with(Provider::Openai, || panic!("denial should be cached"))
                .err()
                .as_deref(),
            Some("denied")
        );
        assert_eq!(gets.load(Ordering::SeqCst), 3);
    }
    #[test]
    fn settings_render_masks_credentials_and_exposes_keyboard_setup() {
        let mut ui = ui(6);
        ui.settings = Some(SettingsEditor {
            draft: Settings::default(),
            field: 2,
            key: "sk-secretabcd".into(),
            reveal: false,
            source: "Keyring".into(),
            confirm_test: false,
            custom_model: false,
            confirm_forget: false,
            update_note: format!("installed {} · Enter checks", update::current_version()),
            update_offer: None,
            confirm_install: false,
            help: false,
        });
        let screen = rendered(&ui, 100, 36);
        assert!(screen.contains("SETTINGS"));
        assert!(screen.contains("sk-••••••••abcd"));
        assert!(!screen.contains("sk-secretabcd"));
        assert!(screen.contains("explicit one-request check"));
        assert!(screen.contains("credential: Keyring"));
        assert!(screen.contains("Ctrl-V reveal"));
        assert!(screen.contains("Ctrl-X forget"));
        assert!(screen.contains("Updates"));
        assert!(screen.contains(update::current_version()));
    }
    #[test]
    fn settings_help_keeps_selection_and_unsaved_edits_without_revealing_key() {
        let mut editor = settings_editor(Settings::default());
        editor.field = 6;
        editor.draft.solver_feedback = true;
        editor.key = "sk-private-value".into();
        assert!(editor.handle_help_key(KeyCode::Char('?')));
        assert!(editor.help);
        let mut ui = ui(2);
        ui.settings = Some(editor);
        let screen = rendered(&ui, 80, 30);
        assert!(screen.contains("SETTINGS HELP · Solver feedback"));
        assert!(screen.contains("eligible heads-up river"));
        assert!(!screen.contains("sk-private-value"));
        let editor = ui.settings.as_mut().unwrap();
        assert!(editor.handle_help_key(KeyCode::Esc));
        assert!(!editor.help);
        assert_eq!(editor.field, 6);
        assert!(editor.draft.solver_feedback);
        assert_eq!(editor.key, "sk-private-value");
        for field in 0..=11 {
            assert!(!settings_help(field).1.is_empty());
        }
    }
    #[test]
    fn disabling_solver_restores_retained_provider_coaching() {
        let mut ui = ui(2);
        while ui.session.view().to_act != Some(hero()) {
            ui.session.step_bot().unwrap();
        }
        let action = ui.session.observation(hero()).unwrap().check_call();
        let decision = ui.session.submit(action).unwrap().clone();
        let mut provider = local_feedback(&decision);
        provider.explanation = "Provider coaching retained".into();
        provider.evidence_basis = "heuristic".into();
        let mut solver = provider.clone();
        solver.explanation = "Solver grade retained".into();
        solver.evidence_basis = "solver".into();
        ui.session.settings.solver_feedback = true;
        ui.baseline_feedback = Some(provider.clone());
        ui.feedback = Some(solver);
        ui.solver_feedback = true;
        ui.provider_feedback = true;
        let mut updated = ui.session.settings.clone();
        updated.solver_feedback = false;
        apply_saved_settings(&mut ui, updated, false).unwrap();
        assert!(!ui.solver_feedback);
        assert!(ui.provider_feedback);
        assert_eq!(
            ui.feedback.as_ref().unwrap().explanation,
            "Provider coaching retained"
        );
        assert_eq!(
            ui.baseline_feedback.as_ref().unwrap().explanation,
            "Provider coaching retained"
        );
    }

    #[test]
    fn stale_solver_result_cannot_replace_current_coaching() {
        let mut ui = ui(2);
        while ui.session.view().to_act != Some(hero()) {
            ui.session.step_bot().unwrap();
        }
        let action = ui.session.observation(hero()).unwrap().check_call();
        let decision = ui.session.submit(action).unwrap().clone();
        let local = local_feedback(&decision);
        let mut stale = local.clone();
        stale.evidence_basis = "solver".into();
        ui.session.settings.solver_feedback = true;
        ui.baseline_feedback = Some(local.clone());
        ui.feedback = Some(local);
        let store = Store {
            root: std::env::temp_dir().join("openfelt-stale-solver-test-no-write"),
        };
        assert!(!ui.accept_solver_result(
            decision.observation.hand_id,
            decision.observation.revision + 1,
            Ok(stale),
            &store
        ));
        assert!(!ui.solver_feedback);
        assert_ne!(ui.feedback.as_ref().unwrap().evidence_basis, "solver");
    }

    #[test]
    fn play_persists_late_feedback_after_hand_without_provider_request() {
        let root = std::env::temp_dir().join(format!(
            "openfelt-play-late-review-{}-{:016x}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let store = Store { root: root.clone() };
        let mut ui = ui(2);
        ui.session.settings.practice_pace = PracticePace::Play;
        while ui.session.view().to_act != Some(hero()) {
            ui.session.step_bot().unwrap();
        }
        let decision = ui.session.submit(Action::Fold).unwrap().clone();
        ui.session.continue_hand();
        assert!(ui.session.finished());
        ui.save_progress(&store);
        let mut cloud = local_feedback(&decision);
        cloud.assessment = "uncertain".into();
        cloud.explanation = "Review the price and remaining stack.".into();
        cloud.concept = "Decision review".into();
        cloud.assumptions.clear();
        cloud.alternative_action = None;
        assert!(provider::validate_feedback(&cloud, &decision).is_ok());
        ui.accept_provider_result(
            decision,
            Ok(provider::ProviderResult {
                feedback: cloud,
                input_tokens: 4,
                output_tokens: 6,
            }),
            &store,
        );
        assert!(ui.review_changed);
        ui.save_progress(&store);
        assert!(ui.replay_persisted());
        let archive = super::super::replay::Archive::load(&root).unwrap();
        assert_eq!(archive.hands.len(), 1);
        assert_eq!(
            archive.hands[0].decisions[0].feedback.explanation,
            "Review the price and remaining stack."
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn replay_practice_progress_survives_resuming_table_and_next_decision() {
        let root = std::env::temp_dir().join(format!(
            "openfelt-replay-progress-{}-{:016x}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let store = Store { root: root.clone() };
        let mut ui = ui(2);
        ui.save_progress(&store);
        let mut persisted = store.progress().unwrap();
        persisted.drills.insert(
            "pot_odds".into(),
            super::super::storage::DrillProgress {
                attempts: 1,
                correct: 1,
                completed_sets: 1,
            },
        );
        store.save("progress.json", &persisted).unwrap();
        ui.sync_practice_progress(&store);
        while ui.session.view().to_act != Some(hero()) {
            ui.session.step_bot().unwrap();
        }
        let decision = ui.session.submit(Action::Fold).unwrap().clone();
        let feedback = local_feedback(&decision);
        ui.progress.decisions += 1;
        ui.progress
            .note_review(&feedback.concept, &feedback.assessment);
        ui.save_progress(&store);
        let after = store.progress().unwrap();
        assert_eq!(after.decisions, 1);
        assert_eq!(after.drills["pot_odds"].completed_sets, 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn solver_and_provider_explanations_remain_separate_in_review() {
        let mut ui = ui(2);
        while ui.session.view().to_act != Some(hero()) {
            ui.session.step_bot().unwrap();
        }
        let action = ui.session.observation(hero()).unwrap().check_call();
        let decision = ui.session.submit(action).unwrap().clone();
        let mut provider = local_feedback(&decision);
        provider.explanation = "Provider coaching details stay available".into();
        let mut solver = provider.clone();
        solver.explanation = "Solver numerical assessment is primary".into();
        solver.evidence_basis = "solver".into();
        solver.alternative_action = Some("Use the solver's best action".into());
        solver.assumptions = vec!["Modeled ranges and sizes".into()];
        ui.session.settings.solver_feedback = true;
        ui.baseline_feedback = Some(provider);
        ui.feedback = Some(solver);
        ui.solver_feedback = true;
        ui.provider_feedback = true;
        let compact = rendered(&ui, 100, 40);
        assert!(compact.contains("Solver numerical assessment"));
        assert!(compact.contains("Use the solver's best action"));
        assert!(compact.contains("Solver · modeled ranges"));
        ui.deep = true;
        let expanded = rendered(&ui, 100, 40);
        assert!(expanded.contains("SOLVER ANALYSIS"));
        ui.deep_scroll = 12;
        let lower = rendered(&ui, 100, 40);
        assert!(lower.contains("COACHING EXPLANATION"));
        assert!(
            expanded_review_copy(&ui, ui.session.coaching.as_ref().unwrap())
                .contains("Provider coaching details")
        );
    }
    #[test]
    fn api_key_paste_keeps_x_and_provider() {
        struct FakeStore;
        impl provider::CredentialStore for FakeStore {
            fn get(&self, _: Provider) -> Result<Option<provider::Credential>, String> {
                Ok(None)
            }
            fn set(&self, _: Provider, _: provider::Credential) -> Result<(), String> {
                Ok(())
            }
            fn delete(&self, _: Provider) -> Result<(), String> {
                Ok(())
            }
        }

        let mut editor = SettingsEditor {
            draft: Settings {
                coaching: CoachingMode::Openai,
                ..Default::default()
            },
            field: 2,
            key: String::new(),
            reveal: false,
            source: "Missing".into(),
            confirm_test: false,
            custom_model: false,
            confirm_forget: false,
            update_note: "installed".into(),
            update_offer: None,
            confirm_install: false,
            help: false,
        };
        let mut status = String::new();
        let store = FakeStore;
        // Mimic a paste that includes lowercase and uppercase x (and v), which used
        // to arm Forget / toggle Reveal instead of inserting.
        let pasted = "sk-proj-abcxXvwzgA";
        for ch in pasted.chars() {
            assert!(apply_api_key_field(
                &mut editor,
                &mut status,
                event::KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE),
                &store,
            ));
        }
        assert_eq!(editor.key, pasted);
        assert_eq!(editor.draft.coaching, CoachingMode::Openai);
        assert!(!editor.confirm_forget);
        assert!(!editor.reveal);
        assert!(status.is_empty());

        // Ctrl-X arms forget without inserting; provider stays until confirmed.
        assert!(apply_api_key_field(
            &mut editor,
            &mut status,
            event::KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
            &store,
        ));
        assert_eq!(editor.key, pasted);
        assert!(editor.confirm_forget);
        assert!(status.contains("Ctrl-X"));
        assert_eq!(editor.draft.coaching, CoachingMode::Openai);
    }
    #[test]
    fn provider_switch_discards_uncommitted_secret_and_confirmations() {
        let mut editor = SettingsEditor {
            draft: Settings {
                coaching: CoachingMode::Openai,
                ..Default::default()
            },
            field: 0,
            key: "openai-secret".into(),
            reveal: true,
            source: "Environment".into(),
            confirm_test: true,
            custom_model: true,
            confirm_forget: true,
            update_note: "installed".into(),
            update_offer: None,
            confirm_install: false,
            help: false,
        };
        editor.switch_provider(CoachingMode::Anthropic, "Missing".into());
        assert!(editor.key.is_empty());
        assert!(!editor.reveal && !editor.confirm_test && !editor.confirm_forget);
        assert_eq!(editor.draft.cloud.model, Provider::Anthropic.models()[0]);
    }
    #[test]
    fn replay_opens_only_between_hands() {
        let mut ui = ui(2);
        assert!(!replay_available(&ui));
        for _ in 0..100 {
            if ui.session.finished() {
                break;
            }
            if ui.session.view().to_act == Some(hero()) {
                ui.session.submit(Action::Fold).unwrap();
                ui.session.continue_hand();
            } else {
                ui.session.step_bot().unwrap();
            }
        }
        assert!(ui.session.finished());
        assert!(replay_available(&ui));
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
        assert!(text.contains("TABLE BEFORE ACTION"));
        assert!(text.contains("YOUR ACTION"));
        assert!(text.contains("WHY"));
        assert!(text.contains("CONSIDER"));
        assert!(text.contains("Enter continue"));
        ui.deep = true;
        let text = rendered(&ui, 80, 30);
        let full = expanded_review_copy(&ui, ui.session.coaching.as_ref().unwrap());
        assert!(full.contains("Legal call:"));
        assert!(full.contains("Contestable pot assumes"));
        assert!(text.contains("Enter continue"));
        assert!(!text.contains("[hidden]"));

        ui.feedback.as_mut().unwrap().explanation = format!(
            "{} FULL_FEEDBACK_TAIL",
            "A deliberately long coaching explanation with concrete strategic context. ".repeat(30)
        );
        let text = rendered(&ui, 80, 30);
        assert!(!text.contains("FULL_FEEDBACK_TAIL"));
        for _ in 0..400 {
            assert!(scroll_deep(&mut ui.deep_scroll, KeyCode::PageDown));
            if rendered(&ui, 80, 30).contains("FULL_FEEDBACK_TAIL") {
                break;
            }
        }
        let text = rendered(&ui, 80, 30);
        assert!(text.contains("FULL_FEEDBACK_TAIL"));
        let scrolled = ui.deep_scroll;
        assert!(scroll_deep(&mut ui.deep_scroll, KeyCode::Up));
        assert_eq!(ui.deep_scroll, scrolled - 1);
        assert!(scroll_deep(&mut ui.deep_scroll, KeyCode::PageUp));
        assert_eq!(ui.deep_scroll, scrolled - 9);
    }

    #[test]
    fn compact_and_expanded_coaching_share_reasoning_without_side_effects() {
        let mut ui = ui(6);
        while ui.session.view().to_act != Some(hero()) {
            ui.session.step_bot().unwrap();
        }
        let action = ui.session.observation(hero()).unwrap().check_call();
        let decision = ui.session.submit(action).unwrap();
        let mut feedback = local_feedback(decision);
        feedback.explanation = format!(
            "First line explains the price.\n\n{}REASONING_TAIL",
            "Second line preserves the positional reason and adds enough context. ".repeat(4)
        );
        feedback.alternative_action =
            Some("ALTERNATIVE_SENTINEL: fold and preserve the stack.".into());
        let expected = coaching_explanation(&feedback);
        assert_eq!(expected, collapsed_coaching_explanation(&feedback));
        assert!(!expected.contains('\n'));
        assert_eq!(
            structured_coaching_copy(
                decision,
                &expected,
                feedback.alternative_action.as_deref(),
                false,
            ),
            structured_coaching_copy(
                decision,
                &expected,
                feedback.alternative_action.as_deref(),
                true,
            )
        );
        ui.feedback = Some(feedback);

        let compact = rendered(&ui, 80, 30);
        assert!(compact.contains("WHY"));
        assert!(compact.contains('…'));
        assert!(compact.contains("CONSIDER  ALTERNATIVE_SENTINEL"));
        assert_eq!(compact.matches("Enter continue").count(), 1);

        let assessment = ui.feedback.as_ref().unwrap().assessment.clone();
        assert!(ui.pending.is_none());
        toggle_coaching_details(&mut ui);
        assert!(ui.deep);
        assert!(ui.pending.is_none());
        assert_eq!(ui.feedback.as_ref().unwrap().assessment, assessment);
        let expanded = rendered(&ui, 100, 36);
        assert!(expanded.contains("TOPIC"));
        assert!(
            expanded_review_copy(&ui, ui.session.coaching.as_ref().unwrap())
                .contains("First line explains the price")
        );
        assert!(!expanded.contains("Full reasoning shown above"));
    }

    #[test]
    fn confirmation_and_edit_modes_are_visible_on_the_live_table() {
        let mut ui = ui(2);
        while ui.session.view().to_act != Some(hero()) {
            ui.session.step_bot().unwrap();
        }
        ui.input = Input::AllIn;
        let screen = rendered(&ui, 80, 30);
        assert!(screen.contains("CONFIRM ALL-IN"));
        assert!(screen.contains("Esc"));
        assert!(screen.contains("cancels"));

        ui.input = Input::Withdraw("125".into());
        let screen = rendered(&ui, 80, 30);
        assert!(screen.contains("WITHDRAW CHIPS"));
        assert!(screen.contains("Amount: 125"));
    }

    #[test]
    fn raise_display_and_submission_share_clamped_amounts() {
        let mut ui = ui(6);
        while ui.session.view().to_act != Some(hero()) {
            ui.session.step_bot().unwrap();
        }
        let observation = ui.session.observation(hero()).unwrap();
        let minimum = observation
            .legal
            .min_raise_to
            .or(observation.legal.min_bet_to)
            .unwrap();
        let maximum = observation.legal.all_in_to;
        assert_eq!(raise_view_for(&observation, "0").amount, minimum);
        assert_eq!(clamp_raise(&observation, 0), minimum);
        assert_eq!(raise_view_for(&observation, "999999").amount, maximum);
        assert_eq!(clamp_raise(&observation, 999_999), maximum);
        assert!(raise_presets(&observation)
            .into_iter()
            .all(|amount| (minimum..=maximum).contains(&amount)));
        assert_eq!(raise_presets(&observation)[4], maximum);
    }

    #[test]
    fn pending_provider_review_is_neutral_until_validated_feedback_arrives() {
        let mut ui = ui(2);
        while ui.session.view().to_act != Some(hero()) {
            ui.session.step_bot().unwrap();
        }
        let action = ui.session.observation(hero()).unwrap().check_call();
        let decision = ui.session.submit(action).unwrap();
        let mut feedback = local_feedback(decision);
        feedback.assessment = "reasonable".into();
        assert_eq!(
            review_presentation(Some(&feedback), false, false, false, false, false, false),
            (
                Some(crate::trainer::table_ui::ReviewTone::Good),
                Some("Local guidance · heuristic")
            )
        );
        assert_eq!(
            review_presentation(Some(&feedback), false, true, false, false, false, false),
            (
                Some(crate::trainer::table_ui::ReviewTone::Uncertain),
                Some("Awaiting provider heuristic")
            )
        );
        feedback.assessment = "reconsider".into();
        assert_eq!(
            review_presentation(Some(&feedback), false, false, false, false, false, false).0,
            Some(crate::trainer::table_ui::ReviewTone::Reconsider)
        );
        feedback.assessment = "uncertain".into();
        assert_eq!(
            review_presentation(Some(&feedback), true, false, false, false, false, false),
            (
                Some(crate::trainer::table_ui::ReviewTone::Uncertain),
                Some("AI suggestion · heuristic")
            )
        );
    }

    #[test]
    fn compact_paused_tables_keep_every_seat_and_review_step_visible() {
        for seats in [6, 9] {
            let mut ui = ui(seats);
            while ui.session.view().to_act != Some(hero()) {
                ui.session.step_bot().unwrap();
            }
            let action = ui.session.observation(hero()).unwrap().check_call();
            let decision = ui.session.submit(action).unwrap();
            ui.feedback = Some(local_feedback(decision));
            let projection = ui.session.view();
            let text = rendered(&ui, 80, 30);
            for seat in projection.seats.iter().filter(|seat| seat.seat != hero()) {
                assert!(text.contains(&format!("bot {}", seat.seat.as_u8())));
                assert!(text.contains(&seat.stack.to_string()));
            }
            for label in ["TABLE BEFORE ACTION", "YOUR ACTION", "WHY", "CONSIDER"] {
                assert!(text.contains(label), "missing {label} at {seats} seats");
            }
            assert_eq!(text.matches("Enter continue").count(), 1);
        }
    }
}
