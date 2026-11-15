# sdlc-state adoption — design

**Status:** approved by the handoff envelope `worktracker-676.5.2.14` (2026-11-15).

## Problem

The superpowers SDLC enforces most of its rules as prose in the agent prompt. Three
structural gaps survive: a spec requirement need not reach a test, a consumer-facing
feature need not be reachable through its real entry point, and a change need not update
a discovery surface. The verification gate is sound in isolation, but nothing requires
each initial requirement to have a test, each consumer-facing requirement to name an
entry that is exercised unmocked, or each change to update a routing surface.

## What this adopts

A personal skill `sdlc-state`, one durable checkbox artifact `STATE.md`, and one stdlib
checker `state-check`, wired through the seams superpowers already leaves open: the spec
file, the plan, the progress ledger, `task-done`, the review request, and git/CI hooks.
The superpowers plugin is not edited.

## Non-goals

- No new platform, database, server, dashboard, or required tracker. `STATE.md` plus git
  is the system; beads stays optional.
- No fork or edit of any superpowers plugin skill.
- No semantic proof that a test asserts the right thing; the reviewer question covers
  that. The checker proves existence, mapping, and re-execution.

## Acceptance Examples

### R1 — A valid STATE.md parses; a done step without evidence fails
- consumer: internal
- entry: `python3 scripts/state-check --struct`
- example: given a STATE.md with `schema: sdlc-state/v1`, one REQ row, and one step with check/expect/evidence → when `python3 scripts/state-check --struct` runs → then exit 0 and stdout contains `ok (1 state file)`
- edge: given a `[x]` step with empty evidence → when the same command runs → then exit 1 and stderr contains `without evidence`
- non-goal: no database or tracker

### R2 — Spec requirement IDs and STATE REQ IDs are the same set
- consumer: internal
- entry: `python3 scripts/state-check --struct`
- example: given a spec with `### R1` and `### R2` and a STATE with rows R1 and R2 → when `python3 scripts/state-check --struct` runs → then exit 0
- edge: given a spec with R1 and R2 but a STATE with only R1 → when the same command runs → then exit 1 and stderr contains `REQ drift`
- non-goal: no semantic diff of requirement text

### R3 — A consumer-facing REQ has a wire step that invokes its declared entry
- consumer: internal
- entry: `python3 scripts/state-check --struct`
- example: given consumer `agent:opencode`, entry `tool sync`, and W-check S02 with check `tool sync` and `kind: wire` → when `python3 scripts/state-check --struct` runs → then exit 0
- edge: given W-check S02 with check `internal_fn` and `kind: build` → when the same command runs → then exit 1 and stderr contains `does not invoke entry` and `must have kind: wire`
- non-goal: no proof that the entry is semantically correct

### R4 — The gate runs through its real entry and reports findings
- consumer: agent:opencode
- entry: `python3 scripts/state-check --struct`
- example: given a valid pilot STATE.md → when `python3 scripts/state-check --struct` runs → then exit 0 and stdout contains `ok (1 state file)`
- edge: given a STATE.md with a step that has no check command → when the same command runs → then exit 1 and stderr contains `no check command`
- non-goal: no automatic fix
