//! Browser tools over the Chrome DevTools Protocol (loopback CDP).
//!
//! Passive tools attach to a user-started browser (`--remote-debugging-port`) or, when asked,
//! launch a located Chromium headless; they observe only and never navigate, click or evaluate.
//! The one active tool, [`web_walkthrough_record`], runs a scripted session and is consent-gated.
//! Every connection is deadlined and cancellable, output is bounded, and captures are
//! data-minimised: cookies, auth headers and storage values are never kept, JSON bodies are
//! reduced to outlines, and declared secrets are redacted in every encoding.

use super::native_common::spec;
use super::web_common::{self, Browser, LaunchOptions, Secrets};
use crate::tool::{Ctx, Effects, Requires, ToolOutput, ToolSpec, Toolset, parse_params, schema_of};
use craft_anything_analysis::audio::{self, Edge, EnvelopeSample, Fingerprint, GraphSignature};
use craft_anything_engines::cdp::{self, CdpClient, Deadline, PAGE_MAX_MESSAGE};
use craft_anything_evidence::{Authority, Confidence, Coverage, CoverageStatus, ErrorCode, EvidenceBuilder, Limitation, Location, Subject, ToolError};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::Duration;

pub fn specs() -> Vec<ToolSpec> {
    vec![
        spec(
            "web_browser_connect",
            "Connect to a browser",
            "Attach to a loopback CDP endpoint (a browser started with --remote-debugging-port) or launch a located Chromium headless, and report the product, protocol version and debuggable target count. A quick connectivity check before the other browser tools.",
            "Check Craft Anything can talk to a web browser for live inspection.",
            Toolset::Browser,
            Effects::NETWORK_EVIDENCE.union(Effects::LAUNCHES_PROCESS),
            Requires::Nothing,
            schema_of::<ConnectParams>,
            web_browser_connect,
        ),
        spec(
            "web_pages_list",
            "List browser pages",
            "List the debuggable targets (pages, workers, iframes) of a loopback CDP endpoint with their ids, kinds and origin-only URLs, plus the browser product and protocol version. Read-only discovery over HTTP; no WebSocket.",
            "See which pages are open in a browser you can inspect.",
            Toolset::Browser,
            Effects::NETWORK_EVIDENCE.union(Effects::LAUNCHES_PROCESS),
            Requires::Nothing,
            schema_of::<ListTargetsParams>,
            web_pages_list,
        ),
        spec(
            "web_page_show",
            "Inspect a web page",
            "Passively observe one page over CDP for a short window: frame origins, a DOM summary (node names/types and attribute names with value lengths only), parsed scripts (optionally their sources, each with its sha256), resources, and — each opt-in — accessibility names, console entries, network request outlines, WebSocket outlines and storage key names. Never navigates, clicks or evaluates. Cookies, auth headers and storage values are never kept; JSON bodies become outlines.",
            "Look at what a web page is made of and what it loads, without touching it or exposing passwords.",
            Toolset::Browser,
            Effects::NETWORK_EVIDENCE.union(Effects::LAUNCHES_PROCESS),
            Requires::Nothing,
            schema_of::<InspectPageParams>,
            web_page_show,
        ),
        spec(
            "web_page_js_analyze",
            "Analyse a page's JavaScript",
            "Fetch the scripts a live page has loaded over CDP and run the built-in JavaScript analyser on them: chunk graph, module boundaries, routes, HTTP endpoints and vendor fingerprints. Optionally decodes inline (data:) source maps and fetches same-origin maps. Passive; nothing is executed.",
            "Map how a website's JavaScript is put together, straight from a running page.",
            Toolset::Browser,
            Effects::NETWORK_EVIDENCE.union(Effects::LAUNCHES_PROCESS),
            Requires::Nothing,
            schema_of::<AnalyzeBundleParams>,
            web_page_js_analyze,
        ),
        spec(
            "web_page_watch",
            "Watch a page over time",
            "Passively watch one page over a longer window (default 10s) and record a bounded timeline of network request outlines, console entries, WebSocket frame outlines and navigation/lifecycle events, all data-minimised. Never navigates or evaluates.",
            "Watch what a web page does over a few seconds — what it loads and sends — without touching it.",
            Toolset::Browser,
            Effects::NETWORK_EVIDENCE.union(Effects::LAUNCHES_PROCESS),
            Requires::Nothing,
            schema_of::<ObserveSessionParams>,
            web_page_watch,
        ),
        spec(
            "web_mcp_tools_list",
            "Find in-page MCP tools",
            "Best effort: enable the experimental WebMCP CDP domain and collect any tools the page advertises (names and declared input outlines only) during a short window. The tools are never invoked. Honest when the browser does not support the domain.",
            "Check whether a web page offers built-in AI tools, without running any of them.",
            Toolset::Browser,
            Effects::NETWORK_EVIDENCE.union(Effects::LAUNCHES_PROCESS),
            Requires::Nothing,
            schema_of::<DiscoverWebmcpParams>,
            web_mcp_tools_list,
        ),
        spec(
            "web_page_screenshot_take",
            "Screenshot a page",
            "Capture a PNG screenshot of the current state of one page and return it inline with its sha256 (plus bytes, dimensions, base64). Passive; the page is not navigated.",
            "Take a picture of a web page as it looks right now.",
            Toolset::Browser,
            Effects::NETWORK_EVIDENCE.union(Effects::LAUNCHES_PROCESS),
            Requires::Nothing,
            schema_of::<ScreenshotParams>,
            web_page_screenshot_take,
        ),
        spec(
            "web_snapshots_compare",
            "Compare two page captures",
            "Compare two web_page_show captures (frame origins, elements, resources, endpoints and script URLs added or removed), or two web_walkthrough_record runs step by step (steps aligned by step_id, then position: action, status, URL, error and artifact changes; network requests; event counts). normalization lists plain text replacements (e.g. a build id) applied to both sides first. Pure computation over evidence; no browser.",
            "See what changed between two looks at a web page.",
            Toolset::Browser,
            Effects::EVIDENCE,
            Requires::Nothing,
            schema_of::<CompareCapturesParams>,
            web_snapshots_compare,
        ),
        spec(
            "web_pixels_compare",
            "Compare two screenshots",
            "Compare two PNG screenshots pixel by pixel with a per-channel threshold (0..255): whether they are comparable, how many pixels differ, the difference fraction, the largest channel delta and the changed bounding box. No pixel data is returned. Pure computation.",
            "Measure how two pictures of a web page differ.",
            Toolset::Browser,
            Effects::EVIDENCE,
            Requires::Nothing,
            schema_of::<CompareScreenshotsParams>,
            web_pixels_compare,
        ),
        spec(
            "web_walkthrough_record",
            "Run a scripted web session",
            "Run a scripted browser session (goto/click/fill/press/select/wait; elements by CSS selector, test_id, or role and accessible name; optional step_id per step) over CDP, attaching to a loopback endpoint or launching a located Chromium. A fixed environment (viewport, locale, time zone, colour scheme, reduced motion, service workers; 1280x720, en-US, UTC, light by default) makes runs repeatable. Captures per-step network request outlines and optional screenshots; keep_bodies can also keep request/response bodies and header values (off by default, credentials and declared secrets redacted). storage can start the run with cookies and local/session storage (values treated as secrets). capture records screenshot, DOM, accessibility tree, history and storage key names after each step and/or at the end. capture.events records console, page errors, WebSockets, frames, workers, popups and (blocked) downloads, each tagged with its step. Executes page interactions and accesses the network, so it needs your approval; declared secrets are redacted in every encoding and bodies are reduced to outlines.",
            "Drive a website through a few steps and record what it does, with your permission.",
            Toolset::Browser,
            Effects::EVIDENCE.union(Effects::EXECUTES_FOREIGN_CODE).union(Effects::ACCESSES_NETWORK).union(Effects::LAUNCHES_PROCESS),
            Requires::Nothing,
            schema_of::<WalkthroughParams>,
            web_walkthrough_record,
        ),
        spec(
            "web_walkthrough_check",
            "Check a scripted web session",
            "Check a script for web_walkthrough_record without running a browser, and list every problem with the step it's in: unknown step kinds, missing fields, selectors CSS doesn't have (XPath, :has-text and the like), locators that aren't exactly one of selector, test_id or role with name, bad settings. Errors are what the recorder would refuse, by the same rules; warnings are what it would run but probably not as meant (a misspelt field it ignores, a repeated step_id). Touches no target.",
            "Check a scripted browser session for mistakes before running it.",
            Toolset::Browser,
            Effects::READ_ONLY,
            Requires::Nothing,
            schema_of::<script_check::WalkthroughCheckParams>,
            script_check::web_walkthrough_check,
        ),
        spec(
            "web_audio_fingerprint",
            "Fingerprint an audio scene",
            "Turn a web_audio_capture result (the evidence record, its id, or its raw {graph, spectrum, rms, mixer_state, mixer_envelope} object) — or an inline spectrum and RMS envelope — into a versioned audio fingerprint: 48 log-spaced spectrum bins, a 1 Hz loudness envelope, the audio-graph signature and the page's mixer state. Pure computation over evidence already in hand; no browser.",
            "Record what a web page's audio sounds like, so two captures can be compared.",
            Toolset::Browser,
            Effects::EVIDENCE,
            Requires::Nothing,
            schema_of::<FingerprintAudioParams>,
            web_audio_fingerprint,
        ),
        spec(
            "web_audio_compare",
            "Compare two audio fingerprints",
            "Compare two audio fingerprints (from web_audio_fingerprint or web_audio_offline_render): log-bin cosine, RMS-envelope correlation, graph differences, mixer-envelope distance, and whether the two scenes are alike (spectral cosine and RMS correlation within their tolerances; 0.85 and 0.80 by default). Pure computation; no browser.",
            "Measure whether a rebuilt audio scene sounds like the original.",
            Toolset::Browser,
            Effects::EVIDENCE,
            Requires::Nothing,
            schema_of::<CompareAudioParams>,
            web_audio_compare,
        ),
        spec(
            "web_audio_capture",
            "Capture a page's live audio",
            "Listen to a page's live Web Audio graph over CDP for 5 to 120 seconds: node kinds and connections, an AnalyserNode's averaged frequency spectrum, a 1 Hz loudness envelope, the page's mixer state (scenario id, layer gains and pans), and an outline of the requests the page made while listening. Injects an introspection script, so it needs your approval and Deep mode; cookies, storage values and declared secrets are redacted, and no audio is recorded.",
            "Measure what a web page is playing right now, so a rebuild can be compared against it.",
            Toolset::Browser,
            Effects::EVIDENCE.union(Effects::EXECUTES_FOREIGN_CODE).union(Effects::ACCESSES_NETWORK).union(Effects::LAUNCHES_PROCESS),
            Requires::Nothing,
            schema_of::<CaptureAudioParams>,
            web_audio_capture,
        ),
    ]
}

// ---------------------------------------------------------------------------------------------
// Shared parameters

fn d_timeout() -> u64 {
    60
}

/// How to obtain a browser; shared by every tool.
pub(crate) fn resolve_browser(
    ctx: &Ctx<'_>,
    endpoint: Option<&str>,
    launch: bool,
    headless: bool,
    no_sandbox: bool,
    dl: &Deadline,
) -> Result<Browser, ToolError> {
    let opts = LaunchOptions { allow_launch: launch, headless, no_sandbox, startup_secs: 30 };
    Browser::resolve(ctx, endpoint, &opts, dl)
}

pub(crate) fn deadline(ctx: &Ctx<'_>, timeout_secs: u64) -> Result<Deadline, ToolError> {
    if !(1..=600).contains(&timeout_secs) {
        return Err(ToolError::invalid("timeout_secs must be 1..=600"));
    }
    Ok(Deadline::new(Duration::from_secs(timeout_secs), ctx.cancel.clone()))
}

pub(crate) fn browser_env(clean_room: bool, b: &Browser) -> craft_anything_evidence::Environment {
    web_common::cdp_env(clean_room, Some(b.version.clone()))
}

pub(crate) fn window_deadline(ctx: &Ctx<'_>, watch_ms: u64, overall: &Deadline) -> Deadline {
    let ms = watch_ms.clamp(1, 600_000);
    let dl = Deadline::new(Duration::from_millis(ms), ctx.cancel.clone());
    if overall.expired() { overall.clone() } else { dl }
}

// ---------------------------------------------------------------------------------------------
// web_browser_connect

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConnectParams {
    /// Loopback CDP endpoint, e.g. http://127.0.0.1:9222. Omit to launch a browser.
    #[serde(default)]
    pub cdp_endpoint: Option<String>,
    /// Launch a located Chromium when no endpoint is given.
    #[serde(default)]
    pub launch: bool,
    /// Launch headless (when launching).
    #[serde(default = "yes")]
    pub headless: bool,
    /// Pass --no-sandbox when launching (needed in some containers).
    #[serde(default)]
    pub no_sandbox: bool,
    #[serde(default = "d_timeout")]
    pub timeout_secs: u64,
}

fn yes() -> bool {
    true
}

fn web_browser_connect(ctx: &mut Ctx<'_>, params: Value) -> Result<ToolOutput, ToolError> {
    let p: ConnectParams = parse_params(params)?;
    let dl = deadline(ctx, p.timeout_secs)?;
    let browser = resolve_browser(ctx, p.cdp_endpoint.as_deref(), p.launch, p.headless, p.no_sandbox, &dl)?;
    let targets = cdp::json_list(&browser.endpoint, &dl).unwrap_or_default();
    let summary = format!(
        "Connected to {} ({}) at {}; {} debuggable target(s).",
        browser.product,
        if browser.launched { "launched" } else { "attached" },
        browser.endpoint.origin,
        targets.len()
    );
    let ev = EvidenceBuilder::new("web_browser_connect", Subject { kind: "browser".into(), sha256: None, name: Some(browser.endpoint.origin.clone()) }, browser_env(ctx.session.clean_room(), &browser))
        .params(json!({"cdp_endpoint": p.cdp_endpoint, "launched": browser.launched}))
        .result(json!({"endpoint": browser.endpoint.origin, "product": browser.product, "protocol_version": browser.version, "launched": browser.launched, "targets": targets.len()}))
        .summary(summary)
        .confidence(Confidence::Observed)
        .authority(Authority::OutsideService)
        .coverage(Coverage::complete(1))
        .location(Location::new("url", browser.endpoint.origin.clone()))
        .build()?;
    Ok(ToolOutput::evidence(ev))
}

// ---------------------------------------------------------------------------------------------
// web_pages_list

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListTargetsParams {
    #[serde(default)]
    pub cdp_endpoint: Option<String>,
    #[serde(default)]
    pub launch: bool,
    #[serde(default = "yes")]
    pub headless: bool,
    #[serde(default)]
    pub no_sandbox: bool,
    #[serde(default = "d_timeout")]
    pub timeout_secs: u64,
}

fn target_row(t: &Value) -> Value {
    json!({
        "id": t.get("id").and_then(Value::as_str).unwrap_or_default(),
        "type": t.get("type").and_then(Value::as_str).unwrap_or_default(),
        "url": web_common::sanitize_url(t.get("url").and_then(Value::as_str).unwrap_or_default()),
        "title": t.get("title").and_then(Value::as_str).unwrap_or_default(),
        "has_debugger": t.get("webSocketDebuggerUrl").is_some(),
    })
}

fn web_pages_list(ctx: &mut Ctx<'_>, params: Value) -> Result<ToolOutput, ToolError> {
    let p: ListTargetsParams = parse_params(params)?;
    let dl = deadline(ctx, p.timeout_secs)?;
    let browser = resolve_browser(ctx, p.cdp_endpoint.as_deref(), p.launch, p.headless, p.no_sandbox, &dl)?;
    let targets = cdp::json_list(&browser.endpoint, &dl)?;
    let rows: Vec<Value> = targets.iter().take(2000).map(target_row).collect();
    let pages = rows.iter().filter(|r| r["type"] == "page").count();
    let summary = format!("{} target(s) at {} ({} page(s)); {} {}.", rows.len(), browser.endpoint.origin, pages, browser.product, browser.version);
    let ev = EvidenceBuilder::new(
        "web_pages_list",
        Subject { kind: "browser".into(), sha256: None, name: Some(browser.endpoint.origin.clone()) },
        browser_env(ctx.session.clean_room(), &browser),
    )
    .params(json!({"cdp_endpoint": p.cdp_endpoint}))
    .result(json!({"endpoint": browser.endpoint.origin, "product": browser.product, "protocol_version": browser.version, "targets": rows}))
    .summary(summary)
    .confidence(Confidence::Observed)
    .authority(Authority::OutsideService)
    .coverage(Coverage::complete(rows.len() as u64))
    .location(Location::new("url", browser.endpoint.origin.clone()))
    .build()?;
    Ok(ToolOutput::evidence(ev))
}

// ---------------------------------------------------------------------------------------------
// Shared passive page capture

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct CaptureFlags {
    pub(crate) accessibility_text: bool,
    pub(crate) console_text: bool,
    pub(crate) body_outlines: bool,
    pub(crate) websocket_outlines: bool,
    pub(crate) storage_keys: bool,
    pub(crate) script_sources: bool,
    pub(crate) console: bool,
    pub(crate) network: bool,
}

/// A script seen on the page, with optional source text.
pub(crate) struct CapturedScript {
    pub(crate) url: String,
    pub(crate) text: Option<String>,
    pub(crate) length: Option<u64>,
    pub(crate) source_map_url: Option<String>,
}

pub(crate) struct PageCapture {
    pub(crate) result: Value,
    pub(crate) scripts: Vec<CapturedScript>,
    pub(crate) dropped: u64,
}

const SCRIPT_RECORD_CAP: usize = 50_000;
const SCRIPT_META_BUDGET: usize = 8 << 20;
const SOURCE_TEXT_BUDGET: usize = 32 << 20;
const NETWORK_RECORD_CAP: usize = 20_000;

/// Enable the needed domains, observe for `watch_ms`, and build a data-minimised capture.
pub(crate) fn capture_page(
    client: &mut CdpClient,
    origin_label: &str,
    flags: CaptureFlags,
    watch_ms: u64,
    secrets: &mut Secrets,
    ctx: &Ctx<'_>,
    dl: &Deadline,
) -> Result<PageCapture, ToolError> {
    // Enable domains. Network/Console/Debugger replay existing state as events.
    let _ = client.call("Page.enable", json!({}), None, dl)?;
    let _ = client.call("DOM.enable", json!({}), None, dl);
    let _ = client.call("Runtime.enable", json!({}), None, dl);
    if flags.network {
        let _ = client.call("Network.enable", json!({"maxTotalBufferSize": 16 << 20, "maxResourceBufferSize": 8 << 20}), None, dl);
    }
    if flags.console {
        let _ = client.call("Log.enable", json!({}), None, dl);
    }
    // Debugger.enable replays scriptParsed for every existing script.
    let _ = client.call("Debugger.enable", json!({"maxScriptsCacheSize": SCRIPT_META_BUDGET}), None, dl);

    // Observe.
    let window = window_deadline(ctx, watch_ms, dl);
    client.pump_until(&window)?;
    let events = client.take_events();

    // Frame tree → origins.
    let frames = client.call("Page.getFrameTree", json!({}), None, dl).unwrap_or(Value::Null);
    let mut origins: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    collect_frame_origins(frames.get("frameTree"), &mut origins);

    // DOM summary.
    let dom = match client.call("DOM.getDocument", json!({"depth": -1, "pierce": true}), None, dl) {
        Ok(d) => web_common::dom_summary(d.get("root").unwrap_or(&Value::Null), 200_000),
        Err(_) => Value::Null,
    };

    // Resources.
    let resource_tree = client.call("Page.getResourceTree", json!({}), None, dl).unwrap_or(Value::Null);
    let resources = collect_resources(&resource_tree);

    // Scripts from scriptParsed events (+ optional sources).
    let (scripts, scripts_json, mut dropped, scripts_truncated) = collect_scripts(client, &events, flags.script_sources, dl);

    // Network from events (+ optional JSON body outlines).
    let network =
        if flags.network { collect_network(client, &events, flags.body_outlines, NetworkContent::default(), secrets, dl, &mut dropped) } else { Value::Null };

    // WebSocket outlines.
    let websockets = if flags.websocket_outlines { collect_websockets(&events, secrets) } else { Value::Null };

    // Console.
    let console = if flags.console { collect_console(&events, flags.console_text, secrets) } else { Value::Null };

    // Storage KEYS only.
    let storage = if flags.storage_keys { collect_storage_keys(client, &origins, dl) } else { Value::Null };

    // Accessibility names (opt-in).
    let accessibility = if flags.accessibility_text { collect_accessibility(client, dl) } else { Value::Null };

    dropped = dropped.saturating_add(client.dropped_events());

    let result = json!({
        "origin": origin_label,
        "frame_origins": origins.into_iter().collect::<Vec<_>>(),
        "dom": dom,
        "resources": resources,
        "scripts": scripts_json,
        "scripts_truncated": scripts_truncated,
        "network": network,
        "websockets": websockets,
        "console": console,
        "storage_keys": storage,
        "accessibility": accessibility,
        "watch_ms": watch_ms,
        "dropped_events": dropped,
        "secret_redactions": secrets.redactions,
    });
    Ok(PageCapture { result, scripts, dropped })
}

fn collect_frame_origins(frame_tree: Option<&Value>, out: &mut std::collections::BTreeSet<String>) {
    collect_frame_origins_at(frame_tree, out, 0);
}

fn collect_frame_origins_at(frame_tree: Option<&Value>, out: &mut std::collections::BTreeSet<String>, depth: usize) {
    let Some(ft) = frame_tree else { return };
    if depth > 256 {
        return;
    }
    if let Some(url) = ft.get("frame").and_then(|f| f.get("url")).and_then(Value::as_str) {
        out.insert(origin_of(&web_common::sanitize_url(url)));
    }
    for child in ft.get("childFrames").and_then(Value::as_array).into_iter().flatten() {
        if out.len() >= 1000 {
            break;
        }
        collect_frame_origins_at(Some(child), out, depth + 1);
    }
}

fn origin_of(url: &str) -> String {
    if let Some(i) = url.find("://") {
        let rest = url.get(i + 3..).unwrap_or("");
        let host = rest.split(['/', '?', '#']).next().unwrap_or(rest);
        return format!("{}{host}", url.get(..i + 3).unwrap_or_default());
    }
    url.split(['/', '?', '#']).next().unwrap_or(url).to_string()
}

fn collect_resources(tree: &Value) -> Vec<Value> {
    let mut out = Vec::new();
    fn walk(node: &Value, out: &mut Vec<Value>, depth: usize) {
        if out.len() >= 5000 || depth > 256 {
            return;
        }
        if let Some(res) = node.get("resources").and_then(Value::as_array) {
            for r in res {
                if out.len() >= 5000 {
                    break;
                }
                out.push(json!({
                    "url": web_common::sanitize_url(r.get("url").and_then(Value::as_str).unwrap_or_default()),
                    "type": r.get("type").and_then(Value::as_str).unwrap_or_default(),
                    "mime_type": r.get("mimeType").and_then(Value::as_str).unwrap_or_default(),
                }));
            }
        }
        for child in node.get("childFrames").and_then(Value::as_array).into_iter().flatten() {
            walk(child, out, depth + 1);
        }
    }
    if let Some(root) = tree.get("frameTree") {
        walk(root, &mut out, 0);
    }
    out
}

fn collect_scripts(client: &mut CdpClient, events: &[cdp::CdpEvent], want_sources: bool, dl: &Deadline) -> (Vec<CapturedScript>, Vec<Value>, u64, bool) {
    let mut scripts: Vec<CapturedScript> = Vec::new();
    let mut json_rows: Vec<Value> = Vec::new();
    let mut dropped = 0u64;
    let mut meta_bytes = 0usize;
    let mut source_bytes = 0usize;
    let mut truncated = false;
    let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for ev in events.iter().filter(|e| e.method == "Debugger.scriptParsed") {
        let url = ev.params.get("url").and_then(Value::as_str).unwrap_or_default().to_string();
        if url.is_empty() || !seen.insert(url.clone()) {
            continue;
        }
        if scripts.len() >= SCRIPT_RECORD_CAP || meta_bytes >= SCRIPT_META_BUDGET {
            truncated = true;
            dropped += 1;
            continue;
        }
        meta_bytes = meta_bytes.saturating_add(url.len() + 128);
        let length = ev.params.get("length").and_then(Value::as_u64);
        let smu = ev.params.get("sourceMapURL").and_then(Value::as_str).filter(|s| !s.is_empty()).map(String::from);
        let mut text = None;
        if want_sources
            && source_bytes < SOURCE_TEXT_BUDGET
            && let Some(sid) = ev.params.get("scriptId").and_then(Value::as_str)
            && let Ok(src) = client.call("Debugger.getScriptSource", json!({"scriptId": sid}), None, dl)
            && let Some(s) = src.get("scriptSource").and_then(Value::as_str)
        {
            source_bytes = source_bytes.saturating_add(s.len());
            if source_bytes <= SOURCE_TEXT_BUDGET {
                text = Some(s.to_string());
            } else {
                truncated = true;
            }
        }
        let san = web_common::sanitize_url(&url);
        let mut row = json!({"url": san, "length": length, "source_map_url": smu});
        if let Some(t) = &text {
            // A WebTextArtifact that carries its own hash: `text` + `sha256` over it, with `url` alongside so
            // web_scripts_save and js_app_map can consume this capture.
            row["media_type"] = json!("application/javascript");
            row["charset"] = json!("utf-8");
            row["bytes"] = json!(t.len());
            row["sha256"] = json!(craft_anything_evidence::sha256_hex(t.as_bytes()));
            row["text"] = json!(t);
        }
        json_rows.push(row);
        scripts.push(CapturedScript { url, text, length, source_map_url: smu });
    }
    (scripts, json_rows, dropped, truncated)
}

pub(crate) fn collect_network(
    client: &mut CdpClient,
    events: &[cdp::CdpEvent],
    want_body_outlines: bool,
    content: NetworkContent,
    secrets: &mut Secrets,
    dl: &Deadline,
    dropped: &mut u64,
) -> Value {
    use std::collections::BTreeMap;
    let mut reqs: BTreeMap<String, Value> = BTreeMap::new();
    let mut order: Vec<String> = Vec::new();
    for ev in events {
        match ev.method.as_str() {
            "Network.requestWillBeSent" => {
                let id = ev.params.get("requestId").and_then(Value::as_str).unwrap_or_default().to_string();
                if id.is_empty() {
                    continue;
                }
                if !reqs.contains_key(&id) {
                    if reqs.len() >= NETWORK_RECORD_CAP {
                        *dropped += 1;
                        continue;
                    }
                    order.push(id.clone());
                }
                let r = ev.params.get("request");
                let url = secrets.text(&web_common::sanitize_url(r.and_then(|r| r.get("url")).and_then(Value::as_str).unwrap_or_default()));
                let method = r.and_then(|r| r.get("method")).and_then(Value::as_str).unwrap_or_default().to_string();
                let initiator = ev.params.get("initiator").and_then(|i| i.get("type")).and_then(Value::as_str).unwrap_or_default().to_string();
                let entry = reqs.entry(id).or_insert(json!({"method": method, "url": url, "initiator": initiator, "type": ev.params.get("type")}));
                if content.header_values {
                    entry["request_headers"] = web_common::header_values(r.and_then(|r| r.get("headers")).unwrap_or(&Value::Null), secrets);
                }
                if content.request_body {
                    match r.and_then(|r| r.get("postData")).and_then(Value::as_str) {
                        Some(body) => entry["request_body"] = web_common::kept_body(body.as_bytes(), secrets),
                        None if r.and_then(|r| r.get("hasPostData")).and_then(Value::as_bool) == Some(true) => {
                            entry["request_body"] = json!({"state": "not_exposed"});
                        }
                        None => {}
                    }
                }
            }
            "Network.responseReceived" => {
                let id = ev.params.get("requestId").and_then(Value::as_str).unwrap_or_default().to_string();
                if let Some(entry) = reqs.get_mut(&id) {
                    let resp = ev.params.get("response");
                    entry["status"] = resp.and_then(|r| r.get("status")).cloned().unwrap_or(Value::Null);
                    entry["mime_type"] = resp.and_then(|r| r.get("mimeType")).cloned().unwrap_or(Value::Null);
                    entry["response_headers"] = web_common::project_headers(resp.and_then(|r| r.get("headers")).unwrap_or(&Value::Null));
                    if content.header_values {
                        entry["response_header_values"] = web_common::header_values(resp.and_then(|r| r.get("headers")).unwrap_or(&Value::Null), secrets);
                    }
                }
            }
            _ => {}
        }
    }
    // Optional JSON body outlines for JSON responses.
    if want_body_outlines {
        let ids: Vec<String> = order.iter().take(200).cloned().collect();
        for id in ids {
            if dl.expired() {
                break;
            }
            let is_json = reqs.get(&id).and_then(|e| e.get("mime_type")).and_then(Value::as_str).is_some_and(|m| m.contains("json"));
            if !is_json {
                continue;
            }
            if let Ok(body) = client.call("Network.getResponseBody", json!({"requestId": id}), None, dl)
                && let Some(text) = body.get("body").and_then(Value::as_str)
            {
                let decoded = if body.get("base64Encoded").and_then(Value::as_bool) == Some(true) {
                    web_common::b64_decode(text).ok().and_then(|b| String::from_utf8(b).ok())
                } else {
                    Some(text.to_string())
                };
                if let Some(t) = decoded
                    && let Ok(v) = serde_json::from_str::<Value>(&t)
                    && let Some(entry) = reqs.get_mut(&id)
                {
                    entry["json_body_outline"] = web_common::json_outline(&v);
                }
            }
        }
    }
    if content.response_body {
        let ids: Vec<String> = order.iter().take(200).cloned().collect();
        for id in ids {
            if dl.expired() {
                break;
            }
            let kept = match client.call("Network.getResponseBody", json!({"requestId": id}), None, dl) {
                Ok(body) => {
                    let text = body.get("body").and_then(Value::as_str).unwrap_or_default();
                    let raw = if body.get("base64Encoded").and_then(Value::as_bool) == Some(true) {
                        web_common::b64_decode(text).unwrap_or_default()
                    } else {
                        text.as_bytes().to_vec()
                    };
                    web_common::kept_body(&raw, secrets)
                }
                Err(e) => json!({"state": "unavailable", "reason": e.message}),
            };
            if let Some(entry) = reqs.get_mut(&id) {
                entry["response_body"] = kept;
            }
        }
    }
    let items: Vec<Value> = order.iter().filter_map(|id| reqs.get(id).cloned()).collect();
    json!({"request_count": items.len(), "requests": items})
}

fn collect_websockets(events: &[cdp::CdpEvent], secrets: &mut Secrets) -> Value {
    let mut conns: std::collections::BTreeMap<String, Value> = std::collections::BTreeMap::new();
    let mut frames: Vec<Value> = Vec::new();
    for ev in events {
        match ev.method.as_str() {
            "Network.webSocketCreated" => {
                let id = ev.params.get("requestId").and_then(Value::as_str).unwrap_or_default().to_string();
                let url = secrets.text(&web_common::sanitize_url(ev.params.get("url").and_then(Value::as_str).unwrap_or_default()));
                conns.insert(id, json!({"url": url}));
            }
            "Network.webSocketFrameSent" | "Network.webSocketFrameReceived" => {
                if frames.len() >= 5000 {
                    continue;
                }
                let direction = if ev.method.ends_with("Sent") { "sent" } else { "received" };
                let f = ev.params.get("response");
                let opcode = f.and_then(|r| r.get("opcode")).cloned().unwrap_or(Value::Null);
                let len = f.and_then(|r| r.get("payloadData")).and_then(Value::as_str).map(str::len).unwrap_or(0);
                frames.push(json!({"direction": direction, "opcode": opcode, "payload_length": len}));
            }
            _ => {}
        }
    }
    json!({"connections": conns.into_values().collect::<Vec<_>>(), "frames": frames})
}

fn collect_console(events: &[cdp::CdpEvent], want_text: bool, secrets: &mut Secrets) -> Value {
    let mut items: Vec<Value> = Vec::new();
    for ev in events {
        if items.len() >= 5000 {
            break;
        }
        match ev.method.as_str() {
            "Runtime.consoleAPICalled" => {
                let kind = ev.params.get("type").and_then(Value::as_str).unwrap_or_default();
                let mut row = json!({"source": "console-api", "level": kind});
                if want_text && let Some(args) = ev.params.get("args").and_then(Value::as_array) {
                    let text: String = args.iter().filter_map(|a| a.get("value").and_then(Value::as_str)).collect::<Vec<_>>().join(" ");
                    row["text"] = json!(secrets.text(&text));
                }
                items.push(row);
            }
            "Log.entryAdded" => {
                let entry = ev.params.get("entry");
                let mut row = json!({"source": entry.and_then(|e| e.get("source")).cloned().unwrap_or(Value::Null), "level": entry.and_then(|e| e.get("level")).cloned().unwrap_or(Value::Null)});
                if want_text && let Some(t) = entry.and_then(|e| e.get("text")).and_then(Value::as_str) {
                    row["text"] = json!(secrets.text(t));
                }
                items.push(row);
            }
            _ => {}
        }
    }
    json!({"count": items.len(), "entries": items, "has_text": want_text})
}

fn collect_storage_keys(client: &mut CdpClient, origins: &std::collections::BTreeSet<String>, dl: &Deadline) -> Value {
    let _ = client.call("DOMStorage.enable", json!({}), None, dl);
    let mut out: Vec<Value> = Vec::new();
    for origin in origins.iter().filter(|o| o.starts_with("http")).take(50) {
        for is_local in [true, false] {
            if dl.expired() {
                break;
            }
            let sid = json!({"securityOrigin": origin, "isLocalStorage": is_local});
            if let Ok(items) = client.call("DOMStorage.getDOMStorageItems", json!({"storageId": sid}), None, dl) {
                let keys: Vec<String> = items
                    .get("entries")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|e| e.as_array().and_then(|kv| kv.first()).and_then(Value::as_str).map(String::from))
                    .take(5000)
                    .collect();
                if !keys.is_empty() {
                    out.push(json!({"origin": origin, "area": if is_local { "localStorage" } else { "sessionStorage" }, "keys": keys}));
                }
            }
        }
    }
    json!({"areas": out, "note": "key names only; values are never read"})
}

fn collect_accessibility(client: &mut CdpClient, dl: &Deadline) -> Value {
    let _ = client.call("Accessibility.enable", json!({}), None, dl);
    match client.call("Accessibility.getFullAXTree", json!({}), None, dl) {
        Ok(tree) => {
            let nodes: Vec<Value> = tree
                .get("nodes")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .take(5000)
                .map(|n| {
                    json!({
                        "role": n.get("role").and_then(|r| r.get("value")).cloned().unwrap_or(Value::Null),
                        "name": n.get("name").and_then(|r| r.get("value")).cloned().unwrap_or(Value::Null),
                    })
                })
                .collect();
            json!({"nodes": nodes})
        }
        Err(_) => json!({"available": false}),
    }
}

// ---------------------------------------------------------------------------------------------
// web_page_show

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InspectPageParams {
    #[serde(default)]
    pub cdp_endpoint: Option<String>,
    #[serde(default)]
    pub launch: bool,
    #[serde(default = "yes")]
    pub headless: bool,
    #[serde(default)]
    pub no_sandbox: bool,
    /// The target id to inspect (from web_pages_list); the first page otherwise.
    #[serde(default)]
    pub target_id: Option<String>,
    /// Passive observation window in milliseconds.
    #[serde(default = "d_inspect_window")]
    pub watch_ms: u64,
    #[serde(default)]
    pub with_accessibility_text: bool,
    #[serde(default)]
    pub with_console_text: bool,
    #[serde(default)]
    pub with_body_outlines: bool,
    #[serde(default)]
    pub with_websocket_outlines: bool,
    #[serde(default)]
    pub with_storage_names: bool,
    #[serde(default)]
    pub with_script_text: bool,
    /// Values to redact wherever they appear (any encoding). Only the count is recorded.
    #[serde(default)]
    pub hidden_values: Vec<String>,
    #[serde(default = "d_timeout")]
    pub timeout_secs: u64,
}

fn d_inspect_window() -> u64 {
    500
}

fn web_page_show(ctx: &mut Ctx<'_>, params: Value) -> Result<ToolOutput, ToolError> {
    let p: InspectPageParams = parse_params(params)?;
    let dl = deadline(ctx, p.timeout_secs)?;
    let browser = resolve_browser(ctx, p.cdp_endpoint.as_deref(), p.launch, p.headless, p.no_sandbox, &dl)?;
    let targets = cdp::json_list(&browser.endpoint, &dl)?;
    let target = web_common::pick_target(&targets, p.target_id.as_deref(), Some("page"))?;
    let page_url = web_common::sanitize_url(target.get("url").and_then(Value::as_str).unwrap_or_default());
    let mut client = web_common::connect_target(&browser, target, PAGE_MAX_MESSAGE, &dl)?;
    let flags = CaptureFlags {
        accessibility_text: p.with_accessibility_text,
        console_text: p.with_console_text,
        body_outlines: p.with_body_outlines,
        websocket_outlines: p.with_websocket_outlines,
        storage_keys: p.with_storage_names,
        script_sources: p.with_script_text,
        console: true,
        network: true,
    };
    let mut secrets = Secrets::new(&p.hidden_values);
    let cap = capture_page(&mut client, &page_url, flags, p.watch_ms, &mut secrets, ctx, &dl);
    cleanup(&mut client, &dl);
    let cap = cap?;
    let scripts_n = cap.result["scripts"].as_array().map_or(0, Vec::len);
    let req_n = cap.result["network"]["request_count"].as_u64().unwrap_or(0);
    let summary = format!(
        "Inspected {page_url}: {scripts_n} script(s), {req_n} request outline(s), {} frame origin(s).",
        cap.result["frame_origins"].as_array().map_or(0, Vec::len)
    );
    let ev = passive_evidence(
        "web_page_show",
        ctx.session.clean_room(),
        &browser,
        &page_url,
        cap.result,
        summary,
        cap.dropped,
        p.hidden_values.len(),
        secrets.redactions,
    )?;
    Ok(ToolOutput::evidence(ev))
}

/// Disable the enabled domains and close our own socket, each bounded by a short fresh deadline so
/// cleanup can never hang even if the caller's deadline has already passed.
pub(crate) fn cleanup(client: &mut CdpClient, _dl: &Deadline) {
    let quick = Deadline::new(Duration::from_secs(5), crate::tool::CancelToken::default());
    for d in ["Network.disable", "Debugger.disable", "Runtime.disable", "Log.disable", "DOM.disable", "Page.disable"] {
        let _ = client.call(d, json!({}), None, &quick);
    }
    client.close(&quick);
}

#[allow(clippy::too_many_arguments)]
fn passive_evidence(
    op: &'static str,
    clean_room: bool,
    browser: &Browser,
    origin: &str,
    result: Value,
    summary: String,
    dropped: u64,
    declared_secrets: usize,
    redactions: u64,
) -> Result<craft_anything_evidence::Evidence, ToolError> {
    let mut b = EvidenceBuilder::new(op, Subject { kind: "website".into(), sha256: None, name: Some(origin.to_string()) }, browser_env(clean_room, browser))
        .params(json!({"endpoint": browser.endpoint.origin, "sensitive_value_declarations": declared_secrets}))
        .result(result)
        .summary(summary)
        .confidence(Confidence::Observed)
        .authority(Authority::EarlierRecord)
        .limitation(Limitation::new(
            "passive_snapshot",
            "A passive observation of one moment in a live page; it shows what happened during the window, not everything the page can do.",
        ))
        .limitation(Limitation::new(
            "values_not_kept",
            "Cookies, authorization headers and storage values are never kept; JSON bodies are reduced to outlines and declared secrets are redacted.",
        ))
        .coverage(Coverage { status: CoverageStatus::Partial, examined: None, total: None, next_offset: None })
        .location(Location::new("url", origin.to_string()));
    if dropped > 0 {
        b = b.limitation(Limitation::new("events_dropped", format!("{dropped} event(s) were dropped against the capture budget")));
    }
    if declared_secrets > 0 {
        b = b.limitation(Limitation::new("secrets_redacted", format!("{redactions} occurrence(s) of {declared_secrets} declared value(s) were redacted")));
    }
    b.build()
}

// ---------------------------------------------------------------------------------------------
// web_page_js_analyze

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AnalyzeBundleParams {
    #[serde(default)]
    pub cdp_endpoint: Option<String>,
    #[serde(default)]
    pub launch: bool,
    #[serde(default = "yes")]
    pub headless: bool,
    #[serde(default)]
    pub no_sandbox: bool,
    #[serde(default)]
    pub target_id: Option<String>,
    #[serde(default = "d_inspect_window")]
    pub watch_ms: u64,
    /// Decode inline (data:) source maps and fetch same-origin maps.
    #[serde(default)]
    pub load_source_maps: bool,
    /// First module of the page in the result.
    #[serde(default)]
    pub offset: usize,
    #[serde(default = "d_module_limit")]
    pub limit: usize,
    #[serde(default = "d_timeout")]
    pub timeout_secs: u64,
}

fn d_module_limit() -> usize {
    500
}

fn web_page_js_analyze(ctx: &mut Ctx<'_>, params: Value) -> Result<ToolOutput, ToolError> {
    let p: AnalyzeBundleParams = parse_params(params)?;
    let dl = deadline(ctx, p.timeout_secs)?;
    let browser = resolve_browser(ctx, p.cdp_endpoint.as_deref(), p.launch, p.headless, p.no_sandbox, &dl)?;
    let targets = cdp::json_list(&browser.endpoint, &dl)?;
    let target = web_common::pick_target(&targets, p.target_id.as_deref(), Some("page"))?;
    let page_url = web_common::sanitize_url(target.get("url").and_then(Value::as_str).unwrap_or_default());
    let mut client = web_common::connect_target(&browser, target, PAGE_MAX_MESSAGE, &dl)?;
    let flags = CaptureFlags { script_sources: true, network: false, console: false, ..CaptureFlags::default() };
    let mut secrets = Secrets::new(&[]);
    let cap = capture_page(&mut client, &page_url, flags, p.watch_ms, &mut secrets, ctx, &dl);
    cleanup(&mut client, &dl);
    let cap = cap?;

    // Build an AppInput from the fetched script sources and reuse the built-in JS analyser.
    use craft_anything_analysis::js_app::{AppFile, AppInput, AppOptions, analyze_application};
    let mut input = AppInput { label: page_url.clone(), format: "web".into(), ..Default::default() };
    let mut source_maps: Vec<Value> = Vec::new();
    for s in &cap.scripts {
        let path = script_path(&s.url);
        if let Some(text) = &s.text {
            input.listing.push((path.clone(), text.len() as u64));
            input.files.push(AppFile { path: path.clone(), bytes: text.clone().into_bytes() });
            if p.load_source_maps
                && let Some(smu) = &s.source_map_url
            {
                source_maps.push(resolve_source_map(&browser, smu, &s.url, &dl));
            }
        } else {
            input.listing.push((path, s.length.unwrap_or(0)));
        }
    }
    if input.files.is_empty() {
        return Err(ToolError::new(craft_anything_evidence::ErrorCode::FeatureMissing, "the page exposed no readable script sources over CDP")
            .with_remediation("Some pages block source access; try web_page_show, or export and analyse the scripts offline."));
    }
    let cancel = ctx.cancel.clone();
    let check = move || cancel.is_cancelled();
    let graph = analyze_application(&input, &AppOptions::default(), &check).map_err(|_| ToolError::cancelled())?;
    let bundlers = graph.bundlers.clone();
    let total_modules = graph.modules.len();
    let endpoints = graph.endpoints.len();
    let routes = graph.routes.len();
    let mut result = serde_json::to_value(&graph).map_err(|e| ToolError::internal(e.to_string()))?;
    if let Some(o) = result.as_object_mut() {
        let page: Vec<Value> = graph.modules.iter().skip(p.offset).take(p.limit.max(1)).filter_map(|m| serde_json::to_value(m).ok()).collect();
        o.insert("modules".into(), json!(page));
        o.insert("modules_total".into(), json!(total_modules));
        o.insert("origin".into(), json!(page_url));
        o.insert("source_maps".into(), json!(source_maps));
        o.insert("scripts_seen".into(), json!(cap.scripts.len()));
    }
    let summary = format!(
        "Analysed {} script(s) from {page_url}: {total_modules} module(s), {endpoints} endpoint(s), {routes} route(s){}.",
        cap.scripts.len(),
        if bundlers.is_empty() { String::new() } else { format!(", bundled with {}", bundlers.join("/")) }
    );
    let ev = EvidenceBuilder::new(
        "web_page_js_analyze",
        Subject { kind: "website".into(), sha256: None, name: Some(page_url.clone()) },
        browser_env(ctx.session.clean_room(), &browser),
    )
    .params(json!({"endpoint": browser.endpoint.origin, "load_source_maps": p.load_source_maps, "offset": p.offset, "limit": p.limit}))
    .result(result)
    .summary(summary)
    .confidence(Confidence::Derived)
    .authority(Authority::EarlierRecord)
    .limitation(Limitation::new(
        "live_subset",
        "Only the scripts this live page had loaded during the window were analysed; lazily loaded chunks may be missing.",
    ))
    .limitation(Limitation::new("static_only", "Static reading only: code built at run time (eval, computed imports) is invisible."))
    .coverage(if p.offset == 0 && p.limit >= total_modules {
        Coverage::complete(total_modules as u64)
    } else {
        Coverage::page(p.limit.min(total_modules.saturating_sub(p.offset)) as u64, total_modules as u64, p.offset as u64)
    })
    .location(Location::new("url", page_url))
    .build()?;
    Ok(ToolOutput::evidence(ev))
}

fn script_path(url: &str) -> String {
    let san = web_common::sanitize_url(url);
    if let Some(i) = san.find("://") {
        let rest = san.get(i + 3..).unwrap_or("");
        let no_query = rest.split(['?', '#']).next().unwrap_or(rest);
        let cleaned = no_query.trim_end_matches('/');
        if cleaned.is_empty() { "index".into() } else { cleaned.to_string() }
    } else {
        san
    }
}

/// Resolve a declared source-map URL: decode inline data: maps; fetch maps on the script's own
/// loopback origin (literal 127.0.0.1 or [::1] with a port); record others unfetched.
fn resolve_source_map(_browser: &Browser, declared: &str, script_url: &str, dl: &Deadline) -> Value {
    if declared.starts_with("data:") {
        match craft_anything_formats::sourcemap::decode_data_url(declared) {
            Some(bytes) => {
                json!({"declared_url": "inline data: URL", "effective_url": "inline", "fetched": true, "bytes": bytes.len(), "script": web_common::sanitize_url(script_url)})
            }
            None => json!({"declared_url": "inline data: URL", "fetched": false, "reason": "not valid base64", "script": web_common::sanitize_url(script_url)}),
        }
    } else if let Some((ep, path)) = same_loopback_origin_map(declared, script_url) {
        let effective = web_common::sanitize_url(&format!("{}{path}", ep.origin));
        match cdp::http_get(&ep, &path, dl) {
            Ok((status, bytes)) if (200..300).contains(&status) => {
                json!({"declared_url": web_common::sanitize_url(declared), "effective_url": effective, "fetched": true, "bytes": bytes.len(), "script": web_common::sanitize_url(script_url)})
            }
            Ok((status, _)) => {
                json!({"declared_url": web_common::sanitize_url(declared), "effective_url": effective, "fetched": false, "reason": format!("the page's server answered HTTP {status}"), "script": web_common::sanitize_url(script_url)})
            }
            Err(e) => {
                json!({"declared_url": web_common::sanitize_url(declared), "effective_url": effective, "fetched": false, "reason": e.message, "script": web_common::sanitize_url(script_url)})
            }
        }
    } else {
        json!({"declared_url": web_common::sanitize_url(declared), "fetched": false, "reason": "cross-origin or remote maps are not fetched; download and use js_source_locate", "script": web_common::sanitize_url(script_url)})
    }
}

/// Split an `http://host:port/path` URL into its loopback endpoint and path (query kept, fragment
/// dropped). `None` unless the host is literal loopback with an explicit port.
fn loopback_origin_and_path(url: &str) -> Option<(cdp::Endpoint, String)> {
    let rest = url.strip_prefix("http://")?;
    let (authority, path) = match rest.find(['/', '?', '#']) {
        Some(i) => (rest.get(..i)?, rest.get(i..)?),
        None => (rest, ""),
    };
    let ep = cdp::parse_endpoint(&format!("http://{authority}")).ok()?;
    let path = path.split('#').next().unwrap_or("");
    let path = if path.starts_with('/') { path.to_string() } else { format!("/{path}") };
    Some((ep, path))
}

/// Resolve a declared source-map URL against its script and return the endpoint and request path
/// only when the map lives on the script's own loopback origin.
fn same_loopback_origin_map(declared: &str, script_url: &str) -> Option<(cdp::Endpoint, String)> {
    let declared = declared.trim();
    if declared.is_empty() || declared.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return None;
    }
    let (script_ep, script_path) = loopback_origin_and_path(script_url)?;
    let lower = declared.to_ascii_lowercase();
    let absolute = if lower.starts_with("http://") {
        declared.to_string()
    } else if declared.starts_with("//") {
        format!("http:{declared}")
    } else if declared.starts_with('/') {
        format!("{}{declared}", script_ep.origin)
    } else if declared.split(['/', '?', '#']).next().is_some_and(|first| first.contains(':')) {
        // Another scheme (https:, file:, blob:, …).
        return None;
    } else {
        let dir = script_path.split('?').next().unwrap_or("");
        let dir = dir.get(..dir.rfind('/').map_or(0, |i| i + 1)).unwrap_or("/");
        format!("{}{dir}{declared}", script_ep.origin)
    };
    let (ep, path) = loopback_origin_and_path(&absolute)?;
    (ep == script_ep).then_some((ep, path))
}

// ---------------------------------------------------------------------------------------------
// web_page_watch

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ObserveSessionParams {
    #[serde(default)]
    pub cdp_endpoint: Option<String>,
    #[serde(default)]
    pub launch: bool,
    #[serde(default = "yes")]
    pub headless: bool,
    #[serde(default)]
    pub no_sandbox: bool,
    #[serde(default)]
    pub target_id: Option<String>,
    /// Observation window in milliseconds (default 10s).
    #[serde(default = "d_session_window")]
    pub watch_ms: u64,
    #[serde(default)]
    pub with_websocket_outlines: bool,
    #[serde(default)]
    pub hidden_values: Vec<String>,
    #[serde(default = "d_session_timeout")]
    pub timeout_secs: u64,
}

fn d_session_window() -> u64 {
    10_000
}
fn d_session_timeout() -> u64 {
    120
}

fn web_page_watch(ctx: &mut Ctx<'_>, params: Value) -> Result<ToolOutput, ToolError> {
    let p: ObserveSessionParams = parse_params(params)?;
    let dl = deadline(ctx, p.timeout_secs)?;
    let browser = resolve_browser(ctx, p.cdp_endpoint.as_deref(), p.launch, p.headless, p.no_sandbox, &dl)?;
    let targets = cdp::json_list(&browser.endpoint, &dl)?;
    let target = web_common::pick_target(&targets, p.target_id.as_deref(), Some("page"))?;
    let page_url = web_common::sanitize_url(target.get("url").and_then(Value::as_str).unwrap_or_default());
    let mut client = web_common::connect_target(&browser, target, PAGE_MAX_MESSAGE, &dl)?;
    let flags = CaptureFlags { network: true, console: true, websocket_outlines: p.with_websocket_outlines, ..CaptureFlags::default() };
    let mut secrets = Secrets::new(&p.hidden_values);
    let cap = capture_page(&mut client, &page_url, flags, p.watch_ms, &mut secrets, ctx, &dl);
    cleanup(&mut client, &dl);
    let cap = cap?;
    let req_n = cap.result["network"]["request_count"].as_u64().unwrap_or(0);
    let con_n = cap.result["console"]["count"].as_u64().unwrap_or(0);
    let summary = format!("Watched {page_url} for {}ms: {req_n} request outline(s), {con_n} console entr(y/ies).", p.watch_ms);
    let ev = passive_evidence(
        "web_page_watch",
        ctx.session.clean_room(),
        &browser,
        &page_url,
        cap.result,
        summary,
        cap.dropped,
        p.hidden_values.len(),
        secrets.redactions,
    )?;
    Ok(ToolOutput::evidence(ev))
}

// ---------------------------------------------------------------------------------------------
// web_mcp_tools_list

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DiscoverWebmcpParams {
    #[serde(default)]
    pub cdp_endpoint: Option<String>,
    #[serde(default)]
    pub launch: bool,
    #[serde(default = "yes")]
    pub headless: bool,
    #[serde(default)]
    pub no_sandbox: bool,
    #[serde(default)]
    pub target_id: Option<String>,
    #[serde(default = "d_webmcp_window")]
    pub watch_ms: u64,
    #[serde(default = "d_timeout")]
    pub timeout_secs: u64,
}

fn d_webmcp_window() -> u64 {
    100
}

fn web_mcp_tools_list(ctx: &mut Ctx<'_>, params: Value) -> Result<ToolOutput, ToolError> {
    let p: DiscoverWebmcpParams = parse_params(params)?;
    let dl = deadline(ctx, p.timeout_secs)?;
    let browser = resolve_browser(ctx, p.cdp_endpoint.as_deref(), p.launch, p.headless, p.no_sandbox, &dl)?;
    let targets = cdp::json_list(&browser.endpoint, &dl)?;
    let target = web_common::pick_target(&targets, p.target_id.as_deref(), Some("page"))?;
    let page_url = web_common::sanitize_url(target.get("url").and_then(Value::as_str).unwrap_or_default());
    let mut client = web_common::connect_target(&browser, target, PAGE_MAX_MESSAGE, &dl)?;
    let supported = client.call("WebMCP.enable", json!({}), None, &dl);
    let (available, tools, note) = match supported {
        Ok(_) => {
            let window = window_deadline(ctx, p.watch_ms, &dl);
            let _ = client.pump_until(&window);
            let events = client.take_events();
            let mut tools: Vec<Value> = Vec::new();
            for ev in events.iter().filter(|e| e.method == "WebMCP.toolsAdded") {
                for t in ev.params.get("tools").and_then(Value::as_array).into_iter().flatten().take(500) {
                    tools.push(json!({"name": t.get("name").and_then(Value::as_str).unwrap_or_default(), "input_outline": t.get("inputSchema").map(web_common::json_outline).unwrap_or(Value::Null)}));
                }
            }
            (true, tools, "tools are advertised only; none were invoked")
        }
        Err(e) if e.code == craft_anything_evidence::ErrorCode::FeatureMissing => {
            (false, Vec::new(), "this browser does not implement the experimental WebMCP CDP domain")
        }
        Err(e) => return cleanup_then_err(&mut client, &dl, e),
    };
    cleanup(&mut client, &dl);
    let summary = if available {
        format!("WebMCP on {page_url}: {} tool(s) advertised.", tools.len())
    } else {
        format!("WebMCP is not supported by the browser at {}.", browser.endpoint.origin)
    };
    let ev = EvidenceBuilder::new(
        "web_mcp_tools_list",
        Subject { kind: "website".into(), sha256: None, name: Some(page_url.clone()) },
        browser_env(ctx.session.clean_room(), &browser),
    )
    .params(json!({"endpoint": browser.endpoint.origin}))
    .result(json!({"origin": page_url, "webmcp_supported": available, "tools": tools, "note": note}))
    .summary(summary)
    .confidence(Confidence::Observed)
    .authority(Authority::EarlierRecord)
    .limitation(Limitation::new("experimental", "WebMCP is an experimental CDP domain; absence here does not prove the page offers no in-page tools."))
    .coverage(Coverage { status: CoverageStatus::Partial, examined: Some(tools.len() as u64), total: None, next_offset: None })
    .location(Location::new("url", page_url))
    .build()?;
    Ok(ToolOutput::evidence(ev))
}

fn cleanup_then_err(client: &mut CdpClient, dl: &Deadline, e: ToolError) -> Result<ToolOutput, ToolError> {
    cleanup(client, dl);
    Err(e)
}

// ---------------------------------------------------------------------------------------------
// web_page_screenshot_take

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScreenshotParams {
    #[serde(default)]
    pub cdp_endpoint: Option<String>,
    #[serde(default)]
    pub launch: bool,
    #[serde(default = "yes")]
    pub headless: bool,
    #[serde(default)]
    pub no_sandbox: bool,
    #[serde(default)]
    pub target_id: Option<String>,
    #[serde(default = "d_timeout")]
    pub timeout_secs: u64,
}

fn web_page_screenshot_take(ctx: &mut Ctx<'_>, params: Value) -> Result<ToolOutput, ToolError> {
    let p: ScreenshotParams = parse_params(params)?;
    let dl = deadline(ctx, p.timeout_secs)?;
    let browser = resolve_browser(ctx, p.cdp_endpoint.as_deref(), p.launch, p.headless, p.no_sandbox, &dl)?;
    let targets = cdp::json_list(&browser.endpoint, &dl)?;
    let target = web_common::pick_target(&targets, p.target_id.as_deref(), Some("page"))?;
    let page_url = web_common::sanitize_url(target.get("url").and_then(Value::as_str).unwrap_or_default());
    let mut client = web_common::connect_target(&browser, target, PAGE_MAX_MESSAGE, &dl)?;
    let _ = client.call("Page.enable", json!({}), None, &dl);
    let shot = client.call("Page.captureScreenshot", json!({"format": "png"}), None, &dl);
    cleanup(&mut client, &dl);
    let shot = shot?;
    let data = shot
        .get("data")
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::new(craft_anything_evidence::ErrorCode::GarbledOutput, "the browser returned no screenshot data"))?;
    let bytes = web_common::b64_decode(data)?;
    let artifact = web_common::png_artifact(&bytes)?;
    let summary = format!("Captured a {}×{} PNG of {page_url} ({} bytes).", artifact["width"], artifact["height"], bytes.len());
    let ev = EvidenceBuilder::new(
        "web_page_screenshot_take",
        Subject { kind: "website".into(), sha256: None, name: Some(page_url.clone()) },
        browser_env(ctx.session.clean_room(), &browser),
    )
    .params(json!({"endpoint": browser.endpoint.origin, "target_id": p.target_id}))
    .result(json!({"origin": page_url, "screenshot": artifact}))
    .summary(summary)
    .confidence(Confidence::Observed)
    .authority(Authority::EarlierRecord)
    .limitation(Limitation::new("single_frame", "One image of the page's current state."))
    .coverage(Coverage::complete(1))
    .location(Location::new("url", page_url))
    .build()?;
    Ok(ToolOutput::evidence(ev))
}

// ---------------------------------------------------------------------------------------------
// web_snapshots_compare (pure computation)

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CompareCapturesParams {
    /// The earlier capture: web_page_show / web_page_watch evidence (inline or retained).
    pub before: Value,
    /// The later capture.
    pub after: Value,
    /// Exact-literal replacements applied to every text value of both sides before comparing, in
    /// rule_id order (at most 100), so volatile values such as build ids don't count as changes.
    #[serde(default)]
    pub normalization: Vec<NormalizationRule>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NormalizationRule {
    pub rule_id: String,
    /// The exact text to replace (no patterns).
    #[serde(rename = "match")]
    pub find: String,
    pub replacement: String,
}

/// Apply the rules to every string in `v`; returns how many replacements each rule made.
fn normalize(v: &mut Value, rules: &[NormalizationRule], counts: &mut [u64]) {
    match v {
        Value::String(s) => {
            for (rule, n) in rules.iter().zip(counts.iter_mut()) {
                let hits = s.matches(rule.find.as_str()).count() as u64;
                if hits > 0 {
                    *s = s.replace(rule.find.as_str(), &rule.replacement);
                    *n += hits;
                }
            }
        }
        Value::Array(a) => a.iter_mut().for_each(|x| normalize(x, rules, counts)),
        Value::Object(o) => o.values_mut().for_each(|x| normalize(x, rules, counts)),
        _ => {}
    }
}

fn web_snapshots_compare(ctx: &mut Ctx<'_>, params: Value) -> Result<ToolOutput, ToolError> {
    let mut p: CompareCapturesParams = parse_params(params)?;
    if p.normalization.len() > 100 || p.normalization.iter().any(|r| r.find.is_empty()) {
        return Err(ToolError::invalid("normalization: at most 100 rules, each with a non-empty match"));
    }
    p.normalization.sort_by(|a, b| a.rule_id.cmp(&b.rule_id));
    let expected = &["web_page_show", "web_page_watch", "web_page_js_analyze", "web_walkthrough_record"];
    let mut before = super::artifact_tools::resolve_evidence_input(ctx, &p.before, expected)?;
    let mut after = super::artifact_tools::resolve_evidence_input(ctx, &p.after, expected)?;
    let mut counts = vec![0u64; p.normalization.len()];
    normalize(&mut before.result, &p.normalization, &mut counts);
    normalize(&mut after.result, &p.normalization, &mut counts);
    let applied: Vec<Value> = p.normalization.iter().zip(&counts).map(|(r, n)| json!({"rule_id": r.rule_id, "replacements": n})).collect();
    let walkthrough = |r: &Value| r.get("steps").is_some() && r.get("action_count").is_some();
    if walkthrough(&before.result) || walkthrough(&after.result) {
        if !(walkthrough(&before.result) && walkthrough(&after.result)) {
            return Err(ToolError::invalid("compare a scripted run with another scripted run, not with a page capture"));
        }
        let mut diff = diff_walkthroughs(&before.result, &after.result);
        diff["normalization"] = json!(applied);
        let changed = diff["summary"]["changed_steps"].as_u64().unwrap_or(0);
        let net = &diff["network_requests"];
        let summary = format!(
            "Compared two scripted runs: {changed} step(s) differ; {} request(s) only in the later run, {} only in the earlier one.",
            net["added_count"], net["removed_count"]
        );
        let ev = EvidenceBuilder::new(
            "web_snapshots_compare",
            Subject { kind: "comparison".into(), sha256: None, name: Some("scripted runs".into()) },
            super::artifact_tools::env(ctx),
        )
        .params(json!({"before": before.id, "after": after.id, "normalization_rules": p.normalization.len()}))
        .result(diff)
        .summary(summary)
        .confidence(Confidence::Derived)
        .limitation(Limitation::new("not_proof_of_equality", "Missing, aborted or truncated runs never prove that two runs behave the same."))
        .coverage(Coverage::complete(1))
        .link(before.id)
        .link(after.id)
        .build()?;
        return Ok(ToolOutput::evidence(ev));
    }
    let mut diff = diff_captures(&before.result, &after.result);
    if !applied.is_empty() {
        diff["normalization"] = json!(applied);
    }
    let changed = diff["summary"]["changed_sections"].as_array().map_or(0, Vec::len);
    let summary =
        format!("Compared two captures of {}: {changed} section(s) changed.", before.result.get("origin").and_then(Value::as_str).unwrap_or("the page"));
    let ev = EvidenceBuilder::new(
        "web_snapshots_compare",
        Subject { kind: "comparison".into(), sha256: None, name: before.result.get("origin").and_then(Value::as_str).map(String::from) },
        super::artifact_tools::env(ctx),
    )
    .params(json!({"before": before.id, "after": after.id}))
    .result(diff)
    .summary(summary)
    .confidence(Confidence::Derived)
    .limitation(Limitation::new("normalized", "Timing and event sequence are dropped before comparison; only structural differences are reported."))
    .coverage(Coverage::complete(1))
    .link(before.id)
    .link(after.id)
    .build()?;
    Ok(ToolOutput::evidence(ev))
}

fn string_set(v: &Value, pointer: &str, field: &str) -> std::collections::BTreeSet<String> {
    v.pointer(pointer)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|e| if field.is_empty() { e.as_str().map(String::from) } else { e.get(field).and_then(Value::as_str).map(String::from) })
        .collect()
}

fn set_diff(before: &std::collections::BTreeSet<String>, after: &std::collections::BTreeSet<String>) -> Value {
    let added: Vec<&String> = after.difference(before).collect();
    let removed: Vec<&String> = before.difference(after).collect();
    json!({"added": added, "removed": removed, "added_count": added.len(), "removed_count": removed.len()})
}

fn diff_captures(before: &Value, after: &Value) -> Value {
    let sections = [
        ("frame_origins", "/frame_origins", ""),
        ("resources", "/resources", "url"),
        ("scripts", "/scripts", "url"),
        ("network_requests", "/network/requests", "url"),
        ("endpoints", "/endpoints", "url"),
    ];
    let mut out = serde_json::Map::new();
    let mut changed: Vec<&str> = Vec::new();
    for (name, ptr, field) in sections {
        let b = string_set(before, ptr, field);
        let a = string_set(after, ptr, field);
        if b.is_empty() && a.is_empty() {
            continue;
        }
        let d = set_diff(&b, &a);
        if d["added_count"].as_u64() != Some(0) || d["removed_count"].as_u64() != Some(0) {
            changed.push(name);
        }
        out.insert(name.to_string(), d);
    }
    // DOM element-count deltas.
    let dom_b = before.pointer("/dom/elements_by_name").and_then(Value::as_object).cloned().unwrap_or_default();
    let dom_a = after.pointer("/dom/elements_by_name").and_then(Value::as_object).cloned().unwrap_or_default();
    if dom_b != dom_a && (!dom_b.is_empty() || !dom_a.is_empty()) {
        changed.push("dom");
    }
    json!({"sections": out, "dom": {"before": dom_b, "after": dom_a}, "summary": {"changed_sections": changed}})
}

/// The parts of a step that matter when comparing runs: what it did, where it ended up, and a
/// digest (or the value) of each artifact.
fn step_fingerprint(step: &Value) -> serde_json::Map<String, Value> {
    let mut f = serde_json::Map::new();
    for k in ["action", "status", "url", "error"] {
        f.insert(k.into(), step.get(k).cloned().unwrap_or(Value::Null));
    }
    for (name, art) in step.get("artifacts").and_then(Value::as_object).into_iter().flatten() {
        f.insert(format!("artifacts.{name}"), art.get("sha256").cloned().unwrap_or_else(|| art.clone()));
    }
    if let Some(shot) = step.get("screenshot") {
        f.insert("screenshot".into(), shot.get("sha256").cloned().unwrap_or_else(|| shot.clone()));
    }
    f
}

fn field_changes(before: &serde_json::Map<String, Value>, after: &serde_json::Map<String, Value>) -> Vec<Value> {
    let keys: std::collections::BTreeSet<&String> = before.keys().chain(after.keys()).collect();
    keys.into_iter()
        .filter_map(|k| {
            let (b, a) = (before.get(k).unwrap_or(&Value::Null), after.get(k).unwrap_or(&Value::Null));
            (b != a).then(|| json!({"field": k, "before": b, "after": a}))
        })
        .collect()
}

/// Two web_walkthrough_record results, step by step.
fn diff_walkthroughs(before: &Value, after: &Value) -> Value {
    let key = |(i, s): (usize, &Value)| s.get("step_id").and_then(Value::as_str).map(String::from).unwrap_or_else(|| format!("#{i}"));
    let steps = |r: &Value| -> Vec<(String, Value)> {
        r.get("steps").and_then(Value::as_array).into_iter().flatten().enumerate().map(|(i, s)| (key((i, s)), s.clone())).collect()
    };
    let (b_steps, a_steps) = (steps(before), steps(after));
    let a_map: std::collections::BTreeMap<&str, &Value> = a_steps.iter().map(|(k, v)| (k.as_str(), v)).collect();
    let mut rows = Vec::new();
    let mut changed = 0u64;
    for (k, b) in &b_steps {
        match a_map.get(k.as_str()) {
            Some(a) => {
                let changes = field_changes(&step_fingerprint(b), &step_fingerprint(a));
                if !changes.is_empty() {
                    changed += 1;
                }
                rows.push(json!({"step": k, "status": if changes.is_empty() { "same" } else { "changed" }, "changes": changes}));
            }
            None => {
                changed += 1;
                rows.push(json!({"step": k, "status": "only_before"}));
            }
        }
    }
    for (k, _) in a_steps.iter().filter(|(k, _)| !b_steps.iter().any(|(bk, _)| bk == k)) {
        changed += 1;
        rows.push(json!({"step": k, "status": "only_after"}));
    }
    let requests = |r: &Value| -> std::collections::BTreeSet<String> {
        r.pointer("/network/requests")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|q| format!("{} {}", q.get("method").and_then(Value::as_str).unwrap_or_default(), q.get("url").and_then(Value::as_str).unwrap_or_default()))
            .collect()
    };
    let event_counts = |r: &Value| -> std::collections::BTreeMap<String, u64> {
        let mut m = std::collections::BTreeMap::new();
        for e in r.pointer("/events/items").and_then(Value::as_array).into_iter().flatten() {
            *m.entry(e.get("kind").and_then(Value::as_str).unwrap_or_default().to_string()).or_insert(0) += 1;
        }
        m
    };
    let finals = |r: &Value| {
        let mut f = serde_json::Map::new();
        for (name, art) in r.get("final_artifacts").and_then(Value::as_object).into_iter().flatten() {
            f.insert(name.clone(), art.get("sha256").cloned().unwrap_or_else(|| art.clone()));
        }
        f
    };
    let aborted: Vec<&str> = [("before", before), ("after", after)].into_iter().filter(|(_, r)| !r["aborted"].is_null()).map(|(n, _)| n).collect();
    let (eb, ea) = (event_counts(before), event_counts(after));
    json!({
        "kind": "walkthrough",
        "steps": rows,
        "network_requests": set_diff(&requests(before), &requests(after)),
        "event_counts": {"before": eb, "after": ea, "same": eb == ea},
        "final_artifacts": field_changes(&finals(before), &finals(after)),
        "comparable": aborted.is_empty(),
        "aborted_runs": aborted,
        "summary": {"changed_steps": changed, "steps_before": b_steps.len(), "steps_after": a_steps.len()},
    })
}

// ---------------------------------------------------------------------------------------------
// web_pixels_compare (pure computation)

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CompareScreenshotsParams {
    /// Earlier PNG: a web_page_screenshot_take evidence (inline/retained), or inline {base64} / {path}.
    pub before: Value,
    /// Later PNG.
    pub after: Value,
    /// Per-channel difference a pixel must exceed to count as changed (0..=255).
    #[serde(default)]
    pub tolerance: u8,
    /// Compare the area both images share (top-left) when their sizes differ, and add a coarse
    /// `layout_similarity` (0..1, share of 16 px tiles with matching mean colour) and a
    /// `perceptual_similarity` (0..1, mean SSIM of brightness: edges, text, texture). Use it to
    /// compare a rebuild with the original page, whose heights rarely match exactly.
    #[serde(default)]
    pub common_area: bool,
    /// With common_area: a noise-<width>.png mask from web_site_capture ({path} or {base64});
    /// its white squares (parts that change by themselves) are left out of the layout and detail.
    #[serde(default)]
    pub ignore: Option<Value>,
}

fn web_pixels_compare(ctx: &mut Ctx<'_>, params: Value) -> Result<ToolOutput, ToolError> {
    let p: CompareScreenshotsParams = parse_params(params)?;
    let a = load_png(ctx, &p.before, "before")?;
    let b = load_png(ctx, &p.after, "after")?;
    let mask = p.ignore.as_ref().map(|v| load_png(ctx, v, "ignore").and_then(|m| web_common::NoiseMask::from_png(&m))).transpose()?;
    let mut img_a = web_common::decode_png(&a)?;
    let mut img_b = web_common::decode_png(&b)?;
    let sizes = json!({"before": {"width": img_a.width, "height": img_a.height}, "after": {"width": img_b.width, "height": img_b.height}});
    if p.common_area {
        let (w, h) = (img_a.width.min(img_b.width), img_a.height.min(img_b.height));
        img_a = web_common::crop(&img_a, w, h);
        img_b = web_common::crop(&img_b, w, h);
    }
    let mut diff = web_common::pixel_diff(&img_a, &img_b, p.tolerance);
    if p.common_area
        && let Some(o) = diff.as_object_mut()
    {
        let sim = web_common::block_similarity_masked(&img_a, &img_b, 16, 24, mask.as_ref());
        o.insert("layout_similarity".into(), json!(sim));
        o.insert("perceptual_similarity".into(), json!(web_common::perceptual_similarity_masked(&img_a, &img_b, mask.as_ref())));
        o.insert("sizes".into(), sizes);
        if let Some(m) = &mask {
            o.insert("ignored_tiles".into(), json!(m.count()));
        }
    }
    let summary = if let Some(sim) = diff.get("layout_similarity").and_then(Value::as_f64) {
        format!(
            "Over the shared {}×{} area the layout is {:.0}% alike and the detail {:.0}% ({} pixel(s) differ exactly).",
            img_a.width,
            img_a.height,
            sim * 100.0,
            diff.get("perceptual_similarity").and_then(Value::as_f64).unwrap_or(0.0) * 100.0,
            diff["differing_pixels"]
        )
    } else if diff["comparable"] == false {
        "The two screenshots have different dimensions and cannot be compared pixel by pixel.".to_string()
    } else if diff["identical"] == true {
        "The two screenshots are identical within the threshold.".to_string()
    } else {
        format!("{} of {} pixel(s) differ (max channel delta {}).", diff["differing_pixels"], diff["total_pixels"], diff["max_channel_delta"])
    };
    let ev = EvidenceBuilder::new("web_pixels_compare", Subject { kind: "comparison".into(), sha256: None, name: Some("screenshots".into()) }, super::artifact_tools::env(ctx))
        .params(json!({"tolerance": p.tolerance, "common_area": p.common_area, "before_sha256": craft_anything_evidence::sha256_hex(&a), "after_sha256": craft_anything_evidence::sha256_hex(&b)}))
        .result(diff)
        .summary(summary)
        .confidence(Confidence::Derived)
        .coverage(Coverage::complete(1))
        .build()?;
    Ok(ToolOutput::evidence(ev))
}

/// Load PNG bytes from capture evidence, inline {base64}, or {path}.
fn load_png(ctx: &Ctx<'_>, v: &Value, what: &str) -> Result<Vec<u8>, ToolError> {
    if let Some(b64) = v.get("base64").and_then(Value::as_str) {
        return web_common::b64_decode(b64);
    }
    if let Some(path) = v.get("path").and_then(Value::as_str) {
        let path = super::artifact_tools::abs_path(path, &format!("{what}.path"))?;
        return super::artifact_tools::read_capped(&path, web_common::MAX_SCREENSHOT_BYTES as u64);
    }
    // Evidence from web_page_screenshot_take.
    let ev = super::artifact_tools::resolve_evidence_input(ctx, v, &["web_page_screenshot_take"])?;
    let b64 =
        ev.result.pointer("/screenshot/base64").and_then(Value::as_str).ok_or_else(|| ToolError::invalid(format!("{what} evidence has no screenshot")))?;
    web_common::b64_decode(b64)
}

// ---------------------------------------------------------------------------------------------
// web_audio_fingerprint / web_audio_compare (pure computation)

/// One interaction-traced mixer reading, as a capture or a caller records it.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MixerSample {
    /// Milliseconds since the trace started.
    pub t_ms: u64,
    /// The mixer layer this reading belongs to.
    pub layer_id: String,
    /// Layer gain at the reading.
    pub gain: f32,
    /// Stereo pan at the reading.
    pub pan: f32,
}

impl From<MixerSample> for EnvelopeSample {
    fn from(sample: MixerSample) -> Self {
        EnvelopeSample { t_ms: sample.t_ms, layer_id: sample.layer_id, gain: sample.gain, pan: sample.pan }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FingerprintAudioParams {
    /// A capture from web_audio_capture: the evidence record, {"kind":"saved","evidence_id":…}, its
    /// id, or the raw result object {graph, spectrum, rms, mixer_state, mixer_envelope}.
    #[serde(default)]
    pub capture: Option<Value>,
    /// Raw spectrum values to quantise into 48 log-spaced bins (inline alternative to `capture`).
    #[serde(default)]
    pub spectrum: Option<Vec<f32>>,
    /// One RMS value per second of capture.
    #[serde(default)]
    pub rms: Option<Vec<f32>>,
    /// The audio graph signature: {"node_counts": {"gain": 2}, "edges": [{"from": …, "to": …, "kind": "audio"}]}.
    #[serde(default)]
    pub graph: Option<Value>,
    /// The page's mixer state object, e.g. {"scenario_id": "focus"}.
    #[serde(default)]
    pub mixer_state: Option<Value>,
    /// Interaction-traced readings: [{"t_ms": 0, "layer_id": "pad", "gain": 0.4, "pan": 0.0}].
    #[serde(default)]
    pub mixer_envelope: Option<Vec<MixerSample>>,
    /// When the capture happened, in Unix milliseconds.
    #[serde(default)]
    pub captured_at_ms: Option<u64>,
}

fn web_audio_fingerprint(ctx: &mut Ctx<'_>, params: Value) -> Result<ToolOutput, ToolError> {
    let p: FingerprintAudioParams = parse_params(params)?;
    let has_input =
        p.capture.is_some() || p.spectrum.is_some() || p.rms.is_some() || p.graph.is_some() || p.mixer_state.is_some() || p.mixer_envelope.is_some();
    if !has_input {
        return Err(ToolError::invalid("there is no audio capture to fingerprint")
            .with_remediation("Pass `capture` from web_audio_capture, or inline `spectrum`/`rms`/`graph`/`mixer_state`."));
    }

    let mut spectrum = p.spectrum.unwrap_or_default();
    let mut rms = p.rms.unwrap_or_default();
    let mut graph = p.graph.unwrap_or(Value::Null);
    let mut mixer = p.mixer_state.unwrap_or(Value::Null);
    let mut envelope: Option<Vec<EnvelopeSample>> = p.mixer_envelope.map(|samples| samples.into_iter().map(EnvelopeSample::from).collect());
    let mut captured_at_ms = p.captured_at_ms.unwrap_or(0);
    let mut deep_mode = false;
    let mut capture_id: Option<String> = None;
    let mut source = "inline";
    if let Some(value) = &p.capture {
        let (id, result, deep) = resolve_audio_capture(ctx, value)?;
        source = if id.is_some() { "a capture's evidence" } else { "a capture object" };
        deep_mode = deep;
        capture_id = id;
        if spectrum.is_empty() {
            spectrum = number_list(&result, "spectrum")?;
        }
        if rms.is_empty() {
            rms = number_list(&result, "rms")?;
        }
        if graph.is_null() {
            graph = result.get("graph").cloned().unwrap_or(Value::Null);
        }
        if mixer.is_null() {
            mixer = result.get("mixer_state").cloned().unwrap_or(Value::Null);
        }
        if envelope.is_none() {
            envelope = Some(mixer_samples(result.get("mixer_envelope"), "the capture's mixer_envelope")?);
        }
        if captured_at_ms == 0 {
            captured_at_ms = result.get("captured_at_ms").and_then(Value::as_u64).unwrap_or(0);
        }
    }
    let envelope = envelope.unwrap_or_default();

    let mut fingerprint = audio::fingerprint_from_capture(&spectrum, &rms, graph, mixer, &envelope)?;
    fingerprint.deep_mode = deep_mode;
    fingerprint.captured_at_ms = captured_at_ms;
    let scenario = if fingerprint.scenario_id.is_empty() { "an audio scene".to_string() } else { fingerprint.scenario_id.clone() };
    let summary = format!(
        "Fingerprinted {scenario} from {source}: {} log bin(s), {} RMS value(s), {} graph node kind(s), {} mixer sample(s).",
        fingerprint.log_bins.len(),
        fingerprint.rms_envelope.len(),
        fingerprint.graph_signature.node_counts.len(),
        fingerprint.mixer_envelope.len()
    );
    let params = json!({
        "source": source,
        "capture_evidence_id": capture_id,
        "spectrum_values": spectrum.len(),
        "rms_values": fingerprint.rms_envelope.len(),
        "deep_mode": deep_mode,
        "captured_at_ms": captured_at_ms,
    });
    let mut builder = EvidenceBuilder::new(
        "web_audio_fingerprint",
        Subject { kind: "audio-scene".into(), sha256: None, name: Some(scenario) },
        super::artifact_tools::env(ctx),
    )
    .params(params)
    .result(json!({"fingerprint": fingerprint}))
    .summary(summary)
    .confidence(Confidence::Derived)
    .limitation(Limitation::new("summary_not_a_recording", "A fingerprint is a summary, not a recording: it keeps no audio samples."))
    .coverage(Coverage::complete(1));
    if let Some(id) = capture_id {
        builder = builder.link(id);
    }
    Ok(ToolOutput::evidence(builder.build()?))
}

/// The raw capture object behind `capture` (the evidence record's result, or the object itself), the
/// evidence id when there was one, and whether the capture ran in Deep mode.
fn resolve_audio_capture(ctx: &Ctx<'_>, value: &Value) -> Result<(Option<String>, Value, bool), ToolError> {
    let looks_like_evidence =
        value.is_string() || value.get("kind").and_then(Value::as_str) == Some("saved") || (value.get("operation").is_some() && value.get("result").is_some());
    if !looks_like_evidence {
        if !value.is_object() {
            return Err(ToolError::invalid("`capture` isn't a capture object")
                .with_remediation("Pass the evidence web_audio_capture returned, or its {graph, spectrum, rms, mixer_state} object."));
        }
        return Ok((None, value.clone(), false));
    }
    let ev = super::artifact_tools::resolve_evidence_input(ctx, value, &["web_audio_capture"])?;
    let deep = !ev.environment.clean_room;
    Ok((Some(ev.id), ev.result, deep))
}

/// A list of numbers from a capture object; a missing or null key is an empty list.
fn number_list(capture: &Value, key: &str) -> Result<Vec<f32>, ToolError> {
    match capture.get(key) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(value) => serde_json::from_value(value.clone()).map_err(|error| {
            ToolError::invalid(format!("the capture's `{key}` isn't a list of numbers: {error}"))
                .with_remediation("Pass the numbers web_audio_capture returned for `spectrum` and `rms`.")
        }),
    }
}

/// Interaction-traced mixer readings from a capture object or a bare list.
fn mixer_samples(value: Option<&Value>, what: &str) -> Result<Vec<EnvelopeSample>, ToolError> {
    let Some(value) = value.filter(|value| !value.is_null()) else { return Ok(Vec::new()) };
    let rows: Vec<MixerSample> = serde_json::from_value(value.clone()).map_err(|error| {
        ToolError::invalid(format!("{what} isn't a list of mixer readings: {error}"))
            .with_remediation("Pass [{t_ms, layer_id, gain, pan}] samples, or omit the envelope.")
    })?;
    Ok(rows.into_iter().map(EnvelopeSample::from).collect())
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CompareAudioParams {
    /// The earlier fingerprint: web_audio_fingerprint evidence, a web_audio_offline_render result,
    /// {"kind":"saved","evidence_id":…}, or a bare fingerprint object.
    pub before: Value,
    /// The later fingerprint, in the same forms.
    pub after: Value,
    /// How far the log-bin cosine may fall short of 1.0 and still count as alike (0..=1, default 0.15).
    #[serde(default = "d_log_bin_tolerance")]
    pub log_bin_tolerance: f64,
    /// How far the RMS-envelope cosine may fall short of 1.0 and still count as alike (0..=1, default 0.20).
    #[serde(default = "d_rms_tolerance")]
    pub rms_tolerance: f64,
}

fn d_log_bin_tolerance() -> f64 {
    0.15
}

fn d_rms_tolerance() -> f64 {
    0.20
}

fn web_audio_compare(ctx: &mut Ctx<'_>, params: Value) -> Result<ToolOutput, ToolError> {
    let p: CompareAudioParams = parse_params(params)?;
    for (name, value) in [("log_bin_tolerance", p.log_bin_tolerance), ("rms_tolerance", p.rms_tolerance)] {
        if !value.is_finite() || !(0.0..=1.0).contains(&value) {
            return Err(ToolError::invalid(format!("{name} must be 0..=1"))
                .with_remediation("Use 0.15 for the spectrum and 0.20 for RMS unless you need a looser or tighter match."));
        }
    }
    let (before, before_id) = parse_fingerprint_input(ctx, &p.before, "before")?;
    let (after, after_id) = parse_fingerprint_input(ctx, &p.after, "after")?;

    let log_bin_cosine = audio::spectral_cosine(&before.log_bins, &after.log_bins)?;
    let rms_correlation = rms_correlation(&before.rms_envelope, &after.rms_envelope);
    let envelope_distance = audio::envelope_distance(&before.mixer_envelope, &after.mixer_envelope);
    let graph_diff = graph_diff(&before.graph_signature, &after.graph_signature);
    let likeness_pass = log_bin_cosine >= 1.0 - p.log_bin_tolerance && rms_correlation >= 1.0 - p.rms_tolerance;
    let low_energy = !has_energy(&before.log_bins) || !has_energy(&after.log_bins) || !has_energy(&before.rms_envelope) || !has_energy(&after.rms_envelope);
    let graphs_differ = graph_diff["same"] != true;
    let summary = format!(
        "The two audio fingerprints {}: log-bin cosine {log_bin_cosine:.3}, RMS correlation {rms_correlation:.3}, mixer-envelope distance {envelope_distance:.3}{}.",
        if likeness_pass { "are alike" } else { "differ" },
        if graphs_differ { "; their audio graphs differ" } else { "" }
    );
    let mut builder = EvidenceBuilder::new(
        "web_audio_compare",
        Subject { kind: "comparison".into(), sha256: None, name: Some("audio fingerprints".into()) },
        super::artifact_tools::env(ctx),
    )
    .params(json!({
        "before": before_id,
        "after": after_id,
        "log_bin_tolerance": p.log_bin_tolerance,
        "rms_tolerance": p.rms_tolerance,
    }))
    .result(json!({
        "log_bin_cosine": log_bin_cosine,
        "rms_correlation": rms_correlation,
        "graph_diff": graph_diff,
        "envelope_distance": envelope_distance,
        "likeness_pass": likeness_pass,
    }))
    .summary(summary)
    .confidence(Confidence::Derived)
    .limitation(Limitation::new(
        "spectral_summary_only",
        "The verdict compares spectral shape and loudness: it cannot hear timbre, stereo image or content beyond the 1 Hz envelope.",
    ))
    .coverage(Coverage::complete(1));
    if low_energy {
        builder = builder.limitation(Limitation::new(
            "low_energy",
            "At least one side has no spectral or loudness energy (silence, or an analyser that returned zeros); the verdict cannot pass.",
        ));
    }
    if before.rms_envelope.len() != after.rms_envelope.len() {
        builder = builder.limitation(Limitation::new(
            "rms_resampled",
            format!(
                "The two RMS envelopes have different lengths ({} and {} values) and were proportionally resampled before their correlation was measured.",
                before.rms_envelope.len(),
                after.rms_envelope.len()
            ),
        ));
    }
    if before.mixer_envelope.len() != after.mixer_envelope.len() && !before.mixer_envelope.is_empty() && !after.mixer_envelope.is_empty() {
        builder = builder.limitation(Limitation::new(
            "envelope_resampled",
            "The two mixer envelopes have different lengths and were proportionally resampled before their distance was measured.",
        ));
    }
    if let Some(id) = before_id {
        builder = builder.link(id);
    }
    if let Some(id) = after_id {
        builder = builder.link(id);
    }
    Ok(ToolOutput::evidence(builder.build()?))
}

/// A fingerprint from a tool result ({fingerprint: {…}} or a bare fingerprint), migrated to a
/// supported version, with the evidence id it came from when it was an evidence record.
fn parse_fingerprint_input(ctx: &Ctx<'_>, value: &Value, what: &str) -> Result<(Fingerprint, Option<String>), ToolError> {
    let looks_like_evidence =
        value.is_string() || value.get("kind").and_then(Value::as_str) == Some("saved") || (value.get("operation").is_some() && value.get("result").is_some());
    let (fingerprint, id) = if looks_like_evidence {
        let ev = super::artifact_tools::resolve_evidence_input(ctx, value, &["web_audio_fingerprint", "web_audio_offline_render"])?;
        let fingerprint = fingerprint_object(&ev.result).ok_or_else(|| {
            ToolError::invalid(format!("{what} evidence {} carries no audio fingerprint", ev.id))
                .with_remediation("Pass the result of web_audio_fingerprint or web_audio_offline_render.")
        })?;
        (fingerprint, Some(ev.id))
    } else {
        (value.get("fingerprint").cloned().unwrap_or_else(|| value.clone()), None)
    };
    let fingerprint: Fingerprint = serde_json::from_value(fingerprint).map_err(|error| {
        ToolError::invalid(format!("{what} isn't an audio fingerprint: {error}"))
            .with_remediation("Pass the evidence a fingerprint tool returned, or its `fingerprint` object.")
    })?;
    Ok((audio::migrate(fingerprint)?, id))
}

/// The fingerprint object inside a result: `{fingerprint: {…}}`, or the result itself when it already
/// is a fingerprint (a bare object or an offline render's `{fingerprint, rendered_ms}`).
fn fingerprint_object(result: &Value) -> Option<Value> {
    result.get("fingerprint").filter(|value| value.is_object()).cloned().or_else(|| result.is_object().then(|| result.clone()))
}

/// True when any value is finite and non-zero.
fn has_energy(values: &[f32]) -> bool {
    values.iter().any(|value| value.is_finite() && *value != 0.0)
}

/// Cosine similarity of two RMS envelopes, in `-1..=1`.
///
/// Scale-invariant, so a louder recording of the same shape still matches; a mirrored shape
/// (0→1 against 1→0) scores low. Envelopes of different lengths are linearly resampled to the
/// longer one first (98 vs 100 readings compares, never panics). Zero energy on either side
/// scores `0.0` rather than a NaN.
fn rms_correlation(a: &[f32], b: &[f32]) -> f64 {
    let len = a.len().max(b.len());
    if len == 0 {
        return 0.0;
    }
    let left = resample_envelope(a, len);
    let right = resample_envelope(b, len);
    let mut dot = 0.0f64;
    let mut left_norm = 0.0f64;
    let mut right_norm = 0.0f64;
    for (left, right) in left.iter().zip(right.iter()) {
        let left = if left.is_finite() { f64::from(*left) } else { 0.0 };
        let right = if right.is_finite() { f64::from(*right) } else { 0.0 };
        dot += left * right;
        left_norm += left * left;
        right_norm += right * right;
    }
    if left_norm == 0.0 || right_norm == 0.0 {
        return 0.0;
    }
    let cosine = dot / (left_norm.sqrt() * right_norm.sqrt());
    if cosine.is_finite() { cosine.clamp(-1.0, 1.0) } else { 0.0 }
}

/// Linearly resample an envelope to `len` values; an empty envelope reads as silence.
fn resample_envelope(points: &[f32], len: usize) -> Vec<f32> {
    if len == 0 {
        return Vec::new();
    }
    if points.is_empty() {
        return vec![0.0; len];
    }
    if points.len() == len {
        return points.to_vec();
    }
    let last = points.len().saturating_sub(1);
    let mut out = Vec::with_capacity(len);
    for index in 0..len {
        let position = ((index as f64 + 0.5) * points.len() as f64 / len as f64 - 0.5).clamp(0.0, last as f64);
        let lo = position.floor() as usize;
        let hi = (lo + 1).min(last);
        let weight = (position - lo as f64) as f32;
        let low = points.get(lo).copied().unwrap_or(0.0);
        let high = points.get(hi).copied().unwrap_or(0.0);
        out.push(low + (high - low) * weight);
    }
    out
}

/// What changed between two audio graphs: node counts per kind, and connections.
fn graph_diff(before: &GraphSignature, after: &GraphSignature) -> Value {
    let mut kinds: std::collections::BTreeSet<&String> = before.node_counts.keys().collect();
    kinds.extend(after.node_counts.keys());
    let mut added_nodes = serde_json::Map::new();
    let mut removed_nodes = serde_json::Map::new();
    for kind in kinds {
        let was = before.node_counts.get(kind).copied().unwrap_or(0);
        let now = after.node_counts.get(kind).copied().unwrap_or(0);
        if now > was {
            added_nodes.insert(kind.clone(), json!(now - was));
        }
        if was > now {
            removed_nodes.insert(kind.clone(), json!(was - now));
        }
    }
    let before_edges: std::collections::BTreeSet<&Edge> = before.edges.iter().collect();
    let after_edges: std::collections::BTreeSet<&Edge> = after.edges.iter().collect();
    let added_edges: Vec<&Edge> = after_edges.difference(&before_edges).copied().collect();
    let removed_edges: Vec<&Edge> = before_edges.difference(&after_edges).copied().collect();
    let same = added_nodes.is_empty() && removed_nodes.is_empty() && added_edges.is_empty() && removed_edges.is_empty();
    json!({
        "same": same,
        "added_nodes": added_nodes,
        "removed_nodes": removed_nodes,
        "added_edges": added_edges,
        "removed_edges": removed_edges,
    })
}

// ---------------------------------------------------------------------------------------------
// web_audio_capture (Deep-only: live audio graph, spectrum and mixer state)

/// Most spectrum values a capture keeps (a 32k-point FFT is 16k bins; 4k is plenty to bin).
const MAX_CAPTURE_SPECTRUM: usize = 4_096;
/// Most loudness values a capture keeps (ten minutes at one per second).
const MAX_CAPTURE_RMS: usize = 600;
/// Most graph edges a capture keeps.
const MAX_CAPTURE_EDGES: usize = 2_000;
/// Most node kinds a capture keeps.
const MAX_CAPTURE_KINDS: usize = 64;
/// Largest mixer-state object a capture keeps, in bytes.
const MAX_MIXER_BYTES: usize = 256 * 1024;
/// Most endpoints the network outline names.
const MAX_OUTLINE_ENDPOINTS: usize = 50;

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CaptureAudioParams {
    /// Loopback CDP endpoint, e.g. http://127.0.0.1:9222. Omit to launch a browser.
    #[serde(default)]
    pub cdp_endpoint: Option<String>,
    /// Launch a located Chromium when no endpoint is given.
    #[serde(default)]
    pub launch: bool,
    /// Launch headless (when launching).
    #[serde(default = "yes")]
    pub headless: bool,
    /// Pass --no-sandbox when launching (needed in some containers).
    #[serde(default)]
    pub no_sandbox: bool,
    /// The page target to listen to (default: the first page).
    #[serde(default)]
    pub target_id: Option<String>,
    /// How long to sample the live audio, in milliseconds (5000..=120000).
    #[serde(default = "d_capture_duration_ms")]
    pub duration_ms: u64,
    /// Overall limit for the whole capture, in seconds (1..=600).
    #[serde(default = "d_capture_timeout")]
    pub timeout_secs: u64,
    /// Values to redact wherever they appear in the capture (cookies, tokens, names).
    #[serde(default)]
    pub hidden_values: Vec<String>,
}

fn d_capture_duration_ms() -> u64 {
    30_000
}

fn d_capture_timeout() -> u64 {
    180
}

fn web_audio_capture(ctx: &mut Ctx<'_>, params: Value) -> Result<ToolOutput, ToolError> {
    let p: CaptureAudioParams = parse_params(params)?;
    if !(5_000..=120_000).contains(&p.duration_ms) {
        return Err(ToolError::invalid(format!("duration_ms must be 5000..=120000; {} is outside that window", p.duration_ms))
            .with_remediation("Listen for five seconds or longer (up to two minutes) so the spectrum and one-per-second loudness envelope mean something."));
    }
    let dl = deadline(ctx, p.timeout_secs)?;
    let launch = p.launch || p.cdp_endpoint.is_none();
    let browser = resolve_browser(ctx, p.cdp_endpoint.as_deref(), launch, p.headless, p.no_sandbox, &dl)?;
    let targets = cdp::json_list(&browser.endpoint, &dl)?;
    let target = web_common::pick_target(&targets, p.target_id.as_deref(), Some("page"))?;
    let page_url = web_common::sanitize_url(target.get("url").and_then(Value::as_str).unwrap_or_default());
    let mut client = web_common::connect_target(&browser, target, PAGE_MAX_MESSAGE, &dl)?;
    let mut secrets = Secrets::new(&p.hidden_values);
    // Runtime runs the introspection script, Media reports which audio is playing, and Network
    // outlines the requests made while the capture listens.
    let _ = client.call("Runtime.enable", json!({}), None, &dl);
    let _ = client.call("Media.enable", json!({}), None, &dl);
    let network_enabled = client.call("Network.enable", json!({}), None, &dl).is_ok();

    let reply = match client.call(
        "Runtime.evaluate",
        json!({
            "expression": audio_capture_script(p.duration_ms),
            "awaitPromise": true,
            "returnByValue": true,
            "userGesture": true,
        }),
        None,
        &dl,
    ) {
        Ok(reply) => reply,
        Err(error) => return cleanup_then_err(&mut client, &dl, error),
    };
    if let Some(details) = reply.get("exceptionDetails") {
        let text = details
            .pointer("/exception/description")
            .and_then(Value::as_str)
            .or_else(|| details.get("text").and_then(Value::as_str))
            .unwrap_or("the introspection script failed");
        let error = ToolError::new(ErrorCode::ProgramFailed, format!("the page rejected the audio introspection: {}", clip_text(text, 400)))
            .with_remediation("Reload the page and try again; a Content-Security-Policy that blocks injected scripts also blocks this capture.");
        return cleanup_then_err(&mut client, &dl, error);
    }
    let capture = match parse_audio_capture_reply(&reply) {
        Ok(capture) => capture,
        Err(error) => return cleanup_then_err(&mut client, &dl, error),
    };
    let network_outline = audio_network_outline(&mut client, &mut secrets, network_enabled);
    cleanup(&mut client, &dl);

    let page_url = secrets.text(&page_url);
    let scenario = capture.pointer("/mixer_state/scenario_id").and_then(Value::as_str).unwrap_or_default().to_string();
    let name = secrets.text(&if scenario.is_empty() { page_url.clone() } else { scenario });
    let node_kinds = capture.pointer("/graph/node_counts").and_then(Value::as_object).map(|counts| counts.len()).unwrap_or(0);
    let node_count: u64 =
        capture.pointer("/graph/node_counts").and_then(Value::as_object).map(|counts| counts.values().filter_map(Value::as_u64).sum()).unwrap_or(0);
    let edge_count = capture.pointer("/graph/edges").and_then(Value::as_array).map(Vec::len).unwrap_or(0);
    let spectrum_len = capture.get("spectrum").and_then(Value::as_array).map(Vec::len).unwrap_or(0);
    let rms_len = capture.get("rms").and_then(Value::as_array).map(Vec::len).unwrap_or(0);
    let graph_truncated = capture.get("graph_truncated").and_then(Value::as_bool).unwrap_or(false);
    let spectrum_truncated = capture.get("spectrum_truncated").and_then(Value::as_bool).unwrap_or(false);
    let sample_rate = capture.get("sample_rate").and_then(Value::as_u64).unwrap_or(48_000);
    let fft_size = capture.get("fft_size").and_then(Value::as_u64).unwrap_or(0);
    let samples = capture.get("samples").and_then(Value::as_u64).unwrap_or(0);
    let summary = secrets.text(&format!(
        "Captured {} ms of {name}'s live audio: {node_count} node(s) across {node_kinds} kind(s), {edge_count} connection(s), {spectrum_len} spectrum bin(s) and {rms_len} loudness value(s) at {sample_rate} Hz.",
        p.duration_ms
    ));

    let result = json!({
        "browser": {"product": browser.product, "started_by": if browser.launched { "craft-anything" } else { "external" }, "cleanup": if browser.launched { "closed" } else { "disconnected" }},
        "url": page_url,
        "duration_ms": p.duration_ms,
        "captured_at_ms": now_unix_ms(),
        "graph": capture.get("graph").cloned().unwrap_or(Value::Null),
        "graph_truncated": graph_truncated,
        "spectrum": capture.get("spectrum").cloned().unwrap_or_else(|| json!([])),
        "spectrum_truncated": spectrum_truncated,
        "rms": capture.get("rms").cloned().unwrap_or_else(|| json!([])),
        "mixer_state": capture.get("mixer_state").cloned().unwrap_or_else(|| json!({})),
        "mixer_source": capture.get("mixer_source").cloned().unwrap_or_else(|| json!("none")),
        "mixer_envelope": [],
        "network_outline": network_outline,
        "sample_rate": sample_rate,
        "fft_size": fft_size,
        "samples": samples,
        "secrets_hidden": p.hidden_values.len(),
        "secret_redactions": 0,
    });
    let result = secrets.scrub(&result);
    // Stamp the count after the scrub: scrubbing the page's graph and mixer state redacts values
    // too, so a count taken while building the object would under-report.
    let mut result = result;
    if let Some(object) = result.as_object_mut() {
        object.insert("secret_redactions".into(), json!(secrets.redactions));
    }

    let mut builder = EvidenceBuilder::new(
        "web_audio_capture",
        Subject { kind: "audio-scene".into(), sha256: None, name: Some(name) },
        browser_env(ctx.session.clean_room(), &browser),
    )
    .params(json!({
        "endpoint": browser.endpoint.origin,
        "launched": browser.launched,
        "duration_ms": p.duration_ms,
        "target_id": p.target_id,
        "sensitive_value_declarations": p.hidden_values.len(),
    }))
    .result(result)
    .summary(summary)
    .confidence(Confidence::Observed)
    .authority(Authority::OurRun)
    .limitation(Limitation::new(
        "analyser_snapshot",
        "The spectrum is one AnalyserNode's averaged FFT and its loudness one value per second, not a recording: layers, timbre beyond the bin resolution and everything shorter than a second are lost.",
    ))
    .limitation(Limitation::new(
        "live_stochastic_scene",
        "This is one live window of a page's audio: a second capture of a generative scene sounds different, so compare fingerprints rather than exact samples.",
    ))
    .limitation(Limitation::new(
        "no_interaction_trace",
        "This capture did not trace interactions, so mixer_envelope is empty and only the mixer state at the end is recorded.",
    ))
    .coverage(Coverage::complete(1))
    .location(Location::new("url", page_url));
    if node_count == 0 {
        builder = builder.limitation(Limitation::new(
            "graph_not_observed",
            "No audio node was created or connected after the script started, so the graph is empty: nodes the page built before the capture cannot be recovered.",
        ));
    }
    if graph_truncated || spectrum_truncated {
        builder =
            builder.limitation(Limitation::new("bounded_capture", "Some graph edges, node kinds or spectrum bins were dropped to keep the capture bounded."));
    }
    if samples == 0 {
        builder = builder.limitation(Limitation::new(
            "no_audio_samples",
            "The analyser returned no samples during the window: the audio context may be suspended, or the page plays nothing.",
        ));
    }
    if p.hidden_values.iter().any(|value| !value.is_empty()) {
        builder = builder.limitation(Limitation::new("secrets_redacted", format!("{} occurrence(s) of declared values were redacted", secrets.redactions)));
    }
    Ok(ToolOutput::evidence(builder.build()?))
}

/// The page script, with the requested window substituted. It resolves an existing AudioContext
/// (null when the page has none), instruments node creation and connection, finds an analyser,
/// then averages its spectrum and one-per-second loudness over the window. Bounded throughout.
const AUDIO_CAPTURE_SCRIPT: &str = r#"
(async () => {
  const MAX_SPECTRUM = 4096, MAX_EDGES = 2000, MAX_NODES = 2000, MAX_LAYERS = 64, MAX_KINDS = 64, MAX_RMS = 600;
  const DURATION_MS = __DURATION_MS__;
  const AC = window.AudioContext || window.webkitAudioContext;
  if (!AC) return null;
  const kindOf = (node) => {
    try {
      const name = node && node.constructor && node.constructor.name;
      if (typeof name !== 'string' || name.length === 0 || name.length > 64) return 'AudioNode';
      return name.replace(/Node$/, '').replace(/([a-z0-9])([A-Z])/g, '$1_$2').toLowerCase();
    } catch (error) { return 'AudioNode'; }
  };
  const findContext = () => {
    const registry = window.__craftAnythingAudioContexts;
    if (Array.isArray(registry)) {
      for (const candidate of registry) {
        if (candidate && typeof candidate.createAnalyser === 'function') return candidate;
      }
    }
    for (const key of ['__craftAnythingAudioContext', 'audioContext', 'ctx']) {
      let candidate;
      try { candidate = window[key]; } catch (error) { continue; }
      if (candidate && typeof candidate.createAnalyser === 'function' && candidate.destination) return candidate;
    }
    let seen = 0;
    for (const key of Object.getOwnPropertyNames(window)) {
      if (seen >= 5000) break;
      seen += 1;
      if (key.length > 128) continue;
      let candidate;
      try { candidate = window[key]; } catch (error) { continue; }
      if (candidate && typeof candidate === 'object' && typeof candidate.createAnalyser === 'function' && typeof candidate.currentTime === 'number') return candidate;
    }
    return null;
  };
  const context = findContext();
  if (!context) return null;
  if (context.state === 'suspended') {
    try { await context.resume(); } catch (error) {}
  }
  const tracker = (() => {
    const prior = window.__craftAnythingAudioGraph;
    const state = prior && prior.nodes instanceof Set && Array.isArray(prior.edges) ? prior : { nodes: new Set(), edges: [], truncated: false };
    state.kindOf = kindOf;
    const record = (node) => {
      try { if (node && state.nodes.size < MAX_NODES) state.nodes.add(node); } catch (error) {}
      return node;
    };
    const creators = ['createGain', 'createOscillator', 'createAnalyser', 'createBiquadFilter', 'createDelay', 'createConvolver', 'createDynamicsCompressor', 'createPanner', 'createStereoPanner', 'createWaveShaper', 'createChannelSplitter', 'createChannelMerger', 'createConstantSource', 'createBufferSource', 'createMediaElementSource', 'createMediaStreamSource', 'createScriptProcessor', 'createPeriodicWave'];
    for (const name of creators) {
      try {
        const original = AC.prototype[name];
        if (typeof original !== 'function' || original.__craftWrapped) continue;
        const wrapped = function (...args) { return record(original.apply(this, args)); };
        wrapped.__craftWrapped = true;
        AC.prototype[name] = wrapped;
      } catch (error) {}
    }
    try {
      const prototype = window.AudioNode && window.AudioNode.prototype;
      const original = prototype && prototype.connect;
      if (typeof original === 'function' && !original.__craftWrapped) {
        const wrapped = function (destination, ...rest) {
          try {
            const isParam = typeof window.AudioParam === 'function' && destination instanceof window.AudioParam;
            if (state.edges.length < MAX_EDGES) {
              state.edges.push({ from: kindOf(this), to: isParam ? 'AudioParam' : kindOf(destination), kind: isParam ? 'param' : 'audio' });
            } else {
              state.truncated = true;
            }
            record(this);
            if (!isParam) record(destination);
          } catch (error) {}
          return original.call(this, destination, ...rest);
        };
        wrapped.__craftWrapped = true;
        prototype.connect = wrapped;
      }
    } catch (error) {}
    try { Object.defineProperty(window, '__craftAnythingAudioGraph', { value: state, configurable: true }); } catch (error) {}
    return state;
  })();
  const snapshot = () => {
    const counts = {};
    const edges = [];
    const seen = new Set();
    let truncated = !!tracker.truncated;
    for (const node of tracker.nodes) {
      const kind = kindOf(node);
      counts[kind] = (counts[kind] || 0) + 1;
      if (Object.keys(counts).length > MAX_KINDS) truncated = true;
    }
    for (const edge of tracker.edges.slice(0, MAX_EDGES)) {
      const key = edge.from + '\u0000' + edge.to + '\u0000' + edge.kind;
      if (seen.has(key)) continue;
      seen.add(key);
      edges.push({ from: edge.from.slice(0, 64), to: edge.to.slice(0, 64), kind: edge.kind.slice(0, 32) });
    }
    return { graph: { node_counts: counts, edges: edges }, truncated: truncated };
  };
  const findAnalyser = () => {
    const named = window.__craftAnythingAudioAnalyser;
    if (named && typeof named.getByteFrequencyData === 'function') return named;
    const registry = window.__craftAnythingAudioAnalysers;
    if (Array.isArray(registry)) {
      for (const candidate of registry) {
        if (candidate && typeof candidate.getByteFrequencyData === 'function') return candidate;
      }
    }
    for (const node of tracker.nodes) {
      if (node && typeof node.getByteFrequencyData === 'function') return node;
    }
    return null;
  };
  const analyser = findAnalyser();
  const usable = analyser && typeof analyser.getByteFrequencyData === 'function' && typeof analyser.getByteTimeDomainData === 'function' && analyser.frequencyBinCount > 0;
  if (!usable) {
    const graph = snapshot();
    return { error: 'no_analyser', graph: graph.graph, graph_truncated: graph.truncated };
  }
  const mixer = (() => {
    for (const key of ['__craftAnythingMixer', '__craftAnythingMixerState', 'audioMixerState', '__audioMixerState', 'mixerState']) {
      let candidate;
      try { candidate = window[key]; } catch (error) { continue; }
      if (!candidate || typeof candidate !== 'object' || Array.isArray(candidate)) continue;
      try {
        const copy = JSON.parse(JSON.stringify(candidate));
        if (copy && typeof copy === 'object') return { source: key, state: copy };
      } catch (error) {}
    }
    const layers = [];
    let index = 0;
    for (const node of tracker.nodes) {
      if (layers.length >= MAX_LAYERS) break;
      try {
        if (node && node.gain && typeof node.gain.value === 'number') {
          layers.push({ id: String(node.__layerId || ('layer_' + index)), gain: node.gain.value, pan: 0 });
          index += 1;
        } else if (node && node.pan && typeof node.pan.value === 'number') {
          layers.push({ id: String(node.__layerId || ('layer_' + index)), gain: 1, pan: node.pan.value });
          index += 1;
        }
      } catch (error) {}
    }
    const title = typeof document !== 'undefined' && document ? String(document.title || '').slice(0, 120) : '';
    return { source: layers.length > 0 ? 'graph' : 'none', state: { scenario_id: title, layers: layers } };
  })();
  const fftSize = Math.max(1, Math.min(analyser.frequencyBinCount | 0, MAX_SPECTRUM));
  const frequency = new Uint8Array(analyser.frequencyBinCount);
  const waveform = new Uint8Array(analyser.fftSize);
  const sums = new Float64Array(fftSize);
  const rms = [];
  let samples = 0, bucketSum = 0, bucketCount = 0, bucketStart = 0;
  const started = performance.now();
  while (true) {
    const elapsed = performance.now() - started;
    if (elapsed >= DURATION_MS) break;
    analyser.getByteFrequencyData(frequency);
    analyser.getByteTimeDomainData(waveform);
    for (let index = 0; index < fftSize; index += 1) sums[index] += frequency[index] || 0;
    let squares = 0;
    for (let index = 0; index < waveform.length; index += 1) {
      const value = (waveform[index] - 128) / 128;
      squares += value * value;
    }
    bucketSum += waveform.length > 0 ? Math.sqrt(squares / waveform.length) : 0;
    bucketCount += 1;
    samples += 1;
    if (elapsed - bucketStart >= 1000) {
      if (rms.length < MAX_RMS) rms.push(bucketCount > 0 ? bucketSum / bucketCount : 0);
      bucketSum = 0;
      bucketCount = 0;
      bucketStart += 1000;
    }
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  if (bucketCount > 0 && rms.length < MAX_RMS) rms.push(bucketSum / bucketCount);
  const spectrum = [];
  for (let index = 0; index < fftSize; index += 1) spectrum.push(samples > 0 ? sums[index] / samples : 0);
  const graph = snapshot();
  return {
    graph: graph.graph,
    graph_truncated: graph.truncated,
    spectrum: spectrum,
    spectrum_truncated: analyser.frequencyBinCount > MAX_SPECTRUM,
    rms: rms,
    mixer_state: mixer.state,
    mixer_source: mixer.source,
    mixer_envelope: [],
    sample_rate: context.sampleRate,
    fft_size: analyser.fftSize,
    samples: samples,
    duration_ms: Math.round(performance.now() - started)
  };
})()
"#;

fn audio_capture_script(duration_ms: u64) -> String {
    AUDIO_CAPTURE_SCRIPT.replace("__DURATION_MS__", &duration_ms.to_string())
}

/// Validate and bound the page's audio report. A `null` result means the page has no
/// AudioContext; `no_analyser` means it has a graph but nothing to read a spectrum from.
fn parse_audio_capture_reply(reply: &Value) -> Result<Value, ToolError> {
    let value = reply.get("result").and_then(|result| result.get("value")).cloned().unwrap_or(Value::Null);
    if value.is_null() {
        return Err(ToolError::new(ErrorCode::FeatureMissing, "the page has no AudioContext to listen to")
            .with_remediation(
                "Play the page's audio first: the capture found no AudioContext. If the sound plays in a cross-origin iframe, pass that frame's target_id.",
            )
            .with_details(json!({"reason": "no_audio_graph"})));
    }
    if !value.is_object() {
        return Err(ToolError::new(ErrorCode::GarbledOutput, "the page's audio report wasn't an object").with_remediation("Reload the page and try again."));
    }
    if value.get("error").and_then(Value::as_str) == Some("no_analyser") {
        return Err(ToolError::new(ErrorCode::FeatureMissing, "the page's audio graph has no AnalyserNode to read")
            .with_remediation("Expose the page's analyser on window.__craftAnythingAudioAnalyser, or add an AnalyserNode to the graph, then capture again.")
            .with_details(json!({"reason": "no_analyser"})));
    }

    let spectrum_all = value.get("spectrum").and_then(Value::as_array).map(Vec::len).unwrap_or(0);
    let mut spectrum = number_list(&value, "spectrum")?;
    spectrum.truncate(MAX_CAPTURE_SPECTRUM);
    let mut rms = number_list(&value, "rms")?;
    rms.truncate(MAX_CAPTURE_RMS);

    let mut graph = value.get("graph").filter(|graph| graph.is_object()).cloned().unwrap_or_else(|| json!({"node_counts": {}, "edges": []}));
    let mut graph_truncated = value.get("graph_truncated").and_then(Value::as_bool).unwrap_or(false);
    if let Some(edges) = graph.get_mut("edges").and_then(Value::as_array_mut)
        && edges.len() > MAX_CAPTURE_EDGES
    {
        edges.truncate(MAX_CAPTURE_EDGES);
        graph_truncated = true;
    }
    if let Some(counts) = graph.get_mut("node_counts").and_then(Value::as_object_mut) {
        while counts.len() > MAX_CAPTURE_KINDS {
            let Some(kind) = counts.keys().next().cloned() else { break };
            counts.remove(&kind);
            graph_truncated = true;
        }
    }

    let mixer = value.get("mixer_state").filter(|state| state.is_object()).cloned().unwrap_or_else(|| json!({}));
    let mixer_bytes = serde_json::to_string(&mixer).map(|text| text.len()).unwrap_or(0);
    if mixer_bytes > MAX_MIXER_BYTES {
        return Err(ToolError::new(ErrorCode::OverLimit, format!("the page's mixer state is {mixer_bytes} bytes; at most {MAX_MIXER_BYTES} are kept"))
            .with_remediation("Capture a page whose mixer state is smaller, or expose only the fields the comparison needs."));
    }

    let sample_rate = value.get("sample_rate").and_then(Value::as_u64).filter(|rate| (1..=384_000).contains(rate)).unwrap_or(48_000);
    let fft_size = value.get("fft_size").and_then(Value::as_u64).unwrap_or(0);
    let samples = value.get("samples").and_then(Value::as_u64).unwrap_or(0);
    Ok(json!({
        "graph": graph,
        "graph_truncated": graph_truncated,
        "spectrum": spectrum,
        "spectrum_truncated": value.get("spectrum_truncated").and_then(Value::as_bool).unwrap_or(false) || spectrum_all > MAX_CAPTURE_SPECTRUM,
        "rms": rms,
        "mixer_state": mixer,
        "mixer_source": value.get("mixer_source").and_then(Value::as_str).unwrap_or("none"),
        "mixer_envelope": [],
        "sample_rate": sample_rate,
        "fft_size": fft_size,
        "samples": samples,
    }))
}

/// An outline of the requests the page made while the capture listened: how many, their origins
/// and a bounded endpoint list. Query values are dropped and declared secrets are redacted.
///
/// `enabled` is whether `Network.enable` succeeded; when it didn't, or when the event queue
/// overflowed, the outline says so instead of looking like a page that made no requests.
fn audio_network_outline(client: &mut CdpClient, secrets: &mut Secrets, enabled: bool) -> Value {
    use std::collections::{BTreeMap, BTreeSet};
    let dropped = client.dropped_events();
    let events = client.take_events();
    let mut count = 0u64;
    let mut origins: BTreeMap<String, u64> = BTreeMap::new();
    let mut endpoints: Vec<Value> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut truncated = false;
    for event in &events {
        if event.method != "Network.requestWillBeSent" {
            continue;
        }
        count = count.saturating_add(1);
        let url = secrets.text(&web_common::sanitize_url(event.params.pointer("/request/url").and_then(Value::as_str).unwrap_or_default()));
        let (origin, path) = split_origin_path(&url);
        if !origin.is_empty() {
            let entry = origins.entry(origin.clone()).or_insert(0);
            *entry = entry.saturating_add(1);
        }
        let method = event.params.pointer("/request/method").and_then(Value::as_str).unwrap_or("GET");
        let endpoint = format!("{origin}{path}");
        if seen.insert(format!("{method} {endpoint}")) {
            if endpoints.len() < MAX_OUTLINE_ENDPOINTS {
                endpoints.push(json!({"method": method, "endpoint": endpoint}));
            } else {
                truncated = true;
            }
        }
    }
    let mut unavailable: Vec<&str> = Vec::new();
    if !enabled {
        unavailable.push("network (the browser refused Network.enable)");
    }
    if dropped > 0 {
        unavailable.push("some events (the browser event queue overflowed)");
    }
    json!({"request_count": count, "origins": origins, "endpoints": endpoints, "truncated": truncated, "dropped": dropped, "unavailable": unavailable})
}

/// Origin and path of a URL, with the query and fragment dropped. A relative or malformed URL
/// keeps no origin.
fn split_origin_path(url: &str) -> (String, String) {
    let url = url.trim();
    let Some(scheme_end) = url.find("://") else {
        return (String::new(), url.split(['?', '#']).next().unwrap_or("").to_string());
    };
    let authority_start = scheme_end + 3;
    let rest = url.get(authority_start..).unwrap_or("");
    let (authority, tail) = match rest.find(['/', '?', '#']) {
        Some(at) => rest.split_at(at),
        None => (rest, ""),
    };
    let origin = format!("{}{}", url.get(..authority_start).unwrap_or(""), authority);
    (origin, tail.split(['?', '#']).next().unwrap_or("").to_string())
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)).unwrap_or(0)
}

/// At most `max` characters, with an ellipsis when the text was cut.
fn clip_text(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max).collect();
    out.push('…');
    out
}

// ---------------------------------------------------------------------------------------------
// web_walkthrough_record (active, consent-gated)

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum WalkthroughAction {
    Goto {
        url: String,
        #[serde(default)]
        wait_ms: Option<u64>,
    },
    Click {
        #[serde(flatten)]
        at: Locator,
    },
    Fill {
        #[serde(flatten)]
        at: Locator,
        value: String,
    },
    Press {
        #[serde(flatten)]
        at: Locator,
        key: String,
    },
    Select {
        #[serde(flatten)]
        at: Locator,
        value: String,
    },
    Wait {
        ms: u64,
    },
}

/// The element a step acts on: exactly one of a CSS `selector`, a `test_id` (data-testid), or a
/// `role` with its accessible `name` (what a screen reader says, e.g. role button, name "Sign in").
#[derive(Debug, Deserialize, JsonSchema)]
pub struct Locator {
    #[serde(default)]
    pub selector: Option<String>,
    #[serde(default)]
    pub test_id: Option<String>,
    /// ARIA role: button, link, textbox, checkbox, combobox, tab, menuitem…
    #[serde(default)]
    pub role: Option<String>,
    /// The exact accessible name to match with `role`.
    #[serde(default)]
    pub name: Option<String>,
}

impl Locator {
    /// The DOM node it names, or a plain error saying what wasn't found. Its form is judged by
    /// [`Locator::check`], before this.
    fn node(&self, client: &mut CdpClient, dl: &Deadline) -> Result<u64, ToolError> {
        match (&self.selector, &self.test_id, &self.role, &self.name) {
            (Some(css), None, None, None) => query_node(client, css, dl),
            (None, Some(id), None, None) => query_node(client, &format!("[data-testid=\"{}\"]", id.replace('\\', "\\\\").replace('"', "\\\"")), dl),
            (None, None, Some(role), Some(name)) => role_node(client, role, name, dl),
            _ => Err(ToolError::invalid(script_check::LOCATOR_SHAPE)),
        }
    }
}

/// The first element in the accessibility tree with this role and exact accessible name.
fn role_node(client: &mut CdpClient, role: &str, name: &str, dl: &Deadline) -> Result<u64, ToolError> {
    let doc = client.call("DOM.getDocument", json!({"depth": 0}), None, dl)?;
    let root = doc
        .pointer("/root/nodeId")
        .and_then(Value::as_u64)
        .ok_or_else(|| ToolError::new(craft_anything_evidence::ErrorCode::ProgramFailed, "no document root"))?;
    let _ = client.call("Accessibility.enable", json!({}), None, dl);
    let found = client.call("Accessibility.queryAXTree", json!({"nodeId": root, "accessibleName": name, "role": role}), None, dl)?;
    let missing = || ToolError::new(craft_anything_evidence::ErrorCode::NotFound, format!("no {role} named {name:?} on the page"));
    let backend = found
        .get("nodes")
        .and_then(Value::as_array)
        .and_then(|nodes| nodes.iter().filter(|n| n.get("ignored") != Some(&Value::Bool(true))).find_map(|n| n.get("backendDOMNodeId").and_then(Value::as_u64)))
        .ok_or_else(missing)?;
    let pushed = client.call("DOM.pushNodesByBackendIdsToFrontend", json!({"backendNodeIds": [backend]}), None, dl)?;
    pushed.pointer("/nodeIds/0").and_then(Value::as_u64).filter(|n| *n != 0).ok_or_else(missing)
}

/// One scripted step: an action and, optionally, your name for it (echoed in the result).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct WalkthroughStep {
    #[serde(default)]
    pub step_id: Option<String>,
    #[serde(flatten)]
    pub action: WalkthroughAction,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WalkthroughParams {
    #[serde(default)]
    pub cdp_endpoint: Option<String>,
    /// Launch a located Chromium (default when no endpoint is given).
    #[serde(default)]
    pub launch: bool,
    #[serde(default = "yes")]
    pub headless: bool,
    #[serde(default)]
    pub no_sandbox: bool,
    #[serde(default)]
    pub target_id: Option<String>,
    /// The scripted actions, run in order (1..=200).
    pub actions: Vec<WalkthroughStep>,
    /// Capture a screenshot after each step.
    #[serde(default)]
    pub screenshot_each_step: bool,
    /// Record network request outlines during the run.
    #[serde(default = "yes")]
    pub capture_network: bool,
    /// Values to redact wherever they appear, in every encoding.
    #[serde(default)]
    pub hidden_values: Vec<String>,
    /// Per-action deadline in milliseconds.
    #[serde(default = "d_action_ms")]
    pub action_timeout_ms: u64,
    #[serde(default = "d_walkthrough_timeout")]
    pub timeout_secs: u64,
    /// Fixed browser settings for the run, so two runs of the same steps see the same page.
    #[serde(default)]
    pub environment: WalkthroughEnvironment,
    /// Network content to keep besides request outlines (all off by default; needs capture_network).
    #[serde(default)]
    pub keep_bodies: NetworkContent,
    /// What to record after each step and at the end of the run (besides each step's URL).
    #[serde(default)]
    pub capture: WalkthroughCapture,
    /// Cookies and storage to start with (e.g. to explore the signed-in side of your own site).
    /// Every value given here is treated as a secret and redacted wherever it shows up.
    /// To seed from a browser's CDP `Storage.getCookies` export (via a Page session, e.g.
    /// `await page.cdp("Storage.getCookies")` — not `task.cdp`, which only takes Target/Browser
    /// commands), filter that `cookies` array to the site's domain first (at most 200) and map
    /// each entry per `StartingCookie`. Never log or store the exported `value`s: the run
    /// reports only which names were set.
    #[serde(default)]
    pub storage: StartingStorage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Artifact {
    Screenshot,
    /// The page's HTML, secrets redacted (256 KiB per step, 1 MiB at the end).
    Dom,
    /// Roles and names, as a screen reader sees the page.
    Accessibility,
    /// The tab's navigation history.
    History,
    /// local/session storage key names and cookie names for the current origin (never values).
    StorageKeys,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct WalkthroughCapture {
    pub after_each_step: Vec<Artifact>,
    pub at_end: Vec<Artifact>,
    /// Event families to record over the run, each event tagged with the step it happened in.
    pub events: Vec<EventFamily>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EventFamily {
    /// Console messages (text redacted, capped).
    Console,
    /// Uncaught exceptions on the page.
    PageErrors,
    /// WebSocket connections and frames (text frames redacted, capped).
    Websockets,
    /// Frames attached, navigated and detached.
    Frames,
    /// Web, shared and service workers started and stopped.
    Workers,
    /// Windows the page opened.
    Popups,
    /// Downloads the page started (they are blocked, never saved).
    Downloads,
}

/// Most events a run keeps; the rest are counted as dropped.
const MAX_WALKTHROUGH_EVENTS: usize = 20_000;

/// Text kept from one console message, exception or WebSocket frame.
const MAX_EVENT_TEXT: usize = 2048;

fn capped(text: &str, secrets: &mut Secrets) -> String {
    let t = secrets.text(text);
    let mut end = t.len().min(MAX_EVENT_TEXT);
    while !t.is_char_boundary(end) {
        end -= 1;
    }
    t.get(..end).unwrap_or_default().to_string()
}

/// One event as a row of a requested family, or None. `targets` remembers which created targets
/// were workers or popups, so their end can be named too.
fn event_row(
    families: &std::collections::BTreeSet<EventFamily>,
    ev: &cdp::CdpEvent,
    targets: &mut std::collections::BTreeMap<String, &'static str>,
    secrets: &mut Secrets,
) -> Option<Value> {
    let p = &ev.params;
    let s = |v: Option<&Value>| v.and_then(Value::as_str).unwrap_or_default().to_string();
    let has = |f: EventFamily| families.contains(&f);
    let row = match ev.method.as_str() {
        "Runtime.consoleAPICalled" if has(EventFamily::Console) => {
            let text: Vec<&str> =
                p.get("args").and_then(Value::as_array).into_iter().flatten().filter_map(|a| a.get("value").and_then(Value::as_str)).collect();
            json!({"kind": "console", "level": s(p.get("type")), "text": capped(&text.join(" "), secrets)})
        }
        "Log.entryAdded" if has(EventFamily::Console) => {
            let e = p.get("entry");
            json!({"kind": "console", "level": s(e.and_then(|e| e.get("level"))), "text": capped(&s(e.and_then(|e| e.get("text"))), secrets)})
        }
        "Runtime.exceptionThrown" if has(EventFamily::PageErrors) => {
            let d = p.get("exceptionDetails");
            let msg = d.and_then(|d| d.pointer("/exception/description")).or_else(|| d.and_then(|d| d.get("text")));
            json!({"kind": "page-error", "message": capped(&s(msg), secrets)})
        }
        "Network.webSocketCreated" if has(EventFamily::Websockets) => {
            json!({"kind": "websocket-opened", "request_id": s(p.get("requestId")), "url": secrets.text(&web_common::sanitize_url(&s(p.get("url"))))})
        }
        "Network.webSocketClosed" if has(EventFamily::Websockets) => json!({"kind": "websocket-closed", "request_id": s(p.get("requestId"))}),
        "Network.webSocketFrameSent" | "Network.webSocketFrameReceived" if has(EventFamily::Websockets) => {
            let f = p.get("response");
            let opcode = f.and_then(|f| f.get("opcode")).and_then(Value::as_u64);
            let payload = s(f.and_then(|f| f.get("payloadData")));
            let mut row = json!({
                "kind": if ev.method.ends_with("Sent") { "websocket-frame-sent" } else { "websocket-frame-received" },
                "request_id": s(p.get("requestId")),
                "opcode": opcode,
                "payload_bytes": payload.len(),
            });
            if opcode == Some(1) {
                row["text"] = json!(capped(&payload, secrets));
            }
            row
        }
        "Page.frameAttached" if has(EventFamily::Frames) => {
            json!({"kind": "frame-attached", "frame_id": s(p.get("frameId")), "parent_frame_id": s(p.get("parentFrameId"))})
        }
        "Page.frameNavigated" if has(EventFamily::Frames) => {
            let f = p.get("frame");
            json!({"kind": "frame-navigated", "frame_id": s(f.and_then(|f| f.get("id"))), "url": secrets.text(&web_common::sanitize_url(&s(f.and_then(|f| f.get("url")))))})
        }
        "Page.frameDetached" if has(EventFamily::Frames) => json!({"kind": "frame-detached", "frame_id": s(p.get("frameId"))}),
        "Target.targetCreated" => {
            let info = p.get("targetInfo");
            let kind = s(info.and_then(|i| i.get("type")));
            let opened = info.and_then(|i| i.get("openerId")).is_some();
            let family = match kind.as_str() {
                "worker" | "shared_worker" | "service_worker" if has(EventFamily::Workers) => "worker",
                "page" if opened && has(EventFamily::Popups) => "popup",
                _ => return None,
            };
            if targets.len() < 1000 {
                targets.insert(s(info.and_then(|i| i.get("targetId"))), family);
            }
            json!({"kind": format!("{family}-opened"), "type": kind, "url": secrets.text(&web_common::sanitize_url(&s(info.and_then(|i| i.get("url")))))})
        }
        "Target.targetDestroyed" => {
            let family = targets.remove(&s(p.get("targetId")))?;
            json!({"kind": format!("{family}-closed")})
        }
        "Page.downloadWillBegin" if has(EventFamily::Downloads) => {
            json!({"kind": "download-blocked", "url": secrets.text(&web_common::sanitize_url(&s(p.get("url")))), "suggested_filename": secrets.text(&s(p.get("suggestedFilename")))})
        }
        _ => return None,
    };
    Some(row)
}

/// Record the requested artifacts now. Each is either its value or `{"state": "unavailable"}`.
fn capture_artifacts(client: &mut CdpClient, kinds: &[Artifact], dom_cap: usize, secrets: &mut Secrets, dl: &Deadline) -> Value {
    let unavailable = || json!({"state": "unavailable"});
    let kinds: std::collections::BTreeSet<Artifact> = kinds.iter().copied().collect();
    let mut out = serde_json::Map::new();
    let history = client.call("Page.getNavigationHistory", json!({}), None, dl).ok();
    let current_url = history.as_ref().and_then(|h| {
        let idx = h.get("currentIndex").and_then(Value::as_u64).unwrap_or(0) as usize;
        h.get("entries").and_then(Value::as_array).and_then(|a| a.get(idx)).and_then(|e| e.get("url")).and_then(Value::as_str).map(String::from)
    });
    for kind in kinds {
        let v = match kind {
            Artifact::Screenshot => client
                .call("Page.captureScreenshot", json!({"format": "png"}), None, dl)
                .ok()
                .and_then(|shot| shot.get("data").and_then(Value::as_str).and_then(|d| web_common::b64_decode(d).ok()))
                .and_then(|bytes| web_common::png_artifact(&bytes).ok())
                .unwrap_or_else(unavailable),
            Artifact::Dom => client
                .call("DOM.getDocument", json!({"depth": 0}), None, dl)
                .ok()
                .and_then(|d| d.pointer("/root/nodeId").and_then(Value::as_u64))
                .and_then(|root| client.call("DOM.getOuterHTML", json!({"nodeId": root}), None, dl).ok())
                .and_then(|h| h.get("outerHTML").and_then(Value::as_str).map(|html| web_common::kept_text(html.as_bytes(), secrets, dom_cap)))
                .unwrap_or_else(unavailable),
            Artifact::Accessibility => {
                let mut tree = collect_accessibility(client, dl);
                for n in tree.get_mut("nodes").and_then(Value::as_array_mut).into_iter().flatten() {
                    if let Some(name) = n.get("name").and_then(Value::as_str).map(|s| secrets.text(s)) {
                        n["name"] = json!(name);
                    }
                }
                tree
            }
            Artifact::History => match &history {
                Some(h) => {
                    let entries: Vec<Value> = h
                        .get("entries")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .take(500)
                        .map(|e| json!({"url": secrets.text(&web_common::sanitize_url(e.get("url").and_then(Value::as_str).unwrap_or_default()))}))
                        .collect();
                    json!({"current_index": h.get("currentIndex"), "entries": entries})
                }
                None => unavailable(),
            },
            Artifact::StorageKeys => match current_url.as_deref().and_then(web_common::origin_of) {
                Some(origin) => {
                    let mut keys = collect_storage_keys(client, &std::iter::once(origin.clone()).collect::<std::collections::BTreeSet<_>>(), dl);
                    let cookies: Vec<String> = client
                        .call("Network.getCookies", json!({"urls": [current_url]}), None, dl)
                        .ok()
                        .and_then(|c| c.get("cookies").and_then(Value::as_array).cloned())
                        .into_iter()
                        .flatten()
                        .filter_map(|c| c.get("name").and_then(Value::as_str).map(String::from))
                        .take(500)
                        .collect();
                    keys["origin"] = json!(origin);
                    keys["cookie_names"] = json!(cookies);
                    keys
                }
                None => unavailable(),
            },
        };
        let name = match kind {
            Artifact::Screenshot => "screenshot",
            Artifact::Dom => "dom",
            Artifact::Accessibility => "accessibility",
            Artifact::History => "history",
            Artifact::StorageKeys => "storage_keys",
        };
        out.insert(name.into(), v);
    }
    Value::Object(out)
}

/// Cookies and storage seeded before a walkthrough run.
///
/// An ego `page.cdp("Storage.getCookies")` result (`{"cookies": [...]}` of CDP
/// `Network.Cookie`) becomes `{"cookies": [...], "local_storage": [], "session_storage": []}`
/// by this exact mapping, entry by entry (names only in every log; values never logged):
///
/// | export field | `storage.cookies` field | rule |
/// |---|---|---|
/// | `name` | `name` | as-is |
/// | `value` | `value` | as-is (a secret: redacted in every output) |
/// | `domain` + `path` + `sourceScheme`/`secure` + `sourcePort` | `url` | `scheme://host[:port]path`: host is `domain` without a leading `.`; scheme is `https` when `sourceScheme` is `Secure` or `secure` is true, else `http`; a non-default `sourcePort` is kept, the default for the scheme is omitted; an empty `path` becomes `/` |
/// | `httpOnly` | `http_only` | boolean as-is |
/// | `secure` | `secure` | boolean as-is |
/// | `sameSite` | `same_site` | `Strict`/`Lax`/`None` as-is; omit when absent |
/// | `expires`, `session`, `priority`, `size` | — | dropped: the cookie becomes session-scoped for the run |
///
/// A domain cookie (`.example.com`) becomes a host cookie for `example.com` (the leading dot
/// is stripped); `Network.setCookie` with a `url` sets host-only cookies. local/session storage
/// is a separate CDP domain (`DOMStorage`), not part of `Storage.getCookies`, and is seeded
/// through `local_storage`/`session_storage` instead. Filter the export to the site before
/// seeding: `storage.cookies` caps at 200 entries.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct StartingStorage {
    pub cookies: Vec<StartingCookie>,
    pub local_storage: Vec<OriginEntries>,
    pub session_storage: Vec<OriginEntries>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StartingCookie {
    /// The http(s) URL the cookie belongs to (from an export's `domain`, `path`, `sourceScheme`
    /// or `secure`, and non-default `sourcePort`; see `StartingStorage`).
    pub url: String,
    /// The cookie name (export `name`).
    pub name: String,
    /// The cookie value (export `value`; a secret, redacted in every output).
    pub value: String,
    /// HttpOnly flag (export `httpOnly`).
    #[serde(default)]
    pub http_only: bool,
    /// Secure flag (export `secure`).
    #[serde(default)]
    pub secure: bool,
    /// Strict, Lax or None (export `sameSite`; omit when absent).
    #[serde(default)]
    pub same_site: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OriginEntries {
    /// The http(s) origin, e.g. https://app.example.com
    pub origin: String,
    /// Key → value.
    pub entries: std::collections::BTreeMap<String, String>,
}

impl StartingStorage {
    fn check(&self) -> Result<(), ToolError> {
        let web = |u: &str| u.starts_with("http://") || u.starts_with("https://");
        if self.cookies.len() > 200 {
            return Err(ToolError::invalid("storage.cookies: at most 200"));
        }
        for c in &self.cookies {
            if !web(&c.url) || c.name.is_empty() {
                return Err(ToolError::invalid("storage.cookies: each needs an http(s) url and a name"));
            }
            if c.same_site.as_deref().is_some_and(|s| !matches!(s, "Strict" | "Lax" | "None")) {
                return Err(ToolError::invalid("storage.cookies: same_site is Strict, Lax or None"));
            }
        }
        let areas = self.local_storage.iter().chain(&self.session_storage);
        if areas.clone().map(|o| o.entries.len()).sum::<usize>() > 1000 {
            return Err(ToolError::invalid("storage: at most 1000 local/session entries"));
        }
        if areas.clone().any(|o| !web(&o.origin)) {
            return Err(ToolError::invalid("storage: each origin must be http(s)"));
        }
        Ok(())
    }

    /// Every value, so the run can redact them.
    fn values(&self) -> impl Iterator<Item = &String> {
        self.cookies.iter().map(|c| &c.value).chain(self.local_storage.iter().chain(&self.session_storage).flat_map(|o| o.entries.values()))
    }

    /// Set everything; reports how many of each were set (names only, never values).
    fn apply(&self, client: &mut CdpClient, dl: &Deadline) -> Value {
        let mut cookies_set = Vec::new();
        let mut failed = Vec::new();
        for c in &self.cookies {
            let mut params = json!({"url": c.url, "name": c.name, "value": c.value, "httpOnly": c.http_only, "secure": c.secure});
            if let Some(ss) = &c.same_site {
                params["sameSite"] = json!(ss);
            }
            match client.call("Network.setCookie", params, None, dl) {
                Ok(r) if r.get("success") != Some(&Value::Bool(false)) => cookies_set.push(c.name.clone()),
                _ => failed.push(format!("cookie {}", c.name)),
            }
        }
        let _ = client.call("DOMStorage.enable", json!({}), None, dl);
        let mut entries_set = 0usize;
        for (local, areas) in [(true, &self.local_storage), (false, &self.session_storage)] {
            for o in areas {
                for (k, v) in &o.entries {
                    let id = json!({"securityOrigin": o.origin, "isLocalStorage": local});
                    match client.call("DOMStorage.setDOMStorageItem", json!({"storageId": id, "key": k, "value": v}), None, dl) {
                        Ok(_) => entries_set += 1,
                        Err(_) => failed.push(format!("{} {} {k}", if local { "localStorage" } else { "sessionStorage" }, o.origin)),
                    }
                }
            }
        }
        json!({"cookies_set": cookies_set, "storage_entries_set": entries_set, "failed": failed})
    }
}

/// Which network values a run keeps. Kept text has declared secrets redacted; credential headers
/// are always redacted.
#[derive(Debug, Default, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct NetworkContent {
    pub request_body: bool,
    pub response_body: bool,
    pub header_values: bool,
}

impl NetworkContent {
    fn any(self) -> bool {
        self.request_body || self.response_body || self.header_values
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ColorScheme {
    #[default]
    Light,
    Dark,
    NoPreference,
}

/// Viewport, locale, time zone and media settings applied before the first step.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields, default)]
pub struct WalkthroughEnvironment {
    /// Viewport width in CSS pixels (1..=7680).
    pub width: u32,
    /// Viewport height in CSS pixels (1..=7680).
    pub height: u32,
    /// Device pixel ratio (0.5..=4).
    pub device_scale_factor: f64,
    /// BCP 47 locale, e.g. en-US.
    pub locale: String,
    /// IANA time zone, e.g. UTC or Europe/Berlin.
    pub timezone: String,
    pub color_scheme: ColorScheme,
    /// Ask pages to reduce motion (prefers-reduced-motion: reduce).
    pub reduced_motion: bool,
    /// Requests skip service workers, so an offline cache can't hide the network.
    pub bypass_service_workers: bool,
}

impl Default for WalkthroughEnvironment {
    fn default() -> Self {
        WalkthroughEnvironment {
            width: 1280,
            height: 720,
            device_scale_factor: 1.0,
            locale: "en-US".into(),
            timezone: "UTC".into(),
            color_scheme: ColorScheme::Light,
            reduced_motion: true,
            bypass_service_workers: true,
        }
    }
}

impl WalkthroughEnvironment {
    fn check(&self) -> Result<(), ToolError> {
        if !(1..=7680).contains(&self.width) || !(1..=7680).contains(&self.height) {
            return Err(ToolError::invalid("environment width and height must be 1..=7680"));
        }
        if !(0.5..=4.0).contains(&self.device_scale_factor) {
            return Err(ToolError::invalid("environment device_scale_factor must be 0.5..=4"));
        }
        let locale_ok =
            !self.locale.is_empty() && self.locale.len() <= 35 && self.locale.split('-').all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric()));
        if !locale_ok {
            return Err(ToolError::invalid(format!("environment locale {:?} isn't a BCP 47 tag like en-US", self.locale)));
        }
        let tz_ok = !self.timezone.is_empty()
            && self.timezone.len() <= 64
            && !self.timezone.contains("..")
            && self.timezone.chars().all(|c| c.is_ascii_alphanumeric() || "_+-/".contains(c));
        if !tz_ok {
            return Err(ToolError::invalid(format!("environment timezone {:?} isn't an IANA name like Europe/Berlin", self.timezone)));
        }
        Ok(())
    }

    /// Apply every setting; each one reports `applied` or `unavailable` (the browser refused it).
    fn apply(&self, client: &mut CdpClient, dl: &Deadline) -> Value {
        let scheme = match self.color_scheme {
            ColorScheme::Light => "light",
            ColorScheme::Dark => "dark",
            ColorScheme::NoPreference => "no-preference",
        };
        let motion = if self.reduced_motion { "reduce" } else { "no-preference" };
        let calls = [
            (
                "viewport",
                "Emulation.setDeviceMetricsOverride",
                json!({"width": self.width, "height": self.height, "deviceScaleFactor": self.device_scale_factor, "mobile": false}),
            ),
            ("locale", "Emulation.setLocaleOverride", json!({"locale": self.locale})),
            ("timezone", "Emulation.setTimezoneOverride", json!({"timezoneId": self.timezone})),
            (
                "media",
                "Emulation.setEmulatedMedia",
                json!({"features": [{"name": "prefers-color-scheme", "value": scheme}, {"name": "prefers-reduced-motion", "value": motion}]}),
            ),
            ("service_workers", "Network.setBypassServiceWorker", json!({"bypass": self.bypass_service_workers})),
        ];
        let settings: serde_json::Map<String, Value> =
            calls.into_iter().map(|(k, m, p)| (k.to_string(), json!(if client.call(m, p, None, dl).is_ok() { "applied" } else { "unavailable" }))).collect();
        json!({"requested": self, "settings": settings})
    }
}

fn d_action_ms() -> u64 {
    15_000
}
fn d_walkthrough_timeout() -> u64 {
    180
}

fn web_walkthrough_record(ctx: &mut Ctx<'_>, params: Value) -> Result<ToolOutput, ToolError> {
    let p: WalkthroughParams = parse_params(params)?;
    p.check_settings()?;
    let dl = deadline(ctx, p.timeout_secs)?;
    // Default to launching when no endpoint is given.
    let launch = p.launch || p.cdp_endpoint.is_none();
    let browser = resolve_browser(ctx, p.cdp_endpoint.as_deref(), launch, p.headless, p.no_sandbox, &dl)?;
    let targets = cdp::json_list(&browser.endpoint, &dl)?;
    let target = web_common::pick_target(&targets, p.target_id.as_deref(), Some("page"))?;
    let start_url = web_common::sanitize_url(target.get("url").and_then(Value::as_str).unwrap_or_default());
    let mut client = web_common::connect_target(&browser, target, PAGE_MAX_MESSAGE, &dl)?;
    let declared: Vec<String> = p.hidden_values.iter().chain(p.storage.values()).cloned().collect();
    let mut secrets = Secrets::new(&declared);
    let _ = client.call("Page.enable", json!({}), None, &dl);
    let _ = client.call("DOM.enable", json!({}), None, &dl);
    let _ = client.call("Runtime.enable", json!({}), None, &dl);
    if p.capture_network {
        let _ = client.call("Network.enable", json!({}), None, &dl);
    }
    let families: std::collections::BTreeSet<EventFamily> = p.capture.events.iter().copied().collect();
    let mut families_unavailable: Vec<&str> = Vec::new();
    let mut enable = |client: &mut CdpClient, method: &str, params: Value, what: &'static str| {
        if client.call(method, params, None, &dl).is_err() {
            families_unavailable.push(what);
        }
    };
    if families.contains(&EventFamily::Console) {
        enable(&mut client, "Log.enable", json!({}), "console (browser log)");
    }
    if families.contains(&EventFamily::Websockets) && !p.capture_network {
        enable(&mut client, "Network.enable", json!({}), "websockets");
    }
    if families.contains(&EventFamily::Workers) || families.contains(&EventFamily::Popups) {
        enable(&mut client, "Target.setDiscoverTargets", json!({"discover": true}), "workers and popups");
    }
    if families.contains(&EventFamily::Downloads) {
        enable(&mut client, "Page.setDownloadBehavior", json!({"behavior": "deny"}), "downloads (they may not be blocked)");
    }
    let environment = p.environment.apply(&mut client, &dl);
    let storage = p.storage.apply(&mut client, &dl);

    // Every event of the run, with the step it arrived in (None before the first step).
    let mut events: Vec<(Option<usize>, cdp::CdpEvent)> = Vec::new();
    let mut events_dropped = 0u64;
    let mut keep = |client: &mut CdpClient, step: Option<usize>, events: &mut Vec<(Option<usize>, cdp::CdpEvent)>| {
        for ev in client.take_events() {
            if events.len() < MAX_WALKTHROUGH_EVENTS {
                events.push((step, ev));
            } else {
                events_dropped += 1;
            }
        }
    };
    keep(&mut client, None, &mut events);

    let mut steps: Vec<Value> = Vec::new();
    let mut aborted: Option<String> = None;
    // The evidence names the site the run actually examined (the last page a step left
    // the tab on), not the tab it started on: `start_url` is kept in the result, but
    // naming it as the website misreports every run that navigates elsewhere.
    let mut last_url = start_url.clone();
    for (i, WalkthroughStep { step_id, action }) in p.actions.iter().enumerate() {
        if dl.expired() {
            aborted = Some("the overall deadline passed".into());
            break;
        }
        ctx.report(Some(i as f32 / p.actions.len() as f32), format!("walkthrough step {}", i + 1));
        let step_dl = Deadline::new(Duration::from_millis(p.action_timeout_ms.clamp(1, 120_000)), ctx.cancel.clone());
        let outcome = run_action(&mut client, action, &mut secrets, &step_dl);
        let mut step = json!({"index": i, "step_id": step_id.as_deref().map(|id| secrets.text(id)), "action": action_name(action), "status": if outcome.is_ok() { "ok" } else { "error" }});
        if let Err(e) = &outcome {
            step["error"] = json!(e.message);
        }
        // Current URL after the step.
        if let Ok(hist) = client.call("Page.getNavigationHistory", json!({}), None, &step_dl) {
            let idx = hist.get("currentIndex").and_then(Value::as_u64).unwrap_or(0) as usize;
            if let Some(entry) = hist.get("entries").and_then(Value::as_array).and_then(|a| a.get(idx)) {
                let page_url = secrets.text(&web_common::sanitize_url(entry.get("url").and_then(Value::as_str).unwrap_or_default()));
                last_url.clone_from(&page_url);
                step["url"] = json!(page_url);
            }
        }
        if p.screenshot_each_step
            && let Ok(shot) = client.call("Page.captureScreenshot", json!({"format": "png"}), None, &step_dl)
            && let Some(data) = shot.get("data").and_then(Value::as_str)
            && let Ok(bytes) = web_common::b64_decode(data)
            && let Ok(art) = web_common::png_artifact(&bytes)
        {
            step["screenshot"] = art;
        }
        if !p.capture.after_each_step.is_empty() {
            step["artifacts"] = capture_artifacts(&mut client, &p.capture.after_each_step, 256 * 1024, &mut secrets, &step_dl);
        }
        steps.push(step);
        keep(&mut client, Some(i), &mut events);
        if outcome.is_err() {
            aborted = Some(format!("step {} failed", i + 1));
            break;
        }
    }

    let final_artifacts =
        if p.capture.at_end.is_empty() { Value::Null } else { capture_artifacts(&mut client, &p.capture.at_end, 1024 * 1024, &mut secrets, &dl) };
    keep(&mut client, steps.len().checked_sub(1), &mut events);
    // Network capture over the whole run.
    let network = if p.capture_network {
        let all: Vec<cdp::CdpEvent> = events.iter().map(|(_, e)| e.clone()).collect();
        let mut dropped = 0u64;
        collect_network(&mut client, &all, false, p.keep_bodies, &mut secrets, &dl, &mut dropped)
    } else {
        Value::Null
    };
    let event_rows = if families.is_empty() {
        Value::Null
    } else {
        let mut targets = std::collections::BTreeMap::new();
        let items: Vec<Value> = events
            .iter()
            .filter_map(|(step, ev)| {
                event_row(&families, ev, &mut targets, &mut secrets).map(|mut row| {
                    row["step_index"] = json!(step);
                    row
                })
            })
            .enumerate()
            .map(|(n, mut row)| {
                row["sequence"] = json!(n + 1);
                row
            })
            .collect();
        json!({"families": families, "items": items, "dropped": events_dropped, "unavailable": families_unavailable})
    };
    cleanup(&mut client, &dl);

    let ok_steps = steps.iter().filter(|s| s["status"] == "ok").count();
    let summary =
        format!("Ran {} of {} scripted step(s) on {last_url}{}.", ok_steps, p.actions.len(), aborted.as_ref().map(|a| format!(" ({a})")).unwrap_or_default());
    let result = json!({
        "browser": {"product": browser.product, "started_by": if browser.launched { "craft-anything" } else { "external" }, "cleanup": if browser.launched { "closed" } else { "disconnected" }},
        "start_url": start_url,
        "action_count": p.actions.len(),
        "keep_bodies": p.keep_bodies,
        "environment": environment,
        "starting_storage": storage,
        "final_artifacts": final_artifacts,
        "events": event_rows,
        "steps": steps,
        "network": network,
        "aborted": aborted,
        "secrets_hidden": p.hidden_values.len(),
        "secret_redactions": secrets.redactions,
    });
    let mut b = EvidenceBuilder::new("web_walkthrough_record", Subject { kind: "website".into(), sha256: None, name: Some(last_url.clone()) }, browser_env(ctx.session.clean_room(), &browser))
        .params(json!({"endpoint": browser.endpoint.origin, "launched": browser.launched, "action_count": p.actions.len(), "sensitive_value_declarations": p.hidden_values.len()}))
        .result(result)
        .summary(summary)
        .confidence(Confidence::Observed)
        .authority(Authority::OurRun)
        .limitation(Limitation::new("controlled_run", "This is one controlled run of scripted steps; different steps or timing would show different behaviour."))
        .limitation(if p.keep_bodies.any() {
            Limitation::new("values_kept", "The requested network content is kept (capped per body); credential headers are redacted and declared secrets are redacted in every encoding.")
        } else {
            Limitation::new("values_not_kept", "Request/response bodies are reduced to outlines, cookies and auth headers are dropped, and declared secrets are redacted in every encoding.")
        })
        .coverage(Coverage::complete(p.actions.len() as u64))
        .location(Location::new("url", last_url));
    if p.hidden_values.iter().any(|s| !s.is_empty()) {
        b = b.limitation(Limitation::new("secrets_redacted", format!("{} occurrence(s) of declared values were redacted", secrets.redactions)));
    }
    Ok(ToolOutput::evidence(b.build()?))
}

fn action_name(a: &WalkthroughAction) -> &'static str {
    match a {
        WalkthroughAction::Goto { .. } => "goto",
        WalkthroughAction::Click { .. } => "click",
        WalkthroughAction::Fill { .. } => "fill",
        WalkthroughAction::Press { .. } => "press",
        WalkthroughAction::Select { .. } => "select",
        WalkthroughAction::Wait { .. } => "wait",
    }
}

fn run_action(client: &mut CdpClient, action: &WalkthroughAction, secrets: &mut Secrets, dl: &Deadline) -> Result<(), ToolError> {
    action.check()?;
    match action {
        WalkthroughAction::Goto { url, wait_ms } => {
            let _ = secrets.text(url);
            let r = client.call("Page.navigate", json!({"url": url}), None, dl)?;
            if let Some(err) = r.get("errorText").and_then(Value::as_str).filter(|s| !s.is_empty()) {
                return Err(ToolError::new(craft_anything_evidence::ErrorCode::ProgramFailed, format!("navigation failed: {err}")));
            }
            let settle = Deadline::new(Duration::from_millis(wait_ms.unwrap_or(1000).min(script_check::MAX_GOTO_WAIT_MS)), crate::tool::CancelToken::default());
            let _ = client.pump_until(&settle);
            Ok(())
        }
        WalkthroughAction::Wait { ms } => {
            let settle = Deadline::new(Duration::from_millis((*ms).min(script_check::MAX_WAIT_MS)), crate::tool::CancelToken::default());
            let _ = client.pump_until(&settle);
            Ok(())
        }
        WalkthroughAction::Click { at } => {
            let node = at.node(client, dl)?;
            let (x, y) = node_center(client, node, dl)?;
            click_at(client, x, y, dl)
        }
        WalkthroughAction::Fill { at, value } => {
            let node = at.node(client, dl)?;
            client.call("DOM.focus", json!({"nodeId": node}), None, dl)?;
            // Clear then insert the text.
            let _ = client.call("Input.dispatchKeyEvent", json!({"type": "keyDown", "modifiers": 0, "key": "a", "commands": ["selectAll"]}), None, dl);
            client.call("Input.insertText", json!({"text": value}), None, dl).map(|_| ())
        }
        WalkthroughAction::Press { at, key } => {
            let node = at.node(client, dl)?;
            client.call("DOM.focus", json!({"nodeId": node}), None, dl)?;
            client.call("Input.dispatchKeyEvent", json!({"type": "keyDown", "key": key}), None, dl)?;
            client.call("Input.dispatchKeyEvent", json!({"type": "keyUp", "key": key}), None, dl).map(|_| ())
        }
        WalkthroughAction::Select { at, value } => {
            // Set the <select>'s value via the DOM, then dispatch change — done through focus + typing
            // of the visible option is unreliable, so set the value attribute by node.
            let node = at.node(client, dl)?;
            let _ = client.call("DOM.focus", json!({"nodeId": node}), None, dl);
            // Use Runtime to set value only inside a walkthrough (foreign code is permitted here).
            let obj = client.call("DOM.resolveNode", json!({"nodeId": node}), None, dl)?;
            let object_id = obj
                .pointer("/object/objectId")
                .and_then(Value::as_str)
                .ok_or_else(|| ToolError::new(craft_anything_evidence::ErrorCode::ProgramFailed, "could not resolve the select element"))?;
            client
                .call(
                    "Runtime.callFunctionOn",
                    json!({"objectId": object_id, "functionDeclaration": "function(v){this.value=v;this.dispatchEvent(new Event('change',{bubbles:true}));}", "arguments": [{"value": value}]}),
                    None,
                    dl,
                )
                .map(|_| ())
        }
    }
}

pub(crate) fn query_node(client: &mut CdpClient, selector: &str, dl: &Deadline) -> Result<u64, ToolError> {
    let doc = client.call("DOM.getDocument", json!({"depth": 0}), None, dl)?;
    let root = doc
        .pointer("/root/nodeId")
        .and_then(Value::as_u64)
        .ok_or_else(|| ToolError::new(craft_anything_evidence::ErrorCode::ProgramFailed, "no document root"))?;
    let found = client.call("DOM.querySelector", json!({"nodeId": root, "selector": selector}), None, dl)?;
    let node = found
        .get("nodeId")
        .and_then(Value::as_u64)
        .filter(|n| *n != 0)
        .ok_or_else(|| ToolError::new(craft_anything_evidence::ErrorCode::NotFound, format!("no element matches {selector:?}")))?;
    Ok(node)
}

pub(crate) fn element_center(client: &mut CdpClient, selector: &str, dl: &Deadline) -> Result<(f64, f64), ToolError> {
    let node = query_node(client, selector, dl)?;
    node_center(client, node, dl)
}

fn node_center(client: &mut CdpClient, node: u64, dl: &Deadline) -> Result<(f64, f64), ToolError> {
    let box_model = client.call("DOM.getBoxModel", json!({"nodeId": node}), None, dl)?;
    let content = box_model
        .pointer("/model/content")
        .and_then(Value::as_array)
        .ok_or_else(|| ToolError::new(craft_anything_evidence::ErrorCode::ProgramFailed, "element has no box"))?;
    let nums: Vec<f64> = content.iter().filter_map(Value::as_f64).collect();
    if nums.len() < 8 {
        return Err(ToolError::new(craft_anything_evidence::ErrorCode::ProgramFailed, "element box is malformed"));
    }
    let x = (nums[0] + nums[2] + nums[4] + nums[6]) / 4.0;
    let y = (nums[1] + nums[3] + nums[5] + nums[7]) / 4.0;
    Ok((x, y))
}

pub(crate) fn click_at(client: &mut CdpClient, x: f64, y: f64, dl: &Deadline) -> Result<(), ToolError> {
    let base = json!({"x": x, "y": y, "button": "left", "clickCount": 1});
    let mut down = base.clone();
    let mut up = base;
    if let Some(o) = down.as_object_mut() {
        o.insert("type".into(), json!("mousePressed"));
    }
    if let Some(o) = up.as_object_mut() {
        o.insert("type".into(), json!("mouseReleased"));
    }
    client.call("Input.dispatchMouseEvent", down, None, dl)?;
    client.call("Input.dispatchMouseEvent", up, None, dl).map(|_| ())
}

mod script_check;

#[cfg(test)]
mod tests;
