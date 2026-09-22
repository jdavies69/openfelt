//! Reproducible coaching evaluation with executable offline scenarios.
use super::{
    facts::{local_feedback, Decision},
    provider::{request_body, validate_feedback, ProviderSettings},
};
use serde::{Deserialize, Serialize};
use std::time::Instant;

pub const PROMPT_SCHEMA_VERSION: &str = "openfelt-feedback-v1";
pub const SCENARIO_SET_VERSION: &str = "coaching-scenarios-v1";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioDefinition {
    pub id: String,
    pub category: String,
    pub seed: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScenarioResult {
    pub id: String,
    pub category: String,
    pub predecision_only: bool,
    pub schema_valid: bool,
    pub stale_response_rejected: bool,
    pub unsupported_claim_rejected: bool,
    pub local_fallback_usable: bool,
    pub latency_ms: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub factual_correctness: Option<u8>,
    pub useful_explanation: Option<u8>,
    pub appropriate_uncertainty: Option<u8>,
    pub unsupported_strategic_claims: Option<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluationReport {
    pub version: u16,
    pub mode: String,
    pub model: String,
    pub evaluated_at: String,
    pub prompt_schema_version: String,
    pub scenario_set_version: String,
    pub rubric: String,
    pub scenarios: Vec<ScenarioResult>,
    pub latency_ms: Vec<u64>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_basis: String,
    pub estimated_cost_usd: Option<f64>,
    pub release_criteria: Vec<String>,
    pub coverage_gaps: Vec<String>,
}

pub fn definitions() -> Result<Vec<ScenarioDefinition>, String> {
    let items: Vec<ScenarioDefinition> = serde_json::from_str(include_str!(
        "../../tests/fixtures/coaching_eval/scenarios.json"
    ))
    .map_err(|_| "Committed evaluation corpus is invalid")?;
    if items.is_empty()
        || items
            .iter()
            .any(|s| s.id.is_empty() || s.category.is_empty())
    {
        return Err("Committed evaluation corpus failed validation".into());
    }
    Ok(items)
}

pub fn evaluate_offline_case(definition: &ScenarioDefinition) -> Result<ScenarioResult, String> {
    let started = Instant::now();
    let decision = scenario_decision(definition.seed)?;
    let feedback = local_feedback(&decision);
    let schema_valid = validate_feedback(&feedback, &decision).is_ok();
    let mut stale = feedback.clone();
    stale.revision = stale.revision.saturating_add(1);
    let mut unsupported = feedback.clone();
    unsupported.explanation = "This has 75 percent equity and is GTO optimal".into();
    let body = request_body(
        &decision,
        &ProviderSettings {
            model: "offline-fixture".into(),
            ..Default::default()
        },
    );
    let predecision_only = body["input"]
        .as_str()
        .and_then(|input| serde_json::from_str::<Decision>(input).ok())
        .is_some_and(|sent| sent == decision);
    Ok(ScenarioResult {
        id: definition.id.clone(),
        category: definition.category.clone(),
        predecision_only,
        schema_valid,
        stale_response_rejected: validate_feedback(&stale, &decision).is_err(),
        unsupported_claim_rejected: validate_feedback(&unsupported, &decision).is_err(),
        local_fallback_usable: schema_valid && !feedback.explanation.is_empty(),
        latency_ms: started.elapsed().as_millis() as u64,
        input_tokens: 0,
        output_tokens: 0,
        factual_correctness: None,
        useful_explanation: None,
        appropriate_uncertainty: None,
        unsupported_strategic_claims: None,
    })
}

pub fn offline_report() -> Result<EvaluationReport, String> {
    let scenarios = definitions()?
        .iter()
        .map(evaluate_offline_case)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(EvaluationReport {
        version: 1, mode: "offline_fixture".into(), model: "local-heuristic-no-provider-call".into(),
        evaluated_at: "reproducible committed fixture".into(), prompt_schema_version: PROMPT_SCHEMA_VERSION.into(),
        scenario_set_version: SCENARIO_SET_VERSION.into(),
        rubric: "Human review scores zero through four: factual correctness, useful explanation, appropriate uncertainty, and unsupported strategic claims. Null means unreviewed. Schema and safety checks are measured separately.".into(),
        latency_ms: scenarios.iter().map(|s| s.latency_ms).collect(), input_tokens: 0, output_tokens: 0,
        cost_basis: "Offline local evaluation; no provider calls and no cost.".into(), estimated_cost_usd: Some(0.0), scenarios,
        release_criteria: vec!["Every response passes schema and stale-response validation.".into(), "A human reviewer approves teaching correctness for the exact model and configuration.".into(), "Hidden-information isolation and local fallback checks pass.".into(), "Measured cost and latency remain within the explicitly approved release budget.".into()],
        coverage_gaps: vec!["Offline checks do not establish live coaching quality, latency, cancellation behavior, or cost.".into(), "Human rubric fields remain unreviewed.".into()],
    })
}

pub fn scenario_decision(seed: u64) -> Result<Decision, String> {
    let mut session = super::Session::new_seeded_for_evaluation(Default::default(), seed)?;
    for _ in 0..64 {
        if session.view().to_act == Some(super::hero()) {
            let action = session.observation(super::hero())?.check_call();
            return session.submit(action).cloned();
        }
        session.step_bot()?;
    }
    Err("Evaluation scenario did not reach a player decision".into())
}
