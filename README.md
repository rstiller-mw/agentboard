# agentboard

**One list for all the AI coding agents you have running.**

If you keep a handful of Claude Code sessions open across terminals and workspaces, plus a few background jobs, you eventually lose track of which one is waiting for you, which one finished an hour ago, and which one is quietly burning money. agentboard is a small terminal UI that puts them all on one screen. Press Enter on a card and you land in the terminal that agent lives in, even if it is on another workspace.

```
  agentboard                                             1 working  ·  1 waiting  ·  1 idle  ·  1/3

  ╭──────────────────────────────────────────────────────────────────────────────────────────────╮
  │ ███  ●  Add retry handling to the payment client                                             │
  │ ███     working  ·  claude  ·  42m  ·  ~$2.31  ·  ~/projects/billing-api                     │
  ╰──────────────────────────────────────────────────────────────────────────────────────────────╯

  ╭──────────────────────────────────────────────────────────────────────────────────────────────╮
  │ ███  ◆  Fix flaky integration test                                                           │
  │ ███     waiting  ·  claude  ·  2h 5m  ·  ~$0.84  ·  ~/projects/search-service                │
  ╰──────────────────────────────────────────────────────────────────────────────────────────────╯

  ╭──────────────────────────────────────────────────────────────────────────────────────────────╮
  │ ▒▒▒  ○  Summarise last week's incidents                                                      │
  │ ▒▒▒     idle  ·  claude job  ·  1d 3h  ·  ~$0.12  ·  ~/projects/ops-notes                    │
  ╰──────────────────────────────────────────────────────────────────────────────────────────────╯

  j/k move  ·  enter/space jump  ·  x remove  ·  h hide finished  ·  q quit
```

(A mock-up with invented data. The real thing is in colour and follows your terminal theme.)

## What it does

- **Finds your agents by itself.** Claude Code sessions and background jobs, plus Codex, Gemini CLI and opencode processes. Nothing to configure or register.
- **Shows what matters at a glance.** Status (working, waiting for you, idle, done, failed), how long it has been running, the folder, and a cost estimate for Claude.
- **Jumps to the right terminal.** On Hyprland, Enter focuses the window the agent runs in, switching workspace if needed. For a background job it opens a new terminal with `claude attach` or `claude --resume`.
- **Gives sessions readable names.** Claude's auto-generated names like `billing-api-93` are replaced with the title Claude wrote for the conversation. Names you chose yourself are left alone.
- **Lets you tidy up.** `x` (or `d`) removes a card from the list without touching the agent. It stays hidden for 30 days, then the entry expires.
- **Can tap you on the shoulder.** `agentboard watch` sends a desktop notification when an agent finishes or needs input, and clicking it jumps there.
- **Stays small.** About 500 KB, starts in roughly 40 ms, and the only dependencies are `libc` and `serde_json`. There is no TUI framework; it draws with plain escape codes.

## Install

You need Rust 1.85 or newer (the project uses the 2024 edition).

```bash
git clone https://github.com/<you>/agentboard && cd agentboard
./install.sh            # builds and installs to ~/.local/bin/agentboard
./install.sh --watch    # also enables desktop notifications (systemd user service)
```

Or, if you prefer cargo: `cargo install --path .`

Then run `agentboard` in any terminal. On Omarchy you can bind it to a key with `omarchy launch or focus tui agentboard`.

It is built for Hyprland and Omarchy. The list works in any Linux terminal, but jumping to a window needs `hyprctl`. There is no tmux integration, on purpose.

## Keys

| Key | What it does |
|-----|--------------|
| `j` `k`, arrows | move up and down |
| `g` `G` | first / last |
| `Enter`, `Space` | jump to the agent's terminal; for a background job, reopen it (`claude attach <id>` if it is running, `claude --resume <id>` if it finished) |
| `h` | hide or show finished items |
| `x`, `d` | remove the selected card from the list (the agent is untouched) |
| `q`, `Esc` | quit |

Removed cards are remembered in `~/.local/state/agentboard/dismissed` (or under `$XDG_STATE_HOME`). This only affects the interactive list; `--json` and `watch` still see everything.

Other commands: `agentboard --json` prints the same list as JSON (handy for scripts and status bars), `agentboard --version`, `agentboard --help`.

## Reading a card

The three-cell bar on the left is the agent's brand colour (Claude orange, Codex green, Gemini blue, opencode grey). `███` is an interactive session and `▒▒▒` is a background job.

The status icon and label take their colours from your Omarchy theme, or sensible defaults if you don't use Omarchy:

| Icon | Meaning | Colour |
|------|---------|--------|
| `●` | working | green |
| `◆` | waiting for you | yellow |
| `○` | idle | blue |
| `✓` | done | cyan |
| `✗` | failed | red |
| `?` | a state agentboard doesn't recognise | dim |

Cards are ordered by creation date, oldest first, so they don't jump around while you work.

## About the cost figure

Claude Code doesn't store a running total, so agentboard estimates it: it adds up the token usage recorded in the session transcript (sub-agents included) and prices it with Claude Code's own price list. It is shown as `~$1.42`, or `<$0.01` for tiny amounts.

Treat it as an estimate. It usually lands 0 to 17 percent below the number Claude shows, because some calls (title generation, for example) never reach the transcript. A model it has no price for is priced like a current Opus model. Other agents show no cost. In `--json` it is the `cost` field, in dollars or `null`.

## How agents are found

| Agent | Where agentboard looks | Where the status comes from |
|-------|------------------------|-----------------------------|
| Claude Code | `~/.claude/sessions/*.json` and `~/.claude/jobs/*/state.json` (`CLAUDE_CONFIG_DIR` is honoured) | Claude itself: busy, idle or waiting |
| Codex, Gemini CLI, opencode | a scan of `/proc` for the process and its working directory | a CPU-based guess: working or idle |

A session file only counts while its process is alive and its start time matches, so stale files and reused PIDs don't show up as ghosts.

**Heads up:** Claude's files are undocumented internals and may change. The parser tolerates missing and oddly typed fields and is tested against real files from Claude Code 2.1.233 and 2.1.283 (anonymised copies live in `src/fixtures/`). Codex and Gemini detection is based on how those tools install themselves; only opencode has been seen running for real. Reports and fixtures from other versions and tools are very welcome.

## Terminals with several windows per process

Ghostty and Kitty can run many windows in one process, so the process tree alone can't tell which window an agent is in. When that happens agentboard briefly sets a unique title on the agent's tty, finds the Hyprland window that picked it up, and restores the title.

## Development

```bash
cargo test
cargo build --release
```

The code is small and split by job: `claude.rs` and `scan.rs` find agents, `cost.rs` prices transcripts, `ui.rs` draws, `jump.rs` and `launch.rs` do the jumping, `notify.rs` is the watcher. Tests use fixtures and temp directories, never your real agent data.

## License

MIT, see [LICENSE](LICENSE).
