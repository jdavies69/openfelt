use super::*;
use crate::game::table::HandParticipation;
use facts::{local_feedback, Feedback};

fn at_hero(session: &mut Session) {
    for _ in 0..100 {
        if session.hand.to_act == Some(hero()) || session.finished() {
            return;
        }
        session.step_bot().unwrap();
    }
    panic!("hero not reached");
}
fn sample_decision() -> facts::Decision {
    let mut s = Session::new(Settings::default()).unwrap();
    at_hero(&mut s);
    let a = s.observation(hero()).unwrap().check_call();
    s.submit(a).unwrap().clone()
}

#[test]
fn six_max_starts_with_exactly_one_hundred_bb_and_only_own_cards() {
    let s = Session::new(Settings::default()).unwrap();
    let p = s.view();
    assert_eq!(p.seats.len(), 6);
    assert_eq!(p.button, hero());
    assert_eq!(p.small_blind.as_u8(), 1);
    assert_eq!(p.big_blind.as_u8(), 2);
    assert!(p
        .seats
        .iter()
        .all(|seat| seat.stack + seat.hand_contribution == 200));
    assert_eq!(s.hand.total_chips(), 1200);
    assert_eq!(p.seats.iter().filter(|s| s.hole_cards.is_some()).count(), 1);
}
#[test]
fn configurable_blinds_control_starting_stacks() {
    let s = Session::new(Settings {
        small_blind: 3,
        big_blind: 7,
        seats: 9,
        ..Settings::default()
    })
    .unwrap();
    assert_eq!(s.hand.total_chips(), 6300);
    assert_eq!(s.hand.blind_values.big_blind, 7);
}
#[test]
fn accepted_decision_freezes_view_and_bots_until_continue() {
    for action_kind in 0..3 {
        let mut s = Session::new(Settings::default()).unwrap();
        at_hero(&mut s);
        let before = s.view();
        let o = s.observation(hero()).unwrap();
        let a = match action_kind {
            0 => Action::Fold,
            1 => o.check_call(),
            _ => Action::AllIn(o.legal.all_in_to),
        };
        s.submit(a).unwrap();
        let accepted_len = s.hand.action_history.len();
        for _ in 0..20 {
            assert!(!s.step_bot().unwrap());
            assert_eq!(s.view(), before);
        }
        assert_eq!(s.hand.action_history.len(), accepted_len);
        assert!(s.submit(a).is_err());
        s.continue_hand();
        assert!(s.coaching.is_none());
    }
}
#[test]
fn invalid_raise_does_not_commit_or_open_coaching() {
    let mut s = Session::new(Settings::default()).unwrap();
    at_hero(&mut s);
    let before = s.view();
    assert!(s.submit(Action::Raise(1)).is_err());
    assert_eq!(s.view(), before);
    assert!(s.coaching.is_none());
}
#[test]
fn hidden_card_and_deck_changes_do_not_change_coach_or_bot_input() {
    let mut s = Session::new(Settings::default()).unwrap();
    at_hero(&mut s);
    let before = s.observation(hero()).unwrap();
    let mut replacement = MultiwayHand::new_seeded_for_review(
        s.hand.table_size,
        s.hand.button,
        &s.hand
            .occupied_seats()
            .map(|p| (p, 200))
            .collect::<Vec<_>>(),
        974,
    )
    .unwrap();
    for p in s.hand.occupied_seats().collect::<Vec<_>>() {
        if p != hero() {
            s.hand.seats[p.index()].as_mut().unwrap().hole_cards =
                std::mem::take(&mut replacement.seats[p.index()].as_mut().unwrap().hole_cards);
        }
    }
    assert_eq!(
        serde_json::to_vec(&before).unwrap(),
        serde_json::to_vec(&s.observation(hero()).unwrap()).unwrap()
    );
    let mut b1 = Opponent::new(1, s.settings.opponents.clone());
    let mut b2 = Opponent::new(1, s.settings.opponents.clone());
    assert_eq!(
        b1.choose(&before),
        b2.choose(&s.observation(hero()).unwrap())
    );
    // A fresh authority with a different deck and identical public/own fields gives the same DTO.
    let original = s.hand.clone();
    replacement.seats = original.seats.clone();
    replacement.phase = original.phase;
    replacement.board = original.board.clone();
    replacement.current_wager = original.current_wager;
    replacement.to_act = original.to_act;
    replacement.action_history = original.action_history.clone();
    replacement.last_full_raise_size = original.last_full_raise_size;
    s.hand = replacement;
    assert_eq!(before, s.observation(hero()).unwrap());
}
#[test]
fn actual_engine_short_stack_call_excludes_uncalled_excess() {
    let mut h = MultiwayHand::new_seeded_for_review(
        TableSize::new(2).unwrap(),
        hero(),
        &[(hero(), 50), (SeatId::new(1).unwrap(), 130)],
        23,
    )
    .unwrap();
    let villain = SeatId::new(1).unwrap();
    for (actor, action) in [
        (hero(), Action::Call(1)),
        (villain, Action::Check),
        (villain, Action::Bet(28)),
        (hero(), Action::Call(28)),
        (villain, Action::AllIn(100)),
    ] {
        h.apply_command(SeatCommand::new(actor, action)).unwrap();
    }
    let p = project_hand(&h, HandId(1), ProjectionAudience::Player(hero())).unwrap();
    let o = Observation::from_projection(&p, 5, vec![]).unwrap();
    let f = Facts::calculate(&o);
    assert_eq!(o.wager, 100);
    assert_eq!(f.call_cost, 20);
    assert_eq!(f.contestable_pot_after_call, 100);
    assert_eq!(f.effective_remaining_by_opponent, vec![(villain, 0)]);
    h.apply_command(SeatCommand::new(hero(), o.check_call()))
        .unwrap();
    assert_eq!(h.total_chips(), 180);
    assert_eq!(h.returned_excess.iter().map(|r| r.amount).sum::<u32>(), 80);
}
#[test]
fn side_pot_fact_includes_folded_money_but_not_ineligible_layers() {
    let mut d = sample_decision();
    let o = &mut d.observation;
    o.wager = 120;
    o.seats = vec![
        facts::PublicSeat {
            seat: hero(),
            stack: 30,
            street_contribution: 20,
            hand_contribution: 20,
            participation: HandParticipation::Live,
        },
        facts::PublicSeat {
            seat: SeatId::new(1).unwrap(),
            stack: 0,
            street_contribution: 120,
            hand_contribution: 120,
            participation: HandParticipation::AllIn,
        },
        facts::PublicSeat {
            seat: SeatId::new(2).unwrap(),
            stack: 0,
            street_contribution: 130,
            hand_contribution: 130,
            participation: HandParticipation::AllIn,
        },
        facts::PublicSeat {
            seat: SeatId::new(3).unwrap(),
            stack: 50,
            street_contribution: 10,
            hand_contribution: 10,
            participation: HandParticipation::Folded,
        },
    ];
    let facts = Facts::calculate(o);
    assert_eq!(facts.call_cost, 30);
    assert_eq!(facts.contestable_pot_after_call, 160);
    assert!(facts
        .effective_remaining_by_opponent
        .iter()
        .all(|(_, n)| *n == 0));
}
#[test]
fn long_cash_sessions_conserve_chips_except_recorded_cash_flows() {
    for seats in [2, 6, 9] {
        for profile in [
            policy::Profile::Fundamentals,
            policy::Profile::Recreational,
            policy::Profile::Competent,
        ] {
            let settings = Settings {
                seats,
                opponents: policy::PolicySettings {
                    profile,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut s = Session::new(settings).unwrap();
            let initial = u64::from(seats) * 200;
            for _ in 0..20 {
                for _ in 0..500 {
                    if s.finished() {
                        break;
                    }
                    if s.hand.to_act == Some(hero()) {
                        let a = s.observation(hero()).unwrap().check_call();
                        s.submit(a).unwrap();
                        s.continue_hand();
                    } else {
                        s.step_bot().unwrap();
                    }
                }
                assert!(s.finished());
                let profit = s.session_profit;
                s.top_up().unwrap();
                assert_eq!(profit, s.session_profit);
                s.next_hand().unwrap();
                assert_eq!(
                    u64::from(s.hand.total_chips()),
                    initial
                        + s.cash_events
                            .iter()
                            .map(|e| u64::from(e.added))
                            .sum::<u64>()
                        - s.cash_events
                            .iter()
                            .map(|e| u64::from(e.withdrawn))
                            .sum::<u64>()
                );
                assert_eq!(s.hand.occupied_seats().count(), usize::from(seats));
            }
        }
    }
}
#[test]
fn every_style_and_difficulty_completes_legal_hands() {
    for style in [
        policy::Style::Tight,
        policy::Style::Balanced,
        policy::Style::Loose,
    ] {
        for difficulty in [policy::Difficulty::Beginner, policy::Difficulty::Practiced] {
            let mut s = Session::new(Settings {
                opponents: policy::PolicySettings {
                    style,
                    difficulty,
                    ..Default::default()
                },
                ..Default::default()
            })
            .unwrap();
            for _ in 0..500 {
                if s.finished() {
                    break;
                }
                if s.view().to_act == Some(hero()) {
                    let a = s.observation(hero()).unwrap().check_call();
                    s.submit(a).unwrap();
                    s.continue_hand();
                } else {
                    s.step_bot().unwrap();
                }
            }
            assert!(s.finished(), "style {style:?} difficulty {difficulty:?}");
            assert_eq!(s.hand.total_chips(), 1200);
        }
    }
}
#[test]
fn opponent_choices_are_seed_reproducible_for_every_style_and_difficulty() {
    let d = sample_decision();
    for style in [
        policy::Style::Tight,
        policy::Style::Balanced,
        policy::Style::Loose,
    ] {
        for difficulty in [policy::Difficulty::Beginner, policy::Difficulty::Practiced] {
            let settings = policy::PolicySettings {
                style,
                difficulty,
                ..Default::default()
            };
            let mut a = Opponent::new(0x0000_fe17, settings.clone());
            let mut b = Opponent::new(0x0000_fe17, settings);
            for _ in 0..24 {
                assert_eq!(a.choose(&d.observation), b.choose(&d.observation));
            }
        }
    }
}
#[test]
fn style_and_difficulty_have_seeded_observable_behavior() {
    let mut o = sample_decision().observation;
    use crate::game::deck::{Card, Rank, Suit};
    o.actor = o.button;
    o.history.clear();
    o.hole_cards = vec![
        Card::new(Rank::Ace, Suit::Clubs),
        Card::new(Rank::Eight, Suit::Diamonds),
    ];
    let own = o.seats.iter_mut().find(|s| s.seat == o.actor).unwrap();
    own.street_contribution = 0;
    o.wager = 2;
    o.legal.can_check = false;
    o.legal.can_fold = true;
    o.legal.call_amount = Some(2);
    let base = policy::PolicySettings {
        aggression: 0.0,
        bluff_rate: 0.0,
        mistake_rate: 0.0,
        difficulty: policy::Difficulty::Practiced,
        ..Default::default()
    };
    let mut tight = Opponent::new(
        9,
        policy::PolicySettings {
            style: policy::Style::Tight,
            ..base.clone()
        },
    );
    let mut loose = Opponent::new(
        9,
        policy::PolicySettings {
            style: policy::Style::Loose,
            ..base.clone()
        },
    );
    assert_eq!(tight.choose(&o), Action::Fold);
    assert_eq!(loose.choose(&o), Action::Call(2));
    let mut beginner_continues = 0;
    for seed in 0..128 {
        let mut bot = Opponent::new(
            seed,
            policy::PolicySettings {
                style: policy::Style::Tight,
                difficulty: policy::Difficulty::Beginner,
                ..base.clone()
            },
        );
        if bot.choose(&o) != Action::Fold {
            beginner_continues += 1;
        }
    }
    assert!(beginner_continues > 0); // fixed seeds exercise the documented extra inconsistency
    assert!(policy::behavior_limitations()
        .iter()
        .any(|note| note.contains("not been calibrated")));
    assert!(policy::behavior_limitations()
        .iter()
        .any(|note| note.contains("measured skill rating")));
}
#[test]
fn stored_settings_and_progress_roundtrip_without_secret_fields() {
    let root = std::env::temp_dir().join(format!("openfelt-storage-{}", rand::random::<u64>()));
    let store = storage::Store { root: root.clone() };
    store.save("settings.json", &Settings::default()).unwrap();
    store
        .save(
            "progress.json",
            &storage::Progress {
                decisions: 7,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(store.settings().unwrap().seats, 6);
    assert_eq!(store.progress().unwrap().decisions, 7);
    assert!(serde_json::from_str::<Settings>(r#"{"api_key":"canary"}"#).is_err());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(root.join("settings.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn provider_contract_rejects_stale_malformed_numerical_and_secret_output() {
    let d = sample_decision();
    let f = local_feedback(&d);
    let wrap = |f: &Feedback| {
        serde_json::to_vec(&serde_json::json!({"status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":serde_json::to_string(f).unwrap()}]}],"usage":{"input_tokens":100,"output_tokens":50}})).unwrap()
    };
    assert!(provider::parse_response(&wrap(&f), &d, "fake-canary-key").is_ok());
    let mut stale = f.clone();
    stale.revision += 1;
    assert!(provider::parse_response(&wrap(&stale), &d, "").is_err());
    let mut numerical = f.clone();
    numerical.explanation = "You have 70% equity".into();
    assert!(provider::parse_response(&wrap(&numerical), &d, "").is_err());
    let mut secret = f.clone();
    secret.explanation = "fake-canary-key".into();
    let err = provider::parse_response(&wrap(&secret), &d, "fake-canary-key")
        .err()
        .unwrap();
    assert!(!err.contains("fake-canary-key"));
    let escaped = String::from_utf8(wrap(&secret))
        .unwrap()
        .replace("fake-canary-key", "fake\\u002dcanary\\u002dkey");
    assert!(provider::parse_response(escaped.as_bytes(), &d, "fake-canary-key").is_err());
    assert!(provider::parse_response(b"not json", &d, "").is_err());
    let body = provider::request_body(&d, &Default::default()).to_string();
    assert!(!body.contains("fake-canary-key"));
    assert!(!body.contains("opponent_cards"));
    assert!(!body.contains("seed"));
}
#[test]
fn request_cap_and_budget_reserve_before_sending() {
    let s = provider::ProviderSettings {
        max_requests: 1,
        ..Default::default()
    };
    let mut usage = provider::Usage::default();
    usage.reserve(&s, 100).unwrap();
    assert!(usage.reserve(&s, 100).is_err());
    let s = provider::ProviderSettings {
        input_usd_per_million: Some(10.0),
        output_usd_per_million: Some(20.0),
        budget_usd: Some(0.001),
        pricing_as_of: Some("test fixture prices".into()),
        ..Default::default()
    };
    let mut usage = provider::Usage::default();
    assert!(usage.reserve(&s, 100).is_err());
    assert_eq!(usage.requests, 0);
}

#[test]
fn short_all_in_does_not_reopen_hero_raise() {
    let mut s = Session::new(Settings {
        seats: 2,
        ..Default::default()
    })
    .unwrap();
    let villain = SeatId::new(1).unwrap();
    s.hand = MultiwayHand::new_seeded_for_review(
        TableSize::new(2).unwrap(),
        hero(),
        &[(hero(), 200), (villain, 14)],
        3,
    )
    .unwrap();
    s.submit(Action::Raise(10)).unwrap();
    s.continue_hand();
    s.hand
        .apply_command(SeatCommand::new(villain, Action::AllIn(14)))
        .unwrap();
    assert!(!s.observation(hero()).unwrap().legal.raise_reopened);
    assert!(s.submit(Action::AllIn(200)).is_err());
    assert!(s.coaching.is_none());
    s.submit(Action::Call(4)).unwrap();
}
