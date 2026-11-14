//! Pure audio fingerprint math: log-bin quantisation, RMS envelopes, graph signatures and
//! similarity measures. No I/O, no browser, wasm-safe.
//!
//! The `Fingerprint` type is the on-disk `audio-fingerprint.json` shape. Version bumps go through
//! [`migrate`]; an unknown version is a typed error, never a panic.

use craft_anything_evidence::{ErrorCode, ToolError};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

/// The fingerprint format this build writes.
pub const FINGERPRINT_VERSION: u32 = 1;
/// The oldest fingerprint format this build can read.
pub const MIN_VERSION: u32 = 1;

/// Log-frequency bins a fingerprint keeps unless the caller asks for another count.
const DEFAULT_BINS: usize = 48;
/// Most log-frequency bins a fingerprint may ask for.
const MAX_BINS: usize = 256;
/// Most raw spectrum values a single quantisation may read.
const MAX_ELEMENTS: usize = 1_000_000;
/// Most 1 Hz envelope values a fingerprint may keep (ten minutes).
const MAX_RMS_SAMPLES: usize = 600;
/// Most interaction-traced mixer samples a fingerprint may keep (ten minutes at 10 Hz).
const MAX_ENVELOPE_SAMPLES: usize = 6_000;
/// Sample rate assumed when a capture doesn't say otherwise.
const DEFAULT_SAMPLE_RATE: u32 = 48_000;
/// Most seconds an RMS envelope may cover.
const MAX_ENVELOPE_SECONDS: u32 = 600;

/// A measurable audio scene: spectrum, loudness envelope, graph shape and mixer state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Fingerprint {
    /// Format version; see [`FINGERPRINT_VERSION`].
    pub version: u32,
    /// The scenario the page was playing (`focus`, …), if the page exposes one.
    #[serde(default)]
    pub scenario_id: String,
    /// Sample rate of the captured audio, in Hz.
    #[serde(default)]
    pub sample_rate: u32,
    /// Capture length the RMS envelope covers, in milliseconds.
    #[serde(default)]
    pub window_ms: u32,
    /// Log-spaced spectrum averages; zero energy when the scene is silent.
    #[serde(default)]
    pub log_bins: Vec<f32>,
    /// One RMS value per second of capture.
    #[serde(default)]
    pub rms_envelope: Vec<f32>,
    /// Node counts and edges of the live audio graph.
    #[serde(default)]
    pub graph_signature: GraphSignature,
    /// The page's mixer-state object, kept as evidence (never replayed here).
    #[serde(default)]
    pub mixer_state: MixerState,
    /// Interaction-traced `(layer, gain, pan)` trajectory; empty without a trace.
    #[serde(default)]
    pub mixer_envelope: Vec<EnvelopeSample>,
    /// When the capture happened, in Unix milliseconds; `0` when the caller didn't stamp it.
    #[serde(default)]
    pub captured_at_ms: u64,
    /// Version of the build that produced the fingerprint.
    #[serde(default)]
    pub engine_version: String,
    /// Whether the capture ran in Deep mode (the only mode that looks inside the target).
    #[serde(default)]
    pub deep_mode: bool,
}

/// The shape of an audio graph: how many nodes of each kind, and how they connect.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphSignature {
    /// Node kind (`gain`, `oscillator`, …) to count.
    #[serde(default)]
    pub node_counts: BTreeMap<String, u64>,
    /// Connections between nodes, canonicalised (sorted and deduplicated).
    #[serde(default)]
    pub edges: Vec<Edge>,
}

/// One connection in an audio graph.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Edge {
    /// Node the sound leaves, as the page names it.
    pub from: String,
    /// Node the sound reaches.
    pub to: String,
    /// What flows or how it is routed (`audio`, …).
    #[serde(default)]
    pub kind: String,
}

/// The page's mixer state, kept whole so evidence reaches the person unedited.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MixerState {
    /// The named scenario the mixer is playing.
    #[serde(default)]
    pub scenario_id: String,
    /// Every other key the page exposed (layer gains, tempo, scale shape, …).
    #[serde(flatten)]
    pub fields: BTreeMap<String, Value>,
}

/// One interaction-traced mixer reading: a layer's gain and pan at a moment in time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnvelopeSample {
    /// Milliseconds since the trace started.
    pub t_ms: u64,
    /// The mixer layer this reading belongs to.
    pub layer_id: String,
    /// Layer gain at the reading; finite, normally `0.0..=1.0`.
    pub gain: f32,
    /// Stereo pan at the reading; finite, normally `-1.0..=1.0`.
    pub pan: f32,
}

/// Build a fingerprint from a capture's spectrum, RMS envelope, graph and mixer state.
///
/// `spectrum` is quantised to `DEFAULT_BINS` log-spaced bins. `rms` is already a 1 Hz envelope
/// (`rms_envelope` produces one from raw samples). `graph` is `{node_counts, edges}` and `mixer`
/// is the page's mixer-state object; either may be `null` or `{}`.
///
/// The capture layer stamps `captured_at_ms`, `sample_rate`, `engine_version` and `deep_mode`
/// after construction when it knows better values.
pub fn fingerprint_from_capture(spectrum: &[f32], rms: &[f32], graph: Value, mixer: Value, envelope: &[EnvelopeSample]) -> Result<Fingerprint, ToolError> {
    if rms.len() > MAX_RMS_SAMPLES {
        return Err(ToolError::new(ErrorCode::OverLimit, format!("the RMS envelope has {} values; at most {MAX_RMS_SAMPLES} are kept", rms.len()))
            .with_remediation("Keep the capture short (at most ten minutes) before fingerprinting."));
    }
    if envelope.len() > MAX_ENVELOPE_SAMPLES {
        return Err(ToolError::new(
            ErrorCode::OverLimit,
            format!("the mixer envelope has {} samples; at most {MAX_ENVELOPE_SAMPLES} are kept", envelope.len()),
        )
        .with_remediation("Trace fewer interaction steps or a shorter capture."));
    }

    let log_bins = quantise_log_bins(spectrum, DEFAULT_BINS)?;
    let graph_signature = parse_graph(&graph)?;
    let mixer_state = parse_mixer(&mixer)?;
    let rms_envelope: Vec<f32> = rms.iter().map(|value| finite_or_zero(*value)).collect();
    let mixer_envelope: Vec<EnvelopeSample> = envelope
        .iter()
        .map(|sample| EnvelopeSample {
            t_ms: sample.t_ms,
            layer_id: sample.layer_id.clone(),
            gain: finite_or_zero(sample.gain),
            pan: finite_or_zero(sample.pan),
        })
        .collect();
    // The RMS envelope holds one value per second, so its length is the capture window.
    let window_ms = u32::try_from((rms_envelope.len() as u64).saturating_mul(1_000).min(u64::from(u32::MAX))).unwrap_or(u32::MAX);

    Ok(Fingerprint {
        version: FINGERPRINT_VERSION,
        scenario_id: mixer_state.scenario_id.clone(),
        sample_rate: DEFAULT_SAMPLE_RATE,
        window_ms,
        log_bins,
        rms_envelope,
        graph_signature,
        mixer_state,
        mixer_envelope,
        captured_at_ms: 0,
        engine_version: env!("CARGO_PKG_VERSION").to_string(),
        deep_mode: false,
    })
}

/// Put a spectrum into log-spaced bins by averaging each bin's raw range.
///
/// More than 1,000,000 raw values is refused with [`ErrorCode::OverLimit`] before any
/// binning. `bins` is clamped to `1..=256`; an empty spectrum yields zero-energy bins. Non-finite
/// values (NaN, infinity) count as no energy so the result always serialises.
pub fn quantise_log_bins(raw: &[f32], bins: usize) -> Result<Vec<f32>, ToolError> {
    if raw.len() > MAX_ELEMENTS {
        return Err(ToolError::new(ErrorCode::OverLimit, format!("the spectrum has {} values; at most {MAX_ELEMENTS} are read", raw.len()))
            .with_remediation("Average the spectrum before fingerprinting, or capture a shorter window."));
    }

    let bins = bins.clamp(1, MAX_BINS);
    let len = raw.len();
    if len == 0 {
        return Ok(vec![0.0; bins]);
    }

    let ln_max = (len as f64 + 1.0).ln();
    let mut out = Vec::with_capacity(bins);
    for index in 0..bins {
        let start = (ln_max * index as f64 / bins as f64).exp() - 1.0;
        let end = (ln_max * (index as f64 + 1.0) / bins as f64).exp() - 1.0;
        let lo = (start.floor().max(0.0) as usize).min(len.saturating_sub(1));
        let hi = (end.ceil() as usize).max(lo + 1).min(len);
        let mut sum = 0.0f64;
        let mut count = 0usize;
        if let Some(range) = raw.get(lo..hi) {
            for value in range {
                if value.is_finite() {
                    sum += f64::from(*value);
                    count += 1;
                }
            }
        }
        out.push(if count == 0 { 0.0 } else { (sum / count as f64) as f32 });
    }
    Ok(out)
}

/// Turn raw samples into a 1 Hz RMS envelope: one value per second of audio.
///
/// `sample_rate` sets the one-second window; `window_ms` bounds how many seconds are reported,
/// clamped to `1..=600`. A capture shorter than the window yields only the seconds it holds. An
/// empty capture yields an empty envelope. Non-finite samples count as silence.
pub fn rms_envelope(samples: &[f32], sample_rate: u32, window_ms: u32) -> Vec<f32> {
    if samples.is_empty() {
        return Vec::new();
    }
    let seconds = (window_ms / 1_000).clamp(1, MAX_ENVELOPE_SECONDS) as usize;
    let per_window = sample_rate.max(1) as usize;
    let available = samples.len().div_ceil(per_window);
    let count = seconds.min(available);

    let mut out = Vec::with_capacity(count);
    for window in samples.chunks(per_window).take(count) {
        let mut sum = 0.0f64;
        let mut finite = 0usize;
        for value in window {
            if value.is_finite() {
                sum += f64::from(*value) * f64::from(*value);
                finite += 1;
            }
        }
        out.push(if finite == 0 { 0.0 } else { (sum / finite as f64).sqrt() as f32 });
    }
    out
}

/// Cosine similarity of two spectra, in `-1..=1`.
///
/// Identical shapes score `1.0`. When either spectrum has zero energy (silence, or an analyser
/// that returned zeros) the score is `0.0` rather than a NaN, so the caller can record a
/// `low_energy` limitation and fail the likeness verdict. Different lengths are a
/// [`ErrorCode::BadInput`]: fingerprint both captures with the same `bins` count.
pub fn spectral_cosine(a: &[f32], b: &[f32]) -> Result<f64, ToolError> {
    if a.len() != b.len() {
        return Err(ToolError::invalid(format!("the two spectra have {} and {} values", a.len(), b.len()))
            .with_remediation("Fingerprint both captures with the same `bins` value."));
    }
    if a.len() > MAX_ELEMENTS {
        return Err(ToolError::new(ErrorCode::OverLimit, format!("the spectra have {} values; at most {MAX_ELEMENTS} are compared", a.len()))
            .with_remediation("Fingerprint both captures with fewer `bins`."));
    }

    let mut dot = 0.0f64;
    let mut left_norm = 0.0f64;
    let mut right_norm = 0.0f64;
    for (left, right) in a.iter().zip(b.iter()) {
        let left = f64::from(finite_or_zero(*left));
        let right = f64::from(finite_or_zero(*right));
        dot += left * right;
        left_norm += left * left;
        right_norm += right * right;
    }
    if left_norm == 0.0 || right_norm == 0.0 {
        return Ok(0.0);
    }
    let cosine = dot / (left_norm.sqrt() * right_norm.sqrt());
    Ok(if cosine.is_finite() { cosine.clamp(-1.0, 1.0) } else { 0.0 })
}

/// Mean per-sample L1 distance between two mixer trajectories, averaged over their layers.
///
/// Each layer's `(gain, pan)` readings are ordered by `t_ms` and proportionally resampled when
/// the two sides differ in length (98 vs 100 readings compares, never panics). A layer that only
/// one side has compares against silence (`gain = pan = 0`). An identical trajectory scores
/// `0.0`; a full gain or pan swing scores `1.0`, so mirrored slider moves are far apart.
pub fn envelope_distance(a: &[EnvelopeSample], b: &[EnvelopeSample]) -> f64 {
    let mut layers: BTreeSet<String> = BTreeSet::new();
    for sample in a.iter().chain(b.iter()) {
        layers.insert(sample.layer_id.clone());
    }

    let mut total = 0.0f64;
    let mut measured = 0usize;
    for layer in &layers {
        let left = track(a, layer);
        let right = track(b, layer);
        let len = left.len().max(right.len()).min(MAX_ENVELOPE_SAMPLES);
        if len == 0 {
            continue;
        }
        let left = resample(&left, len);
        let right = resample(&right, len);
        let mut sum = 0.0f64;
        let mut count = 0usize;
        for ((left_gain, left_pan), (right_gain, right_pan)) in left.iter().zip(right.iter()) {
            sum += f64::from(*left_gain - *right_gain).abs() + f64::from(*left_pan - *right_pan).abs();
            count += 1;
        }
        if count > 0 {
            total += sum / count as f64;
            measured += 1;
        }
    }

    if measured == 0 { 0.0 } else { (total / measured as f64).max(0.0) }
}

/// Read a fingerprint from any supported version.
///
/// Versions outside `MIN_VERSION..=FINGERPRINT_VERSION` are a [`ErrorCode::BadInput`]. Hostile
/// lengths are [`ErrorCode::OverLimit`]. Values that could never be serialised (NaN, infinity) are
/// read as zero so a corrupt file still compares through its graph signature instead of crashing.
pub fn migrate(mut fingerprint: Fingerprint) -> Result<Fingerprint, ToolError> {
    if !(MIN_VERSION..=FINGERPRINT_VERSION).contains(&fingerprint.version) {
        return Err(ToolError::invalid(format!(
            "audio fingerprint version {} is not supported; this build reads versions {MIN_VERSION} to {FINGERPRINT_VERSION}",
            fingerprint.version
        ))
        .with_remediation("Re-capture with this build, or upgrade Craft Anything to read newer fingerprints."));
    }
    if fingerprint.log_bins.len() > MAX_ELEMENTS {
        return Err(ToolError::new(
            ErrorCode::OverLimit,
            format!("the fingerprint has {} log bins; at most {MAX_ELEMENTS} are read", fingerprint.log_bins.len()),
        )
        .with_remediation("Re-fingerprint the capture with fewer `bins`."));
    }
    if fingerprint.rms_envelope.len() > MAX_RMS_SAMPLES {
        return Err(ToolError::new(
            ErrorCode::OverLimit,
            format!("the fingerprint has {} RMS values; at most {MAX_RMS_SAMPLES} are read", fingerprint.rms_envelope.len()),
        )
        .with_remediation("Re-fingerprint the capture with a window of at most ten minutes."));
    }
    if fingerprint.mixer_envelope.len() > MAX_ENVELOPE_SAMPLES {
        return Err(ToolError::new(
            ErrorCode::OverLimit,
            format!("the fingerprint has {} mixer samples; at most {MAX_ENVELOPE_SAMPLES} are read", fingerprint.mixer_envelope.len()),
        )
        .with_remediation("Re-trace the capture with fewer interaction steps."));
    }

    for value in &mut fingerprint.log_bins {
        *value = finite_or_zero(*value);
    }
    for value in &mut fingerprint.rms_envelope {
        *value = finite_or_zero(*value);
    }
    for sample in &mut fingerprint.mixer_envelope {
        sample.gain = finite_or_zero(sample.gain);
        sample.pan = finite_or_zero(sample.pan);
    }
    Ok(fingerprint)
}

fn finite_or_zero(value: f32) -> f32 {
    if value.is_finite() { value } else { 0.0 }
}

fn parse_graph(value: &Value) -> Result<GraphSignature, ToolError> {
    if value.is_null() {
        return Ok(GraphSignature::default());
    }
    let mut graph: GraphSignature = serde_json::from_value(value.clone()).map_err(|error| {
        ToolError::invalid(format!("the audio graph signature isn't readable: {error}"))
            .with_remediation("Pass `graph` as `{node_counts: {...}, edges: [{from, to, kind}]}`.")
    })?;
    graph.edges.sort();
    graph.edges.dedup();
    Ok(graph)
}

fn parse_mixer(value: &Value) -> Result<MixerState, ToolError> {
    if value.is_null() {
        return Ok(MixerState::default());
    }
    serde_json::from_value(value.clone()).map_err(|error| {
        ToolError::invalid(format!("the mixer state isn't readable: {error}"))
            .with_remediation("Pass `mixer_state` as the page's mixer object, for example `{scenario_id: \"focus\"}`.")
    })
}

fn track(samples: &[EnvelopeSample], layer: &str) -> Vec<(f32, f32)> {
    let mut points: Vec<(u64, f32, f32)> = samples
        .iter()
        .filter(|sample| sample.layer_id == layer)
        .take(MAX_ENVELOPE_SAMPLES)
        .map(|sample| (sample.t_ms, finite_or_zero(sample.gain), finite_or_zero(sample.pan)))
        .collect();
    points.sort_by_key(|(t_ms, _, _)| *t_ms);
    points.into_iter().map(|(_, gain, pan)| (gain, pan)).collect()
}

fn resample(points: &[(f32, f32)], len: usize) -> Vec<(f32, f32)> {
    if len == 0 {
        return Vec::new();
    }
    if points.is_empty() {
        return vec![(0.0, 0.0); len];
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
        let (lo_gain, lo_pan) = points.get(lo).copied().unwrap_or((0.0, 0.0));
        let (hi_gain, hi_pan) = points.get(hi).copied().unwrap_or((0.0, 0.0));
        out.push((lo_gain + (hi_gain - lo_gain) * weight, lo_pan + (hi_pan - lo_pan) * weight));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    #[allow(clippy::manual_range_contains)] // the range is written out so the failure names both bounds
    fn silent_fingerprint_fails_compare_not_panics() {
        let a = fingerprint_from_capture(&[0.0; 64], &[0.0; 30], json!({"node_counts":{}}), json!({}), &[]).unwrap();
        let s = spectral_cosine(&a.log_bins, &a.log_bins).unwrap();
        assert!(s.is_finite() && s <= 1.0 && s >= -1.0, "zero-energy cosine {s}");
    }

    #[test]
    fn oversized_bins_are_refused() {
        let err = quantise_log_bins(&vec![1.0; 1_000_001], 48).unwrap_err();
        assert_eq!(err.code, craft_anything_evidence::ErrorCode::OverLimit);
    }

    #[test]
    fn unknown_version_is_typed_error() {
        let mut f = fingerprint_from_capture(&[1.0; 8], &[1.0; 4], json!({"node_counts":{}}), json!({}), &[]).unwrap();
        f.version = 99;
        assert_eq!(migrate(f).unwrap_err().code, craft_anything_evidence::ErrorCode::BadInput);
    }

    #[test]
    fn identical_spectrum_scores_one() {
        let bins = quantise_log_bins(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0], 8).unwrap();
        let score = spectral_cosine(&bins, &bins).unwrap();
        assert!((score - 1.0).abs() < 1e-6, "identical spectra cosine {score}");
    }

    #[test]
    fn log_bins_are_finite_and_bounded() {
        let bins = quantise_log_bins(&[f32::NAN, 2.0, f32::INFINITY, f32::NEG_INFINITY, 4.0], 3).unwrap();
        assert_eq!(bins.len(), 3);
        assert!(bins.iter().all(|value| value.is_finite()), "non-finite log bins: {bins:?}");
    }

    #[test]
    fn bin_count_is_clamped() {
        assert_eq!(quantise_log_bins(&[1.0], 0).unwrap().len(), 1);
        assert_eq!(quantise_log_bins(&[1.0], 999).unwrap().len(), 256);
    }

    #[test]
    fn empty_spectrum_is_zero_energy() {
        let bins = quantise_log_bins(&[], 48).unwrap();
        assert_eq!(bins.len(), 48);
        assert!(bins.iter().all(|value| *value == 0.0));
        assert_eq!(spectral_cosine(&bins, &bins).unwrap(), 0.0);
    }

    #[test]
    fn rms_envelope_holds_one_value_per_second() {
        let samples = vec![0.5f32; 300];
        let envelope = rms_envelope(&samples, 100, 3_000);
        assert_eq!(envelope.len(), 3);
        assert!(envelope.iter().all(|value| (*value - 0.5).abs() < 1e-6), "RMS {envelope:?}");
    }

    #[test]
    fn rms_envelope_reports_only_the_seconds_it_has() {
        let envelope = rms_envelope(&[0.25f32; 100], 100, 30_000);
        assert_eq!(envelope.len(), 1);
        assert!((envelope[0] - 0.25).abs() < 1e-6);
        assert!(rms_envelope(&[], 48_000, 30_000).is_empty());
    }

    #[test]
    fn mismatched_spectrum_lengths_are_bad_input() {
        let err = spectral_cosine(&[1.0, 2.0], &[1.0]).unwrap_err();
        assert_eq!(err.code, craft_anything_evidence::ErrorCode::BadInput);
    }

    #[test]
    fn fingerprint_round_trips_through_json() {
        let fingerprint = fingerprint_from_capture(
            &[1.0, 2.0, 3.0, 4.0],
            &[0.1, 0.2],
            json!({"node_counts": {"gain": 2}, "edges": [{"from": "osc", "to": "gain", "kind": "audio"}]}),
            json!({"scenario_id": "focus", "layers": {"pad": {"gain": 0.4}}}),
            &[EnvelopeSample { t_ms: 0, layer_id: "pad".into(), gain: 0.2, pan: -0.1 }],
        )
        .unwrap();
        assert_eq!(fingerprint.scenario_id, "focus");
        assert_eq!(fingerprint.version, FINGERPRINT_VERSION);
        assert_eq!(fingerprint.window_ms, 2_000);

        let text = serde_json::to_string(&fingerprint).unwrap();
        let read: Fingerprint = serde_json::from_str(&text).unwrap();
        assert_eq!(read, fingerprint);
        assert_eq!(migrate(read).unwrap(), fingerprint);
    }

    #[test]
    fn graph_edges_are_canonical() {
        let fingerprint = fingerprint_from_capture(
            &[1.0],
            &[1.0],
            json!({"node_counts": {"gain": 1}, "edges": [
                {"from": "b", "to": "c"},
                {"from": "a", "to": "b"},
                {"from": "b", "to": "c"}
            ]}),
            json!({}),
            &[],
        )
        .unwrap();
        assert_eq!(fingerprint.graph_signature.edges.len(), 2);
        assert_eq!(fingerprint.graph_signature.edges.first().map(|edge| edge.from.as_str()), Some("a"));
    }

    #[test]
    fn oversized_rms_or_mixer_envelopes_are_refused() {
        let err = fingerprint_from_capture(&[1.0], &vec![0.1; MAX_RMS_SAMPLES + 1], json!({}), json!({}), &[]).unwrap_err();
        assert_eq!(err.code, craft_anything_evidence::ErrorCode::OverLimit);

        let sample = EnvelopeSample { t_ms: 0, layer_id: "pad".into(), gain: 0.0, pan: 0.0 };
        let err = fingerprint_from_capture(&[1.0], &[0.1], json!({}), json!({}), &vec![sample; MAX_ENVELOPE_SAMPLES + 1]).unwrap_err();
        assert_eq!(err.code, craft_anything_evidence::ErrorCode::OverLimit);
    }

    #[test]
    fn migrate_refuses_oversized_log_bins() {
        let mut fingerprint = fingerprint_from_capture(&[1.0], &[1.0], json!({}), json!({}), &[]).unwrap();
        fingerprint.log_bins = vec![1.0; MAX_ELEMENTS + 1];
        assert_eq!(migrate(fingerprint).unwrap_err().code, craft_anything_evidence::ErrorCode::OverLimit);
    }

    #[test]
    fn mirrored_slider_steps_fail_compare() {
        let up = vec![EnvelopeSample { t_ms: 0, layer_id: "pad".into(), gain: 0.0, pan: 0.0 }];
        let down = vec![EnvelopeSample { t_ms: 0, layer_id: "pad".into(), gain: 1.0, pan: 0.0 }];
        assert!(envelope_distance(&up, &down) > 0.5, "mirrored steps must not compare as alike");
    }

    #[test]
    fn envelope_distance_is_zero_for_identical_trajectories() {
        let trace = vec![
            EnvelopeSample { t_ms: 0, layer_id: "pad".into(), gain: 0.0, pan: 0.0 },
            EnvelopeSample { t_ms: 1_000, layer_id: "pad".into(), gain: 1.0, pan: 0.5 },
        ];
        assert!(envelope_distance(&trace, &trace) < 1e-6);
        assert_eq!(envelope_distance(&[], &[]), 0.0);
    }

    #[test]
    fn mirrored_slider_steps_are_far_apart() {
        let up = vec![
            EnvelopeSample { t_ms: 0, layer_id: "pad".into(), gain: 0.0, pan: 0.0 },
            EnvelopeSample { t_ms: 1_000, layer_id: "pad".into(), gain: 1.0, pan: 0.0 },
        ];
        let down = vec![
            EnvelopeSample { t_ms: 0, layer_id: "pad".into(), gain: 1.0, pan: 0.0 },
            EnvelopeSample { t_ms: 1_000, layer_id: "pad".into(), gain: 0.0, pan: 0.0 },
        ];
        let distance = envelope_distance(&up, &down);
        assert!(distance > 0.5 && distance.is_finite(), "mirrored distance {distance}");
    }

    #[test]
    fn envelope_distance_resamples_mismatched_lengths() {
        let reference: Vec<EnvelopeSample> =
            (0..100).map(|index| EnvelopeSample { t_ms: index as u64 * 10, layer_id: "pad".into(), gain: index as f32 / 100.0, pan: 0.0 }).collect();
        let rendered: Vec<EnvelopeSample> =
            (0..98).map(|index| EnvelopeSample { t_ms: index as u64 * 10, layer_id: "pad".into(), gain: index as f32 / 98.0, pan: 0.0 }).collect();
        let distance = envelope_distance(&reference, &rendered);
        assert!(distance.is_finite() && distance < 0.02, "resampled distance {distance}");
    }
}
