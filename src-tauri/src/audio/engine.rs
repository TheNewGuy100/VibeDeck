use std::collections::VecDeque;
use std::fs::File;
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Condvar, Mutex, OnceLock};
use std::thread;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rustfft::{num_complex::Complex, FftPlanner};
use serde::{Deserialize, Serialize};
use symphonia::core::audio::{AudioBufferRef, SampleBuffer};
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::{MediaSource, MediaSourceStream};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::default::{get_codecs, get_probe};

const BUFFER_LIMIT: usize = 512 * 1024;
const LEVEL_BANDS: usize = 32;
const FFT_SIZE: usize = 2048;
const FFT_HOP: usize = FFT_SIZE / 2;

struct DownloadBuffer {
    bytes: Mutex<Vec<u8>>,
    changed: Condvar,
    finished: AtomicBool,
    content_length: AtomicU64,
    failed: Mutex<Option<String>>,
}

impl DownloadBuffer {
    fn new() -> Self {
        Self {
            bytes: Mutex::new(Vec::new()),
            changed: Condvar::new(),
            finished: AtomicBool::new(false),
            content_length: AtomicU64::new(0),
            failed: Mutex::new(None),
        }
    }
}

struct GrowingReader {
    buffer: Arc<DownloadBuffer>,
    position: usize,
}

impl Read for GrowingReader {
    fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
        if output.is_empty() {
            return Ok(0);
        }
        let mut bytes = self
            .buffer
            .bytes
            .lock()
            .map_err(|_| std::io::Error::other("buffer poisoned"))?;
        loop {
            if self.position < bytes.len() {
                let count = output.len().min(bytes.len() - self.position);
                output[..count].copy_from_slice(&bytes[self.position..self.position + count]);
                self.position += count;
                return Ok(count);
            }
            if self.buffer.finished.load(Ordering::Acquire) {
                if let Some(error) = self
                    .buffer
                    .failed
                    .lock()
                    .ok()
                    .and_then(|value| value.clone())
                {
                    return Err(std::io::Error::other(error));
                }
                return Ok(0);
            }
            bytes = self
                .buffer
                .changed
                .wait(bytes)
                .map_err(|_| std::io::Error::other("buffer poisoned"))?;
        }
    }
}

impl Seek for GrowingReader {
    fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        let mut bytes = self
            .buffer
            .bytes
            .lock()
            .map_err(|_| std::io::Error::other("buffer poisoned"))?;
        let target = match position {
            SeekFrom::Start(value) => value as i128,
            SeekFrom::Current(value) => self.position as i128 + value as i128,
            SeekFrom::End(value) => {
                while !self.buffer.finished.load(Ordering::Acquire) {
                    bytes = self
                        .buffer
                        .changed
                        .wait(bytes)
                        .map_err(|_| std::io::Error::other("buffer poisoned"))?;
                }
                bytes.len() as i128 + value as i128
            }
        };
        if target < 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "negative seek",
            ));
        }
        while target as usize > bytes.len() && !self.buffer.finished.load(Ordering::Acquire) {
            bytes = self
                .buffer
                .changed
                .wait(bytes)
                .map_err(|_| std::io::Error::other("buffer poisoned"))?;
        }
        self.position = target as usize;
        Ok(self.position as u64)
    }
}

impl MediaSource for GrowingReader {
    fn is_seekable(&self) -> bool {
        true
    }
    fn byte_len(&self) -> Option<u64> {
        let content_length = self.buffer.content_length.load(Ordering::Acquire);
        if content_length > 0 {
            Some(content_length)
        } else {
            self.buffer.bytes.lock().ok().and_then(|bytes| {
                self.buffer
                    .finished
                    .load(Ordering::Acquire)
                    .then_some(bytes.len() as u64)
            })
        }
    }
}

struct PcmQueue {
    samples: Mutex<VecDeque<f32>>,
    changed: Condvar,
}

static STOP: OnceLock<Mutex<Option<Arc<AtomicBool>>>> = OnceLock::new();
#[derive(Clone)]
enum AudioSource {
    Url(String),
    File(std::path::PathBuf),
}

static CURRENT_SOURCE: OnceLock<Mutex<Option<AudioSource>>> = OnceLock::new();
static PAUSED: AtomicBool = AtomicBool::new(false);
static ACTIVE: AtomicBool = AtomicBool::new(false);
static VOLUME_BITS: AtomicU32 = AtomicU32::new(1.0_f32.to_bits());
static VOLUME_SEQUENCE: AtomicU64 = AtomicU64::new(0);
static POSITION_MICROS: AtomicU64 = AtomicU64::new(0);
static DURATION_MICROS: AtomicU64 = AtomicU64::new(0);
static REVISION: AtomicU64 = AtomicU64::new(0);
static LEVELS: OnceLock<[AtomicU32; LEVEL_BANDS]> = OnceLock::new();
static ANALYZER_BUFFER: OnceLock<Mutex<Vec<f32>>> = OnceLock::new();
static FFT_PLAN: OnceLock<Arc<dyn rustfft::Fft<f32>>> = OnceLock::new();

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeTrackSnapshot {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration: String,
    pub source: String,
    pub accent: String,
    #[serde(default)]
    pub cover_url: Option<String>,
    #[serde(default)]
    pub file_name: Option<String>,
    #[serde(default)]
    pub duration_seconds: Option<f64>,
    #[serde(default)]
    pub youtube_id: Option<String>,
    #[serde(default)]
    pub managed_path: Option<String>,
    #[serde(default)]
    pub available: Option<bool>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerActivation {
    pub track: NativeTrackSnapshot,
    #[serde(default)]
    pub queue: Vec<NativeTrackSnapshot>,
    pub playlist_id: String,
    #[serde(default)]
    pub queue_source: String,
    pub queue_generation: u64,
    pub queue_index: i64,
    #[serde(default)]
    pub shuffled: bool,
    #[serde(default)]
    pub repeat_mode: String,
    #[serde(default)]
    pub terminal_behavior: String,
    #[serde(default)]
    pub activation_sequence: u64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepeatModeRequest {
    pub repeat_mode: String,
    #[serde(default)]
    pub terminal_behavior: String,
    pub queue: Vec<NativeTrackSnapshot>,
    pub queue_index: i64,
    pub queue_generation: u64,
    #[serde(default)]
    pub playlist_id: String,
    #[serde(default)]
    pub queue_source: String,
    #[serde(default)]
    pub shuffled: bool,
    #[serde(default)]
    pub track_id: String,
    #[serde(default)]
    pub base_revision: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerSnapshot {
    pub track: Option<NativeTrackSnapshot>,
    pub queue: Vec<NativeTrackSnapshot>,
    pub playlist_id: Option<String>,
    pub queue_source: Option<String>,
    pub queue_generation: u64,
    pub queue_index: i64,
    pub shuffled: bool,
    pub repeat_mode: String,
    pub terminal_behavior: String,
    pub is_playing: bool,
    pub position: f64,
    pub duration: f64,
    pub volume: f32,
    pub muted: bool,
    pub volume_sequence: u64,
    pub source: Option<String>,
    pub native_mode: bool,
    pub revision: u64,
    #[serde(default)]
    pub activation_sequence: u64,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SeekRequest {
    pub track_id: String,
    pub queue_generation: u64,
    pub seconds: f64,
    pub revision: u64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VolumeRequest {
    pub volume: f32,
    pub muted: bool,
    pub sequence: u64,
}

static PLAYER_METADATA: OnceLock<Mutex<Option<PlayerActivation>>> = OnceLock::new();
static PLAYER_ACTIVATION_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

pub struct AudioEngine;

fn activation_is_stale(activation: &PlayerActivation, current: Option<&PlayerActivation>) -> bool {
    current.is_some_and(|current| {
        activation.queue_generation < current.queue_generation
            || (current.activation_sequence > 0
                && activation.activation_sequence <= current.activation_sequence)
    })
}

#[derive(Debug)]
pub enum FileValidationError {
    Io(String),
    UnsupportedContainer,
    InvalidContainer(String),
    DecoderFailure(String),
}

impl AudioEngine {
    pub fn set_repeat_mode(request: RepeatModeRequest) -> Result<(), String> {
        if !matches!(request.repeat_mode.as_str(), "off" | "playlist" | "track")
            || request.terminal_behavior != terminal_behavior_for(&request.repeat_mode)
        {
            return Err("Invalid repeat mode".to_string());
        }
        if let Some(metadata) = PLAYER_METADATA.get() {
            let mut current = metadata
                .lock()
                .map_err(|_| "Audio state poisoned".to_string())?;
            if let Some(activation) = current.as_mut() {
                if request.queue_generation < activation.queue_generation {
                    return Err(
                        "A solicitação pertence a uma geração de fila obsoleta.".to_string()
                    );
                }
                if !request.track_id.is_empty() && request.track_id != activation.track.id {
                    return Err("A solicitação de repetição pertence a outra faixa.".to_string());
                }
                if request.queue_generation == activation.queue_generation
                    && (request.base_revision != REVISION.load(Ordering::Acquire)
                        || (!request.track_id.is_empty()
                            && request.track_id != activation.track.id))
                {
                    return Err("A solicitação de repetição está obsoleta.".to_string());
                }
                activation.repeat_mode = request.repeat_mode;
                activation.terminal_behavior = request.terminal_behavior;
                activation.queue = request.queue;
                activation.queue_index = request.queue_index;
                activation.queue_generation = request.queue_generation;
                if !request.playlist_id.is_empty() {
                    activation.playlist_id = request.playlist_id;
                }
                if !request.queue_source.is_empty() {
                    activation.queue_source = request.queue_source;
                }
                activation.shuffled = request.shuffled;
                REVISION.fetch_add(1, Ordering::AcqRel);
            }
        }
        Ok(())
    }

    pub fn validate_file(path: &std::path::Path) -> Result<(), FileValidationError> {
        let file = File::open(path).map_err(|error| FileValidationError::Io(error.to_string()))?;
        let mut hint = Hint::new();
        if let Some(extension) = path.extension().and_then(|value| value.to_str()) {
            hint.with_extension(extension);
        }
        let probed = get_probe()
            .format(
                &hint,
                MediaSourceStream::new(Box::new(file), Default::default()),
                &FormatOptions::default(),
                &MetadataOptions::default(),
            )
            .map_err(|error| FileValidationError::InvalidContainer(error.to_string()))?;
        let track = probed.format.default_track().ok_or_else(|| {
            FileValidationError::InvalidContainer("audio stream not found".to_string())
        })?;
        get_codecs()
            .make(&track.codec_params, &DecoderOptions::default())
            .map(|_| ())
            .map_err(|error| FileValidationError::DecoderFailure(error.to_string()))
    }

    pub fn validate_output_device() -> Result<(), String> {
        let device = cpal::default_host()
            .default_output_device()
            .ok_or_else(|| "no default audio output device".to_string())?;
        device
            .default_output_config()
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    pub fn play_url(url: String, activation: PlayerActivation) -> Result<(), String> {
        let _activation_guard = PLAYER_ACTIVATION_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .map_err(|_| "Audio activation state poisoned".to_string())?;
        Self::set_activation(activation.clone())?;
        Self::stop();
        let duration = activation_duration(&activation);
        ACTIVE.store(true, Ordering::Release);
        *CURRENT_SOURCE
            .get_or_init(|| Mutex::new(None))
            .lock()
            .map_err(|_| "Audio state poisoned".to_string())? = Some(AudioSource::Url(url.clone()));
        PAUSED.store(false, Ordering::Release);
        POSITION_MICROS.store(0, Ordering::Release);
        DURATION_MICROS.store(duration, Ordering::Release);
        let stop = Arc::new(AtomicBool::new(false));
        *STOP
            .get_or_init(|| Mutex::new(None))
            .lock()
            .map_err(|_| "Audio state poisoned".to_string())? = Some(stop.clone());
        thread::spawn(move || {
            if let Err(error) = run_playback(AudioSource::Url(url), stop.clone(), 0.0, None) {
                if !stop.load(Ordering::Acquire) {
                    eprintln!("VibeDeck audio error: {error}");
                }
            }
            if !stop.load(Ordering::Acquire) {
                ACTIVE.store(false, Ordering::Release);
                REVISION.fetch_add(1, Ordering::AcqRel);
            }
            stop.store(true, Ordering::Release);
        });
        Ok(())
    }

    pub fn play_file(path: std::path::PathBuf, activation: PlayerActivation) -> Result<(), String> {
        let _activation_guard = PLAYER_ACTIVATION_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .map_err(|_| "Audio activation state poisoned".to_string())?;
        Self::set_activation(activation.clone())?;
        Self::stop();
        let duration = activation_duration(&activation);
        ACTIVE.store(true, Ordering::Release);
        *CURRENT_SOURCE
            .get_or_init(|| Mutex::new(None))
            .lock()
            .map_err(|_| "Audio state poisoned".to_string())? =
            Some(AudioSource::File(path.clone()));
        PAUSED.store(false, Ordering::Release);
        POSITION_MICROS.store(0, Ordering::Release);
        DURATION_MICROS.store(duration, Ordering::Release);
        let stop = Arc::new(AtomicBool::new(false));
        *STOP
            .get_or_init(|| Mutex::new(None))
            .lock()
            .map_err(|_| "Audio state poisoned".to_string())? = Some(stop.clone());
        let (startup_tx, startup_rx) = mpsc::channel();
        let startup_error_tx = startup_tx.clone();
        thread::spawn(move || {
            if let Err(error) =
                run_playback(AudioSource::File(path), stop.clone(), 0.0, Some(startup_tx))
            {
                let _ = startup_error_tx.send(Err(error.clone()));
                if !stop.load(Ordering::Acquire) {
                    eprintln!("VibeDeck local audio error: {error}");
                }
            }
            if !stop.load(Ordering::Acquire) {
                ACTIVE.store(false, Ordering::Release);
                REVISION.fetch_add(1, Ordering::AcqRel);
            }
            stop.store(true, Ordering::Release);
        });
        match startup_rx.recv_timeout(std::time::Duration::from_secs(10)) {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => {
                Self::stop();
                Err(format!("outputStart: {error}"))
            }
            Err(error) => {
                Self::stop();
                Err(format!("Local audio output did not start: {error}"))
            }
        }
    }

    pub fn stop() {
        ACTIVE.store(false, Ordering::Release);
        REVISION.fetch_add(1, Ordering::AcqRel);
        if let Some(state) = STOP.get() {
            if let Ok(mut current) = state.lock() {
                if let Some(stop) = current.take() {
                    stop.store(true, Ordering::Release);
                }
            }
        }
    }

    pub fn seek(request: SeekRequest) -> Result<(), String> {
        if !request.seconds.is_finite() {
            return Err("A posição solicitada é inválida.".to_string());
        }
        if !Self::is_active() {
            return Err("Nenhuma faixa nativa está carregada.".to_string());
        }
        let metadata = PLAYER_METADATA
            .get_or_init(|| Mutex::new(None))
            .lock()
            .map_err(|_| "Audio state poisoned".to_string())?
            .clone()
            .ok_or_else(|| "Nenhuma faixa nativa está carregada.".to_string())?;
        if metadata.track.id != request.track_id
            || metadata.queue_generation != request.queue_generation
            || REVISION.load(Ordering::Acquire) != request.revision
        {
            return Err("A solicitação de seek está obsoleta.".to_string());
        }
        let source = CURRENT_SOURCE
            .get_or_init(|| Mutex::new(None))
            .lock()
            .map_err(|_| "Audio state poisoned".to_string())?
            .clone()
            .ok_or_else(|| "Nenhuma faixa nativa está carregada.".to_string())?;
        let paused = PAUSED.load(Ordering::Acquire);
        if let Some(state) = STOP.get() {
            if let Ok(mut current) = state.lock() {
                if let Some(stop) = current.take() {
                    stop.store(true, Ordering::Release);
                }
            }
        }
        let seconds = request.seconds.max(0.0);
        POSITION_MICROS.store((seconds * 1_000_000.0) as u64, Ordering::Release);
        if let Ok(mut levels) = ANALYZER_BUFFER
            .get_or_init(|| Mutex::new(Vec::with_capacity(FFT_SIZE * 2)))
            .lock()
        {
            levels.clear();
        }
        for level in
            LEVELS.get_or_init(|| std::array::from_fn(|_| AtomicU32::new(0.0_f32.to_bits())))
        {
            level.store(0.0_f32.to_bits(), Ordering::Release);
        }
        ACTIVE.store(true, Ordering::Release);
        let stop = Arc::new(AtomicBool::new(false));
        *STOP
            .get_or_init(|| Mutex::new(None))
            .lock()
            .map_err(|_| "Audio state poisoned".to_string())? = Some(stop.clone());
        thread::spawn(move || {
            if let Err(error) = run_playback(source, stop.clone(), seconds.max(0.0), None) {
                if !stop.load(Ordering::Acquire) {
                    eprintln!("VibeDeck audio seek error: {error}");
                }
            }
            stop.store(true, Ordering::Release);
        });
        PAUSED.store(paused, Ordering::Release);
        REVISION.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }

    pub fn set_paused(paused: bool) {
        PAUSED.store(paused, Ordering::Release);
        REVISION.fetch_add(1, Ordering::AcqRel);
    }

    pub fn is_active() -> bool {
        ACTIVE.load(Ordering::Acquire)
    }

    pub fn is_paused() -> bool {
        PAUSED.load(Ordering::Acquire)
    }

    pub fn set_volume(request: VolumeRequest) {
        let mut current = VOLUME_SEQUENCE.load(Ordering::Acquire);
        while request.sequence >= current {
            match VOLUME_SEQUENCE.compare_exchange(
                current,
                request.sequence,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    let volume = if request.muted {
                        0.0
                    } else {
                        request.volume.clamp(0.0, 1.0)
                    };
                    VOLUME_BITS.store(volume.to_bits(), Ordering::Release);
                    REVISION.fetch_add(1, Ordering::AcqRel);
                    break;
                }
                Err(next) => current = next,
            }
        }
    }

    pub fn position() -> f64 {
        POSITION_MICROS.load(Ordering::Acquire) as f64 / 1_000_000.0
    }

    pub fn duration() -> f64 {
        DURATION_MICROS.load(Ordering::Acquire) as f64 / 1_000_000.0
    }

    pub fn levels() -> Vec<f32> {
        LEVELS
            .get_or_init(|| std::array::from_fn(|_| AtomicU32::new(0.0_f32.to_bits())))
            .iter()
            .map(|level| f32::from_bits(level.load(Ordering::Acquire)))
            .collect()
    }

    fn set_activation(mut activation: PlayerActivation) -> Result<(), String> {
        let expected_terminal = terminal_behavior_for(&activation.repeat_mode);
        if activation.terminal_behavior.is_empty() {
            activation.terminal_behavior = expected_terminal.to_string();
        } else if activation.terminal_behavior != expected_terminal {
            return Err("Queue terminal behavior does not match repeat mode".to_string());
        }
        let mut metadata = PLAYER_METADATA
            .get_or_init(|| Mutex::new(None))
            .lock()
            .map_err(|_| "Audio state poisoned".to_string())?;
        if activation_is_stale(&activation, metadata.as_ref()) {
            return Err("A ativação pertence a uma geração/intenção obsoleta.".to_string());
        }
        *metadata = Some(activation);
        REVISION.fetch_add(1, Ordering::AcqRel);
        Ok(())
    }

    pub fn snapshot() -> PlayerSnapshot {
        let metadata = PLAYER_METADATA
            .get_or_init(|| Mutex::new(None))
            .lock()
            .ok()
            .and_then(|value| value.clone());
        let active = Self::is_active();
        let has_activation = metadata.is_some();
        let activation_sequence = metadata
            .as_ref()
            .map(|activation| activation.activation_sequence)
            .unwrap_or(0);
        let (
            track,
            queue,
            playlist_id,
            queue_source,
            queue_generation,
            queue_index,
            shuffled,
            repeat_mode,
            terminal_behavior,
            source,
        ) = metadata
            .map(|activation| {
                let source = Some(activation.track.source.clone());
                (
                    Some(activation.track),
                    activation.queue,
                    Some(activation.playlist_id),
                    Some(activation.queue_source),
                    activation.queue_generation,
                    activation.queue_index,
                    activation.shuffled,
                    activation.repeat_mode,
                    activation.terminal_behavior,
                    source,
                )
            })
            .unwrap_or((
                None,
                Vec::new(),
                None,
                None,
                0,
                -1,
                false,
                "off".to_string(),
                "stop".to_string(),
                None,
            ));
        PlayerSnapshot {
            track,
            queue,
            playlist_id,
            queue_source,
            queue_generation,
            queue_index,
            shuffled,
            repeat_mode,
            terminal_behavior,
            is_playing: active && !Self::is_paused(),
            position: if active { Self::position() } else { 0.0 },
            duration: if active { Self::duration() } else { 0.0 },
            volume: f32::from_bits(VOLUME_BITS.load(Ordering::Acquire)),
            muted: f32::from_bits(VOLUME_BITS.load(Ordering::Acquire)) <= 0.0,
            volume_sequence: VOLUME_SEQUENCE.load(Ordering::Acquire),
            source,
            native_mode: has_activation,
            revision: REVISION.load(Ordering::Acquire),
            activation_sequence,
            error: None,
        }
    }
}

fn terminal_behavior_for(repeat_mode: &str) -> &str {
    match repeat_mode {
        "playlist" => "wrapToStart",
        "track" => "restartCurrent",
        _ => "stop",
    }
}

#[cfg(test)]
mod queue_contract_tests {
    use super::{
        activation_is_stale, terminal_behavior_for, NativeTrackSnapshot, PlayerActivation,
        PlayerSnapshot, RepeatModeRequest,
    };

    #[test]
    fn repeat_modes_have_the_shared_terminal_semantics() {
        assert_eq!(terminal_behavior_for("off"), "stop");
        assert_eq!(terminal_behavior_for("playlist"), "wrapToStart");
        assert_eq!(terminal_behavior_for("track"), "restartCurrent");
    }

    #[test]
    fn activation_intents_are_monotonic_within_the_same_queue_generation() {
        let current = PlayerActivation {
            track: NativeTrackSnapshot {
                id: "track-2".into(),
                title: "Track 2".into(),
                artist: "Artist".into(),
                album: "Album".into(),
                duration: "1:00".into(),
                source: "youtube".into(),
                accent: "#000".into(),
                cover_url: None,
                file_name: None,
                duration_seconds: None,
                youtube_id: None,
                managed_path: None,
                available: None,
            },
            queue: vec![],
            playlist_id: "playlist".into(),
            queue_source: "youtube".into(),
            queue_generation: 3,
            queue_index: 1,
            shuffled: false,
            repeat_mode: "off".into(),
            terminal_behavior: "stop".into(),
            activation_sequence: 8,
        };
        let older = PlayerActivation {
            activation_sequence: 7,
            queue_index: 0,
            ..current.clone()
        };
        let newer = PlayerActivation {
            activation_sequence: 9,
            queue_index: 2,
            ..current.clone()
        };
        assert!(activation_is_stale(&older, Some(&current)));
        assert!(!activation_is_stale(&newer, Some(&current)));
    }

    #[test]
    fn repeat_request_uses_the_shared_camel_case_contract() {
        let request: RepeatModeRequest = serde_json::from_value(serde_json::json!({
            "repeatMode": "off",
            "terminalBehavior": "stop",
            "queue": [],
            "queueIndex": 0,
            "queueGeneration": 9,
            "playlistId": "playlist-a",
            "queueSource": "youtube",
            "shuffled": false,
            "trackId": "track-2",
            "baseRevision": 31
        }))
        .expect("repeat request must deserialize");
        assert_eq!(request.queue_generation, 9);
        assert_eq!(request.track_id, "track-2");
        assert_eq!(request.terminal_behavior, "stop");
        assert_eq!(request.playlist_id, "playlist-a");
    }

    #[test]
    fn native_snapshot_roundtrips_terminal_policy_and_queue_position() {
        let snapshot = PlayerSnapshot {
            track: None,
            queue: Vec::new(),
            playlist_id: Some("playlist".into()),
            queue_source: Some("youtube".into()),
            queue_generation: 12,
            queue_index: 2,
            shuffled: false,
            repeat_mode: "playlist".into(),
            terminal_behavior: "wrapToStart".into(),
            is_playing: true,
            position: 0.0,
            duration: 0.0,
            volume: 1.0,
            muted: false,
            volume_sequence: 0,
            source: None,
            native_mode: true,
            revision: 32,
            activation_sequence: 4,
            error: None,
        };
        let value = serde_json::to_value(&snapshot).expect("snapshot serializes");
        assert_eq!(value["terminalBehavior"], "wrapToStart");
        assert_eq!(value["queueGeneration"], 12);
        assert_eq!(value["queueIndex"], 2);
        let restored: PlayerSnapshot = serde_json::from_value(value).expect("snapshot roundtrips");
        assert_eq!(restored.terminal_behavior, "wrapToStart");
        assert_eq!(restored.queue_index, 2);
    }
}

fn activation_duration(activation: &PlayerActivation) -> u64 {
    activation
        .track
        .duration_seconds
        .filter(|duration| duration.is_finite() && *duration > 0.0)
        .map(|duration| (duration * 1_000_000.0) as u64)
        .unwrap_or(0)
}

fn run_playback(
    source: AudioSource,
    stop: Arc<AtomicBool>,
    start_seconds: f64,
    startup: Option<mpsc::Sender<Result<(), String>>>,
) -> Result<(), String> {
    if let AudioSource::File(path) = &source {
        let file = File::open(path).map_err(|error| error.to_string())?;
        let media = MediaSourceStream::new(Box::new(file), Default::default());
        return decode_media(media, stop, start_seconds, startup);
    }
    let AudioSource::Url(url) = source else {
        unreachable!()
    };
    let download = Arc::new(DownloadBuffer::new());
    let download_for_thread = download.clone();
    thread::spawn(move || {
        let result = (|| -> Result<(), String> {
            let request_url = without_ump(&url)?;
            let mut response = reqwest::blocking::Client::new()
                .get(request_url)
                .header("Origin", "https://music.youtube.com")
                .header("Referer", "https://music.youtube.com/")
                .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/153 Safari/537.36")
                .send()
                .map_err(|error| error.to_string())?;
            let status = response.status();
            let content_type = response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .unwrap_or("unknown")
                .to_string();
            if let Some(content_length) = response.content_length() {
                download_for_thread
                    .content_length
                    .store(content_length, Ordering::Release);
                eprintln!("VibeDeck audio content-length: {content_length}");
            }
            if !status.is_success() {
                return Err(format!("stream HTTP {status} content-type={content_type}"));
            }
            let mut chunk = vec![0_u8; 64 * 1024];
            let mut first_read = true;
            loop {
                let size = response
                    .read(&mut chunk)
                    .map_err(|error| error.to_string())?;
                if size == 0 {
                    break;
                }
                if first_read {
                    first_read = false;
                    let preview = chunk[..size.min(16)]
                        .iter()
                        .map(|byte| format!("{byte:02x}"))
                        .collect::<Vec<_>>()
                        .join(" ");
                    eprintln!("VibeDeck audio response: status={status} content-type={content_type} first-bytes={preview}");
                }
                let mut bytes = download_for_thread
                    .bytes
                    .lock()
                    .map_err(|_| "buffer poisoned".to_string())?;
                bytes.extend_from_slice(&chunk[..size]);
                download_for_thread.changed.notify_all();
            }
            Ok(())
        })();
        if let Err(error) = result {
            if let Ok(mut failure) = download_for_thread.failed.lock() {
                *failure = Some(error);
            }
        }
        if let Ok(bytes) = download_for_thread.bytes.lock() {
            eprintln!("VibeDeck audio download finished: bytes={}", bytes.len());
        }
        download_for_thread.finished.store(true, Ordering::Release);
        download_for_thread.changed.notify_all();
    });

    let mut downloaded = download
        .bytes
        .lock()
        .map_err(|_| "buffer poisoned".to_string())?;
    while !download.finished.load(Ordering::Acquire) {
        downloaded = download
            .changed
            .wait(downloaded)
            .map_err(|_| "buffer poisoned".to_string())?;
    }
    if let Some(error) = download.failed.lock().ok().and_then(|value| value.clone()) {
        return Err(error);
    }
    let source = Cursor::new(std::mem::take(&mut *downloaded));
    drop(downloaded);
    let media = MediaSourceStream::new(Box::new(source), Default::default());
    decode_media(media, stop, start_seconds, startup)
}

fn decode_media(
    media: MediaSourceStream,
    stop: Arc<AtomicBool>,
    start_seconds: f64,
    startup: Option<mpsc::Sender<Result<(), String>>>,
) -> Result<(), String> {
    let probed = get_probe()
        .format(
            &Hint::new(),
            media,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|error| error.to_string())?;
    let mut format = probed.format;
    let (track_id, codec_params) = {
        let track = format
            .default_track()
            .ok_or_else(|| "audio stream has no default track".to_string())?;
        (track.id, track.codec_params.clone())
    };
    let source_rate = codec_params.sample_rate.unwrap_or(44_100);
    let source_channels = codec_params
        .channels
        .map(|channels| channels.count() as u16)
        .unwrap_or(2);
    if let Some(frames) = codec_params.n_frames {
        DURATION_MICROS.store(
            ((frames as f64 / source_rate as f64) * 1_000_000.0) as u64,
            Ordering::Release,
        );
    }
    let mut decoder = get_codecs()
        .make(&codec_params, &DecoderOptions::default())
        .map_err(|error| error.to_string())?;
    if start_seconds > 0.0 {
        use symphonia::core::formats::{SeekMode, SeekTo};
        use symphonia::core::units::Time;
        let seconds = start_seconds.floor() as u64;
        let frac = start_seconds - seconds as f64;
        format
            .seek(
                SeekMode::Accurate,
                SeekTo::Time {
                    time: Time::new(seconds, frac),
                    track_id: Some(track_id),
                },
            )
            .map_err(|error| error.to_string())?;
    }
    eprintln!("VibeDeck audio source format: rate={source_rate} channels={source_channels}");
    let queue = Arc::new(PcmQueue {
        samples: Mutex::new(VecDeque::new()),
        changed: Condvar::new(),
    });
    let (stream, output_rate, output_channels) =
        open_output_stream(queue.clone(), stop.clone(), source_rate, source_channels)?;
    stream.play().map_err(|error| error.to_string())?;
    let mut startup = startup;
    let mut decoder_started = false;

    while !stop.load(Ordering::Acquire) {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(SymphoniaError::ResetRequired) => continue,
            Err(SymphoniaError::IoError(error))
                if error.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                eprintln!("VibeDeck audio decoder reached EOF");
                break;
            }
            Err(error) => {
                eprintln!("VibeDeck audio decoder packet error: {error}");
                return Err(error.to_string());
            }
        };
        let decoded = decoder.decode(&packet).map_err(|error| error.to_string())?;
        let spec = *decoded.spec();
        let mut samples = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
        match decoded {
            AudioBufferRef::F32(buffer) => {
                samples.copy_interleaved_ref(AudioBufferRef::F32(buffer))
            }
            AudioBufferRef::U8(buffer) => samples.copy_interleaved_ref(AudioBufferRef::U8(buffer)),
            AudioBufferRef::U16(buffer) => {
                samples.copy_interleaved_ref(AudioBufferRef::U16(buffer))
            }
            AudioBufferRef::U24(buffer) => {
                samples.copy_interleaved_ref(AudioBufferRef::U24(buffer))
            }
            AudioBufferRef::U32(buffer) => {
                samples.copy_interleaved_ref(AudioBufferRef::U32(buffer))
            }
            AudioBufferRef::S8(buffer) => samples.copy_interleaved_ref(AudioBufferRef::S8(buffer)),
            AudioBufferRef::S16(buffer) => {
                samples.copy_interleaved_ref(AudioBufferRef::S16(buffer))
            }
            AudioBufferRef::S24(buffer) => {
                samples.copy_interleaved_ref(AudioBufferRef::S24(buffer))
            }
            AudioBufferRef::S32(buffer) => {
                samples.copy_interleaved_ref(AudioBufferRef::S32(buffer))
            }
            AudioBufferRef::F64(buffer) => {
                samples.copy_interleaved_ref(AudioBufferRef::F64(buffer))
            }
        }
        let mut output = queue
            .samples
            .lock()
            .map_err(|_| "PCM queue poisoned".to_string())?;
        while output.len() > BUFFER_LIMIT && !stop.load(Ordering::Acquire) {
            output = queue
                .changed
                .wait(output)
                .map_err(|_| "PCM queue poisoned".to_string())?;
        }
        output.extend(resample_interleaved(
            samples.samples(),
            source_rate,
            output_rate,
            source_channels,
            output_channels,
        ));
        queue.changed.notify_all();
        if !decoder_started {
            decoder_started = true;
            if let Some(startup) = startup.take() {
                let _ = startup.send(Ok(()));
            }
        }
    }
    while !stop.load(Ordering::Acquire) {
        let has_samples = queue
            .samples
            .lock()
            .map(|samples| !samples.is_empty())
            .unwrap_or(false);
        if !has_samples {
            break;
        }
        thread::sleep(std::time::Duration::from_millis(20));
    }
    Ok(())
}

fn without_ump(value: &str) -> Result<String, String> {
    let mut parsed = url::Url::parse(value).map_err(|error| error.to_string())?;
    let mut serializer = url::form_urlencoded::Serializer::new(String::new());
    for (key, value) in parsed
        .query_pairs()
        .filter(|(key, _)| key != "ump" && key != "range")
    {
        serializer.append_pair(&key, &value);
    }
    let query = serializer.finish();
    parsed.set_query(Some(&query));
    Ok(parsed.to_string())
}

fn open_output_stream(
    queue: Arc<PcmQueue>,
    stop: Arc<AtomicBool>,
    requested_rate: u32,
    requested_channels: u16,
) -> Result<(cpal::Stream, u32, u16), String> {
    let device = cpal::default_host()
        .default_output_device()
        .ok_or_else(|| "no default audio output device".to_string())?;
    let config = device
        .supported_output_configs()
        .map_err(|error| error.to_string())?
        .find(|range| {
            range.channels() == requested_channels
                && range.min_sample_rate().0 <= requested_rate
                && range.max_sample_rate().0 >= requested_rate
        })
        .map(|range| range.with_sample_rate(cpal::SampleRate(requested_rate)))
        .or_else(|| device.default_output_config().ok())
        .ok_or_else(|| "no compatible audio output configuration".to_string())?;
    eprintln!(
        "VibeDeck audio output format: rate={} channels={} sample={:?}",
        config.sample_rate().0,
        config.channels(),
        config.sample_format()
    );
    let sample_rate = config.sample_rate().0 as f64;
    let channels = config.channels() as f64;
    let error_callback = |error| eprintln!("VibeDeck output error: {error}");
    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => device.build_output_stream(
            &config.config(),
            move |data: &mut [f32], _| fill_output(data, &queue, &stop, sample_rate, channels),
            error_callback,
            None,
        ),
        cpal::SampleFormat::I16 => device.build_output_stream(
            &config.config(),
            move |data: &mut [i16], _| fill_output(data, &queue, &stop, sample_rate, channels),
            error_callback,
            None,
        ),
        cpal::SampleFormat::U16 => device.build_output_stream(
            &config.config(),
            move |data: &mut [u16], _| fill_output(data, &queue, &stop, sample_rate, channels),
            error_callback,
            None,
        ),
        sample_format => return Err(format!("unsupported output format: {sample_format:?}")),
    };
    stream
        .map(|stream| (stream, config.sample_rate().0, config.channels()))
        .map_err(|error| error.to_string())
}

fn resample_interleaved(
    samples: &[f32],
    source_rate: u32,
    output_rate: u32,
    source_channels: u16,
    output_channels: u16,
) -> Vec<f32> {
    if samples.is_empty() || (source_rate == output_rate && source_channels == output_channels) {
        return samples.to_vec();
    }
    let source_channels = source_channels.max(1) as usize;
    let output_channels = output_channels.max(1) as usize;
    let frames = samples.len() / source_channels;
    if frames == 0 {
        return Vec::new();
    }
    let output_frames =
        ((frames as f64 * output_rate as f64 / source_rate as f64).round() as usize).max(1);
    let mut output = Vec::with_capacity(output_frames * output_channels);
    for frame in 0..output_frames {
        let position = frame as f64 * source_rate as f64 / output_rate as f64;
        let left = (position.floor() as usize).min(frames - 1);
        let right = (left + 1).min(frames - 1);
        let fraction = (position - left as f64) as f32;
        for channel in 0..output_channels {
            let source_channel = channel.min(source_channels - 1);
            let first = samples[left * source_channels + source_channel];
            let second = samples[right * source_channels + source_channel];
            output.push(first + (second - first) * fraction);
        }
    }
    output
}

fn fill_output<T: cpal::Sample + cpal::FromSample<f32>>(
    data: &mut [T],
    queue: &Arc<PcmQueue>,
    stop: &Arc<AtomicBool>,
    sample_rate: f64,
    channels: f64,
) {
    let mut samples = queue.samples.lock().expect("PCM queue poisoned");
    let paused = PAUSED.load(Ordering::Acquire);
    let volume = f32::from_bits(VOLUME_BITS.load(Ordering::Acquire));
    let levels = LEVELS.get_or_init(|| std::array::from_fn(|_| AtomicU32::new(0.0_f32.to_bits())));
    let channel_count = channels.max(1.0) as usize;
    let mut frame_sum = 0.0_f32;
    let mut analysis_frames = Vec::with_capacity(data.len() / channel_count.max(1));
    for (index, sample) in data.iter_mut().enumerate() {
        let value = if paused {
            0.0
        } else {
            samples.pop_front().unwrap_or(0.0) * volume
        };
        frame_sum += value;
        if (index + 1) % channel_count == 0 {
            analysis_frames.push(frame_sum / channel_count as f32);
            frame_sum = 0.0;
        }
        *sample = T::from_sample(value);
    }
    if let Ok(mut buffer) = ANALYZER_BUFFER
        .get_or_init(|| Mutex::new(Vec::with_capacity(FFT_SIZE * 2)))
        .lock()
    {
        buffer.extend(analysis_frames);
        if buffer.len() >= FFT_SIZE {
            let window = buffer[..FFT_SIZE].to_vec();
            buffer.drain(..FFT_HOP);
            drop(buffer);
            update_spectrum(&window, levels);
        }
    }
    if !paused && !stop.load(Ordering::Acquire) && sample_rate > 0.0 && channels > 0.0 {
        let advanced = data.len() as f64 / channels / sample_rate;
        POSITION_MICROS.fetch_add((advanced * 1_000_000.0) as u64, Ordering::AcqRel);
    }
    queue.changed.notify_all();
}

fn update_spectrum(samples: &[f32], levels: &[AtomicU32; LEVEL_BANDS]) {
    let fft = FFT_PLAN.get_or_init(|| {
        let mut planner = FftPlanner::<f32>::new();
        planner.plan_fft_forward(FFT_SIZE)
    });
    let mut input = samples
        .iter()
        .enumerate()
        .map(|(index, sample)| {
            let window = 0.5 - 0.5 * (std::f32::consts::TAU * index as f32 / FFT_SIZE as f32).cos();
            Complex::new(sample * window, 0.0)
        })
        .collect::<Vec<_>>();
    fft.process(&mut input);

    let nyquist = FFT_SIZE / 2;
    let mut magnitudes = [0.0_f32; LEVEL_BANDS];
    for (band, magnitude) in magnitudes.iter_mut().enumerate() {
        let low = (nyquist as f32).powf(band as f32 / LEVEL_BANDS as f32) as usize;
        let high = (nyquist as f32).powf((band + 1) as f32 / LEVEL_BANDS as f32) as usize;
        let start = low.min(nyquist - 1);
        let end = high.max(start + 1).min(nyquist);
        let average = (start..end)
            .map(|bin| input[bin].norm() / FFT_SIZE as f32)
            .sum::<f32>()
            / (end - start) as f32;
        *magnitude = (average * 14.0).sqrt().min(1.0);
    }
    for (index, level) in levels.iter().enumerate() {
        level.store(magnitudes[index].to_bits(), Ordering::Release);
    }
}
