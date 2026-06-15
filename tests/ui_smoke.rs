//! Render smoke test: the real UI must paint the main frame (with topic-grouped
//! navigator) and the stats overlay to a `TestBackend` without panicking, and
//! the buffers must contain the expected landmarks.

use std::path::Path;
use std::time::Duration;

use chrono::NaiveDate;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;

use mamba::app::App;
use mamba::storage::{problem, progress, vault};
use mamba::ui;

fn text(buf: &Buffer) -> String {
    let area = buf.area;
    let mut s = String::new();
    for y in 0..area.height {
        for x in 0..area.width {
            s.push_str(buf[(x, y)].symbol());
        }
    }
    s
}

#[test]
fn renders_navigator_and_stats_overlay() {
    let problems = problem::load_all(Path::new("problems")).expect("bank loads");
    let vault = std::env::temp_dir().join("mamba-ui-smoke");
    let _ = std::fs::remove_dir_all(&vault);

    // Seed a few solves so the overlay has data (before `problems` is moved).
    let day = NaiveDate::from_ymd_opt(2026, 6, 15).unwrap();
    for p in problems.iter().take(3) {
        vault::export(&vault, p, "x", 1, Duration::from_secs(60), day, None).unwrap();
    }

    let mut app = App::new(problems, vault.clone());
    let mut terminal = Terminal::new(TestBackend::new(120, 44)).unwrap();

    // Main frame: a domain header and a topic sub-header both render.
    terminal.draw(|f| ui::draw(f, &mut app)).unwrap();
    let main = text(terminal.backend().buffer());
    assert!(main.contains("Symbolic"), "expected a domain header");
    assert!(
        main.contains("Machine Learning") || main.contains("Algebra"),
        "expected a topic sub-header"
    );
    assert!(
        main.contains("Recap"),
        "expected the recap (shown by default)"
    );

    // Stats overlay: open it with a snapshot and confirm the dashboard paints.
    app.stats = Some(progress::stats(&vault, day));
    app.stats_visible = true;
    terminal.draw(|f| ui::draw(f, &mut app)).unwrap();
    let overlay = text(terminal.backend().buffer());
    assert!(overlay.contains("Progress"), "expected the overlay title");
    assert!(overlay.contains("By domain"), "expected the domain section");
    assert!(
        overlay.contains("Topic mastery"),
        "expected the mastery section"
    );
    assert!(overlay.contains("solved"), "expected the totals line");

    // The AI tutor panel renders in its right-hand column when visible.
    app.stats_visible = false;
    app.agent_visible = true;
    terminal.draw(|f| ui::draw(f, &mut app)).unwrap();
    assert!(
        text(terminal.backend().buffer()).contains("Tutor"),
        "expected the tutor panel"
    );

    let _ = std::fs::remove_dir_all(&vault);
}
