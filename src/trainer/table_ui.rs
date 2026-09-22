//! Purpose-built OpenFelt practice-table renderer.
//!
//! This module accepts only the hero's authorized projection and display copy.
//! It owns geometry and styling, never game state or input handling.

use crate::{
    game::{deck::Card, seat::SeatId, table::HandParticipation},
    protocol::{ProjectedSeat, TableProjection},
};
use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap},
    Frame,
};

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

#[derive(Clone, Copy, Debug)]
pub struct RaiseView {
    pub amount: u32,
    pub minimum: u32,
    pub maximum: u32,
    pub presets: [u32; 5],
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

    let rows = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(20),
        Constraint::Length(6),
        Constraint::Length(1),
    ])
    .split(area);
    render_header(frame, state, rows[0]);
    let body = Layout::horizontal([Constraint::Min(56), Constraint::Length(23)]).split(rows[1]);
    render_stage(frame, state, body[0]);
    render_rail(frame, state, body[1]);
    render_controls(frame, state, rows[2]);
    let footer =
        Layout::horizontal([Constraint::Percentage(35), Constraint::Percentage(65)]).split(rows[3]);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("● ", Style::default().fg(GREEN)),
            Span::styled("local", Style::default().fg(MUTED)),
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

/// Expanded coaching keeps the hero cards and controls visible below a wide,
/// readable teaching panel.
pub fn render_coaching_details(frame: &mut Frame<'_>, title: &str, body: &str, scroll: u16) {
    let area = frame.area();
    let panel = Rect::new(
        area.x + 5,
        area.y + 3,
        area.width.saturating_sub(10),
        area.height.saturating_sub(15),
    );
    frame.render_widget(Clear, panel);
    frame.render_widget(
        Paragraph::new(body)
            .scroll((
                scroll.min(coaching_max_scroll(body, panel.width, panel.height)),
                0,
            ))
            .style(Style::default().fg(TEXT).bg(BG))
            .wrap(Wrap { trim: true })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(title)
                    .border_style(Style::default().fg(GREEN))
                    .style(Style::default().bg(BG)),
            ),
        panel,
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
        let len = word.chars().count();
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
    let seat_w = if area.width >= 70 { 14 } else { 12 };
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
    let width = slots * 5 - 1;
    let x = table.x + table.width.saturating_sub(width) / 2;
    for index in 0..slots {
        let card_area = Rect::new(x + index * 5, center_y.saturating_sub(1), 4, 3);
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
    let hero_rect = Rect::new(
        table.x + table.width.saturating_sub(22) / 2,
        stage.y + stage.height.saturating_sub(5),
        22.min(table.width),
        5,
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
    Rect::new(x, y, width, 6)
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
            let width = 5;
            let total = width * 2 + 1;
            let start = inner.x + inner.width.saturating_sub(total) / 2;
            for (index, card) in cards.iter().take(2).enumerate() {
                render_card(
                    frame,
                    Rect::new(start + index as u16 * (width + 1), inner.y, width, 3),
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
                Rect::new(start + index * (card_width + 1), area.y + 1, card_width, 3),
            );
        }
    } else {
        let status = if seat.participation == HandParticipation::Folded {
            "folded"
        } else {
            "waiting"
        };
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled(
                    seat.stack.to_string(),
                    Style::default().fg(TEXT),
                )),
                Line::from(""),
                Line::from(Span::styled(status, Style::default().fg(MUTED))),
            ])
            .alignment(Alignment::Center),
            area,
        );
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
        "T"
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
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled(rank.to_string(), face),
                Span::styled(" ".repeat(width.saturating_sub(1)), face),
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
                Span::styled(" ".repeat(width.saturating_sub(1)), face),
                Span::styled(rank.to_string(), face),
            ]),
        ]),
        area,
    );
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
    if let Some(title) = state.notice_title {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            title,
            Style::default().fg(GREEN).add_modifier(Modifier::BOLD),
        )));
    }
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
    action_button(frame, buttons[4], "r  RAISE", MUSTARD, can_raise);
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
    for (index, amount) in raise.presets.iter().enumerate() {
        let selected = *amount == raise.amount;
        preset_spans.push(Span::styled(
            format!(" {} {} ", index + 1, amount),
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
            Line::from(Span::styled("RAISE TO", Style::default().fg(MUTED))),
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
