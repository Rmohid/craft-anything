# sdlc-state adoption — plan

**Spec:** `docs/superpowers/specs/2026-11-15-sdlc-state-design.md`
**State:** `docs/superpowers/state/sdlc-state/STATE.md`

One task per plan step. STATE.md carries one `build` step per task below;
the TDD sub-steps (failing test → run → implement → run → commit) live here,
not in STATE.md.

## T1 — Spec with Acceptance Examples

Write the design spec with an `## Acceptance Examples` appendix: one
`### R<n>` per requirement with a consumer, an entry, at least one concrete
`example:` and the real `edge:` cases. No function names, no file layouts.

State step S01. Check: the spec contains four `### R<n>` headings.

## T2 — Approve the spec and record REQ-APPROVED

Ask for approval of the spec and the appendix together, then write
`| R1-R4 | REQ-APPROVED |` to the ledger with the date.

State step S02. Check: the ledger contains one REQ-APPROVED line.

## T3 — Install the personal skill

Install the skill at `~/.agents/skills/sdlc-state` (opencode loads
`~/.agents/skills`): `SKILL.md`, `templates/`, `scripts/state-check` and its
tests. The repo's `scripts/state-check` is a byte-identical copy of the
skill's `scripts/state-check`.

State step S03. Check: the skill's `SKILL.md` exists at the install location
and names the skill.

## T4 — Vendor state-check and its tests

Copy `state-check` and its test suite into the repo and keep them
byte-identical to the skill copy.

State step S04. Check: `cmp` says the vendored copy is identical.

## T5 — Write this plan

State step S05. Check: this file exists.

## T6 — PLAN-SYNC: fill the REQ and Touchpoints tables and steps

One `build` step per task above; one `wire` step per consumer-facing
requirement, whose check invokes the declared entry literally; one `route`
step per touchpoint; then `review`, then `finish`.

State step S06. Check: `python3 scripts/state-check --struct` exits 0.

## T7 — Wire the Stop hook

`.claude/settings.json` Stop hook runs
`python3 "$CLAUDE_PROJECT_DIR"/scripts/state-check --struct --hook`.
Claude Code convenience only; `doctor` does not require it.

State step S07. Check: `state-check` appears in `.claude/settings.json`.

## T8 — Wire the pre-push hook

`.githooks/pre-push` runs `python3 scripts/state-check --run`, skipping
cleanly when the branch carries no `STATE.md`.

State step S08. Check: `state-check` appears in `.githooks/pre-push`.

## T9 — Wire the CI job

`.github/workflows/state.yml` runs `python3 scripts/state-check --run --final`
on every pull request into the default branch.

State step S09. Check: `state-check` appears in `.github/workflows/state.yml`.

## T10 — WIRING-E2E: run the gate through its real entry

The wire step for R4, whose consumer is `agent:opencode`. Its check invokes
the declared entry literally: `python3 scripts/state-check --struct`.

State step S10. Check: the command exits 0.

## T11 — ROUTING: index the skill

`AGENTS.md` and `docs/superpowers/README.md` both name `sdlc-state`.

State step S11. Check: `sdlc-state` appears in both files.

## T12 — Cold-reader test

Give a fresh subagent only `STATE.md` and ask it to name the next step, the
command, the output that means done, and one trap. It passes with no
questions asked, and the pass is recorded in the ledger.

State step S12. Check: `Cold-reader: PASS` appears in the ledger.

## T13 — FINISH

`python3 scripts/state-check --run --final` exits 0, and the ledger carries
the FINAL-GREEN line with the sha.

State step S13. Check: the command exits 0.
