//! Deterministic TestBackend captures for the actual OpenFelt trainer renderer.
use std::{env, fs, path::Path};

use ratatui::{backend::TestBackend, Terminal};
use serde_json::json;
use terminal_poker::trainer::{
    facts::local_feedback,
    hero,
    storage::Settings,
    table_ui::{self, RaiseView, TableMode, TableRenderState},
    Session,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = env::args().nth(1).ok_or("output directory required")?;
    fs::create_dir_all(&output)?;
    for seats in [2, 6, 9] {
        let mut session = hero_turn(seats)?;
        for (width, height) in [(80, 30), (100, 36)] {
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
        if seats == 6 {
            let observation = session.observation(hero())?;
            let minimum = observation
                .legal
                .min_raise_to
                .or(observation.legal.min_bet_to)
                .unwrap_or(observation.legal.all_in_to);
            let maximum = observation.legal.all_in_to;
            let spread = maximum.saturating_sub(minimum);
            let presets = [
                minimum,
                minimum + spread / 4,
                minimum + spread / 2,
                minimum + spread.saturating_mul(3) / 4,
                maximum,
            ];
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
                    amount: presets[2],
                    minimum,
                    maximum,
                    presets,
                }),
                None,
            )?;
            let decision = session.submit(observation.check_call())?;
            let feedback = local_feedback(decision);
            let coaching = format!(
                "You {}. The visible hand is frozen.\nEnter continues · ? details\n{} · {}\n{}\nLegal call: {} chips. Contestable pot after call: {} chips.",
                decision.accepted_action.description(),
                feedback.concept,
                feedback.assessment,
                feedback.explanation.replace("LateOpen", "late-position range"),
                decision.facts.call_cost,
                decision.facts.contestable_pot_after_call,
            );
            for (width, height) in [(80, 30), (100, 36)] {
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
                    Some("Enter continues · ? close details"),
                    None,
                    Some((&coaching, " AFTER YOUR DECISION ")),
                )?;
            }
        }
    }
    println!("TRAINER_TABLE_CAPTURES_PASS {output}");
    Ok(())
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
