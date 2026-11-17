---
project: craft-anything
covers_commit: 5bd84b1b80738caee76dd3611743b9fd6500000a
---

# craft-anything — project record

The project's authoritative record of state, decisions, open work, evidence and
process. Checked at done time by
`~/code/atlas/agent-machinery/library/bin/project-record check --done craft-anything`;
the routing row is `~/code/atlas/projects/craft-anything.md`.

## Current state

- Craft Anything (storytold upstream clone; fork `Rmohid/craft-anything`) is under active agent development: ego-browser driver and Endel audio-reproduction tools (`web_audio_capture`, `web_audio_fingerprint`, `web_audio_compare`, `web_audio_offline_render`) landed on local `main`; the unprompted-audio-choice slice is green on its feature branch.
- The record covers the upstream tip `5bd84b1b` (`origin/main`); feature-branch work sits in Open work until it is merged.

## Decisions

- **This repo is `upstream`-typed** — because it is a fork of an external repo where local agent work lands; the upstream tip is the landed ref the checker reads.
- **Keep the `PLAN.md` endel plan as a feature-branch artifact, not a record source** — because the checker reads sources on the covered ref and the plan lives only on the feature branch.
- **Touch `AGENTS.md` only for agent guidance** — it is the upstream workspace map; the record owns project state, not upstream docs.

## Open work and next actions

- `feature/r3.13-unprompted-audio-choice` — merge the Endel audio toolset (PR from the feature branch), then re-run `project-record check craft-anything`.
- Name the reproduction verdicts in the record once the audio comparison is machine-checked.

## Evidence and how to verify

- `git -C ~/code/craft-anything log --oneline -5 origin/main` → shows the covered tip.
- `~/code/atlas/agent-machinery/library/bin/project-record check craft-anything` → `PASS: craft-anything — record current`.
- `cargo test` in the workspace → the engine gates for the audio tools stay green.

## Process — best known way to proceed

- Run `project-record onboard` before the first landing in any new `~/code` repo; a durable project lands only with a slug.
- Keep the record on its own commit and move `covers_commit:` at each landing; the reconciler heals only the mechanical facts.
- Land feature branches to `main` by merge; upstream syncs from `origin/main`.

## Rejected improvements

- **2026-11-16 — Making the record cover the feature tip, REJECTED.** Tried: covering `918dbb4d` directly; decision rule frozen before measuring: the checker requires `covers_commit` to be an ancestor of `origin/main`; measurement: `git merge-base --is-ancestor 918dbb4d 5bd84b1b` → exit 1; verdict: the feature is not landed, so the record covers `5bd84b1b` and names the branch as open work. Evidence: this section.

## Open conflicts

- none

## Change log

- 2026-11-16 — record: created — covers upstream tip 5bd84b1b; feature work named as open — evidence: project-record check craft-anything
