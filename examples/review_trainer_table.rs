//! Deterministic TestBackend captures for the actual OpenFelt trainer renderer.
use std::{env, fs, path::Path};

use ratatui::{backend::TestBackend, Terminal};
use serde_json::json;
use terminal_poker::trainer::{
    facts::local_feedback,
    hero,
    storage::Settings,
    table_ui::{self, RaiseView, TableMode, TableRenderState},
    tui::{
        collapsed_coaching_explanation, present_hand_label, raise_view_for,
        structured_coaching_copy,
    },
    Session,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = env::args().nth(1).ok_or("output directory required")?;
    fs::create_dir_all(&output)?;
    for seats in [3, 6, 9] {
        let mut session = hero_turn(seats)?;
        let mut sizes = vec![(80, 30), (100, 36)];
        if seats == 3 {
            sizes.push((176, 48));
        }
        for (width, height) in sizes {
            capture(
                &output,
                &format!("trainer-{seats}seat-hero-{width}x{height}"),
                width,
                height,
                &session,
                TableMode::Playing,
                Some("YOUR NEXT DECISION"),
                Some("Take your time. Coaching appears after your choice."),
                None,
                None,
            )?;
        }
        if seats == 3 {
            let postflop = postflop_hero_turn()?;
            capture(
                &output,
                "trainer-3seat-flop-176x48",
                176,
                48,
                &postflop,
                TableMode::Playing,
                Some("YOUR NEXT DECISION"),
                Some("Read the current board and choose your action."),
                None,
                None,
            )?;
        }
        if seats == 9 {
            let observation = session.observation(hero())?;
            let decision = session.submit(observation.check_call())?;
            let feedback = local_feedback(decision);
            let explanation = collapsed_coaching_explanation(&feedback);
            let coaching = structured_coaching_copy(
                decision,
                &explanation,
                feedback.alternative_action.as_deref(),
                false,
            );
            capture(
                &output,
                "trainer-9seat-coaching-80x30",
                80,
                30,
                &session,
                TableMode::Paused,
                Some("AFTER YOUR DECISION"),
                Some(&coaching),
                None,
                None,
            )?;
        }
        if seats == 6 {
            let observation = session.observation(hero())?;
            let raise = raise_view_for(&observation, "0");
            capture(
                &output,
                "trainer-6seat-raise-100x36",
                100,
                36,
                &session,
                TableMode::Playing,
                Some("YOUR NEXT DECISION"),
                Some("Choose a total street amount."),
                Some(RaiseView {
                    amount: raise.presets[2],
                    ..raise
                }),
                None,
            )?;
            capture(
                &output,
                "trainer-6seat-raise-176x48",
                176,
                48,
                &session,
                TableMode::Playing,
                Some("YOUR NEXT DECISION"),
                Some("Choose a legal total street amount."),
                Some(RaiseView {
                    amount: raise.presets[3],
                    ..raise
                }),
                None,
            )?;
            let decision = session.submit(observation.check_call())?;
            let feedback = local_feedback(decision);
            let explanation = collapsed_coaching_explanation(&feedback);
            let coaching = structured_coaching_copy(
                decision,
                &explanation,
                feedback.alternative_action.as_deref(),
                false,
            );
            let deep = format!(
                "{}\nLegal call: {} chips. Contestable pot after call: {} chips.\n{}",
                structured_coaching_copy(
                    decision,
                    &explanation,
                    feedback.alternative_action.as_deref(),
                    true,
                ),
                decision.facts.call_cost,
                decision.facts.contestable_pot_after_call,
                decision.facts.assumptions[0],
            );
            for (width, height) in [(80, 30), (100, 36), (176, 48)] {
                capture(
                    &output,
                    &format!("trainer-6seat-coaching-{width}x{height}"),
                    width,
                    height,
                    &session,
                    TableMode::Paused,
                    Some("AFTER YOUR DECISION"),
                    Some(&coaching),
                    None,
                    None,
                )?;
                capture(
                    &output,
                    &format!("trainer-6seat-coaching-deep-{width}x{height}"),
                    width,
                    height,
                    &session,
                    TableMode::Paused,
                    Some("AFTER YOUR DECISION"),
                    Some(&coaching),
                    None,
                    Some((&deep, " AFTER YOUR DECISION ")),
                )?;
            }
            session.continue_hand();
            finish_hand(&mut session)?;
            capture(
                &output,
                "trainer-6seat-complete-100x36",
                100,
                36,
                &session,
                TableMode::Complete,
                Some("HAND COMPLETE"),
                session.result_summary().as_deref(),
                None,
                None,
            )?;
        }
    }
    capture_settings(&output, 100, 36)?;
    println!("TRAINER_TABLE_CAPTURES_PASS {output}");
    Ok(())
}

fn finish_hand(session: &mut Session) -> Result<(), String> {
    for _ in 0..256 {
        if session.finished() {
            return Ok(());
        }
        if session.view().to_act == Some(hero()) {
            let observation = session.observation(hero())?;
            session.submit(observation.check_call())?;
            session.continue_hand();
        } else {
            session.step_bot()?;
        }
    }
    Err("fixture did not finish".into())
}

fn capture_settings(
    output: impl AsRef<Path>,
    width: u16,
    height: u16,
) -> Result<(), Box<dyn std::error::Error>> {
    let rows = vec![
        "Provider       Local".to_string(),
        "Model          (none)  [←/→ curated · E custom]".to_string(),
        "API key        (leave unchanged)".to_string(),
        "Session limit  5 requests".to_string(),
        "Table          6 seats · 1/2 blinds".to_string(),
        "Opponents      Standard".to_string(),
        "Test key       explicit one-request check".to_string(),
        "Save and return".to_string(),
        "Updates        no check performed".to_string(),
        "Cloud limits   600 output tokens · budget unset".to_string(),
        "Status         Settings are local until saved".to_string(),
    ];
    let mut terminal = Terminal::new(TestBackend::new(width, height))?;
    terminal.draw(|frame| {
        table_ui::render_settings_panel(
            frame,
            frame.area(),
            "SETTINGS · credential: none",
            &rows,
            4,
        )
    })?;
    let cells: Vec<_> = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| {
            json!({
                "symbol": cell.symbol(), "foreground": format!("{:?}", cell.fg),
                "background": format!("{:?}", cell.bg), "modifiers": cell.modifier.bits(),
            })
        })
        .collect();
    fs::write(
        output.as_ref().join("trainer-settings-100x36.json"),
        serde_json::to_vec_pretty(&json!({
            "renderer":"terminal_poker::trainer::table_ui::render_settings_panel", "backend":"ratatui::backend::TestBackend",
            "fixture":"trainer-settings-100x36", "width":width, "height":height, "cells":cells,
        }))?,
    )?;
    Ok(())
}

fn postflop_hero_turn() -> Result<Session, String> {
    for seed in 1..=2_000 {
        let mut session = Session::new_seeded_for_evaluation(
            Settings {
                seats: 3,
                ..Settings::default()
            },
            seed,
        )?;
        for _ in 0..96 {
            let view = session.view();
            if view.phase.name() == "Flop" && view.to_act == Some(hero()) {
                let label = session.observation(hero()).map(|o| {
                    terminal_poker::trainer::facts::Facts::calculate(&o).hand_classification
                })?;
                if label.to_ascii_lowercase().contains("pair")
                    && label.to_ascii_lowercase().contains("queen")
                {
                    return Ok(session);
                }
                break;
            }
            if session.finished() {
                break;
            }
            if view.to_act == Some(hero()) {
                let observation = session.observation(hero())?;
                session.submit(observation.check_call())?;
                session.continue_hand();
            } else {
                session.step_bot()?;
            }
        }
    }
    Err("could not build deterministic three-seat flop pair-of-queens fixture".into())
}

fn hero_turn(seats: u8) -> Result<Session, String> {
    let mut session = Session::new_seeded_for_evaluation(
        Settings {
            seats,
            ..Settings::default()
        },
        20_260_922 + u64::from(seats),
    )?;
    for _ in 0..64 {
        if session.view().to_act == Some(hero()) || session.finished() {
            break;
        }
        session.step_bot()?;
    }
    if session.view().to_act != Some(hero()) {
        return Err(format!("fixture did not reach hero for {seats} seats"));
    }
    Ok(session)
}

#[allow(clippy::too_many_arguments)]
fn capture(
    output: impl AsRef<Path>,
    name: &str,
    width: u16,
    height: u16,
    session: &Session,
    mode: TableMode,
    notice_title: Option<&str>,
    notice: Option<&str>,
    raise: Option<RaiseView>,
    details: Option<(&str, &str)>,
) -> Result<(), Box<dyn std::error::Error>> {
    let projection = session.view();
    let actions = session.recent_actions();
    let feedback = session.coaching.as_ref().map(local_feedback);
    let hand_label = session
        .coaching
        .as_ref()
        .map(|d| present_hand_label(&d.facts.hand_classification))
        .or_else(|| {
            session.observation(hero()).ok().map(|o| {
                present_hand_label(
                    &terminal_poker::trainer::facts::Facts::calculate(&o).hand_classification,
                )
            })
        });
    let mut terminal = Terminal::new(TestBackend::new(width, height))?;
    terminal.draw(|frame| {
        table_ui::render(
            frame,
            &TableRenderState {
                projection: &projection,
                hero: hero(),
                hand_id: session.hand_id,
                recent_actions: &actions,
                status: "deterministic review",
                mode,
                notice_title,
                notice,
                raise,
                hand_label: hand_label.as_deref(),
                review_tone: feedback
                    .as_ref()
                    .map(|feedback| table_ui::ReviewTone::from_assessment(&feedback.assessment)),
                guidance_source: feedback.as_ref().map(|_| "Local guidance · heuristic"),
            },
        );
        if let Some((body, title)) = details {
            table_ui::render_coaching_details(frame, title, body, 0);
        }
    })?;
    let cells: Vec<_> = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| {
            json!({
                "symbol": cell.symbol(),
                "foreground": format!("{:?}", cell.fg),
                "background": format!("{:?}", cell.bg),
                "modifiers": cell.modifier.bits(),
            })
        })
        .collect();
    fs::write(
        output.as_ref().join(format!("{name}.json")),
        serde_json::to_vec_pretty(&json!({
            "renderer": "terminal_poker::trainer::table_ui::render",
            "backend": "ratatui::backend::TestBackend",
            "fixture": name,
            "width": width,
            "height": height,
            "seats": projection.table_size.get(),
            "phase": projection.phase.name(),
            "to_act": projection.to_act.map(|seat| seat.as_u8()),
            "recent_actions": actions,
            "raise": raise.map(|view| json!({
                "amount": view.amount,
                "minimum": view.minimum,
                "maximum": view.maximum,
                "presets": view.presets,
            })),
            "cells": cells,
        }))?,
    )?;
    Ok(())
}
