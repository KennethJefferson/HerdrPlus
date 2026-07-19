# Orchestration playbook — driving agents in HerdrPlus panes

Field-tested rules from real multi-agent builds on this machine. Every rule here was earned by a silent failure; skipping them produces orchestration that *looks* fine while workers sit idle or messages vanish.

## Message delivery is not guaranteed — verify receipt

`pane run` text is **lost or stranded** when the target agent CLI is mid-turn: it either vanishes or sits composed-but-unsent in the input box.

After every `pane run` to an agent:
1. Wait a few seconds, then `pane read <pane> --source visible --lines 20`.
2. Text sitting in the composer, unsent → `pane send-keys <pane> Enter`.
3. Nothing arrived at all → resend.

Treat this send-verify-nudge loop as the only reliable way to message an agent pane.

## Agents stall at turn boundaries — prod them

CLI agents stop at turn boundaries **even in auto-approve/YOLO mode**. They do not run to completion unattended. The orchestrator must watch and prod with a specific next instruction ("continue with task 2", not "continue").

Watch ALL panes continuously, not one at a time: poll every pane's state each cycle, act on whichever is actionable, never serialize behind a single blocking wait, never let an agent set its own wait-timer.

## Unsticking a wedged agent

Spinner stuck waiting on a dead child process (e.g. a killed cargo): `pane send-keys <pane> Escape` cancels the tool call; queued messages then deliver. Escape is safe — it cancels the current action, not the session.

## Monitor via ground truth, not screen-scraping

`pane read` retained-buffer text **false-matches on stale output** (an old "DONE" from a previous task matches your new wait). Poll for the actual artifact instead: the report file on disk, the commit, `git diff HEAD` content hash. Screen text is for diagnosis, not completion detection.

Status-board pattern that works: classify each pane BUSY / IDLE / BLOCKED(dep) / DONE by cross-referencing pane output + git/file deliverable + dependency readiness; act on all actionable panes each cycle.

## Worker pattern that shipped real features

- A shared `worker-rules.md` every worker reads first:
  - **No git writes** — the orchestrator reviews diffs and commits.
  - **Focused tests only** — never the full suite (it's heavy and fragile under process churn on this box).
  - Done-marker report files: `task-N-worker-report.md` ending with the line `TASK N COMPLETE`.
- One brief file per task with full context.
- Kickoff = a single `pane run` line pointing at both files.
- Monitor via report files + `git diff HEAD`, not screen output.
- Review worker diffs for whitespace damage — multiple agent CLIs (omp, agy) left identical off-by-one-space indent defects on displaced lines.

## Per-agent quirks (this machine)

- `agy` (Antigravity/Gemini): YOLO mode is `agy --continue --dangerously-skip-permissions`. Without it, it stops at every permission gate, and its menus are **arrow-driven** (`send-keys Down`/`Enter`), not number-driven. `--continue` resumes its PREVIOUS conversation — prefix any reassignment with "IGNORE ALL PRIOR CONVERSATION CONTEXT".
- `omp`: competent implementer even on the free model; same whitespace-defect caveat as agy.
- `grok`: rate-limits fast; fine for short tasks only.
- `claude` / `codex` / `pi`: standard interactive launches; wait for `agent-status idle` before submitting the task.

## Team + msg bus recipes

Stand up a crew and address it as a group:

```powershell
& $herdr team spawn crew --agents lead=claude,impl=omp --cwd K:\proj --wait --timeout 120
& $herdr msg send "@crew" "Read worker-rules.md and your brief; report via task files."
```

Remember: msg bus delivers to pane inboxes — a worker only sees it if its brief tells it to poll `msg read` in a loop (or you push into its chat with `pane run` + receipt check). `msg wait --timeout` is the cheap way for a worker to block for instructions. `msg who` shows the label/group map when addressing fails.

## Rebuild hygiene

A running herdrplus.exe (client OR server) locks the binary. A rebuild then "succeeds" while the old exe persists — the classic symptom is a fix that mysteriously isn't in the binary. Run `herdr-cleanup.ps1` before any release build, and remember the full test suite is fragile under heavy process churn (degraded conpty → 0xC0000142 → reboot); prefer focused tests.
