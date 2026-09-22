//! Small reviewed offline drills. Answers are intentionally categorical and explain ambiguity.
use super::storage::{DrillProgress, Progress};
use rand::{rngs::StdRng, seq::SliceRandom, SeedableRng};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, Write};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, clap::ValueEnum,
)]
#[serde(rename_all = "snake_case")]
pub enum DrillTopic {
    StartingHands,
    Position,
    CallingPrices,
    ValueBetting,
}
impl DrillTopic {
    pub fn title(self) -> &'static str {
        match self {
            Self::StartingHands => "starting hands",
            Self::Position => "position",
            Self::CallingPrices => "calling prices",
            Self::ValueBetting => "value betting",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Question {
    pub prompt: &'static str,
    pub choices: &'static [&'static str],
    pub accepted: &'static [usize],
    pub explanation: &'static str,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    pub accepted: bool,
    pub explanation: &'static str,
}
pub fn grade(q: &Question, choice: usize) -> Answer {
    Answer {
        accepted: q.accepted.contains(&choice),
        explanation: q.explanation,
    }
}

pub fn questions(topic: DrillTopic) -> Vec<Question> {
    match topic {
 DrillTopic::StartingHands=>vec![
  q("Six-handed, first to enter from early position with 7♣ 2♦. Baseline?", &["Fold","Call","Raise"], &[0], "This reviewed starting band folds weak, disconnected offsuit hands early."),
  q("Six-handed, unopened button with A♠ 8♠. Baseline?", &["Fold","Raise","Only call"], &[1], "A suited ace clears the trainer's late-position opening threshold."),
  q("Facing a raise with A♣ A♦ at deep stacks. Reasonable choices?", &["Fold","Call","Re-raise"], &[1,2], "Calling and re-raising can both be defensible. The exercise does not prescribe a solver frequency."),
  q("Facing a raise with K♣ 4♦ and no read. Baseline?", &["Continue","Fold","Always bluff re-raise"], &[1], "Weak offsuit kings are often dominated and sit outside this trainer's continue band."),],
 DrillTopic::Position=>vec![
  q("Why can the button usually open more hands than early position?", &["It acts later with fewer players remaining","Its cards are stronger","It pays no blinds"], &[0], "Position changes available information and how many players remain, not card strength."),
  q("Who normally acts first after the flop?", &["Button","First seat left of the button that can act","Last preflop raiser always"], &[1], "Postflop action begins left of the button and skips folded or all-in seats that cannot act; this is often, but not always, a blind."),
  q("Same marginal hand, unopened pot: where is a raise more defensible?", &["Early position","Button","Position never matters"], &[1], "Fewer players remain behind on the button."),
  q("What does position guarantee?", &["Profit","More information when acting later","Winning at showdown"], &[1], "Position supplies information; it does not guarantee an outcome."),],
 DrillTopic::CallingPrices=>vec![
  q("Pot is 20 before a 5-chip call. Contestable pot after calling?", &["20","25","30"], &[1], "The after-call pot includes the five chips you add: 25. This is arithmetic, not a claim the call is profitable."),
  q("You have 3 chips left facing 10. What is the legal call cost?", &["10","3","0"], &[1], "A call is capped by the remaining stack."),
  q("Does a cheap call automatically make a weak hand profitable?", &["Yes","No","Only on the river"], &[1], "Price is one input. Opponent ranges, future action, and the chance of winning still matter."),
  q("An opponent's unmatched excess is included in your contestable pot?", &["Always","Only if you can match it","Never any opponent chips"], &[1], "You can win only matched contribution layers for which you are eligible."),],
 DrillTopic::ValueBetting=>vec![
  q("A value bet primarily expects what?", &["Worse hands to call","Better hands to fold","Everyone to fold"], &[0], "Value comes from plausible worse hands continuing."),
  q("A bluff primarily expects what?", &["Worse hands to call","Better hands to fold","A guaranteed win"], &[1], "A bluff targets folds from hands that currently beat yours; success is never guaranteed."),
  q("You cannot name a worse hand that calls or a better hand that folds. Best review?", &["Bet automatically","Clarify the purpose before betting","Always all-in"], &[1], "A clear value or bluff target makes the betting plan reviewable."),
  q("Can checking a strong hand ever be defensible?", &["Yes, depending on ranges and action","Never","Only if all-in"], &[0], "Multiple lines can be defensible; this drill avoids declaring one universal action."),],
}
}
fn q(
    prompt: &'static str,
    choices: &'static [&'static str],
    accepted: &'static [usize],
    explanation: &'static str,
) -> Question {
    Question {
        prompt,
        choices,
        accepted,
        explanation,
    }
}

pub fn selected(topic: DrillTopic, seed: u64, count: usize) -> Vec<Question> {
    let mut all = questions(topic);
    all.shuffle(&mut StdRng::seed_from_u64(seed));
    all.truncate(count.min(all.len()));
    all
}
pub fn record(progress: &mut Progress, topic: DrillTopic, answers: &[bool]) {
    let p = progress
        .drills
        .entry(format!("{:?}", topic).to_ascii_lowercase())
        .or_default();
    p.attempts += answers.len() as u64;
    p.correct += answers.iter().filter(|a| **a).count() as u64;
    p.completed_sets += 1;
}
pub fn recommendation(progress: &Progress) -> Option<String> {
    progress.drills.iter().filter(|(_,p)|p.attempts>=6).min_by_key(|(_,p)|p.correct*100/p.attempts.max(1)).and_then(|(topic,p)| { let pct=p.correct*100/p.attempts.max(1); (pct<75).then(||format!("Practice {topic}: {}/{} reviewed answers were accepted across {} completed sets.",p.correct,p.attempts,p.completed_sets)) })
}

pub fn run<R: BufRead, W: Write>(
    topic: DrillTopic,
    seed: u64,
    input: &mut R,
    out: &mut W,
    progress: &mut Progress,
) -> Result<(), String> {
    writeln!(
        out,
        "OpenFelt drill: {} (local reviewed heuristics; no solver claims)",
        topic.title()
    )
    .map_err(|_| "Cannot write drill")?;
    let set = selected(topic, seed, 3);
    let mut results = Vec::new();
    for (i, q) in set.iter().enumerate() {
        writeln!(out, "\n{}. {}", i + 1, q.prompt).map_err(|_| "Cannot write drill")?;
        for (j, c) in q.choices.iter().enumerate() {
            writeln!(out, "  {}. {}", j + 1, c).map_err(|_| "Cannot write drill")?;
        }
        write!(out, "> ").map_err(|_| "Cannot write drill")?;
        out.flush().map_err(|_| "Cannot write drill")?;
        let mut line = String::new();
        let bytes = input
            .read_line(&mut line)
            .map_err(|_| "Cannot read drill answer")?;
        if bytes == 0 || line.trim().eq_ignore_ascii_case("q") {
            return Err("Drill ended before completion; progress was not recorded".into());
        }
        let choice = line
            .trim()
            .parse::<usize>()
            .ok()
            .and_then(|n| n.checked_sub(1));
        let answer = choice.map(|c| grade(q, c)).unwrap_or(Answer {
            accepted: false,
            explanation: q.explanation,
        });
        results.push(answer.accepted);
        writeln!(
            out,
            "{} — {}",
            if answer.accepted {
                "Accepted"
            } else {
                "Review"
            },
            answer.explanation
        )
        .map_err(|_| "Cannot write drill")?;
    }
    record(progress, topic, &results);
    writeln!(
        out,
        "\nSet complete: {}/{} accepted. Multiple defensible answers are accepted where listed.",
        results.iter().filter(|v| **v).count(),
        results.len()
    )
    .map_err(|_| "Cannot write drill")?;
    Ok(())
}

#[allow(dead_code)]
fn _keeps_type_documented(_: DrillProgress) {}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_topic_has_variants_and_only_declared_answers_score() {
        for topic in [
            DrillTopic::StartingHands,
            DrillTopic::Position,
            DrillTopic::CallingPrices,
            DrillTopic::ValueBetting,
        ] {
            let qs = questions(topic);
            assert!(qs.len() >= 4);
            for q in qs {
                assert!(!q.accepted.is_empty());
                for i in 0..q.choices.len() {
                    assert_eq!(grade(&q, i).accepted, q.accepted.contains(&i));
                }
            }
        }
        assert_ne!(
            selected(DrillTopic::Position, 1, 3)
                .iter()
                .map(|q| q.prompt)
                .collect::<Vec<_>>(),
            selected(DrillTopic::Position, 2, 3)
                .iter()
                .map(|q| q.prompt)
                .collect::<Vec<_>>()
        );
    }
    #[test]
    fn walkthrough_persists_attempts_separately_and_recommends_only_after_evidence() {
        let mut p = Progress::default();
        let mut output = Vec::new();
        run(
            DrillTopic::CallingPrices,
            7,
            &mut std::io::Cursor::new(b"1\n1\n1\n"),
            &mut output,
            &mut p,
        )
        .unwrap();
        assert_eq!(p.decisions, 0);
        let dp = p.drills.get("callingprices").unwrap();
        assert_eq!(dp.attempts, 3);
        assert_eq!(dp.completed_sets, 1);
        assert!(String::from_utf8(output).unwrap().contains("Set complete"));
        assert!(recommendation(&p).is_none());
        record(&mut p, DrillTopic::CallingPrices, &[false, false, false]);
        assert!(recommendation(&p).unwrap().contains("6 reviewed answers"));
    }
    #[test]
    fn truncated_input_does_not_record_a_completed_set() {
        let mut p = Progress::default();
        let mut out = Vec::new();
        assert!(run(
            DrillTopic::Position,
            3,
            &mut std::io::Cursor::new(b"1\n"),
            &mut out,
            &mut p
        )
        .is_err());
        assert!(p.drills.is_empty());
    }
}
