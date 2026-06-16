# 🐍 Mamba Quick Guide

A one-page reference for keybindings, the CLI, and configuration.

## Keybindings (in the TUI)

### Anywhere
| Key | Action |
|-----|--------|
| `Ctrl-Q` / `Ctrl-C` | Quit |
| `Ctrl-A` | Toggle the AI tutor panel |
| `Ctrl-E` | Ask the tutor to explain the last wrong answer |
| `Ctrl-G` | Generate a fresh, similar problem and engine-cross-verify it |
| `Ctrl-S` | Save the active generated problem into the bank |
| `Ctrl-P` | Toggle the progress / stats overlay |
| `/` | Open the fuzzy problem finder |
| `t` | Cycle the topic filter (`all → topic → … → all`) |
| `n` | Jump to the adaptively-recommended next problem (due + weakest topic) |
| `r` | Toggle spaced-repetition review mode (only due problems) |
| `s` | Start/stop a study session over the due queue (progress HUD + summary) |
| `g` | Toggle the recap (preliminary reading) in the problem pane |
| `Tab` / `Shift-Tab` | Cycle focus between panes |
| `Esc` | Return to the navigator (or close a modal) |

*(`/`, `t`, `r`, `g` are literal characters while typing in the answer or tutor input.)*

### Navigator (problem list)
| Key | Action |
|-----|--------|
| `j` / `k` or `↓` / `↑` | Move selection (opens the problem as you browse) |
| `Enter` | Open the selected problem and jump to the answer bar |

### Answer bar
| Key | Action |
|-----|--------|
| *(type)* | Enter your answer |
| `Enter` | Verify the answer |
| `?…` then `Enter` | Send the rest to the tutor as a question (bare `?` = ask for a hint) |
| `Esc` | Return to the navigator |

### Problem / Verification panes
| Key | Action |
|-----|--------|
| `j` / `k` or `↓` / `↑` | Scroll the focused pane |

### Finder (`/`) — fuzzy or semantic
| Key | Action |
|-----|--------|
| *(type)* | Filter problems (title, topic, difficulty, domain) |
| `Tab` | Toggle fuzzy (by characters) ↔ semantic (by meaning) |
| `↑` / `↓` | Move the highlight |
| `Enter` | Open the highlight — in semantic mode, the first `Enter` runs the search |
| `Esc` | Cancel |

Semantic mode ranks problems by *meaning* using local `nomic-embed-text`
embeddings (so "rate of change of a curve" finds the derivative problems). The
bank is embedded once on first use and cached; switching the embedding model or
editing a problem re-indexes automatically.

### Stats overlay (`Ctrl-P`)
| Key | Action |
|-----|--------|
| `Esc` / `q` / `Ctrl-P` | Close |

### Tutor panel (`Ctrl-A`)
| Key | Action |
|-----|--------|
| *(type)* | Compose a message |
| `Enter` | Send |
| `Ctrl-E` | Explain the last wrong answer |
| `Esc` / `Tab` | Leave the input (panel stays open) |
| `Ctrl-A` | Hide the panel |

### Verified problem generation (`Ctrl-G`)
`Ctrl-G` asks the local model to **author** a new problem like the open one, then
Mamba's own engine **cross-verifies** it: the answer must parse for its domain,
and an *independent* self-solve must match it through the engine (algebraic
equivalence, not string match). The result opens for you to attempt, badged
`✓ cross-verified` or `⚠ unverified` (the model's two attempts disagreed —
practice it, but don't trust the answer). `Ctrl-S` saves it into the bank;
`Ctrl-G` again regenerates.

## Answer syntax cheatsheet

- **Symbolic**: `^` powers, explicit `*` (incl. between variables: `2*x*y`),
  functions `exp`, `ln`, `sin`, `cos`, `tanh`, … — any equivalent form passes
  (`(x-3)(x+2)` = `x^2-x-6`).
- **Tensor**: rows separated by `;`, elements by spaces/commas; exact fractions
  allowed (`3/5 -7/10; -1/5 2/5`). A scalar is just a number.
- **Exact**: a whole integer; factorizations as prime powers (`2^3 * 3^2 * 5`).
- **Units** (physics/engineering): optional. Append one and it's dimensionally
  checked — `30 N` or `30 kg*m/s^2` pass, `30 kg` is rejected. Omit it and only
  the number is checked. A wrong answer also shows a `💡` near-miss hint (off by
  a sign, a factor, the transpose, …).

## CLI

```sh
mamba                          # launch the TUI
mamba --version | -V
mamba --help | -h
mamba --check                  # load the bank and report (no TUI)
mamba --verify [DIR]           # verify every problem's canonical answer
mamba solve <id> "<answer>"    # verify one answer (exit 0 iff correct)
mamba review                   # list the problems due for review
mamba next                     # recommend the next problem to attempt
mamba stats                    # print the progress dashboard
mamba path                     # print the curriculum (topic prerequisites)
mamba mcp                      # run an MCP server (verification engine as a tool)
```

## Environment

| Variable | Default | Meaning |
|----------|---------|---------|
| `MAMBA_PROBLEMS` | `./problems` or the data dir | problem-bank directory |
| `MAMBA_VAULT` | `~/OnyxVault/Mamba` | Markdown export vault |
| `MAMBA_THEME` | `venom` | `venom` · `midnight` · `slate` |
| `MAMBA_OLLAMA_MODEL` | `gemma4:e2b-it-qat` | tutor model tag |
| `MAMBA_OLLAMA_URL` | `http://localhost:11434` | Ollama server URL |
| `MAMBA_EMBED_MODEL` | `nomic-embed-text` | semantic-search embedding model |

Set any of these once in `~/.config/mamba/config.toml` (keys `vault`,
`problems`, `theme`, `ollama_model`, `ollama_url`, `embed_model`) instead of
exporting them. Precedence: **env var > config file > default**.
