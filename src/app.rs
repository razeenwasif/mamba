//! Application state and the event → state-transition logic.
//!
//! [`App`] is the single source of truth, exactly as in `~/Kata`. The render
//! layer ([`crate::ui`]) reads it; the input layer ([`App::on_key`]) mutates
//! it. Nothing else holds state, which keeps the immediate-mode draw loop
//! trivially correct.

use std::path::PathBuf;
use std::time::Instant;

use chrono::Local;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::widgets::ListState;

use crate::engine::{self, Verdict};
use crate::storage::problem::Problem;
use crate::storage::vault;

/// Which pane currently owns keyboard input. Exactly one is focused at a time;
/// `Tab` cycles forward through the variants in declaration order and
/// `Shift-Tab` cycles backward.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    /// The left-hand domain-grouped problem navigator.
    Navigator,
    /// The scrollable problem-description pane.
    Description,
    /// The bottom-anchored answer/command bar.
    Input,
    /// The verification console.
    Console,
}

impl Focus {
    /// All panes, in `Tab`-cycling order.
    const ORDER: [Focus; 4] = [
        Focus::Navigator,
        Focus::Description,
        Focus::Input,
        Focus::Console,
    ];

    /// The pane after `self` in cycling order (wraps around).
    fn next(self) -> Focus {
        let i = Self::ORDER.iter().position(|f| *f == self).unwrap_or(0);
        Self::ORDER[(i + 1) % Self::ORDER.len()]
    }

    /// The pane before `self` in cycling order (wraps around).
    fn prev(self) -> Focus {
        let i = Self::ORDER.iter().position(|f| *f == self).unwrap_or(0);
        Self::ORDER[(i + Self::ORDER.len() - 1) % Self::ORDER.len()]
    }
}

/// One row of the navigation tree: either a non-selectable domain header or a
/// selectable problem (an index into [`App::problems`]).
#[derive(Debug, Clone)]
pub enum NavRow {
    /// A domain group header. Selection skips over these.
    Header(String),
    /// A selectable problem, stored as an index into [`App::problems`].
    Problem(usize),
}

/// The complete, mutable state of a Mamba session.
pub struct App {
    /// The loaded problem bank, sorted by (domain, difficulty, topic, title).
    pub problems: Vec<Problem>,
    /// Flattened navigation tree: domain headers interleaved with problems.
    pub nav_rows: Vec<NavRow>,
    /// Selection state for the navigator `List` widget. Always points at a
    /// [`NavRow::Problem`] row (never a header).
    pub nav_state: ListState,
    /// The pane that currently receives keyboard input.
    pub focus: Focus,
    /// Index into [`problems`](App::problems) of the problem currently being
    /// attempted (what gets verified and exported).
    pub open_problem: Option<usize>,
    /// The answer currently being typed into the input bar.
    pub input: String,
    /// The most recent verification result, shown in the console. `None`
    /// before the first submission for the open problem.
    pub verdict: Option<Verdict>,
    /// How many answers have been submitted for the open problem.
    pub attempts: u32,
    /// Vertical scroll offset of the description pane, in lines.
    pub description_scroll: u16,
    /// Vertical scroll offset of the console pane, in lines.
    pub console_scroll: u16,
    /// Root directory of the Markdown vault solved problems are exported into
    /// (default `~/OnyxVault/Mamba`, overridable via `MAMBA_VAULT`).
    pub vault_path: PathBuf,
    /// When the open problem was loaded; used for the `time_spent` frontmatter.
    pub opened_at: Instant,
    /// Vault file written by the most recent correct answer, if any.
    pub exported_to: Option<PathBuf>,
    /// When `true`, the main loop tears down the terminal and exits.
    pub should_quit: bool,
}

impl App {
    /// Builds the initial state from a loaded problem bank and a vault path,
    /// opening the first problem (if any).
    pub fn new(problems: Vec<Problem>, vault_path: PathBuf) -> Self {
        let nav_rows = build_nav_rows(&problems);
        let mut app = Self {
            problems,
            nav_rows,
            nav_state: ListState::default(),
            focus: Focus::Navigator,
            open_problem: None,
            input: String::new(),
            verdict: None,
            attempts: 0,
            description_scroll: 0,
            console_scroll: 0,
            vault_path,
            opened_at: Instant::now(),
            exported_to: None,
            should_quit: false,
        };
        if let Some(first) = app
            .nav_rows
            .iter()
            .position(|r| matches!(r, NavRow::Problem(_)))
        {
            app.nav_state.select(Some(first));
            app.open_selected();
        }
        app
    }

    /// Index into [`problems`](App::problems) the navigator points at, if any.
    pub fn selected_index(&self) -> Option<usize> {
        match self.nav_rows.get(self.nav_state.selected()?)? {
            NavRow::Problem(i) => Some(*i),
            NavRow::Header(_) => None,
        }
    }

    /// The problem currently open for attempting (what verifies and exports).
    pub fn open_problem_ref(&self) -> Option<&Problem> {
        self.problems.get(self.open_problem?)
    }

    /// Routes a key event to the global bindings first, then to whichever pane
    /// holds focus. This is the *only* place state transitions happen.
    pub fn on_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

        // -- Global bindings (work regardless of focus) -------------------
        if (key.code == KeyCode::Char('q') || key.code == KeyCode::Char('c')) && ctrl {
            self.should_quit = true;
            return;
        }

        // -- The answer bar owns the keyboard while it has focus ----------
        if self.focus == Focus::Input {
            self.on_input_key(key, ctrl);
            return;
        }

        match key.code {
            KeyCode::Tab => {
                self.focus = self.focus.next();
                return;
            }
            KeyCode::BackTab => {
                self.focus = self.focus.prev();
                return;
            }
            // Esc always bails out to the navigator.
            KeyCode::Esc => {
                self.focus = Focus::Navigator;
                return;
            }
            _ => {}
        }

        // -- Pane-local bindings ------------------------------------------
        match self.focus {
            Focus::Navigator => self.on_navigator_key(key),
            Focus::Description => self.scroll_pane(key, true),
            Focus::Console => self.scroll_pane(key, false),
            Focus::Input => {} // routed earlier; never reached
        }
    }

    /// Keys handled while the navigator owns focus.
    fn on_navigator_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => self.select_adjacent(1),
            KeyCode::Char('k') | KeyCode::Up => self.select_adjacent(-1),
            KeyCode::Enter => {
                self.open_selected();
                self.focus = Focus::Input;
            }
            _ => {}
        }
    }

    /// Keys handled while the answer bar has focus. Enter submits; printable
    /// characters extend the answer; Esc/Tab leave the bar.
    fn on_input_key(&mut self, key: KeyEvent, ctrl: bool) {
        match key.code {
            KeyCode::Esc => self.focus = Focus::Navigator,
            KeyCode::Tab => self.focus = self.focus.next(),
            KeyCode::BackTab => self.focus = self.focus.prev(),
            KeyCode::Enter => self.submit_answer(),
            KeyCode::Backspace => {
                self.input.pop();
            }
            KeyCode::Char(c) if !ctrl => self.input.push(c),
            _ => {}
        }
    }

    /// Scrolls the description (`desc == true`) or console pane by one line.
    fn scroll_pane(&mut self, key: KeyEvent, desc: bool) {
        let offset = if desc {
            &mut self.description_scroll
        } else {
            &mut self.console_scroll
        };
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => *offset = offset.saturating_add(1),
            KeyCode::Char('k') | KeyCode::Up => *offset = offset.saturating_sub(1),
            _ => {}
        }
    }

    /// Moves the navigator selection up (`-1`) or down (`+1`), skipping domain
    /// headers so the cursor always lands on a problem, and opens it.
    fn select_adjacent(&mut self, dir: isize) {
        let Some(current) = self.nav_state.selected() else {
            return;
        };
        let mut i = current as isize;
        loop {
            i += dir;
            if i < 0 || i as usize >= self.nav_rows.len() {
                return; // hit the top/bottom edge — keep current selection
            }
            if matches!(self.nav_rows[i as usize], NavRow::Problem(_)) {
                self.nav_state.select(Some(i as usize));
                self.open_selected();
                return;
            }
        }
    }

    /// Opens the selected problem, resetting all per-problem state (input,
    /// verdict, attempts, scroll, timer).
    fn open_selected(&mut self) {
        let Some(idx) = self.selected_index() else {
            return;
        };
        self.open_problem = Some(idx);
        self.input.clear();
        self.verdict = None;
        self.attempts = 0;
        self.description_scroll = 0;
        self.console_scroll = 0;
        self.exported_to = None;
        self.opened_at = Instant::now();
    }

    /// Verifies the typed answer through the [universal router](engine::verify)
    /// and, on a correct answer, exports the solve to the Markdown vault.
    fn submit_answer(&mut self) {
        let Some(problem) = self.open_problem_ref().cloned() else {
            return;
        };
        let answer = self.input.clone();

        let verdict = engine::verify(&problem, &answer);
        self.attempts += 1;
        self.console_scroll = 0;

        if verdict.is_correct() {
            self.export_solution(&problem, &answer);
        }
        self.verdict = Some(verdict);
    }

    /// Writes the accepted answer into the Markdown vault, recording any
    /// failure as an [`Verdict::Incorrect`] so a silent data-loss never hides
    /// behind a green check.
    fn export_solution(&mut self, problem: &Problem, answer: &str) {
        match vault::export(
            &self.vault_path,
            problem,
            answer,
            self.opened_at.elapsed(),
            Local::now().date_naive(),
        ) {
            Ok(path) => self.exported_to = Some(path),
            Err(err) => {
                self.verdict = Some(Verdict::incorrect(format!("export failed: {err:#}")));
            }
        }
    }
}

/// Flattens the problem bank into navigator rows, inserting a header row
/// whenever the domain changes. Assumes `problems` is sorted by domain first —
/// [`crate::storage::problem::load_all`] guarantees this.
fn build_nav_rows(problems: &[Problem]) -> Vec<NavRow> {
    let mut rows = Vec::with_capacity(problems.len() + 4);
    let mut last_domain: Option<String> = None;
    for (i, p) in problems.iter().enumerate() {
        let domain = p.domain.to_string();
        if last_domain.as_deref() != Some(domain.as_str()) {
            rows.push(NavRow::Header(domain.clone()));
            last_domain = Some(domain);
        }
        rows.push(NavRow::Problem(i));
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::problem::{AnswerKind, Difficulty, Domain};

    fn problem(title: &str, domain: Domain, expected: &str, kind: AnswerKind) -> Problem {
        Problem {
            id: title.to_lowercase().replace(' ', "-"),
            title: title.into(),
            domain,
            topic: "X".into(),
            difficulty: Difficulty::Easy,
            prompt: "p".into(),
            expected: expected.into(),
            answer_kind: kind,
            hint: None,
        }
    }

    fn app() -> App {
        let problems = vec![
            problem("Expand", Domain::Symbolic, "x^2 - x - 6", AnswerKind::Scalar),
            problem("Determinant", Domain::Tensor, "10", AnswerKind::Scalar),
            problem("Factorize", Domain::Exact, "104729", AnswerKind::Factorization),
        ];
        App::new(problems, std::env::temp_dir().join("mamba-vault-test"))
    }

    #[test]
    fn nav_rows_insert_one_header_per_domain_run() {
        let a = app();
        let headers = a
            .nav_rows
            .iter()
            .filter(|r| matches!(r, NavRow::Header(_)))
            .count();
        assert_eq!(headers, 3); // Symbolic, Tensor, Exact
        assert!(matches!(a.nav_rows[0], NavRow::Header(_)));
        // First selectable row is auto-opened.
        assert_eq!(a.open_problem, Some(0));
    }

    #[test]
    fn selection_skips_headers_in_both_directions() {
        let mut a = app();
        // Rows: [H(Symbolic), P0, H(Tensor), P1, H(Exact), P2]; starts on P0=1.
        assert_eq!(a.nav_state.selected(), Some(1));
        a.select_adjacent(1);
        assert_eq!(a.nav_state.selected(), Some(3)); // skipped the Tensor header
        a.select_adjacent(-1);
        assert_eq!(a.nav_state.selected(), Some(1));
        a.select_adjacent(-1); // top edge — unchanged
        assert_eq!(a.nav_state.selected(), Some(1));
    }

    #[test]
    fn submitting_a_correct_answer_marks_it_and_exports() {
        let mut a = app();
        a.input = "(x-3)(x+2)".into();
        a.submit_answer();
        assert_eq!(a.verdict, Some(Verdict::Correct));
        assert_eq!(a.attempts, 1);
        assert!(a.exported_to.is_some());
    }

    #[test]
    fn submitting_a_wrong_answer_does_not_export() {
        let mut a = app();
        a.input = "x^2 + x - 6".into();
        a.submit_answer();
        assert!(matches!(a.verdict, Some(Verdict::Incorrect { .. })));
        assert!(a.exported_to.is_none());
    }
}
