//! Performance and quality measurements (original spec sections 11 and 14,
//! Phase 9).
//!
//! `kaseifu-bench perf` measures, on the machine it runs on, the numbers Phase
//! 9 asks for. It is a research command: nothing here changes the product path,
//! and every synthetic input lives under one temporary directory that is
//! removed before the command returns.
//!
//! - the cheapest FFI round trip (the availability call) and Apple Vision on an
//!   image when one is given, which is the cost of the boundary itself;
//! - one generation per call, timed through the same `fm-rename-v1` request the
//!   product builds, with and without a reused inference session (section
//!   11.5);
//! - whether reuse bleeds one file's answer into the next one, which is the
//!   risk section 11.5 names and the reason the product keeps one session per
//!   generation;
//! - folder sampling cost on a synthetic folder of a chosen size (section
//!   11.2, Directory Sampling);
//! - a video's perception cost with the duration it reports (section 11.4);
//! - the peak resident set size of this process (section 11, memory).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde::Serialize;

use crate::context::image_metadata::MetadataOptions;
use crate::semantic::contract::{Availability, FmStatus, parse_availability, parse_response};
use crate::semantic::ffi::{self, FmContext};

/// FFI round trips timed for the boundary measurement.
const FFI_ROUND_TRIPS: usize = 200;

/// The word the first bleed document carries.
const BLEED_FIRST_WORD: &str = "zebra";

/// The word the second bleed document carries.
const BLEED_SECOND_WORD: &str = "quasar";

/// The first bleed document's name (a transcript bleed tends to surface it).
const BLEED_FIRST_FILE: &str = "perf-bleed-a1b2c3";

/// A path that does not exist, used to time the boundary without the media
/// frameworks doing any work.
const MISSING_IMAGE: &str = "/nonexistent/kaseifu-perf-image.png";

/// What to measure.
#[derive(Debug, Clone, Default)]
pub struct PerfRequest {
    /// Generations (and repetitions) per measurement.
    pub generations: usize,
    /// Ask the bridge to reuse one inference session per instruction set.
    pub reuse_session: bool,
    /// Run the bleed check (needs a reused session to be meaningful).
    pub bleed: bool,
    /// Entries in the synthetic folder used for the sampling measurement.
    pub folder_entries: usize,
    /// A PNG or JPEG to time Apple Vision on.
    pub image: Option<PathBuf>,
    /// A video to time AVFoundation and Vision on (any duration).
    pub video: Option<PathBuf>,
    /// Timeout of one generation.
    pub timeout_ms: u64,
}

/// A set of wall-clock samples.
#[derive(Debug, Clone, PartialEq, Serialize, Default)]
pub struct Timings {
    pub calls: usize,
    pub min: f64,
    pub median: f64,
    pub max: f64,
    pub mean: f64,
}

impl Timings {
    /// Summarise one set of samples (the unit is the caller's).
    fn from_samples(samples: Vec<f64>) -> Self {
        if samples.is_empty() {
            return Self::default();
        }
        let mut sorted = samples.clone();
        sorted.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
        let count = sorted.len();
        let middle = count / 2;
        let median = if count.is_multiple_of(2) {
            (sorted[middle - 1] + sorted[middle]) / 2.0
        } else {
            sorted[middle]
        };
        let mean = samples.iter().sum::<f64>() / count as f64;
        Self {
            calls: count,
            min: sorted[0],
            median,
            max: sorted[count - 1],
            mean,
        }
    }

    /// One line, rounded for reading.
    fn describe(&self, unit: &str) -> String {
        if self.calls == 0 {
            return "not measured".to_string();
        }
        format!(
            "{} calls, min {:.2}, median {:.2}, mean {:.2}, max {:.2} {unit}",
            self.calls, self.min, self.median, self.mean, self.max
        )
    }
}

/// The folder sampling measurement.
#[derive(Debug, Clone, PartialEq, Serialize, Default)]
pub struct FolderMeasurement {
    pub entries_requested: usize,
    pub entries_read: usize,
    pub truncated: bool,
    pub read_us: f64,
}

/// The video perception measurement.
#[derive(Debug, Clone, PartialEq, Serialize, Default)]
pub struct VideoMeasurement {
    pub path: String,
    pub duration_seconds: Option<f64>,
    pub frames: Option<usize>,
    pub perception: Timings,
}

/// What the bleed check found.
#[derive(Debug, Clone, PartialEq, Serialize, Default)]
pub struct BleedCheck {
    /// True when the second answer repeated the first document's word.
    pub second_carries_the_first_word: bool,
    /// True when the second answer named the first document's file.
    pub second_carries_the_first_name: bool,
    pub first_title: Option<String>,
    pub second_title: Option<String>,
}

/// Everything one run measured.
#[derive(Debug, Clone, PartialEq, Serialize, Default)]
pub struct PerfReport {
    /// The model the measurements came from, or the reason there is none.
    pub model: String,
    pub reuse_session: bool,
    /// The availability call (microseconds).
    pub ffi_available_us: Timings,
    /// The same boundary calling Apple Vision with a path that does not exist:
    /// marshalling and the error envelope, with no media work.
    pub ffi_refused_us: Timings,
    /// One generation per call (milliseconds).
    pub generation_ms: Timings,
    /// Generations that did not answer successfully.
    pub generation_failures: usize,
    pub image_ms: Option<Timings>,
    pub folder: Option<FolderMeasurement>,
    pub video: Option<VideoMeasurement>,
    pub bleed: Option<BleedCheck>,
    /// Peak resident set size of this process (bytes; macOS reports bytes).
    pub peak_rss_bytes: u64,
}

/// Measure everything the request asks for.
///
/// The on-device model is required: without it every generation would report
/// `unavailable` and the numbers would be meaningless, so the run stops with
/// that reason instead (original spec section 15).
pub fn run(request: &PerfRequest) -> Result<PerfReport> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("build the measurement runtime")?;
    runtime.block_on(measure(request))
}

async fn measure(request: &PerfRequest) -> Result<PerfReport> {
    let work = work_dir()?;
    let model = require_model()?;
    let document = work.join("document.txt");
    std::fs::write(
        &document,
        "Quarterly budget review for the marketing team: 12000 EUR was spent \
         on the spring campaign, and the next phase needs approval.\n",
    )?;
    let request_json = request_json(&document, "perf-0001")?;

    let (generation_ms, generation_failures) = measure_generations(request, &request_json).await;
    let (ffi_available_us, ffi_refused_us) = measure_ffi();
    let report = PerfReport {
        model,
        reuse_session: request.reuse_session,
        ffi_available_us,
        ffi_refused_us,
        generation_ms,
        generation_failures,
        image_ms: measure_image(request)?,
        folder: Some(measure_folder(request, &work)?),
        video: measure_video(request).await?,
        bleed: measure_bleed(request, &work).await?,
        peak_rss_bytes: peak_rss_bytes(),
    };
    let _ = std::fs::remove_dir_all(&work);
    Ok(report)
}

/// The one temporary directory this run uses.
fn work_dir() -> Result<PathBuf> {
    let dir = std::env::temp_dir().join(format!("kaseifu-perf-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
    Ok(dir)
}

/// One FFI failure as an `anyhow` error.
///
/// The boundary error is not a `std::error::Error` (it is an ABI-level code and
/// a message), so it is carried across by its message.
fn ffi_message(error: crate::semantic::ffi::FfiError) -> anyhow::Error {
    anyhow::anyhow!("{error}")
}

/// The model's availability, as the report states it.
fn require_model() -> Result<String> {
    let json = ffi::available().map_err(ffi_message)?;
    let availability = parse_availability(&json).map_err(|error| anyhow::anyhow!("{error}"))?;
    match availability {
        Availability::Available => Ok("available".to_string()),
        Availability::Unavailable { reason, detail } => {
            let detail = detail.map(|text| format!(": {text}")).unwrap_or_default();
            anyhow::bail!("the on-device model is not available ({reason}{detail})")
        }
    }
}

/// The `fm-rename-v1` request for one path, built exactly like the product.
///
/// The observation is not timed: it happens once, outside every loop, so the
/// numbers describe the boundary and the model only.
fn request_json(path: &Path, request_id: &str) -> Result<String> {
    let file_context = crate::context::builder::build(path)?;
    let normalized =
        crate::normalize::normalize_with_options(file_context, &MetadataOptions::default())?;
    let request = crate::semantic::provider::build_request(&normalized, request_id, "ja");
    Ok(serde_json::to_string(&request)?)
}

/// Time the boundary itself.
///
/// The availability call asks the system model service, so it is an upper bound
/// for the marshalling cost; the same perception call with a path that does not
/// exist returns before ImageIO opens anything, which is the boundary alone.
fn measure_ffi() -> (Timings, Timings) {
    let mut available = Vec::with_capacity(FFI_ROUND_TRIPS);
    for _ in 0..FFI_ROUND_TRIPS {
        let started = Instant::now();
        let _ = ffi::available();
        available.push(elapsed_us(started));
    }
    let missing = Path::new(MISSING_IMAGE);
    let mut refused = Vec::with_capacity(FFI_ROUND_TRIPS);
    for _ in 0..FFI_ROUND_TRIPS {
        let started = Instant::now();
        let _ = ffi::vision_perceive(missing);
        refused.push(elapsed_us(started));
    }
    (
        Timings::from_samples(available),
        Timings::from_samples(refused),
    )
}

/// Time one generation per call, the way the product path runs them.
async fn measure_generations(request: &PerfRequest, request_json: &str) -> (Timings, usize) {
    let calls = request.generations.max(1);
    let timeout = Duration::from_millis(request.timeout_ms);
    let mut samples = Vec::with_capacity(calls);
    let mut failures = 0;
    if request.reuse_session {
        let shared = FmContext::create_with_options(REUSE_OPTIONS);
        let Ok(shared) = shared else {
            return (Timings::default(), calls);
        };
        for _ in 0..calls {
            let (ms, ok) = one_generation(&shared, request_json, timeout).await;
            samples.push(ms);
            failures += usize::from(!ok);
        }
    } else {
        for _ in 0..calls {
            let Ok(context) = FmContext::create() else {
                failures += 1;
                continue;
            };
            let (ms, ok) = one_generation(&context, request_json, timeout).await;
            samples.push(ms);
            failures += usize::from(!ok);
        }
    }
    (Timings::from_samples(samples), failures)
}

/// The options one bench context is created with.
const REUSE_OPTIONS: &str = "{\"reuse_session\":true}";

/// One generation: its wall time in milliseconds and whether it answered.
async fn one_generation(context: &FmContext, request_json: &str, timeout: Duration) -> (f64, bool) {
    let started = Instant::now();
    let outcome = context.generate(request_json, timeout).await;
    let ms = started.elapsed().as_secs_f64() * 1_000.0;
    let ok = outcome
        .ok()
        .and_then(|json| parse_response(&json).ok())
        .is_some_and(|response| matches!(response.status, FmStatus::Success));
    (ms, ok)
}

/// Time Apple Vision on one image (when one was given).
fn measure_image(request: &PerfRequest) -> Result<Option<Timings>> {
    let Some(path) = request.image.as_deref() else {
        return Ok(None);
    };
    let mut samples = Vec::new();
    for _ in 0..request.generations.max(1) {
        let started = Instant::now();
        ffi::vision_perceive(path).map_err(ffi_message)?;
        samples.push(started.elapsed().as_secs_f64() * 1_000.0);
    }
    Ok(Some(Timings::from_samples(samples)))
}

/// Read one synthetic folder of a chosen size and time the read.
fn measure_folder(request: &PerfRequest, work: &Path) -> Result<FolderMeasurement> {
    let folder = work.join("folder");
    std::fs::create_dir_all(&folder)?;
    for index in 0..request.folder_entries {
        std::fs::write(folder.join(format!("entry-{index:05}.txt")), b"x")?;
    }
    let started = Instant::now();
    let facts = crate::context::directory::read_directory_facts(&folder)?;
    Ok(FolderMeasurement {
        entries_requested: request.folder_entries,
        entries_read: facts.total_entries,
        truncated: facts.truncated,
        read_us: elapsed_us(started),
    })
}

/// Time AVFoundation and Vision on one video (when one was given).
async fn measure_video(request: &PerfRequest) -> Result<Option<VideoMeasurement>> {
    let Some(path) = request.video.as_deref() else {
        return Ok(None);
    };
    let mut samples = Vec::new();
    let mut duration_seconds = None;
    let mut frames = None;
    for _ in 0..request.generations.max(1) {
        let started = Instant::now();
        let json = ffi::video_perceive(path).map_err(ffi_message)?;
        samples.push(started.elapsed().as_secs_f64() * 1_000.0);
        if let Ok(perceived) = crate::semantic::video::parse(&json) {
            duration_seconds = perceived.info.duration_seconds.or(duration_seconds);
            frames = Some(perceived.frames.len());
        }
    }
    Ok(Some(VideoMeasurement {
        path: path.display().to_string(),
        duration_seconds,
        frames,
        perception: Timings::from_samples(samples),
    }))
}

/// Check whether a reused session carries one file into the next answer.
///
/// Two documents whose content is unmistakable are generated through one reuse
/// context; the second answer is inspected for the first document's word. The
/// product creates one session per generation, so this is the evidence for that
/// decision (original spec section 11.5).
async fn measure_bleed(request: &PerfRequest, work: &Path) -> Result<Option<BleedCheck>> {
    if !request.bleed {
        return Ok(None);
    }
    let first = bleed_document(work, "perf-bleed-a1b2c3.txt", BLEED_FIRST_WORD)?;
    let second = bleed_document(work, "perf-bleed-d4e5f6.txt", BLEED_SECOND_WORD)?;
    let timeout = Duration::from_millis(request.timeout_ms);
    let context = FmContext::create_with_options(REUSE_OPTIONS).map_err(ffi_message)?;
    let first_answer = context
        .generate(&first, timeout)
        .await
        .map_err(ffi_message)?;
    let second_answer = context
        .generate(&second, timeout)
        .await
        .map_err(ffi_message)?;
    let answer = second_answer.to_ascii_lowercase();
    Ok(Some(BleedCheck {
        second_carries_the_first_word: answer.contains(BLEED_FIRST_WORD),
        second_carries_the_first_name: answer.contains(BLEED_FIRST_FILE),
        first_title: title_of(&first_answer),
        second_title: title_of(&second_answer),
    }))
}

/// Write one bleed document and answer with its request JSON.
///
/// The content is rich enough to produce a real title, and the file name says
/// nothing about it, so an answer that repeats the other document's word can
/// only come from the transcript.
fn bleed_document(work: &Path, name: &str, word: &str) -> Result<String> {
    let path = work.join(name);
    std::fs::write(
        &path,
        format!(
            "Field notes from the {word} survey in 2026: the {word} count, \
             the crossing points, and what the team measured on each visit.\n"
        ),
    )?;
    request_json(&path, &format!("perf-bleed-{name}"))
}

/// The `title` of a response, when it answered with one.
fn title_of(json: &str) -> Option<String> {
    parse_response(json)
        .ok()
        .and_then(|response| response.semantic)
        .and_then(|fields| fields.title)
}

/// Microseconds since `started`.
fn elapsed_us(started: Instant) -> f64 {
    started.elapsed().as_secs_f64() * 1_000_000.0
}

/// The peak resident set size of this process in bytes.
///
/// macOS reports `ru_maxrss` in bytes (Linux reports kilobytes); this tool only
/// runs on macOS, and the report states the unit.
fn peak_rss_bytes() -> u64 {
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) } != 0 {
        return 0;
    }
    usage.ru_maxrss.max(0) as u64
}

/// Print the report the way the rest of this tool prints (plain ASCII lines).
pub fn print_text(report: &PerfReport) {
    println!("KASEIFU / PERF");
    println!("model: {}", report.model);
    println!(
        "reuse_session: {}",
        if report.reuse_session { "yes" } else { "no" }
    );
    println!("ffi_available: {}", report.ffi_available_us.describe("us"));
    println!("ffi_refused: {}", report.ffi_refused_us.describe("us"));
    println!(
        "generation: {} (failures {})",
        report.generation_ms.describe("ms"),
        report.generation_failures
    );
    match &report.image_ms {
        Some(timings) => println!("vision_image: {}", timings.describe("ms")),
        None => println!("vision_image: not measured (no --image)"),
    }
    match &report.folder {
        Some(folder) => println!(
            "folder_sampling: {} entries read of {} in {:.1} us (truncated {})",
            folder.entries_read, folder.entries_requested, folder.read_us, folder.truncated
        ),
        None => println!("folder_sampling: not measured"),
    }
    match &report.video {
        Some(video) => println!(
            "video: {} duration {:?}s frames {:?}, {}",
            video.path,
            video.duration_seconds,
            video.frames,
            video.perception.describe("ms")
        ),
        None => println!("video: not measured (no --video)"),
    }
    match &report.bleed {
        Some(bleed) => println!(
            "bleed: second answer carries the first word: {}, the first file: {} (first {:?}, second {:?})",
            bleed.second_carries_the_first_word,
            bleed.second_carries_the_first_name,
            bleed.first_title,
            bleed.second_title
        ),
        None => println!("bleed: not measured (no --bleed)"),
    }
    println!("peak_rss: {} bytes", report.peak_rss_bytes);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timings_summarise_odd_and_even_samples() {
        let odd = Timings::from_samples(vec![3.0, 1.0, 2.0]);
        assert_eq!(odd.calls, 3);
        assert_eq!(odd.min, 1.0);
        assert_eq!(odd.median, 2.0);
        assert_eq!(odd.max, 3.0);
        assert_eq!(odd.mean, 2.0);
        let even = Timings::from_samples(vec![4.0, 1.0, 3.0, 2.0]);
        assert_eq!(even.median, 2.5);
        assert_eq!(Timings::from_samples(Vec::new()), Timings::default());
    }

    #[test]
    fn a_missing_measurement_says_so() {
        assert_eq!(Timings::default().describe("ms"), "not measured");
        assert!(
            Timings::from_samples(vec![1.0])
                .describe("ms")
                .contains("1 calls")
        );
    }

    #[test]
    fn the_request_carries_the_name_target_kind_and_language() {
        let work = work_dir().expect("work dir");
        let path = work.join("note.txt");
        std::fs::write(&path, "a short note about the launch plan\n").expect("document");
        let json = request_json(&path, "perf-test").expect("request");
        let value: serde_json::Value = serde_json::from_str(&json).expect("json");
        assert_eq!(value["schema_version"], "fm-rename-v1");
        assert_eq!(value["request_id"], "perf-test");
        assert_eq!(value["target_type"], "document");
        assert_eq!(value["language"], "ja");
        assert!(value["input"]["filename"].is_string());
        let _ = std::fs::remove_dir_all(&work);
    }

    #[test]
    fn the_peak_memory_is_a_positive_number() {
        assert!(peak_rss_bytes() > 0, "getrusage must answer");
    }
}
