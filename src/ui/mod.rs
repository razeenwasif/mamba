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

    // The AI tutor claims a right-hand column when visible (Ctrl-A).
    let agent_w = if app.agent_visible { 46 } else { 0 };
    let [sidebar, center, agent_area] = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(30),
            Constraint::Min(0),
            Constraint::Length(agent_w),
        ])
        .areas(main);

    let [description, console] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(62), Constraint::Percentage(38)])
        .areas(center);

    draw_topbar(frame, app, topbar);
    draw_navigator(frame, app, sidebar);
    draw_description(frame, app, description);
    draw_console(frame, app, console);
    if app.agent_visible {
        draw_agent(frame, app, agent_area);
    }
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
    let w = (area.width * 7 / 10).clamp(44, 74);
    let h = (area.height * 9 / 10).clamp(12, 34);
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
        // Topic mastery: solved / total per topic (from the bank), with the
        // least-mastered topics first — what to study next.
        let mut totals: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
        for p in &app.problems {
            *totals.entry(p.topic.as_str()).or_default() += 1;
        }
        let solved: std::collections::HashMap<&str, usize> = stats
            .by_topic
            .iter()
            .map(|(t, c)| (t.as_str(), *c))
            .collect();
        let mut mastery: Vec<(&str, usize, usize)> = totals
            .iter()
            .map(|(&t, &total)| (t, solved.get(t).copied().unwrap_or(0), total))
            .collect();
        mastery.sort_by(|a, b| {
            let ratio = |(_, s, t): &(&str, usize, usize)| *s as f64 / *t as f64;
            ratio(a)
                .partial_cmp(&ratio(b))
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.0.cmp(b.0))
        });
        if !mastery.is_empty() {
            lines.push(Line::default());
            lines.push(section(" Topic mastery"));
            const SHOWN: usize = 9;
            for (topic, s, total) in mastery.iter().take(SHOWN) {
                lines.push(mastery_line(topic, *s, *total));
            }
            if mastery.len() > SHOWN {
                lines.push(Line::from(Span::styled(
                    format!("   … +{} more (mastered or near)", mastery.len() - SHOWN),
                    Style::default().fg(pal.overlay),
                )));
            }
        }
        if !stats.recent.is_empty() {
            lines.push(Line::default());
            lines.push(section(" Recently solved"));
            for r in stats.recent.iter().take(3) {
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

/// One topic-mastery row: `topic  solved/total` and a coverage bar (`█` filled,
/// `░` remaining), coloured green when complete, yellow when started, dim when
/// untouched.
fn mastery_line(topic: &str, solved: usize, total: usize) -> Line<'static> {
    const BAR: usize = 12;
    let pal = theme::palette();
    let filled = if total > 0 {
        (solved * BAR / total).min(BAR)
    } else {
        0
    };
    let color = if total > 0 && solved >= total {
        pal.green
    } else if solved > 0 {
        pal.yellow
    } else {
        pal.overlay
    };
    Line::from(vec![
        Span::styled(
            format!(" {topic:<19} {solved:>2}/{total:<3} "),
            Style::default().fg(pal.subtext),
        ),
        Span::styled("█".repeat(filled), Style::default().fg(color)),
        Span::styled(
            "░".repeat(BAR.saturating_sub(filled)),
            Style::default().fg(pal.surface1),
        ),
    ])
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
    let title = if app.search_semantic {
        "  ✦  Semantic search  "
    } else {
        "  ⌕  Find a problem  "
    };
    let hint = if app.search_semantic {
        "  ↵ search · ↑↓ move · ⇥ fuzzy · esc cancel  "
    } else {
        "  ↑↓ move · ↵ open · ⇥ semantic · esc cancel  "
    };
    let block = Block::default()
        .title(Span::styled(
            title,
            Style::default()
                .fg(pal.accent_deep)
                .add_modifier(Modifier::BOLD),
        ))
        .title_bottom(Span::styled(hint, Style::default().fg(pal.overlay)))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(pal.accent))
        .style(Style::default().bg(pal.base));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let [query_area, status_area, list_area] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
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

    // Status line: indexing progress, an error, or a "press ↵" hint (semantic).
    let status_line = match &app.search_status {
        Some(s) => {
            let color = if s.starts_with('⚠') {
                pal.red
            } else {
                pal.subtext
            };
            Line::from(Span::styled(format!(" {s}"), Style::default().fg(color)))
        }
        None => Line::default(),
    };
    frame.render_widget(
        Paragraph::new(status_line).style(Style::default().bg(pal.base)),
        status_area,
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
    let legend = if let Some(s) = &app.session {
        Line::from(Span::styled(
            format!(" 📚 {}/{} · {}m ", s.solved_count(), s.total(), s.minutes()),
            theme::badge(p.accent),
        ))
    } else if app.search_active {
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

    // Review mode with nothing due → a friendly empty state.
    let items = if items.is_empty() && app.review_due.is_some() {
        vec![ListItem::new(Line::from(Span::styled(
            "  ✓ all caught up!",
            Style::default().fg(pal.green).add_modifier(Modifier::BOLD),
        )))]
    } else {
        items
    };

    let title = if let Some(s) = &app.session {
        format!("Session · {}/{}", s.solved_count(), s.total())
    } else {
        match (&app.review_due, &app.topic_filter) {
            (Some(due), _) => format!("Review · {} due", due.len()),
            (None, Some(topic)) => format!("Topic · {topic}"),
            (None, None) => "Domains".to_string(),
        }
    };
    let list = List::new(items)
        .block(theme::pane_block(title, app.focus == Focus::Navigator))
        .highlight_style(theme::selection());
    frame.render_stateful_widget(list, area, &mut app.nav_state);
}

/// Central pane: the problem statement (title, badges, prompt). Unicode in the
/// prompt — superscripts, matrix brackets — is rendered verbatim.
fn draw_description(frame: &mut Frame, app: &App, area: Rect) {
    let pal = theme::palette();
    let mut lines: Vec<Line> = Vec::new();
    if let Some(p) = app.open_problem_ref() {
        // Banner for an AI-generated problem: verified vs unverified, plus the
        // one-line note (and `^S to save` once it can join the bank).
        if app.generated.is_some() {
            let (badge, color) = if app.generated_verified {
                (" ✨ AI-generated · ✓ cross-verified ", pal.green)
            } else {
                (" ✨ AI-generated · ⚠ unverified ", pal.yellow)
            };
            lines.push(Line::from(Span::styled(badge, theme::badge(color))));
            if let Some(note) = &app.generated_note {
                lines.push(Line::from(Span::styled(
                    note.clone(),
                    Style::default().fg(pal.subtext),
                )));
            }
            lines.push(Line::from(Span::styled(
                "^S save to bank · ^G regenerate",
                Style::default().fg(pal.overlay),
            )));
            lines.push(Line::default());
        }
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

        // Preliminary reading: a recap of the subject matter (toggle with `g`).
        if let Some(recap) = p.recap.as_deref().filter(|_| app.show_recap) {
            lines.push(Line::default());
            lines.push(Line::from(Span::styled(
                "📖 Recap",
                Style::default()
                    .fg(pal.accent_deep)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::default());
            for line in formula::render(recap) {
                lines.push(line.style(Style::default().fg(pal.subtext)));
            }
        }
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

    // A generation run's progress (or error) sits at the very top while active.
    if let Some(status) = &app.gen_status {
        let color = if status.starts_with('⚠') {
            pal.red
        } else {
            pal.accent_deep
        };
        lines.push(Line::from(Span::styled(
            status.clone(),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::default());
    }

    // A session result (or "nothing due") takes the top line until superseded.
    if let Some(summary) = &app.session_summary {
        lines.push(Line::from(Span::styled(
            summary.clone(),
            Style::default()
                .fg(pal.accent_deep)
                .add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::default());
    }

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
                "Type your answer and press Enter · start with ? to ask the tutor.",
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
            Some(Verdict::Incorrect { detail, hint }) => {
                lines.push(Line::from(Span::styled(
                    "✘ Not quite.",
                    Style::default().fg(pal.red).add_modifier(Modifier::BOLD),
                )));
                lines.push(Line::from(Span::styled(
                    format!("  {detail}"),
                    Style::default().fg(pal.subtext),
                )));
                // A recognized near-miss (off by a sign, a factor, the
                // transpose, …) — the most actionable line, so accent it.
                if let Some(h) = hint {
                    lines.push(Line::from(Span::styled(
                        format!("  💡 {h}"),
                        Style::default().fg(pal.yellow).add_modifier(Modifier::BOLD),
                    )));
                }
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

    // After a failed answer, offer the tutor.
    if matches!(
        &app.verdict,
        Some(Verdict::Incorrect { .. } | Verdict::ParseError { .. })
    ) {
        lines.push(Line::from(Span::styled(
            "  press ^E to ask the tutor why",
            Style::default().fg(pal.teal),
        )));
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

    // A peek at this problem's prior solve history, read from the vault.
    if let Some(h) = &app.open_history {
        let last = h
            .last_solved
            .map(|d| d.format("%Y-%m-%d").to_string())
            .unwrap_or_else(|| "—".into());
        lines.push(Line::from(Span::styled(
            format!("📒 solved {}× · last {last}", h.solves),
            Style::default().fg(pal.blue),
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

/// Right-hand AI tutor panel: a streaming chat with the local Ollama model,
/// scoped to the open problem and the user's current answer.
fn draw_agent(frame: &mut Frame, app: &App, area: Rect) {
    let pal = theme::palette();
    let focused = app.focus == Focus::Agent;
    let block = theme::pane_block(format!("Tutor · {}", app.agent_model), focused);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let [transcript, input] = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(3)])
        .areas(inner);

    let mut lines: Vec<Line> = Vec::new();
    if app.agent_messages.is_empty() {
        for hint in [
            "Ask about the open problem:",
            "  \"give me a hint\"",
            "  \"explain the method\"",
            "  \"why is my answer wrong?\"",
            "",
            "Ctrl-A hides this panel.",
        ] {
            lines.push(Line::from(Span::styled(
                hint,
                Style::default().fg(pal.overlay),
            )));
        }
    }
    for (mi, m) in app.agent_messages.iter().enumerate() {
        let (label, color) = if m.role == "user" {
            ("you", pal.subtext)
        } else {
            ("tutor", pal.accent)
        };
        lines.push(Line::from(Span::styled(
            label,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )));
        let is_last = mi + 1 == app.agent_messages.len();
        if m.content.is_empty() && is_last && app.agent_streaming {
            lines.push(Line::from(Span::styled(
                "▌",
                Style::default().fg(pal.accent),
            )));
        } else {
            for l in m.content.lines() {
                lines.push(Line::from(Span::styled(
                    l.to_string(),
                    Style::default().fg(pal.text),
                )));
            }
            if is_last
                && app.agent_streaming
                && let Some(last) = lines.last_mut()
            {
                last.spans
                    .push(Span::styled(" ▌", Style::default().fg(pal.accent)));
            }
        }
        lines.push(Line::default());
    }

    // Auto-scroll to the bottom (estimate wrapped rows for accuracy).
    let w = transcript.width.max(1) as usize;
    let wrapped: usize = lines
        .iter()
        .map(|l| {
            let len: usize = l.spans.iter().map(|s| s.content.chars().count()).sum();
            len / w + 1
        })
        .sum();
    let scroll = wrapped.saturating_sub(transcript.height as usize) as u16;
    frame.render_widget(
        Paragraph::new(lines)
            .style(theme::body())
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0)),
        transcript,
    );

    // Input box.
    let (in_title, cursor) = if app.agent_streaming {
        (" generating… ", "")
    } else {
        (" ask ", "▌")
    };
    let in_block = Block::default()
        .title(Span::styled(in_title, Style::default().fg(pal.subtext)))
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(if focused { pal.accent } else { pal.surface1 }))
        .style(Style::default().bg(pal.base));
    let in_line = Line::from(vec![
        Span::styled(" › ", Style::default().fg(pal.accent)),
        Span::styled(app.agent_input.clone(), Style::default().fg(pal.text)),
        Span::styled(cursor, Style::default().fg(pal.accent)),
    ]);
    frame.render_widget(Paragraph::new(in_line).block(in_block), input);
}

/// One-line status bar: pill-style key hints plus the live verdict badge.
fn draw_footer(frame: &mut Frame, app: &App, area: Rect) {
    let mut spans: Vec<Span> = vec![Span::raw(" ")];
    for (key, action) in [
        ("/", "find"),
        ("n", "next"),
        ("s", "session"),
        ("r", "review"),
        ("g", "recap"),
        ("^A", "tutor"),
        ("^P", "stats"),
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
