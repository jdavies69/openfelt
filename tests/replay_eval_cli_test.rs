use std::{fs, process::Command};
use terminal_poker::trainer::{hero, Session};

fn temp_dir(label: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "openfelt-{label}-{}-{:016x}",
        std::process::id(),
        rand::random::<u64>()
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

fn completed_hand() -> terminal_poker::trainer::replay::CompletedHand {
    let mut session = Session::new(Default::default()).unwrap();
    for _ in 0..500 {
        if session.finished() {
            break;
        }
        if session.view().to_act == Some(hero()) {
            let action = session.observation(hero()).unwrap().check_call();
            session.submit(action).unwrap();
            session.continue_hand();
        } else {
            session.step_bot().unwrap();
        }
    }
    assert!(session.finished());
    session.replay_ready.remove(0)
}

#[test]
fn replay_cli_lists_navigates_and_persists_bookmark_while_skipping_bad_line() {
    let root = temp_dir("replay-cli");
    let hand = completed_hand();
    fs::write(
        root.join("completed-hands.jsonl"),
        format!("{}\nnot-json\n", serde_json::to_string(&hand).unwrap()),
    )
    .unwrap();
    let exe = env!("CARGO_BIN_EXE_openfelt-replay");
    let list = Command::new(exe)
        .args(["--data-dir", root.to_str().unwrap(), "list"])
        .output()
        .unwrap();
    assert!(list.status.success());
    assert!(String::from_utf8_lossy(&list.stdout).contains(&hand.hand_id));
    assert!(String::from_utf8_lossy(&list.stderr).contains("Skipped 1"));
    let show = Command::new(exe)
        .args([
            "--data-dir",
            root.to_str().unwrap(),
            "show",
            &hand.hand_id,
            "--decision",
            "0",
        ])
        .output()
        .unwrap();
    assert!(show.status.success());
    assert!(String::from_utf8_lossy(&show.stdout).contains("saved pre-decision state"));
    let bookmarked = Command::new(exe)
        .args([
            "--data-dir",
            root.to_str().unwrap(),
            "bookmark",
            &hand.hand_id,
            "0",
        ])
        .output()
        .unwrap();
    assert!(bookmarked.status.success());
    let reopened = terminal_poker::trainer::replay::Archive::load(&root).unwrap();
    assert!(reopened.bookmarks.iter().any(|b| b.hand_id == hand.hand_id));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn evaluation_cli_defaults_offline_and_live_guard_makes_no_call() {
    let exe = env!("CARGO_BIN_EXE_openfelt-eval");
    let offline = Command::new(exe).output().unwrap();
    assert!(offline.status.success());
    let report: serde_json::Value = serde_json::from_slice(&offline.stdout).unwrap();
    assert_eq!(report["mode"], "offline_fixture");
    assert_eq!(report["estimated_cost_usd"], 0.0);
    assert_eq!(report["scenarios"].as_array().unwrap().len(), 3);
    for scenario in report["scenarios"].as_array().unwrap() {
        assert_eq!(scenario["schema_valid"], true);
        assert_eq!(scenario["stale_response_rejected"], true);
        assert_eq!(scenario["unsupported_claim_rejected"], true);
        assert_eq!(scenario["predecision_only"], true);
        assert!(scenario["factual_correctness"].is_null());
    }
    let live = Command::new(exe)
        .arg("--live")
        .env_remove("OPENAI_API_KEY")
        .output()
        .unwrap();
    assert!(!live.status.success());
    assert!(String::from_utf8_lossy(&live.stderr).contains("requires OPENAI_API_KEY"));
}
