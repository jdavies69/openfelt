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
    /// Makes paid provider calls. Requires an explicit model, budget, request cap, and dated prices.
    #[arg(long)]
    live: bool,
    #[arg(long, value_enum)]
    provider: Option<Provider>,
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
        let provider = select_live_provider(args.provider)?;
        if args.model.as_deref().is_none_or(str::is_empty)
            || args.max_requests.is_none_or(|n| n == 0)
            || args.budget_usd.is_none_or(|n| !n.is_finite() || n <= 0.0)
        {
            return Err(format!(
                "Live evaluation requires {} plus explicit --model, --max-requests, and positive --budget-usd",
                provider.environment()
            )
            .into());
        }
        if args.input_usd_per_million.is_none()
            || args.output_usd_per_million.is_none()
            || args.pricing_as_of.as_deref().is_none_or(str::is_empty)
        {
            return Err(
                "Live evaluation also requires both token prices and --pricing-as-of so the report has a dated cost basis"
                    .into(),
            );
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
        let key = Credential::from_environment(provider)?;
        let mut usage = Usage::default();
        let mut report = offline_report()?;
        report.mode = "live_unreviewed".into();
        report.model = format!("{:?}/{}", provider, settings.model);
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
        report.coverage_gaps = vec![
            "Live responses require independent human rubric review before release.".into(),
            "Schema validity is measured separately from teaching correctness.".into(),
        ];
        report.latency_ms.clear();
        for (index, seed) in [11_u64, 29, 47]
            .into_iter()
            .take(requested as usize)
            .enumerate()
        {
            let decision = scenario_decision(seed)?;
            let started = std::time::Instant::now();
            let pending = provider::start(
                decision.clone(),
                provider,
                settings.clone(),
                key.clone(),
                &mut usage,
            )?;
            let result = loop {
                if let Some(result) = pending.poll() {
                    break result;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            };
            report.latency_ms.push(started.elapsed().as_millis() as u64);
            match result {
                Ok(result) => {
                    let schema_valid =
                        provider::validate_feedback(&result.feedback, &decision).is_ok();
                    report.scenarios[index].schema_valid = schema_valid;
                    report.scenarios[index].input_tokens = result.input_tokens;
                    report.scenarios[index].output_tokens = result.output_tokens;
                    report.scenarios[index].local_fallback_usable =
                        local_feedback_usable(&decision);
                    if schema_valid {
                        usage.input_tokens += result.input_tokens;
                        usage.output_tokens += result.output_tokens;
                    }
                }
                Err(_) => {
                    report.scenarios[index].schema_valid = false;
                    report.scenarios[index].local_fallback_usable =
                        local_feedback_usable(&decision);
                }
            }
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

fn select_live_provider(requested: Option<Provider>) -> Result<Provider, String> {
    if let Some(provider) = requested {
        if std::env::var_os(provider.environment()).is_none() {
            return Err(format!(
                "{} is required for --provider {:?}",
                provider.environment(),
                provider
            ));
        }
        return Ok(provider);
    }
    match (
        std::env::var_os("OPENAI_API_KEY").is_some(),
        std::env::var_os("ANTHROPIC_API_KEY").is_some(),
    ) {
        (true, false) => Ok(Provider::Openai),
        (false, true) => Ok(Provider::Anthropic),
        (true, true) => Err(
            "Both OPENAI_API_KEY and ANTHROPIC_API_KEY are set; pass --provider openai or --provider anthropic"
                .into(),
        ),
        (false, false) => Err(
            "Live evaluation requires OPENAI_API_KEY or ANTHROPIC_API_KEY plus explicit --model, --max-requests, and positive --budget-usd"
                .into(),
        ),
    }
}

fn local_feedback_usable(decision: &terminal_poker::trainer::facts::Decision) -> bool {
    let feedback = terminal_poker::trainer::facts::local_feedback(decision);
    provider::validate_feedback(&feedback, decision).is_ok() && !feedback.explanation.is_empty()
}
