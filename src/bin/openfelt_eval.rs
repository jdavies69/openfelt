use clap::Parser;
use std::{fs, path::PathBuf};
use terminal_poker::trainer::{
    evaluation::{offline_report, scenario_decision},
    provider::{self, Credential, Provider, ProviderSettings, Usage},
};

#[derive(Parser)]
#[command(about = "Evaluate coaching fixtures; live mode is guarded and never the default")]
struct Args {
    #[arg(long, conflicts_with = "live")]
    offline: bool,
    /// Acknowledges intent to make paid calls. Live execution is disabled until an adapter runner is selected.
    #[arg(long)]
    live: bool,
    #[arg(long)]
    model: Option<String>,
    #[arg(long)]
    max_requests: Option<u32>,
    #[arg(long)]
    budget_usd: Option<f64>,
    #[arg(long)]
    input_usd_per_million: Option<f64>,
    #[arg(long)]
    output_usd_per_million: Option<f64>,
    #[arg(long)]
    pricing_as_of: Option<String>,
    #[arg(long)]
    output: Option<PathBuf>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    if args.live {
        if std::env::var_os("OPENAI_API_KEY").is_none()
            || args.model.as_deref().is_none_or(str::is_empty)
            || args.max_requests.is_none_or(|n| n == 0)
            || args.budget_usd.is_none_or(|n| !n.is_finite() || n <= 0.0)
        {
            return Err("Live evaluation requires OPENAI_API_KEY plus explicit --model, --max-requests, and positive --budget-usd".into());
        }
        if args.input_usd_per_million.is_none()
            || args.output_usd_per_million.is_none()
            || args.pricing_as_of.as_deref().is_none_or(str::is_empty)
        {
            return Err("Live evaluation also requires both token prices and --pricing-as-of so the report has a dated cost basis".into());
        }
        let requested = args.max_requests.unwrap();
        if requested > 3 {
            return Err(
                "This scenario set contains three cases; --max-requests must be at most 3".into(),
            );
        }
        let settings = ProviderSettings {
            model: args.model.clone().unwrap(),
            max_requests: requested,
            input_usd_per_million: args.input_usd_per_million,
            output_usd_per_million: args.output_usd_per_million,
            budget_usd: args.budget_usd,
            pricing_as_of: args.pricing_as_of.clone(),
            ..Default::default()
        };
        settings.validate()?;
        let _ = Credential::from_environment(Provider::Openai)?;
        let mut usage = Usage::default();
        let mut report = offline_report()?;
        report.mode = "live_unreviewed".into();
        report.model = settings.model.clone();
        report.evaluated_at = format!(
            "unix:{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_secs()
        );
        report.cost_basis = format!(
            "Input ${}/million tokens; output ${}/million tokens; {}",
            settings.input_usd_per_million.unwrap(),
            settings.output_usd_per_million.unwrap(),
            settings.pricing_as_of.as_deref().unwrap()
        );
        report.coverage_gaps =
            vec!["Live responses require independent human rubric review before release.".into()];
        report.latency_ms.clear();
        for (index, seed) in [11_u64, 29, 47]
            .into_iter()
            .take(requested as usize)
            .enumerate()
        {
            let decision = scenario_decision(seed)?;
            let started = std::time::Instant::now();
            let pending = provider::start(
                decision,
                Provider::Openai,
                settings.clone(),
                Credential::new(std::env::var("OPENAI_API_KEY")?)?,
                &mut usage,
            )?;
            let result = loop {
                if let Some(result) = pending.poll() {
                    break result;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }?;
            report.latency_ms.push(started.elapsed().as_millis() as u64);
            report.scenarios[index].schema_valid = true;
            report.scenarios[index].input_tokens = result.input_tokens;
            report.scenarios[index].output_tokens = result.output_tokens;
            usage.input_tokens += result.input_tokens;
            usage.output_tokens += result.output_tokens;
            report.scenarios[index].factual_correctness = None;
            report.scenarios[index].useful_explanation = None;
            report.scenarios[index].appropriate_uncertainty = None;
            report.scenarios[index].unsupported_strategic_claims = None;
        }
        report.scenarios.truncate(requested as usize);
        report.input_tokens = usage.input_tokens;
        report.output_tokens = usage.output_tokens;
        report.estimated_cost_usd = Some(
            (usage.input_tokens as f64 * settings.input_usd_per_million.unwrap()
                + usage.output_tokens as f64 * settings.output_usd_per_million.unwrap())
                / 1_000_000.0,
        );
        let json = serde_json::to_string_pretty(&report)?;
        if let Some(path) = args.output {
            fs::write(path, json)?;
        } else {
            println!("{json}");
        }
        return Ok(());
    }
    let report = offline_report()?;
    let json = serde_json::to_string_pretty(&report)?;
    if let Some(path) = args.output {
        fs::write(path, json)?;
    } else {
        println!("{json}");
    }
    Ok(())
}
