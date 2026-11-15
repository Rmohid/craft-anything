//! What a target says about sound, as typed signals.
//!
//! The audio tools exist but nothing told an agent when to reach for them. This module turns what
//! a capture or a triage actually saw into `audio_evidence` on the result, and into suggestions
//! (`Affordance`s) that are display-only and never part of an evidence id.
//!
//! Two rules keep it honest:
//!
//! * **A signal is what was seen, never what it means.** `offline_ctx_only` is what a browser
//!   fingerprinting script looks like, and `media_element` is a sound file the page plays through
//!   an `<audio>` tag: neither has a Web Audio graph our capture can listen to. Only
//!   [`Kind::WebAudioCtor`] suggests `web_audio_capture`.
//! * **Absence is not proof.** When scripts or requests could not be read (Clean room, an opaque
//!   origin, a deadline), the caller passes a coverage sentence saying so, and no signal is a
//!   finding about the page.
//!
//! Pure: no I/O, no browser. The probing lives in the tools that own a browser connection.

use craft_anything_evidence::Affordance;
use serde_json::{Value, json};

/// How much of one script is scanned. A bundle longer than this is read from its start, which is
/// where a constructor call usually is; the coverage sentence says how much was read.
pub const SCRIPT_BYTES: usize = 192 * 1024;

/// How many scripts one capture reads. A page can list 120; the first few same-origin ones carry
/// the app's own code in every layout we have seen, and the coverage sentence says how many of how
/// many were read.
pub const SCRIPT_COUNT: usize = 6;

/// A kind of thing that says the target has sound. Deciding on a typed kind, never on text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    /// The scripts construct a Web Audio context (or an oscillator, or a worklet): a live graph
    /// `web_audio_capture` can listen to.
    WebAudioCtor,
    /// Only `OfflineAudioContext` appears: how browser fingerprinting scripts probe audio, and
    /// nothing a capture can listen to.
    OfflineCtxOnly,
    /// The page has an `<audio>` or `<video>` element. It plays sound, but through a media element,
    /// not a Web Audio graph, so capture has nothing to tap.
    MediaElement,
    /// The page loaded a sound file (a request or an asset with an audio type). A rebuild can be
    /// rendered from it.
    AudioAssetUrl,
}

impl Kind {
    /// The wire name. Stable: stored evidence and tests read it.
    pub fn id(self) -> &'static str {
        match self {
            Kind::WebAudioCtor => "webaudio_ctor",
            Kind::OfflineCtxOnly => "offline_ctx_only",
            Kind::MediaElement => "media_element",
            Kind::AudioAssetUrl => "audio_asset_url",
        }
    }

    /// What it means, in one plain sentence (shown to people and to an agent).
    pub fn meaning(self) -> &'static str {
        match self {
            Kind::WebAudioCtor => "the scripts build a Web Audio graph, so the page's sound can be listened to",
            Kind::OfflineCtxOnly => "only an offline audio context appears (browser fingerprinting), so there is nothing to listen to",
            Kind::MediaElement => "the page plays a sound file through an audio or video element, which has no Web Audio graph to listen to",
            Kind::AudioAssetUrl => "the page loads sound files, which a rebuild can be rendered from",
        }
    }
}

/// One signal: what was seen, and where.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Signal {
    pub kind: Kind,
    /// Where it was seen: a script URL, a request URL, or `dom` for the page's own elements.
    pub at: String,
}

impl Signal {
    pub fn new(kind: Kind, at: impl Into<String>) -> Self {
        Signal { kind, at: at.into() }
    }
}

/// Tokens that only a real-time Web Audio graph has. The context is the discriminator, not the
/// nodes: a browser fingerprinting script also calls `createOscillator` on its
/// `OfflineAudioContext` to render a probe tone, so an oscillator alone says nothing (found live
/// on a fingerprinting fixture, 2026-11-14). A worklet module is decisive too: it is loaded by a
/// running graph, never by a one-off render.
const WEBAUDIO_TOKENS: [&str; 8] = [
    "new AudioContext",
    "new webkitAudioContext",
    "new window.AudioContext",
    "new(window.AudioContext",
    "new (window.AudioContext",
    "new(AudioContext",
    "new (AudioContext",
    "audioWorklet.addModule(",
];

/// The token browser fingerprinting scripts use, and nothing else in a page that makes music.
const OFFLINE_TOKEN: &str = "OfflineAudioContext";

/// Sound file extensions, matched case-insensitively at the end of a URL's path.
const AUDIO_EXTENSIONS: [&str; 8] = [".mp3", ".ogg", ".oga", ".wav", ".flac", ".opus", ".m4a", ".aac"];

/// What one script says, from its text. Offline-only is reported only when no live-graph token is
/// present: a bundle that does both is a page that makes sound, and fingerprinting on the side
/// must not hide that.
pub fn scan_script(url: &str, text: &str) -> Vec<Signal> {
    if WEBAUDIO_TOKENS.iter().any(|t| text.contains(t)) {
        return vec![Signal::new(Kind::WebAudioCtor, url)];
    }
    if text.contains(OFFLINE_TOKEN) {
        return vec![Signal::new(Kind::OfflineCtxOnly, url)];
    }
    Vec::new()
}

/// Sound files among the URLs a page loaded or a folder holds.
pub fn asset_signals<'a>(urls: impl IntoIterator<Item = &'a str>) -> Vec<Signal> {
    urls.into_iter().filter(|u| is_audio_url(u)).map(|u| Signal::new(Kind::AudioAssetUrl, u)).collect()
}

/// Whether a URL names a sound file. The query string is ignored.
pub fn is_audio_url(url: &str) -> bool {
    let path = url.split(['?', '#']).next().unwrap_or(url).to_ascii_lowercase();
    AUDIO_EXTENSIONS.iter().any(|e| path.ends_with(e))
}

/// The `<audio>`/`<video>` elements a page shows, as signals (`at` is `dom`).
pub fn media_element_signals(tags: &[String]) -> Vec<Signal> {
    tags.iter().map(|t| Signal::new(Kind::MediaElement, format!("dom:{t}"))).collect()
}

/// Drop duplicates and keep a stable order, so the same page always produces the same list.
pub fn normalise(mut signals: Vec<Signal>) -> Vec<Signal> {
    signals.sort();
    signals.dedup();
    signals
}

/// The `audio_evidence` value for a result: what was seen, and how far the look reached.
pub fn evidence_json(signals: &[Signal], coverage: &str) -> Value {
    json!({
        "signals": signals.iter().map(|s| json!({"kind": s.kind.id(), "at": s.at})).collect::<Vec<_>>(),
        "coverage": coverage,
    })
}

/// What one page said about sound, and how far the look reached. Built by the capture tool from
/// the page's own answers, so the wording of the coverage is testable without a browser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Probe {
    pub signals: Vec<Signal>,
    pub coverage: String,
}

/// Compose a page's answer: its media elements, the scripts that could be read (with how many of
/// the listed ones were not), the requests it made, and whether the reader was a visitor (Clean
/// room), which sees elements only.
pub fn from_observation(tags: &[String], scripts: &[(String, String)], listed: usize, requests: &[&str], visitor: bool, unread: usize) -> Probe {
    let mut signals = media_element_signals(tags);
    if visitor {
        return Probe {
            signals: normalise(signals),
            coverage: "Clean room: only the page's visible elements were read, not its scripts or requests, so a Web Audio graph cannot be seen from here."
                .to_string(),
        };
    }
    for (url, text) in scripts {
        signals.extend(scan_script(url, text));
    }
    signals.extend(asset_signals(requests.iter().copied()));
    let unread_note = if unread > 0 { format!(" ({unread} same-origin fetch(es) failed or were skipped)") } else { String::new() };
    let coverage = format!("Deep mode: read {} of {listed} script(s){unread_note}; listed {} request(s).", scripts.len(), requests.len());
    Probe { signals: normalise(signals), coverage }
}

/// One plain sentence for the evidence summary, or none when nothing was seen.
pub fn summary_sentence(signals: &[Signal]) -> Option<String> {
    let kinds: Vec<Kind> = {
        let mut k: Vec<Kind> = signals.iter().map(|s| s.kind).collect();
        k.sort();
        k.dedup();
        k
    };
    match kinds.as_slice() {
        [] => None,
        [one] => Some(format!("{}.", capitalise(one.meaning()))),
        many => {
            let said: Vec<&str> = many.iter().map(|k| k.meaning()).collect();
            Some(format!("The target has sound: {}.", said.join("; ")))
        }
    }
}

fn capitalise(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}

/// What the evidence suggests, for the mode it was built in. A tool the mode refuses is never
/// suggested: in Clean room the capture is Deep-only, so a page's live graph suggests nothing
/// there, and the sentence in the coverage says why.
pub fn affordances(signals: &[Signal], clean_room: bool) -> Vec<Affordance> {
    let has = |k: Kind| signals.iter().any(|s| s.kind == k);
    let mut out = Vec::new();
    if clean_room {
        return out;
    }
    if has(Kind::WebAudioCtor) {
        out.push(Affordance::new("web_audio_capture", "the page builds a Web Audio graph, so its sound can be listened to and measured").in_toolset("browser"));
    }
    if has(Kind::AudioAssetUrl) {
        out.push(Affordance::new("web_audio_offline_render", "the page loads sound files, so a rebuild can be rendered from them").in_toolset("browser"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_live_graph_is_recognised_by_its_context() {
        assert_eq!(scan_script("app.js", "const c = new AudioContext();").len(), 1);
        assert_eq!(scan_script("app.js", "new webkitAudioContext")[0].kind, Kind::WebAudioCtor);
        // Minified, with no space after `new`.
        assert_eq!(scan_script("app.js", "new(window.AudioContext||window.webkitAudioContext)()")[0].kind, Kind::WebAudioCtor);
        assert_eq!(scan_script("app.js", "ctx.audioWorklet.addModule('worklet.js')")[0].kind, Kind::WebAudioCtor);
    }

    /// Found live, 2026-11-14: a fingerprinting script renders a probe tone with
    /// `createOscillator` on an `OfflineAudioContext`. An oscillator is not a music page.
    #[test]
    fn a_fingerprinting_script_that_renders_a_tone_is_still_not_a_music_page() {
        let s = scan_script(
            "fp.js",
            "const p = new OfflineAudioContext(1, 44100, 44100); const o = p.createOscillator(); o.connect(p.destination); o.start(); p.startRendering();",
        );
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].kind, Kind::OfflineCtxOnly);
        assert!(affordances(&s, false).is_empty());
    }

    #[test]
    fn a_fingerprinting_script_is_not_a_music_page() {
        let s = scan_script("fp.js", "const c = new OfflineAudioContext(1, 44100, 44100);");
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].kind, Kind::OfflineCtxOnly);
        assert!(affordances(&s, false).is_empty(), "nothing to capture in a fingerprinting script");
    }

    #[test]
    fn a_bundle_that_does_both_is_a_page_that_makes_sound() {
        let s = scan_script("app.js", "new OfflineAudioContext(1,1,1); const c = new AudioContext();");
        assert_eq!(s[0].kind, Kind::WebAudioCtor, "the live graph wins over the fingerprint probe");
    }

    #[test]
    fn a_plain_bundle_says_nothing() {
        assert!(scan_script("vendor.js", "function t(e){return e.map(n=>n+1)}").is_empty());
        assert!(summary_sentence(&[]).is_none());
    }

    #[test]
    fn sound_files_are_recognised_and_the_query_string_ignored() {
        assert!(is_audio_url("https://cdn.test/pad.ogg"));
        assert!(is_audio_url("https://cdn.test/pad.MP3?v=2"));
        assert!(!is_audio_url("https://cdn.test/app.js"));
        assert!(!is_audio_url("https://cdn.test/audio-guide.html"));
        let s = asset_signals(["https://cdn.test/pad.ogg", "https://cdn.test/app.js"]);
        assert_eq!(s, vec![Signal::new(Kind::AudioAssetUrl, "https://cdn.test/pad.ogg")]);
    }

    #[test]
    fn a_media_element_has_no_graph_to_capture() {
        let s = media_element_signals(&["audio".into()]);
        assert_eq!(s[0].kind, Kind::MediaElement);
        assert!(affordances(&s, false).is_empty(), "an <audio> tag is not a Web Audio graph");
    }

    #[test]
    fn clean_room_suggests_no_capture_it_would_refuse() {
        let s = vec![Signal::new(Kind::WebAudioCtor, "app.js")];
        assert!(affordances(&s, true).is_empty());
        let deep = affordances(&s, false);
        assert_eq!(deep[0].tool, "web_audio_capture");
        assert_eq!(deep[0].needs_toolset.as_deref(), Some("browser"));
    }

    #[test]
    fn the_same_page_always_produces_the_same_list() {
        let a = normalise(vec![Signal::new(Kind::WebAudioCtor, "b.js"), Signal::new(Kind::WebAudioCtor, "a.js"), Signal::new(Kind::WebAudioCtor, "b.js")]);
        assert_eq!(a, vec![Signal::new(Kind::WebAudioCtor, "a.js"), Signal::new(Kind::WebAudioCtor, "b.js")]);
    }

    #[test]
    fn the_evidence_says_what_was_seen_and_how_far_it_looked() {
        let v = evidence_json(&[Signal::new(Kind::WebAudioCtor, "app.js")], "scripts: read 1 of 1");
        assert_eq!(v["signals"][0]["kind"], "webaudio_ctor");
        assert_eq!(v["signals"][0]["at"], "app.js");
        assert_eq!(v["coverage"], "scripts: read 1 of 1");
    }

    #[test]
    fn a_silent_page_says_nothing_and_still_says_how_far_it_looked() {
        let p = from_observation(&[], &[("vendor.js".into(), "export const x = 1;".into())], 1, &[], false, 0);
        assert!(p.signals.is_empty());
        assert!(p.coverage.contains("read 1 of 1"), "{}", p.coverage);
        assert!(affordances(&p.signals, false).is_empty());
    }

    #[test]
    fn a_page_that_makes_sound_suggests_capture_and_says_why() {
        let p = from_observation(&[], &[("app.js".into(), "const c = new AudioContext()".into())], 3, &[], false, 2);
        assert_eq!(p.signals, vec![Signal::new(Kind::WebAudioCtor, "app.js")]);
        assert!(p.coverage.contains("read 1 of 3") && p.coverage.contains("2 same-origin"), "{}", p.coverage);
        let a = affordances(&p.signals, false);
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].tool, "web_audio_capture");
        assert!(summary_sentence(&p.signals).unwrap().contains("Web Audio graph"));
    }

    #[test]
    fn clean_room_reports_elements_only_and_says_what_it_could_not_see() {
        let p = from_observation(&["audio".into()], &[], 0, &[], true, 0);
        assert_eq!(p.signals[0].kind, Kind::MediaElement);
        assert!(p.coverage.contains("Clean room"), "{}", p.coverage);
        assert!(affordances(&p.signals, true).is_empty());
    }
}
