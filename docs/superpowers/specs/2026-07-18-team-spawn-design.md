# Team Spawn Primitive — Design Spec

Date: 2026-07-18
Status: approved by Tony (brainstorm session)
Builds on: pane balance (PR #1, protocol 17), msg bus (PR #2, protocol 18)

## Problem

Standing up an agent team today means hand-scripting: `workspace create`, N × `pane split`, N × `pane rename`, N × `pane run`, N × `msg group join`, `pane balance`. It is slow, racy, and partial failures leave half-built workspaces. Orchestrators (Claude Code or scripts driving herdrplus via CLI) need one command that produces a ready-to-address team and returns the label→pane map.

## Decisions

- **Server-native** `team.spawn` wire method (protocol 19), matching the precedent of pane balance and msg bus. One request, one owner of rollback, reusable later by the TUI.
- Mixed roster via `--agents`, labels auto-generated with optional `label=` override per entry.
- Agent launch commands resolved server-side from a new `[team.agents]` config registry; unknown names pass through verbatim as commands.
- Orchestrator pane is opt-in via `--with-orch [cmd]`.
- Spawn returns immediately; `--wait` (client-side polling) is opt-in.
- One shared `--cwd` for the whole team. Worktree-per-agent stays out of v1 (agent CLIs like grok's `--worktree` already cover it via registry config).

## CLI surface

```
herdrplus team spawn <team-name>
    --agents <entry>[,<entry>...]     # entry = agent | label=agent
    [--cwd <dir>]                     # shared by all panes; default: caller's cwd
    [--with-orch [cmd]]               # extra pane labeled "orch"; default cmd = plain shell
    [--wait] [--timeout <secs>]       # poll until all agent panes ready (timeout default: 60)
```

(Amended at plan stage: the originally-specced `--json` flag is dropped — herdr's CLI convention for API-backed commands is JSON output always, so the pane map is always machine-readable.)

- `<team-name>` becomes both the workspace label and the msg group name.
- Label defaults: per-agent counters — `claude-1`, `claude-2`, `grok-1`. Explicit labels via `ws1=claude`.
- Example: `herdrplus team spawn review --agents ws1=claude,ws2=claude,reviewer=grok --wait`
- Output pane map: `{workspace_id, group, panes: [{label, pane_id, agent, command}]}`.
- Out of scope v1: `team status` (covered by `pane list` + `msg who`); teardown (covered by `workspace close`, whose msg-group cascade shipped in PR #2).

## Agent registry (config)

New optional config section; values are full command lines typed into the pane's shell (same mechanism as `pane run` / `PaneSendInput` — no argv parsing by herdrplus):

```toml
[team.agents]
claude = "claude --dangerously-skip-permissions"
agy    = "agy --continue --dangerously-skip-permissions"
grok   = "grok --always-approve"
omp    = "omp --approval-mode yolo"
pi     = "pi"                          # pi has no approval prompts; runs YOLO by default
```

(Flags verified against the installed CLIs on 2026-07-18: grok also accepts `--permission-mode bypassPermissions`; omp also accepts `--auto-approve`.)

- Resolution is **server-side** (server owns config).
- Registry hit → configured command string. Miss → the roster entry name itself is the command, verbatim (quoted entries like `--agents "ws1=pwsh -NoLogo"` work).

## Wire method

- `team.spawn`, protocol bumped to **19** (strict-equality client/server gate).
- Bump checklist (all four sites): `src/protocol/wire.rs`, schema artifact regen, `tests/api_ping.rs`, `tests/cli_wrapper.rs`, `tests/support/mod.rs`.
- Params: `{name, entries: [{label?, agent}], cwd?, orch_command??, focus?}`.
  (`orch_command` is present-with-default vs absent: absent = no orch pane; present-null/empty = plain shell.)
- Result: the pane map above.

## Data flow (server handler, in order)

1. **Resolve** every roster entry against `[team.agents]`. Any failure aborts before creation.
2. **Create workspace** (label = team name, cwd; no focus steal unless `focus`). Its initial pane is agent pane #1.
3. **Split** to N (+1 for orch) panes: repeatedly split the largest-area pane, cutting across its longer axis (yields a rough grid; exact ratios don't matter because of step 4).
4. **Balance** via the PR #1 `layout.balance` logic.
5. **Label** each pane (existing rename path) and **join** each to msg group `<team-name>` — membership complete before any agent starts, so no early send hits a partially-joined group.
6. **Send command text** to each pane last (`PaneSendInput` + Enter). ConPTY buffers typed input until the shell reads it, so shell-startup racing is expected-safe — explicitly on the live-e2e verify list, not assumed.
7. Return the pane map.

`--wait` (client-side): poll pane agent state until every roster pane reports a state other than `unknown` (detect module recognized a running agent), or timeout. Panes whose command is not a detectable agent (e.g. `pwsh` passthrough; the orch pane always) are excluded and reported as `not detectable`. Note: detect currently knows claude/codex/gemini/pi; omp may classify as pi or unknown — live e2e will tell.

## Error handling

- **Parse-time (exit 2, nothing created):** empty roster; duplicate labels; label `orch` in roster combined with `--with-orch`; labels/team name violating msg addressing syntax (no `@`, `/`, whitespace — same charset the msg resolver enforces).
- **Team-name collision (exit 1):** if msg group `<name>` already has members, refuse: "team already exists — pick another name or close the old workspace". Prevents merging two teams' inboxes.
- **Server-side failure at any step:** close the just-created workspace (cascading pane + msg-group teardown rides the PR #2-hardened path); return one error naming the failed step and pane. No half-built teams.
- **Exit codes:** 0 success; 2 usage; 1 spawn failed (rolled back); 3 `--wait` timeout — spawn *succeeded*, pane map still printed with per-pane readiness. Orchestrators must distinguish "team up, agent slow" from "no team".

## Testing

- **Unit:** roster parsing (labels, dupes, passthrough with quotes/spaces), registry resolution precedence, label/name validation.
- **Integration (test-server harness):** spawn with passthrough entries → workspace exists, N panes with expected labels, `msg who` shows full membership, layout balanced, each pane received its command text (`pane read`); forced mid-spawn failure → no workspace/group residue; name-collision refusal.
- **Protocol 19:** all four checklist sites updated.
- **Live e2e before PR (real binary):** spawn a real mixed team (`claude` + `grok`), agents boot, `--wait` reaches ready, msg round-trip between two spawned panes, and the ConPTY input-buffering assumption holds.

## Post-review amendments (Sol56 spec review, 2026-07-18)

External review (GPT-5.6 Sol, high effort; 5 major findings, no blockers) landed after
implementation began. Resolutions:

1. **Readiness signal:** `--wait` treats a pane as ready when `PaneInfo.agent` is set
   (process recognized) OR `agent_status != unknown` — not `agent_status` alone. The detect
   module recognizes ~21 agents including omp and grok; roster names it can't identify are
   excluded from the wait and reported as `not detectable`.
2. **Name rules:** team name `all` is reserved (`@all` is the broadcast target). Labels
   matching canonical pane-id grammar (`w1:p2`) are rejected as unaddressable. Explicit
   labels are trimmed before validation/duplicate checks (pane rename trims). Team spawn is
   intentionally stricter than the raw msg-bus validator (which permits whitespace and
   embedded `@`).
3. **cwd ownership:** the CLI always resolves and sends `cwd` (defaulting to the caller's
   `current_dir()`); an omitted wire `cwd` is server-default behavior for raw API callers only.
4. **Atomicity contract:** atomic means *no persistent herdr workspace/msg state remains*
   after a failed spawn, and the caller's `active`/`selected` focus is restored. It does NOT
   mean external side effects are undone: launch commands already delivered to earlier panes
   may have briefly executed, and event subscribers may observe partial-team
   creation events followed by `WorkspaceClosed` (the composed handlers emit their normal
   events). Deferred-event/low-level-mutation rework is explicitly out of v1 scope.
5. **Roster grammar:** an entry is `label=agent` only when the text before the first `=` is a
   simple identifier (`[A-Za-z0-9_-]+`); otherwise the whole entry is the command, verbatim
   (so `claude --model=x` is a command, not a label). Commas remain structural — commands
   containing commas must be defined in `[team.agents]`.
6. **Roster cap:** max 24 panes per team (matches the layout API's generated-layout cap).
   Error `team_too_large`.

## Review provenance

Brainstormed and approved in-session 2026-07-18. Sol56 spec review completed post-hoc same
day (5 majors folded in as fix tasks 6-7; verdict: server-native direction sound, teardown
foundation verified clean). Sol56 diff review still to come at whole-branch stage, per the
two-reviewer gate.
