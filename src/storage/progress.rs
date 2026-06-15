//! The rolling progress note: a living dashboard written into the vault next to
//! the per-problem notes.
//!
//! The vault is the single source of truth — [`update`] scans every per-problem
//! note's YAML frontmatter (the same frontmatter [`super::vault::export`]
//! writes) and re-renders `_progress.md` with totals, a per-domain and
//! per-difficulty breakdown, day streaks, and the most recent solves. No
//! separate ledger to keep in sync.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Context;
use chrono::NaiveDate;

/// The dashboard file name; skipped when scanning so it never counts itself.
const PROGRESS_FILE: &str = "_progress.md";

/// Canonical display order for the breakdowns.
const DOMAINS: [&str; 3] = ["Symbolic", "Tensor", "Exact"];
const DIFFICULTIES: [&str; 3] = ["Easy", "Medium", "Hard"];

/// How many recent solves to list.
const RECENT: usize = 10;

/// One solved-problem note, distilled to the fields the dashboard needs.
struct Note {
    title: String,
    domain: String,
    difficulty: String,
    date: NaiveDate,
}

/// Scans `vault`, aggregates the solved notes, and writes `_progress.md`,
/// returning its path. `today` is injected so callers and tests control the
/// rendered date and streak.
pub fn update(vault: &Path, today: NaiveDate) -> anyhow::Result<PathBuf> {
    let notes = scan(vault);
    let path = vault.join(PROGRESS_FILE);
    fs::write(&path, render(&notes, today))
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(path)
}

/// Reads every `*.md` in `vault` (except the dashboard) and parses the notes
/// whose frontmatter carries the fields we need. Unreadable or foreign files
/// are skipped, not fatal — the dashboard is best-effort.
fn scan(vault: &Path) -> Vec<Note> {
    let mut notes = Vec::new();
    let Ok(entries) = fs::read_dir(vault) else {
        return notes;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "md") {
            continue;
        }
        if path.file_name().and_then(|n| n.to_str()) == Some(PROGRESS_FILE) {
            continue;
        }
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        if let Some(note) = parse_note(&content) {
            notes.push(note);
        }
    }
    notes
}

/// Parses a note's leading `---`…`---` YAML frontmatter into a [`Note`], or
/// `None` if it lacks the required `domain`, `difficulty`, and `timestamp`.
fn parse_note(content: &str) -> Option<Note> {
    let mut lines = content.lines();
    if lines.next()?.trim() != "---" {
        return None;
    }
    let (mut domain, mut difficulty, mut title, mut date) = (None, None, None, None);
    for line in lines {
        if line.trim() == "---" {
            break;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim().trim_matches('"').to_string();
        match key.trim() {
            "domain" => domain = Some(value),
            "difficulty" => difficulty = Some(value),
            "title" => title = Some(value),
            "timestamp" => date = NaiveDate::parse_from_str(&value, "%Y-%m-%d").ok(),
            _ => {}
        }
    }
    Some(Note {
        title: title.unwrap_or_default(),
        domain: domain?,
        difficulty: difficulty?,
        date: date?,
    })
}

/// Renders the dashboard Markdown from the aggregated notes.
fn render(notes: &[Note], today: NaiveDate) -> String {
    let dates: BTreeSet<NaiveDate> = notes.iter().map(|n| n.date).collect();
    let current = current_streak(&dates, today);
    let longest = longest_streak(&dates);

    let mut out = String::new();
    out.push_str(&format!(
        "---\n\
         title: Mamba Progress\n\
         updated: {}\n\
         tags: [mamba, progress]\n\
         ---\n\n\
         # 🐍 Mamba Progress\n\n\
         **{} solved** · current streak **{}** · longest **{}**\n\n",
        today.format("%Y-%m-%d"),
        notes.len(),
        current,
        longest,
    ));

    out.push_str("## By domain\n\n");
    for d in DOMAINS {
        let n = notes.iter().filter(|note| note.domain == d).count();
        out.push_str(&format!("- {d:<9} {n}\n"));
    }

    out.push_str("\n## By difficulty\n\n");
    for d in DIFFICULTIES {
        let n = notes.iter().filter(|note| note.difficulty == d).count();
        out.push_str(&format!("- {d:<7} {n}\n"));
    }

    out.push_str("\n## Recently solved\n\n");
    let mut recent: Vec<&Note> = notes.iter().collect();
    // Most recent first; ties broken by title for a stable, readable order.
    recent.sort_by(|a, b| b.date.cmp(&a.date).then(a.title.cmp(&b.title)));
    if recent.is_empty() {
        out.push_str("- (nothing yet)\n");
    }
    for note in recent.into_iter().take(RECENT) {
        out.push_str(&format!(
            "- {}  {}  [{} · {}]\n",
            note.date.format("%Y-%m-%d"),
            note.title,
            note.domain,
            note.difficulty,
        ));
    }

    out
}

/// The run of consecutive days ending today (or, if today has no solve,
/// ending yesterday) — the "current streak".
fn current_streak(dates: &BTreeSet<NaiveDate>, today: NaiveDate) -> u32 {
    let start = if dates.contains(&today) {
        Some(today)
    } else {
        today.pred_opt().filter(|d| dates.contains(d))
    };
    let Some(mut day) = start else {
        return 0;
    };
    let mut streak = 1;
    while let Some(prev) = day.pred_opt() {
        if !dates.contains(&prev) {
            break;
        }
        streak += 1;
        day = prev;
    }
    streak
}

/// The longest run of consecutive days anywhere in the history.
fn longest_streak(dates: &BTreeSet<NaiveDate>) -> u32 {
    let (mut longest, mut run) = (0u32, 0u32);
    let mut prev: Option<NaiveDate> = None;
    for &day in dates {
        // `dates` iterates ascending, so a run continues iff the previous day
        // is exactly one before this one.
        run = match prev {
            Some(p) if p.succ_opt() == Some(day) => run + 1,
            _ => 1,
        };
        longest = longest.max(run);
        prev = Some(day);
    }
    longest
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, day).unwrap()
    }

    #[test]
    fn current_streak_counts_back_from_today() {
        let dates: BTreeSet<_> = [d(2026, 6, 13), d(2026, 6, 14), d(2026, 6, 15)]
            .into_iter()
            .collect();
        assert_eq!(current_streak(&dates, d(2026, 6, 15)), 3);
        // A gap breaks it.
        let gapped: BTreeSet<_> = [d(2026, 6, 12), d(2026, 6, 15)].into_iter().collect();
        assert_eq!(current_streak(&gapped, d(2026, 6, 15)), 1);
        // Solved yesterday but not today → still counts (from yesterday).
        assert_eq!(current_streak(&dates, d(2026, 6, 16)), 3);
        // Idle for two days → zero.
        assert_eq!(current_streak(&dates, d(2026, 6, 18)), 0);
    }

    #[test]
    fn longest_streak_finds_the_best_run() {
        let dates: BTreeSet<_> = [
            d(2026, 6, 1),
            d(2026, 6, 2),
            d(2026, 6, 3), // run of 3
            d(2026, 6, 10),
            d(2026, 6, 11), // run of 2
        ]
        .into_iter()
        .collect();
        assert_eq!(longest_streak(&dates), 3);
        assert_eq!(longest_streak(&BTreeSet::new()), 0);
    }

    #[test]
    fn scan_and_render_aggregate_the_vault() {
        let dir = std::env::temp_dir().join(format!("mamba-progress-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let note = |slug: &str, domain: &str, diff: &str, day: u32| {
            let body = format!(
                "---\ntitle: \"{slug}\"\ntopic: \"X\"\ndomain: {domain}\n\
                 difficulty: {diff}\ntimestamp: 2026-06-{day:02}\n\
                 time_spent: \"1m\"\ntags: [mamba, math]\n---\n\n# {slug}\n"
            );
            fs::write(dir.join(format!("{slug}.md")), body).unwrap();
        };
        note("a", "Symbolic", "Easy", 14);
        note("b", "Tensor", "Hard", 15);
        note("c", "Exact", "Easy", 15);

        let path = update(&dir, d(2026, 6, 15)).unwrap();
        let rendered = fs::read_to_string(&path).unwrap();

        // A "- <label> … <count>" line exists, regardless of padding width.
        let row = |label: &str, count: usize| {
            rendered.lines().any(|l| {
                l.starts_with(&format!("- {label}"))
                    && l.split_whitespace().last() == Some(count.to_string().as_str())
            })
        };

        assert!(rendered.contains("**3 solved**"));
        assert!(rendered.contains("current streak **2**")); // 14th + 15th
        assert!(row("Symbolic", 1));
        assert!(row("Tensor", 1));
        assert!(row("Exact", 1));
        assert!(row("Easy", 2));
        assert!(row("Hard", 1));
        // The dashboard never counts itself.
        let reread = update(&dir, d(2026, 6, 15)).unwrap();
        assert!(fs::read_to_string(reread).unwrap().contains("**3 solved**"));

        let _ = fs::remove_dir_all(&dir);
    }
}
