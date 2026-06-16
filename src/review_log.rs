//! The append-only review-event log (`reviews.jsonl`).
//!
//! Every graded review — a solve or a lapse — appends one JSON line here. The
//! Markdown vault stays the source of truth for *scheduling* (each note's
//! frontmatter carries the current FSRS memory), but a per-review history is
//! what [`crate::fsrs`] parameter optimisation needs, and the frontmatter only
//! keeps the latest state. This log is that history: a derived, rebuildable,
//! plain-text corpus — never the source of truth (see the storage notes).

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::fsrs::Grade;
use crate::storage;

/// One graded review of one card.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    /// The problem's stable id.
    pub id: String,
    /// Review date, `YYYY-MM-DD`.
    pub date: String,
    /// FSRS grade: 1 = Again (lapse), 2 = Hard, 3 = Good, 4 = Easy.
    pub grade: u8,
}

impl Event {
    /// The review date parsed back to a [`NaiveDate`], if well-formed.
    pub fn date(&self) -> Option<NaiveDate> {
        NaiveDate::parse_from_str(&self.date, "%Y-%m-%d").ok()
    }
}

/// Appends a review to `path` (creating it and its parent as needed).
/// Best-effort: a logging failure must never derail a solve.
pub fn append_to(path: &Path, id: &str, date: NaiveDate, grade: Grade) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let event = Event {
        id: id.to_string(),
        date: date.format("%Y-%m-%d").to_string(),
        grade: grade as u8,
    };
    if let (Ok(line), Ok(mut file)) = (
        serde_json::to_string(&event),
        OpenOptions::new().create(true).append(true).open(path),
    ) {
        let _ = writeln!(file, "{line}");
    }
}

/// Reads every well-formed event from `path` (missing file → empty).
pub fn load_from(path: &Path) -> Vec<Event> {
    fs::read_to_string(path)
        .map(|content| {
            content
                .lines()
                .filter_map(|l| serde_json::from_str::<Event>(l).ok())
                .collect()
        })
        .unwrap_or_default()
}

/// Appends a review to the default log location (best-effort).
pub fn append(id: &str, date: NaiveDate, grade: Grade) {
    if let Ok(path) = storage::default_reviews_path() {
        append_to(&path, id, date, grade);
    }
}

/// Loads every review from the default log location.
pub fn load() -> Vec<Event> {
    storage::default_reviews_path()
        .map(|p| load_from(&p))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appends_and_reads_back_in_order() {
        let path = std::env::temp_dir().join(format!("mamba-reviews-{}.jsonl", std::process::id()));
        let _ = fs::remove_file(&path);
        let day = |d: u32| NaiveDate::from_ymd_opt(2026, 6, d).unwrap();

        append_to(&path, "alpha", day(14), Grade::Good);
        append_to(&path, "alpha", day(16), Grade::Again);
        append_to(&path, "beta", day(16), Grade::Hard);

        let events = load_from(&path);
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].id, "alpha");
        assert_eq!(events[0].grade, Grade::Good as u8);
        assert_eq!(events[1].grade, Grade::Again as u8);
        assert_eq!(events[1].date(), Some(day(16)));
        assert_eq!(events[2].id, "beta");

        let _ = fs::remove_file(&path);
    }
}
