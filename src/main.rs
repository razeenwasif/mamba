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
use mamba::{storage, ui};

/// How long the event loop blocks waiting for input before redrawing.
///
/// 100 ms keeps the UI responsive to terminal resizes while leaving the
/// process effectively idle (no busy-loop) when the user is thinking.
const TICK_RATE: Duration = Duration::from_millis(100);

fn main() -> anyhow::Result<()> {
    // Lightweight CLI handling *before* we touch the terminal, so `mamba
    // --version` and `mamba --help` work from any directory and are scriptable
    // (a TUI binary that only ever launches an alternate screen is impossible
    // to smoke-test on PATH). No `clap` dependency for two flags.
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "-V" | "--version" => {
                println!("mamba {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "-h" | "--help" => {
                print_help();
                return Ok(());
            }
            // Non-interactive health check: resolve and load the problem bank,
            // then report — a scriptable way to confirm the installed binary
            // finds its problems from any directory.
            "--check" => {
                let dir = storage::default_problems_path()?;
                let problems = storage::problem::load_all(&dir)
                    .with_context(|| format!("loading problem bank from {}", dir.display()))?;
                println!("ok: {} problems loaded from {}", problems.len(), dir.display());
                return Ok(());
            }
            other => {
                eprintln!("mamba: unrecognized argument '{other}'\n");
                print_help();
                std::process::exit(2);
            }
        }
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

/// Prints CLI usage, in-TUI key bindings, and the environment overrides.
fn print_help() {
    println!(
        "mamba {} — a terminal mathematics practice engine\n\n\
         USAGE:\n    \
         mamba [OPTIONS]\n\n\
         OPTIONS:\n    \
         -h, --help       Print this help and exit\n    \
         -V, --version    Print the version and exit\n    \
             --check      Load the problem bank and report (no TUI), then exit\n\n\
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
        terminal.draw(|frame| ui::draw(frame, app))?;

        if event::poll(TICK_RATE)?
            && let Event::Key(key) = event::read()?
            // Windows terminals emit Release events too; act on Press only.
            && key.kind == KeyEventKind::Press
        {
            app.on_key(key);
        }
    }
    Ok(())
}
