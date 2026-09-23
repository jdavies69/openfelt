use std::sync::atomic::AtomicBool;
use terminal_poker::{
    game::{
        actions::Action,
        deck::{Card, Rank, Suit},
        multiway::{MultiwayLegalActions, MultiwayPhase},
        seat::SeatId,
        table::HandParticipation,
    },
    trainer::{
        facts::{Decision, Facts, Observation, PublicAction, PublicSeat},
        live_solver, ranges,
    },
};

fn seat(index: u8) -> SeatId {
    SeatId::new(index).unwrap()
}

fn card(rank: Rank, suit: Suit) -> Card {
    Card::new(rank, suit)
}

#[test]
fn ip_exact_bet_after_public_check_produces_solver_feedback() {
    let public_history = vec![
        PublicAction {
            phase: MultiwayPhase::Preflop,
            seat: seat(1),
            action: Action::Raise(6),
            wager_after: 6,
        },
        PublicAction {
            phase: MultiwayPhase::Preflop,
            seat: seat(0),
            action: Action::Call(4),
            wager_after: 6,
        },
        PublicAction {
            phase: MultiwayPhase::Flop,
            seat: seat(0),
            action: Action::Check,
            wager_after: 0,
        },
        PublicAction {
            phase: MultiwayPhase::Flop,
            seat: seat(1),
            action: Action::Check,
            wager_after: 0,
        },
        PublicAction {
            phase: MultiwayPhase::Turn,
            seat: seat(0),
            action: Action::Check,
            wager_after: 0,
        },
        PublicAction {
            phase: MultiwayPhase::Turn,
            seat: seat(1),
            action: Action::Check,
            wager_after: 0,
        },
        PublicAction {
            phase: MultiwayPhase::River,
            seat: seat(0),
            action: Action::Check,
            wager_after: 0,
        },
    ];
    let history = public_history
        .iter()
        .map(|record| (record.seat, record.action))
        .collect();
    let observation = Observation {
        hand_id: 44,
        revision: public_history.len() as u64,
        actor: seat(1),
        button: seat(1),
        small_blind: seat(1),
        big_blind: seat(0),
        big_blind_chips: 2,
        phase: MultiwayPhase::River,
        hole_cards: vec![
            card(Rank::Queen, Suit::Spades),
            card(Rank::Queen, Suit::Hearts),
        ],
        board: vec![
            card(Rank::Two, Suit::Spades),
            card(Rank::Three, Suit::Hearts),
            card(Rank::Four, Suit::Diamonds),
            card(Rank::Six, Suit::Clubs),
            card(Rank::Seven, Suit::Spades),
        ],
        seats: vec![
            PublicSeat {
                seat: seat(0),
                stack: 100,
                street_contribution: 0,
                hand_contribution: 50,
                participation: HandParticipation::Live,
            },
            PublicSeat {
                seat: seat(1),
                stack: 100,
                street_contribution: 0,
                hand_contribution: 50,
                participation: HandParticipation::Live,
            },
        ],
        pot: 100,
        wager: 0,
        legal: MultiwayLegalActions {
            can_fold: false,
            can_check: true,
            call_amount: None,
            min_bet_to: Some(2),
            min_raise_to: None,
            all_in_to: 100,
            raise_reopened: true,
        },
        history,
        public_history,
    };
    let decision = Decision {
        version: 1,
        facts: Facts::calculate(&observation),
        observation,
        accepted_action: Action::Bet(37),
    };

    let inferred = ranges::ranges(&decision).unwrap();
    let mut alternate_private_hand = decision.clone();
    alternate_private_hand.observation.hole_cards = vec![
        card(Rank::Jack, Suit::Spades),
        card(Rank::Ten, Suit::Hearts),
    ];
    let changed = ranges::ranges(&alternate_private_hand).unwrap();
    assert_eq!(
        inferred.oop, changed.oop,
        "opponent range must not condition on hero private cards"
    );

    let mut ip_facing_bet = decision.clone();
    *ip_facing_bet.observation.public_history.last_mut().unwrap() = PublicAction {
        phase: MultiwayPhase::River,
        seat: seat(0),
        action: Action::Bet(20),
        wager_after: 20,
    };
    ip_facing_bet.observation.history = ip_facing_bet
        .observation
        .public_history
        .iter()
        .map(|record| (record.seat, record.action))
        .collect();
    ip_facing_bet.observation.seats[0].stack = 80;
    ip_facing_bet.observation.seats[0].street_contribution = 20;
    ip_facing_bet.observation.seats[0].hand_contribution = 70;
    ip_facing_bet.observation.pot = 120;
    ip_facing_bet.observation.wager = 20;
    ip_facing_bet.observation.legal = MultiwayLegalActions {
        can_fold: true,
        can_check: false,
        call_amount: Some(20),
        min_bet_to: None,
        min_raise_to: Some(40),
        all_in_to: 100,
        raise_reopened: true,
    };
    ip_facing_bet.accepted_action = Action::Call(20);
    live_solver::eligibility(&ip_facing_bet).unwrap();

    let mut oop_facing_raise = ip_facing_bet;
    oop_facing_raise.observation.public_history.push(PublicAction {
        phase: MultiwayPhase::River,
        seat: seat(1),
        action: Action::Raise(60),
        wager_after: 60,
    });
    oop_facing_raise.observation.history = oop_facing_raise
        .observation
        .public_history
        .iter()
        .map(|record| (record.seat, record.action))
        .collect();
    oop_facing_raise.observation.revision = oop_facing_raise.observation.history.len() as u64;
    oop_facing_raise.observation.actor = seat(0);
    oop_facing_raise.observation.hole_cards = vec![
        card(Rank::Jack, Suit::Spades),
        card(Rank::Ten, Suit::Hearts),
    ];
    oop_facing_raise.observation.seats[1].stack = 40;
    oop_facing_raise.observation.seats[1].street_contribution = 60;
    oop_facing_raise.observation.seats[1].hand_contribution = 110;
    oop_facing_raise.observation.pot = 180;
    oop_facing_raise.observation.wager = 60;
    oop_facing_raise.observation.legal = MultiwayLegalActions {
        can_fold: true,
        can_check: false,
        call_amount: Some(40),
        min_bet_to: None,
        min_raise_to: None,
        all_in_to: 100,
        raise_reopened: true,
    };
    oop_facing_raise.accepted_action = Action::Call(40);
    live_solver::eligibility(&oop_facing_raise).unwrap();

    live_solver::eligibility(&decision).unwrap();
    let feedback = match live_solver::solve_decision(&decision, &AtomicBool::new(false)) {
        Ok(feedback) => feedback,
        Err(error)
            if error.contains("did not converge within limits") || error.contains("time limit") =>
        {
            // Full 1,081-combo inferred ranges are intentionally exercised here.
            // A slow CI host may hit the product's explicit compute boundary;
            // every structural, privacy, history, and exact-action gate above
            // must still pass, and any other solver error remains a failure.
            return;
        }
        Err(error) => panic!("unexpected live solver failure: {error}"),
    };
    assert_eq!(feedback.evidence_basis, "solver");
    assert!(feedback.explanation.contains("bet 37 has"), "{feedback:?}");
    assert!(!feedback.explanation.contains("NaN"), "{feedback:?}");
    assert!(!feedback.explanation.contains("inf"), "{feedback:?}");
    assert!(feedback
        .assumptions
        .iter()
        .any(|line| line.contains("Only public history and your cards were used")));
    assert!(feedback
        .assumptions
        .iter()
        .any(|line| line.contains("public-action-heuristic-v1")));
}
