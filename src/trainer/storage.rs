//! Private local settings and learning history; never accepts credential fields.
use super::{
    facts::{Decision, Feedback},
    policy::PolicySettings,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub seats: u8,
    pub small_blind: u32,
    pub big_blind: u32,
    pub opponents: PolicySettings,
    pub coaching: CoachingMode,
    /// Opt-in local solver feedback for supported heads-up river decisions.
    pub solver_feedback: bool,
    pub cloud: super::provider::ProviderSettings,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum, Default)]
#[serde(rename_all = "snake_case")]
pub enum CoachingMode {
    #[default]
    Local,
    Off,
    Openai,
    Anthropic,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            seats: 6,
            small_blind: 1,
            big_blind: 2,
            opponents: PolicySettings::default(),
            coaching: CoachingMode::Local,
            solver_feedback: false,
            cloud: super::provider::ProviderSettings::default(),
        }
    }
}
impl Settings {
    pub fn validate(&self) -> Result<(), String> {
        if !(2..=9).contains(&self.seats)
            || self.small_blind == 0
            || self.small_blind >= self.big_blind
            || self.big_blind > 10_000
        {
            return Err("Use 2–9 seats and blinds 0 < small < big <= 10000".into());
        }
        for rate in [
            self.opponents.aggression,
            self.opponents.bluff_rate,
            self.opponents.mistake_rate,
        ] {
            if !rate.is_finite() || !(0.0..=1.0).contains(&rate) {
                return Err("Policy probabilities must be between 0 and 1".into());
            }
        }
        self.cloud.validate()?;
        Ok(())
    }
}
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Progress {
    pub hands: u64,
    pub decisions: u64,
    pub profit_chips: i64,
    pub concepts: BTreeMap<String, u64>,
    /// Drill outcomes are distinct from passive concept exposure during hands.
    pub drills: BTreeMap<String, DrillProgress>,
    /// Local heuristic assessments. Exposure alone is not treated as a weakness.
    pub review_patterns: BTreeMap<String, ReviewPattern>,
}
impl Progress {
    pub fn note_review(&mut self, concept: &str, assessment: &str) {
        if concept.is_empty() {
            return;
        }
        let entry = self.review_patterns.entry(concept.to_string()).or_default();
        entry.seen += 1;
        if assessment == "reconsider" {
            entry.reconsider += 1;
        }
    }
}
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DrillProgress {
    pub attempts: u64,
    pub correct: u64,
    pub completed_sets: u64,
}
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ReviewPattern {
    pub seen: u64,
    pub reconsider: u64,
}
pub struct Store {
    pub root: PathBuf,
}
impl Store {
    pub fn default_location() -> Result<Self, String> {
        Ok(Self {
            root: dirs::data_local_dir()
                .ok_or("No local data directory")?
                .join("openfelt"),
        })
    }
    pub fn settings(&self) -> Result<Settings, String> {
        self.load("settings.json")
    }
    pub fn has_settings(&self) -> bool {
        self.root.join("settings.json").is_file()
    }
    pub fn progress(&self) -> Result<Progress, String> {
        self.load("progress.json")
    }
    fn load<T: serde::de::DeserializeOwned + Default>(&self, name: &str) -> Result<T, String> {
        match fs::read(self.root.join(name)) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|_| format!("Invalid {name}; original preserved")),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
            Err(_) => Err(format!("Cannot read {name}")),
        }
    }
    pub fn save<T: Serialize>(&self, name: &str, value: &T) -> Result<(), String> {
        self.prepare()?;
        let tmp = self.root.join(format!("{name}.{}.tmp", std::process::id()));
        let bytes = serde_json::to_vec_pretty(value).map_err(|_| "Cannot serialize local state")?;
        let mut file = private_file(&tmp, false)?;
        file.write_all(&bytes)
            .map_err(|_| "Cannot write local state")?;
        file.sync_all().map_err(|_| "Cannot sync local state")?;
        fs::rename(tmp, self.root.join(name)).map_err(|_| "Cannot save local state".into())
    }
    pub fn decision(&self, d: &Decision, f: &Feedback) -> Result<(), String> {
        self.append(
            "decisions.jsonl",
            &serde_json::json!({"decision":d,"feedback":f}),
        )
    }
    pub fn append<T: Serialize>(&self, name: &str, value: &T) -> Result<(), String> {
        self.prepare()?;
        let mut file = private_file(&self.root.join(name), true)?;
        let mut bytes = serde_json::to_vec(value).map_err(|_| "Cannot serialize history")?;
        bytes.push(b'\n');
        file.write_all(&bytes)
            .map_err(|_| "Cannot write history".into())
    }
    fn prepare(&self) -> Result<(), String> {
        fs::create_dir_all(&self.root).map_err(|_| "Cannot create OpenFelt data directory")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&self.root, fs::Permissions::from_mode(0o700))
                .map_err(|_| "Cannot protect data directory")?;
        }
        Ok(())
    }
}
fn private_file(path: &Path, append: bool) -> Result<fs::File, String> {
    let mut options = OpenOptions::new();
    options
        .create(true)
        .write(true)
        .append(append)
        .truncate(!append);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(path)
        .map_err(|_| "Cannot open private state file".into())
}

#[cfg(test)]
mod tests {
    use super::Settings;

    #[test]
    fn solver_feedback_defaults_off_for_existing_settings_and_roundtrips() {
        let old = serde_json::json!({"seats": 2, "small_blind": 1, "big_blind": 2});
        let mut settings: Settings = serde_json::from_value(old).unwrap();
        assert!(!settings.solver_feedback);
        settings.solver_feedback = true;
        let saved = serde_json::to_string(&settings).unwrap();
        assert!(
            serde_json::from_str::<Settings>(&saved)
                .unwrap()
                .solver_feedback
        );
    }
}
