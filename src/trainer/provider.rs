//! Optional direct OpenAI Responses adapter. No credentials in Debug, settings, prompts or errors.
use super::facts::{Decision, Feedback};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{sync::mpsc, time::Duration};

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

pub struct Credential(String);
impl Credential {
    pub fn from_environment() -> Result<Self, String> {
        let key = std::env::var("OPENAI_API_KEY")
            .map_err(|_| "OPENAI_API_KEY is missing; local play remains available")?;
        Self::new(key)
    }
    pub fn new(key: String) -> Result<Self, String> {
        if key.trim().is_empty() || key.contains(['\r', '\n']) {
            return Err("Invalid credential".into());
        }
        Ok(Self(key))
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
    settings: ProviderSettings,
    key: Credential,
    usage: &mut Usage,
) -> Result<Pending, String> {
    settings.validate()?;
    if settings.model.is_empty() {
        return Err("Choose an OpenAI model before enabling cloud coaching".into());
    }
    let body = request_body(&d, &settings);
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
                result=send_request("https://api.openai.com/v1/responses",&d,&settings,&key,body)=>result,
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
    let schema = json!({"type":"object","additionalProperties":false,"properties":{
        "version":{"type":"integer","enum":[1]},"hand_id":{"type":"integer"},"revision":{"type":"integer"},
        "assessment":{"type":"string","enum":["uncertain","reasonable","reconsider"]},"explanation":{"type":"string"},"concept":{"type":"string"},
        "assumptions":{"type":"array","items":{"type":"string"}},"evidence_basis":{"type":"string","enum":["heuristic"]},"alternative_action":{"type":["string","null"],"enum":["fold","check_call","raise",null]}},
        "required":["version","hand_id","revision","assessment","explanation","concept","assumptions","evidence_basis","alternative_action"]});
    json!({"model":s.model,"store":false,"max_output_tokens":s.max_output_tokens,
        "instructions":"You teach a beginner local play-money Hold'em after an accepted decision. Use only this pre-decision observation. Teach one concept briefly. Copy hand_id/revision from observation. Label all strategic advice heuristic. Several choices may be reasonable: use uncertain when information is insufficient. Do not infer hidden cards or future outcomes. Do not state numeric equity, EV, odds, frequencies, amounts, percentages, or solver/GTO optimality: verified numbers are displayed separately by the app. Do not use digits in explanation, concept, assumptions or alternative_action. No tools. Return the required JSON only.",
        "input":serde_json::to_string(d).expect("serializable decision"),"text":{"format":{"type":"json_schema","name":"openfelt_feedback","strict":true,"schema":schema}}})
}

async fn send_request(
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
    let response = client
        .post(endpoint)
        .bearer_auth(&key.0)
        .json(&body)
        .send()
        .await
        .map_err(|_| "Coaching unavailable: connection failed or timed out")?;
    let status = response.status();
    if !status.is_success() {
        return Err(match status.as_u16() {
            401 | 403 => "Coaching unavailable: authentication rejected",
            429 => "Coaching unavailable: rate limit or credit exhausted",
            300..=399 => "Coaching unavailable: redirects are not allowed",
            _ => "Coaching unavailable: provider error",
        }
        .into());
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
    parse_response(&bytes, d, &key.0)
}
pub fn parse_response(bytes: &[u8], d: &Decision, secret: &str) -> Result<ProviderResult, String> {
    if bytes.len() > 64_000 {
        return Err("Coaching response exceeds size limit".into());
    }
    if !secret.is_empty() && String::from_utf8_lossy(bytes).contains(secret) {
        return Err("Coaching response rejected".into());
    }
    let value: Value =
        serde_json::from_slice(bytes).map_err(|_| "Coaching unavailable: invalid response")?;
    if value["status"] != "completed" {
        return Err("Coaching unavailable: incomplete or refused response".into());
    }
    let text = value["output"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|v| v["type"] == "message")
        .flat_map(|m| m["content"].as_array().into_iter().flatten())
        .filter(|v| v["type"] == "output_text")
        .filter_map(|v| v["text"].as_str())
        .collect::<Vec<_>>()
        .join("");
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
