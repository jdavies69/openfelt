use clap::Parser;
use terminal_poker::trainer::{
    policy::Profile,
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
    #[arg(long)]
    aggression: Option<f64>,
    #[arg(long)]
    bluff_rate: Option<f64>,
    #[arg(long)]
    mistake_rate: Option<f64>,
    #[arg(long, value_enum)]
    coaching: Option<CoachingMode>,
    /// OpenAI model ID; use a model supporting Responses structured output.
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
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let store = match args.data_dir {
        Some(root) => Store { root },
        None => Store::default_location()?,
    };
    if args.stats {
        let p = store.progress()?;
        println!(
            "OpenFelt data: {}\n{} hands · {} decisions · {:+} completed-hand chips",
            store.root.display(),
            p.hands,
            p.decisions,
            p.profit_chips
        );
        return Ok(());
    }
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
    tui::run(s, store)
}
