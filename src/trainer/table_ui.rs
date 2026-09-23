//! Purpose-built OpenFelt practice-table renderer.
//!
//! This module accepts only the hero's authorized projection and display copy.
//! It owns geometry and styling, never game state or input handling.

use crate::{
    game::{
        deck::Card,
        hand::evaluate_hand,
        multiway::{build_pots, Contribution},
        seat::SeatId,
        table::HandParticipation,
    },
    protocol::{ProjectedSeat, TableProjection},
};
use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap},
    Frame,
};
use std::collections::BTreeMap;

const BG: Color = Color::Rgb(30, 30, 30);
const TABLE: Color = Color::Rgb(4, 4, 4);
const PANEL: Color = Color::Rgb(24, 24, 24);
const RULE: Color = Color::Rgb(83, 83, 83);
const TEXT: Color = Color::Rgb(222, 222, 218);
const MUTED: Color = Color::Rgb(126, 126, 122);
const YELLOW: Color = Color::Rgb(244, 202, 47);
const GREEN: Color = Color::Rgb(52, 204, 104);
const RED: Color = Color::Rgb(196, 0, 0);
const TEAL: Color = Color::Rgb(4, 119, 125);
const MUSTARD: Color = Color::Rgb(138, 132, 0);
const CARD_FACE: Color = Color::Rgb(218, 217, 239);
const CARD_INK: Color = Color::Rgb(27, 27, 31);
const CARD_RED: Color = Color::Rgb(190, 40, 56);
const CARD_BACK: Color = Color::Rgb(88, 89, 99);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableMode {
    Playing,
    Paused,
    Complete,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReviewTone {
    Good,
    Reconsider,
    Uncertain,
}
impl ReviewTone {
    pub fn from_assessment(assessment: &str) -> Self {
        match assessment {
            "reasonable" => Self::Good,
            "reconsider" => Self::Reconsider,
            _ => Self::Uncertain,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RaiseView {
    pub amount: u32,
    pub minimum: u32,
    pub maximum: u32,
    pub presets: [u32; 5],
    pub is_bet: bool,
}

pub struct TableRenderState<'a> {
    pub projection: &'a TableProjection,
    pub hero: SeatId,
    pub hand_id: u64,
    pub recent_actions: &'a [String],
    pub status: &'a str,
    pub mode: TableMode,
    pub notice_title: Option<&'a str>,
    pub notice: Option<&'a str>,
    pub raise: Option<RaiseView>,
    pub hand_label: Option<&'a str>,
    pub review_tone: Option<ReviewTone>,
    pub guidance_source: Option<&'a str>,
    pub active_coaching: &'a str,
}

pub fn render(frame: &mut Frame<'_>, state: &TableRenderState<'_>) {
    let area = frame.area();
    frame.render_widget(Block::default().style(Style::default().bg(BG)), area);
    if area.width < 80 || area.height < 30 {
        frame.render_widget(
            Paragraph::new(format!(
                "OpenFelt needs 80 × 30 or larger. Current: {} × {}.\nResize to continue. Q quits safely.",
                area.width, area.height
            ))
            .style(Style::default().fg(TEXT).bg(BG)),
            area,
        );
        return;
    }

    let shell_width = area.width.min(128);
    let shell_height = area.height.min(38);
    let shell = Rect::new(
        area.x + area.width.saturating_sub(shell_width) / 2,
        area.y + area.height.saturating_sub(shell_height) / 2,
        shell_width,
        shell_height,
    );
    let paused = state.mode == TableMode::Paused;
    let rows = if paused {
        Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(14),
            Constraint::Length(8),
            Constraint::Length(1),
        ])
        .split(shell)
    } else if state.mode == TableMode::Complete {
        Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(19),
            Constraint::Length(7),
            Constraint::Length(1),
        ])
        .split(shell)
    } else {
        Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(20),
            Constraint::Length(6),
            Constraint::Length(1),
        ])
        .split(shell)
    };
    render_header(frame, state, rows[0]);
    let rail_width = if shell.width >= 110 { 28 } else { 23 };
    let body =
        Layout::horizontal([Constraint::Min(56), Constraint::Length(rail_width)]).split(rows[1]);
    render_stage(frame, state, body[0]);
    render_rail(frame, state, body[1]);
    if paused {
        render_coaching(frame, state, rows[2]);
    } else {
        render_controls(frame, state, rows[2]);
    }
    let footer =
        Layout::horizontal([Constraint::Percentage(35), Constraint::Percentage(65)]).split(rows[3]);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("● ", Style::default().fg(GREEN)),
            Span::styled(state.active_coaching, Style::default().fg(MUTED)),
        ]))
        .style(Style::default().bg(BG)),
        footer[0],
    );
    frame.render_widget(
        Paragraph::new(Span::styled(
            "? help · s settings · q quit",
            Style::default().fg(MUTED),
        ))
        .alignment(Alignment::Right)
        .style(Style::default().bg(BG)),
        footer[1],
    );
}

pub fn render_settings_panel(
    frame: &mut Frame<'_>,
    area: Rect,
    title: &str,
    rows: &[String],
    selected: usize,
) {
    let width = area.width.saturating_sub(8).min(92);
    let height = (rows.len() as u16 + 4).min(area.height.saturating_sub(4));
    let panel = Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    );
    let lines = rows
        .iter()
        .enumerate()
        .map(|(index, row)| {
            let active = index == selected;
            Line::from(vec![
                Span::styled(
                    if active { " › " } else { "   " },
                    Style::default().fg(if active { GREEN } else { MUTED }),
                ),
                Span::styled(
                    row.clone(),
                    Style::default()
                        .fg(if active { TEXT } else { MUTED })
                        .add_modifier(if active {
                            Modifier::BOLD
                        } else {
                            Modifier::empty()
                        }),
                ),
            ])
        })
        .collect::<Vec<_>>();
    frame.render_widget(Clear, panel);
    frame.render_widget(
        Paragraph::new(lines)
            .style(Style::default().bg(PANEL))
            .wrap(Wrap { trim: false })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(GREEN))
                    .style(Style::default().bg(PANEL))
                    .title(Span::styled(
                        format!(" {title} "),
                        Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
                    )),
            ),
        panel,
    );
}

fn render_coaching(frame: &mut Frame<'_>, state: &TableRenderState<'_>, area: Rect) {
    let title = state.notice_title.unwrap_or("REVIEW");
    let (badge, color) = match state.review_tone.unwrap_or(ReviewTone::Uncertain) {
        ReviewTone::Good => ("✓ GOOD DECISION", GREEN),
        ReviewTone::Reconsider => ("! QUESTIONABLE DECISION", YELLOW),
        ReviewTone::Uncertain => ("? UNCERTAIN", MUTED),
    };
    let rows = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(area);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(GREEN))
        .style(Style::default().bg(PANEL))
        .title(Span::styled(
            format!(" {title} "),
            Style::default().fg(GREEN).add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(rows[0]);
    frame.render_widget(block, rows[0]);
    let copy = state.notice.unwrap_or(state.status);
    let receipt = copy
        .lines()
        .find(|line| line.starts_with("BEFORE ACTION"))
        .unwrap_or("");
    let why = copy
        .lines()
        .find(|line| line.starts_with("WHY"))
        .unwrap_or("");
    let consider = copy
        .lines()
        .find(|line| line.starts_with("CONSIDER"))
        .unwrap_or("");
    let fields = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(2),
        Constraint::Length(1),
    ])
    .split(inner);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                format!(" {badge} "),
                Style::default()
                    .fg(Color::Black)
                    .bg(color)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(
                    "  {}",
                    state.guidance_source.unwrap_or("heuristic guidance")
                ),
                Style::default().fg(MUTED),
            ),
        ])),
        fields[0],
    );
    frame.render_widget(
        Paragraph::new(receipt).style(Style::default().fg(TEXT)),
        fields[1],
    );
    frame.render_widget(
        Paragraph::new(wrapped_excerpt(why, fields[2].width, 2)).style(Style::default().fg(TEXT)),
        fields[2],
    );
    frame.render_widget(
        Paragraph::new(consider).style(Style::default().fg(TEXT)),
        fields[3],
    );
    frame.render_widget(
        Paragraph::new("Enter continue  ·  ? full explanation")
            .alignment(Alignment::Center)
            .style(
                Style::default()
                    .fg(TEXT)
                    .bg(GREEN)
                    .add_modifier(Modifier::BOLD),
            ),
        rows[1],
    );
}

fn wrapped_excerpt(copy: &str, width: u16, max_lines: usize) -> String {
    let width = usize::from(width).max(1);
    let mut lines = vec![String::new()];
    let mut truncated = false;
    'words: for word in copy.split_whitespace() {
        let current = lines.last_mut().expect("one line");
        let separator = usize::from(!current.is_empty());
        if Line::from(word).width() <= width
            && Line::from(current.as_str()).width() + separator + Line::from(word).width() <= width
        {
            if separator == 1 {
                current.push(' ');
            }
            current.push_str(word);
        } else if Line::from(word).width() <= width {
            if lines.len() == max_lines {
                truncated = true;
                break;
            }
            lines.push(word.to_string());
        } else {
            if !current.is_empty() {
                if lines.len() == max_lines {
                    truncated = true;
                    break;
                }
                lines.push(String::new());
            }
            for character in word.chars() {
                let current = lines.last_mut().expect("one line");
                let cell_width = Line::from(character.to_string()).width();
                if Line::from(current.as_str()).width() + cell_width > width {
                    if lines.len() == max_lines {
                        truncated = true;
                        break 'words;
                    }
                    lines.push(String::new());
                }
                lines.last_mut().expect("one line").push(character);
            }
        }
    }
    if truncated {
        let last = lines.last_mut().expect("one line");
        while Line::from(last.as_str()).width() + Line::from("…").width() > width {
            last.pop();
        }
        *last = format!("{}…", last.trim_end());
    }
    lines.join("\n")
}

/// Expanded coaching occupies the right rail, preserving board and hero cards.
pub fn render_coaching_details(frame: &mut Frame<'_>, title: &str, body: &str, scroll: u16) {
    let area = frame.area();
    if area.width < 80 || area.height < 30 {
        return;
    }
    let shell_width = area.width.min(128);
    let shell_height = area.height.min(38);
    let shell = Rect::new(
        area.x + area.width.saturating_sub(shell_width) / 2,
        area.y + area.height.saturating_sub(shell_height) / 2,
        shell_width,
        shell_height,
    );
    let rail_width = if shell.width >= 110 { 28 } else { 23 };
    let rail_x = shell.x + shell.width.saturating_sub(rail_width);
    let detail_title = if title.contains("SOLVER") {
        " SOLVER REVIEW "
    } else {
        " DECISION REVIEW "
    };
    let panel = Rect::new(
        rail_x,
        shell.y + 3,
        area.x + area.width - rail_x,
        shell.height.saturating_sub(3 + 8 + 1),
    );
    frame.render_widget(Clear, panel);
    let rows = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).split(panel);
    frame.render_widget(
        Paragraph::new(body)
            .scroll((
                scroll.min(coaching_max_scroll(body, rows[0].width, rows[0].height)),
                0,
            ))
            .style(Style::default().fg(TEXT).bg(BG))
            .wrap(Wrap { trim: true })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(detail_title)
                    .border_style(Style::default().fg(GREEN))
                    .style(Style::default().bg(BG)),
            ),
        rows[0],
    );
    frame.render_widget(
        Paragraph::new(if panel.width < 35 {
            "↑↓ scroll · Esc back"
        } else {
            "↑/↓ scroll · Esc close · Enter continue"
        })
        .alignment(Alignment::Center)
        .style(
            Style::default()
                .fg(Color::Black)
                .bg(GREEN)
                .add_modifier(Modifier::BOLD),
        ),
        rows[1],
    );
}

/// Maximum vertical scroll for the expanded coaching panel. This intentionally
/// uses the same word-wrapping rules as the paragraph closely enough to pin an
/// oversized scroll request to the final readable page instead of a blank one.
pub fn coaching_max_scroll(body: &str, panel_width: u16, panel_height: u16) -> u16 {
    let width = usize::from(panel_width.saturating_sub(2)).max(1);
    let visible = usize::from(panel_height.saturating_sub(2)).max(1);
    let lines = body
        .lines()
        .map(|line| wrapped_lines(line, width))
        .sum::<usize>();
    lines.saturating_sub(visible).min(usize::from(u16::MAX)) as u16
}

fn wrapped_lines(line: &str, width: usize) -> usize {
    if line.is_empty() {
        return 1;
    }
    let mut lines = 1;
    let mut used = 0;
    for word in line.split_whitespace() {
        let len = Line::from(word).width();
        if used > 0 && used + 1 + len > width {
            lines += 1;
            used = len;
        } else {
            used += usize::from(used > 0) + len;
        }
        while used > width {
            lines += 1;
            used -= width;
        }
    }
    lines
}

fn render_header(frame: &mut Frame<'_>, state: &TableRenderState<'_>, area: Rect) {
    let phase = state.projection.phase.name().to_ascii_uppercase();
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled(
                    " OPENFELT ",
                    Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
                ),
                Span::styled("play the hand · learn the game", Style::default().fg(MUTED)),
                Span::raw("  "),
                Span::styled(
                    format!("hand #{}", state.hand_id),
                    Style::default().fg(YELLOW).add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::from(Span::styled(
                format!(
                    " {phase}  ·  {}/{} blinds  ·  {} seats  ·  no rake",
                    state.projection.small_blind_amount,
                    state.projection.big_blind_amount,
                    state.projection.table_size.get()
                ),
                Style::default().fg(MUTED),
            )),
        ])
        .style(Style::default().bg(BG)),
        area,
    );
}

fn render_stage(frame: &mut Frame<'_>, state: &TableRenderState<'_>, area: Rect) {
    let stage_height = area.height.min(26);
    let area = Rect::new(
        area.x,
        area.y + area.height.saturating_sub(stage_height) / 2,
        area.width,
        stage_height,
    );
    let seat_w = if area.width >= 90 {
        16
    } else if area.width >= 70 {
        14
    } else {
        12
    };
    let table = Rect::new(
        area.x + seat_w / 2,
        area.y + 6,
        area.width.saturating_sub(seat_w),
        area.height.saturating_sub(10),
    );
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(RULE))
            .style(Style::default().bg(TABLE)),
        table,
    );
    render_board(frame, state.projection, table);
    render_seats(frame, state, area, table, seat_w);
    if let Some(label) = state.hand_label {
        let label_width = table.width.saturating_sub(2).min(34);
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("YOUR HAND  ", Style::default().fg(MUTED)),
                Span::styled(
                    label,
                    Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
                ),
            ]))
            .alignment(Alignment::Center)
            .style(Style::default().bg(TABLE)),
            Rect::new(
                table.x + table.width.saturating_sub(label_width) / 2,
                table.y + 1,
                label_width,
                1,
            ),
        );
    }
}

#[derive(Debug, PartialEq, Eq)]
struct HandResult {
    headline: String,
    detail: String,
    winning_hand: Option<String>,
    hero_won: bool,
    hero_net: i64,
}

fn hand_result(projection: &TableProjection, hero: SeatId) -> Option<HandResult> {
    let mut totals = BTreeMap::<SeatId, u32>::new();
    for payout in projection.awards.iter().flat_map(|award| &award.payouts) {
        *totals.entry(payout.seat).or_default() += payout.amount;
    }
    if totals.is_empty() {
        return None;
    }

    let hero_total = totals.get(&hero).copied().unwrap_or_default();
    let any_shared = projection
        .awards
        .iter()
        .any(|award| award.winners.len() > 1);
    let headline = if any_shared
        && projection
            .awards
            .iter()
            .all(|award| award.winners.len() > 1)
    {
        "SPLIT POT".to_string()
    } else if totals.len() > 1 && hero_total > 0 {
        "YOU WON A POT".to_string()
    } else if totals.len() > 1 {
        "MULTIPLE POT WINNERS".to_string()
    } else if hero_total > 0 {
        "YOU WIN".to_string()
    } else if totals.len() == 1 {
        let winner = *totals.keys().next().expect("non-empty payouts");
        format!("{} WINS", seat_name(winner, hero).to_ascii_uppercase())
    } else {
        "HAND COMPLETE".to_string()
    };
    let recipients = totals
        .iter()
        .map(|(seat, amount)| {
            if *seat == hero {
                format!("You receive {amount} chips")
            } else {
                format!("Bot {} receives {amount} chips", seat.as_u8())
            }
        })
        .collect::<Vec<_>>();
    let detail = if recipients.len() > 2 {
        format!(
            "{}  ·  +{} more",
            recipients[..2].join("  ·  "),
            recipients.len() - 2
        )
    } else {
        recipients.join("  ·  ")
    };

    // A single winner's evaluated hand is unambiguous. For a genuinely shared
    // pot, show the hand only when every winner's authorized cards are visible
    // and evaluate to the same description. Never infer from hidden cards.
    let shared_winners = projection.awards.first().and_then(|first| {
        (first.winners.len() > 1
            && projection
                .awards
                .iter()
                .all(|award| award.winners == first.winners))
        .then(|| first.winners.clone())
    });
    let winning_seats = if totals.len() == 1 {
        totals.keys().copied().collect::<Vec<_>>()
    } else {
        shared_winners.unwrap_or_default()
    };
    let winning_hand = visible_shared_description(projection, &winning_seats);

    let contribution = projection
        .seats
        .iter()
        .find(|seat| seat.seat == hero)
        .map_or(0, |seat| seat.hand_contribution);
    let total_contributions: u32 = projection
        .seats
        .iter()
        .map(|seat| seat.hand_contribution)
        .sum();
    let total_awards: u32 = projection.awards.iter().map(|award| award.amount).sum();
    let returned = if total_awards < total_contributions {
        let entries = projection
            .seats
            .iter()
            .map(|seat| Contribution {
                seat: seat.seat,
                amount: seat.hand_contribution,
                eligible: matches!(
                    seat.participation,
                    HandParticipation::Live | HandParticipation::AllIn
                ),
            })
            .collect::<Vec<_>>();
        build_pots(&entries)
            .returned
            .iter()
            .filter(|item| item.seat == hero)
            .map(|item| item.amount)
            .sum::<u32>()
    } else {
        0
    };
    let hero_net = i64::from(hero_total) + i64::from(returned) - i64::from(contribution);

    Some(HandResult {
        headline,
        detail,
        winning_hand,
        hero_won: hero_total > 0,
        hero_net,
    })
}

fn visible_shared_description(projection: &TableProjection, winners: &[SeatId]) -> Option<String> {
    if projection.board.len() != 5 || winners.is_empty() {
        return None;
    }
    let descriptions = winners
        .iter()
        .map(|winner| {
            let cards = projection
                .seats
                .iter()
                .find(|seat| seat.seat == *winner)?
                .hole_cards
                .as_deref()?;
            (cards.len() == 2).then(|| evaluate_hand(cards, &projection.board).description)
        })
        .collect::<Option<Vec<_>>>()?;
    descriptions
        .iter()
        .all(|description| description == &descriptions[0])
        .then(|| descriptions[0].clone())
}

fn render_hand_result(frame: &mut Frame<'_>, state: &TableRenderState<'_>, available: Rect) {
    let Some(result) = hand_result(state.projection, state.hero) else {
        return;
    };
    let height = available
        .height
        .min(if result.winning_hand.is_some() { 7 } else { 6 });
    let width = available.width.saturating_sub(8).min(64);
    let area = Rect::new(
        available.x + available.width.saturating_sub(width) / 2,
        available.y,
        width,
        height,
    );
    let accent = if result.hero_won { GREEN } else { TEXT };
    let mut lines = vec![
        Line::from(Span::styled(
            result.headline,
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(result.detail, Style::default().fg(TEXT))),
    ];
    if let Some(description) = result.winning_hand {
        lines.push(Line::from(Span::styled(
            format!("Winning hand · {description}"),
            Style::default().fg(MUTED),
        )));
    }
    lines.push(Line::from(Span::styled(
        format!("Your hand: {:+} chips", result.hero_net),
        Style::default()
            .fg(if result.hero_net >= 0 { GREEN } else { YELLOW })
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(Span::styled(
        "Enter · next hand   V · replay",
        Style::default().fg(TEXT).add_modifier(Modifier::BOLD),
    )));
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Center)
            .style(Style::default().bg(PANEL))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(accent))
                    .style(Style::default().bg(PANEL)),
            ),
        area,
    );
}

fn render_board(frame: &mut Frame<'_>, projection: &TableProjection, table: Rect) {
    let center_y = table.y + table.height / 2;
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("pot ", Style::default().fg(YELLOW)),
            Span::styled(
                projection.pot_total.to_string(),
                Style::default().fg(YELLOW).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  ·  {}", projection.phase.name().to_ascii_uppercase()),
                Style::default().fg(MUTED),
            ),
        ]))
        .alignment(Alignment::Center)
        .style(Style::default().bg(TABLE)),
        Rect::new(table.x + 1, center_y.saturating_sub(3), table.width - 2, 1),
    );
    let slots = if table.width >= 27 { 5 } else { 3 };
    let card_w = if table.width >= 72 && table.height >= 11 {
        7
    } else {
        4
    };
    let card_h = if card_w == 7 { 5 } else { 3 };
    let gap = 1;
    let width = slots * (card_w + gap) - gap;
    let x = table.x + table.width.saturating_sub(width) / 2;
    for index in 0..slots {
        let card_area = Rect::new(
            x + index * (card_w + gap),
            center_y.saturating_sub(card_h / 2),
            card_w,
            card_h,
        );
        if let Some(card) = projection.board.get(index as usize) {
            render_card(frame, card_area, card);
        } else {
            render_empty_card(frame, card_area);
        }
    }
}

fn render_seats(
    frame: &mut Frame<'_>,
    state: &TableRenderState<'_>,
    stage: Rect,
    table: Rect,
    seat_w: u16,
) {
    let opponents = state.projection.table_size.get().saturating_sub(1);
    for (index, seat_id) in (1..state.projection.table_size.get()).enumerate() {
        let rect = opponent_rect(index as u8, opponents, stage, table, seat_w);
        let seat = state
            .projection
            .seats
            .iter()
            .find(|candidate| candidate.seat.as_u8() == seat_id);
        render_seat(frame, state, seat, rect, false, table);
    }
    let hero_h = if stage.width >= 90 && stage.height >= 22 {
        7
    } else {
        5
    };
    let hero_w = if hero_h == 7 { 28 } else { 22 };
    let hero_rect = Rect::new(
        table.x + table.width.saturating_sub(hero_w) / 2,
        stage.y + stage.height.saturating_sub(hero_h),
        hero_w.min(table.width),
        hero_h,
    );
    let hero_seat = state
        .projection
        .seats
        .iter()
        .find(|seat| seat.seat == state.hero);
    render_seat(frame, state, hero_seat, hero_rect, true, table);
}

fn opponent_rect(index: u8, count: u8, stage: Rect, _table: Rect, width: u16) -> Rect {
    #[derive(Clone, Copy)]
    enum Place {
        Top(u8, u8),
        Left(bool),
        Right(bool),
    }
    use Place::{Left, Right, Top};
    let places: &[Place] = match count {
        1 => &[Top(0, 1)],
        2 => &[Top(0, 2), Top(1, 2)],
        3 => &[Left(false), Top(0, 1), Right(false)],
        4 => &[Left(false), Top(0, 2), Top(1, 2), Right(false)],
        5 => &[
            Left(true),
            Left(false),
            Top(0, 1),
            Right(false),
            Right(true),
        ],
        6 => &[
            Left(true),
            Left(false),
            Top(0, 2),
            Top(1, 2),
            Right(false),
            Right(true),
        ],
        7 => &[
            Left(true),
            Left(false),
            Top(0, 3),
            Top(1, 3),
            Top(2, 3),
            Right(false),
            Right(true),
        ],
        _ => &[
            Left(true),
            Left(false),
            Top(0, 4),
            Top(1, 4),
            Top(2, 4),
            Top(3, 4),
            Right(false),
            Right(true),
        ],
    };
    let (x, y) = match places[index as usize] {
        Top(slot, total) => {
            let usable = stage.width.saturating_sub(width);
            let x = if total <= 1 {
                stage.x + usable / 2
            } else {
                stage.x + usable.saturating_mul(u16::from(slot)) / u16::from(total - 1)
            };
            (x, stage.y)
        }
        Left(lower) => (
            stage.x,
            if lower {
                stage.y + stage.height.saturating_sub(8)
            } else {
                stage.y + 6
            },
        ),
        Right(lower) => (
            stage.x + stage.width.saturating_sub(width),
            if lower {
                stage.y + stage.height.saturating_sub(8)
            } else {
                stage.y + 6
            },
        ),
    };
    Rect::new(x, y, width, if stage.height < 20 { 4 } else { 6 })
}

fn render_seat(
    frame: &mut Frame<'_>,
    state: &TableRenderState<'_>,
    seat: Option<&ProjectedSeat>,
    area: Rect,
    hero_seat: bool,
    table: Rect,
) {
    let Some(seat) = seat else { return };
    let active = state.projection.to_act == Some(seat.seat) && state.mode == TableMode::Playing;
    let marker = if seat.seat == state.projection.button {
        " D"
    } else if seat.seat == state.projection.small_blind {
        " SB"
    } else if seat.seat == state.projection.big_blind {
        " BB"
    } else {
        ""
    };
    let name = if hero_seat {
        "you".to_string()
    } else {
        format!("bot {}", seat.seat.as_u8())
    };
    let border = if active { GREEN } else { RULE };
    let title = if hero_seat {
        format!(" {name}{marker} · {} ", seat.stack)
    } else {
        format!(" {name}{marker} ")
    };
    let block = Block::default()
        .title(Span::styled(
            title,
            Style::default()
                .fg(if active { GREEN } else { TEXT })
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(border))
        .style(Style::default().bg(PANEL));
    let inner = block.inner(area);
    frame.render_widget(Clear, area);
    frame.render_widget(block, area);
    if hero_seat {
        if let Some(cards) = &seat.hole_cards {
            let width = if inner.height >= 5 { 7 } else { 5 };
            let height = if inner.height >= 5 { 5 } else { 3 };
            let total = width * 2 + 1;
            let start = inner.x + inner.width.saturating_sub(total) / 2;
            for (index, card) in cards.iter().take(2).enumerate() {
                render_card(
                    frame,
                    Rect::new(start + index as u16 * (width + 1), inner.y, width, height),
                    card,
                );
            }
        }
    } else {
        render_opponent_holding(frame, inner, seat);
    }
    if seat.street_contribution > 0 {
        let chip = format!("● {}", seat.street_contribution);
        let x = if hero_seat {
            area.x + area.width.saturating_sub(chip.len() as u16) / 2
        } else if area.x < table.x {
            area.x + area.width + 1
        } else if area.y < table.y {
            area.x + area.width.saturating_sub(chip.len() as u16) / 2
        } else {
            area.x.saturating_sub(chip.len() as u16 + 1)
        };
        let y = if hero_seat {
            area.y.saturating_sub(1)
        } else if area.y < table.y {
            area.y + area.height
        } else {
            area.y + area.height / 2
        };
        frame.render_widget(
            Paragraph::new(Span::styled(chip, Style::default().fg(YELLOW))),
            Rect::new(x, y, area.width.min(8), 1),
        );
    }
}

fn render_opponent_holding(frame: &mut Frame<'_>, area: Rect, seat: &ProjectedSeat) {
    if let Some(cards) = &seat.hole_cards {
        let mut spans = Vec::new();
        for (index, card) in cards.iter().take(2).enumerate() {
            if index > 0 {
                spans.push(Span::raw(" "));
            }
            spans.push(Span::styled(
                format!(" {} ", card),
                Style::default()
                    .fg(if card.suit.is_red() {
                        CARD_RED
                    } else {
                        CARD_INK
                    })
                    .bg(CARD_FACE)
                    .add_modifier(Modifier::BOLD),
            ));
        }
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled(
                    seat.stack.to_string(),
                    Style::default().fg(TEXT),
                )),
                Line::from(""),
                Line::from(spans),
            ])
            .alignment(Alignment::Center),
            area,
        );
        return;
    }
    if matches!(
        seat.participation,
        HandParticipation::Live | HandParticipation::AllIn
    ) {
        frame.render_widget(
            Paragraph::new(Span::styled(
                seat.stack.to_string(),
                Style::default().fg(TEXT),
            ))
            .alignment(Alignment::Center),
            Rect::new(area.x, area.y, area.width, 1),
        );
        let card_width = 4;
        let total = card_width * 2 + 1;
        let start = area.x + area.width.saturating_sub(total) / 2;
        for index in 0..2 {
            render_card_back(
                frame,
                Rect::new(
                    start + index * (card_width + 1),
                    area.y + 1,
                    card_width,
                    area.height.saturating_sub(1).min(3),
                ),
            );
        }
    } else {
        let status = if seat.participation == HandParticipation::Folded {
            "folded"
        } else {
            "waiting"
        };
        let lines = if area.height < 3 {
            vec![
                Line::from(Span::styled(
                    seat.stack.to_string(),
                    Style::default().fg(TEXT),
                )),
                Line::from(Span::styled(status, Style::default().fg(MUTED))),
            ]
        } else {
            vec![
                Line::from(Span::styled(
                    seat.stack.to_string(),
                    Style::default().fg(TEXT),
                )),
                Line::from(""),
                Line::from(Span::styled(status, Style::default().fg(MUTED))),
            ]
        };
        frame.render_widget(Paragraph::new(lines).alignment(Alignment::Center), area);
    }
}

fn render_card_back(frame: &mut Frame<'_>, area: Rect) {
    let style = Style::default().fg(Color::Rgb(116, 117, 128)).bg(CARD_BACK);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(" ▪  ", style)),
            Line::from(Span::styled("  ▪ ", style)),
            Line::from(Span::styled(" ▪  ", style)),
        ]),
        area,
    );
}

fn render_card(frame: &mut Frame<'_>, area: Rect, card: &Card) {
    let rank = if card.rank == crate::game::deck::Rank::Ten {
        "10"
    } else {
        card.rank.symbol()
    };
    let ink = if card.suit.is_red() {
        CARD_RED
    } else {
        CARD_INK
    };
    let face = Style::default()
        .fg(ink)
        .bg(CARD_FACE)
        .add_modifier(Modifier::BOLD);
    let width = usize::from(area.width);
    let rank_width = Line::from(rank).width();
    let mut lines = vec![
        Line::from(vec![
            Span::styled(rank.to_string(), face),
            Span::styled(" ".repeat(width.saturating_sub(rank_width)), face),
        ]),
        Line::from(Span::styled(
            format!(
                "{}{}{}",
                " ".repeat(width.saturating_sub(1) / 2),
                card.suit.symbol(),
                " ".repeat(width.saturating_sub(width.saturating_sub(1) / 2 + 1))
            ),
            face,
        )),
        Line::from(vec![
            Span::styled(" ".repeat(width.saturating_sub(rank_width)), face),
            Span::styled(rank.to_string(), face),
        ]),
    ];
    if area.height >= 5 {
        lines.insert(2, Line::from(Span::styled(" ".repeat(width), face)));
        lines.insert(3, Line::from(Span::styled(" ".repeat(width), face)));
    }
    frame.render_widget(Paragraph::new(lines), area);
}

fn render_empty_card(frame: &mut Frame<'_>, area: Rect) {
    let style = Style::default().fg(Color::Rgb(82, 82, 82)).bg(TABLE);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled("┌──┐", style)),
            Line::from(Span::styled("│  │", style)),
            Line::from(Span::styled("└──┘", style)),
        ]),
        area,
    );
}

fn render_rail(frame: &mut Frame<'_>, state: &TableRenderState<'_>, area: Rect) {
    frame.render_widget(
        Block::default()
            .borders(Borders::LEFT)
            .border_style(Style::default().fg(RULE))
            .style(Style::default().bg(BG)),
        area,
    );
    let inner = Rect::new(
        area.x + 2,
        area.y,
        area.width.saturating_sub(3),
        area.height,
    );
    let mut lines = vec![
        Line::from(Span::styled("ACTION", Style::default().fg(MUTED))),
        Line::from(Span::styled(
            format!("hand #{}", state.hand_id),
            Style::default().fg(YELLOW).add_modifier(Modifier::BOLD),
        )),
    ];
    if state.mode == TableMode::Paused {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "TABLE BEFORE ACTION",
            Style::default().fg(GREEN).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(Span::styled(
            "Pot, stacks and cards are frozen for review.",
            Style::default().fg(TEXT),
        )));
    } else if let Some(title) = state.notice_title {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            title,
            Style::default().fg(GREEN).add_modifier(Modifier::BOLD),
        )));
    }
    if state.mode != TableMode::Paused {
        if let Some(notice) = state.notice {
            for line in notice.lines() {
                lines.push(Line::from(Span::styled(
                    line.to_string(),
                    Style::default().fg(TEXT),
                )));
            }
        } else if !state.status.is_empty() {
            lines.push(Line::from(Span::styled(
                state.status,
                Style::default().fg(TEXT),
            )));
        }
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "HISTORY",
        Style::default().fg(MUTED),
    )));
    lines.extend([
        Line::from(Span::styled(
            format!(
                "{} posts SB {}",
                seat_name(state.projection.small_blind, state.hero),
                state.projection.small_blind_amount
            ),
            Style::default().fg(MUTED),
        )),
        Line::from(Span::styled(
            format!(
                "{} posts BB {}",
                seat_name(state.projection.big_blind, state.hero),
                state.projection.big_blind_amount
            ),
            Style::default().fg(MUTED),
        )),
    ]);
    for action in state.recent_actions.iter().rev().take(5).rev() {
        lines.push(Line::from(Span::styled(
            action.clone(),
            Style::default().fg(TEXT),
        )));
    }
    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .style(Style::default().bg(BG)),
        inner,
    );
}

fn render_controls(frame: &mut Frame<'_>, state: &TableRenderState<'_>, area: Rect) {
    if let Some(raise) = state.raise {
        render_raise(frame, raise, area);
        return;
    }
    if state.mode == TableMode::Complete {
        render_hand_result(frame, state, area);
        return;
    }
    let hero_turn = state.projection.to_act == Some(state.hero) && state.mode == TableMode::Playing;
    let banner = Rect::new(area.x, area.y, area.width, 1);
    let label = match state.mode {
        TableMode::Paused => "  HAND PAUSED · ENTER TO CONTINUE  ".to_string(),
        TableMode::Complete => "  HAND COMPLETE · ENTER FOR NEXT HAND  ".to_string(),
        TableMode::Playing if hero_turn => "  ▸ YOUR TURN ◂  ".to_string(),
        TableMode::Playing => state
            .projection
            .to_act
            .map(|seat| format!("  BOT {} IS THINKING  ", seat.as_u8()))
            .unwrap_or_else(|| "  DEALER IS SETTLING THE HAND  ".into()),
    };
    frame.render_widget(
        Paragraph::new(Span::styled(
            label,
            Style::default()
                .fg(if hero_turn { Color::Black } else { TEXT })
                .bg(if hero_turn { GREEN } else { PANEL })
                .add_modifier(Modifier::BOLD),
        ))
        .alignment(Alignment::Center),
        banner,
    );
    let buttons = Layout::horizontal([
        Constraint::Percentage(28),
        Constraint::Length(1),
        Constraint::Percentage(35),
        Constraint::Length(1),
        Constraint::Min(20),
    ])
    .horizontal_margin(5)
    .split(Rect::new(area.x, area.y + 1, area.width, 3));
    let legal = state.projection.legal_actions.as_ref();
    let call = legal.and_then(|l| l.call_amount);
    let can_fold = hero_turn && legal.is_some_and(|legal| legal.can_fold);
    let can_check_call = hero_turn
        && legal.is_some_and(|legal| {
            legal.can_check || legal.call_amount.is_some() || legal.can_all_in()
        });
    let can_raise =
        hero_turn && legal.is_some_and(|legal| legal.min_raise_to.or(legal.min_bet_to).is_some());
    action_button(frame, buttons[0], "f  FOLD", RED, can_fold);
    action_button(
        frame,
        buttons[2],
        &if legal.is_some_and(|legal| legal.can_check) {
            "c  CHECK".into()
        } else if let Some(amount) = call {
            format!("c  CALL {amount}")
        } else {
            "c  ALL-IN".into()
        },
        TEAL,
        can_check_call,
    );
    let wager_label = if legal.is_some_and(|legal| legal.min_bet_to.is_some()) {
        "r  BET"
    } else {
        "r  RAISE"
    };
    action_button(frame, buttons[4], wager_label, MUSTARD, can_raise);
}

fn action_button(frame: &mut Frame<'_>, area: Rect, label: &str, color: Color, enabled: bool) {
    frame.render_widget(
        Paragraph::new(Span::styled(
            label,
            Style::default()
                .fg(if enabled { Color::White } else { MUTED })
                .bg(if enabled { color } else { PANEL })
                .add_modifier(Modifier::BOLD),
        ))
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(if enabled { color } else { RULE }))
                .style(Style::default().bg(if enabled { color } else { PANEL })),
        ),
        area,
    );
}

fn render_raise(frame: &mut Frame<'_>, raise: RaiseView, area: Rect) {
    let overlay = Rect::new(
        area.x + 4,
        area.y,
        area.width.saturating_sub(8),
        area.height,
    );
    frame.render_widget(Clear, overlay);
    let mut preset_spans = Vec::new();
    let labels = ["min", "½ pot", "¾ pot", "pot", "all-in"];
    for (index, amount) in raise.presets.iter().enumerate() {
        let selected = *amount == raise.amount;
        preset_spans.push(Span::styled(
            format!(" {} {} {} ", index + 1, labels[index], amount),
            Style::default()
                .fg(if selected { Color::Black } else { MUTED })
                .bg(if selected { TEAL } else { PANEL })
                .add_modifier(if selected {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                }),
        ));
        preset_spans.push(Span::raw(" "));
    }
    let range = raise.maximum.saturating_sub(raise.minimum).max(1);
    let offset = raise.amount.saturating_sub(raise.minimum).min(range);
    let filled = usize::try_from(offset.saturating_mul(30) / range).unwrap_or(0);
    let slider = format!(
        "{}●{}",
        "━".repeat(filled),
        "─".repeat(30usize.saturating_sub(filled))
    );
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                if raise.is_bet {
                    "BET TO · total street chips"
                } else {
                    "RAISE TO · total street chips"
                },
                Style::default().fg(MUTED),
            )),
            Line::from(preset_spans),
            Line::from(vec![
                Span::styled(
                    format!("min {}  ", raise.minimum),
                    Style::default().fg(MUTED),
                ),
                Span::styled(slider, Style::default().fg(YELLOW)),
                Span::styled(
                    format!("  all-in {}", raise.maximum),
                    Style::default().fg(MUTED),
                ),
            ]),
            Line::from(vec![
                Span::styled("arrows ±1/10 · T type  ", Style::default().fg(TEAL)),
                Span::styled(
                    format!("raise to {}", raise.amount),
                    Style::default().fg(YELLOW).add_modifier(Modifier::BOLD),
                ),
                Span::styled("  Enter confirm · Esc cancel", Style::default().fg(MUTED)),
            ]),
        ])
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(RULE))
                .style(Style::default().bg(PANEL)),
        ),
        overlay,
    );
}

fn seat_name(seat: SeatId, hero: SeatId) -> String {
    if seat == hero {
        "you".into()
    } else {
        format!("bot {}", seat.as_u8())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        game::{
            actions::Action,
            deck::{Rank, Suit},
            multiway::{MultiwayPhase, PotAward, SeatPayout},
            seat::TableSize,
        },
        protocol::{HandId, ProjectionKind},
        trainer::{hero, policy::PolicySettings, storage::Settings, Session},
    };
    use ratatui::{backend::TestBackend, Terminal};

    fn seat(index: u8) -> SeatId {
        SeatId::new(index).unwrap()
    }

    fn card(rank: Rank, suit: Suit) -> Card {
        Card::new(rank, suit)
    }

    fn projection(awards: Vec<PotAward>, bot_cards: Option<Vec<Card>>) -> TableProjection {
        TableProjection {
            showdown: None,
            mucked: vec![],
            shown: vec![],
            always_show: false,
            hand_id: HandId(7),
            audience: ProjectionKind::Player { seat: seat(0) },
            table_size: TableSize::new(2).unwrap(),
            phase: MultiwayPhase::HandComplete,
            button: seat(0),
            small_blind: seat(0),
            big_blind: seat(1),
            small_blind_amount: 1,
            big_blind_amount: 2,
            ante_amount: 0,
            to_act: None,
            board: vec![
                card(Rank::Ace, Suit::Spades),
                card(Rank::King, Suit::Hearts),
                card(Rank::Queen, Suit::Clubs),
                card(Rank::Jack, Suit::Diamonds),
                card(Rank::Two, Suit::Spades),
            ],
            current_wager: 0,
            pot_total: awards.iter().map(|award| award.amount).sum(),
            seats: vec![
                ProjectedSeat {
                    seat: seat(0),
                    stack: 120,
                    street_contribution: 0,
                    hand_contribution: 20,
                    participation: HandParticipation::Live,
                    hole_cards: Some(vec![
                        card(Rank::Ten, Suit::Spades),
                        card(Rank::Nine, Suit::Spades),
                    ]),
                },
                ProjectedSeat {
                    seat: seat(1),
                    stack: 80,
                    street_contribution: 0,
                    hand_contribution: 20,
                    participation: HandParticipation::Live,
                    hole_cards: bot_cards,
                },
            ],
            pots: vec![],
            awards,
            legal_actions: None,
        }
    }

    fn award(amount: u32, winners: &[u8], payouts: &[(u8, u32)]) -> PotAward {
        PotAward {
            pot_index: 0,
            amount,
            eligible: vec![seat(0), seat(1)],
            winners: winners.iter().copied().map(seat).collect(),
            payouts: payouts
                .iter()
                .map(|(winner, amount)| SeatPayout {
                    seat: seat(*winner),
                    amount: *amount,
                })
                .collect(),
        }
    }

    fn rendered(projection: &TableProjection, tone: Option<ReviewTone>) -> String {
        let state = TableRenderState {
            projection,
            hero: seat(0),
            hand_id: 7,
            recent_actions: &[],
            status: "",
            mode: if tone.is_some() {
                TableMode::Paused
            } else {
                TableMode::Complete
            },
            notice_title: tone.map(|_| "REVIEW"),
            notice: tone.map(|_| "BEFORE ACTION hero called\nWHY price was poor\nCONSIDER folding"),
            raise: None,
            hand_label: None,
            review_tone: tone,
            guidance_source: Some("heuristic guidance"),
            active_coaching: "local coaching",
        };
        let mut terminal = Terminal::new(TestBackend::new(80, 30)).unwrap();
        terminal.draw(|frame| render(frame, &state)).unwrap();
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>()
    }

    #[test]
    fn minimum_viewport_makes_hero_win_prominent() {
        let projection = projection(vec![award(40, &[0], &[(0, 40)])], None);
        let text = rendered(&projection, None);
        let winning_hand = hand_result(&projection, seat(0))
            .unwrap()
            .winning_hand
            .unwrap();
        assert!(text.contains("YOU WIN"));
        assert!(text.contains("You receive 40 chips"));
        assert!(text.contains(&format!("Winning hand · {winning_hand}")));
        assert!(text.contains("Your hand: +20 chips"));
        assert!(text.contains("Enter · next hand"));
    }

    #[test]
    fn loss_does_not_leak_an_unshown_winning_hand() {
        let projection = projection(vec![award(40, &[1], &[(1, 40)])], None);
        let text = rendered(&projection, None);
        assert!(text.contains("BOT 1 WINS"));
        assert!(text.contains("Bot 1 receives 40 chips"));
        assert!(!text.contains("Winning hand"));
        assert!(text.contains("Your hand: -20 chips"));
    }

    #[test]
    fn fold_win_omits_an_unavailable_winning_hand() {
        let mut projection = projection(vec![award(12, &[0], &[(0, 12)])], None);
        projection.board.truncate(3);
        let text = rendered(&projection, None);
        assert!(text.contains("YOU WIN"));
        assert!(text.contains("You receive 12 chips"));
        assert!(!text.contains("Winning hand"));
    }

    #[test]
    fn unmatched_excess_is_included_in_hand_net() {
        let mut projection = projection(vec![award(80, &[0], &[(0, 80)])], None);
        projection.seats[0].hand_contribution = 100;
        projection.seats[1].hand_contribution = 40;
        let result = hand_result(&projection, seat(0)).unwrap();
        assert_eq!(result.hero_net, 40);
    }

    #[test]
    fn settled_seeded_hands_match_engine_stack_change() {
        for seats in [2, 3, 6] {
            for style in 0..3 {
                for seed in [1, 7, 23, 41] {
                    let settings = Settings {
                        seats,
                        opponents: PolicySettings {
                            aggression: 0.0,
                            bluff_rate: 0.0,
                            mistake_rate: 0.0,
                            ..PolicySettings::default()
                        },
                        ..Settings::default()
                    };
                    let mut session = Session::new_seeded_for_evaluation(settings, seed).unwrap();
                    for _ in 0..256 {
                        if session.finished() {
                            break;
                        }
                        if session.view().to_act == Some(hero()) {
                            let observation = session.observation(hero()).unwrap();
                            let action = match style {
                                1 if observation.legal.can_fold => Action::Fold,
                                2 if observation.legal.can_all_in() => {
                                    Action::AllIn(observation.legal.all_in_to)
                                }
                                _ => observation.check_call(),
                            };
                            session.submit(action).unwrap();
                            session.continue_hand();
                        } else {
                            assert!(
                                session.step_bot().unwrap(),
                                "no actor advanced for {seats} seats, style {style}, seed {seed}"
                            );
                        }
                    }
                    assert!(
                        session.finished(),
                        "hand did not settle for {seats} seats, style {style}, seed {seed}"
                    );
                    let final_projection = session.view();
                    let final_stack = final_projection
                        .seats
                        .iter()
                        .find(|seat| seat.seat == hero())
                        .unwrap()
                        .stack;
                    let expected = i64::from(final_stack) - i64::from(session.opening_hero_stack);
                    let result = hand_result(&final_projection, hero()).unwrap();
                    assert_eq!(
                        result.hero_net, expected,
                        "{seats} seats, style {style}, seed {seed}; awards={:?}",
                        final_projection.awards
                    );
                }
            }
        }
    }

    #[test]
    fn expanded_review_preserves_cards_at_common_terminal_sizes() {
        let projection = projection(vec![], None);
        for (width, height) in [(80, 30), (120, 40), (165, 47)] {
            let state = TableRenderState {
                projection: &projection, hero: seat(0), hand_id: 7, recent_actions: &[],
                status: "", mode: TableMode::Paused, notice_title: Some("REVIEW"),
                notice: Some("BEFORE ACTION you checked\nWHY a careful check controls the pot\nCONSIDER a bet"),
                raise: None, hand_label: Some("Broadway straight"), review_tone: Some(ReviewTone::Good),
                guidance_source: Some("solver review"), active_coaching: "local review",
            };
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|frame| render(frame, &state)).unwrap();
            let before = terminal.backend().buffer().clone();
            terminal.draw(|frame| {
                render(frame,&state);
                render_coaching_details(frame," AFTER YOUR DECISION · SOLVER ",
                    "YOUR CHOICE\nCheck: the board and your cards remain visible.\nMODEL\nRanges and bet sizes are assumptions.\nScroll for more detail.",0);
            }).unwrap();
            let after = terminal.backend().buffer();
            let shell_width = width.min(128);
            let shell_x = (width - shell_width) / 2;
            let rail_width = if shell_width >= 110 { 28 } else { 23 };
            let stage_right = shell_x + shell_width - rail_width;
            let shell_y = (height - height.min(38)) / 2;
            for y in shell_y + 3..shell_y + height.min(38) - 9 {
                for x in shell_x..stage_right {
                    assert_eq!(
                        before[(x, y)].symbol(),
                        after[(x, y)].symbol(),
                        "table cell changed at {width}x{height} ({x},{y})"
                    );
                }
            }
            let visible = after
                .content
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>();
            assert!(
                visible.contains("10"),
                "hero ten card missing at {width}x{height}"
            );
            assert!(
                visible.contains("SOLVER REVIEW"),
                "detail pane missing at {width}x{height}"
            );
            assert!(
                visible.contains("GOOD DECISION"),
                "compact grade missing at {width}x{height}"
            );
            assert!(
                visible.contains("Enter continue"),
                "continue cue missing at {width}x{height}"
            );
            let long_body = format!(
                "{}\nENDVISIBLE",
                "A detailed coaching line that wraps in the pane.\n".repeat(40)
            );
            terminal
                .draw(|frame| {
                    render(frame, &state);
                    render_coaching_details(frame, " REVIEW ", &long_body, u16::MAX);
                })
                .unwrap();
            let at_end = terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>();
            assert!(
                at_end.contains("ENDVISIBLE"),
                "last explanation line unreachable at {width}x{height}"
            );
        }
    }

    #[test]
    fn shared_pot_is_distinct_from_separate_side_pot_winners() {
        let split = projection(vec![award(41, &[0, 1], &[(0, 21), (1, 20)])], None);
        assert!(rendered(&split, None).contains("SPLIT POT"));

        let mut main = award(60, &[1], &[(1, 60)]);
        main.pot_index = 0;
        let mut side = award(20, &[0], &[(0, 20)]);
        side.pot_index = 1;
        let side_pots = projection(vec![main, side], None);
        let text = rendered(&side_pots, None);
        assert!(text.contains("YOU WON A POT"));
        assert!(text.contains("You receive 20 chips"));
        assert!(text.contains("Bot 1 receives 60 chips"));
        assert!(!text.contains("SPLIT POT"));
        assert!(!text.contains("Winning hand"));
    }

    #[test]
    fn questionable_decision_badge_is_explicit_at_minimum_size() {
        let projection = projection(vec![award(40, &[1], &[(1, 40)])], None);
        let text = rendered(&projection, Some(ReviewTone::Reconsider));
        assert!(text.contains("QUESTIONABLE DECISION"));
        assert!(text.contains("CONSIDER folding"));
    }

    #[test]
    fn compact_why_uses_two_width_aware_lines_and_marks_overflow() {
        let short = wrapped_excerpt("WHY one useful sentence", 40, 2);
        assert_eq!(short, "WHY one useful sentence");

        let long = wrapped_excerpt(
            "WHY this explanation contains enough strategic context to exceed the available coaching width without consuming the alternative row",
            32,
            2,
        );
        assert_eq!(long.lines().count(), 2);
        assert!(long.ends_with('…'));
        assert!(long.lines().all(|line| Line::from(line).width() <= 32));

        let unbroken = wrapped_excerpt(&format!("WHY {}", "x".repeat(100)), 12, 2);
        assert_eq!(unbroken.lines().count(), 2);
        assert!(unbroken.ends_with('…'));
        assert!(unbroken.lines().all(|line| Line::from(line).width() <= 12));

        let wide = wrapped_excerpt("WHY 手牌选择需要谨慎考虑位置和价格", 14, 2);
        assert!(wide.ends_with('…'));
        assert!(wide.lines().all(|line| Line::from(line).width() <= 14));
    }
}
