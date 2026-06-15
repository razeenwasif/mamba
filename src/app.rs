//! Application state and the event → state-transition logic.
//!
//! [`App`] is the single source of truth, exactly as in `~/Kata`. The render
//! layer ([`crate::ui`]) reads it; the input layer ([`App::on_key`]) mutates
//! it. Nothing else holds state, which keeps the immediate-mode draw loop
//! trivially correct.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::Instant;

use chrono::Local;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;
use ratatui::widgets::ListState;

use crate::agent::{self, AgentEvent, ChatMessage};
use crate::engine::{self, Verdict};
use crate::generate;
use crate::recommend;
use crate::review;
use crate::semantic;
use crate::storage::problem::Problem;
use crate::storage::{progress, vault};

/// An in-flight verification running on a worker thread. It carries everything
/// the main thread needs to finalize the result once the worker replies — the
/// *captured* problem and answer (so navigating away can't corrupt the export)
/// and the instant the problem was opened (for the `time_spent` frontmatter).
struct VerifyJob {
    problem: Problem,
    answer: String,
    opened_at: Instant,
    rx: Receiver<Verdict>,
}

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
    /// The AI tutor chat input (toggled with `Ctrl-A`, not in the `Tab` cycle).
    Agent,
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

/// A focused study session: a fixed set of due problems worked through with a
/// live progress counter and an end-of-session summary.
#[derive(Debug, Clone)]
pub struct Session {
    /// Problem indices in this session (the due set when it started).
    due: HashSet<usize>,
    /// Those solved so far this session.
    solved: HashSet<usize>,
    /// When the session began (for the elapsed-time readout).
    started: Instant,
}

impl Session {
    /// Total problems in the session.
    pub fn total(&self) -> usize {
        self.due.len()
    }
    /// How many have been solved so far.
    pub fn solved_count(&self) -> usize {
        self.solved.len()
    }
    /// Whole minutes elapsed since the session started.
    pub fn minutes(&self) -> u64 {
        self.started.elapsed().as_secs() / 60
    }
    /// Whether every problem in the session has been solved.
    pub fn is_complete(&self) -> bool {
        !self.due.is_empty() && self.solved.len() == self.due.len()
    }
}

/// One row of the navigation tree: a non-selectable domain header, a
/// non-selectable topic sub-header, or a selectable problem (an index into
/// [`App::problems`]). Selection skips both header kinds.
#[derive(Debug, Clone)]
pub enum NavRow {
    /// A top-level domain group header (`Symbolic`, `Tensor`, `Exact`).
    DomainHeader(String),
    /// A second-level topic header within a domain (`Machine Learning`, …).
    TopicHeader(String),
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
    /// Active topic filter (`t` cycles it); `None` shows every topic.
    pub topic_filter: Option<String>,
    /// Distinct topics in the bank, sorted — the cycle order for the filter.
    pub topics: Vec<String>,
    /// When `Some`, review mode (`r`) is on and this holds the indices of the
    /// problems due for review; the navigator shows only those.
    pub review_due: Option<HashSet<usize>>,
    /// The active study session (`s`), if any: the navigator is filtered to its
    /// queue and progress is tracked.
    pub session: Option<Session>,
    /// A transient summary line (e.g. an ended session's result) shown in the
    /// console until the next submission or session.
    pub session_summary: Option<String>,
    /// The pane that currently receives keyboard input.
    pub focus: Focus,
    /// Whether the fuzzy problem finder (`/`) overlay is active. While `true`
    /// it owns the keyboard and floats over the panes.
    pub search_active: bool,
    /// The current fuzzy-search query.
    pub search_query: String,
    /// `(problem index, score)` for problems matching the query, ranked
    /// best-first. Meaningful only while [`search_active`](App::search_active);
    /// an empty query lists every problem (score 0).
    pub filtered: Vec<(usize, i64)>,
    /// Selection/scroll state for the floating search results — separate from
    /// [`nav_state`](App::nav_state), which keeps the tree selection behind the
    /// overlay.
    pub search_state: ListState,
    /// Index into [`problems`](App::problems) of the problem currently being
    /// attempted (what gets verified and exported).
    pub open_problem: Option<usize>,
    /// The answer currently being typed into the input bar.
    pub input: String,
    /// The most recent *completed* verification result, shown in the console.
    /// `None` before the first submission, or while one is in flight.
    pub verdict: Option<Verdict>,
    /// The verification currently running on a worker thread, if any. While
    /// `Some`, the UI shows a `Verifying…` state and further submits are
    /// ignored. Drained by [`poll_verify`](App::poll_verify); never read
    /// directly by the UI (use [`is_verifying`](App::is_verifying)).
    verify_job: Option<VerifyJob>,
    /// How many answers have been submitted for the open problem.
    pub attempts: u32,
    /// Whether the problem's recap (preliminary reading) is shown in the
    /// description pane. Toggled with `g`; on by default.
    pub show_recap: bool,
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
    /// The open problem's solve history (solves, last date), read from the vault
    /// when it opens and after each solve. `None` if never solved.
    pub open_history: Option<vault::ProblemHistory>,
    /// Whether the progress/stats overlay is shown (toggle: `Ctrl-P`). Modal:
    /// it floats over the panes and owns the keyboard while open.
    pub stats_visible: bool,
    /// Stats snapshot, recomputed from the vault each time the overlay opens.
    pub stats: Option<progress::Stats>,
    /// Whether the AI tutor panel is shown on the right (toggle: `Ctrl-A`).
    pub agent_visible: bool,
    /// The tutor conversation so far (user/assistant turns).
    pub agent_messages: Vec<ChatMessage>,
    /// The prompt currently being typed into the tutor input.
    pub agent_input: String,
    /// `true` while a model response is streaming in.
    pub agent_streaming: bool,
    /// Receiver for the in-flight streaming response, if any.
    pub agent_rx: Option<Receiver<AgentEvent>>,
    /// Ollama model tag and server URL (from `MAMBA_OLLAMA_*` or defaults).
    pub agent_model: String,
    pub agent_url: String,
    /// Whether the finder is in semantic mode (`Tab` toggles it): the query is
    /// ranked by meaning via embeddings instead of fuzzy character matching.
    pub search_semantic: bool,
    /// The embedding model tag for semantic search (`MAMBA_EMBED_MODEL`).
    pub embed_model: String,
    /// In-memory bank embeddings, built on the first semantic search and kept
    /// for the rest of the session so repeat queries only embed the query.
    embed_store: Option<semantic::Store>,
    /// Receiver for an in-flight semantic search worker, if any.
    search_rx: Option<Receiver<semantic::SearchEvent>>,
    /// The query a running/just-finished semantic search was launched for, so
    /// `filtered` is known to correspond to it (and Enter can open vs re-search).
    semantic_done_query: Option<String>,
    /// A transient status line for the finder (indexing progress, an error, or a
    /// "press ↵ to search" hint in semantic mode).
    pub search_status: Option<String>,
    /// The AI-generated problem currently being attempted, if any (*generated
    /// mode*). It lives outside the bank until saved; while `Some` it is what
    /// [`open_problem_ref`](App::open_problem_ref) returns.
    pub generated: Option<Problem>,
    /// Whether the active generated problem was cross-verified by the engine.
    pub generated_verified: bool,
    /// The one-line verdict/explanation for the active generated problem.
    pub generated_note: Option<String>,
    /// Receiver for an in-flight generation run, if any.
    gen_rx: Option<Receiver<generate::GenEvent>>,
    /// A transient status line for generation progress / errors.
    pub gen_status: Option<String>,
    /// When `true`, the main loop tears down the terminal and exits.
    pub should_quit: bool,
}

impl App {
    /// Builds the initial state from a loaded problem bank and a vault path,
    /// opening the first problem (if any).
    pub fn new(problems: Vec<Problem>, vault_path: PathBuf) -> Self {
        let nav_rows = build_nav_rows(&problems, |_, _| true);
        let mut topics: Vec<String> = problems.iter().map(|p| p.topic.clone()).collect();
        topics.sort();
        topics.dedup();
        let mut app = Self {
            problems,
            nav_rows,
            nav_state: ListState::default(),
            topic_filter: None,
            topics,
            review_due: None,
            session: None,
            session_summary: None,
            focus: Focus::Navigator,
            search_active: false,
            search_query: String::new(),
            filtered: Vec::new(),
            search_state: ListState::default(),
            open_problem: None,
            input: String::new(),
            verdict: None,
            verify_job: None,
            attempts: 0,
            show_recap: true,
            description_scroll: 0,
            console_scroll: 0,
            vault_path,
            opened_at: Instant::now(),
            exported_to: None,
            open_history: None,
            stats_visible: false,
            stats: None,
            agent_visible: true,
            agent_messages: Vec::new(),
            agent_input: String::new(),
            agent_streaming: false,
            agent_rx: None,
            agent_model: agent::default_model(),
            agent_url: agent::default_url(),
            search_semantic: false,
            embed_model: semantic::default_model(),
            embed_store: None,
            search_rx: None,
            semantic_done_query: None,
            search_status: None,
            generated: None,
            generated_verified: false,
            generated_note: None,
            gen_rx: None,
            gen_status: None,
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

    /// Index into [`problems`](App::problems) the user currently points at —
    /// the highlighted search result while the finder is open, otherwise the
    /// navigator selection. `None` on a header row or an empty result set.
    pub fn selected_index(&self) -> Option<usize> {
        if self.search_active {
            self.filtered
                .get(self.search_state.selected()?)
                .map(|&(i, _)| i)
        } else {
            match self.nav_rows.get(self.nav_state.selected()?)? {
                NavRow::Problem(i) => Some(*i),
                _ => None,
            }
        }
    }

    /// The problem the user currently points at (search-aware), if any. Drives
    /// the finder's preview; the navigator otherwise tracks the open problem.
    pub fn selected_problem(&self) -> Option<&Problem> {
        self.problems.get(self.selected_index()?)
    }

    /// The problem currently open for attempting (what verifies and exports).
    /// In *generated mode* this is the AI-authored problem held in
    /// [`generated`](App::generated), which lives outside the bank; otherwise
    /// it's the bank problem at [`open_problem`](App::open_problem).
    pub fn open_problem_ref(&self) -> Option<&Problem> {
        if let Some(p) = &self.generated {
            return Some(p);
        }
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

        // Ctrl-A toggles the AI tutor panel from anywhere.
        if key.code == KeyCode::Char('a') && ctrl {
            self.toggle_agent();
            return;
        }
        // Ctrl-E asks the tutor to explain the last wrong answer.
        if key.code == KeyCode::Char('e') && ctrl {
            self.ask_why_wrong();
            return;
        }
        // Ctrl-G generates a fresh, similar problem and cross-verifies it with
        // the engine before opening it to attempt.
        if key.code == KeyCode::Char('g') && ctrl {
            self.generate_verified();
            return;
        }
        // Ctrl-S saves the active generated problem into the bank.
        if key.code == KeyCode::Char('s') && ctrl {
            self.save_generated();
            return;
        }
        // The tutor input owns the keyboard while it has focus (so `/`, `t`,
        // `r`, etc. are literal characters there).
        if self.focus == Focus::Agent {
            self.on_agent_key(key, ctrl);
            return;
        }

        // Ctrl-P toggles the progress/stats overlay from anywhere.
        if key.code == KeyCode::Char('p') && ctrl {
            self.toggle_stats();
            return;
        }
        // The stats overlay is modal: Esc or q closes it, and it consumes every
        // other key so stray input never leaks to the panes behind it.
        if self.stats_visible {
            if matches!(key.code, KeyCode::Esc | KeyCode::Char('q')) {
                self.stats_visible = false;
            }
            return;
        }

        // -- The fuzzy finder owns the keyboard while it is open ----------
        if self.search_active {
            self.on_search_key(key, ctrl);
            return;
        }

        // -- The answer bar owns the keyboard while it has focus ----------
        if self.focus == Focus::Input {
            self.on_input_key(key, ctrl);
            return;
        }

        // `/` opens the fuzzy finder from any non-input pane (it stays a
        // literal character — e.g. in a fraction — while typing an answer).
        if key.code == KeyCode::Char('/') && !ctrl {
            self.enter_search();
            return;
        }

        // `t` cycles the navigator's topic filter (also non-input only).
        if key.code == KeyCode::Char('t') && !ctrl {
            self.cycle_topic_filter();
            return;
        }

        // `r` toggles spaced-repetition review mode (also non-input only).
        if key.code == KeyCode::Char('r') && !ctrl {
            self.toggle_review();
            return;
        }

        // `g` toggles the recap (preliminary reading) in the description pane.
        if key.code == KeyCode::Char('g') && !ctrl {
            self.show_recap = !self.show_recap;
            self.description_scroll = 0;
            return;
        }

        // `s` starts or ends a focused study session over the due problems.
        if key.code == KeyCode::Char('s') && !ctrl {
            self.toggle_session();
            return;
        }

        // `n` jumps to the adaptively-recommended next problem.
        if key.code == KeyCode::Char('n') && !ctrl {
            self.jump_to_next();
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
            Focus::Input | Focus::Agent => {} // routed earlier; never reached
        }
    }

    /// Cycles the navigator's topic filter: `None` → first topic → … → last →
    /// `None`, rebuilding the visible rows each step.
    fn cycle_topic_filter(&mut self) {
        self.topic_filter = match &self.topic_filter {
            None => self.topics.first().cloned(),
            Some(current) => {
                let i = self.topics.iter().position(|t| t == current).unwrap_or(0);
                self.topics.get(i + 1).cloned() // past the end → back to "all"
            }
        };
        self.rebuild_nav();
    }

    /// Toggles spaced-repetition review mode. On entry it scans the vault to
    /// find every problem due for review (see [`crate::review`]); the navigator
    /// then shows only those.
    fn toggle_review(&mut self) {
        self.review_due = match self.review_due {
            Some(_) => None,
            None => Some(self.due_problems()),
        };
        self.rebuild_nav();
    }

    /// Jumps to the adaptively-recommended next problem (see
    /// [`crate::recommend`]): opens it, moves the navigator cursor onto it (if
    /// visible under the current filter), drops the user into the answer bar, and
    /// flashes the rationale in the console. A no-op for an empty bank.
    fn jump_to_next(&mut self) {
        let today = Local::now().date_naive();
        if let Some(pick) = recommend::next(&self.problems, &self.vault_path, today) {
            self.open_index(pick.index);
            self.select_problem_row(pick.index);
            self.focus = Focus::Input;
            self.session_summary = Some(format!(
                "➡ Next: {} · {}",
                self.problems[pick.index].title, pick.reason
            ));
        }
    }

    /// The set of problem indices due for review as of today (scans the vault).
    fn due_problems(&self) -> HashSet<usize> {
        let today = Local::now().date_naive();
        self.problems
            .iter()
            .enumerate()
            .filter(|(_, p)| review::is_due(vault::history(&self.vault_path, p).as_ref(), today))
            .map(|(i, _)| i)
            .collect()
    }

    /// Starts a study session over today's due problems, or ends the running
    /// one with a summary. An empty due set just reports "all caught up".
    fn toggle_session(&mut self) {
        if self.session.is_some() {
            self.end_session();
            return;
        }
        let due = self.due_problems();
        if due.is_empty() {
            self.session_summary = Some("✓ Nothing due — all caught up!".into());
            return;
        }
        self.session = Some(Session {
            due,
            solved: HashSet::new(),
            started: std::time::Instant::now(),
        });
        self.session_summary = None;
        self.rebuild_nav();
    }

    /// Records the just-solved open problem against the active session, and
    /// auto-ends the session (with its summary) once everything is solved.
    fn mark_session_solved(&mut self) {
        let idx = self.open_problem;
        if let (Some(session), Some(i)) = (self.session.as_mut(), idx)
            && session.due.contains(&i)
        {
            session.solved.insert(i);
        }
        if self.session.as_ref().is_some_and(Session::is_complete) {
            self.end_session();
        }
    }

    /// Ends the running session, recording a one-line summary.
    fn end_session(&mut self) {
        if let Some(s) = self.session.take() {
            let (done, total, mins) = (s.solved.len(), s.due.len(), s.minutes());
            let tail = if done == total {
                "🎉 all done!"
            } else {
                "keep going"
            };
            self.session_summary = Some(format!(
                "📚 Session ended — {done}/{total} solved in {mins}m · {tail}"
            ));
        }
        self.rebuild_nav();
    }

    /// Rebuilds the navigator rows for the current topic + review filters
    /// (combined with AND) and opens the first still-visible problem.
    fn rebuild_nav(&mut self) {
        let topic = self.topic_filter.clone();
        let due = self.review_due.clone();
        let session = self.session.as_ref().map(|s| s.due.clone());
        self.nav_rows = build_nav_rows(&self.problems, |i, p| {
            topic.as_deref().is_none_or(|t| p.topic == t)
                && due.as_ref().is_none_or(|set| set.contains(&i))
                && session.as_ref().is_none_or(|set| set.contains(&i))
        });
        match self
            .nav_rows
            .iter()
            .position(|r| matches!(r, NavRow::Problem(_)))
        {
            Some(first) => {
                self.nav_state.select(Some(first));
                self.open_selected();
            }
            None => self.nav_state.select(None),
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

    // -- Fuzzy finder (`/`) --------------------------------------------------

    /// Opens the finder with an empty query (all problems listed).
    fn enter_search(&mut self) {
        self.search_active = true;
        self.stats_visible = false; // the two modals are mutually exclusive
        self.search_query.clear();
        self.search_semantic = false;
        self.semantic_done_query = None;
        self.search_status = None;
        self.focus = Focus::Navigator;
        self.recompute_filter();
    }

    /// Closes the finder, restoring the navigator selection to whatever problem
    /// is currently open (or the first problem).
    fn exit_search(&mut self) {
        self.search_active = false;
        self.search_query.clear();
        self.search_status = None;
        self.semantic_done_query = None;
        match self.open_problem {
            Some(idx) => self.select_problem_row(idx),
            None => {
                if let Some(first) = self
                    .nav_rows
                    .iter()
                    .position(|r| matches!(r, NavRow::Problem(_)))
                {
                    self.nav_state.select(Some(first));
                }
            }
        }
    }

    /// Keys handled while the finder is active: printable characters (including
    /// `j`/`k`) extend the query, arrows move the highlight, Enter opens the
    /// highlighted problem and jumps to the answer bar, Esc cancels.
    fn on_search_key(&mut self, key: KeyEvent, ctrl: bool) {
        match key.code {
            KeyCode::Esc => self.exit_search(),
            // `Tab` flips between fuzzy (character) and semantic (meaning) modes.
            KeyCode::Tab | KeyCode::BackTab => self.toggle_search_mode(),
            // In semantic mode, the first Enter runs the search (the results
            // aren't live); once they match the query, Enter opens the highlight.
            KeyCode::Enter
                if self.search_semantic
                    && self.semantic_done_query.as_deref() != Some(self.search_query.trim())
                    && !self.search_query.trim().is_empty() =>
            {
                self.run_semantic_search();
            }
            KeyCode::Enter if self.selected_index().is_some() => {
                // `open_selected` is search-aware while the finder is still
                // active, so it opens the highlighted result.
                self.open_selected();
                self.exit_search();
                self.focus = Focus::Input;
            }
            KeyCode::Up => self.search_move(-1),
            KeyCode::Down => self.search_move(1),
            KeyCode::Backspace => {
                self.search_query.pop();
                self.on_query_edited();
            }
            KeyCode::Char(c) if !ctrl => {
                self.search_query.push(c);
                self.on_query_edited();
            }
            _ => {}
        }
    }

    /// Reacts to a query edit: fuzzy mode filters live; semantic mode defers the
    /// (network) search to Enter, so it just invalidates the stale results and
    /// shows a hint.
    fn on_query_edited(&mut self) {
        if self.search_semantic {
            self.filtered.clear();
            self.search_state.select(None);
            self.semantic_done_query = None;
            self.search_status = (!self.search_query.trim().is_empty())
                .then(|| "press ↵ to search by meaning".to_string());
        } else {
            self.recompute_filter();
        }
    }

    /// Flips the finder between fuzzy and semantic ranking, keeping the query.
    fn toggle_search_mode(&mut self) {
        self.search_semantic = !self.search_semantic;
        if self.search_semantic {
            self.filtered.clear();
            self.search_state.select(None);
            self.semantic_done_query = None;
            self.search_status = if self.search_query.trim().is_empty() {
                Some("type a description, then ↵ to search by meaning".into())
            } else {
                Some("press ↵ to search by meaning".into())
            };
        } else {
            self.search_status = None;
            self.semantic_done_query = None;
            self.recompute_filter();
        }
    }

    /// Launches a semantic search worker for the current query (a no-op if one
    /// is already running or the query is blank). The first run also indexes the
    /// bank; later runs reuse the in-memory [`embed_store`](App::embed_store).
    fn run_semantic_search(&mut self) {
        let query = self.search_query.trim().to_string();
        if query.is_empty() || self.search_rx.is_some() {
            return;
        }
        let docs: Vec<semantic::Doc> = self
            .problems
            .iter()
            .map(|p| semantic::Doc {
                id: p.id.clone(),
                text: format!("{} — {} · {}\n{}", p.title, p.topic, p.domain, p.prompt),
            })
            .collect();
        let cache = crate::storage::default_embeddings_path()
            .unwrap_or_else(|_| std::path::PathBuf::from("mamba-embeddings.json"));
        self.search_status = Some(if self.embed_store.is_none() {
            "indexing the bank… (first run only)".into()
        } else {
            "searching…".into()
        });
        self.semantic_done_query = Some(query.clone());
        self.search_rx = Some(semantic::search(
            self.agent_url.clone(),
            self.embed_model.clone(),
            cache,
            docs,
            self.embed_store.clone(),
            query,
        ));
    }

    /// `true` while a semantic search worker is in flight (drives the faster
    /// main-loop poll so results land promptly).
    pub fn search_pending(&self) -> bool {
        self.search_rx.is_some()
    }

    /// Drains a finished semantic search into [`filtered`](App::filtered).
    /// Called once per main-loop tick, like the verification and tutor pollers.
    pub fn poll_search(&mut self) {
        let Some(rx) = &self.search_rx else {
            return;
        };
        // Drain everything available this tick; keep the latest indexing count.
        loop {
            match rx.try_recv() {
                Ok(semantic::SearchEvent::Indexing { done, total }) => {
                    self.search_status = Some(format!("indexing the bank… {done}/{total}"));
                }
                Ok(semantic::SearchEvent::Ranked { store, results }) => {
                    if let Some(s) = store {
                        self.embed_store = Some(s);
                    }
                    self.filtered = results
                        .into_iter()
                        // Cosine [-1, 1] → permille, reusing the i64 score slot.
                        .map(|(i, sim)| (i, (sim * 1000.0) as i64))
                        .collect();
                    self.search_state
                        .select((!self.filtered.is_empty()).then_some(0));
                    self.search_status = self.filtered.is_empty().then(|| "no matches".to_string());
                    self.search_rx = None;
                    return;
                }
                Ok(semantic::SearchEvent::Error(e)) => {
                    self.search_status = Some(format!("⚠ {e}"));
                    self.semantic_done_query = None;
                    self.search_rx = None;
                    return;
                }
                Err(TryRecvError::Empty) => return, // still working — next tick
                Err(TryRecvError::Disconnected) => {
                    self.search_status = Some("⚠ semantic search worker stopped".into());
                    self.semantic_done_query = None;
                    self.search_rx = None;
                    return;
                }
            }
        }
    }

    /// Recomputes [`filtered`](App::filtered) for the current query, ranking
    /// matches best-first, and resets the highlight to the top match. An empty
    /// query lists every problem (score 0).
    fn recompute_filter(&mut self) {
        if self.search_query.is_empty() {
            self.filtered = (0..self.problems.len()).map(|i| (i, 0)).collect();
        } else {
            let matcher = SkimMatcherV2::default();
            let mut scored: Vec<(usize, i64)> = self
                .problems
                .iter()
                .enumerate()
                .filter_map(|(i, p)| {
                    // Match against title, topic, difficulty, and domain, so
                    // "calculus", "hard", or "exact" also narrow the list.
                    let hay = format!("{} {} {} {}", p.title, p.topic, p.difficulty, p.domain);
                    matcher
                        .fuzzy_match(&hay, &self.search_query)
                        .map(|s| (i, s))
                })
                .collect();
            // Highest score first; ties keep the bank's order.
            scored.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            self.filtered = scored;
        }
        self.search_state
            .select((!self.filtered.is_empty()).then_some(0));
    }

    /// Moves the search highlight within the filtered results (no open).
    fn search_move(&mut self, dir: isize) {
        if self.filtered.is_empty() {
            return;
        }
        let cur = self.search_state.selected().unwrap_or(0) as isize;
        let next = (cur + dir).clamp(0, self.filtered.len() as isize - 1) as usize;
        self.search_state.select(Some(next));
    }

    /// Selects the `NavRow::Problem(idx)` row in the (non-search) navigator.
    fn select_problem_row(&mut self, idx: usize) {
        if let Some(row) = self
            .nav_rows
            .iter()
            .position(|r| matches!(r, NavRow::Problem(i) if *i == idx))
        {
            self.nav_state.select(Some(row));
        }
    }

    /// Keys handled while the answer bar has focus. Enter submits; printable
    /// characters extend the answer; Esc/Tab leave the bar.
    fn on_input_key(&mut self, key: KeyEvent, ctrl: bool) {
        match key.code {
            KeyCode::Esc => self.focus = Focus::Navigator,
            KeyCode::Tab => self.focus = self.focus.next(),
            KeyCode::BackTab => self.focus = self.focus.prev(),
            KeyCode::Enter => self.submit_or_ask(),
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
        self.open_index(idx);
    }

    /// Opens the problem at `idx` (an index into [`problems`](App::problems)),
    /// resetting all per-problem state (input, verdict, attempts, scroll, timer)
    /// and cancelling any in-flight verification. Unlike [`open_selected`], this
    /// works from any source — the navigator cursor, the fuzzy finder, or the
    /// `n` recommendation — not just the current selection.
    fn open_index(&mut self, idx: usize) {
        // Opening a bank problem leaves generated mode.
        self.generated = None;
        self.generated_note = None;
        self.open_problem = Some(idx);
        self.input.clear();
        self.verdict = None;
        // Cancel any in-flight verification for the previous problem: dropping
        // the job drops the receiver, so the worker's result is discarded.
        self.verify_job = None;
        self.attempts = 0;
        self.description_scroll = 0;
        self.console_scroll = 0;
        self.exported_to = None;
        // The tutor chat is scoped to the open problem, so it persists cleanly
        // into that problem's note on a solve.
        self.agent_messages.clear();
        self.agent_input.clear();
        self.agent_rx = None;
        self.agent_streaming = false;
        self.opened_at = Instant::now();
        self.refresh_open_history();
    }

    /// Formats the current tutor conversation as a Markdown transcript for the
    /// vault note, or `None` if there's nothing worth saving.
    fn tutor_transcript(&self) -> Option<String> {
        let lines: Vec<String> = self
            .agent_messages
            .iter()
            .filter(|m| !m.content.trim().is_empty())
            .map(|m| {
                let who = if m.role == "user" {
                    "**you:**"
                } else {
                    "**tutor:**"
                };
                format!("{who} {}", m.content.trim())
            })
            .collect();
        (!lines.is_empty()).then(|| lines.join("\n\n"))
    }

    /// Reads the open problem's solve history from the vault (if it has a note),
    /// so the console can show a "solved N×" peek. Called on open and re-read
    /// after each solve.
    fn refresh_open_history(&mut self) {
        self.open_history = self
            .open_problem_ref()
            .cloned()
            .and_then(|problem| vault::history(&self.vault_path, &problem));
    }

    /// Enter in the answer bar: if the text starts with `?`, send the rest to
    /// the tutor as a one-shot question (showing the panel); otherwise verify it
    /// as an answer.
    fn submit_or_ask(&mut self) {
        if let Some(question) = self.input.strip_prefix('?') {
            let question = question.trim().to_string();
            self.input.clear();
            self.agent_visible = true;
            self.focus = Focus::Agent;
            self.send_message(if question.is_empty() {
                "Give me a hint for this problem.".into()
            } else {
                question
            });
        } else {
            self.submit_answer();
        }
    }

    /// Kicks off verification of the typed answer **on a worker thread**, so a
    /// slow check (a future CAS, a large factorization) never blocks input. The
    /// worker runs the [universal router](engine::verify) and sends back only a
    /// [`Verdict`]; the result is finalized on the main thread by
    /// [`poll_verify`](App::poll_verify). The UI shows `Verifying…` meanwhile.
    fn submit_answer(&mut self) {
        // One verification at a time — ignore Enter while one is in flight.
        if self.verify_job.is_some() {
            return;
        }
        let Some(problem) = self.open_problem_ref().cloned() else {
            return;
        };
        let answer = self.input.trim().to_string();
        if answer.is_empty() {
            return; // don't spawn a worker — or count an attempt — for a blank
        }

        self.attempts += 1;
        self.console_scroll = 0;
        self.session_summary = None; // a fresh attempt supersedes any old summary
        self.verdict = None; // clear the previous result while we recompute

        // Owned copies move into the worker; it returns only the `Verdict`, so
        // every App mutation (the vault export included) stays on this thread.
        let (tx, rx) = mpsc::channel();
        let job_problem = problem.clone();
        let job_answer = answer.clone();
        thread::spawn(move || {
            let verdict = engine::verify(&job_problem, &job_answer);
            // A send error just means the receiver was dropped (the user moved
            // on) — the stale result is harmlessly discarded.
            let _ = tx.send(verdict);
        });

        self.verify_job = Some(VerifyJob {
            problem,
            answer,
            opened_at: self.opened_at,
            rx,
        });
    }

    /// Drains a finished worker verification into state: records the verdict
    /// and, on success, exports the *captured* solve to the Markdown vault.
    /// Called once per main-loop tick, so the result appears promptly without
    /// the UI ever having blocked on it. A no-op when nothing is in flight.
    pub fn poll_verify(&mut self) {
        let Some(job) = &self.verify_job else {
            return;
        };
        match job.rx.try_recv() {
            Ok(verdict) => {
                let job = self.verify_job.take().expect("just matched Some");
                if verdict.is_correct() {
                    self.export_solution(&job.problem, &job.answer, job.opened_at);
                    self.mark_session_solved();
                }
                self.verdict = Some(verdict);
                self.console_scroll = 0;
            }
            Err(TryRecvError::Empty) => {} // still computing — try again next tick
            Err(TryRecvError::Disconnected) => {
                // The worker panicked or exited without sending — surface it
                // rather than spin forever showing `Verifying…`.
                self.verify_job = None;
                self.verdict = Some(Verdict::incorrect(
                    "verification worker stopped before returning a result",
                ));
            }
        }
    }

    /// `true` while a verification worker is in flight (drives the `Verifying…`
    /// console state and the faster main-loop poll).
    pub fn is_verifying(&self) -> bool {
        self.verify_job.is_some()
    }

    /// Shows or hides the progress overlay, recomputing the snapshot from the
    /// vault on open. Closes the fuzzy finder so the two modals never overlap.
    fn toggle_stats(&mut self) {
        if self.stats_visible {
            self.stats_visible = false;
        } else {
            let today = Local::now().date_naive();
            self.stats = Some(progress::stats(&self.vault_path, today));
            self.search_active = false;
            self.stats_visible = true;
        }
    }

    // -- AI tutor (`Ctrl-A`) -------------------------------------------------

    /// Summon/dismiss the tutor: if it already has focus, hide it and return to
    /// the answer bar; otherwise show it (if needed) and focus it.
    fn toggle_agent(&mut self) {
        if self.focus == Focus::Agent {
            self.agent_visible = false;
            self.focus = Focus::Input;
        } else {
            self.agent_visible = true;
            self.focus = Focus::Agent;
        }
    }

    /// Keys handled while the tutor input has focus. Enter sends; Esc/Tab leave
    /// the input (the panel stays visible); printable chars extend the prompt.
    fn on_agent_key(&mut self, key: KeyEvent, ctrl: bool) {
        match key.code {
            KeyCode::Esc | KeyCode::Tab => self.focus = Focus::Input,
            KeyCode::Enter => self.send_prompt(),
            KeyCode::Backspace => {
                self.agent_input.pop();
            }
            KeyCode::Char(c) if !ctrl => self.agent_input.push(c),
            _ => {}
        }
    }

    /// A system prompt giving the model the current problem, the user's
    /// in-progress answer, and the Quick Guide (so it can remind the user of
    /// keybindings/commands when asked), so its help is targeted.
    fn agent_context(&self) -> String {
        /// The user-facing reference, embedded at compile time so the tutor can
        /// answer questions about keys, commands, and configuration.
        const QUICK_GUIDE: &str = include_str!("../QUICKGUIDE.md");

        let base = "You are a concise math tutor inside Mamba, a terminal practice tool. \
             Prefer hints, the method, and intuition over giving the full answer unless \
             explicitly asked. Keep replies short and terminal-friendly — plain text, no \
             markdown tables. If the user asks about Mamba's keys, shortcuts, commands, or \
             configuration, answer from the QUICK GUIDE below.";

        let problem = match self.open_problem_ref() {
            Some(p) => format!(
                "\n\nThe user is working on:\n{} [{}] — {} · {}\n\n{}\n\nTheir current answer attempt:\n{}",
                p.title,
                p.difficulty,
                p.domain,
                p.topic,
                p.prompt,
                if self.input.trim().is_empty() {
                    "(blank)"
                } else {
                    self.input.trim()
                },
            ),
            None => String::new(),
        };

        format!("{base}{problem}\n\n--- MAMBA QUICK GUIDE ---\n{QUICK_GUIDE}")
    }

    /// Sends the typed prompt to the tutor and begins streaming the reply.
    fn send_prompt(&mut self) {
        if self.agent_streaming {
            return;
        }
        let prompt = std::mem::take(&mut self.agent_input);
        self.send_message(prompt);
    }

    /// Appends a user message and streams a reply. The empty assistant turn is
    /// the sink the streamed tokens append into.
    fn send_message(&mut self, user_text: String) {
        if self.agent_streaming {
            return;
        }
        let text = user_text.trim().to_string();
        if text.is_empty() {
            return;
        }
        self.agent_messages.push(ChatMessage::user(text));

        let mut wire = vec![ChatMessage::system(self.agent_context())];
        wire.extend(self.agent_messages.iter().cloned());
        self.agent_messages
            .push(ChatMessage::assistant(String::new()));

        self.agent_streaming = true;
        self.agent_rx = Some(agent::stream_chat(
            self.agent_url.clone(),
            self.agent_model.clone(),
            wire,
        ));
    }

    /// Asks the tutor to explain the most recent *failed* verdict — wired to
    /// `Ctrl-E`. Shows and focuses the panel; a no-op when the last result was
    /// correct or there is none.
    fn ask_why_wrong(&mut self) {
        let detail = match &self.verdict {
            Some(Verdict::Incorrect { detail }) => detail.clone(),
            Some(Verdict::ParseError { message, .. }) => message.clone(),
            _ => return, // nothing to explain
        };
        if self.agent_streaming {
            return;
        }
        let prompt = format!(
            "I answered \"{}\" but it was marked wrong ({detail}). Without giving the \
             full answer, explain what's going wrong and how to fix my approach.",
            self.input.trim()
        );
        self.agent_visible = true;
        self.focus = Focus::Agent;
        self.send_message(prompt);
    }

    /// Generates a fresh problem in the vein of the open one and cross-verifies
    /// it with the engine — wired to `Ctrl-G`. Spawns the off-thread pipeline
    /// (see [`crate::generate`]); [`poll_generate`](App::poll_generate) opens the
    /// result. A no-op when no problem is open or a run is already in flight.
    fn generate_verified(&mut self) {
        if self.gen_rx.is_some() {
            return;
        }
        let Some(reference) = self.open_problem_ref() else {
            return;
        };
        let spec = generate::Spec {
            topic: reference.topic.clone(),
            domain: reference.domain,
            difficulty: reference.difficulty,
            reference_prompt: reference.prompt.clone(),
            reference_expected: reference.expected.clone(),
        };
        self.gen_status = Some("✨ Generating a problem…".into());
        self.gen_rx = Some(generate::generate(
            self.agent_url.clone(),
            self.agent_model.clone(),
            spec,
        ));
    }

    /// `true` while a generation run is in flight (drives the faster poll).
    pub fn gen_pending(&self) -> bool {
        self.gen_rx.is_some()
    }

    /// Drains a finished generation run: progress lines update the status, and a
    /// completed problem is opened in *generated mode* for the user to attempt.
    /// Called once per main-loop tick like the other pollers.
    pub fn poll_generate(&mut self) {
        let Some(rx) = &self.gen_rx else {
            return;
        };
        loop {
            match rx.try_recv() {
                Ok(generate::GenEvent::Status(s)) => self.gen_status = Some(format!("✨ {s}")),
                Ok(generate::GenEvent::Done {
                    problem,
                    verified,
                    note,
                }) => {
                    self.open_generated(*problem, verified, note);
                    self.gen_rx = None;
                    return;
                }
                Ok(generate::GenEvent::Error(e)) => {
                    self.gen_status = Some(format!("⚠ {e}"));
                    self.gen_rx = None;
                    return;
                }
                Err(TryRecvError::Empty) => return, // still working — next tick
                Err(TryRecvError::Disconnected) => {
                    self.gen_status = Some("⚠ the generator stopped unexpectedly".into());
                    self.gen_rx = None;
                    return;
                }
            }
        }
    }

    /// Enters generated mode: makes `problem` the active (off-bank) problem,
    /// resetting per-problem state exactly as opening a bank problem would.
    fn open_generated(&mut self, problem: Problem, verified: bool, note: String) {
        // Cancel any in-flight verification and clear the per-problem slate.
        self.verify_job = None;
        self.open_problem = None; // generated mode: no bank index
        self.generated = Some(problem);
        self.generated_verified = verified;
        self.generated_note = Some(note);
        self.gen_status = None;
        self.input.clear();
        self.verdict = None;
        self.attempts = 0;
        self.description_scroll = 0;
        self.console_scroll = 0;
        self.exported_to = None;
        self.session_summary = None;
        self.agent_messages.clear();
        self.agent_input.clear();
        self.agent_rx = None;
        self.agent_streaming = false;
        self.opened_at = Instant::now();
        self.refresh_open_history();
        self.focus = Focus::Input;
    }

    /// Saves the active generated problem into the bank: writes its JSON to the
    /// problems directory and adds it to the live, sorted bank. Wired to
    /// `Ctrl-S` while in generated mode. A no-op otherwise.
    fn save_generated(&mut self) {
        let Some(mut problem) = self.generated.clone() else {
            return;
        };
        // Ensure a unique id (and thus filename) against the live bank.
        let mut id = problem.id.clone();
        let mut n = 2;
        while self.problems.iter().any(|p| p.id == id) {
            id = format!("{}-{n}", problem.id);
            n += 1;
        }
        problem.id = id.clone();

        let dir = match crate::storage::default_problems_path() {
            Ok(d) => d,
            Err(e) => {
                self.gen_status = Some(format!("⚠ couldn't resolve the bank dir: {e}"));
                return;
            }
        };
        let path = dir.join(format!("{id}.json"));
        let json = serde_json::to_string_pretty(&problem).unwrap_or_default();
        match std::fs::write(&path, json + "\n") {
            Ok(()) => {
                // Add to the live bank (re-sorted so the navigator groups it).
                self.problems.push(problem);
                self.problems.sort_by(|a, b| {
                    (a.domain, &a.topic, a.difficulty, &a.title).cmp(&(
                        b.domain,
                        &b.topic,
                        b.difficulty,
                        &b.title,
                    ))
                });
                self.rebuild_nav();
                self.generated_note = Some(format!("💾 Saved to the bank as {id}.json"));
            }
            Err(e) => self.gen_status = Some(format!("⚠ save failed: {e}")),
        }
    }

    /// Drains any streamed tutor tokens into the last assistant message. Called
    /// once per main-loop tick so replies appear live.
    pub fn poll_agent(&mut self) {
        let mut tokens = String::new();
        let mut finished = false;
        let mut error = None;
        if let Some(rx) = &self.agent_rx {
            loop {
                match rx.try_recv() {
                    Ok(AgentEvent::Token(t)) => tokens.push_str(&t),
                    Ok(AgentEvent::Done) => {
                        finished = true;
                        break;
                    }
                    Ok(AgentEvent::Error(e)) => {
                        error = Some(e);
                        break;
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        finished = true;
                        break;
                    }
                }
            }
        }
        if !tokens.is_empty()
            && let Some(last) = self.agent_messages.last_mut()
        {
            last.content.push_str(&tokens);
        }
        if let Some(e) = error {
            if let Some(last) = self.agent_messages.last_mut()
                && last.role == "assistant"
                && last.content.is_empty()
            {
                last.content = format!("⚠ {e}");
            } else {
                self.agent_messages
                    .push(ChatMessage::assistant(format!("⚠ {e}")));
            }
            finished = true;
        }
        if finished {
            self.agent_streaming = false;
            self.agent_rx = None;
        }
    }

    /// Writes the accepted answer into the Markdown vault, recording any
    /// failure as a [`Verdict::Incorrect`] so a silent data-loss never hides
    /// behind a green check. `opened_at` is the captured open-time of the
    /// verified problem, so `time_spent` is correct even if the user has since
    /// navigated elsewhere.
    fn export_solution(&mut self, problem: &Problem, answer: &str, opened_at: Instant) {
        let today = Local::now().date_naive();
        let transcript = self.tutor_transcript();
        match vault::export(
            &self.vault_path,
            problem,
            answer,
            self.attempts,
            opened_at.elapsed(),
            today,
            transcript.as_deref(),
        ) {
            Ok(path) => {
                self.exported_to = Some(path);
                // Best-effort: refresh the rolling progress dashboard. A failure
                // here must not sink an otherwise-correct, already-saved solve.
                let _ = progress::update(&self.vault_path, today);
                self.refresh_open_history();
            }
            Err(err) => {
                self.verdict = Some(Verdict::incorrect(format!("export failed: {err:#}")));
            }
        }
    }
}

/// Flattens the problem bank into navigator rows, inserting a domain header
/// when the domain changes and a topic sub-header when the topic changes within
/// a domain. Assumes `problems` is sorted by (domain, topic, …) —
/// [`crate::storage::problem::load_all`] guarantees this.
fn build_nav_rows(
    problems: &[Problem],
    is_visible: impl Fn(usize, &Problem) -> bool,
) -> Vec<NavRow> {
    let mut rows = Vec::with_capacity(problems.len() + 16);
    let mut last_domain: Option<String> = None;
    let mut last_topic: Option<String> = None;
    for (i, p) in problems.iter().enumerate() {
        if !is_visible(i, p) {
            continue; // hidden by the active topic / review filter
        }
        let domain = p.domain.to_string();
        if last_domain.as_deref() != Some(domain.as_str()) {
            rows.push(NavRow::DomainHeader(domain.clone()));
            last_domain = Some(domain);
            last_topic = None; // topics restart within each new domain
        }
        if last_topic.as_deref() != Some(p.topic.as_str()) {
            rows.push(NavRow::TopicHeader(p.topic.clone()));
            last_topic = Some(p.topic.clone());
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
            recap: None,
        }
    }

    fn app() -> App {
        let problems = vec![
            problem(
                "Expand",
                Domain::Symbolic,
                "x^2 - x - 6",
                AnswerKind::Scalar,
            ),
            problem("Determinant", Domain::Tensor, "10", AnswerKind::Scalar),
            problem(
                "Factorize",
                Domain::Exact,
                "104729",
                AnswerKind::Factorization,
            ),
        ];
        App::new(problems, std::env::temp_dir().join("mamba-vault-test"))
    }

    /// Submits an answer and pumps `poll_verify` until the worker thread
    /// finishes, so tests can assert on the (now asynchronous) verdict.
    fn submit_blocking(app: &mut App, answer: &str) {
        app.input = answer.into();
        app.submit_answer();
        for _ in 0..2000 {
            app.poll_verify();
            if !app.is_verifying() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        panic!("verification did not complete within the timeout");
    }

    #[test]
    fn nav_rows_insert_one_domain_and_topic_header_per_run() {
        let a = app();
        let domain_headers = a
            .nav_rows
            .iter()
            .filter(|r| matches!(r, NavRow::DomainHeader(_)))
            .count();
        assert_eq!(domain_headers, 3); // Symbolic, Tensor, Exact
        // Each of the three (all topic "X") gets one topic sub-header too.
        let topic_headers = a
            .nav_rows
            .iter()
            .filter(|r| matches!(r, NavRow::TopicHeader(_)))
            .count();
        assert_eq!(topic_headers, 3);
        assert!(matches!(a.nav_rows[0], NavRow::DomainHeader(_)));
        // First selectable row is auto-opened.
        assert_eq!(a.open_problem, Some(0));
    }

    #[test]
    fn selection_skips_headers_in_both_directions() {
        let mut a = app();
        // Rows: [DH, TH, P0=2, DH, TH, P1=5, DH, TH, P2=8].
        assert_eq!(a.nav_state.selected(), Some(2));
        a.select_adjacent(1);
        assert_eq!(a.nav_state.selected(), Some(5)); // skipped a domain + topic header
        a.select_adjacent(-1);
        assert_eq!(a.nav_state.selected(), Some(2));
        a.select_adjacent(-1); // top edge — unchanged
        assert_eq!(a.nav_state.selected(), Some(2));
    }

    #[test]
    fn submitting_a_correct_answer_marks_it_and_exports() {
        let mut a = app();
        submit_blocking(&mut a, "(x-3)(x+2)");
        assert_eq!(a.verdict, Some(Verdict::Correct));
        assert_eq!(a.attempts, 1);
        assert!(a.exported_to.is_some());
    }

    #[test]
    fn open_history_peek_reflects_solves() {
        // A fresh, unique vault so the solve count is deterministic.
        let problems = vec![problem(
            "Expand",
            Domain::Symbolic,
            "x^2 - x - 6",
            AnswerKind::Scalar,
        )];
        let vault = std::env::temp_dir().join(format!("mamba-app-hist-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&vault);

        let mut a = App::new(problems, vault.clone());
        assert!(a.open_history.is_none()); // never solved yet
        submit_blocking(&mut a, "(x-3)(x+2)");
        assert_eq!(a.open_history.as_ref().map(|h| h.solves), Some(1));

        let _ = std::fs::remove_dir_all(&vault);
    }

    #[test]
    fn submitting_a_wrong_answer_does_not_export() {
        let mut a = app();
        submit_blocking(&mut a, "x^2 + x - 6");
        assert!(matches!(a.verdict, Some(Verdict::Incorrect { .. })));
        assert!(a.exported_to.is_none());
    }

    #[test]
    fn a_blank_answer_neither_verifies_nor_counts_an_attempt() {
        let mut a = app();
        a.input = "   ".into();
        a.submit_answer();
        assert!(!a.is_verifying());
        assert_eq!(a.attempts, 0);
        assert_eq!(a.verdict, None);
    }

    #[test]
    fn navigating_away_cancels_an_in_flight_verification() {
        let mut a = app();
        a.input = "(x-3)(x+2)".into();
        a.submit_answer();
        assert!(a.is_verifying());
        // Opening another problem must drop the job (and its receiver).
        a.select_adjacent(1);
        assert!(!a.is_verifying());
        assert_eq!(a.verdict, None);
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn topic_filter_cycles_and_filters_the_navigator() {
        let mut p1 = problem("A", Domain::Symbolic, "x", AnswerKind::Scalar);
        p1.topic = "Algebra".into();
        let mut p2 = problem("B", Domain::Symbolic, "x", AnswerKind::Scalar);
        p2.topic = "Calculus".into();
        let mut a = App::new(vec![p1, p2], std::env::temp_dir().join("mamba-topic-test"));

        let visible = |a: &App| {
            a.nav_rows
                .iter()
                .filter(|r| matches!(r, NavRow::Problem(_)))
                .count()
        };
        assert_eq!(
            a.topics,
            vec!["Algebra".to_string(), "Calculus".to_string()]
        );
        assert!(a.topic_filter.is_none());
        assert_eq!(visible(&a), 2);

        a.cycle_topic_filter(); // → Algebra
        assert_eq!(a.topic_filter.as_deref(), Some("Algebra"));
        assert_eq!(visible(&a), 1);

        a.cycle_topic_filter(); // → Calculus
        assert_eq!(a.topic_filter.as_deref(), Some("Calculus"));
        assert_eq!(visible(&a), 1);

        a.cycle_topic_filter(); // → all
        assert!(a.topic_filter.is_none());
        assert_eq!(visible(&a), 2);
    }

    #[test]
    fn review_mode_shows_only_due_problems() {
        let p1 = problem("Fresh", Domain::Symbolic, "x", AnswerKind::Scalar);
        let p2 = problem("Solved", Domain::Symbolic, "x", AnswerKind::Scalar);
        let vault = std::env::temp_dir().join(format!("mamba-review-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&vault);
        // Solve "Solved" today → not due yet (interval is 1 day); "Fresh" is new.
        let today = Local::now().date_naive();
        vault::export(
            &vault,
            &p2,
            "x",
            1,
            std::time::Duration::from_secs(1),
            today,
            None,
        )
        .unwrap();

        let mut a = App::new(vec![p1, p2], vault.clone());
        a.toggle_review();
        assert!(a.review_due.is_some());
        let titles: Vec<&str> = a
            .nav_rows
            .iter()
            .filter_map(|r| match r {
                NavRow::Problem(i) => Some(a.problems[*i].title.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(titles, vec!["Fresh"]); // only the never-solved one is due

        a.toggle_review();
        assert!(a.review_due.is_none());
        let _ = std::fs::remove_dir_all(&vault);
    }

    #[test]
    fn study_session_serves_the_due_queue_and_tracks_solves() {
        // Two never-solved problems → both due, so both join the session.
        let p1 = problem("Alpha", Domain::Symbolic, "x^2 - x - 6", AnswerKind::Scalar);
        let p2 = problem("Beta", Domain::Symbolic, "x^2 - x - 6", AnswerKind::Scalar);
        let vault = std::env::temp_dir().join(format!("mamba-session-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&vault);

        let mut a = App::new(vec![p1, p2], vault.clone());
        assert!(a.session.is_none());

        a.toggle_session();
        let s = a.session.as_ref().expect("session started");
        assert_eq!(s.total(), 2);
        assert_eq!(s.solved_count(), 0);

        // Solving the open problem advances the session (1/2 → not yet complete).
        submit_blocking(&mut a, "(x-3)(x+2)");
        assert_eq!(a.session.as_ref().map(Session::solved_count), Some(1));
        assert!(a.session.is_some());

        // Ending the session yields a summary and clears the filter.
        a.toggle_session();
        assert!(a.session.is_none());
        assert!(
            a.session_summary
                .as_deref()
                .is_some_and(|s| s.contains("Session ended") && s.contains("1/2"))
        );
        let _ = std::fs::remove_dir_all(&vault);
    }

    #[test]
    fn study_session_with_nothing_due_reports_caught_up() {
        let p = problem("Solved", Domain::Symbolic, "x", AnswerKind::Scalar);
        let vault =
            std::env::temp_dir().join(format!("mamba-session-caught-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&vault);
        // Solve it today → not due (interval is 1 day), so the queue is empty.
        vault::export(
            &vault,
            &p,
            "x",
            1,
            std::time::Duration::from_secs(1),
            Local::now().date_naive(),
            None,
        )
        .unwrap();

        let mut a = App::new(vec![p], vault.clone());
        a.toggle_session();
        assert!(a.session.is_none()); // nothing to serve
        assert!(
            a.session_summary
                .as_deref()
                .is_some_and(|s| s.contains("caught up"))
        );
        let _ = std::fs::remove_dir_all(&vault);
    }

    #[test]
    fn explain_is_a_noop_without_a_failed_verdict() {
        let mut a = app();
        a.ask_why_wrong(); // no verdict → no message, no focus change
        assert!(a.agent_messages.is_empty());
        assert_ne!(a.focus, Focus::Agent);
        a.verdict = Some(Verdict::Correct);
        a.ask_why_wrong(); // correct → nothing to explain
        assert!(a.agent_messages.is_empty());
        assert_ne!(a.focus, Focus::Agent);
    }

    #[test]
    fn explain_after_a_wrong_answer_messages_the_tutor() {
        let mut a = app();
        a.agent_url = "http://127.0.0.1:1".into(); // unreachable → worker fails fast
        a.input = "5".into();
        a.verdict = Some(Verdict::Incorrect {
            detail: "got 5, expected 6".into(),
        });
        a.ask_why_wrong();
        assert!(a.agent_visible);
        assert_eq!(a.focus, Focus::Agent);
        assert!(
            a.agent_messages
                .iter()
                .any(|m| m.role == "user" && m.content.contains("marked wrong"))
        );
    }

    #[test]
    fn generate_verified_starts_a_run_for_the_open_problem() {
        let mut a = app();
        a.agent_url = "http://127.0.0.1:1".into(); // unreachable → the worker errors out
        assert!(a.generated.is_none());
        a.generate_verified();
        // A run is in flight (the worker thread owns the request); status set.
        assert!(a.gen_pending());
        assert!(a.gen_status.is_some());
    }

    #[test]
    fn generated_mode_overrides_the_open_problem_and_clears_on_navigation() {
        let mut a = app();
        let made = problem(
            "AI Gen",
            Domain::Symbolic,
            "x^2 - x - 6",
            AnswerKind::Scalar,
        );
        a.open_generated(made, true, "ok".into());

        // The generated problem is now what verifies, not the bank one.
        assert!(a.generated.is_some());
        assert_eq!(
            a.open_problem_ref().map(|p| p.title.clone()),
            Some("AI Gen".to_string())
        );
        assert_eq!(a.focus, Focus::Input);
        submit_blocking(&mut a, "(x-3)(x+2)");
        assert_eq!(a.verdict, Some(Verdict::Correct));

        // Opening a bank problem leaves generated mode.
        a.open_index(0);
        assert!(a.generated.is_none());
    }

    #[test]
    fn question_prefix_asks_the_tutor_instead_of_verifying() {
        let mut a = app();
        a.agent_url = "http://127.0.0.1:1".into();
        a.input = "?what is a determinant".into();
        a.submit_or_ask();
        assert!(a.agent_visible);
        assert_eq!(a.focus, Focus::Agent);
        assert!(a.input.is_empty());
        assert_eq!(a.verdict, None); // routed to the tutor, not verified
        assert!(
            a.agent_messages
                .iter()
                .any(|m| m.role == "user" && m.content.contains("determinant"))
        );
    }

    #[test]
    fn fuzzy_finder_filters_ranks_and_restores() {
        // app(): Expand (Symbolic), Determinant (Tensor), Factorize (Exact).
        let mut a = app();
        assert_eq!(a.open_problem, Some(0));

        a.enter_search();
        assert!(a.search_active);
        assert_eq!(a.filtered.len(), 3); // empty query lists every problem

        a.search_query.push_str("determ");
        a.recompute_filter();
        let idx = a.selected_index().expect("a match");
        assert_eq!(a.problems[idx].title, "Determinant");
        // The open problem is untouched while previewing search results.
        assert_eq!(a.open_problem, Some(0));

        // Esc cancels and restores the selection to the open problem.
        a.exit_search();
        assert!(!a.search_active);
        assert_eq!(a.selected_index(), Some(0));
    }

    #[test]
    fn finder_enter_opens_the_highlight_and_focuses_the_answer_bar() {
        let mut a = app();
        // `/` opens the finder from the navigator.
        a.on_key(key(KeyCode::Char('/')));
        assert!(a.search_active);

        for c in "factor".chars() {
            a.on_key(key(KeyCode::Char(c)));
        }
        a.on_key(key(KeyCode::Enter));

        assert!(!a.search_active);
        assert_eq!(a.focus, Focus::Input);
        assert_eq!(a.open_problem_ref().unwrap().title, "Factorize");
    }

    #[test]
    fn finder_tab_toggles_fuzzy_and_semantic_modes() {
        let mut a = app();
        a.on_key(key(KeyCode::Char('/')));
        assert!(a.search_active && !a.search_semantic);

        // Tab → semantic: results are dropped (they're not live) and a hint shows.
        a.on_key(key(KeyCode::Tab));
        assert!(a.search_semantic);
        assert!(a.filtered.is_empty());
        assert!(a.search_status.is_some());

        // Typing in semantic mode does NOT fuzzy-filter — the search is deferred
        // to Enter, so the result list stays empty until then.
        a.on_key(key(KeyCode::Char('d')));
        assert_eq!(a.search_query, "d");
        assert!(a.filtered.is_empty());

        // Tab back → fuzzy mode re-filters live against the same query.
        a.on_key(key(KeyCode::Tab));
        assert!(!a.search_semantic);
        assert!(a.search_status.is_none());
        assert!(!a.filtered.is_empty());
    }

    #[test]
    fn slash_is_a_literal_character_in_the_answer_bar() {
        let mut a = app();
        a.focus = Focus::Input;
        a.on_key(key(KeyCode::Char('1')));
        a.on_key(key(KeyCode::Char('/')));
        a.on_key(key(KeyCode::Char('2')));
        assert!(!a.search_active);
        assert_eq!(a.input, "1/2");
    }
}
