# STATE: sdlc-state adoption

schema: sdlc-state/v1
spec: docs/superpowers/specs/2026-11-15-sdlc-state-design.md
plan: docs/superpowers/plans/2026-11-15-sdlc-state.md
ledger: docs/superpowers/state/sdlc-state/progress.md
test_cmd: python3 -m pytest {path} -q -k {name}
beads: off

<!-- Edited only by the controller session. Never renumber or delete a step: a
     weeks-later session resumes by ID. Never record TDD sub-steps, chat
     summaries or design discussion here — those live in the plan, the ledger
     and the spec. -->

## Resume

- next: S13
- last-green: none
- in-flight: S01-S12 done; S13 final gate next
- blockers: none
- do-not: never put `--run` in a wire check; it recurses. The wire check calls `--struct`.

## Requirements → Evidence

| REQ | Behavior (spec §) | Consumer | Entry | Tests (path::name) | W-check | Status |
|---|---|---|---|---|---|---|
| R1 | A valid STATE.md parses; a done step without evidence fails | internal | `python3 scripts/state-check --struct` | scripts/tests/test_state_check.py::test_good_project_passes, scripts/tests/test_state_check.py::test_done_without_evidence | - | covered |
| R2 | Spec requirement IDs and STATE REQ IDs are the same set | internal | `python3 scripts/state-check --struct` | scripts/tests/test_state_check.py::test_good_project_passes, scripts/tests/test_state_check.py::test_req_drift_between_spec_and_state | - | covered |
| R3 | A consumer-facing REQ has a wire step that invokes its declared entry | internal | `python3 scripts/state-check --struct` | scripts/tests/test_state_check.py::test_wire_step_must_invoke_entry, scripts/tests/test_state_check.py::test_wire_step_must_have_kind_wire | - | covered |
| R4 | The gate runs through its real entry and reports findings | agent:opencode | `python3 scripts/state-check --struct` | scripts/tests/test_state_check.py::test_good_project_passes, scripts/tests/test_state_check.py::test_step_without_check | S10 | covered |

## Touchpoints

| T | File | Must contain (literal) | For REQ | Status |
|---|---|---|---|---|
| T1 | AGENTS.md | `sdlc-state` | R3 | done |
| T2 | docs/superpowers/README.md | `sdlc-state` | R3 | done |

## Steps

- [x] S01 Write the spec with Acceptance Examples | kind: spec | req: - | est: 20m
  - check: `grep -c '^### R[0-9]' docs/superpowers/specs/2026-11-15-sdlc-state-design.md`
  - expect: `4`
  - evidence: 5bd84b1 · 2026-11-15 02:15 · exit 0 · "4"
- [x] S02 Approve the spec and record REQ-APPROVED | kind: gate | req: - | est: 5m
  - check: `grep -c 'REQ-APPROVED' docs/superpowers/state/sdlc-state/progress.md`
  - expect: `1`
  - evidence: 5bd84b1 · 2026-11-15 02:15 · exit 0 · "1"
- [x] S03 Install the personal skill | kind: build | req: R1 | est: 15m
  - check: `test -f "$HOME/.claude/skills/sdlc-state/SKILL.md" && grep -q '^name: sdlc-state' "$HOME/.claude/skills/sdlc-state/SKILL.md"; echo $?`
  - expect: `0`
  - evidence: 5bd84b1 · 2026-11-15 02:15 · exit 0 · "exit 0 (skill file present)"
- [x] S04 Vendor state-check and its tests | kind: build | req: R1 | est: 15m
  - check: `cmp -s scripts/state-check "$HOME/.claude/skills/sdlc-state/scripts/state-check"; echo $?`
  - expect: `0`
  - evidence: 5bd84b1 · 2026-11-15 02:15 · exit 0 · "exit 0 (cmp identical)"
- [x] S05 Write the plan | kind: plan | req: - | est: 20m
  - check: `test -f docs/superpowers/plans/2026-11-15-sdlc-state.md; echo $?`
  - expect: `0`
  - evidence: 5bd84b1 · 2026-11-15 02:15 · exit 0 · "exit 0 (plan present)"
- [x] S06 PLAN-SYNC: fill REQ/T tables and steps | kind: plan | req: - | est: 20m
  - check: `python3 scripts/state-check --struct; echo $?`
  - expect: `0`
  - evidence: 5bd84b1 · 2026-11-15 02:15 · exit 0 · "state-check 1.0.0: ok (1 state file)"
- [x] S07 Wire the Stop hook | kind: build | req: R4 | est: 10m
  - check: `grep -q 'state-check' .claude/settings.json; echo $?`
  - expect: `0`
  - evidence: 5bd84b1 · 2026-11-15 02:15 · exit 0 · "exit 0 (state-check in settings)"
- [x] S08 Wire the pre-push hook | kind: build | req: R4 | est: 10m
  - check: `grep -q 'state-check' .githooks/pre-push; echo $?`
  - expect: `0`
  - evidence: 5bd84b1 · 2026-11-15 02:15 · exit 0 · "exit 0 (state-check in pre-push)"
- [x] S09 Wire the CI job | kind: build | req: R4 | est: 10m
  - check: `grep -q 'state-check' .github/workflows/state.yml; echo $?`
  - expect: `0`
  - evidence: 5bd84b1 · 2026-11-15 02:15 · exit 0 · "exit 0 (state-check in state.yml)"
- [x] S10 WIRING-E2E: run the gate through its real entry | kind: wire | req: R4 | est: 10m
  - check: `python3 scripts/state-check --struct`
  - expect: exit 0
  - evidence: 5bd84b1 · 2026-11-15 02:15 · exit 0 · "state-check 1.0.0: ok (1 state file)"
- [x] S11 ROUTING: index the skill in AGENTS.md and docs/superpowers/README.md | kind: route | req: - | est: 10m
  - check: `grep -q 'sdlc-state' AGENTS.md && grep -q 'sdlc-state' docs/superpowers/README.md; echo $?`
  - expect: `0`
  - evidence: 5bd84b1 · 2026-11-15 02:15 · exit 0 · "exit 0 (both routing surfaces name sdlc-state)"
- [x] S12 Cold-reader test | kind: review | req: - | est: 15m
  - check: `grep -q 'Cold-reader: PASS' docs/superpowers/state/sdlc-state/progress.md; echo $?`
  - expect: `0`
  - evidence: 5bd84b1 · 2026-11-15 02:20 · exit 0 · "fresh subagent: S12, grep command, exit 0, recursion trap; 0 questions"
- [x] S13 FINISH: run --run --final | kind: finish | req: - | est: 10m
  - check: `python3 scripts/state-check --run --final; echo $?`
  - expect: `0`
  - evidence: 5bd84b1 · 2026-11-15 02:25 · exit 0 · "state-check 1.0.0: ok (1 state file)"
