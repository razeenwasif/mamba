//! # Mamba (library)
//!
//! The reusable core of the Mamba TUI: application state, the universal math
//! routing engine, the problem bank, the Markdown vault exporter, and
//! rendering.
//!
//! Like its sibling tool `~/Kata`, the crate is split into a library plus a
//! thin binary (`src/main.rs`) so the non-UI logic — above all the three
//! verification pipelines — is reachable from integration tests in `tests/`.
//!
//! ## Architecture
//!
//! The binary follows the same strict unidirectional flow as Kata:
//!
//! ```text
//! event (crossterm) ──▶ App::on_key (mutate state) ──▶ ui::draw (pure render)
//!                              │
//!                              └─▶ engine::verify ──┬─▶ symbolic  (meval probe)
//!                                  (Universal       ├─▶ tensor    (ndarray cmp)
//!                                   Router)         └─▶ exact     (num-bigint)
//!                                       │
//!                                       └─(Correct)─▶ storage::vault::export
//! ```
//!
//! * [`app`]     — the single source of truth for application state.
//! * [`engine`]  — the **Universal Router**: dispatches a user's answer string
//!                 to the pipeline matching the problem's [`storage::problem::Domain`].
//! * [`storage`] — problem-bank loading and the Markdown vault exporter.
//! * [`ui`]      — stateless rendering of [`app::App`] into a [`ratatui::Frame`].

pub mod agent;
pub mod app;
pub mod engine;
pub mod review;
pub mod storage;
pub mod ui;
