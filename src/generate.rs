//! Verified problem generation: the tutor authors a new problem, and Mamba's
//! own engine cross-checks it before you ever see it.
//!
//! This is the one move neither an LLM-only nor an engine-only tool can make.
//! The pipeline (all on a worker thread, results over an [`mpsc`] channel):
//!
//! 1. **Author** — the local model is asked for a problem *and* its answer, as
//!    JSON, in the same vein as a reference problem.
//! 2. **Parse gate** — the answer is run through [`engine::verify`] against
//!    itself; if it isn't valid Mamba syntax for the stated domain it's
//!    rejected and re-asked (up to a few attempts).
//! 3. **Cross-check** — the model is asked, *independently and without seeing
//!    its own answer*, to solve the problem. That solution is checked against
//!    the stated answer **through the engine** (algebraic equivalence, not
//!    string match). Agreement is real evidence the math is right; disagreement
//!    flags the problem as unverified practice.
//!
//! So the engine guarantees the problem is well-formed and answerable, and the
//! cross-check turns two independent model derivations into a verdict — keeping
//! [ADR 0001]'s "verified, not LLM-graded" line intact for anything we bless.

use std::sync::mpsc::{self, Receiver};
use std::thread;

use serde_json::Value;

use crate::engine;
use crate::storage::problem::{AnswerKind, Difficulty, Domain, Problem};

/// What to generate, distilled from the currently open problem.
#[derive(Debug, Clone)]
pub struct Spec {
    pub topic: String,
    pub domain: Domain,
    pub difficulty: Difficulty,
    pub reference_prompt: String,
    pub reference_expected: String,
}

/// Pulls a JSON field as a string, coercing numbers and bools — models often
/// emit `"expected": 42` (a number) or omit the quotes, which strict
/// `String` deserialization would reject.
fn get_str(v: &Value, key: &str) -> Option<String> {
    match v.get(key) {
        Some(Value::String(s)) => Some(s.clone()),
        Some(Value::Number(n)) => Some(n.to_string()),
        Some(Value::Bool(b)) => Some(b.to_string()),
        _ => None,
    }
}

/// Progress and the final result of a generation run.
#[derive(Debug)]
pub enum GenEvent {
    /// A human-readable progress line for the console.
    Status(String),
    /// A finished, well-formed problem. `verified` is true iff an independent
    /// self-solve agreed with the stated answer *through the engine*.
    Done {
        problem: Box<Problem>,
        verified: bool,
        note: String,
    },
    /// Generation failed (server down, no valid JSON after retries, …).
    Error(String),
}

/// Mamba's answer syntax, embedded in the author/solve prompts so the model
/// emits answers the engine can actually parse.
const SYNTAX_GUIDE: &str = "\
Mamba answer syntax by domain:
- Symbolic (algebra/calculus): a single expression. Use ^ for powers, an explicit * for \
every product (including between variables, e.g. 2*x*y), and functions exp, ln, sin, cos, \
tan, tanh. Any algebraically equivalent form is accepted.
- Tensor (linear algebra): a scalar number, or a matrix with rows separated by ';' and \
entries by spaces (e.g. '1 2; 3 4'). Exact fractions like 3/5 are allowed. A complex number \
is written as 'real imag' (e.g. '0 -1' for -i).
- Exact (number theory): a single integer, or a prime factorization like '2^3 * 3^2 * 5'.";

/// Calls Ollama's chat endpoint once in JSON mode and returns the reply's
/// `message.content` (a JSON document as a string).
fn chat_json(url: &str, model: &str, system: &str, user: &str) -> anyhow::Result<String> {
    let endpoint = format!("{}/api/chat", url.trim_end_matches('/'));
    let body = serde_json::json!({
        "model": model,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user},
        ],
        "stream": false,
        // Force a JSON object so we don't have to scrape prose.
        "format": "json",
        "options": {"temperature": 0.7},
    });
    let resp: Value = ureq::post(&endpoint)
        .send_json(body)
        .map_err(|e| {
            anyhow::anyhow!(
                "Ollama request failed: {e}. Is `ollama serve` running and is the model pulled?"
            )
        })?
        .into_json()?;
    resp.get("message")
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("the model returned an empty reply"))
}

/// Strips ```json … ``` fences a model may add despite JSON mode.
fn strip_fences(s: &str) -> &str {
    let s = s.trim();
    let s = s
        .strip_prefix("```json")
        .or_else(|| s.strip_prefix("```"))
        .unwrap_or(s);
    s.strip_suffix("```").unwrap_or(s).trim()
}

fn parse_domain(s: &str) -> Option<Domain> {
    match s.trim().to_lowercase().as_str() {
        "symbolic" => Some(Domain::Symbolic),
        "tensor" => Some(Domain::Tensor),
        "exact" => Some(Domain::Exact),
        _ => None,
    }
}

fn parse_difficulty(s: &str) -> Option<Difficulty> {
    match s.trim().to_lowercase().as_str() {
        "easy" => Some(Difficulty::Easy),
        "medium" => Some(Difficulty::Medium),
        "hard" => Some(Difficulty::Hard),
        _ => None,
    }
}

fn parse_answer_kind(s: &str) -> AnswerKind {
    match s.trim().to_lowercase().as_str() {
        "factorization" => AnswerKind::Factorization,
        _ => AnswerKind::Scalar,
    }
}

/// A filesystem/id-safe slug of a title.
fn slug(title: &str) -> String {
    let s: String = title
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    s.trim_matches('-')
        .split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

/// Builds a [`Problem`] from the model's JSON object, applying the spec as a
/// fallback for any mislabeled or omitted field so it can't derail
/// verification. Returns `None` if the essential `prompt`/`expected` are
/// missing (a retry-worthy reply).
fn build_problem(v: &Value, spec: &Spec) -> Option<Problem> {
    let prompt = get_str(v, "prompt")?;
    let expected = get_str(v, "expected")?;
    if prompt.trim().is_empty() || expected.trim().is_empty() {
        return None;
    }
    let title = get_str(v, "title").unwrap_or_else(|| "Generated Problem".into());
    let domain = get_str(v, "domain")
        .as_deref()
        .and_then(parse_domain)
        .unwrap_or(spec.domain);
    let difficulty = get_str(v, "difficulty")
        .as_deref()
        .and_then(parse_difficulty)
        .unwrap_or(spec.difficulty);
    Some(Problem {
        id: format!("gen-ai-{}", slug(&title)),
        title,
        domain,
        topic: get_str(v, "topic").unwrap_or_else(|| spec.topic.clone()),
        difficulty,
        prompt,
        expected: expected.trim().to_string(),
        answer_kind: get_str(v, "answer_kind")
            .as_deref()
            .map(parse_answer_kind)
            .unwrap_or_default(),
        hint: get_str(v, "hint"),
        recap: None,
    })
}

/// Authors a fresh problem in the vein of `spec`, cross-verifies it against an
/// independent self-solve, and reports over the returned channel.
pub fn generate(url: String, model: String, spec: Spec) -> Receiver<GenEvent> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let author_system = format!(
            "You are a careful math problem author for Mamba, a terminal practice tool whose engine \
             verifies answers exactly. Reply with ONLY a JSON object with the fields: title, prompt, \
             domain (\"Symbolic\" | \"Tensor\" | \"Exact\"), topic, difficulty (\"Easy\" | \"Medium\" \
             | \"Hard\"), expected, hint. The 'expected' field MUST be the correct answer written \
             exactly in Mamba syntax.\n\n{SYNTAX_GUIDE}"
        );
        let author_user = format!(
            "Author ONE new problem similar in type and difficulty to this reference, but with \
             different numbers or wording.\n\nReference [{} · {} · {}]:\n{}\n\nReference answer: {}",
            spec.domain,
            spec.topic,
            spec.difficulty,
            spec.reference_prompt,
            spec.reference_expected,
        );

        // --- Author + parse gate (a few attempts) --------------------------
        let _ = tx.send(GenEvent::Status("authoring a new problem…".into()));
        let mut problem: Option<Problem> = None;
        for attempt in 0..4 {
            let content = match chat_json(&url, &model, &author_system, &author_user) {
                Ok(c) => c,
                Err(e) => {
                    let _ = tx.send(GenEvent::Error(e.to_string()));
                    return;
                }
            };
            let Some(candidate) = serde_json::from_str::<Value>(strip_fences(&content))
                .ok()
                .as_ref()
                .and_then(|v| build_problem(v, &spec))
            else {
                let _ = tx.send(GenEvent::Status(format!(
                    "retrying — incomplete reply from the model (attempt {})",
                    attempt + 1
                )));
                continue;
            };
            // Parse gate: the answer must be valid Mamba syntax for its domain,
            // else the user could never be marked correct against it.
            if engine::verify(&candidate, &candidate.expected).is_correct() {
                problem = Some(candidate);
                break;
            }
            let _ = tx.send(GenEvent::Status(format!(
                "retrying — the answer didn't parse for the {} engine (attempt {})",
                candidate.domain,
                attempt + 1
            )));
        }
        let Some(problem) = problem else {
            let _ = tx.send(GenEvent::Error(
                "couldn't get a well-formed problem from the model after 3 tries".into(),
            ));
            return;
        };

        // --- Cross-check: independent self-solve through the engine ---------
        let _ = tx.send(GenEvent::Status(
            "cross-checking by solving it independently…".into(),
        ));
        let solve_system = format!(
            "You are solving a math problem for Mamba. Reply with ONLY a JSON object {{\"answer\": \
             \"<your answer in Mamba syntax>\"}}.\n\n{SYNTAX_GUIDE}"
        );
        let solve_user = format!(
            "Solve this problem ({} domain):\n\n{}",
            problem.domain, problem.prompt
        );
        let verified = match chat_json(&url, &model, &solve_system, &solve_user) {
            Ok(content) => serde_json::from_str::<Value>(strip_fences(&content))
                .ok()
                .and_then(|v| get_str(&v, "answer"))
                .map(|ans| engine::verify(&problem, ans.trim()).is_correct())
                .unwrap_or(false),
            Err(_) => false,
        };

        let note = if verified {
            "✓ cross-verified — an independent solve matched through the engine".into()
        } else {
            "⚠ unverified — couldn't reproduce the answer independently; treat as practice".into()
        };
        let _ = tx.send(GenEvent::Done {
            problem: Box::new(problem),
            verified,
            note,
        });
    });
    rx
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hits a *live* Ollama server, so it's ignored by default. Run with:
    /// `cargo test -- --ignored generates_and_cross_verifies`
    /// (a capable model helps — `MAMBA_OLLAMA_MODEL=gemma4:12b-it-qat`).
    #[test]
    #[ignore = "requires a running ollama server with the chat model pulled"]
    fn generates_and_cross_verifies_live() {
        use std::time::Duration;

        let spec = Spec {
            topic: "Algebra".into(),
            domain: Domain::Symbolic,
            difficulty: Difficulty::Easy,
            reference_prompt: "Expand into standard form: (x + 2)(x - 5)".into(),
            reference_expected: "x^2 - 3*x - 10".into(),
        };
        let rx = generate(
            crate::agent::default_url(),
            crate::agent::default_model(),
            spec,
        );
        let mut done = None;
        while let Ok(ev) = rx.recv_timeout(Duration::from_secs(180)) {
            match ev {
                GenEvent::Status(s) => eprintln!("status: {s}"),
                GenEvent::Done {
                    problem,
                    verified,
                    note,
                } => {
                    eprintln!(
                        "GENERATED [{} · {}]: {}\n  expected: {}\n  {note}",
                        problem.domain, problem.difficulty, problem.prompt, problem.expected
                    );
                    done = Some((problem, verified));
                    break;
                }
                GenEvent::Error(e) => panic!("generation error: {e}"),
            }
        }
        let (problem, _verified) = done.expect("a finished problem");
        // The parse gate guarantees this much regardless of the model: the
        // canonical answer is valid Mamba syntax for its domain.
        assert!(
            engine::verify(&problem, &problem.expected).is_correct(),
            "the generated answer must pass its own parse gate"
        );
    }

    #[test]
    fn strip_fences_unwraps_code_blocks() {
        assert_eq!(strip_fences("```json\n{\"a\":1}\n```"), "{\"a\":1}");
        assert_eq!(strip_fences("```\n{\"a\":1}\n```"), "{\"a\":1}");
        assert_eq!(strip_fences("{\"a\":1}"), "{\"a\":1}");
    }

    #[test]
    fn slug_is_id_safe() {
        assert_eq!(slug("Expand a Binomial!"), "expand-a-binomial");
        assert_eq!(slug("  Ohm's Law  "), "ohm-s-law");
    }

    #[test]
    fn parsers_map_strings_to_enums_with_fallbacks() {
        assert_eq!(parse_domain("tensor"), Some(Domain::Tensor));
        assert_eq!(parse_domain("nonsense"), None);
        assert_eq!(parse_difficulty("HARD"), Some(Difficulty::Hard));
        assert_eq!(
            parse_answer_kind("Factorization"),
            AnswerKind::Factorization
        );
        assert_eq!(parse_answer_kind("whatever"), AnswerKind::Scalar);
    }

    #[test]
    fn build_problem_falls_back_to_the_spec_and_coerces_numbers() {
        let spec = Spec {
            topic: "Algebra".into(),
            domain: Domain::Symbolic,
            difficulty: Difficulty::Medium,
            reference_prompt: "p".into(),
            reference_expected: "x".into(),
        };
        // 'domain' is a model-ism ("Algebra", not a Mamba domain) and 'expected'
        // is a bare number — both must be tolerated.
        let v: Value = serde_json::from_str(
            r#"{"title":"Test","prompt":"do it","domain":"Algebra","expected":42}"#,
        )
        .unwrap();
        let p = build_problem(&v, &spec).expect("a problem");
        assert_eq!(p.domain, Domain::Symbolic); // fell back to the spec
        assert_eq!(p.topic, "Algebra");
        assert_eq!(p.difficulty, Difficulty::Medium);
        assert_eq!(p.expected, "42"); // number coerced to a string
        assert_eq!(p.id, "gen-ai-test");
    }

    #[test]
    fn build_problem_rejects_a_reply_missing_the_answer() {
        let spec = Spec {
            topic: "Algebra".into(),
            domain: Domain::Symbolic,
            difficulty: Difficulty::Easy,
            reference_prompt: "p".into(),
            reference_expected: "x".into(),
        };
        let v: Value = serde_json::from_str(r#"{"prompt":"do it"}"#).unwrap();
        assert!(build_problem(&v, &spec).is_none());
    }
}
