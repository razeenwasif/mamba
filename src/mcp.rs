//! A Model Context Protocol (MCP) server exposing Mamba's verification engine.
//!
//! `mamba mcp` speaks MCP over stdio — newline-delimited JSON-RPC 2.0 — so any
//! MCP client (Claude Code, an agent framework, …) can use Mamba's *real* math
//! engine as a tool: submit an answer and get a genuine verdict, not an LLM's
//! guess. The headline tool is `verify`; the bank is also browsable and
//! solvable. This is "Mamba as a math-verification backend".
//!
//! The protocol surface is tiny (`initialize`, `tools/list`, `tools/call`,
//! `ping`), so it's hand-rolled here rather than pulling in an async SDK —
//! keeping the binary synchronous, pure-Rust, and dependency-light. Register it
//! with, e.g., `claude mcp add mamba -- mamba mcp`.

use std::io::{BufRead, Write};

use chrono::Local;
use serde_json::{Value, json};

use crate::engine;
use crate::recommend;
use crate::storage::problem::{AnswerKind, Difficulty, Domain, Problem};
use crate::{curriculum, review, storage};

/// The MCP protocol version we default to when the client doesn't pin one.
const PROTOCOL_VERSION: &str = "2025-06-18";

/// Runs the stdio MCP server until the client closes the input stream.
pub fn serve() -> anyhow::Result<()> {
    // Load the bank once; tolerate its absence so `verify` (which needs no bank)
    // still works. The vault is read per-call for scheduling/recommendation.
    let bank = storage::default_problems_path()
        .and_then(|d| storage::problem::load_all(&d))
        .unwrap_or_default();
    let vault = storage::default_vault_path().ok();

    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let Ok(msg) = serde_json::from_str::<Value>(&line) else {
            continue; // ignore unparseable frames
        };
        // A message with no `id` is a notification — handled, never answered.
        let Some(id) = msg.get("id").cloned() else {
            continue;
        };
        let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
        let params = msg.get("params").cloned().unwrap_or_else(|| json!({}));

        let response = match dispatch(method, &params, &bank, vault.as_deref()) {
            Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
            Err((code, message)) => {
                json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
            }
        };
        writeln!(stdout, "{}", serde_json::to_string(&response)?)?;
        stdout.flush()?;
    }
    Ok(())
}

/// Routes one JSON-RPC method to its handler. `Ok` is the `result` payload;
/// `Err((code, message))` becomes a JSON-RPC error.
fn dispatch(
    method: &str,
    params: &Value,
    bank: &[Problem],
    vault: Option<&std::path::Path>,
) -> Result<Value, (i64, String)> {
    match method {
        "initialize" => {
            // Echo the client's protocol version when present (maximal compat).
            let version = params
                .get("protocolVersion")
                .and_then(Value::as_str)
                .unwrap_or(PROTOCOL_VERSION);
            Ok(json!({
                "protocolVersion": version,
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "mamba", "version": env!("CARGO_PKG_VERSION")},
            }))
        }
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({"tools": tool_specs()})),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            let args = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            Ok(call_tool(name, &args, bank, vault))
        }
        other => Err((-32601, format!("method not found: {other}"))),
    }
}

/// The advertised tools and their JSON-Schema inputs.
fn tool_specs() -> Value {
    json!([
        {
            "name": "verify",
            "description": "Verify a math answer against an expected one using Mamba's exact engine \
                (algebraic equivalence, matrix comparison, or big-integer/number-theory). Returns \
                a real verdict with an optional near-miss diagnosis.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "domain": {"type": "string", "enum": ["Symbolic", "Tensor", "Exact"],
                        "description": "Symbolic = algebra/calculus expressions; Tensor = scalars/matrices; Exact = integers/number theory."},
                    "expected": {"type": "string", "description": "The canonical correct answer, in Mamba syntax."},
                    "answer": {"type": "string", "description": "The answer to check."},
                    "answer_kind": {"type": "string", "enum": ["Scalar", "Factorization"],
                        "description": "Optional; 'Factorization' for prime-factorization answers (Exact domain). Default Scalar."},
                    "unit": {"type": "string", "description": "Optional physical unit of the answer (e.g. 'N', 'm/s^2') for a dimensional check."}
                },
                "required": ["domain", "expected", "answer"]
            }
        },
        {
            "name": "solve",
            "description": "Verify an answer against a problem in Mamba's bank, by id.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": {"type": "string", "description": "The problem id (see list_problems)."},
                    "answer": {"type": "string", "description": "The answer to check."}
                },
                "required": ["id", "answer"]
            }
        },
        {
            "name": "get_problem",
            "description": "Fetch one problem's statement (prompt, hint, recap) by id — without revealing the answer.",
            "inputSchema": {
                "type": "object",
                "properties": {"id": {"type": "string"}},
                "required": ["id"]
            }
        },
        {
            "name": "list_problems",
            "description": "List problems in the bank, optionally filtered by topic and/or difficulty.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "topic": {"type": "string"},
                    "difficulty": {"type": "string", "enum": ["Easy", "Medium", "Hard"]},
                    "limit": {"type": "integer", "description": "Max results (default 50)."}
                }
            }
        },
        {
            "name": "recommend_next",
            "description": "Recommend the single best problem to attempt next (spaced repetition + topic mastery + curriculum).",
            "inputSchema": {"type": "object", "properties": {}}
        },
        {
            "name": "due",
            "description": "List the problems due for review now (FSRS spaced repetition), most-overdue first.",
            "inputSchema": {
                "type": "object",
                "properties": {"limit": {"type": "integer", "description": "Max results (default 50)."}}
            }
        },
        {
            "name": "stats",
            "description": "Progress summary: total solved, day streaks, and per-domain / per-difficulty / per-topic counts.",
            "inputSchema": {"type": "object", "properties": {}}
        },
        {
            "name": "curriculum",
            "description": "The topic prerequisite path: every topic in dependency order with its solved/total mastery and whether it's unlocked.",
            "inputSchema": {"type": "object", "properties": {}}
        }
    ])
}

/// Dispatches a `tools/call`, returning the MCP tool-result shape
/// `{content: [...], isError?}`. Tool-level failures are returned as `isError`
/// results (not JSON-RPC errors), per the MCP convention.
fn call_tool(name: &str, args: &Value, bank: &[Problem], vault: Option<&std::path::Path>) -> Value {
    let result = match name {
        "verify" => tool_verify(args),
        "solve" => tool_solve(args, bank),
        "get_problem" => tool_get_problem(args, bank),
        "list_problems" => tool_list_problems(args, bank),
        "recommend_next" => tool_recommend(bank, vault),
        "due" => tool_due(args, bank, vault),
        "stats" => tool_stats(bank, vault),
        "curriculum" => tool_curriculum(bank, vault),
        other => Err(format!("unknown tool: {other}")),
    };
    match result {
        Ok(value) => text_result(
            &serde_json::to_string_pretty(&value).unwrap_or_default(),
            false,
        ),
        Err(message) => text_result(&format!("error: {message}"), true),
    }
}

/// Wraps a string in the MCP `{content, isError}` result envelope.
fn text_result(text: &str, is_error: bool) -> Value {
    json!({"content": [{"type": "text", "text": text}], "isError": is_error})
}

fn parse_domain(s: &str) -> Option<Domain> {
    match s {
        "Symbolic" => Some(Domain::Symbolic),
        "Tensor" => Some(Domain::Tensor),
        "Exact" => Some(Domain::Exact),
        _ => None,
    }
}

fn parse_difficulty(s: &str) -> Option<Difficulty> {
    match s {
        "Easy" => Some(Difficulty::Easy),
        "Medium" => Some(Difficulty::Medium),
        "Hard" => Some(Difficulty::Hard),
        _ => None,
    }
}

/// A [`Verdict`](engine::Verdict) as structured JSON for an agent to act on.
fn verdict_json(verdict: &engine::Verdict) -> Value {
    match verdict {
        engine::Verdict::Correct => json!({"correct": true}),
        engine::Verdict::Incorrect { detail, hint } => {
            json!({"correct": false, "result": "incorrect", "detail": detail, "hint": hint})
        }
        engine::Verdict::ParseError { message, hint } => {
            json!({"correct": false, "result": "parse_error", "detail": message, "hint": hint})
        }
    }
}

fn tool_verify(args: &Value) -> Result<Value, String> {
    let domain = args
        .get("domain")
        .and_then(Value::as_str)
        .and_then(parse_domain)
        .ok_or("`domain` must be one of Symbolic, Tensor, Exact")?;
    let expected = args
        .get("expected")
        .and_then(Value::as_str)
        .ok_or("`expected` is required")?;
    let answer = args
        .get("answer")
        .and_then(Value::as_str)
        .ok_or("`answer` is required")?;
    let answer_kind = match args.get("answer_kind").and_then(Value::as_str) {
        Some("Factorization") => AnswerKind::Factorization,
        _ => AnswerKind::Scalar,
    };
    let unit = args.get("unit").and_then(Value::as_str).map(str::to_string);

    let problem = Problem {
        id: "mcp".into(),
        title: "mcp".into(),
        domain,
        topic: String::new(),
        difficulty: Difficulty::Easy,
        prompt: String::new(),
        expected: expected.to_string(),
        answer_kind,
        hint: None,
        recap: None,
        unit,
    };
    Ok(verdict_json(&engine::verify(&problem, answer)))
}

fn find<'a>(bank: &'a [Problem], id: &str) -> Result<&'a Problem, String> {
    bank.iter()
        .find(|p| p.id == id)
        .ok_or_else(|| format!("no problem with id '{id}' ({} in the bank)", bank.len()))
}

fn tool_solve(args: &Value, bank: &[Problem]) -> Result<Value, String> {
    let id = args
        .get("id")
        .and_then(Value::as_str)
        .ok_or("`id` is required")?;
    let answer = args
        .get("answer")
        .and_then(Value::as_str)
        .ok_or("`answer` is required")?;
    let problem = find(bank, id)?;
    let mut out = verdict_json(&engine::verify(problem, answer));
    out["id"] = json!(problem.id);
    out["title"] = json!(problem.title);
    Ok(out)
}

fn tool_get_problem(args: &Value, bank: &[Problem]) -> Result<Value, String> {
    let id = args
        .get("id")
        .and_then(Value::as_str)
        .ok_or("`id` is required")?;
    let p = find(bank, id)?;
    // Deliberately omit `expected` so the problem stays a real exercise.
    Ok(json!({
        "id": p.id,
        "title": p.title,
        "domain": p.domain.to_string(),
        "topic": p.topic,
        "difficulty": p.difficulty.to_string(),
        "answer_kind": format!("{:?}", p.answer_kind),
        "prompt": p.prompt,
        "hint": p.hint,
        "recap": p.recap,
        "unit": p.unit,
    }))
}

fn tool_list_problems(args: &Value, bank: &[Problem]) -> Result<Value, String> {
    let topic = args.get("topic").and_then(Value::as_str);
    let difficulty = args
        .get("difficulty")
        .and_then(Value::as_str)
        .and_then(parse_difficulty);
    let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(50) as usize;

    let items: Vec<Value> = bank
        .iter()
        .filter(|p| topic.is_none_or(|t| p.topic == t))
        .filter(|p| difficulty.is_none_or(|d| p.difficulty == d))
        .take(limit)
        .map(|p| {
            json!({
                "id": p.id,
                "title": p.title,
                "topic": p.topic,
                "domain": p.domain.to_string(),
                "difficulty": p.difficulty.to_string(),
            })
        })
        .collect();
    Ok(json!({"count": items.len(), "problems": items}))
}

fn tool_recommend(bank: &[Problem], vault: Option<&std::path::Path>) -> Result<Value, String> {
    let vault = vault.ok_or("no vault configured")?;
    let today = Local::now().date_naive();
    let pick = recommend::next(bank, vault, today).ok_or("the bank is empty")?;
    let p = &bank[pick.index];
    // Whether the recommendation is currently due (vs. getting ahead).
    let due = review::is_due(crate::storage::vault::history(vault, p).as_ref(), today);
    Ok(json!({
        "id": p.id,
        "title": p.title,
        "topic": p.topic,
        "domain": p.domain.to_string(),
        "difficulty": p.difficulty.to_string(),
        "reason": pick.reason,
        "due": due,
    }))
}

fn tool_due(
    args: &Value,
    bank: &[Problem],
    vault: Option<&std::path::Path>,
) -> Result<Value, String> {
    let vault = vault.ok_or("no vault configured")?;
    let today = Local::now().date_naive();
    let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(50) as usize;

    let mut due: Vec<(&Problem, i64)> = bank
        .iter()
        .filter_map(|p| {
            let history = storage::vault::history(vault, p);
            review::is_due(history.as_ref(), today)
                .then(|| (p, review::overdue_days(history.as_ref(), today)))
        })
        .collect();
    // Most overdue first; ties by title for a stable order.
    due.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.title.cmp(&b.0.title)));

    let items: Vec<Value> = due
        .iter()
        .take(limit)
        .map(|(p, overdue)| {
            json!({
                "id": p.id,
                "title": p.title,
                "topic": p.topic,
                "domain": p.domain.to_string(),
                "difficulty": p.difficulty.to_string(),
                "overdue_days": overdue,
            })
        })
        .collect();
    Ok(json!({"count": items.len(), "due": items}))
}

fn tool_stats(_bank: &[Problem], vault: Option<&std::path::Path>) -> Result<Value, String> {
    let vault = vault.ok_or("no vault configured")?;
    let s = storage::progress::stats(vault, Local::now().date_naive());
    let pairs = |v: &[(&str, usize)]| -> Vec<Value> {
        v.iter()
            .map(|(k, n)| json!({"label": k, "solved": n}))
            .collect()
    };
    Ok(json!({
        "total_solved": s.total,
        "current_streak": s.current_streak,
        "longest_streak": s.longest_streak,
        "by_domain": pairs(&s.by_domain),
        "by_difficulty": pairs(&s.by_difficulty),
        "by_topic": s.by_topic.iter().map(|(t, n)| json!({"topic": t, "solved": n})).collect::<Vec<_>>(),
    }))
}

fn tool_curriculum(bank: &[Problem], vault: Option<&std::path::Path>) -> Result<Value, String> {
    use std::collections::BTreeMap;
    let vault = vault.ok_or("no vault configured")?;

    let mut total: BTreeMap<String, usize> = BTreeMap::new();
    let mut solved: BTreeMap<String, usize> = BTreeMap::new();
    for p in bank {
        *total.entry(p.topic.clone()).or_default() += 1;
        if storage::vault::history(vault, p).is_some_and(|h| h.solves >= 1) {
            *solved.entry(p.topic.clone()).or_default() += 1;
        }
    }
    let met = |topic: &str| -> bool {
        match (
            solved.get(topic).copied().unwrap_or(0),
            total.get(topic).copied().unwrap_or(0),
        ) {
            (_, 0) => true,
            (s, t) => s as f64 / t as f64 >= curriculum::MET_THRESHOLD,
        }
    };

    let topics: Vec<String> = total.keys().cloned().collect();
    let items: Vec<Value> = curriculum::topo_order(&topics)
        .into_iter()
        .map(|t| {
            let missing = curriculum::missing_prerequisites(&t, met);
            json!({
                "topic": t,
                "solved": solved.get(&t).copied().unwrap_or(0),
                "total": total[&t],
                "unlocked": missing.is_empty(),
                "missing_prerequisites": missing,
            })
        })
        .collect();
    Ok(json!({"topics": items}))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn problem(id: &str, domain: Domain, topic: &str, expected: &str) -> Problem {
        Problem {
            id: id.into(),
            title: id.into(),
            domain,
            topic: topic.into(),
            difficulty: Difficulty::Easy,
            prompt: "p".into(),
            expected: expected.into(),
            answer_kind: AnswerKind::Scalar,
            hint: None,
            recap: None,
            unit: None,
        }
    }

    #[test]
    fn initialize_echoes_protocol_and_advertises_tools() {
        let init = dispatch(
            "initialize",
            &json!({"protocolVersion": "2024-11-05"}),
            &[],
            None,
        )
        .unwrap();
        assert_eq!(init["protocolVersion"], "2024-11-05");
        assert_eq!(init["serverInfo"]["name"], "mamba");
        let tools = dispatch("tools/list", &json!({}), &[], None).unwrap();
        let names: Vec<&str> = tools["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"verify"));
        assert!(names.contains(&"solve"));
    }

    #[test]
    fn unknown_method_is_a_jsonrpc_error() {
        assert_eq!(
            dispatch("frobnicate", &json!({}), &[], None).unwrap_err().0,
            -32601
        );
    }

    #[test]
    fn verify_tool_checks_equivalence_and_reports_near_misses() {
        let call = |args: Value| {
            let r = call_tool("verify", &args, &[], None);
            r["content"][0]["text"].as_str().unwrap().to_string()
        };
        // Algebraically equivalent → correct.
        let ok =
            call(json!({"domain": "Symbolic", "expected": "x^2 - x - 6", "answer": "(x-3)(x+2)"}));
        assert!(ok.contains("\"correct\": true"), "{ok}");
        // A sign error → incorrect, with a near-miss hint.
        let wrong = call(json!({"domain": "Exact", "expected": "12", "answer": "-12"}));
        assert!(wrong.contains("\"correct\": false"));
        assert!(wrong.contains("sign"), "{wrong}");
    }

    #[test]
    fn verify_tool_enforces_units() {
        let r = call_tool(
            "verify",
            &json!({"domain": "Exact", "expected": "30", "answer": "30 kg", "unit": "N"}),
            &[],
            None,
        );
        let text = r["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("wrong units"), "{text}");
    }

    #[test]
    fn solve_and_list_use_the_bank() {
        let bank = vec![
            problem("a", Domain::Symbolic, "Algebra", "x^2 - x - 6"),
            problem("b", Domain::Exact, "Number Theory", "7"),
        ];
        let solved = call_tool(
            "solve",
            &json!({"id": "a", "answer": "(x-3)(x+2)"}),
            &bank,
            None,
        );
        assert!(
            solved["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("\"correct\": true")
        );

        let listed = call_tool("list_problems", &json!({"topic": "Algebra"}), &bank, None);
        let text = listed["content"][0]["text"].as_str().unwrap();
        assert!(
            text.contains("\"count\": 1") && text.contains("\"a\""),
            "{text}"
        );

        // An unknown id is a tool error, not a crash.
        let missing = call_tool("solve", &json!({"id": "zzz", "answer": "1"}), &bank, None);
        assert_eq!(missing["isError"], true);
    }

    #[test]
    fn get_problem_hides_the_expected_answer() {
        let bank = vec![problem("a", Domain::Symbolic, "Algebra", "x^2 - x - 6")];
        let got = call_tool("get_problem", &json!({"id": "a"}), &bank, None);
        let text = got["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("\"prompt\""));
        assert!(
            !text.contains("x^2 - x - 6"),
            "must not leak the answer: {text}"
        );
    }

    #[test]
    fn coaching_tools_are_listed_and_need_a_vault() {
        let names: Vec<String> = tool_specs()
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_string())
            .collect();
        for tool in ["due", "stats", "curriculum"] {
            assert!(names.contains(&tool.to_string()), "missing tool {tool}");
            // Without a vault they fail gracefully (isError), never panic.
            assert_eq!(call_tool(tool, &json!({}), &[], None)["isError"], true);
        }
    }

    #[test]
    fn curriculum_tool_reports_topo_order_and_locks() {
        let dir = std::env::temp_dir().join(format!("mamba-mcp-cur-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let bank = vec![
            problem("a", Domain::Symbolic, "Algebra", "x"),
            problem("c", Domain::Symbolic, "Calculus", "x"),
        ];
        let out = call_tool("curriculum", &json!({}), &bank, Some(&dir));
        let v: Value = serde_json::from_str(out["content"][0]["text"].as_str().unwrap()).unwrap();
        let topics = v["topics"].as_array().unwrap();
        // Algebra (a root) precedes Calculus, and is unlocked; Calculus is locked.
        assert_eq!(topics[0]["topic"], "Algebra");
        assert_eq!(topics[0]["unlocked"], true);
        let calc = topics.iter().find(|t| t["topic"] == "Calculus").unwrap();
        assert_eq!(calc["unlocked"], false);
        assert_eq!(calc["missing_prerequisites"][0], "Algebra");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
