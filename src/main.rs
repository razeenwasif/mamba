//! # Mamba
//!
//! A local, terminal-based mathematics practice engine.
//!
//! Mamba renders an immediate-mode TUI (via [`ratatui`]) with four panes — a
//! domain-grouped problem navigator, a problem description, a bottom-anchored
//! answer bar, and a verification console — then routes the typed answer
//! through one of three specialized math pipelines (symbolic, tensor, exact)
//! and, on a correct answer, archives the solve as a Markdown note with YAML
//! frontmatter in a local Onyx/Obsidian-style vault.
//!
//! See the [library docs](mamba) for the architecture overview.

use std::time::Duration;

use anyhow::Context;
use crossterm::event::{self, Event, KeyEventKind};

use mamba::app::App;
use mamba::engine::{self, Verdict};
use mamba::{recommend, review, storage, ui};

/// How long the event loop blocks waiting for input before redrawing.
///
/// 100 ms keeps the UI responsive to terminal resizes while leaving the
/// process effectively idle (no busy-loop) when the user is thinking.
const TICK_RATE: Duration = Duration::from_millis(100);

/// A tighter poll used only while a verification worker is in flight, so the
/// `Verifying…` state flips to the verdict within a frame instead of waiting up
/// to a full [`TICK_RATE`].
const POLL_BUSY: Duration = Duration::from_millis(16);

fn main() -> anyhow::Result<()> {
    // Lightweight CLI handling *before* we touch the terminal, so `mamba
    // --version` and `mamba --help` work from any directory and are scriptable
    // (a TUI binary that only ever launches an alternate screen is impossible
    // to smoke-test on PATH). No `clap` dependency for two flags.
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("-V" | "--version") => {
            println!("mamba {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        Some("-h" | "--help") => {
            print_help();
            return Ok(());
        }
        // Non-interactive health check: resolve and load the problem bank, then
        // report — a scriptable way to confirm the installed binary finds its
        // problems from any directory.
        Some("--check") => {
            let dir = storage::default_problems_path()?;
            let problems = storage::problem::load_all(&dir)
                .with_context(|| format!("loading problem bank from {}", dir.display()))?;
            println!(
                "ok: {} problems loaded from {}",
                problems.len(),
                dir.display()
            );
            return Ok(());
        }
        // Run every problem's canonical answer back through the engine and print
        // PASS/FAIL, exiting non-zero on any failure. The bank generator uses
        // this to self-verify; it also gates hand edits. Optional directory
        // argument; defaults to the resolved bank.
        Some("--verify") => {
            let dir = match args.get(1) {
                Some(p) => std::path::PathBuf::from(p),
                None => storage::default_problems_path()?,
            };
            return verify_bank(&dir);
        }
        // `mamba solve <id> <answer…>` — verify one problem's answer from the
        // shell (no TUI). The answer may span several args (e.g. a matrix
        // `1 2 ; 3 4`); they are rejoined with spaces. Exit 0 iff correct.
        Some("solve") => {
            let id = args.get(1).map(String::as_str).unwrap_or_default();
            let answer = args.get(2..).map(|a| a.join(" ")).unwrap_or_default();
            return solve_one(id, &answer);
        }
        // `mamba review` — list the problems due for review (no TUI), using the
        // same spaced-repetition schedule and vault history as the `r` mode.
        Some("review") => return review_due(),
        // `mamba next` — print the single adaptively-recommended problem to do
        // next (the same pick the in-TUI `n` key jumps to).
        Some("next") => return recommend_next(),
        Some(other) => {
            eprintln!("mamba: unrecognized argument '{other}'\n");
            print_help();
            std::process::exit(2);
        }
        None => {} // no arguments → launch the TUI below
    }

    let problems_dir =
        storage::default_problems_path().context("failed to resolve the problem-bank directory")?;
    let problems = storage::problem::load_all(&problems_dir).with_context(|| {
        format!(
            "failed to load the problem bank from {} \
             (set $MAMBA_PROBLEMS, run from a checkout with ./problems, \
             or populate the data dir — see README)",
            problems_dir.display()
        )
    })?;
    let vault =
        storage::default_vault_path().context("failed to resolve the markdown vault path")?;

    let mut app = App::new(problems, vault);

    // `ratatui::init` enables raw mode + the alternate screen and installs a
    // panic hook that restores the terminal, so a crash never leaves the
    // user's shell in a broken state.
    let mut terminal = ratatui::init();
    let result = run(&mut terminal, &mut app);
    ratatui::restore();
    result
}

/// Loads every problem in `dir` and runs its canonical `expected` answer back
/// through the engine, printing a PASS/FAIL line per problem. Exits the process
/// with a non-zero status if any problem rejects its own answer, so it doubles
/// as a generator gate and a CI check.
fn verify_bank(dir: &std::path::Path) -> anyhow::Result<()> {
    let problems = storage::problem::load_all(dir)
        .with_context(|| format!("loading problem bank from {}", dir.display()))?;

    let mut failures = 0usize;
    for p in &problems {
        let verdict = engine::verify(p, &p.expected);
        if verdict.is_correct() {
            println!("  PASS  {:<30} [{}]", p.id, p.domain);
        } else {
            failures += 1;
            println!("  FAIL  {:<30} [{}]  {:?}", p.id, p.domain, verdict);
        }
    }
    println!(
        "\n{} problem(s) in {}: {} passed, {} failed",
        problems.len(),
        dir.display(),
        problems.len() - failures,
        failures
    );
    if failures > 0 {
        std::process::exit(1);
    }
    Ok(())
}

/// Verifies a single problem's answer by id and reports the verdict, exiting
/// non-zero if the answer is wrong or unparseable (so it is scriptable).
fn solve_one(id: &str, answer: &str) -> anyhow::Result<()> {
    if id.is_empty() {
        eprintln!("usage: mamba solve <problem-id> <answer>");
        std::process::exit(2);
    }
    let dir = storage::default_problems_path()?;
    let problems = storage::problem::load_all(&dir)
        .with_context(|| format!("loading problem bank from {}", dir.display()))?;

    let Some(problem) = problems.iter().find(|p| p.id == id) else {
        eprintln!("no problem with id '{id}' ({} in the bank)", problems.len());
        std::process::exit(2);
    };

    match engine::verify(problem, answer) {
        Verdict::Correct => {
            println!("✔ Correct — {} [{}]", problem.title, problem.domain);
            Ok(())
        }
        Verdict::Incorrect { detail } => {
            println!("✘ Incorrect: {detail}");
            std::process::exit(1);
        }
        Verdict::ParseError { message, hint } => {
            println!("⚠ {message}");
            if let Some(h) = hint {
                println!("  hint: {h}");
            }
            std::process::exit(1);
        }
    }
}

/// Lists the problems due for review (most overdue first), reading the vault
/// history and applying the same Leitner schedule as the TUI's `r` mode.
fn review_due() -> anyhow::Result<()> {
    let dir = storage::default_problems_path()?;
    let problems = storage::problem::load_all(&dir)
        .with_context(|| format!("loading problem bank from {}", dir.display()))?;
    let vault = storage::default_vault_path()?;
    let today = chrono::Local::now().date_naive();

    let mut due: Vec<_> = problems
        .iter()
        .filter_map(|p| {
            let history = storage::vault::history(&vault, p);
            review::is_due(history.as_ref(), today).then_some((p, history))
        })
        .collect();
    // Most overdue first: never-solved before solved, then oldest last-solve.
    due.sort_by(|(pa, ha), (pb, hb)| {
        let last =
            |h: &Option<storage::vault::ProblemHistory>| h.as_ref().and_then(|x| x.last_solved);
        match (last(ha), last(hb)) {
            (None, None) => pa.title.cmp(&pb.title),
            (None, Some(_)) => std::cmp::Ordering::Less,
            (Some(_), None) => std::cmp::Ordering::Greater,
            (Some(a), Some(b)) => a.cmp(&b).then(pa.title.cmp(&pb.title)),
        }
    });

    if due.is_empty() {
        println!(
            "✓ Nothing due — all caught up! ({} problems)",
            problems.len()
        );
        return Ok(());
    }
    println!(
        "{} of {} problems due for review:\n",
        due.len(),
        problems.len()
    );
    for (p, history) in &due {
        let status = match history {
            Some(h) if h.last_solved.is_some() => {
                let plural = if h.solves == 1 { "" } else { "s" };
                format!(
                    "last {} ({} solve{plural})",
                    h.last_solved.unwrap(),
                    h.solves
                )
            }
            _ => "new".to_string(),
        };
        println!("  {:<34} {:<20} {status}", p.id, p.topic);
    }
    Ok(())
}

/// Prints the single adaptively-recommended problem to attempt next (the same
/// pick the in-TUI `n` key jumps to), reading the vault history and blending
/// spaced repetition with per-topic mastery. See [`mamba::recommend`].
fn recommend_next() -> anyhow::Result<()> {
    let dir = storage::default_problems_path()?;
    let problems = storage::problem::load_all(&dir)
        .with_context(|| format!("loading problem bank from {}", dir.display()))?;
    let vault = storage::default_vault_path()?;
    let today = chrono::Local::now().date_naive();

    match recommend::next(&problems, &vault, today) {
        Some(pick) => {
            let p = &problems[pick.index];
            println!("➡ Next up: {}\n", p.title);
            println!("  id     {}", p.id);
            println!(
                "  topic  {}  ·  {:?}  ·  {:?}",
                p.topic, p.domain, p.difficulty
            );
            println!("  why    {}", pick.reason);
            println!("\nSolve it: mamba solve {} \"<answer>\"", p.id);
        }
        None => println!("The problem bank is empty — nothing to recommend."),
    }
    Ok(())
}

/// Prints CLI usage, in-TUI key bindings, and the environment overrides.
fn print_help() {
    println!(
        "mamba {} — a terminal mathematics practice engine\n\n\
         USAGE:\n    \
         mamba [OPTIONS]\n    \
         mamba solve <problem-id> <answer>   Verify one answer (exit 0 iff correct)\n    \
         mamba review                        List the problems due for review\n    \
         mamba next                          Recommend the next problem to attempt\n\n\
         OPTIONS:\n    \
         -h, --help       Print this help and exit\n    \
         -V, --version    Print the version and exit\n    \
             --check      Load the problem bank and report (no TUI), then exit\n    \
             --verify [DIR]  Verify every problem's canonical answer, then exit\n\n\
         KEYS (inside the TUI):\n    \
         Tab / Shift-Tab  cycle focus between panes\n    \
         Enter            open the selected problem / submit your answer\n    \
         j / k            navigate the list and scroll panes\n    \
         Esc              return to the navigator\n    \
         Ctrl-Q           quit\n\n\
         ENVIRONMENT:\n    \
         MAMBA_PROBLEMS   override the problem-bank directory\n    \
         MAMBA_VAULT      override the export vault (default ~/OnyxVault/Mamba)\n    \
         MAMBA_THEME      venom (default) | midnight | slate",
        env!("CARGO_PKG_VERSION"),
    );
}

/// The immediate-mode event loop: draw the full UI from state, then apply at
/// most one input event. State mutation and rendering never interleave.
fn run(terminal: &mut ratatui::DefaultTerminal, app: &mut App) -> anyhow::Result<()> {
    while !app.should_quit {
        // Drain finished worker output (verification + streamed tutor tokens)
        // so it lands in state before we draw — the UI never blocks on either.
        app.poll_verify();
        app.poll_agent();
        app.poll_search();

        terminal.draw(|frame| ui::draw(frame, app))?;

        // Poll faster while a worker is in flight (verification, tutor stream, or
        // a semantic search) so output appears promptly; idle otherwise.
        let timeout = if app.is_verifying() || app.agent_streaming || app.search_pending() {
            POLL_BUSY
        } else {
            TICK_RATE
        };
        if event::poll(timeout)?
            && let Event::Key(key) = event::read()?
            // Windows terminals emit Release events too; act on Press only.
            && key.kind == KeyEventKind::Press
        {
            app.on_key(key);
        }
    }
    Ok(())
}
