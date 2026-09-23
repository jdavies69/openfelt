use clap::Parser;
use terminal_poker::trainer::{
    drills::{self, DrillTopic},
    policy::{Difficulty, Profile, Style},
    replay,
    solver::RiverScenario,
    solver_ui,
    storage::{CoachingMode, Store},
    tui,
};

#[derive(Parser)]
#[command(
    name = "openfelt",
    version,
    about = "Play the hand. Learn the game. Local play-money Hold'em trainer."
)]
struct Args {
    #[arg(long)]
    seats: Option<u8>,
    #[arg(long)]
    small_blind: Option<u32>,
    #[arg(long)]
    big_blind: Option<u32>,
    #[arg(long, value_enum)]
    opponents: Option<Profile>,
    /// Opponent personality, independent from decision consistency.
    #[arg(long, value_enum)]
    opponent_style: Option<Style>,
    /// Opponent decision consistency; both levels remain heuristic.
    #[arg(long, value_enum)]
    opponent_difficulty: Option<Difficulty>,
    #[arg(long)]
    aggression: Option<f64>,
    #[arg(long)]
    bluff_rate: Option<f64>,
    #[arg(long)]
    mistake_rate: Option<f64>,
    #[arg(long, value_enum)]
    coaching: Option<CoachingMode>,
    /// Provider model ID; use a model supported by the selected coaching adapter.
    #[arg(long)]
    model: Option<String>,
    #[arg(long)]
    max_requests: Option<u32>,
    #[arg(long)]
    max_output_tokens: Option<u32>,
    #[arg(long)]
    timeout_seconds: Option<u64>,
    #[arg(long)]
    input_usd_per_million: Option<f64>,
    #[arg(long)]
    output_usd_per_million: Option<f64>,
    #[arg(long)]
    budget_usd: Option<f64>,
    #[arg(long)]
    pricing_as_of: Option<String>,
    /// Save nonsecret settings before playing. Never saves a key.
    #[arg(long)]
    save_settings: bool,
    #[arg(long)]
    data_dir: Option<std::path::PathBuf>,
    /// Print private storage location and cumulative progress, without launching a table.
    #[arg(long)]
    stats: bool,
    /// Run a three-question offline practice set and exit.
    #[arg(long, value_enum)]
    drill: Option<DrillTopic>,
    /// Practice a local heads-up river decision against explicit ranges and bet sizes.
    #[arg(long)]
    solver_practice: bool,
    /// JSON scenario file for river practice; implies --solver-practice.
    #[arg(long)]
    solver_scenario: Option<std::path::PathBuf>,
    /// Show application licenses, third-party notices, and public source.
    #[arg(long)]
    license: bool,
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    if args.license {
        println!("OpenFelt combined application: AGPL-3.0-or-later.\nOriginal terminal-poker material: MIT; original copyright and license are preserved.\nFull terms: LICENSE, LICENSE-AGPL, LICENSE-MIT, THIRD_PARTY_NOTICES.md.\nCorresponding source: https://github.com/jdavies69/openfelt");
        return Ok(());
    }
    if args.solver_practice || args.solver_scenario.is_some() {
        let scenario = args
            .solver_scenario
            .as_ref()
            .map(
                |path| -> Result<RiverScenario, Box<dyn std::error::Error>> {
                    Ok(serde_json::from_slice(&std::fs::read(path)?)?)
                },
            )
            .transpose()?;
        return solver_ui::run(scenario);
    }
    let explicit_setup = args.seats.is_some()
        || args.small_blind.is_some()
        || args.big_blind.is_some()
        || args.opponents.is_some()
        || args.opponent_style.is_some()
        || args.opponent_difficulty.is_some()
        || args.aggression.is_some()
        || args.bluff_rate.is_some()
        || args.mistake_rate.is_some()
        || args.coaching.is_some()
        || args.model.is_some()
        || args.max_requests.is_some()
        || args.max_output_tokens.is_some()
        || args.timeout_seconds.is_some()
        || args.input_usd_per_million.is_some()
        || args.output_usd_per_million.is_some()
        || args.budget_usd.is_some()
        || args.pricing_as_of.is_some()
        || args.save_settings;
    let store = match args.data_dir {
        Some(root) => Store { root },
        None => Store::default_location()?,
    };
    if let Some(topic) = args.drill {
        let mut progress = store.progress()?;
        let seed = rand::random();
        drills::run(
            topic,
            seed,
            &mut std::io::stdin().lock(),
            &mut std::io::stdout(),
            &mut progress,
        )?;
        store.save("progress.json", &progress)?;
        if let Some(next) = practice_recommendation(&store, &progress) {
            println!("\nRecommended next practice: {next}");
        }
        return Ok(());
    }
    if args.stats {
        let p = store.progress()?;
        println!(
            "OpenFelt data: {}\n{} hands · {} decisions · {:+} completed-hand chips",
            store.root.display(),
            p.hands,
            p.decisions,
            p.profit_chips
        );
        for (topic, d) in &p.drills {
            println!(
                "{topic}: {}/{} accepted answers across {} sets",
                d.correct, d.attempts, d.completed_sets
            );
        }
        if let Some(next) = practice_recommendation(&store, &p) {
            println!("Recommended next practice: {next}");
        }
        return Ok(());
    }
    let first_run = !store.has_settings() && !explicit_setup;
    let mut s = store.settings()?;
    if let Some(v) = args.seats {
        s.seats = v;
    }
    if let Some(v) = args.small_blind {
        s.small_blind = v;
    }
    if let Some(v) = args.big_blind {
        s.big_blind = v;
    }
    if let Some(v) = args.opponents {
        s.opponents.profile = v;
    }
    if let Some(v) = args.opponent_style {
        s.opponents.style = v;
    }
    if let Some(v) = args.opponent_difficulty {
        s.opponents.difficulty = v;
    }
    if let Some(v) = args.aggression {
        s.opponents.aggression = v;
    }
    if let Some(v) = args.bluff_rate {
        s.opponents.bluff_rate = v;
    }
    if let Some(v) = args.mistake_rate {
        s.opponents.mistake_rate = v;
    }
    if let Some(v) = args.coaching {
        s.coaching = v;
    }
    if let Some(v) = args.model {
        s.cloud.model = v;
    }
    if let Some(v) = args.max_requests {
        s.cloud.max_requests = v;
    }
    if let Some(v) = args.max_output_tokens {
        s.cloud.max_output_tokens = v;
    }
    if let Some(v) = args.timeout_seconds {
        s.cloud.timeout_seconds = v;
    }
    if let Some(v) = args.input_usd_per_million {
        s.cloud.input_usd_per_million = Some(v);
    }
    if let Some(v) = args.output_usd_per_million {
        s.cloud.output_usd_per_million = Some(v);
    }
    if let Some(v) = args.budget_usd {
        s.cloud.budget_usd = Some(v);
    }
    if let Some(v) = args.pricing_as_of {
        s.cloud.pricing_as_of = Some(v);
    }
    s.validate()?;
    if args.save_settings {
        store.save("settings.json", &s)?;
    }
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = crossterm::terminal::disable_raw_mode();
        let _ = crossterm::execute!(std::io::stdout(), crossterm::terminal::LeaveAlternateScreen);
        previous(info);
    }));
    tui::run(s, store, first_run)
}

fn practice_recommendation(
    store: &Store,
    progress: &terminal_poker::trainer::storage::Progress,
) -> Option<String> {
    let bookmarks = replay::Archive::load(&store.root)
        .map(|archive| archive.bookmarked_concepts())
        .unwrap_or_default();
    drills::recommendation_with(progress, &bookmarks)
}
