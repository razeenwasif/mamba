# 🐍 Mamba Quick Guide

A one-page reference for keybindings, the CLI, and configuration.

## Keybindings (in the TUI)

### Anywhere
| Key | Action |
|-----|--------|
| `Ctrl-Q` / `Ctrl-C` | Quit |
| `Ctrl-A` | Toggle the AI tutor panel |
| `Ctrl-E` | Ask the tutor to explain the last wrong answer |
| `Ctrl-P` | Toggle the progress / stats overlay |
| `/` | Open the fuzzy problem finder |
| `t` | Cycle the topic filter (`all → topic → … → all`) |
| `r` | Toggle spaced-repetition review mode (only due problems) |
| `Tab` / `Shift-Tab` | Cycle focus between panes |
| `Esc` | Return to the navigator (or close a modal) |

*(`/`, `t`, `r` are literal characters while typing in the answer or tutor input.)*

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

### Fuzzy finder (`/`)
| Key | Action |
|-----|--------|
| *(type)* | Filter problems (title, topic, difficulty, domain) |
| `↑` / `↓` | Move the highlight |
| `Enter` | Open the highlighted problem |
| `Esc` | Cancel |

### Stats overlay (`Ctrl-P`)
| Key | Action |
|-----|--------|
| `Esc` / `q` / `Ctrl-P` | Close |

### Tutor panel (`Ctrl-A`)
| Key | Action |
|-----|--------|
| *(type)* | Compose a message |
| `Enter` | Send |
| `Esc` / `Tab` | Leave the input (panel stays open) |
| `Ctrl-A` | Hide the panel |

## Answer syntax cheatsheet

- **Symbolic**: `^` powers, explicit `*` (incl. between variables: `2*x*y`),
  functions `exp`, `ln`, `sin`, `cos`, `tanh`, … — any equivalent form passes
  (`(x-3)(x+2)` = `x^2-x-6`).
- **Tensor**: rows separated by `;`, elements by spaces/commas; exact fractions
  allowed (`3/5 -7/10; -1/5 2/5`). A scalar is just a number.
- **Exact**: a whole integer; factorizations as prime powers (`2^3 * 3^2 * 5`).

## CLI

```sh
mamba                          # launch the TUI
mamba --version | -V
mamba --help | -h
mamba --check                  # load the bank and report (no TUI)
mamba --verify [DIR]           # verify every problem's canonical answer
mamba solve <id> "<answer>"    # verify one answer (exit 0 iff correct)
mamba review                   # list the problems due for review
```

## Environment

| Variable | Default | Meaning |
|----------|---------|---------|
| `MAMBA_PROBLEMS` | `./problems` or the data dir | problem-bank directory |
| `MAMBA_VAULT` | `~/OnyxVault/Mamba` | Markdown export vault |
| `MAMBA_THEME` | `venom` | `venom` · `midnight` · `slate` |
| `MAMBA_OLLAMA_MODEL` | `gemma4:e2b-it-qat` | tutor model tag |
| `MAMBA_OLLAMA_URL` | `http://localhost:11434` | Ollama server URL |
