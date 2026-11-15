# Superpowers workflow

This directory holds the durable artifacts of the superpowers SDLC:

- `specs/` — approved designs, each with an `## Acceptance Examples` appendix.
- `plans/` — implementation plans derived from a spec.
- `state/<topic>/STATE.md` — the one durable checkbox artifact for multi-session work.
- `state/<topic>/progress.md` — append-only ledger of DONE, SPLIT, RULING and
  REQ-APPROVED lines.

## Multi-session work

Use the personal `sdlc-state` skill when work spans more than one session or more than
about three steps. It keeps `STATE.md` as the single source of truth, maps every spec
requirement to a test that is re-run, and requires a consumer-facing requirement to name
an entry point that is exercised unmocked.

The mechanical gate is `scripts/state-check`:

```sh
python3 scripts/state-check --struct        # fast parse-only check
python3 scripts/state-check --run           # re-run mapped tests and wire checks
python3 scripts/state-check --run --final   # before finishing a branch
python3 scripts/state-check doctor          # are the hooks still installed?
```

The Stop hook, the pre-push hook and the CI job call the same checker. A `[x]` step
without evidence fails `--struct`; only `--run` re-executes the evidence.
