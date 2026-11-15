# Unprompted audio-tool choice — design

Status: built, and its live smoke test ran with a negative result (see the plan's "The smoke test,
run"). Local branch only; no PR.

## Problem

The four audio tools ship, but an agent reaches for them only when a person names them. Three
measured causes: a capture never says whether a page has sound; the model sees one terse sentence
per tool over 307 tools; and Clean room cannot reach the fingerprint and compare it is allowed to
run, because offering is keyed on toolset while permission is keyed on the allow-list.

## Design

1. A capture records `audio_evidence`: typed signal kinds with where each was seen, plus a coverage
   sentence saying how far the look reached. The context is the discriminator, never the node
   methods: a fingerprinting script renders a probe tone with `createOscillator` on an
   `OfflineAudioContext`.
2. Suggestions ride on `Evidence` as a display-only `affordances` field, excluded from the id like
   `summary`, so one observation keeps one identity in every mode.
3. Clean room offers `clean_room::TOOLS ∩ registry` whatever the toolset, in the agent and in MCP.
4. Audio summaries lead with the situation, not the mechanism.
5. One general prompt clause, with no audio example: an example would test instruction-following
   on a hint and poison the experiment that measures discovery.

## Acceptance Examples

### R1

A capture says what a page's sound is, as typed signals with a coverage sentence.

- consumer: agent:cli
- entry: `web_site_capture` (tool id, over MCP or the CLI)
- example: capturing `docs/superpowers/fixtures/audio-choice/audio.html`, whose inline script
  constructs a real-time `AudioContext`, yields `audio_evidence.signals` containing
  `{"kind": "webaudio_ctor", "at": "inline"}` and a coverage sentence naming how many scripts were
  read.
- edge: `fingerprint.html` (an `OfflineAudioContext` and a probe tone) yields `offline_ctx_only`
  and no capture suggestion. `silent.html` yields an empty signal list and still a coverage
  sentence. In Clean room the capture yields no signals and a sentence saying only visible
  elements were read.

### R2

A suggestion never changes an evidence id.

- consumer: internal
- entry: `Evidence::compute_id`
- example: the same observation built in two modes keeps one id while its `affordances` differ.
- edge: a record saved before the field existed still deserialises and verifies.

### R3

Clean room offers the tools it runs, whatever toolset they belong to.

- consumer: agent:opencode, mcp client
- entry: MCP `tools/list` with Clean room on
- example: `web_audio_fingerprint` and `web_audio_compare` appear in the list while
  `web_audio_capture` does not.
- edge: enabling a toolset in Clean room adds nothing, because those tools are already listed.

### R4

An audio tool's description leads with the situation, not the mechanism.

- consumer: agent:opencode
- entry: MCP `tools/list` (the description a model reads)
- example: `web_audio_capture`'s description starts "Measure the sound a web page is playing right
  now".
- edge: the description still names that it needs approval and Deep mode.

### R5

A fingerprinting script is not mistaken for a page that makes music, and a page's own inline script
is read.

- consumer: internal
- entry: `audio_signals::scan_script`
- example: text with `new OfflineAudioContext(...)` and `createOscillator` yields `offline_ctx_only`.
- edge: minified `new(window.AudioContext||window.webkitAudioContext)()` yields `webaudio_ctor`.

## Not in this design

`core_tool_search` and a capability index (the review put them behind the smoke test, and the smoke
test came back negative); `core_target_triage` audio evidence (it holds no file inventory to read);
hidden-graph capture, fingerprint persistence and FFT-robust binning (R3.13 lines).
