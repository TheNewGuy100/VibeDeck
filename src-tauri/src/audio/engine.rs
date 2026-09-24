use std::collections::VecDeque;
use std::io::{Cursor, Read, Seek, SeekFrom};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rustfft::{num_complex::Complex, FftPlanner};
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
static CURRENT_URL: OnceLock<Mutex<Option<String>>> = OnceLock::new();
static PAUSED: AtomicBool = AtomicBool::new(false);
static ACTIVE: AtomicBool = AtomicBool::new(false);
static VOLUME_BITS: AtomicU32 = AtomicU32::new(1.0_f32.to_bits());
static POSITION_MICROS: AtomicU64 = AtomicU64::new(0);
static DURATION_MICROS: AtomicU64 = AtomicU64::new(0);
static LEVELS: OnceLock<[AtomicU32; LEVEL_BANDS]> = OnceLock::new();
static ANALYZER_BUFFER: OnceLock<Mutex<Vec<f32>>> = OnceLock::new();
static FFT_PLAN: OnceLock<Arc<dyn rustfft::Fft<f32>>> = OnceLock::new();

pub struct AudioEngine;

impl AudioEngine {
    pub fn play_url(url: String) -> Result<(), String> {
        Self::stop();
        ACTIVE.store(true, Ordering::Release);
        *CURRENT_URL.get_or_init(|| Mutex::new(None)).lock().map_err(|_| "Audio state poisoned".to_string())? = Some(url.clone());
        PAUSED.store(false, Ordering::Release);
        POSITION_MICROS.store(0, Ordering::Release);
        DURATION_MICROS.store(0, Ordering::Release);
        let stop = Arc::new(AtomicBool::new(false));
        *STOP
            .get_or_init(|| Mutex::new(None))
            .lock()
            .map_err(|_| "Audio state poisoned".to_string())? = Some(stop.clone());
        thread::spawn(move || {
            if let Err(error) = run_playback(url, stop.clone(), 0.0) {
                if !stop.load(Ordering::Acquire) {
                    eprintln!("VibeDeck audio error: {error}");
                }
            }
            if !stop.load(Ordering::Acquire) {
                ACTIVE.store(false, Ordering::Release);
            }
            stop.store(true, Ordering::Release);
        });
        Ok(())
    }

    pub fn stop() {
        ACTIVE.store(false, Ordering::Release);
        if let Some(state) = STOP.get() {
            if let Ok(mut current) = state.lock() {
                if let Some(stop) = current.take() {
                    stop.store(true, Ordering::Release);
                }
            }
        }
    }

    pub fn seek(seconds: f64) -> Result<(), String> {
        let url = CURRENT_URL
            .get_or_init(|| Mutex::new(None))
            .lock()
            .map_err(|_| "Audio state poisoned".to_string())?
            .clone()
            .ok_or_else(|| "Nenhuma faixa nativa está carregada.".to_string())?;
        let paused = PAUSED.load(Ordering::Acquire);
        Self::stop();
        POSITION_MICROS.store((seconds.max(0.0) * 1_000_000.0) as u64, Ordering::Release);
        let stop = Arc::new(AtomicBool::new(false));
        *STOP.get_or_init(|| Mutex::new(None)).lock().map_err(|_| "Audio state poisoned".to_string())? = Some(stop.clone());
        thread::spawn(move || {
            if let Err(error) = run_playback(url, stop.clone(), seconds.max(0.0)) {
                if !stop.load(Ordering::Acquire) { eprintln!("VibeDeck audio seek error: {error}"); }
            }
            stop.store(true, Ordering::Release);
        });
        PAUSED.store(paused, Ordering::Release);
        Ok(())
    }

    pub fn set_paused(paused: bool) {
        PAUSED.store(paused, Ordering::Release);
    }

    pub fn is_active() -> bool {
        ACTIVE.load(Ordering::Acquire)
    }

    pub fn is_paused() -> bool {
        PAUSED.load(Ordering::Acquire)
    }

    pub fn set_volume(volume: f32) {
        VOLUME_BITS.store(volume.clamp(0.0, 1.0).to_bits(), Ordering::Release);
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
}

fn run_playback(url: String, stop: Arc<AtomicBool>, start_seconds: f64) -> Result<(), String> {
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

    let mut downloaded = download.bytes.lock().map_err(|_| "buffer poisoned".to_string())?;
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
        DURATION_MICROS.store(((frames as f64 / source_rate as f64) * 1_000_000.0) as u64, Ordering::Release);
    }
    let mut decoder = get_codecs()
        .make(&codec_params, &DecoderOptions::default())
        .map_err(|error| error.to_string())?;
    if start_seconds > 0.0 {
        use symphonia::core::formats::{SeekMode, SeekTo};
        use symphonia::core::units::Time;
        let seconds = start_seconds.floor() as u64;
        let frac = start_seconds - seconds as f64;
        format.seek(SeekMode::Accurate, SeekTo::Time { time: Time::new(seconds, frac), track_id: Some(track_id) }).map_err(|error| error.to_string())?;
    }
    eprintln!("VibeDeck audio source format: rate={source_rate} channels={source_channels}");
    let queue = Arc::new(PcmQueue {
        samples: Mutex::new(VecDeque::new()),
        changed: Condvar::new(),
    });
    let (stream, output_rate, output_channels) = open_output_stream(queue.clone(), stop.clone(), source_rate, source_channels)?;
    stream.play().map_err(|error| error.to_string())?;

    while !stop.load(Ordering::Acquire) {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(SymphoniaError::ResetRequired) => continue,
            Err(SymphoniaError::IoError(error))
                if error.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                eprintln!("VibeDeck audio decoder reached EOF");
                break
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
        output.extend(resample_interleaved(samples.samples(), source_rate, output_rate, source_channels, output_channels));
        queue.changed.notify_all();
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
    eprintln!("VibeDeck audio output format: rate={} channels={} sample={:?}", config.sample_rate().0, config.channels(), config.sample_format());
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

fn resample_interleaved(samples: &[f32], source_rate: u32, output_rate: u32, source_channels: u16, output_channels: u16) -> Vec<f32> {
    if samples.is_empty() || (source_rate == output_rate && source_channels == output_channels) {
        return samples.to_vec();
    }
    let source_channels = source_channels.max(1) as usize;
    let output_channels = output_channels.max(1) as usize;
    let frames = samples.len() / source_channels;
    if frames == 0 { return Vec::new(); }
    let output_frames = ((frames as f64 * output_rate as f64 / source_rate as f64).round() as usize).max(1);
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
    if let Ok(mut buffer) = ANALYZER_BUFFER.get_or_init(|| Mutex::new(Vec::with_capacity(FFT_SIZE * 2))).lock() {
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
            .sum::<f32>() / (end - start) as f32;
        *magnitude = (average * 14.0).sqrt().min(1.0);
    }
    for (index, level) in levels.iter().enumerate() {
        level.store(magnitudes[index].to_bits(), Ordering::Release);
    }
}
