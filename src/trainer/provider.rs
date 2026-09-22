//! Optional direct provider adapters. No credentials in Debug, settings, prompts or errors.
use super::facts::{Decision, Feedback};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{sync::mpsc, time::Duration};

pub const KEYRING_SERVICE: &str = "dev.openfelt.coaching";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    Openai,
    Anthropic,
}
impl Provider {
    pub const fn account(self) -> &'static str {
        match self {
            Self::Openai => "openai-api-key",
            Self::Anthropic => "anthropic-api-key",
        }
    }
    pub const fn environment(self) -> &'static str {
        match self {
            Self::Openai => "OPENAI_API_KEY",
            Self::Anthropic => "ANTHROPIC_API_KEY",
        }
    }
    pub const fn endpoint(self) -> &'static str {
        match self {
            Self::Openai => "https://api.openai.com/v1/responses",
            Self::Anthropic => "https://api.anthropic.com/v1/messages",
        }
    }
    pub const fn models(self) -> &'static [&'static str] {
        match self {
            Self::Openai => &["gpt-4o-mini", "gpt-4o", "gpt-4.1", "gpt-5"],
            Self::Anthropic => &[
                "claude-haiku-4-5-20251001",
                "claude-sonnet-5",
                "claude-opus-5",
            ],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ProviderSettings {
    pub model: String,
    pub timeout_seconds: u64,
    pub max_output_tokens: u32,
    pub max_requests: u32,
    pub input_usd_per_million: Option<f64>,
    pub output_usd_per_million: Option<f64>,
    pub budget_usd: Option<f64>,
    pub pricing_as_of: Option<String>,
}
impl Default for ProviderSettings {
    fn default() -> Self {
        Self {
            model: String::new(),
            timeout_seconds: 20,
            max_output_tokens: 600,
            max_requests: 30,
            input_usd_per_million: None,
            output_usd_per_million: None,
            budget_usd: None,
            pricing_as_of: None,
        }
    }
}
impl ProviderSettings {
    pub fn validate(&self) -> Result<(), String> {
        if self.model.len() > 128
            || !self
                .model
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"-_.:/".contains(&c))
        {
            return Err("Invalid model ID".into());
        }
        if !(1..=120).contains(&self.timeout_seconds)
            || !(128..=4096).contains(&self.max_output_tokens)
            || !(1..=1000).contains(&self.max_requests)
        {
            return Err("Invalid coaching limits".into());
        }
        for price in [
            self.input_usd_per_million,
            self.output_usd_per_million,
            self.budget_usd,
        ]
        .into_iter()
        .flatten()
        {
            if !price.is_finite() || price < 0.0 {
                return Err("Pricing and budget must be finite and nonnegative".into());
            }
        }
        if self.budget_usd.is_some()
            && (self.input_usd_per_million.is_none()
                || self.output_usd_per_million.is_none()
                || self
                    .pricing_as_of
                    .as_ref()
                    .is_none_or(|s| s.trim().is_empty()))
        {
            return Err(
                "Budget estimates require both token prices and a pricing date/source".into(),
            );
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct Credential(String);
impl Credential {
    pub fn from_environment(provider: Provider) -> Result<Self, String> {
        let key = std::env::var(provider.environment()).map_err(|_| {
            format!(
                "{} is missing; local play remains available",
                provider.environment()
            )
        })?;
        Self::new(key)
    }
    pub fn new(key: String) -> Result<Self, String> {
        // Trim paste whitespace / BOM; reject embedded newlines. Do not log the value.
        let key = key
            .trim()
            .trim_start_matches('\u{feff}')
            .chars()
            .filter(|c| !matches!(c, '\u{200b}' | '\u{200c}' | '\u{200d}' | '\u{2060}'))
            .collect::<String>();
        if key.is_empty() || key.contains(['\r', '\n']) {
            return Err("Invalid credential".into());
        }
        Ok(Self(key))
    }
}

pub trait CredentialStore {
    fn get(&self, provider: Provider) -> Result<Option<Credential>, String>;
    fn set(&self, provider: Provider, credential: Credential) -> Result<(), String>;
    fn delete(&self, provider: Provider) -> Result<(), String>;
}

pub struct SystemKeyring;
impl SystemKeyring {
    #[cfg(not(target_os = "macos"))]
    fn entry(provider: Provider) -> Result<keyring::Entry, String> {
        keyring::Entry::new(KEYRING_SERVICE, provider.account())
            .map_err(|_| "Cannot access the operating-system credential store".into())
    }
}
impl CredentialStore for SystemKeyring {
    fn get(&self, provider: Provider) -> Result<Option<Credential>, String> {
        #[cfg(target_os = "macos")]
        {
            super::macos_credentials::get(provider)
        }
        #[cfg(not(target_os = "macos"))]
        {
            match Self::entry(provider)?.get_password() {
                Ok(value) => Credential::new(value).map(Some),
                Err(keyring::Error::NoEntry) => Ok(None),
                Err(_) => Err("Cannot read the saved coaching credential".into()),
            }
        }
    }
    fn set(&self, provider: Provider, credential: Credential) -> Result<(), String> {
        #[cfg(target_os = "macos")]
        {
            super::macos_credentials::set(provider, &credential.0)
        }
        #[cfg(not(target_os = "macos"))]
        {
            Self::entry(provider)?
                .set_password(&credential.0)
                .map_err(|_| "Cannot save the coaching credential".into())
        }
    }
    fn delete(&self, provider: Provider) -> Result<(), String> {
        #[cfg(target_os = "macos")]
        {
            super::macos_credentials::delete(provider)
        }
        #[cfg(not(target_os = "macos"))]
        {
            match Self::entry(provider)?.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
                Err(_) => Err("Cannot forget the coaching credential".into()),
            }
        }
    }
}

/// Short platform note for Settings / docs. Never mentions secret material.
pub fn credential_storage_hint() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        super::macos_credentials::storage_hint()
    }
    #[cfg(target_os = "windows")]
    {
        "Saved in Windows Credential Manager under the OpenFelt coaching service."
    }
    #[cfg(target_os = "linux")]
    {
        "Saved in the desktop Secret Service / keyring under the OpenFelt coaching service."
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        "Saved in the operating-system credential store when available."
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredentialSource {
    Cli,
    Environment,
    Keyring,
    Missing,
}

pub fn resolve_credential(
    provider: Provider,
    cli: Option<String>,
    keyring: &dyn CredentialStore,
) -> Result<(Option<Credential>, CredentialSource), String> {
    if let Some(value) = cli {
        return Credential::new(value).map(|v| (Some(v), CredentialSource::Cli));
    }
    if let Ok(value) = std::env::var(provider.environment()) {
        return Credential::new(value).map(|v| (Some(v), CredentialSource::Environment));
    }
    Ok(match keyring.get(provider)? {
        Some(value) => (Some(value), CredentialSource::Keyring),
        None => (None, CredentialSource::Missing),
    })
}

pub fn mask_credential(value: &str, reveal: bool) -> String {
    if reveal {
        return value.to_owned();
    }
    let suffix: String = value
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let prefix = value
        .split_once('-')
        .map(|(prefix, _)| format!("{prefix}-"))
        .unwrap_or_default();
    format!("{prefix}••••••••{suffix}")
}

/// Reject secrets that cannot be a provider API key before a billable request.
/// Returns a short status string; never echoes the secret.
pub fn credential_shape_error(provider: Provider, secret: &str) -> Option<&'static str> {
    match provider {
        Provider::Openai => {
            if !secret.starts_with("sk-") {
                Some("Saved secret does not look like an OpenAI API key (expected sk-…)")
            } else if secret.len() < 20 {
                Some("Saved secret is too short to be an OpenAI API key")
            } else {
                None
            }
        }
        Provider::Anthropic => {
            if !(secret.starts_with("sk-ant-") || secret.starts_with("sk-")) {
                Some("Saved secret does not look like an Anthropic API key")
            } else if secret.len() < 20 {
                Some("Saved secret is too short to be an Anthropic API key")
            } else {
                None
            }
        }
    }
}

/// Map provider HTTP failures to short, secret-free status text.
/// Only 401/403 are labeled authentication; 400/404/etc. stay as provider error.
pub fn classify_provider_http_status(status: u16, prefix: &str) -> String {
    match status {
        401 | 403 => format!("{prefix}: authentication rejected (HTTP {status})"),
        429 => format!("{prefix}: rate limit or credit exhausted (HTTP {status})"),
        300..=399 => format!("{prefix}: redirects are not allowed (HTTP {status})"),
        other => format!("{prefix}: provider error (HTTP {other})"),
    }
}

/// Append a safe `error.code` / `error.type` from a provider JSON body when present.
pub fn classify_provider_http_failure(status: u16, body: &[u8], prefix: &str) -> String {
    let mut message = classify_provider_http_status(status, prefix);
    if let Some(code) = safe_provider_error_code(body) {
        message.push_str(" [");
        message.push_str(&code);
        message.push(']');
    }
    message
}

fn safe_provider_error_code(body: &[u8]) -> Option<String> {
    let value: Value = serde_json::from_slice(body).ok()?;
    let code = value
        .pointer("/error/code")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .or_else(|| {
            value
                .pointer("/error/type")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
        })?;
    if code.len() <= 64
        && code
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        Some(code.to_string())
    } else {
        None
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Usage {
    pub requests: u32,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub reserved_estimate_usd: f64,
}
impl Usage {
    pub fn reserve(
        &mut self,
        settings: &ProviderSettings,
        input_bytes: usize,
    ) -> Result<(), String> {
        if self.requests >= settings.max_requests {
            return Err("Session request limit reached; Enter continues locally".into());
        }
        let reserve = match (
            settings.input_usd_per_million,
            settings.output_usd_per_million,
        ) {
            (Some(i), Some(o)) => {
                ((input_bytes + 4096) as f64 * i + f64::from(settings.max_output_tokens) * o)
                    / 1_000_000.0
            }
            _ => 0.0,
        };
        if settings
            .budget_usd
            .is_some_and(|budget| self.reserved_estimate_usd + reserve > budget)
        {
            return Err(
                "Estimated session budget safeguard reached; Enter continues locally".into(),
            );
        }
        self.requests += 1;
        self.reserved_estimate_usd += reserve;
        Ok(())
    }
}
pub struct ProviderResult {
    pub feedback: Feedback,
    pub input_tokens: u64,
    pub output_tokens: u64,
}
pub struct Pending {
    receiver: mpsc::Receiver<Result<ProviderResult, String>>,
    cancel: Option<tokio::sync::oneshot::Sender<()>>,
}
pub struct CredentialTest {
    receiver: mpsc::Receiver<Result<(), String>>,
    cancel: Option<tokio::sync::oneshot::Sender<()>>,
}
impl CredentialTest {
    pub fn poll(&self) -> Option<Result<(), String>> {
        self.receiver.try_recv().ok()
    }
}
impl Drop for CredentialTest {
    fn drop(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            let _ = cancel.send(());
        }
    }
}
pub fn test_credential(
    provider: Provider,
    settings: ProviderSettings,
    key: Credential,
    usage: &mut Usage,
) -> Result<CredentialTest, String> {
    settings.validate()?;
    if settings.model.is_empty() {
        return Err("Choose a model before testing the key".into());
    }
    if let Some(issue) = credential_shape_error(provider, &key.0) {
        return Err(issue.into());
    }
    let body = match provider {
        Provider::Openai => {
            json!({"model":settings.model,"store":false,"max_output_tokens":16,"input":"Reply with OK."})
        }
        Provider::Anthropic => {
            json!({"model":settings.model,"max_tokens":8,"messages":[{"role":"user","content":"Reply with OK."}]})
        }
    };
    let request_bytes = serde_json::to_vec(&body).map_err(|_| "Cannot encode key test")?;
    usage.reserve(&settings, request_bytes.len())?;
    let (sender, receiver) = mpsc::channel();
    let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel();
    std::thread::spawn(move || {
        let result = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
            Ok(runtime) => runtime.block_on(async { tokio::select! {
                _ = cancel_rx => Err("Key test cancelled".into()),
                result = async {
                    let client = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none())
                        .no_proxy().timeout(Duration::from_secs(settings.timeout_seconds)).build()
                        .map_err(|_| "Cannot initialize coaching connection")?;
                    let request = client.post(provider.endpoint()).json(&body);
                    let request = match provider {
                        Provider::Openai => request.bearer_auth(&key.0),
                        Provider::Anthropic => request.header("x-api-key", &key.0).header("anthropic-version", "2023-06-01"),
                    };
                    let response = request.send().await.map_err(|_| "Key test failed: connection failed or timed out")?;
                    let status = response.status().as_u16();
                    if response.status().is_success() {
                        Ok(())
                    } else {
                        let bytes = response.bytes().await.unwrap_or_default();
                        Err(classify_provider_http_failure(status, &bytes, "Key test failed"))
                    }
                } => result,
            }}),
            Err(_) => Err("Cannot start key test".into()),
        };
        let _ = sender.send(result);
    });
    Ok(CredentialTest {
        receiver,
        cancel: Some(cancel_tx),
    })
}
impl Pending {
    pub fn poll(&self) -> Option<Result<ProviderResult, String>> {
        match self.receiver.try_recv() {
            Ok(v) => Some(v),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(_) => Some(Err("Coaching worker stopped".into())),
        }
    }
}
impl Drop for Pending {
    fn drop(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            let _ = cancel.send(());
        }
    }
}

pub fn start(
    d: Decision,
    provider: Provider,
    settings: ProviderSettings,
    key: Credential,
    usage: &mut Usage,
) -> Result<Pending, String> {
    settings.validate()?;
    if settings.model.is_empty() {
        return Err("Choose a model before enabling cloud coaching".into());
    }
    if let Some(issue) = credential_shape_error(provider, &key.0) {
        return Err(issue.into());
    }
    let body = request_body_for(provider, &d, &settings);
    let bytes = serde_json::to_vec(&body).map_err(|_| "Cannot encode coaching request")?;
    if bytes.len() > 64_000 {
        return Err("Coaching request exceeds the local size limit".into());
    }
    usage.reserve(&settings, bytes.len())?;
    let (sender, receiver) = mpsc::channel();
    let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel();
    std::thread::spawn(move || {
        let result=match tokio::runtime::Builder::new_current_thread().enable_all().build() {
            Ok(runtime)=>runtime.block_on(async {tokio::select! {
                _=cancel_rx=>Err("Coaching cancelled".into()),
                result=send_request(provider,provider.endpoint(),&d,&settings,&key,body)=>result,
            }}),Err(_)=>Err("Cannot start coaching worker".into())
        };
        let _ = sender.send(result);
    });
    Ok(Pending {
        receiver,
        cancel: Some(cancel_tx),
    })
}

pub fn request_body(d: &Decision, s: &ProviderSettings) -> Value {
    request_body_for(Provider::Openai, d, s)
}
pub fn request_body_for(provider: Provider, d: &Decision, s: &ProviderSettings) -> Value {
    let schema = json!({"type":"object","additionalProperties":false,"properties":{
        "version":{"type":"integer","enum":[1]},"hand_id":{"type":"integer"},"revision":{"type":"integer"},
        "assessment":{"type":"string","enum":["uncertain","reasonable","reconsider"]},"explanation":{"type":"string"},"concept":{"type":"string"},
        "assumptions":{"type":"array","items":{"type":"string"}},"evidence_basis":{"type":"string","enum":["heuristic"]},"alternative_action":{"type":["string","null"],"enum":["fold","check_call","raise",null]}},
        "required":["version","hand_id","revision","assessment","explanation","concept","assumptions","evidence_basis","alternative_action"]});
    let instruction = "You teach a beginner local play-money Hold'em after an accepted decision. Use only this pre-decision observation. Teach one concept briefly. Copy hand_id/revision from observation. Label all strategic advice heuristic. Several choices may be reasonable: use uncertain when information is insufficient. Do not infer hidden cards or future outcomes. Do not state numeric equity, EV, odds, frequencies, amounts, percentages, or solver/GTO optimality: verified numbers are displayed separately by the app. Do not use digits in explanation, concept, assumptions or alternative_action. No tools. Return the required JSON only.";
    match provider {
        Provider::Openai => {
            json!({"model":s.model,"store":false,"max_output_tokens":s.max_output_tokens,
        "instructions":instruction,
        "input":serde_json::to_string(d).expect("serializable decision"),"text":{"format":{"type":"json_schema","name":"openfelt_feedback","strict":true,"schema":schema}}})
        }
        Provider::Anthropic => json!({"model":s.model,"max_tokens":s.max_output_tokens,
        "system":instruction,"messages":[{"role":"user","content":serde_json::to_string(d).expect("serializable decision")}],
        "output_config":{"format":{"type":"json_schema","schema":schema}}}),
    }
}

async fn send_request(
    provider: Provider,
    endpoint: &str,
    d: &Decision,
    s: &ProviderSettings,
    key: &Credential,
    body: Value,
) -> Result<ProviderResult, String> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .timeout(Duration::from_secs(s.timeout_seconds))
        .build()
        .map_err(|_| "Cannot initialize coaching connection")?;
    let request = client.post(endpoint).json(&body);
    let request = match provider {
        Provider::Openai => request.bearer_auth(&key.0),
        Provider::Anthropic => request
            .header("x-api-key", &key.0)
            .header("anthropic-version", "2023-06-01"),
    };
    let response = request
        .send()
        .await
        .map_err(|_| "Coaching unavailable: connection failed or timed out")?;
    let status = response.status();
    if !status.is_success() {
        let bytes = response.bytes().await.unwrap_or_default();
        return Err(classify_provider_http_failure(
            status.as_u16(),
            &bytes,
            "Coaching unavailable",
        ));
    }
    let mut response = response;
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "Coaching unavailable: incomplete response")?
    {
        if bytes.len() + chunk.len() > 64_000 {
            return Err("Coaching response exceeds size limit".into());
        }
        bytes.extend(chunk);
    }
    parse_response_for(provider, &bytes, d, &key.0)
}
pub fn parse_response(bytes: &[u8], d: &Decision, secret: &str) -> Result<ProviderResult, String> {
    parse_response_for(Provider::Openai, bytes, d, secret)
}
pub fn parse_response_for(
    provider: Provider,
    bytes: &[u8],
    d: &Decision,
    secret: &str,
) -> Result<ProviderResult, String> {
    if bytes.len() > 64_000 {
        return Err("Coaching response exceeds size limit".into());
    }
    if !secret.is_empty() && String::from_utf8_lossy(bytes).contains(secret) {
        return Err("Coaching response rejected".into());
    }
    let value: Value =
        serde_json::from_slice(bytes).map_err(|_| "Coaching unavailable: invalid response")?;
    if provider == Provider::Openai && value["status"] != "completed" {
        return Err("Coaching unavailable: incomplete or refused response".into());
    }
    let text = if provider == Provider::Openai {
        value["output"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|v| v["type"] == "message")
            .flat_map(|m| m["content"].as_array().into_iter().flatten())
            .filter(|v| v["type"] == "output_text")
            .filter_map(|v| v["text"].as_str())
            .collect::<Vec<_>>()
            .join("")
    } else {
        value["content"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|v| v["type"] == "text")
            .filter_map(|v| v["text"].as_str())
            .collect::<Vec<_>>()
            .join("")
    };
    let object: Value = serde_json::from_str(&text)
        .map_err(|_| "Coaching unavailable: response schema mismatch")?;
    if !object
        .as_object()
        .is_some_and(|o| o.contains_key("alternative_action"))
    {
        return Err("Coaching unavailable: missing required nullable field".into());
    }
    let f: Feedback = serde_json::from_str(&text)
        .map_err(|_| "Coaching unavailable: response schema mismatch")?;
    if !secret.is_empty()
        && [&f.assessment, &f.explanation, &f.concept, &f.evidence_basis]
            .into_iter()
            .chain(f.assumptions.iter())
            .chain(f.alternative_action.iter())
            .any(|text| text.contains(secret))
    {
        return Err("Coaching response rejected".into());
    }
    validate_feedback(&f, d)?;
    Ok(ProviderResult {
        feedback: f,
        input_tokens: value["usage"]["input_tokens"].as_u64().unwrap_or(0),
        output_tokens: value["usage"]["output_tokens"].as_u64().unwrap_or(0),
    })
}
pub fn validate_feedback(f: &Feedback, d: &Decision) -> Result<(), String> {
    if f.version != 1 || f.hand_id != d.observation.hand_id || f.revision != d.observation.revision
    {
        return Err("Coaching unavailable: stale response".into());
    }
    if !["uncertain", "reasonable", "reconsider"].contains(&f.assessment.as_str())
        || f.evidence_basis != "heuristic"
        || f.assumptions.len() > 8
        || f.explanation.is_empty()
        || f.concept.is_empty()
    {
        return Err("Coaching unavailable: invalid assessment".into());
    }
    for text in std::iter::once(&f.explanation)
        .chain(std::iter::once(&f.concept))
        .chain(f.assumptions.iter())
        .chain(f.alternative_action.iter())
    {
        let lower = text.to_lowercase();
        if text.len() > 1200
            || text.chars().any(|c| c.is_control() || c.is_ascii_digit())
            || ["gto", "solver", "optimal", "%", "equity", "expected value"]
                .iter()
                .any(|w| lower.contains(w))
        {
            return Err("Coaching unavailable: unsupported claim or unsafe text".into());
        }
    }
    if let Some(alternative) = f.alternative_action.as_deref() {
        let legal = &d.observation.legal;
        let valid = match alternative {
            "fold" => legal.can_fold,
            "check_call" => true,
            "raise" => legal.min_raise_to.is_some() || legal.min_bet_to.is_some(),
            _ => false,
        };
        if !valid {
            return Err("Coaching unavailable: invalid alternative".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
    };
    fn decision() -> Decision {
        let mut s = crate::trainer::Session::new(Default::default()).unwrap();
        while s.view().to_act != Some(crate::trainer::hero()) {
            s.step_bot().unwrap();
        }
        let action = s.observation(crate::trainer::hero()).unwrap().check_call();
        s.submit(action).unwrap().clone()
    }
    #[derive(Default)]
    struct MemoryStore(std::sync::Mutex<Option<String>>);
    impl CredentialStore for MemoryStore {
        fn get(&self, _: Provider) -> Result<Option<Credential>, String> {
            self.0
                .lock()
                .unwrap()
                .clone()
                .map(Credential::new)
                .transpose()
        }
        fn set(&self, _: Provider, credential: Credential) -> Result<(), String> {
            *self.0.lock().unwrap() = Some(credential.0);
            Ok(())
        }
        fn delete(&self, _: Provider) -> Result<(), String> {
            *self.0.lock().unwrap() = None;
            Ok(())
        }
    }
    #[test]
    fn credential_mask_models_and_store_contract() {
        assert_eq!(mask_credential("sk-exampleabcd", false), "sk-••••••••abcd");
        assert_eq!(mask_credential("sk-exampleabcd", true), "sk-exampleabcd");
        assert_eq!(
            Credential::new("  sk-trimmed-key  ".into()).unwrap().0,
            "sk-trimmed-key"
        );
        assert_eq!(
            Credential::new("\nsk-edge-trim\n".into()).unwrap().0,
            "sk-edge-trim"
        );
        assert!(Credential::new("sk-bad\nmiddle".into()).is_err());
        assert!(Provider::Openai.models().contains(&"gpt-5"));
        let auth = classify_provider_http_status(401, "Coaching unavailable");
        assert!(auth.contains("authentication rejected"));
        assert!(auth.contains("HTTP 401"));
        let with_code = classify_provider_http_failure(
            401,
            br#"{"error":{"message":"bad","type":"invalid_request_error","code":"invalid_api_key"}}"#,
            "Key test failed",
        );
        assert!(with_code.contains("invalid_api_key"));
        assert!(!with_code.contains("sk-"));
        let bad_model = classify_provider_http_status(400, "Coaching unavailable");
        assert!(bad_model.contains("provider error"));
        assert!(!bad_model.contains("authentication"));
        let missing = classify_provider_http_status(404, "Coaching unavailable");
        assert!(missing.contains("provider error"));
        assert!(!missing.contains("authentication"));
        assert!(credential_shape_error(Provider::Openai, "not-a-key").is_some());
        assert!(credential_shape_error(Provider::Openai, "sk-short").is_some());
        assert!(
            credential_shape_error(Provider::Openai, &format!("sk-{}", "a".repeat(40))).is_none()
        );
        assert!(
            credential_shape_error(Provider::Anthropic, "sk-ant-abcdefghijklmnopqrstuvwxyz")
                .is_none()
        );
        assert!(Provider::Anthropic
            .models()
            .contains(&"claude-haiku-4-5-20251001"));
        assert!(!credential_storage_hint().is_empty());
        let store = MemoryStore::default();
        store
            .set(
                Provider::Openai,
                Credential::new("stored-secret".into()).unwrap(),
            )
            .unwrap();
        let (value, source) =
            resolve_credential(Provider::Openai, Some("cli-secret".into()), &store).unwrap();
        assert_eq!(source, CredentialSource::Cli);
        assert_eq!(value.unwrap().0, "cli-secret");
        assert_eq!(
            store.get(Provider::Openai).unwrap().unwrap().0,
            "stored-secret"
        );
        store.delete(Provider::Openai).unwrap();
        assert!(store.get(Provider::Openai).unwrap().is_none());
        // Missing path: only assert when the provider env var is unset so
        // precedence stays env → store → none without mutating the process env.
        if std::env::var_os(Provider::Openai.environment()).is_none() {
            let (missing, source) = resolve_credential(Provider::Openai, None, &store).unwrap();
            assert!(missing.is_none());
            assert_eq!(source, CredentialSource::Missing);
        }
    }
    #[test]
    fn anthropic_body_and_response_keep_the_same_feedback_boundary() {
        let d = decision();
        let f = crate::trainer::facts::local_feedback(&d);
        let settings = ProviderSettings {
            model: "fixture-model".into(),
            ..Default::default()
        };
        let body = request_body_for(Provider::Anthropic, &d, &settings);
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["output_config"]["format"]["type"], "json_schema");
        let response = json!({"content":[{"type":"text","text":serde_json::to_string(&f).unwrap()}],"usage":{"input_tokens":7,"output_tokens":9}});
        let result = parse_response_for(
            Provider::Anthropic,
            &serde_json::to_vec(&response).unwrap(),
            &d,
            "fixture-key",
        )
        .unwrap();
        assert_eq!((result.input_tokens, result.output_tokens), (7, 9));
    }
    fn server(
        status: u16,
        body: String,
        delay: Duration,
    ) -> (String, mpsc::Receiver<Vec<u8>>, std::thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = format!("http://{}/v1/responses", listener.local_addr().unwrap());
        let (tx, rx) = mpsc::channel();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut bytes = Vec::new();
            let mut buf = [0; 4096];
            loop {
                let n = stream.read(&mut buf).unwrap();
                if n == 0 {
                    break;
                }
                bytes.extend_from_slice(&buf[..n]);
                if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&bytes[..end]).to_lowercase();
                    let length = headers
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length: "))
                        .and_then(|v| v.parse::<usize>().ok())
                        .unwrap_or(0);
                    if bytes.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            let _ = tx.send(bytes);
            std::thread::sleep(delay);
            let response=format!("HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nLocation: http://127.0.0.1:1/stolen\r\nConnection: close\r\n\r\n{body}",body.len());
            let _ = stream.write_all(response.as_bytes());
        });
        (address, rx, handle)
    }
    #[test]
    fn mock_http_contract_auth_body_errors_and_redirects() {
        let d = decision();
        let f = crate::trainer::facts::local_feedback(&d);
        let good=json!({"status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":serde_json::to_string(&f).unwrap()}]}],"usage":{"input_tokens":11,"output_tokens":22}}).to_string();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        for (status, body) in [
            (200, good),
            (401, "fake-canary-key".into()),
            (429, "rate limited".into()),
            (302, "redirect".into()),
            (200, "malformed".into()),
        ] {
            let (endpoint, requests, thread) = server(status, body.clone(), Duration::ZERO);
            let settings = ProviderSettings {
                model: "fixture-model".into(),
                ..Default::default()
            };
            let key = Credential::new("fake-canary-key".into()).unwrap();
            let result = runtime.block_on(send_request(
                Provider::Openai,
                &endpoint,
                &d,
                &settings,
                &key,
                request_body(&d, &settings),
            ));
            let raw = requests.recv().unwrap();
            let text = String::from_utf8(raw).unwrap();
            let (headers, body_text) = text.split_once("\r\n\r\n").unwrap();
            assert!(headers
                .to_lowercase()
                .contains("authorization: bearer fake-canary-key"));
            assert!(!body_text.contains("fake-canary-key"));
            if status == 200 && body != "malformed" {
                let result = result.ok().unwrap();
                assert_eq!(result.input_tokens, 11);
                assert_eq!(result.output_tokens, 22);
            } else {
                assert!(!result.err().unwrap().contains("fake-canary-key"));
            }
            thread.join().unwrap();
        }
    }
    #[test]
    fn mock_timeout_and_dropping_pending_cancel_without_retries() {
        let d = decision();
        let (endpoint, _, thread) = server(200, "{}".into(), Duration::from_millis(1200));
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let settings = ProviderSettings {
            timeout_seconds: 1,
            ..Default::default()
        };
        let key = Credential::new("fixture-key".into()).unwrap();
        let start = std::time::Instant::now();
        assert!(runtime
            .block_on(send_request(
                Provider::Openai,
                &endpoint,
                &d,
                &settings,
                &key,
                request_body(&d, &settings)
            ))
            .is_err());
        assert!(start.elapsed() < Duration::from_secs(2));
        thread.join().unwrap();
        let (tx, rx) = tokio::sync::oneshot::channel();
        let (_, receiver) = mpsc::channel();
        let pending = Pending {
            receiver,
            cancel: Some(tx),
        };
        drop(pending);
        assert!(runtime.block_on(rx).is_ok());
    }
}
