//! Rendering: a pure projection of [`App`] state onto a [`Frame`].
//!
//! Nothing here mutates domain state (the lone `&mut` — the navigator's
//! `ListState` — is widget-internal presentation state). Every decision was
//! already made in [`crate::app`]; this module only draws it. All colors come
//! from [`theme`]; widgets never hardcode a `Color`.

mod formula;
pub mod theme;

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, List, ListItem, Paragraph, Wrap};

use crate::app::{App, Focus, NavRow};
use crate::engine::Verdict;
use crate::storage::problem::{Difficulty, Domain};

/// Renders the entire UI for one frame.
///
/// ```text
///  🐍 mamba · verify your maths                      6 problems  ● ● ●
/// ╭ Domains ────╮╭ Problem ─────────────────────────────────────────╮
/// │ ◆ Symbolic  ││ Expand a Binomial          [Easy]  Algebra       │
/// │   ● Expand… ││                                                   │
/// │ ◆ Tensor    ││ Expand (x − 3)(x + 2).                            │
/// │   ● Determ… │╰───────────────────────────────────────────────────╯
/// │ ◆ Exact     │╭ Verification ────────────────────────────────────╮
/// │   ● Factor… ││ ✔ Correct — equivalent. exported → …/Expand.md    │
/// ╰─────────────╯╰───────────────────────────────────────────────────╯
/// ╭ Answer · Symbolic ───────────────────────────────────────────────╮
/// │ › (x-3)(x+2)▌                                                     │
/// ╰───────────────────────────────────────────────────────────────────╯
///  / find  ⇥ focus  ↵ submit  esc nav  ^Q quit                CORRECT
/// ```
pub fn draw(frame: &mut Frame, app: &mut App) {
    // Framing chrome painted behind everything.
    frame.render_widget(
        ratatui::widgets::Block::default().style(theme::canvas()),
        frame.area(),
    );

    let [topbar, main, input_bar, footer] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // brand / count
            Constraint::Min(0),    // sidebar + (description / console)
            Constraint::Length(3), // bottom-anchored answer bar
            Constraint::Length(1), // key hints / status badge
        ])
        .areas(frame.area());

    let [sidebar, center] = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(30), Constraint::Min(0)])
        .areas(main);

    let [description, console] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(62), Constraint::Percentage(38)])
        .areas(center);

    draw_topbar(frame, app, topbar);
    draw_navigator(frame, app, sidebar);
    draw_description(frame, app, description);
    draw_console(frame, app, console);
    draw_input(frame, app, input_bar);
    draw_footer(frame, app, footer);

    // A modal overlay floats over everything (the two are mutually exclusive).
    if app.search_active {
        draw_search_overlay(frame, app, frame.area());
    } else if app.stats_visible {
        draw_stats_overlay(frame, app, frame.area());
    }
}

/// The progress/stats overlay (`Ctrl-P`): a centered panel with totals, streaks,
/// per-domain and per-difficulty bars, and recent solves — computed from the
/// vault via [`crate::storage::progress`].
fn draw_stats_overlay(frame: &mut Frame, app: &App, area: Rect) {
    let pal = theme::palette();
    let w = (area.width * 7 / 10).clamp(40, 74);
    let h = (area.height * 8 / 10).clamp(12, 30);
    let popup = centered_rect(w, h, area);

    frame.render_widget(Clear, popup);
    let block = Block::default()
        .title(Span::styled(
            "  📈  Progress  ",
            Style::default()
                .fg(pal.accent_deep)
                .add_modifier(Modifier::BOLD),
        ))
        .title_bottom(Span::styled(
            "  ^P / esc close  ",
            Style::default().fg(pal.overlay),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(pal.accent))
        .style(Style::default().bg(pal.base));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let Some(stats) = &app.stats else {
        return;
    };

    let mut lines: Vec<Line> = Vec::new();
    if stats.total == 0 {
        lines.push(Line::from(Span::styled(
            " No solves yet — answer a problem to start a streak.",
            Style::default().fg(pal.overlay),
        )));
    } else {
        lines.push(Line::from(vec![
            Span::styled(
                format!(" {} solved", stats.total),
                Style::default().fg(pal.text).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("    🔥 streak {}", stats.current_streak),
                Style::default().fg(pal.green),
            ),
            Span::styled(
                format!("    best {}", stats.longest_streak),
                Style::default().fg(pal.subtext),
            ),
        ]));
        lines.push(Line::default());
        lines.push(section(" By domain"));
        let dmax = stats.by_domain.iter().map(|(_, c)| *c).max().unwrap_or(0);
        for (label, count) in &stats.by_domain {
            lines.push(bar_line(
                label,
                *count,
                dmax,
                domain_color_by_name(label),
                9,
            ));
        }
        lines.push(Line::default());
        lines.push(section(" By difficulty"));
        let fmax = stats
            .by_difficulty
            .iter()
            .map(|(_, c)| *c)
            .max()
            .unwrap_or(0);
        for (label, count) in &stats.by_difficulty {
            lines.push(bar_line(
                label,
                *count,
                fmax,
                difficulty_color_by_name(label),
                9,
            ));
        }
        if !stats.by_topic.is_empty() {
            lines.push(Line::default());
            lines.push(section(" By topic"));
            let tmax = stats.by_topic.iter().map(|(_, c)| *c).max().unwrap_or(0);
            for (i, (label, count)) in stats.by_topic.iter().take(8).enumerate() {
                lines.push(bar_line(label, *count, tmax, topic_color(i), 19));
            }
        }
        if !stats.recent.is_empty() {
            lines.push(Line::default());
            lines.push(section(" Recently solved"));
            for r in stats.recent.iter().take(6) {
                lines.push(Line::from(vec![
                    Span::styled(
                        format!("  {}  ", r.date.format("%m-%d")),
                        Style::default().fg(pal.overlay),
                    ),
                    Span::styled(r.title.clone(), Style::default().fg(pal.text)),
                    Span::styled(
                        format!("  · {}", r.domain),
                        Style::default().fg(domain_color_by_name(&r.domain)),
                    ),
                ]));
            }
        }
    }

    frame.render_widget(
        Paragraph::new(lines)
            .style(Style::default().bg(pal.base))
            .wrap(Wrap { trim: false }),
        inner,
    );
}

/// A bold section label inside the stats overlay.
fn section(label: &str) -> Line<'static> {
    Line::from(Span::styled(
        label.to_string(),
        Style::default()
            .fg(theme::palette().subtext)
            .add_modifier(Modifier::BOLD),
    ))
}

/// One left-padded label + count with a proportional bar (longest count fills
/// `BAR`). `width` pads the label so a column of bars line up.
fn bar_line(
    label: &str,
    count: usize,
    max: usize,
    color: ratatui::style::Color,
    width: usize,
) -> Line<'static> {
    const BAR: usize = 20;
    let pal = theme::palette();
    let filled = if max > 0 {
        (count * BAR).div_ceil(max)
    } else {
        0
    };
    Line::from(vec![
        Span::styled(
            format!(
                " {label:lw$} {count:>3}  ",
                label = label,
                count = count,
                lw = width
            ),
            Style::default().fg(pal.subtext),
        ),
        Span::styled("█".repeat(filled), Style::default().fg(color)),
    ])
}

/// A rotating accent for the (dynamic, uncoloured) topic bars.
fn topic_color(i: usize) -> ratatui::style::Color {
    let p = theme::palette();
    [
        p.accent,
        p.blue,
        p.teal,
        p.green,
        p.yellow,
        p.accent_deep,
        p.red,
    ][i % 7]
}

/// Domain accent by name (the overlay works from `&str` labels, not the enum).
fn domain_color_by_name(name: &str) -> ratatui::style::Color {
    let p = theme::palette();
    match name {
        "Tensor" => p.blue,
        "Exact" => p.teal,
        _ => p.accent, // Symbolic
    }
}

/// Difficulty accent by name.
fn difficulty_color_by_name(name: &str) -> ratatui::style::Color {
    let p = theme::palette();
    match name {
        "Medium" => p.yellow,
        "Hard" => p.red,
        _ => p.green, // Easy
    }
}

/// A `Rect` of the given proportions, centered within `area`.
fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let w = width.min(area.width);
    let h = height.min(area.height);
    Rect {
        x: area.x + (area.width.saturating_sub(w)) / 2,
        y: area.y + (area.height.saturating_sub(h)) / 2,
        width: w,
        height: h,
    }
}

/// The Spotlight-style fuzzy finder: a centered floating panel with a query
/// line and a full-width, best-first ranked list of matching problems.
fn draw_search_overlay(frame: &mut Frame, app: &mut App, area: Rect) {
    let pal = theme::palette();
    let w = (area.width * 8 / 10).clamp(48, 96);
    let h = (area.height * 8 / 10).clamp(10, 32);
    let popup = centered_rect(w, h, area);

    // Blank the area, then frame it.
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .title(Span::styled(
            "  ⌕  Find a problem  ",
            Style::default()
                .fg(pal.accent_deep)
                .add_modifier(Modifier::BOLD),
        ))
        .title_bottom(Span::styled(
            "  ↑↓ move · ↵ open · esc cancel  ",
            Style::default().fg(pal.overlay),
        ))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(pal.accent))
        .style(Style::default().bg(pal.base));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let [query_area, list_area] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(0)])
        .areas(inner);

    // Query line: a prompt + the live query + cursor, with a match count.
    let [prompt_area, count_area] = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(0), Constraint::Length(14)])
        .areas(query_area);
    let prompt = Line::from(vec![
        Span::styled(" › ", Style::default().fg(pal.accent)),
        Span::styled(app.search_query.clone(), Style::default().fg(pal.text)),
        Span::styled("▌", Style::default().fg(pal.accent)),
    ]);
    frame.render_widget(
        Paragraph::new(prompt).style(Style::default().bg(pal.base)),
        prompt_area,
    );
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            format!("{} ", app.filtered.len()),
            Style::default().fg(pal.subtext),
        )))
        .alignment(Alignment::Right)
        .style(Style::default().bg(pal.base)),
        count_area,
    );

    // Full-width result rows: dot · title · topic · domain · difficulty · score.
    let show_score = !app.search_query.is_empty();
    let items: Vec<ListItem> = app
        .filtered
        .iter()
        .map(|&(i, score)| {
            let p = &app.problems[i];
            let mut spans = vec![
                Span::styled(" ● ", Style::default().fg(difficulty_color(p.difficulty))),
                Span::styled(p.title.clone(), Style::default().fg(pal.text)),
                Span::styled(
                    format!("   {}  ", p.topic),
                    Style::default().fg(pal.overlay),
                ),
                Span::styled(
                    p.domain.to_string(),
                    Style::default().fg(domain_color(p.domain)),
                ),
                Span::styled(
                    format!("  {}", p.difficulty),
                    Style::default().fg(difficulty_color(p.difficulty)),
                ),
            ];
            if show_score {
                spans.push(Span::styled(
                    format!("   ·  {score}"),
                    Style::default().fg(pal.overlay),
                ));
            }
            ListItem::new(Line::from(spans))
        })
        .collect();
    let list = List::new(items)
        .style(Style::default().bg(pal.base))
        .highlight_style(theme::selection());
    frame.render_stateful_widget(list, list_area, &mut app.search_state);
}

/// Domain → accent color, shared across the navigator and headers.
fn domain_color(d: Domain) -> ratatui::style::Color {
    let p = theme::palette();
    match d {
        Domain::Symbolic => p.accent,
        Domain::Tensor => p.blue,
        Domain::Exact => p.teal,
    }
}

/// Difficulty → accent color.
fn difficulty_color(d: Difficulty) -> ratatui::style::Color {
    let p = theme::palette();
    match d {
        Difficulty::Easy => p.green,
        Difficulty::Medium => p.yellow,
        Difficulty::Hard => p.red,
    }
}

/// Top bar: the brand wordmark and tagline on the left; problem count and a
/// difficulty legend on the right.
fn draw_topbar(frame: &mut Frame, app: &App, area: Rect) {
    let p = theme::palette();
    let [left, right] = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(0), Constraint::Length(30)])
        .areas(area);

    let brand = Line::from(vec![
        Span::styled(
            " 🐍 mamba",
            Style::default().fg(p.accent).add_modifier(Modifier::BOLD),
        ),
        Span::styled("  verify your maths", Style::default().fg(p.subtext)),
    ]);
    frame.render_widget(Paragraph::new(brand).style(theme::canvas()), left);

    // While the finder is open the right side becomes a live match-count
    // badge; otherwise it shows the total and a difficulty legend.
    let legend = if app.search_active {
        Line::from(Span::styled(
            format!(" ⌕ {} / {} ", app.filtered.len(), app.problems.len()),
            theme::badge(p.accent),
        ))
    } else {
        Line::from(vec![
            Span::styled(
                format!("{} problems  ", app.problems.len()),
                Style::default().fg(p.subtext),
            ),
            Span::styled("● ", Style::default().fg(p.green)),
            Span::styled("● ", Style::default().fg(p.yellow)),
            Span::styled("● ", Style::default().fg(p.red)),
        ])
    };
    frame.render_widget(
        Paragraph::new(legend)
            .alignment(Alignment::Right)
            .style(theme::canvas()),
        right,
    );
}

/// Left sidebar: problems grouped under domain headers.
fn draw_navigator(frame: &mut Frame, app: &mut App, area: Rect) {
    let pal = theme::palette();
    let items: Vec<ListItem> = app
        .nav_rows
        .iter()
        .map(|row| match row {
            NavRow::DomainHeader(domain) => ListItem::new(Line::from(Span::styled(
                format!("◆ {domain}"),
                Style::default()
                    .fg(pal.accent_deep)
                    .add_modifier(Modifier::BOLD),
            ))),
            NavRow::TopicHeader(topic) => ListItem::new(Line::from(Span::styled(
                format!("  {topic}"),
                Style::default()
                    .fg(pal.subtext)
                    .add_modifier(Modifier::ITALIC),
            ))),
            NavRow::Problem(i) => {
                let prob = &app.problems[*i];
                ListItem::new(Line::from(vec![
                    Span::raw("    "),
                    Span::styled("● ", Style::default().fg(difficulty_color(prob.difficulty))),
                    Span::styled(prob.title.clone(), Style::default().fg(pal.text)),
                ]))
            }
        })
        .collect();

    let list = List::new(items)
        .block(theme::pane_block("Domains", app.focus == Focus::Navigator))
        .highlight_style(theme::selection());
    frame.render_stateful_widget(list, area, &mut app.nav_state);
}

/// Central pane: the problem statement (title, badges, prompt). Unicode in the
/// prompt — superscripts, matrix brackets — is rendered verbatim.
fn draw_description(frame: &mut Frame, app: &App, area: Rect) {
    let pal = theme::palette();
    let mut lines: Vec<Line> = Vec::new();
    if let Some(p) = app.open_problem_ref() {
        lines.push(Line::from(vec![
            Span::styled(
                p.title.clone(),
                Style::default().fg(pal.text).add_modifier(Modifier::BOLD),
            ),
            Span::raw("   "),
            Span::styled(
                format!("[{}]", p.difficulty),
                Style::default().fg(difficulty_color(p.difficulty)),
            ),
            Span::styled(format!("  {}", p.topic), Style::default().fg(pal.subtext)),
            Span::styled(
                format!("  · {}", p.domain),
                Style::default().fg(domain_color(p.domain)),
            ),
        ]));
        lines.push(Line::default());
        // Render ASCII formula markers (^, _, [[…]]) into typeset math.
        lines.extend(formula::render(&p.prompt));
    } else {
        lines.push(Line::from("No problems found — see README for setup."));
    }

    let paragraph = Paragraph::new(lines)
        .style(theme::body())
        .block(theme::pane_block(
            "Problem",
            app.focus == Focus::Description,
        ))
        .wrap(Wrap { trim: false })
        .scroll((app.description_scroll, 0));
    frame.render_widget(paragraph, area);
}

/// Bottom-central pane: the verification console — parse hints, witnessed
/// mismatches, or a success line with the vault export path.
fn draw_console(frame: &mut Frame, app: &App, area: Rect) {
    let pal = theme::palette();
    let mut lines: Vec<Line> = Vec::new();

    // A verification in flight pre-empts the last verdict — the worker runs off
    // the UI thread, so this frame (and input) stay responsive while it does.
    if app.is_verifying() {
        lines.push(Line::from(Span::styled(
            "⏳ Verifying…",
            Style::default().fg(pal.yellow).add_modifier(Modifier::BOLD),
        )));
        if let Some(p) = app.open_problem_ref() {
            lines.push(Line::from(Span::styled(
                format!("  routing through the {} pipeline", p.domain),
                Style::default().fg(pal.subtext),
            )));
        }
    } else {
        match &app.verdict {
            None => lines.push(Line::from(Span::styled(
                "Type your answer below and press Enter to verify.",
                Style::default().fg(pal.overlay),
            ))),
            Some(Verdict::Correct) => {
                lines.push(Line::from(Span::styled(
                    "✔ Correct — mathematically verified.",
                    Style::default().fg(pal.green).add_modifier(Modifier::BOLD),
                )));
                if let Some(path) = &app.exported_to {
                    lines.push(Line::from(Span::styled(
                        format!("  exported → {}", path.display()),
                        Style::default().fg(pal.blue),
                    )));
                }
            }
            Some(Verdict::Incorrect { detail }) => {
                lines.push(Line::from(Span::styled(
                    "✘ Not quite.",
                    Style::default().fg(pal.red).add_modifier(Modifier::BOLD),
                )));
                lines.push(Line::from(Span::styled(
                    format!("  {detail}"),
                    Style::default().fg(pal.subtext),
                )));
            }
            Some(Verdict::ParseError { message, hint }) => {
                lines.push(Line::from(Span::styled(
                    "⚠ Couldn't read that answer.",
                    Style::default().fg(pal.yellow).add_modifier(Modifier::BOLD),
                )));
                lines.push(Line::from(Span::styled(
                    format!("  {message}"),
                    Style::default().fg(pal.subtext),
                )));
                if let Some(h) = hint {
                    lines.push(Line::from(Span::styled(
                        format!("  hint: {h}"),
                        Style::default().fg(pal.teal),
                    )));
                }
            }
        }
    }

    // A persistent per-problem syntax hint and attempt counter footer line.
    if let Some(p) = app.open_problem_ref()
        && let Some(h) = &p.hint
    {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            format!("syntax: {h}"),
            Style::default().fg(pal.overlay),
        )));
    }
    if app.attempts > 0 {
        lines.push(Line::from(Span::styled(
            format!("attempts: {}", app.attempts),
            Style::default().fg(pal.overlay),
        )));
    }

    let paragraph = Paragraph::new(lines)
        .style(theme::body())
        .block(theme::pane_block(
            "Verification",
            app.focus == Focus::Console,
        ))
        .wrap(Wrap { trim: false })
        .scroll((app.console_scroll, 0));
    frame.render_widget(paragraph, area);
}

/// Bottom-anchored answer/command bar — a single-line input rendered with a
/// prompt glyph and a block cursor, scoped to the open problem's domain.
fn draw_input(frame: &mut Frame, app: &App, area: Rect) {
    let pal = theme::palette();
    let focused = app.focus == Focus::Input;
    let domain = app
        .open_problem_ref()
        .map(|p| p.domain.to_string())
        .unwrap_or_else(|| "—".into());

    let cursor = if focused { "▌" } else { "" };
    let line = Line::from(vec![
        Span::styled(" › ", Style::default().fg(pal.accent)),
        Span::styled(app.input.clone(), Style::default().fg(pal.text)),
        Span::styled(cursor, Style::default().fg(pal.accent)),
    ]);
    frame.render_widget(
        Paragraph::new(line)
            .style(theme::body())
            .block(theme::pane_block(format!("Answer · {domain}"), focused)),
        area,
    );
}

/// One-line status bar: pill-style key hints plus the live verdict badge.
fn draw_footer(frame: &mut Frame, app: &App, area: Rect) {
    let mut spans: Vec<Span> = vec![Span::raw(" ")];
    for (key, action) in [
        ("/", "find"),
        ("⇥", "focus"),
        ("↵", "submit"),
        ("^P", "stats"),
        ("esc", "nav"),
        ("^Q", "quit"),
    ] {
        spans.extend(theme::key_hint(key, action));
    }

    let pal = theme::palette();
    let badge = if app.is_verifying() {
        Some((" VERIFYING ", pal.yellow))
    } else {
        match &app.verdict {
            None => None,
            Some(Verdict::Correct) => Some((" CORRECT ", pal.green)),
            Some(Verdict::Incorrect { .. }) => Some((" WRONG ", pal.red)),
            Some(Verdict::ParseError { .. }) => Some((" INVALID ", pal.yellow)),
        }
    };
    if let Some((label, color)) = badge {
        spans.push(Span::styled(label, theme::badge(color)));
    }

    frame.render_widget(
        Paragraph::new(Line::from(spans)).style(theme::canvas()),
        area,
    );
}
