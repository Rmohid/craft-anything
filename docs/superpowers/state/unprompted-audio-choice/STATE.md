# STATE: unprompted audio-tool choice

schema: sdlc-state/v1
spec: docs/superpowers/specs/2026-11-14-unprompted-audio-choice-design.md
plan: docs/superpowers/plans/2026-11-14-unprompted-audio-choice.md
ledger: docs/superpowers/state/unprompted-audio-choice/progress.md
test_cmd: cargo test --workspace --lib {name}
beads: off

<!-- Edited only by the controller session. Never renumber or delete a step: a
     weeks-later session resumes by ID. Never record TDD sub-steps, chat
     summaries or design discussion here — those live in the plan, the ledger
     and the spec. -->

## Resume

- next: S13 (finish: run the gate, then this branch is done — local only, no PR)
- last-green: 68363266
- in-flight: nothing
- blockers: none
- do-not: the smoke test's negative result is real. Do not rebuild `core_tool_search` or a
  capability index without a page whose sound can be captured but not read. The Kimi OpenRouter
  key is rejected (401 "User not found"); the eval ran on the OpenCode backend instead. A
  concurrent session is editing `docs/STATUS.md` and its own `docs/superpowers/state/sdlc-state/`
  — stage files by name, never `git add -A`.

## Requirements → Evidence

| REQ | Behavior (spec §) | Consumer | Entry | Tests (path::name) | W-check | Status |
|---|---|---|---|---|---|---|
| R1 | A capture reports typed audio signals and a coverage sentence | agent:cli | `web_site_capture` | crates/engine/src/tools/audio_signals.rs::a_page_that_makes_sound_suggests_capture_and_says_why, crates/engine/src/tools/audio_signals.rs::a_silent_page_says_nothing_and_still_says_how_far_it_looked | S05 | covered |
| R2 | A suggestion never changes an evidence id | internal | `Evidence::compute_id` | crates/evidence/src/lib.rs::display_fields_do_not_change_the_id, crates/evidence/src/lib.rs::an_affordance_survives_a_round_trip_and_is_omitted_when_empty | - | covered |
| R3 | Clean room offers the tools it runs, whatever the toolset | agent:opencode, mcp client | `tools/list` | crates/ai/src/agent.rs::clean_room_offers_the_audio_tools_it_runs_whatever_their_toolset, crates/mcp/src/tests.rs::clean_room_lists_the_audio_tools_it_runs_without_enabling_browser | S07 | covered |
| R4 | An audio tool's description leads with the situation | agent:opencode | `tools/list` | crates/engine/src/tools/browser_tools/tests.rs::the_audio_tools_say_what_they_are_for_before_how_they_work, crates/engine/src/tools/browser_tools/tests.rs::the_audio_capture_still_says_it_needs_approval_and_deep_mode | S09 | covered |
| R5 | A fingerprint probe is not a music page; an inline script is read | internal | `audio_signals::scan_script` | crates/engine/src/tools/audio_signals.rs::a_fingerprinting_script_that_renders_a_tone_is_still_not_a_music_page, crates/engine/src/tools/audio_signals.rs::a_live_graph_is_recognised_by_its_context | - | covered |

## Touchpoints

| T | File | Must contain (literal) | For REQ | Status |
|---|---|---|---|---|
| T1 | docs/STATUS.md | `audio_evidence` | R1 | done |
| T2 | docs/ROADMAP.md | `audio_evidence` | R1 | done |
| T3 | docs/tools.md | `web_audio_capture` | R4 | done |

## Steps

- [x] S01 Write the design spec with Acceptance Examples | kind: spec | req: - | est: 20m
  - check: `grep -c '^### R[0-9]' docs/superpowers/specs/2026-11-14-unprompted-audio-choice-design.md`
  - expect: `5`
  - evidence: 68363266 · 2026-11-14 21:45 · exit 0 · "5"
- [x] S02 Record REQ-APPROVED for the five requirements | kind: gate | req: - | est: 5m
  - check: `grep -c REQ-APPROVED docs/superpowers/state/unprompted-audio-choice/progress.md`
  - expect: `1`
  - evidence: 68363266 · 2026-11-14 21:45 · exit 0 · "1"
- [x] S03 Build typed audio signals on the capture | kind: build | req: R1 | est: 30m
  - check: `cargo test --workspace --lib a_page_that_makes_sound_suggests_capture`
  - expect: `test result: ok`
  - evidence: 16f32026 · 2026-11-14 20:52 · exit 0 · "test result: ok. 1 passed"
- [x] S04 Build suggestions as a display-only field | kind: build | req: R2 | est: 20m
  - check: `cargo test --workspace --lib display_fields_do_not_change_the_id`
  - expect: `test result: ok`
  - evidence: 16f32026 · 2026-11-14 20:52 · exit 0 · "test result: ok. 1 passed"
- [x] S05 Wire: the capture's own entry reports the signal | kind: wire | req: R1 | est: 10m
  - check: `d=$(mktemp -d); python3 -m http.server 8747 --bind 127.0.0.1 --directory docs/superpowers/fixtures/audio-choice >/dev/null 2>&1 & srv=$!; sleep 1; { printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","clientInfo":{"name":"gate"}}}' '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"web_site_open","arguments":{"url":"http://127.0.0.1:8747/audio.html"}}}'; printf '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"web_site_capture","arguments":{"output_directory":"%s/capture","widths":[1280]}}}\n' "$d"; } | cargo run -q -p craft-anything-cli -- --deep --allow-run mcp --all-tools | grep -c webaudio_ctor; kill $srv`
  - expect: `1`
  - evidence: ff9bac64 · 2026-11-14 21:20 · exit 0 · "1"
- [x] S06 Build Clean-room offering by permission, not toolset | kind: build | req: R3 | est: 30m
  - check: `cargo test --workspace --lib clean_room_offers_the_audio_tools`
  - expect: `test result: ok`
  - evidence: 16f32026 · 2026-11-14 20:52 · exit 0 · "test result: ok. 1 passed"
- [x] S07 Wire: a Clean-room tools/list names the fingerprint | kind: wire | req: R3 | est: 10m
  - check: `printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","clientInfo":{"name":"gate"}}}' '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}' | cargo run -q -p craft-anything-cli -- --clean-room mcp --all-tools | grep -c web_audio_fingerprint`
  - expect: `1`
  - evidence: 16f32026 · 2026-11-14 20:52 · exit 0 · "1"
- [x] S08 Build descriptions that lead with the situation | kind: build | req: R4 | est: 15m
  - check: `cargo test --workspace --lib the_audio_tools_say_what_they_are_for`
  - expect: `test result: ok`
  - evidence: 68363266 · 2026-11-14 21:42 · exit 0 · "test result: ok. 1 passed"
- [x] S09 Wire: tools/list carries the sentence a model reads | kind: wire | req: R4 | est: 5m
  - check: `printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","clientInfo":{"name":"gate"}}}' '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}' | cargo run -q -p craft-anything-cli -- mcp --all-tools | grep -c "Measure the sound a web page is playing right now"`
  - expect: `1`
  - evidence: 68363266 · 2026-11-14 21:42 · exit 0 · "1"
- [x] S10 Build the context discriminator and inline-script reading | kind: build | req: R5 | est: 30m
  - check: `cargo test --workspace --lib a_fingerprinting_script_that_renders_a_tone`
  - expect: `test result: ok`
  - evidence: ff9bac64 · 2026-11-14 21:11 · exit 0 · "test result: ok. 1 passed"
- [x] S11 Run the smoke test and record the result | kind: spike | req: - | est: 30m
  - check: `grep -c 'neither arm called an audio tool' docs/ROADMAP.md`
  - expect: `1`
  - evidence: fa098a3c · 2026-11-14 21:37 · exit 0 · "1"
- [x] S12 Route the change to the docs people read | kind: route | req: - | est: 10m
  - check: `grep -c audio_evidence docs/STATUS.md`
  - expect: `1`
  - evidence: 26175ac8 · 2026-11-14 21:11 · exit 0 · "1"
- [x] S13 Finish: the gate is green and the branch is done | kind: finish | req: - | est: 10m
  - check: `python3 scripts/state-check --struct`
  - expect: `ok`
  - evidence: 68363266 · 2026-11-15 00:05 · exit 0 · "state-check 1.0.0: ok (2 state files)"
