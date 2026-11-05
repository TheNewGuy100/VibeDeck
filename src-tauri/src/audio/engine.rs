use std::collections::VecDeque;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
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

const MAX_TRACED_UMP_SEGMENTS: usize = 16;
const MAX_TRACED_UMP_FORMATS: usize = 8;
const MAX_UMP_METADATA_BYTES: usize = 64 * 1024;
const MAX_TRACKED_UMP_STREAMS: usize = 8;
const MAX_TRACKED_UMP_SEGMENT_NUMBERS: usize = 1024;
const MAX_TRACKED_UMP_HEADERS: usize = 64;

/// Incrementally unwraps the UMP envelope while keeping only a partial part
/// header plus the current HTTP read chunk in memory.
struct UmpDecoder {
    pending: Vec<u8>,
    cursor: usize,
    part: Option<(u32, u32)>,
    media_header_id: Option<u8>,
    media_part_matches: Option<bool>,
    media_header_payload: Vec<u8>,
    media_end_payload: Vec<u8>,
    format_payload: Vec<u8>,
    current_segment: Option<usize>,
    current_segment_media_prefix: Vec<u8>,
    header_states: Vec<UmpHeaderState>,
    last_header_id: Option<u8>,
    selected_itag: Option<i32>,
    /// Set once any `FORMAT_INITIALIZATION_METADATA` part was parsed. A response
    /// that declares formats may announce the audio format only after the first
    /// segments of another format, so no media can be forwarded before the
    /// audio format is selected.
    saw_format_metadata: bool,
    streams: Vec<UmpAssemblyStream>,
    segments: Vec<UmpSegmentDiagnostic>,
    formats: Vec<UmpFormatDiagnostic>,
    saw_media: bool,
    response_saw_media: bool,
    part_counts: Vec<(u32, u64)>,
    media_output_bytes: u64,
}

/// Media header plus how many bytes of its segment were already consumed. UMP
/// responses can interleave chunks of different segments, so the position of a
/// segment cannot be tracked by one decoder-wide cursor.
struct UmpHeaderState {
    header: UmpMediaHeader,
    cursor: u64,
    /// Deduplication is decided once per header on its first media chunk, so
    /// later chunks of the same segment keep being forwarded.
    duplicate: Option<bool>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct UmpMediaHeader {
    header_id: Option<u8>,
    itag: Option<i32>,
    is_initialization_segment: Option<bool>,
    segment_num: Option<u64>,
    segment_byte_range_start: Option<u64>,
    segment_length_bytes: Option<u64>,
}

/// Sanitized per-segment metadata for diagnosis. It intentionally contains no
/// media bytes, URLs, identifiers, or headers.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UmpSegmentDiagnostic {
    pub order: u32,
    pub header_id: Option<u8>,
    pub itag: Option<i32>,
    pub is_initialization_segment: Option<bool>,
    pub segment_num: Option<u64>,
    pub segment_byte_range_start: Option<u64>,
    pub segment_length_bytes: Option<u64>,
    pub media_bytes: u64,
    pub emitted_media_bytes: u64,
    pub skipped_media_bytes: u64,
    pub first_media_box: Option<String>,
    pub saw_media_end: bool,
}

/// Sanitized format metadata declared by the UMP `FORMAT_INITIALIZATION_METADATA`
/// part. The MIME value is reduced to a fixed category.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UmpFormatDiagnostic {
    pub itag: Option<i32>,
    pub mime_type: String,
    pub has_init_range: bool,
}

/// Tracks how many contiguous media bytes of one format were already handed to
/// the demuxer. SABR contexts can restart inside a single relay session and
/// re-send their format initialization plus earlier segments; those bytes must
/// not be appended to the growing demux input.
struct UmpAssemblyStream {
    itag: Option<i32>,
    written_bytes: u64,
    seen_segments: Vec<u64>,
}

impl UmpDecoder {
    fn new() -> Self {
        Self {
            pending: Vec::with_capacity(16),
            cursor: 0,
            part: None,
            media_header_id: None,
            media_part_matches: None,
            media_header_payload: Vec::new(),
            media_end_payload: Vec::new(),
            format_payload: Vec::new(),
            current_segment: None,
            current_segment_media_prefix: Vec::new(),
            header_states: Vec::new(),
            last_header_id: None,
            selected_itag: None,
            saw_format_metadata: false,
            streams: Vec::new(),
            segments: Vec::new(),
            formats: Vec::new(),
            saw_media: false,
            response_saw_media: false,
            part_counts: Vec::new(),
            media_output_bytes: 0,
        }
    }

    fn feed(&mut self, input: &[u8]) -> Result<Vec<u8>, String> {
        self.pending.extend_from_slice(input);
        let mut audio = Vec::with_capacity(input.len());
        loop {
            if let Some((part_type, remaining)) = self.part {
                if self.cursor == self.pending.len() {
                    break;
                }
                let count = (remaining as usize).min(self.pending.len() - self.cursor);
                let payload = &self.pending[self.cursor..self.cursor + count];
                if part_type == 21 {
                    let mut start = 0;
                    if self.media_part_matches.is_none() {
                        let Some((&header_id, _)) = payload.split_first() else {
                            break;
                        };
                        self.media_header_id = Some(header_id);
                        self.media_part_matches = Some(self.media_header_is_selected(header_id));
                        start = 1;
                    }
                    let media_count = (count - start) as u64;
                    let mut skipped_media = 0_u64;
                    let mut emitted_media = 0_u64;
                    let header_index = self
                        .media_header_id
                        .and_then(|header_id| self.header_state_index(header_id));
                    if self.media_part_matches == Some(true) {
                        if let Some(index) = header_index {
                            self.saw_media = true;
                            self.response_saw_media = true;
                            let (skip, emitted) = UmpDecoder::assembly_window(
                                &mut self.streams,
                                &mut self.header_states[index],
                                count - start,
                            );
                            skipped_media = skip as u64;
                            emitted_media = emitted as u64;
                            audio.extend_from_slice(&payload[start + skip..]);
                            self.media_output_bytes += emitted as u64;
                        }
                    }
                    if media_count > 0 {
                        if let Some(header_id) = self.media_header_id {
                            let traced = self
                                .segments
                                .iter()
                                .rposition(|segment| segment.header_id == Some(header_id));
                            if let Some(index) = traced {
                                if self.current_segment != Some(index) {
                                    self.current_segment = Some(index);
                                    self.current_segment_media_prefix.clear();
                                }
                                if let Some(segment) = self.segments.get_mut(index) {
                                    segment.media_bytes += media_count;
                                    segment.emitted_media_bytes += emitted_media;
                                    segment.skipped_media_bytes += skipped_media;
                                    if segment.first_media_box.is_none()
                                        && self.current_segment_media_prefix.len() < 8
                                    {
                                        let remaining = 8 - self.current_segment_media_prefix.len();
                                        let take = (count - start).min(remaining);
                                        self.current_segment_media_prefix
                                            .extend_from_slice(&payload[start..start + take]);
                                        if self.current_segment_media_prefix.len() >= 4 {
                                            let kind = classify_media_box(
                                                &self.current_segment_media_prefix,
                                            );
                                            if self.current_segment_media_prefix.len() >= 8
                                                || kind != "unknown"
                                            {
                                                segment.first_media_box = Some(kind.to_string());
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                } else if part_type == 20 {
                    if self.media_header_payload.len().saturating_add(count)
                        > MAX_UMP_METADATA_BYTES
                    {
                        return Err("UMP media header exceeds the supported size".to_string());
                    }
                    self.media_header_payload.extend_from_slice(payload);
                } else if part_type == 22 {
                    if self.media_end_payload.len() < 8 {
                        let take = count.min(8 - self.media_end_payload.len());
                        self.media_end_payload.extend_from_slice(&payload[..take]);
                    }
                } else if part_type == 42 {
                    if self.format_payload.len().saturating_add(count) > MAX_UMP_METADATA_BYTES {
                        return Err("UMP format metadata exceeds the supported size".to_string());
                    }
                    self.format_payload.extend_from_slice(payload);
                }
                self.cursor += count;
                let left = remaining - count as u32;
                self.part = (left > 0).then_some((part_type, left));
                if left == 0 {
                    self.media_header_id = None;
                    self.media_part_matches = None;
                    if part_type == 20 {
                        let header = parse_ump_media_header(&self.media_header_payload)?;
                        self.media_header_payload.clear();
                        self.record_segment_header(header);
                    } else if part_type == 22 {
                        if let Some(&header_id) = self.media_end_payload.first() {
                            if let Some(segment) = self.segments.iter_mut().rev().find(|segment| {
                                segment.header_id == Some(header_id) && !segment.saw_media_end
                            }) {
                                segment.saw_media_end = true;
                            }
                        }
                        self.media_end_payload.clear();
                    } else if part_type == 42 {
                        if let Ok(format) = parse_ump_format_initialization(&self.format_payload) {
                            self.observe_format(format);
                        }
                        self.format_payload.clear();
                    }
                }
                continue;
            }

            let Some((part_type, after_type)) = read_ump_varint(&self.pending, self.cursor)? else {
                break;
            };
            let Some((part_size, after_size)) = read_ump_varint(&self.pending, after_type)? else {
                break;
            };
            self.cursor = after_size;
            if let Some((_, count)) = self
                .part_counts
                .iter_mut()
                .find(|(kind, _)| *kind == part_type)
            {
                *count += 1;
            } else {
                self.part_counts.push((part_type, 1));
            }
            if part_type == 21 && part_size == 0 {
                return Err("UMP media part is missing its header id".to_string());
            }
            self.part = (part_size > 0).then_some((part_type, part_size));
        }
        if self.cursor > 0 {
            self.pending.drain(..self.cursor);
            self.cursor = 0;
        }
        Ok(audio)
    }

    fn header_state_index(&self, header_id: u8) -> Option<usize> {
        self.header_states
            .iter()
            .position(|state| state.header.header_id == Some(header_id))
    }

    /// UMP responses can carry several formats (for example audio and video)
    /// whose segments share one envelope. The demuxer input is a single media
    /// stream, so only segments of one format may be forwarded. The selected
    /// format is the first audio format declared by
    /// `FORMAT_INITIALIZATION_METADATA`; responses that never declare metadata
    /// keep the previous behavior of forwarding the latest media header only.
    fn media_header_is_selected(&self, header_id: u8) -> bool {
        if let Some(itag) = self.selected_itag {
            return self
                .header_states
                .iter()
                .find(|state| state.header.header_id == Some(header_id))
                .and_then(|state| state.header.itag)
                == Some(itag);
        }
        // A response that already declared formats may announce the audio
        // format only after the first segments of another format (SABR can send
        // a video format initialization and its segments first). Forwarding
        // those bytes would put a foreign initialization segment in front of
        // the demuxer input and make it select the wrong track, so nothing is
        // forwarded until the audio format is known.
        if self.saw_format_metadata {
            return false;
        }
        // Responses without format metadata keep the legacy behavior: only the
        // most recently announced media header is forwarded.
        self.last_header_id == Some(header_id)
    }

    fn observe_format(&mut self, format: UmpFormatDiagnostic) {
        self.saw_format_metadata = true;
        if self.selected_itag.is_none()
            && format.mime_type.starts_with("audio")
            && format.itag.is_some()
        {
            self.selected_itag = format.itag;
        }
        if self.formats.len() < MAX_TRACED_UMP_FORMATS {
            self.formats.push(format);
        }
    }

    fn record_segment_header(&mut self, header: UmpMediaHeader) {
        self.current_segment_media_prefix.clear();
        self.last_header_id = header.header_id;
        // Header state is tracked independently of the bounded diagnostic
        // trace: segments keep their own cursor for the whole session.
        if let Some(header_id) = header.header_id {
            if let Some(index) = self.header_state_index(header_id) {
                self.header_states.remove(index);
            }
            if self.header_states.len() >= MAX_TRACKED_UMP_HEADERS {
                self.header_states.remove(0);
            }
            self.header_states.push(UmpHeaderState {
                header,
                cursor: 0,
                duplicate: None,
            });
        }
        if self.segments.len() >= MAX_TRACED_UMP_SEGMENTS {
            self.current_segment = None;
            return;
        }
        let order = self.segments.len() as u32;
        self.segments.push(UmpSegmentDiagnostic {
            order,
            header_id: header.header_id,
            itag: header.itag,
            is_initialization_segment: header.is_initialization_segment,
            segment_num: header.segment_num,
            segment_byte_range_start: header.segment_byte_range_start,
            segment_length_bytes: header.segment_length_bytes,
            media_bytes: 0,
            emitted_media_bytes: 0,
            skipped_media_bytes: 0,
            first_media_box: None,
            saw_media_end: false,
        });
        self.current_segment = Some(self.segments.len() - 1);
    }

    /// SABR may restart its streaming context in the middle of a relay session
    /// and re-send the format initialization together with already delivered
    /// segments. The demuxer input is a single linear byte stream, so a payload
    /// is only forwarded when it advances that stream: a repeated segment
    /// number, a repeated initialization, or a byte range already written is
    /// discarded (partially, when the payload overlaps the written range).
    fn assembly_window(
        streams: &mut Vec<UmpAssemblyStream>,
        state: &mut UmpHeaderState,
        media_len: usize,
    ) -> (usize, usize) {
        let header = state.header;
        let cursor = state.cursor;
        let first_chunk = cursor == 0 && state.duplicate.is_none();
        state.cursor = cursor.saturating_add(media_len as u64);
        let itag = header.itag;
        let index = match streams.iter().position(|stream| stream.itag == itag) {
            Some(index) => index,
            None => {
                if streams.len() >= MAX_TRACKED_UMP_STREAMS {
                    return (0, media_len);
                }
                streams.push(UmpAssemblyStream {
                    itag,
                    written_bytes: 0,
                    seen_segments: Vec::new(),
                });
                streams.len() - 1
            }
        };
        let stream = &mut streams[index];
        if first_chunk {
            let duplicate = (header.is_initialization_segment == Some(true)
                && stream.written_bytes > 0)
                || header
                    .segment_num
                    .is_some_and(|number| stream.seen_segments.contains(&number));
            state.duplicate = Some(duplicate);
            if let Some(number) = header.segment_num {
                if stream.seen_segments.len() < MAX_TRACKED_UMP_SEGMENT_NUMBERS
                    && !stream.seen_segments.contains(&number)
                {
                    stream.seen_segments.push(number);
                }
            }
        }
        if state.duplicate == Some(true) {
            return (media_len, 0);
        }
        let skip = header
            .segment_byte_range_start
            .map(|start| {
                let from = start.saturating_add(cursor);
                stream
                    .written_bytes
                    .saturating_sub(from)
                    .min(media_len as u64) as usize
            })
            .unwrap_or(0);
        let emitted = media_len - skip;
        let frontier = header
            .segment_byte_range_start
            .map(|start| start.saturating_add(cursor).saturating_add(emitted as u64))
            .unwrap_or_else(|| stream.written_bytes.saturating_add(emitted as u64));
        stream.written_bytes = stream.written_bytes.max(frontier);
        (skip, emitted)
    }

    fn finish(self) -> Result<(), String> {
        if self.part.is_some() || !self.pending.is_empty() {
            return Err("UMP response ended in a partial part".to_string());
        }
        if !self.saw_media {
            return Err("UMP response contained no media part".to_string());
        }
        Ok(())
    }

    /// A captured response body is a complete UMP envelope, although media
    /// framing state (notably the selected header id) continues across bodies.
    /// SABR responses may legitimately contain control parts only, so a body
    /// without media is accepted here; `finish` still requires media overall.
    fn finish_response(&mut self) -> Result<(), String> {
        if self.part.is_some() || !self.pending.is_empty() {
            return Err("UMP response ended in a partial part".to_string());
        }
        self.response_saw_media = false;
        Ok(())
    }
}

/// UMP varints use a leading prefix to encode a 1-5 byte length. The five-byte
/// form carries an unsigned 32-bit little-endian value after its prefix byte.
fn read_ump_varint(bytes: &[u8], offset: usize) -> Result<Option<(u32, usize)>, String> {
    let Some(&first) = bytes.get(offset) else {
        return Ok(None);
    };
    let size = if first < 0x80 {
        1
    } else if first < 0xc0 {
        2
    } else if first < 0xe0 {
        3
    } else if first < 0xf0 {
        4
    } else if first < 0xf8 {
        5
    } else {
        return Err("Invalid UMP varint prefix".to_string());
    };
    if bytes.len().saturating_sub(offset) < size {
        return Ok(None);
    }
    let value = match size {
        1 => first as u32,
        2 => ((first & 0x3f) as u32) | ((bytes[offset + 1] as u32) << 6),
        3 => {
            ((first & 0x1f) as u32)
                | ((bytes[offset + 1] as u32) << 5)
                | ((bytes[offset + 2] as u32) << 13)
        }
        4 => {
            ((first & 0x0f) as u32)
                | ((bytes[offset + 1] as u32) << 4)
                | ((bytes[offset + 2] as u32) << 12)
                | ((bytes[offset + 3] as u32) << 20)
        }
        _ => u32::from_le_bytes(bytes[offset + 1..offset + 5].try_into().unwrap()),
    };
    Ok(Some((value, offset + size)))
}

/// Protobuf varints use little-endian base-128 groups, unlike UMP's outer framing.
fn read_protobuf_varint(bytes: &[u8], offset: usize) -> Result<Option<(u64, usize)>, String> {
    let mut value = 0u64;
    for index in 0..10 {
        let position = offset
            .checked_add(index)
            .ok_or_else(|| "Invalid UMP media header protobuf offset".to_string())?;
        let Some(&byte) = bytes.get(position) else {
            return Ok(None);
        };

        if index == 9 && byte > 1 {
            return Err("Invalid UMP media header protobuf varint overflow".to_string());
        }
        value |= u64::from(byte & 0x7f) << (index * 7);
        if byte & 0x80 == 0 {
            let end = position
                .checked_add(1)
                .ok_or_else(|| "Invalid UMP media header protobuf offset".to_string())?;
            return Ok(Some((value, end)));
        }
    }

    Err("Invalid UMP media header protobuf varint overflow".to_string())
}

fn parse_ump_media_header(payload: &[u8]) -> Result<UmpMediaHeader, String> {
    let mut offset = 0;
    let mut header = UmpMediaHeader::default();
    while offset < payload.len() {
        let (key, after_key) = read_protobuf_varint(payload, offset)?
            .ok_or_else(|| "Truncated UMP media header protobuf".to_string())?;
        offset = after_key;
        let key = u32::try_from(key)
            .map_err(|_| "Invalid UMP media header protobuf field key".to_string())?;
        let field = key >> 3;
        let wire_type = key & 7;
        if field == 0 || field > 0x1fff_ffff {
            return Err("Invalid UMP media header protobuf field key".to_string());
        }
        if field == 1 && wire_type == 0 {
            let (value, end) = read_protobuf_varint(payload, offset)?
                .ok_or_else(|| "Truncated UMP media header id".to_string())?;
            header.header_id =
                Some(u8::try_from(value).map_err(|_| "Invalid UMP media header id".to_string())?);
            offset = end;
            continue;
        }
        if matches!(field, 3 | 8 | 9) && wire_type == 0 {
            let (value, end) = read_protobuf_varint(payload, offset)?
                .ok_or_else(|| "Truncated UMP media header segment metadata".to_string())?;
            match field {
                3 => header.itag = Some(value as i64 as i32),
                8 => header.is_initialization_segment = Some(value != 0),
                _ => header.segment_num = Some(value),
            }
            offset = end;
            continue;
        }
        if matches!(field, 6 | 14) && wire_type == 0 {
            let (value, end) = read_protobuf_varint(payload, offset)?
                .ok_or_else(|| "Truncated UMP media header segment range".to_string())?;
            if field == 6 {
                header.segment_byte_range_start = Some(value);
            } else {
                header.segment_length_bytes = Some(value);
            }
            offset = end;
            continue;
        }
        offset = match wire_type {
            0 => read_protobuf_varint(payload, offset)?
                .map(|(_, end)| end)
                .ok_or_else(|| "Truncated UMP media header protobuf".to_string())?,
            1 => offset
                .checked_add(8)
                .ok_or_else(|| "Truncated UMP media header protobuf".to_string())?,
            2 => {
                let (size, data_start) = read_protobuf_varint(payload, offset)?
                    .ok_or_else(|| "Truncated UMP media header protobuf".to_string())?;
                let size = usize::try_from(size)
                    .map_err(|_| "Truncated UMP media header protobuf".to_string())?;
                data_start
                    .checked_add(size)
                    .ok_or_else(|| "Truncated UMP media header protobuf".to_string())?
            }
            5 => offset
                .checked_add(4)
                .ok_or_else(|| "Truncated UMP media header protobuf".to_string())?,
            _ => return Err("Invalid UMP media header protobuf wire type".to_string()),
        };
        if offset > payload.len() {
            return Err("Truncated UMP media header protobuf".to_string());
        }
    }
    if header.header_id.is_none() {
        return Err("UMP media header has no header id".to_string());
    }
    Ok(header)
}

fn bmff_box_kind(tag: &[u8]) -> Option<&'static str> {
    Some(match tag {
        b"ftyp" => "bmff_file_type",
        b"styp" => "bmff_segment_type",
        b"moov" => "bmff_movie_header",
        b"moof" => "bmff_media_fragment",
        b"mdat" => "bmff_media_data",
        b"sidx" => "bmff_segment_index",
        b"emsg" => "bmff_event_message",
        b"free" => "bmff_free",
        _ => return None,
    })
}

fn classify_media_box(prefix: &[u8]) -> &'static str {
    if prefix.len() < 4 {
        return "short";
    }
    match &prefix[..4] {
        [0x1a, 0x45, 0xdf, 0xa3] => return "webm_header",
        [0x1f, 0x43, 0xb6, 0x75] => return "webm_cluster",
        [0x18, 0x53, 0x80, 0x67] => return "webm_segment",
        _ => {}
    }
    // BMFF boxes are size-prefixed, so the four-character type follows the
    // 32-bit size. Raw fixtures may still provide the type directly.
    if prefix.len() >= 8 {
        if let Some(kind) = bmff_box_kind(&prefix[4..8]) {
            return kind;
        }
    }
    bmff_box_kind(&prefix[..4]).unwrap_or("unknown")
}

fn mime_category(mime: &str) -> &'static str {
    match mime
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "audio/mp4" => "audio_mp4",
        "audio/webm" => "audio_webm",
        "video/mp4" => "video_mp4",
        "video/webm" => "video_webm",
        _ => "other",
    }
}

fn skip_protobuf_field(
    payload: &[u8],
    offset: usize,
    wire_type: u32,
    limit: usize,
) -> Result<usize, String> {
    let next = match wire_type {
        0 => read_protobuf_varint(payload, offset)?
            .map(|(_, end)| end)
            .ok_or_else(|| "Truncated UMP metadata protobuf".to_string())?,
        1 => offset
            .checked_add(8)
            .ok_or_else(|| "Truncated UMP metadata protobuf".to_string())?,
        2 => {
            let (size, data_start) = read_protobuf_varint(payload, offset)?
                .ok_or_else(|| "Truncated UMP metadata protobuf".to_string())?;
            let size =
                usize::try_from(size).map_err(|_| "Truncated UMP metadata protobuf".to_string())?;
            data_start
                .checked_add(size)
                .ok_or_else(|| "Truncated UMP metadata protobuf".to_string())?
        }
        5 => offset
            .checked_add(4)
            .ok_or_else(|| "Truncated UMP metadata protobuf".to_string())?,
        _ => return Err("Invalid UMP metadata protobuf wire type".to_string()),
    };
    if next > limit {
        return Err("Truncated UMP metadata protobuf".to_string());
    }
    Ok(next)
}

fn parse_format_id_itag(payload: &[u8]) -> Result<Option<i32>, String> {
    let mut offset = 0;
    let mut itag = None;
    while offset < payload.len() {
        let (key, after_key) = read_protobuf_varint(payload, offset)?
            .ok_or_else(|| "Truncated format id".to_string())?;
        offset = after_key;
        let key = u32::try_from(key).map_err(|_| "Invalid format id".to_string())?;
        let field = key >> 3;
        let wire_type = key & 7;
        if field == 1 && wire_type == 0 {
            let (value, end) = read_protobuf_varint(payload, offset)?
                .ok_or_else(|| "Truncated format id".to_string())?;
            itag = Some(value as i64 as i32);
            offset = end;
        } else {
            offset = skip_protobuf_field(payload, offset, wire_type, payload.len())?;
        }
    }
    Ok(itag)
}

fn parse_ump_format_initialization(payload: &[u8]) -> Result<UmpFormatDiagnostic, String> {
    let mut offset = 0;
    let mut itag = None;
    let mut mime_type = "other".to_string();
    let mut has_init_range = false;
    while offset < payload.len() {
        let (key, after_key) = read_protobuf_varint(payload, offset)?
            .ok_or_else(|| "Truncated UMP format metadata".to_string())?;
        offset = after_key;
        let key =
            u32::try_from(key).map_err(|_| "Invalid UMP format metadata field key".to_string())?;
        let field = key >> 3;
        let wire_type = key & 7;
        if field == 0 {
            return Err("Invalid UMP format metadata field key".to_string());
        }
        if matches!((field, wire_type), (2, 2) | (5, 2) | (6, 2)) {
            let (size, data_start) = read_protobuf_varint(payload, offset)?
                .ok_or_else(|| "Truncated UMP format metadata".to_string())?;
            let size =
                usize::try_from(size).map_err(|_| "Truncated UMP format metadata".to_string())?;
            let end = data_start
                .checked_add(size)
                .ok_or_else(|| "Truncated UMP format metadata".to_string())?;
            if end > payload.len() {
                return Err("Truncated UMP format metadata".to_string());
            }
            match field {
                2 => itag = parse_format_id_itag(&payload[data_start..end])?,
                5 => {
                    let raw = std::str::from_utf8(&payload[data_start..end]).unwrap_or("");
                    mime_type = mime_category(raw).to_string();
                }
                _ => has_init_range = true,
            }
            offset = end;
            continue;
        }
        offset = skip_protobuf_field(payload, offset, wire_type, payload.len())?;
    }
    Ok(UmpFormatDiagnostic {
        itag,
        mime_type,
        has_init_range,
    })
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CapturedUmpParseSummary {
    pub outcome: String,
    pub extracted_media_bytes: u64,
    pub part_kinds: Vec<UmpPartCount>,
    pub segments: Vec<UmpSegmentDiagnostic>,
    pub formats: Vec<UmpFormatDiagnostic>,
}

/// Passes an already-captured complete UMP response through the Rust UMP
/// parser. Only sanitized counts/status are returned; media bytes are dropped.
pub(crate) fn parse_captured_ump_for_diagnostics(bytes: &[u8]) -> CapturedUmpParseSummary {
    let mut decoder = UmpDecoder::new();
    match decoder.feed(bytes) {
        Ok(media) => {
            let extracted_media_bytes = media.len() as u64;
            let outcome = if decoder.finish_response().is_ok() && extracted_media_bytes > 0 {
                "media_extracted"
            } else {
                "no_complete_media"
            };
            let part_kinds = decoder
                .part_counts
                .iter()
                .take(32)
                .map(|(kind, count)| UmpPartCount {
                    kind: *kind,
                    count: *count,
                })
                .collect();
            CapturedUmpParseSummary {
                outcome: outcome.into(),
                extracted_media_bytes,
                part_kinds,
                segments: decoder.segments,
                formats: decoder.formats,
            }
        }
        Err(_) => CapturedUmpParseSummary {
            outcome: "parse_error".into(),
            extracted_media_bytes: 0,
            part_kinds: decoder
                .part_counts
                .iter()
                .take(32)
                .map(|(kind, count)| UmpPartCount {
                    kind: *kind,
                    count: *count,
                })
                .collect(),
            segments: decoder.segments,
            formats: decoder.formats,
        },
    }
}

/// Classifies a bounded non-success UMP body without decoding or retaining its
/// payload bytes. Only framing part IDs and parser completeness are returned.
fn classify_ump_error_body(bytes: &[u8]) -> String {
    let mut offset = 0;
    let mut parts = Vec::<(u32, u32)>::new();
    let mut state = "complete";
    while offset < bytes.len() {
        let Some((kind, after_kind)) = read_ump_varint(bytes, offset).ok().flatten() else {
            state = "invalid_or_partial_header";
            break;
        };
        let Some((length, after_length)) = read_ump_varint(bytes, after_kind).ok().flatten() else {
            state = "invalid_or_partial_header";
            break;
        };
        let Some(end) = after_length.checked_add(length as usize) else {
            state = "invalid_length";
            break;
        };
        if end > bytes.len() {
            state = "partial_part";
            break;
        }
        if let Some((_, count)) = parts.iter_mut().find(|(part_kind, _)| *part_kind == kind) {
            *count += 1;
        } else {
            parts.push((kind, 1));
        }
        offset = end;
    }
    let kinds = parts
        .iter()
        .map(|(kind, count)| format!("{kind}:{count}"))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "state={state};kinds={}",
        if kinds.is_empty() { "none" } else { &kinds }
    )
}

fn sanitized_body_read_category(error: &std::io::Error) -> &'static str {
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(error);
    while let Some(current) = source {
        if let Some(error) = current.downcast_ref::<reqwest::Error>() {
            return if error.is_timeout() {
                "timeout"
            } else if error.is_connect() {
                "connection"
            } else if error.is_decode() {
                "decode"
            } else if error.is_body() {
                "body"
            } else if error.is_request() {
                "request"
            } else {
                "reqwest_other"
            };
        }
        source = current.source();
    }
    match error.kind() {
        std::io::ErrorKind::TimedOut => "timeout",
        std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionAborted => "connection",
        std::io::ErrorKind::UnexpectedEof => "unexpected_eof",
        std::io::ErrorKind::InvalidData => "invalid_data",
        std::io::ErrorKind::Interrupted => "interrupted",
        _ => "io_other",
    }
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
    /// A live buffer does not know its final length. Symphonia's ISOM4 reader
    /// requires `byte_len()` whenever the source claims to be seekable, so a
    /// growing UMP stream must report itself unseekable until the length is
    /// known (known `Content-Length`) or the producer has finished.
    fn is_seekable(&self) -> bool {
        self.buffer.content_length.load(Ordering::Acquire) > 0
            || self.buffer.finished.load(Ordering::Acquire)
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
static DIAGNOSTICS: OnceLock<Mutex<AudioDiagnosticsSnapshot>> = OnceLock::new();
static DIAGNOSTIC_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AudioDiagnosticsSnapshot {
    pub activation_sequence: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub player_activation_sequence: Option<u64>,
    pub stage: String,
    pub outcome: String,
    pub source: String,
    pub http_status: Option<u16>,
    pub response_version: Option<String>,
    pub content_type: Option<String>,
    pub response_bytes: u64,
    pub media_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_context: Option<AudioStreamRequestContext>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_headers: Option<AudioRequestHeaderSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_body: Option<AudioRequestBodySummary>,
    pub ump_part_kinds: Vec<UmpPartCount>,
    pub decoder_started: bool,
    pub output_started: bool,
    pub error_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub probe_error_kind: Option<String>,
    #[serde(default)]
    pub ump_segments: Vec<UmpSegmentDiagnostic>,
    #[serde(default)]
    pub ump_formats: Vec<UmpFormatDiagnostic>,
    pub updated_at_unix_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AudioStreamRequestContext {
    pub method: String,
    pub use_ump: bool,
    pub mime_type: String,
    pub itag: Option<u32>,
    pub range_start: Option<u64>,
    pub range_end: Option<u64>,
    pub content_length: Option<u64>,
    pub rn: Option<u64>,
    pub rbuf: Option<u64>,
    pub pot_present: bool,
    pub cpn_present: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AudioRequestHeaderSummary {
    pub header_names: Vec<String>,
    pub origin: String,
    pub referer: String,
    pub user_agent_family: String,
    pub user_agent_major: Option<u16>,
    pub content_type: String,
    pub content_length: Option<u64>,
    pub accept: String,
    pub accept_encoding: String,
    pub cookie_present: bool,
    pub authorization_present: bool,
    pub sec_ch_ua_present: bool,
    pub sec_ch_ua_mobile_present: bool,
    pub sec_ch_ua_platform_present: bool,
    pub sec_fetch_site: String,
    pub x_goog_visitor_id_present: bool,
    pub x_goog_authuser_present: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AudioRequestBodySummary {
    pub byte_length: Option<u64>,
    pub category: String,
}

fn safe_http_version(version: reqwest::Version) -> &'static str {
    match version {
        reqwest::Version::HTTP_09 => "http_09",
        reqwest::Version::HTTP_10 => "http_10",
        reqwest::Version::HTTP_11 => "http_11",
        reqwest::Version::HTTP_2 => "http_2",
        reqwest::Version::HTTP_3 => "http_3",
        _ => "unknown",
    }
}

fn summarize_audio_request_body(body: &[u8]) -> AudioRequestBodySummary {
    let category = match body {
        b"x" => "x",
        b"x=" => "x_equals",
        b"x\0" => "x_nul",
        b"x " => "x_space",
        b"x\r" => "x_cr",
        b"x\n" => "x_lf",
        [] => "empty",
        _ => "other",
    };
    AudioRequestBodySummary {
        byte_length: Some(body.len() as u64),
        category: category.into(),
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UmpPartCount {
    pub kind: u32,
    pub count: u64,
}

fn diagnostics_update(sequence: u64, update: impl FnOnce(&mut AudioDiagnosticsSnapshot)) {
    if let Some(state) = DIAGNOSTICS.get() {
        if let Ok(mut snapshot) = state.lock() {
            if snapshot.activation_sequence == sequence {
                update(&mut snapshot);
                snapshot.updated_at_unix_ms = unix_ms();
            }
        }
    }
}

fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u64::MAX as u128) as u64)
        .unwrap_or(0)
}

fn start_diagnostics(source: &str, player_activation_sequence: u64) -> u64 {
    let sequence = DIAGNOSTIC_SEQUENCE
        .fetch_add(1, Ordering::AcqRel)
        .saturating_add(1);
    let snapshot = AudioDiagnosticsSnapshot {
        activation_sequence: sequence,
        player_activation_sequence: Some(player_activation_sequence),
        stage: "activation".into(),
        outcome: "in_progress".into(),
        source: source.into(),
        http_status: None,
        response_version: None,
        content_type: None,
        response_bytes: 0,
        media_bytes: 0,
        request_context: None,
        request_headers: None,
        request_body: None,
        ump_part_kinds: Vec::new(),
        decoder_started: false,
        output_started: false,
        error_code: None,
        probe_error_kind: None,
        ump_segments: Vec::new(),
        ump_formats: Vec::new(),
        updated_at_unix_ms: unix_ms(),
    };
    *DIAGNOSTICS
        .get_or_init(|| Mutex::new(snapshot.clone()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = snapshot.clone();
    sequence
}

fn diagnostic_error_code(error: &str) -> &'static str {
    if error.starts_with("audio_http_ump") {
        "http_400_ump"
    } else if let Some(status) = error
        .strip_prefix("audio_http status=")
        .and_then(|value| value.split_whitespace().next())
        .and_then(|value| value.parse::<u16>().ok())
    {
        if status == 403 {
            "http_403"
        } else {
            "http_error"
        }
    } else if error.starts_with("audio_stream_url_body")
        || error.starts_with("audio_stream_content_type")
        || error.starts_with("audio_stream_empty_body")
    {
        "invalid_response"
    } else if error.contains("UMP") || error.contains("ump") {
        "ump_decode_error"
    } else if error.contains("no default audio output")
        || error.contains("output device")
        || error.contains("output format")
        || error.contains("output configuration")
    {
        "output_device_error"
    } else if error.contains("playback start") || error.contains("outputStart") {
        "output_start_error"
    } else if error.contains("probe")
        || error.contains("container")
        || error.contains("default track")
    {
        "container_probe_error"
    } else if error.contains("codec") || error.contains("decoder") {
        "codec_error"
    } else if error.contains("cancel") {
        "cancelled"
    } else if error.contains("stream")
        || error.contains("request")
        || error.contains("connection")
        || error.contains("timed out")
    {
        "network_error"
    } else {
        "invalid_response"
    }
}

fn finish_playback_diagnostics(sequence: u64, error: Option<&str>) {
    diagnostics_update(sequence, |snapshot| {
        if snapshot.outcome != "in_progress" {
            return;
        }
        if let Some(error) = error {
            snapshot.stage = "failed".into();
            snapshot.outcome = "error".into();
            snapshot.error_code = Some(diagnostic_error_code(error).into());
        } else {
            snapshot.stage = "ended".into();
            snapshot.outcome = "ended".into();
        }
    });
}

fn sanitized_probe_error_kind(error: &SymphoniaError) -> &'static str {
    match error {
        SymphoniaError::IoError(error) => match error.kind() {
            std::io::ErrorKind::UnexpectedEof => "unexpected_eof",
            std::io::ErrorKind::InvalidData => "invalid_data",
            std::io::ErrorKind::NotFound => "not_found",
            std::io::ErrorKind::PermissionDenied => "permission_denied",
            _ => "io_other",
        },
        SymphoniaError::DecodeError(_) => "malformed_data",
        SymphoniaError::SeekError(_) => "seek_error",
        SymphoniaError::Unsupported(_) => "unsupported",
        SymphoniaError::LimitError(_) => "limit_reached",
        SymphoniaError::ResetRequired => "reset_required",
    }
}

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
    pub fn diagnostics_snapshot() -> AudioDiagnosticsSnapshot {
        DIAGNOSTICS
            .get_or_init(|| {
                Mutex::new(AudioDiagnosticsSnapshot {
                    activation_sequence: 0,
                    player_activation_sequence: None,
                    stage: "idle".into(),
                    outcome: "none".into(),
                    source: "none".into(),
                    http_status: None,
                    response_version: None,
                    content_type: None,
                    response_bytes: 0,
                    media_bytes: 0,
                    request_context: None,
                    request_headers: None,
                    request_body: None,
                    ump_part_kinds: Vec::new(),
                    decoder_started: false,
                    output_started: false,
                    error_code: None,
                    probe_error_kind: None,
                    ump_segments: Vec::new(),
                    ump_formats: Vec::new(),
                    updated_at_unix_ms: unix_ms(),
                })
            })
            .lock()
            .map(|snapshot| snapshot.clone())
            .unwrap_or_else(|_| AudioDiagnosticsSnapshot {
                activation_sequence: 0,
                player_activation_sequence: None,
                stage: "idle".into(),
                outcome: "none".into(),
                source: "none".into(),
                http_status: None,
                response_version: None,
                content_type: None,
                response_bytes: 0,
                media_bytes: 0,
                request_context: None,
                request_headers: None,
                request_body: None,
                ump_part_kinds: Vec::new(),
                decoder_started: false,
                output_started: false,
                error_code: None,
                probe_error_kind: None,
                ump_segments: Vec::new(),
                ump_formats: Vec::new(),
                updated_at_unix_ms: unix_ms(),
            })
    }
    /// Plays complete successful UMP response bodies captured by the WebView.
    /// The receiver may remain open for the lifetime of playback.
    pub fn play_ump_receiver(
        receiver: mpsc::Receiver<Result<Vec<u8>, String>>,
        activation: PlayerActivation,
    ) -> Result<(), String> {
        let _activation_guard = PLAYER_ACTIVATION_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .map_err(|_| "Audio activation state poisoned".to_string())?;
        Self::set_activation(activation.clone())?;
        Self::stop();
        let diagnostic_sequence = start_diagnostics("youtube", activation.activation_sequence);
        ACTIVE.store(true, Ordering::Release);
        *CURRENT_SOURCE
            .get_or_init(|| Mutex::new(None))
            .lock()
            .map_err(|_| "Audio state poisoned".to_string())? = None;
        PAUSED.store(false, Ordering::Release);
        POSITION_MICROS.store(0, Ordering::Release);
        DURATION_MICROS.store(activation_duration(&activation), Ordering::Release);
        let stop = Arc::new(AtomicBool::new(false));
        *STOP
            .get_or_init(|| Mutex::new(None))
            .lock()
            .map_err(|_| "Audio state poisoned".to_string())? = Some(stop.clone());
        let (startup_tx, startup_rx) = mpsc::channel();
        let startup_error_tx = startup_tx.clone();
        thread::spawn(move || {
            let playback_result = run_ump_playback(
                receiver,
                stop.clone(),
                Some(startup_tx),
                diagnostic_sequence,
                0.0,
            );
            match playback_result {
                Err(error) => {
                    finish_playback_diagnostics(diagnostic_sequence, Some(&error));
                    let _ = startup_error_tx.send(Err(error.clone()));
                    if !stop.load(Ordering::Acquire) {
                        eprintln!("VibeDeck captured UMP audio error: {error}");
                        ACTIVE.store(false, Ordering::Release);
                        REVISION.fetch_add(1, Ordering::AcqRel);
                    }
                }
                Ok(()) if !stop.load(Ordering::Acquire) => {
                    finish_playback_diagnostics(diagnostic_sequence, None);
                    ACTIVE.store(false, Ordering::Release);
                    REVISION.fetch_add(1, Ordering::AcqRel);
                }
                Ok(()) => {}
            }
            stop.store(true, Ordering::Release);
        });
        match startup_rx.recv_timeout(std::time::Duration::from_secs(15)) {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => {
                Self::stop();
                Err(stream_bridge_error(&error))
            }
            Err(error) => {
                Self::stop();
                Err(stream_bridge_error(&format!(
                    "Captured UMP audio did not start: {error}"
                )))
            }
        }
    }

    /// Checks that a seek request still refers to the playing track, queue generation and revision.
    /// Shared by seeks that restart a captured-UMP stream instead of re-reading a file or URL.
    pub fn validate_seek(request: &SeekRequest) -> Result<(), String> {
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
        Ok(())
    }

    /// Replaces the running captured-UMP stream by a new one whose position starts at
    /// `start_seconds`, keeping the same activation and queue intent (no new activation is
    /// accepted or needed). The caller feeds the new receiver from the segment it chose; the first
    /// `skip_seconds` of decoded audio (segment start to the wanted point) are dropped.
    pub fn restart_ump_receiver(
        receiver: mpsc::Receiver<Result<Vec<u8>, String>>,
        start_seconds: f64,
        skip_seconds: f64,
    ) -> Result<(), String> {
        let _activation_guard = PLAYER_ACTIVATION_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .map_err(|_| "Audio activation state poisoned".to_string())?;
        let metadata = PLAYER_METADATA
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
        let diagnostic_sequence = start_diagnostics("youtube", metadata.activation_sequence);
        POSITION_MICROS.store((start_seconds.max(0.0) * 1_000_000.0) as u64, Ordering::Release);
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
        let (startup_tx, startup_rx) = mpsc::channel();
        let startup_error_tx = startup_tx.clone();
        thread::spawn(move || {
            let playback_result = run_ump_playback(
                receiver,
                stop.clone(),
                Some(startup_tx),
                diagnostic_sequence,
                skip_seconds,
            );
            match playback_result {
                Err(error) => {
                    finish_playback_diagnostics(diagnostic_sequence, Some(&error));
                    let _ = startup_error_tx.send(Err(error.clone()));
                    if !stop.load(Ordering::Acquire) {
                        eprintln!("VibeDeck captured UMP audio error: {error}");
                        ACTIVE.store(false, Ordering::Release);
                        REVISION.fetch_add(1, Ordering::AcqRel);
                    }
                }
                Ok(()) if !stop.load(Ordering::Acquire) => {
                    finish_playback_diagnostics(diagnostic_sequence, None);
                    ACTIVE.store(false, Ordering::Release);
                    REVISION.fetch_add(1, Ordering::AcqRel);
                }
                Ok(()) => {}
            }
            stop.store(true, Ordering::Release);
        });
        match startup_rx.recv_timeout(std::time::Duration::from_secs(15)) {
            Ok(Ok(())) => {
                PAUSED.store(paused, Ordering::Release);
                REVISION.fetch_add(1, Ordering::AcqRel);
                Ok(())
            }
            Ok(Err(error)) => {
                Self::stop();
                Err(stream_bridge_error(&error))
            }
            Err(error) => {
                Self::stop();
                Err(stream_bridge_error(&format!(
                    "Captured UMP audio did not start: {error}"
                )))
            }
        }
    }

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
        let activation_sequence = start_diagnostics("youtube", activation.activation_sequence);
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
        let (startup_tx, startup_rx) = mpsc::channel();
        let startup_error_tx = startup_tx.clone();
        thread::spawn(move || {
            let playback_result = run_playback(
                AudioSource::Url(url),
                stop.clone(),
                0.0,
                Some(startup_tx),
                activation_sequence,
            );
            match playback_result {
                Err(error) => {
                    finish_playback_diagnostics(activation_sequence, Some(&error));
                    let _ = startup_error_tx.send(Err(error.clone()));
                    if !stop.load(Ordering::Acquire) {
                        eprintln!("VibeDeck audio error: {error}");
                        ACTIVE.store(false, Ordering::Release);
                        REVISION.fetch_add(1, Ordering::AcqRel);
                    }
                }
                Ok(()) if !stop.load(Ordering::Acquire) => {
                    finish_playback_diagnostics(activation_sequence, None);
                    ACTIVE.store(false, Ordering::Release);
                    REVISION.fetch_add(1, Ordering::AcqRel);
                }
                Ok(()) => {}
            }
            stop.store(true, Ordering::Release);
        });
        match startup_rx.recv_timeout(std::time::Duration::from_secs(15)) {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => {
                Self::stop();
                Err(stream_bridge_error(&error))
            }
            Err(error) => {
                Self::stop();
                Err(stream_bridge_error(&format!(
                    "Audio stream did not start: {error}"
                )))
            }
        }
    }

    pub fn play_file(path: std::path::PathBuf, activation: PlayerActivation) -> Result<(), String> {
        let _activation_guard = PLAYER_ACTIVATION_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .map_err(|_| "Audio activation state poisoned".to_string())?;
        Self::set_activation(activation.clone())?;
        Self::stop();
        let duration = activation_duration(&activation);
        let activation_sequence = start_diagnostics("local", activation.activation_sequence);
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
            let playback_result = run_playback(
                AudioSource::File(path),
                stop.clone(),
                0.0,
                Some(startup_tx),
                activation_sequence,
            );
            match playback_result {
                Err(error) => {
                    finish_playback_diagnostics(activation_sequence, Some(&error));
                    let _ = startup_error_tx.send(Err(error.clone()));
                    if !stop.load(Ordering::Acquire) {
                        eprintln!("VibeDeck local audio error: {error}");
                        ACTIVE.store(false, Ordering::Release);
                        REVISION.fetch_add(1, Ordering::AcqRel);
                    }
                }
                Ok(()) if !stop.load(Ordering::Acquire) => {
                    finish_playback_diagnostics(activation_sequence, None);
                    ACTIVE.store(false, Ordering::Release);
                    REVISION.fetch_add(1, Ordering::AcqRel);
                }
                Ok(()) => {}
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
                    if let Some(diagnostics) = DIAGNOSTICS.get() {
                        if let Ok(mut snapshot) = diagnostics.lock() {
                            if snapshot.outcome == "in_progress" {
                                snapshot.stage = "cancelled".into();
                                snapshot.outcome = "cancelled".into();
                                snapshot.error_code = Some("cancelled".into());
                                snapshot.updated_at_unix_ms = unix_ms();
                            }
                        }
                    }
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
        let activation_sequence = start_diagnostics(
            if matches!(&source, AudioSource::File(_)) {
                "local"
            } else {
                "youtube"
            },
            metadata.activation_sequence,
        );
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
            if let Err(error) = run_playback(
                source,
                stop.clone(),
                seconds.max(0.0),
                None,
                activation_sequence,
            ) {
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
        activation_is_stale, audio_request_context_with_ump, audio_request_header_summary,
        classify_media_box, classify_ump_error_body, diagnostic_error_code, diagnostics_update,
        finish_playback_diagnostics, get_probe, outgoing_stream_url,
        parse_captured_ump_for_diagnostics, parse_stream_url, parse_ump_media_header,
        safe_http_version, sanitized_body_read_category, sanitized_content_type, sanitized_range,
        sanitized_ump_request_context, start_diagnostics, stream_bridge_error, stream_http_method,
        stream_uses_ump, summarize_audio_request_body, terminal_behavior_for,
        validate_stream_response, AudioEngine, DownloadBuffer, GrowingReader, NativeTrackSnapshot,
        PlayerActivation, PlayerSnapshot, RepeatModeRequest, SymphoniaError, UmpDecoder,
        UmpPartCount,
    };
    use std::sync::atomic::Ordering;
    use std::sync::Arc;
    use symphonia::core::formats::FormatOptions;
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;
    use symphonia::core::probe::Hint;

    fn encode_ump_varint(value: u32) -> Vec<u8> {
        if value < 0x80 {
            return vec![value as u8];
        }
        if value < 0x4000 {
            return vec![0x80 | (value as u8 & 0x3f), (value >> 6) as u8];
        }
        if value < 0x20_0000 {
            return vec![
                0xc0 | (value as u8 & 0x1f),
                (value >> 5) as u8,
                (value >> 13) as u8,
            ];
        }
        vec![
            0xe0 | (value as u8 & 0x0f),
            (value >> 4) as u8,
            (value >> 12) as u8,
            (value >> 20) as u8,
        ]
    }

    fn encode_protobuf_varint(mut value: u64) -> Vec<u8> {
        let mut bytes = Vec::new();
        while value >= 0x80 {
            bytes.push((value as u8 & 0x7f) | 0x80);
            value >>= 7;
        }
        bytes.push(value as u8);
        bytes
    }

    fn ump_part(kind: u32, data: &[u8]) -> Vec<u8> {
        let mut bytes = encode_ump_varint(kind);
        bytes.extend(encode_ump_varint(data.len() as u32));
        bytes.extend_from_slice(data);
        bytes
    }

    #[test]
    fn ump_decoder_strips_ids_and_filters_other_segments_across_arbitrary_reads() {
        let mut fixture = ump_part(20, &[0x08, 0x00]);
        fixture.extend(ump_part(21, &[1, b'N', b'O']));
        fixture.extend(ump_part(21, &[0, b'f', b't', b'y', b'p']));
        fixture.extend(ump_part(21, &[0, b'D', b'A', b'T', b'A']));
        fixture.extend(ump_part(22, &[0]));

        let mut decoder = UmpDecoder::new();
        let mut output = Vec::new();
        for byte in fixture {
            output.extend(decoder.feed(&[byte]).unwrap());
        }
        decoder.finish().unwrap();
        assert_eq!(output, b"ftypDATA");
    }

    #[test]
    fn ump_extracted_valid_wav_reaches_symphonia_probe_through_growing_reader() {
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&40u32.to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
        wav.extend_from_slice(&1u16.to_le_bytes()); // mono
        wav.extend_from_slice(&8_000u32.to_le_bytes());
        wav.extend_from_slice(&8_000u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes());
        wav.extend_from_slice(&8u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&4u32.to_le_bytes());
        wav.extend_from_slice(&[128, 128, 128, 128]);

        let mut framed = ump_part(20, &[0x08, 7]);
        let mut media_part = vec![7];
        media_part.extend_from_slice(&wav);
        framed.extend(ump_part(21, &media_part));

        let mut decoder = UmpDecoder::new();
        let extracted = decoder.feed(&framed).expect("valid UMP framing");
        decoder.finish().expect("complete media part");
        assert_eq!(extracted, wav);

        let buffer = Arc::new(DownloadBuffer::new());
        *buffer.bytes.lock().expect("buffer lock") = extracted;
        buffer
            .content_length
            .store(wav.len() as u64, Ordering::Release);
        buffer.finished.store(true, Ordering::Release);
        buffer.changed.notify_all();

        let source = GrowingReader {
            buffer,
            position: 0,
        };
        let media = MediaSourceStream::new(Box::new(source), Default::default());
        let probed = get_probe()
            .format(
                &Hint::new(),
                media,
                &FormatOptions::default(),
                &MetadataOptions::default(),
            )
            .expect("extracted valid WAV should probe");
        assert!(probed.format.default_track().is_some());
    }

    #[test]
    fn growing_reader_is_seekable_only_when_length_is_known_or_finished() {
        use symphonia::core::io::MediaSource;

        let buffer = Arc::new(DownloadBuffer::new());
        buffer
            .bytes
            .lock()
            .expect("buffer lock")
            .extend_from_slice(b"seed");
        buffer.changed.notify_all();

        let reader = GrowingReader {
            buffer: buffer.clone(),
            position: 0,
        };
        assert!(!reader.is_seekable());
        assert_eq!(reader.byte_len(), None);

        buffer.finished.store(true, Ordering::Release);
        assert!(reader.is_seekable());
        assert_eq!(reader.byte_len(), Some(4));

        let known = Arc::new(DownloadBuffer::new());
        known.content_length.store(10, Ordering::Release);
        let reader = GrowingReader {
            buffer: known,
            position: 0,
        };
        assert!(reader.is_seekable());
        assert_eq!(reader.byte_len(), Some(10));
    }

    #[test]
    fn media_box_classifier_reads_size_prefixed_bmff_type() {
        assert_eq!(
            super::classify_media_box(&[0, 0, 0, 0x18, b'f', b't', b'y', b'p']),
            "bmff_file_type"
        );
        assert_eq!(
            super::classify_media_box(&[0, 0, 0, 0x20, b'm', b'o', b'o', b'f']),
            "bmff_media_fragment"
        );
        assert_eq!(
            super::classify_media_box(&[0x1a, 0x45, 0xdf, 0xa3, 0, 0, 0, 0]),
            "webm_header"
        );
        assert_eq!(super::classify_media_box(&[0, 0, 0]), "short");
        assert_eq!(
            super::classify_media_box(&[0, 0, 0, 0, 1, 2, 3, 4]),
            "unknown"
        );
    }

    #[test]
    fn probe_error_diagnostics_are_fixed_categories_not_raw_messages() {
        assert_eq!(
            super::sanitized_probe_error_kind(&SymphoniaError::Unsupported("sensitive detail")),
            "unsupported"
        );
        assert_eq!(
            super::sanitized_probe_error_kind(&SymphoniaError::IoError(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "sensitive detail",
            ))),
            "unexpected_eof"
        );
        let serialized = serde_json::to_string(&Some(super::sanitized_probe_error_kind(
            &SymphoniaError::Unsupported("secret"),
        )))
        .unwrap();
        assert_eq!(serialized, "\"unsupported\"");
    }

    #[test]
    fn audio_diagnostic_error_classification_is_allowlisted() {
        assert_eq!(
            diagnostic_error_code("audio_http_ump status=400"),
            "http_400_ump"
        );
        assert_eq!(diagnostic_error_code("audio_http status=403"), "http_403");
        assert_eq!(
            diagnostic_error_code("network request failed"),
            "network_error"
        );
        assert_eq!(
            diagnostic_error_code("container probe failed"),
            "container_probe_error"
        );
        assert_eq!(
            diagnostic_error_code("codec initialization failed"),
            "codec_error"
        );
        assert_eq!(
            diagnostic_error_code("playback start failed"),
            "output_start_error"
        );
    }

    #[test]
    fn ump_request_body_diagnostics_serialize_only_category_and_length() {
        let raw_body = vec![b'x', 0];
        let summary = summarize_audio_request_body(&raw_body);
        assert_eq!(summary.category, "x_nul");
        assert_eq!(summary.byte_length, Some(2));

        let serialized = serde_json::to_string(&summary).unwrap();
        assert_eq!(serialized, r#"{"byteLength":2,"category":"x_nul"}"#);
        assert!(!serialized
            .as_bytes()
            .windows(raw_body.len())
            .any(|window| window == raw_body));
        assert_eq!(safe_http_version(reqwest::Version::HTTP_09), "http_09");
        assert_eq!(safe_http_version(reqwest::Version::HTTP_10), "http_10");
        assert_eq!(safe_http_version(reqwest::Version::HTTP_11), "http_11");
        assert_eq!(safe_http_version(reqwest::Version::HTTP_2), "http_2");
        assert_eq!(safe_http_version(reqwest::Version::HTTP_3), "http_3");
    }

    #[test]
    fn diagnostics_sanitize_content_type_and_reject_stale_sequence_updates() {
        assert_eq!(
            sanitized_content_type("audio/mp4; codecs=secret"),
            "audio/mp4"
        );
        assert_eq!(sanitized_content_type("text/plain\nsecret"), "other");
        let old = start_diagnostics("local", 1);
        let current = start_diagnostics("youtube", 2);
        diagnostics_update(old, |snapshot| snapshot.stage = "must_not_replace".into());
        diagnostics_update(current, |snapshot| {
            snapshot.ump_part_kinds = vec![UmpPartCount { kind: 58, count: 2 }];
        });
        finish_playback_diagnostics(current, Some("audio_http_ump status=400"));
        finish_playback_diagnostics(current, None);
        let snapshot = AudioEngine::diagnostics_snapshot();
        assert_eq!(snapshot.activation_sequence, current);
        assert_eq!(snapshot.stage, "failed");
        assert_eq!(snapshot.outcome, "error");
        assert_eq!(snapshot.error_code.as_deref(), Some("http_400_ump"));
        assert_eq!(snapshot.ump_part_kinds[0].kind, 58);
        assert_eq!(snapshot.ump_part_kinds[0].count, 2);
    }

    #[test]
    fn ump_decoder_handles_large_parts_and_nonzero_header_ids() {
        let mut fixture = ump_part(20, &[0x08, 7]);
        let payload: Vec<u8> = std::iter::once(7)
            .chain((0..300).map(|n| n as u8))
            .collect();
        fixture.extend(ump_part(21, &payload));
        let mut decoder = UmpDecoder::new();
        let extracted = decoder.feed(&fixture).unwrap();
        decoder.finish().unwrap();
        assert_eq!(extracted, payload[1..]);
    }

    #[test]
    fn captured_ump_diagnostic_consumer_extracts_media_without_returning_payload() {
        let mut fixture = ump_part(20, &[0x08, 7]);
        fixture.extend(ump_part(21, &[7, b'f', b't', b'y', b'p']));
        fixture.extend(ump_part(22, &[0]));

        let summary = parse_captured_ump_for_diagnostics(&fixture);
        assert_eq!(summary.outcome, "media_extracted");
        assert_eq!(summary.extracted_media_bytes, 4);
        assert_eq!(summary.part_kinds.len(), 3);
        let serialized = serde_json::to_string(&summary).unwrap();
        assert!(!serialized.contains("ftyp"));
        assert_eq!(
            parse_captured_ump_for_diagnostics(&ump_part(58, b"private error")).outcome,
            "no_complete_media"
        );
    }

    #[test]
    fn ump_decoder_preserves_media_header_across_complete_response_bodies() {
        let mut decoder = UmpDecoder::new();
        let first = [ump_part(20, &[0x08, 7]), ump_part(21, &[7, b'f', b't'])].concat();
        let second = ump_part(21, &[7, b'y', b'p']);
        assert_eq!(decoder.feed(&first).unwrap(), b"ft");
        decoder.finish_response().unwrap();
        assert_eq!(decoder.feed(&second).unwrap(), b"yp");
        decoder.finish_response().unwrap();
        decoder.finish().unwrap();
    }

    #[test]
    fn ump_decoder_rejects_response_boundary_with_partial_part_and_accepts_control_only_response() {
        let mut decoder = UmpDecoder::new();
        decoder.feed(&ump_part(21, &[0, b'f'])[..3]).unwrap();
        assert!(decoder
            .finish_response()
            .unwrap_err()
            .contains("partial part"));

        let mut decoder = UmpDecoder::new();
        decoder.feed(&ump_part(20, &[0x08, 1])).unwrap();
        decoder
            .finish_response()
            .expect("control-only response is accepted at the boundary");
        assert!(decoder.finish().unwrap_err().contains("no media"));
    }

    #[test]
    fn ump_decoder_skips_restarted_format_initialization_and_repeated_segments() {
        fn header(
            header_id: u8,
            itag: u64,
            start: u64,
            init: bool,
            segment: Option<u64>,
        ) -> Vec<u8> {
            let mut payload = vec![0x08];
            payload.extend(encode_protobuf_varint(header_id as u64));
            payload.push(0x18);
            payload.extend(encode_protobuf_varint(itag));
            payload.push(0x30);
            payload.extend(encode_protobuf_varint(start));
            payload.push(0x40);
            payload.extend(encode_protobuf_varint(u64::from(init)));
            if let Some(segment) = segment {
                payload.push(0x48);
                payload.extend(encode_protobuf_varint(segment));
            }
            payload
        }

        let mut decoder = UmpDecoder::new();
        let first = [
            ump_part(20, &header(0, 140, 0, true, None)),
            ump_part(21, &[0, b'F', b'T', b'Y', b'P']),
            ump_part(20, &header(1, 140, 4, false, Some(1))),
            ump_part(21, &[1, b'A', b'A', b'A']),
        ]
        .concat();
        assert_eq!(decoder.feed(&first).unwrap(), b"FTYPAAA");
        decoder.finish_response().unwrap();

        // The WebView restarted its SABR context: the format initialization and
        // segment 1 arrive again, followed by the segment that continues the
        // stream. Only the continuation may reach the demuxer.
        let replay = [
            ump_part(20, &header(0, 140, 0, true, None)),
            ump_part(21, &[0, b'F', b'T', b'Y', b'P']),
            ump_part(20, &header(1, 140, 4, false, Some(1))),
            ump_part(21, &[1, b'A', b'A', b'A']),
            ump_part(20, &header(2, 140, 7, false, Some(2))),
            ump_part(21, &[2, b'B', b'B']),
        ]
        .concat();
        assert_eq!(decoder.feed(&replay).unwrap(), b"BB");
        decoder.finish_response().unwrap();

        // A restarted context can also renumber segments; the byte range still
        // prevents replaying bytes that were already written.
        let renumbered = [
            ump_part(20, &header(3, 140, 4, false, Some(9))),
            ump_part(21, &[3, b'X', b'X', b'X']),
            ump_part(20, &header(4, 140, 9, false, Some(10))),
            ump_part(21, &[4, b'C']),
        ]
        .concat();
        assert_eq!(decoder.feed(&renumbered).unwrap(), b"C");
        decoder.finish_response().unwrap();
        let skipped: Vec<u64> = decoder
            .segments
            .iter()
            .map(|segment| segment.skipped_media_bytes)
            .collect();
        decoder.finish().unwrap();
        assert_eq!(skipped, vec![0, 0, 4, 3, 0, 3, 0]);
    }

    #[test]
    fn ump_decoder_forwards_only_the_selected_audio_format() {
        fn header(
            header_id: u8,
            itag: u64,
            start: u64,
            init: bool,
            segment: Option<u64>,
        ) -> Vec<u8> {
            let mut payload = vec![0x08];
            payload.extend(encode_protobuf_varint(header_id as u64));
            payload.push(0x18);
            payload.extend(encode_protobuf_varint(itag));
            payload.push(0x30);
            payload.extend(encode_protobuf_varint(start));
            payload.push(0x40);
            payload.extend(encode_protobuf_varint(u64::from(init)));
            if let Some(segment) = segment {
                payload.push(0x48);
                payload.extend(encode_protobuf_varint(segment));
            }
            payload
        }

        fn format(itag: u64, mime: &str) -> Vec<u8> {
            let mut format_id = vec![0x08];
            format_id.extend(encode_protobuf_varint(itag));
            let mut payload = vec![0x12];
            payload.extend(encode_protobuf_varint(format_id.len() as u64));
            payload.extend_from_slice(&format_id);
            payload.push(0x2a);
            payload.extend(encode_protobuf_varint(mime.len() as u64));
            payload.extend_from_slice(mime.as_bytes());
            payload.push(0x32);
            payload.extend(encode_protobuf_varint(2));
            payload.push(0x08);
            payload.push(0x00);
            payload
        }

        // One response carrying both an audio and a video format. Their
        // segments alternate, and the audio segment is split across two media
        // parts separated by a video part.
        let fixture = [
            ump_part(42, &format(140, "audio/mp4")),
            ump_part(42, &format(135, "video/mp4")),
            ump_part(20, &header(0, 140, 0, true, None)),
            ump_part(21, &[0, b'A', b'I']),
            ump_part(20, &header(1, 135, 0, true, None)),
            ump_part(21, &[1, b'V', b'I']),
            ump_part(20, &header(2, 140, 2, false, Some(1))),
            ump_part(21, &[2, b'A', b'1']),
            ump_part(20, &header(3, 135, 2, false, Some(1))),
            ump_part(21, &[3, b'V', b'1']),
            ump_part(21, &[2, b'A', b'2']),
            ump_part(20, &header(4, 140, 6, false, Some(2))),
            ump_part(21, &[4, b'A', b'3']),
            ump_part(22, &[4]),
        ]
        .concat();

        let mut decoder = UmpDecoder::new();
        assert_eq!(decoder.feed(&fixture).unwrap(), b"AIA1A2A3");
        assert_eq!(decoder.selected_itag, Some(140));
        decoder.finish_response().unwrap();

        assert_eq!(decoder.formats.len(), 2);
        let audio_segment = decoder
            .segments
            .iter()
            .find(|segment| segment.header_id == Some(2))
            .expect("audio segment traced");
        assert_eq!(audio_segment.media_bytes, 4);
        assert_eq!(audio_segment.emitted_media_bytes, 4);
        assert_eq!(audio_segment.skipped_media_bytes, 0);
        let video_segment = decoder
            .segments
            .iter()
            .find(|segment| segment.header_id == Some(3))
            .expect("video segment traced");
        assert_eq!(video_segment.media_bytes, 2);
        assert_eq!(video_segment.emitted_media_bytes, 0);
        decoder.finish().unwrap();
    }

    #[test]
    fn ump_decoder_waits_for_the_audio_format_before_forwarding_media() {
        // Observed SABR ordering: the video format initialization and its first
        // segments arrive before the audio format metadata. Forwarding them
        // would start the demuxer input with a video initialization segment.
        fn header(
            header_id: u8,
            itag: u64,
            start: u64,
            init: bool,
            segment: Option<u64>,
        ) -> Vec<u8> {
            let mut payload = vec![0x08];
            payload.extend(encode_protobuf_varint(header_id as u64));
            payload.push(0x18);
            payload.extend(encode_protobuf_varint(itag));
            payload.push(0x30);
            payload.extend(encode_protobuf_varint(start));
            payload.push(0x40);
            payload.extend(encode_protobuf_varint(u64::from(init)));
            if let Some(segment) = segment {
                payload.push(0x48);
                payload.extend(encode_protobuf_varint(segment));
            }
            payload
        }

        fn format(itag: u64, mime: &str) -> Vec<u8> {
            let mut format_id = vec![0x08];
            format_id.extend(encode_protobuf_varint(itag));
            let mut payload = vec![0x12];
            payload.extend(encode_protobuf_varint(format_id.len() as u64));
            payload.extend_from_slice(&format_id);
            payload.push(0x2a);
            payload.extend(encode_protobuf_varint(mime.len() as u64));
            payload.extend_from_slice(mime.as_bytes());
            payload
        }

        let mut decoder = UmpDecoder::new();
        let video_only = [
            ump_part(42, &format(135, "video/mp4")),
            ump_part(20, &header(0, 135, 0, true, None)),
            ump_part(21, &[0, b'V', b'I']),
            ump_part(22, &[0]),
            ump_part(20, &header(1, 135, 2, false, Some(1))),
            ump_part(21, &[1, b'V', b'1']),
        ]
        .concat();
        assert!(
            decoder.feed(&video_only).unwrap().is_empty(),
            "no media may be forwarded before the audio format is known"
        );
        assert_eq!(decoder.selected_itag, None);

        let audio = [
            ump_part(42, &format(140, "audio/mp4")),
            ump_part(20, &header(2, 140, 0, true, None)),
            ump_part(21, &[2, b'A', b'I']),
            ump_part(20, &header(3, 140, 2, false, Some(1))),
            ump_part(21, &[3, b'A', b'1']),
            ump_part(22, &[3]),
        ]
        .concat();
        assert_eq!(decoder.feed(&audio).unwrap(), b"AIA1");
        assert_eq!(decoder.selected_itag, Some(140));
        decoder.finish_response().unwrap();

        let emitted: Vec<u64> = decoder
            .segments
            .iter()
            .map(|segment| segment.emitted_media_bytes)
            .collect();
        assert_eq!(emitted, vec![0, 0, 2, 2]);
        decoder.finish().unwrap();
    }

    #[test]
    fn ump_decoder_accepts_control_only_response_between_media_responses() {
        let mut decoder = UmpDecoder::new();
        let first = [ump_part(20, &[0x08, 7]), ump_part(21, &[7, b'f', b't'])].concat();
        assert_eq!(decoder.feed(&first).unwrap(), b"ft");
        decoder.finish_response().expect("complete media response");

        let control = ump_part(58, &[0x01, 0x02, 0x03]);
        assert!(decoder.feed(&control).unwrap().is_empty());
        decoder.finish_response().expect("control-only response");

        let second = ump_part(21, &[7, b'y', b'p']);
        assert_eq!(decoder.feed(&second).unwrap(), b"yp");
        decoder.finish_response().unwrap();
        decoder.finish().unwrap();
    }

    #[test]
    fn media_header_parses_numeric_segment_range_fields() {
        let mut payload = vec![0x08];
        payload.extend(encode_protobuf_varint(76));
        payload.push(0x30);
        payload.extend(encode_protobuf_varint(131_072));
        payload.push(0x70);
        payload.extend(encode_protobuf_varint(65_536));
        let header = parse_ump_media_header(&payload).expect("media header parses");
        assert_eq!(header.header_id, Some(76));
        assert_eq!(header.segment_byte_range_start, Some(131_072));
        assert_eq!(header.segment_length_bytes, Some(65_536));
    }

    #[test]
    fn media_header_parses_format_and_segment_metadata_fields() {
        let mut payload = vec![0x08];
        payload.extend(encode_protobuf_varint(7));
        payload.push(0x18);
        payload.extend(encode_protobuf_varint(140));
        payload.push(0x40);
        payload.extend(encode_protobuf_varint(1));
        payload.push(0x48);
        payload.extend(encode_protobuf_varint(3));
        let header = parse_ump_media_header(&payload).expect("media header parses");
        assert_eq!(header.header_id, Some(7));
        assert_eq!(header.itag, Some(140));
        assert_eq!(header.is_initialization_segment, Some(true));
        assert_eq!(header.segment_num, Some(3));
    }

    #[test]
    fn ump_decoder_records_sanitized_segment_trace_and_media_end() {
        let mut header = vec![0x08];
        header.extend(encode_protobuf_varint(5));
        header.push(0x18);
        header.extend(encode_protobuf_varint(140));
        header.push(0x40);
        header.extend(encode_protobuf_varint(0));
        header.push(0x48);
        header.extend(encode_protobuf_varint(1));

        let mut fixture = ump_part(20, &header);
        fixture.extend(ump_part(21, &[5, b'f', b't', b'y', b'p', 0, 0, 0]));
        fixture.extend(ump_part(22, &[5]));

        let mut decoder = UmpDecoder::new();
        let media = decoder.feed(&fixture).expect("valid UMP framing");
        decoder.finish_response().expect("complete response");
        assert_eq!(media, b"ftyp\0\0\0");
        assert_eq!(decoder.segments.len(), 1);
        let segment = &decoder.segments[0];
        assert_eq!(segment.order, 0);
        assert_eq!(segment.header_id, Some(5));
        assert_eq!(segment.itag, Some(140));
        assert_eq!(segment.is_initialization_segment, Some(false));
        assert_eq!(segment.segment_num, Some(1));
        assert_eq!(segment.media_bytes, 7);
        assert_eq!(segment.first_media_box.as_deref(), Some("bmff_file_type"));
        assert!(segment.saw_media_end);
    }

    #[test]
    fn ump_segment_trace_attributes_media_by_header_id() {
        fn header_payload(header_id: u8, itag: u64) -> Vec<u8> {
            let mut payload = vec![0x08];
            payload.extend(encode_protobuf_varint(header_id as u64));
            payload.push(0x18);
            payload.extend(encode_protobuf_varint(itag));
            payload
        }

        let mut fixture = ump_part(20, &header_payload(1, 140));
        fixture.extend(ump_part(21, &[1, b'f', b't', b'y', b'p']));
        fixture.extend(ump_part(22, &[1]));
        fixture.extend(ump_part(20, &header_payload(2, 251)));
        fixture.extend(ump_part(21, &[2, b'm', b'o', b'o', b'f']));
        fixture.extend(ump_part(21, &[1, b'X', b'Y']));
        fixture.extend(ump_part(22, &[2]));

        let mut decoder = UmpDecoder::new();
        let media = decoder.feed(&fixture).expect("valid UMP framing");
        decoder.finish_response().expect("complete response");
        assert_eq!(media, b"ftypmoof");
        assert_eq!(decoder.segments.len(), 2);
        let first = &decoder.segments[0];
        assert_eq!(first.header_id, Some(1));
        assert_eq!(first.itag, Some(140));
        assert_eq!(first.media_bytes, 6);
        assert_eq!(first.first_media_box.as_deref(), Some("bmff_file_type"));
        assert!(first.saw_media_end);
        let second = &decoder.segments[1];
        assert_eq!(second.header_id, Some(2));
        assert_eq!(second.itag, Some(251));
        assert_eq!(second.media_bytes, 4);
        assert_eq!(
            second.first_media_box.as_deref(),
            Some("bmff_media_fragment")
        );
        assert!(second.saw_media_end);
    }

    #[test]
    fn ump_segment_trace_waits_for_size_prefixed_bmff_type() {
        let mut header = vec![0x08];
        header.extend(encode_protobuf_varint(9));

        let mut fixture = ump_part(20, &header);
        // First MEDIA part carries the header id and the 32-bit box size only.
        fixture.extend(ump_part(21, &[9, 0, 0, 0, 0x18]));
        // The box type arrives in the next MEDIA part.
        fixture.extend(ump_part(21, &[9, b'f', b't', b'y', b'p', 0, 0]));
        fixture.extend(ump_part(22, &[9]));

        let mut decoder = UmpDecoder::new();
        decoder.feed(&fixture).expect("valid UMP framing");
        decoder.finish_response().expect("complete response");
        let segment = &decoder.segments[0];
        assert_eq!(segment.media_bytes, 10);
        assert_eq!(segment.first_media_box.as_deref(), Some("bmff_file_type"));
    }

    #[test]
    fn ump_decoder_records_format_initialization_metadata_category() {
        let mut format_id = vec![0x08];
        format_id.extend(encode_protobuf_varint(140));
        let mut format = vec![0x12];
        format.extend(encode_protobuf_varint(format_id.len() as u64));
        format.extend(format_id);
        format.push(0x2a);
        format.extend(encode_protobuf_varint(b"audio/mp4".len() as u64));
        format.extend_from_slice(b"audio/mp4");
        format.push(0x32);
        format.extend(encode_protobuf_varint(0));

        let mut fixture = ump_part(42, &format);
        fixture.extend(ump_part(20, &[0x08, 1]));
        fixture.extend(ump_part(21, &[1, b'f', b't', b'y', b'p']));
        fixture.extend(ump_part(22, &[1]));

        let mut decoder = UmpDecoder::new();
        decoder.feed(&fixture).expect("valid UMP framing");
        decoder.finish_response().expect("complete response");
        assert_eq!(decoder.formats.len(), 1);
        let parsed = &decoder.formats[0];
        assert_eq!(parsed.itag, Some(140));
        assert_eq!(parsed.mime_type, "audio_mp4");
        assert!(parsed.has_init_range);
    }

    #[test]
    fn media_box_classifier_maps_known_prefixes_without_retaining_bytes() {
        assert_eq!(classify_media_box(b"ftyp..."), "bmff_file_type");
        assert_eq!(classify_media_box(b"moof..."), "bmff_media_fragment");
        assert_eq!(classify_media_box(&[0x1a, 0x45, 0xdf, 0xa3]), "webm_header");
        assert_eq!(
            classify_media_box(&[0x1f, 0x43, 0xb6, 0x75]),
            "webm_cluster"
        );
        assert_eq!(classify_media_box(b"????"), "unknown");
        assert_eq!(classify_media_box(b"fty"), "short");
    }

    #[test]
    fn media_header_skips_all_supported_unknown_protobuf_wire_types() {
        let mut payload = vec![0x08, 76];
        payload.extend([0x18, 0xac, 0x02]); // wire type 0
        payload.extend([0x21, 1, 2, 3, 4, 5, 6, 7, 8]); // wire type 1
        payload.extend([0x2a, 3, b'a', b'b', b'c']); // wire type 2
        payload.extend([0x35, 1, 2, 3, 4]); // wire type 5

        let header = parse_ump_media_header(&payload).expect("unknown fields are skipped");
        assert_eq!(header.header_id, Some(76));
    }

    #[test]
    fn media_header_rejects_truncated_or_overflowing_protobuf_varints() {
        for payload in [
            vec![0x80],       // truncated field key
            vec![0x08, 0x80], // truncated id
            vec![
                0x08, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x02,
            ],
            vec![0x80, 0x80, 0x80, 0x80, 0x10], // field key exceeds u32
        ] {
            assert!(parse_ump_media_header(&payload).is_err());
        }
    }

    #[test]
    fn media_header_rejects_invalid_keys_and_truncated_unknown_fields() {
        for payload in [
            vec![0x00],                    // field number zero
            vec![0x0e],                    // unsupported wire type 6
            vec![0x08, 76, 0x21, 1, 2],    // truncated fixed64
            vec![0x08, 76, 0x2a, 4, 1, 2], // truncated length-delimited field
            vec![0x08, 76, 0x35, 1, 2, 3], // truncated fixed32
        ] {
            assert!(parse_ump_media_header(&payload).is_err());
        }
    }

    #[test]
    fn stream_diagnostics_keep_only_valid_numeric_ranges_and_media_type() {
        let url = parse_stream_url(
            "https://media.example.invalid/videoplayback?range=131072-196607&pot=secret",
        )
        .unwrap();
        assert_eq!(sanitized_range(&url), Some((131_072, 196_607)));
        let invalid =
            parse_stream_url("https://media.example.invalid/videoplayback?range=secret-secret")
                .unwrap();
        assert_eq!(sanitized_range(&invalid), None);
        assert_eq!(
            sanitized_content_type("application/vnd.yt-ump; version=secret"),
            "application/vnd.yt-ump"
        );
        assert_eq!(sanitized_content_type("text/plain\nsecret"), "other");
    }

    #[test]
    fn ump_request_context_logs_only_whitelisted_numeric_values_and_pot_presence() {
        let url = parse_stream_url("https://media.example.invalid/videoplayback?rn=152&rbuf=0&clen=6657472&pot=pot-secret&sig=sig-secret&n=n-secret&custom=custom-secret").unwrap();
        let diagnostics = sanitized_ump_request_context(&url);
        assert_eq!(diagnostics, "rn=152 rbuf=0 clen=6657472 pot_present=true");
        for secret in ["pot-secret", "sig-secret", "n-secret", "custom-secret"] {
            assert!(!diagnostics.contains(secret));
        }

        let invalid = parse_stream_url("https://media.example.invalid/videoplayback?rn=rn-secret&rbuf=-1&clen=clen-secret&pot=").unwrap();
        let diagnostics = sanitized_ump_request_context(&invalid);
        assert_eq!(diagnostics, "rn=none rbuf=none clen=none pot_present=true");
        for secret in ["rn-secret", "-1", "clen-secret"] {
            assert!(!diagnostics.contains(secret));
        }

        let absent =
            parse_stream_url("https://media.example.invalid/videoplayback?other=secret").unwrap();
        assert_eq!(
            sanitized_ump_request_context(&absent),
            "rn=none rbuf=none clen=none pot_present=false"
        );
    }

    #[test]
    fn request_context_retains_only_safe_transport_comparison_fields() {
        let url = parse_stream_url(
            "https://media.example.invalid/videoplayback?ump=1&itag=140&mime=audio%2Fmp4&range=10-20&clen=500&rn=2&rbuf=0&pot=secret&cpn=private&sig=secret",
        )
        .unwrap();
        let outgoing = outgoing_stream_url(&url, true);
        let context = audio_request_context_with_ump(&outgoing, "GET", true);
        assert_eq!(context.method, "GET");
        assert!(context.use_ump);
        assert_eq!(context.mime_type, "audio/mp4");
        assert_eq!(context.itag, Some(140));
        assert_eq!((context.range_start, context.range_end), (None, None));
        assert_eq!(context.content_length, Some(500));
        assert_eq!((context.rn, context.rbuf), (Some(2), Some(0)));
        assert!(context.pot_present);
        assert!(context.cpn_present);
        let serialized = serde_json::to_string(&context).unwrap();
        assert!(!serialized.contains("secret"));
        assert!(!serialized.contains("private"));
    }

    #[test]
    fn request_header_summary_keeps_categories_and_sensitive_presence_only() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert("origin", "https://music.youtube.com".parse().unwrap());
        headers.insert(
            "referer",
            "https://music.youtube.com/watch".parse().unwrap(),
        );
        headers.insert(
            "user-agent",
            "Mozilla/5.0 Chrome/154.0.0.0 Safari/537.36"
                .parse()
                .unwrap(),
        );
        headers.insert(
            "content-type",
            "application/x-www-form-urlencoded;charset=UTF-8"
                .parse()
                .unwrap(),
        );
        headers.insert("content-length", "1".parse().unwrap());
        headers.insert("accept", "*/*".parse().unwrap());
        headers.insert("accept-encoding", "gzip, br".parse().unwrap());
        headers.insert("cookie", "private-cookie-value".parse().unwrap());
        headers.insert("authorization", "Bearer private-token".parse().unwrap());
        headers.insert("sec-ch-ua", "private-client-hint".parse().unwrap());
        headers.insert("sec-fetch-site", "cross-site".parse().unwrap());
        headers.insert("x-goog-visitor-id", "private-visitor-id".parse().unwrap());

        let summary = audio_request_header_summary(&headers);
        assert_eq!(summary.origin, "music_youtube");
        assert_eq!(summary.referer, "music_youtube");
        assert_eq!(summary.user_agent_family, "chrome");
        assert_eq!(summary.user_agent_major, Some(154));
        assert_eq!(summary.content_type, "form_urlencoded");
        assert_eq!(summary.content_length, Some(1));
        assert_eq!(summary.accept, "wildcard");
        assert_eq!(summary.accept_encoding, "gzip_br");
        assert!(summary.cookie_present);
        assert!(summary.authorization_present);
        assert!(summary.sec_ch_ua_present);
        assert_eq!(summary.sec_fetch_site, "cross_site");
        assert!(summary.x_goog_visitor_id_present);

        let serialized = serde_json::to_string(&summary).unwrap();
        for forbidden in [
            "private-cookie-value",
            "private-token",
            "private-client-hint",
            "private-visitor-id",
            "music.youtube.com",
            "Mozilla/5.0",
        ] {
            assert!(!serialized.contains(forbidden));
        }
    }

    #[test]
    fn ump_decoder_rejects_truncated_parts_and_invalid_varints() {
        let mut decoder = UmpDecoder::new();
        let partial = ump_part(21, &[0, b'f', b't', b'y', b'p']);
        decoder.feed(&partial[..partial.len() - 1]).unwrap();
        assert!(decoder.finish().unwrap_err().contains("partial part"));

        let mut decoder = UmpDecoder::new();
        assert!(decoder.feed(&[0xff]).unwrap_err().contains("varint"));
    }

    #[test]
    fn ump_http_error_classification_exposes_only_part_ids_and_completeness() {
        let fixture = ump_part(58, b"private error payload");
        let classification = classify_ump_error_body(&fixture);
        assert_eq!(classification, "state=complete;kinds=58:1");
        assert!(!classification.contains("private"));
        assert_eq!(
            classify_ump_error_body(&[58, 9, b's']),
            "state=partial_part;kinds=none"
        );

        let bridge_error: serde_json::Value = serde_json::from_str(&stream_bridge_error(
            "audio_http_ump status=400 body_bytes=23 parts=state=complete;kinds=58:1",
        ))
        .unwrap();
        assert_eq!(bridge_error["code"], "audio_stream_ump_rejected");
        assert!(bridge_error["message"]
            .as_str()
            .unwrap()
            .contains("HTTP 400"));
        assert!(!bridge_error.to_string().contains("private"));
    }

    #[test]
    fn failed_ump_body_read_preserves_only_safe_partial_framing_and_error_kind() {
        let partial = [58, 4, b's', b'e'];
        assert_eq!(
            classify_ump_error_body(&partial),
            "state=partial_part;kinds=none"
        );
        let error = std::io::Error::new(std::io::ErrorKind::TimedOut, "private detail");
        assert_eq!(sanitized_body_read_category(&error), "timeout");
        assert!(!format!("{}", sanitized_body_read_category(&error)).contains("private"));
        let eof = std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "private detail");
        assert_eq!(sanitized_body_read_category(&eof), "unexpected_eof");
    }

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
    fn ump_stream_contract_strips_only_ump_and_range_and_builds_get_without_body() {
        let url = parse_stream_url(
            "https://media.example.invalid/videoplayback?ump=1&range=131072-196607&itag=140&mime=audio%2Fmp4&rn=2&sig=example-signature&custom=example-value",
        )
        .unwrap();
        assert!(stream_uses_ump(&url));
        let outgoing = outgoing_stream_url(&url, true);
        assert_eq!(stream_http_method(&outgoing), reqwest::Method::GET);
        let query = outgoing.query_pairs().collect::<Vec<_>>();
        assert!(!query.iter().any(|(key, _)| key == "ump" || key == "range"));
        assert_eq!(query.len(), 5);
        for (key, value) in [
            ("itag", "140"),
            ("mime", "audio/mp4"),
            ("rn", "2"),
            ("sig", "example-signature"),
            ("custom", "example-value"),
        ] {
            assert!(query
                .iter()
                .any(|(actual_key, actual_value)| { actual_key == key && actual_value == value }));
        }
        let request = reqwest::blocking::Client::new()
            .get(outgoing)
            .header("Origin", "https://music.youtube.com")
            .header("Referer", "https://music.youtube.com/")
            .header("User-Agent", "ExampleAgent/1.0")
            .build()
            .unwrap();
        assert_eq!(request.method(), reqwest::Method::GET);
        assert!(request.body().is_none());
        assert!(request.headers().contains_key("origin"));
        assert!(request.headers().contains_key("referer"));
        assert!(request.headers().contains_key("user-agent"));
    }

    #[test]
    fn malformed_stream_url_is_rejected_before_http() {
        assert!(parse_stream_url("not a URL").is_err());
        assert!(parse_stream_url("file:///tmp/audio").is_err());
    }

    #[test]
    fn url_text_response_is_classified_as_resolution_failure_not_media() {
        let body = b"https://media.example.invalid/videoplayback?opaque=redacted";
        let error =
            validate_stream_response(reqwest::StatusCode::OK, "text/plain", body).unwrap_err();
        assert!(error.starts_with("audio_stream_url_body"));
        let bridge_error: serde_json::Value =
            serde_json::from_str(&stream_bridge_error(&error)).unwrap();
        assert_eq!(bridge_error["code"], "audio_stream_url_body");
        assert_eq!(bridge_error["operation"], "play_audio_url");
        assert_eq!(bridge_error["retryable"], true);
        assert!(!bridge_error.to_string().contains("example.invalid"));
    }

    #[test]
    fn non_success_http_response_has_actionable_http_classification() {
        let error =
            validate_stream_response(reqwest::StatusCode::FORBIDDEN, "text/plain", b"denied")
                .unwrap_err();
        assert!(error.starts_with("audio_http status=403"));
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&stream_bridge_error(&error)).unwrap()
                ["code"],
            "audio_stream_http_error"
        );
    }

    #[test]
    fn media_content_type_and_payload_are_accepted_but_malformed_payload_is_rejected() {
        assert!(validate_stream_response(
            reqwest::StatusCode::OK,
            "audio/mp4; codecs=\"mp4a.40.2\"",
            b"ftyp"
        )
        .is_ok());
        assert!(validate_stream_response(
            reqwest::StatusCode::OK,
            "application/octet-stream",
            b"media"
        )
        .is_ok());
        assert!(validate_stream_response(
            reqwest::StatusCode::OK,
            "application/vnd.yt-ump",
            &[20, 2, 8, 0]
        )
        .is_ok());
        assert!(validate_stream_response(
            reqwest::StatusCode::OK,
            "text/plain",
            b"unexpected response"
        )
        .unwrap_err()
        .starts_with("audio_stream_content_type"));
        assert_eq!(
            validate_stream_response(reqwest::StatusCode::OK, "audio/mp4", b"").unwrap_err(),
            "audio_stream_empty_body"
        );
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

fn run_ump_playback(
    receiver: mpsc::Receiver<Result<Vec<u8>, String>>,
    stop: Arc<AtomicBool>,
    startup: Option<mpsc::Sender<Result<(), String>>>,
    diagnostic_sequence: u64,
    skip_seconds: f64,
) -> Result<(), String> {
    let download = Arc::new(DownloadBuffer::new());
    let producer_buffer = download.clone();
    let producer_stop = stop.clone();
    thread::spawn(move || {
        let result = (|| -> Result<(), String> {
            let mut decoder = UmpDecoder::new();
            loop {
                if producer_stop.load(Ordering::Acquire) {
                    return Ok(());
                }
                let body = match receiver.recv_timeout(std::time::Duration::from_millis(100)) {
                    Ok(Ok(body)) => body,
                    Ok(Err(error)) => return Err(format!("captured UMP response failed: {error}")),
                    Err(mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                };
                diagnostics_update(diagnostic_sequence, |snapshot| {
                    snapshot.response_bytes =
                        snapshot.response_bytes.saturating_add(body.len() as u64);
                    snapshot.stage = "ump_parsing".into();
                    snapshot.content_type = Some("application/vnd.yt-ump".into());
                });
                if body.is_empty() {
                    return Err("captured UMP response was empty".to_string());
                }
                for chunk in body.chunks(64 * 1024) {
                    if producer_stop.load(Ordering::Acquire) {
                        return Ok(());
                    }
                    let audio = decoder.feed(chunk)?;
                    diagnostics_update(diagnostic_sequence, |snapshot| {
                        snapshot.media_bytes =
                            snapshot.media_bytes.saturating_add(audio.len() as u64);
                        snapshot.ump_part_kinds = decoder
                            .part_counts
                            .iter()
                            .take(32)
                            .map(|(kind, count)| UmpPartCount {
                                kind: *kind,
                                count: *count,
                            })
                            .collect();
                        snapshot.ump_segments = decoder.segments.clone();
                        snapshot.ump_formats = decoder.formats.clone();
                    });
                    if !audio.is_empty() {
                        let mut bytes = producer_buffer
                            .bytes
                            .lock()
                            .map_err(|_| "buffer poisoned".to_string())?;
                        bytes.extend_from_slice(&audio);
                        producer_buffer.changed.notify_all();
                    }
                }
                decoder.finish_response()?;
            }
            decoder.finish()
        })();
        if let Err(error) = result {
            if let Ok(mut failure) = producer_buffer.failed.lock() {
                *failure = Some(error);
            }
        }
        producer_buffer.finished.store(true, Ordering::Release);
        producer_buffer.changed.notify_all();
    });

    let source = GrowingReader {
        buffer: download.clone(),
        position: 0,
    };
    let media = MediaSourceStream::new(Box::new(source), Default::default());
    let decoded = decode_media(media, stop, 0.0, skip_seconds, startup, diagnostic_sequence);
    if let Some(error) = download.failed.lock().ok().and_then(|value| value.clone()) {
        return Err(error);
    }
    decoded
}

fn run_playback(
    source: AudioSource,
    stop: Arc<AtomicBool>,
    start_seconds: f64,
    startup: Option<mpsc::Sender<Result<(), String>>>,
    activation_sequence: u64,
) -> Result<(), String> {
    if let AudioSource::File(path) = &source {
        diagnostics_update(activation_sequence, |snapshot| {
            snapshot.stage = "container_probe".into()
        });
        let file = File::open(path).map_err(|_| "local file open failed".to_string())?;
        let media = MediaSourceStream::new(Box::new(file), Default::default());
        return decode_media(media, stop, start_seconds, 0.0, startup, activation_sequence);
    }
    let AudioSource::Url(url) = source else {
        unreachable!()
    };
    let download = Arc::new(DownloadBuffer::new());
    let download_for_thread = download.clone();
    let stop_for_download = stop.clone();
    thread::spawn(move || {
        let result = (|| -> Result<(), String> {
            diagnostics_update(activation_sequence, |snapshot| {
                snapshot.stage = "request_start".into()
            });
            let parsed_url = parse_stream_url(&url)?;
            let use_ump = stream_uses_ump(&parsed_url);
            let request_url = outgoing_stream_url(&parsed_url, use_ump);
            let range = sanitized_range(&request_url);
            let method = stream_http_method(&request_url);
            let request_context = sanitized_ump_request_context(&request_url);
            diagnostics_update(activation_sequence, |snapshot| {
                snapshot.request_context = Some(audio_request_context_with_ump(
                    &request_url,
                    method.as_str(),
                    use_ump,
                ));
            });
            eprintln!(
                "VibeDeck stream request: activation={} method={} range={} {}",
                activation_sequence,
                method,
                range.map_or_else(
                    || "none".to_string(),
                    |(start, end)| format!("{start}-{end}")
                ),
                request_context
            );
            let client = reqwest::blocking::Client::new();
            let mut request = client
                .request(method.clone(), request_url)
                .header("Origin", "https://music.youtube.com")
                .header("Referer", "https://music.youtube.com/")
                .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/153 Safari/537.36");
            if !use_ump {
                request = request
                    .header("Accept", "*/*")
                    .header("Accept-Encoding", "gzip, br")
                    .header("Sec-Fetch-Site", "cross-site");
            }
            let request = request
                .build()
                .map_err(|_| "network request build failed".to_string())?;
            let safe_headers = audio_request_header_summary(request.headers());
            let request_body_bytes = request
                .body()
                .and_then(|body| body.as_bytes())
                .map(summarize_audio_request_body);
            diagnostics_update(activation_sequence, |snapshot| {
                snapshot.request_headers = Some(safe_headers);
                snapshot.request_body = request_body_bytes;
            });
            let mut response = client
                .execute(request)
                .map_err(|_| "network request failed".to_string())?;
            let response_version = safe_http_version(response.version()).to_string();
            let status = response.status();
            let content_type = response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .unwrap_or("unknown")
                .to_string();
            diagnostics_update(activation_sequence, |snapshot| {
                snapshot.stage = "response_received".into();
                snapshot.http_status = Some(status.as_u16());
                snapshot.response_version = Some(response_version);
                snapshot.content_type = Some(sanitized_content_type(&content_type));
            });
            eprintln!("VibeDeck stream response: activation={} method={} range={} status={} content_type={}", activation_sequence, method, range.map_or_else(|| "none".to_string(), |(start, end)| format!("{start}-{end}")), status.as_u16(), sanitized_content_type(&content_type));
            if !status.is_success() {
                const MAX_ERROR_BODY_BYTES: usize = 4 * 1024;
                let mut body = Vec::with_capacity(MAX_ERROR_BODY_BYTES);
                let body_read_error = response
                    .take(MAX_ERROR_BODY_BYTES as u64)
                    .read_to_end(&mut body)
                    .err();
                if !use_ump {
                    diagnostics_update(activation_sequence, |snapshot| {
                        snapshot.response_bytes = body.len() as u64;
                    });
                }
                if let Some(error) = body_read_error {
                    if use_ump {
                        return Err(format!("audio_http_ump status={}", status.as_u16()));
                    }
                    let category = sanitized_body_read_category(&error);
                    let classification = classify_ump_error_body(&body);
                    eprintln!(
                        "VibeDeck stream error body read: activation={} status={} body_bytes={} category={} kind={:?} ump_parts={}",
                        activation_sequence,
                        status.as_u16(),
                        body.len(),
                        category,
                        error.kind(),
                        classification
                    );
                    if content_type.split(';').next().is_some_and(|value| {
                        value.trim().eq_ignore_ascii_case("application/vnd.yt-ump")
                    }) {
                        return Err(format!(
                            "audio_http_ump status={} body_read={} body_bytes={} parts={}",
                            status.as_u16(),
                            category,
                            body.len(),
                            classification
                        ));
                    }
                    return Err(format!(
                        "audio_http status={} content-type={} body_read={} body_bytes={}",
                        status,
                        sanitized_content_type(&content_type),
                        category,
                        body.len()
                    ));
                }
                if content_type.split(';').next().is_some_and(|value| {
                    value.trim().eq_ignore_ascii_case("application/vnd.yt-ump")
                }) {
                    if use_ump {
                        return Err(format!("audio_http_ump status={}", status.as_u16()));
                    }
                    let classification = classify_ump_error_body(&body);
                    eprintln!(
                        "VibeDeck UMP error response: activation={} status={} body_bytes={} parts={}",
                        activation_sequence,
                        status.as_u16(),
                        body.len(),
                        classification
                    );
                    return Err(format!(
                        "audio_http_ump status={} body_bytes={} parts={}",
                        status.as_u16(),
                        body.len(),
                        classification
                    ));
                }
                return Err(format!(
                    "audio_http status={status} content-type={}",
                    sanitized_content_type(&content_type)
                ));
            }
            let is_ump_response = content_type
                .split(';')
                .next()
                .is_some_and(|value| value.trim().eq_ignore_ascii_case("application/vnd.yt-ump"));
            if !is_ump_response {
                if let Some(content_length) = response.content_length() {
                    download_for_thread
                        .content_length
                        .store(content_length, Ordering::Release);
                }
            }
            let mut ump_decoder = is_ump_response.then(UmpDecoder::new);
            let mut chunk = vec![0_u8; 64 * 1024];
            let mut first_read = true;
            let mut media_output_bytes = 0_u64;
            loop {
                if stop_for_download.load(Ordering::Acquire) {
                    eprintln!(
                        "VibeDeck stream cancelled: activation={activation_sequence} range={}",
                        range.map_or_else(
                            || "none".to_string(),
                            |(start, end)| format!("{start}-{end}")
                        )
                    );
                    return Ok(());
                }
                let size = response
                    .read(&mut chunk)
                    .map_err(|_| "network response read failed".to_string())?;
                if size == 0 {
                    break;
                }
                if first_read {
                    first_read = false;
                    validate_stream_response(status, &content_type, &chunk[..size])?;
                }
                if !use_ump {
                    diagnostics_update(activation_sequence, |snapshot| {
                        snapshot.response_bytes =
                            snapshot.response_bytes.saturating_add(size as u64)
                    });
                }
                let decoded = if let Some(ump) = ump_decoder.as_mut() {
                    ump.feed(&chunk[..size])?
                } else {
                    chunk[..size].to_vec()
                };
                diagnostics_update(activation_sequence, |snapshot| {
                    snapshot.stage = if is_ump_response {
                        "ump_parsing".into()
                    } else {
                        "media_download".into()
                    };
                    if !use_ump {
                        snapshot.media_bytes =
                            snapshot.media_bytes.saturating_add(decoded.len() as u64);
                    }
                    if !use_ump {
                        if let Some(ump) = ump_decoder.as_ref() {
                            snapshot.ump_part_kinds = ump
                                .part_counts
                                .iter()
                                .take(32)
                                .map(|(kind, count)| UmpPartCount {
                                    kind: *kind,
                                    count: *count,
                                })
                                .collect();
                        }
                    }
                });
                if decoded.is_empty() {
                    continue;
                }
                media_output_bytes = media_output_bytes.saturating_add(decoded.len() as u64);
                let mut bytes = download_for_thread
                    .bytes
                    .lock()
                    .map_err(|_| "buffer poisoned".to_string())?;
                bytes.extend_from_slice(&decoded);
                download_for_thread.changed.notify_all();
            }
            if let Some(ump) = ump_decoder {
                eprintln!(
                    "VibeDeck UMP response EOF: range={} media_bytes={}",
                    range.map_or_else(
                        || "none".to_string(),
                        |(start, end)| format!("{start}-{end}")
                    ),
                    media_output_bytes
                );
                ump.finish()?;
            } else {
                eprintln!(
                    "VibeDeck stream response EOF: activation={} range={} media_bytes={} next_range=none",
                    activation_sequence,
                    range.map_or_else(|| "none".to_string(), |(start, end)| format!("{start}-{end}")),
                    media_output_bytes
                );
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

    let source = GrowingReader {
        buffer: download.clone(),
        position: 0,
    };
    let media = MediaSourceStream::new(Box::new(source), Default::default());
    let decoded = decode_media(media, stop, start_seconds, 0.0, startup, activation_sequence);
    if let Some(error) = download.failed.lock().ok().and_then(|value| value.clone()) {
        return Err(error);
    }
    decoded
}

/// Symphonia does not expose a codec category, so audio is inferred from the
/// parameters a container only fills for audio tracks.
fn is_audio_codec_parameters(params: &symphonia::core::codecs::CodecParameters) -> bool {
    params.sample_rate.is_some() || params.channels.is_some()
}

fn decode_media(
    media: MediaSourceStream,
    stop: Arc<AtomicBool>,
    start_seconds: f64,
    // Decoded audio to drop before playing: used when a streamed fragment starts before the wanted point.
    skip_seconds: f64,
    startup: Option<mpsc::Sender<Result<(), String>>>,
    diagnostic_sequence: u64,
) -> Result<(), String> {
    diagnostics_update(diagnostic_sequence, |snapshot| {
        snapshot.stage = "container_probe".into()
    });
    let probed = get_probe()
        .format(
            &Hint::new(),
            media,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|error| {
            let kind = sanitized_probe_error_kind(&error);
            diagnostics_update(diagnostic_sequence, |snapshot| {
                snapshot.probe_error_kind = Some(kind.into());
            });
            "container probe failed".to_string()
        })?;
    let mut format = probed.format;
    let (track_id, codec_params) = {
        // A relayed response may still contain the initialization of another
        // format, so prefer the first track that looks like audio instead of
        // the container's default track.
        let track = format
            .tracks()
            .iter()
            .find(|track| is_audio_codec_parameters(&track.codec_params))
            .or_else(|| format.default_track())
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
    diagnostics_update(diagnostic_sequence, |snapshot| {
        snapshot.stage = "codec_init".into()
    });
    let mut decoder = get_codecs()
        .make(&codec_params, &DecoderOptions::default())
        .map_err(|_| "codec initialization failed".to_string())?;
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
    diagnostics_update(diagnostic_sequence, |snapshot| {
        snapshot.stage = "output_create".into()
    });
    let (stream, output_rate, output_channels) =
        open_output_stream(queue.clone(), stop.clone(), source_rate, source_channels)
            .map_err(|_| "output device setup failed".to_string())?;
    diagnostics_update(diagnostic_sequence, |snapshot| {
        snapshot.stage = "output_start".into()
    });
    stream
        .play()
        .map_err(|_| "playback start failed".to_string())?;
    diagnostics_update(diagnostic_sequence, |snapshot| {
        snapshot.output_started = true;
        snapshot.stage = "decoding".into();
    });
    let mut startup = startup;
    let mut decoder_started = false;
    let mut skip_remaining = ((skip_seconds.max(0.0) * output_rate as f64) as usize)
        .saturating_mul(output_channels.max(1) as usize);

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
        let mut resampled = resample_interleaved(
            samples.samples(),
            source_rate,
            output_rate,
            source_channels,
            output_channels,
        );
        if skip_remaining > 0 {
            let dropped = skip_remaining.min(resampled.len());
            resampled.drain(..dropped);
            skip_remaining -= dropped;
        }
        output.extend(resampled);
        queue.changed.notify_all();
        if !decoder_started {
            decoder_started = true;
            diagnostics_update(diagnostic_sequence, |snapshot| {
                snapshot.decoder_started = true
            });
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

fn validate_stream_response(
    status: reqwest::StatusCode,
    content_type: &str,
    first_bytes: &[u8],
) -> Result<(), String> {
    if !status.is_success() {
        return Err(format!(
            "audio_http status={status} content-type={content_type}"
        ));
    }
    let body_preview = String::from_utf8_lossy(first_bytes)
        .trim_start()
        .to_ascii_lowercase();
    if body_preview.starts_with("https://") || body_preview.starts_with("http://") {
        return Err(format!(
            "audio_stream_url_body status={status} content-type={content_type}"
        ));
    }
    let media_type = content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if !(media_type.starts_with("audio/")
        || media_type == "application/octet-stream"
        || media_type == "application/vnd.yt-ump")
    {
        return Err(format!(
            "audio_stream_content_type status={status} content-type={content_type}"
        ));
    }
    if first_bytes.is_empty() {
        return Err("audio_stream_empty_body".to_string());
    }
    Ok(())
}

fn parse_stream_url(value: &str) -> Result<url::Url, String> {
    let parsed =
        url::Url::parse(value).map_err(|_| "Invalid YouTube audio stream URL".to_string())?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("Invalid YouTube audio stream URL scheme".to_string());
    }
    Ok(parsed)
}

fn stream_uses_ump(parsed: &url::Url) -> bool {
    parsed
        .query_pairs()
        .any(|(key, value)| key == "ump" && value == "1")
}

fn outgoing_stream_url(parsed: &url::Url, use_ump: bool) -> url::Url {
    let mut outgoing = parsed.clone();
    if use_ump {
        let retained = parsed
            .query_pairs()
            .filter(|(key, _)| key != "ump" && key != "range")
            .map(|(key, value)| (key.into_owned(), value.into_owned()))
            .collect::<Vec<_>>();
        outgoing.set_query(None);
        outgoing.query_pairs_mut().extend_pairs(retained);
    }
    outgoing
}

fn sanitized_range(parsed: &url::Url) -> Option<(u64, u64)> {
    let raw = parsed
        .query_pairs()
        .find_map(|(key, value)| (key == "range").then(|| value.into_owned()))?;
    let (start, end) = raw.split_once('-')?;
    let start = start.parse::<u64>().ok()?;
    let end = end.parse::<u64>().ok()?;
    (start <= end).then_some((start, end))
}

pub(crate) fn audio_request_context(parsed: &url::Url, method: &str) -> AudioStreamRequestContext {
    audio_request_context_with_ump(parsed, method, stream_uses_ump(parsed))
}

fn audio_request_context_with_ump(
    parsed: &url::Url,
    method: &str,
    use_ump: bool,
) -> AudioStreamRequestContext {
    let query_number = |name| sanitized_query_number(parsed, name);
    let (range_start, range_end) = sanitized_range(parsed)
        .map(|(start, end)| (Some(start), Some(end)))
        .unwrap_or((None, None));
    let mime_type = parsed
        .query_pairs()
        .find_map(|(key, value)| (key == "mime").then(|| sanitized_content_type(&value)))
        .unwrap_or_else(|| "unknown".into());
    let present = |name| parsed.query_pairs().any(|(key, _)| key == name);
    AudioStreamRequestContext {
        method: match method {
            "GET" => "GET",
            "POST" => "POST",
            _ => "other",
        }
        .into(),
        use_ump,
        mime_type,
        itag: query_number("itag").and_then(|value| u32::try_from(value).ok()),
        range_start,
        range_end,
        content_length: query_number("clen"),
        rn: query_number("rn"),
        rbuf: query_number("rbuf"),
        pot_present: present("pot"),
        cpn_present: present("cpn"),
    }
}

pub(crate) fn audio_request_header_summary(
    headers: &reqwest::header::HeaderMap,
) -> AudioRequestHeaderSummary {
    let mut header_names = headers
        .keys()
        .map(|name| name.as_str().to_ascii_lowercase())
        .collect::<Vec<_>>();
    header_names.sort();
    header_names.dedup();
    let value = |name: &str| headers.get(name).and_then(|header| header.to_str().ok());
    let present = |name: &str| headers.contains_key(name);
    let site_category = |raw: Option<&str>| match raw {
        None => "missing",
        Some(raw) => match url::Url::parse(raw).ok().and_then(|url| {
            url.host_str()
                .map(|host| host.eq_ignore_ascii_case("music.youtube.com"))
        }) {
            Some(true) => "music_youtube",
            Some(false) => "other",
            None => "other",
        },
    };
    let content_type = match value("content-type") {
        None => "missing",
        Some(raw)
            if raw.split(';').next().is_some_and(|mime| {
                mime.trim()
                    .eq_ignore_ascii_case("application/x-www-form-urlencoded")
            }) =>
        {
            "form_urlencoded"
        }
        Some(raw)
            if raw
                .split(';')
                .next()
                .is_some_and(|mime| mime.trim().eq_ignore_ascii_case("text/plain")) =>
        {
            "text_plain"
        }
        Some(_) => "other",
    };
    let accept = match value("accept") {
        None => "missing",
        Some(raw) if raw.contains("*/*") => "wildcard",
        Some(raw) if raw.to_ascii_lowercase().contains("audio/") => "audio",
        Some(raw) if raw.to_ascii_lowercase().contains("application/json") => "json",
        Some(_) => "other",
    };
    let accept_encoding = match value("accept-encoding") {
        None => "missing",
        Some(raw) => {
            let lower = raw.to_ascii_lowercase();
            match (
                lower.contains("gzip"),
                lower.contains("br"),
                lower.contains("zstd"),
            ) {
                (true, true, _) => "gzip_br",
                (true, false, _) => "gzip",
                (false, true, _) => "br",
                (false, false, true) => "zstd",
                _ => "other",
            }
        }
    };
    let raw_user_agent = value("user-agent");
    let user_agent_family = match raw_user_agent {
        None => "missing",
        Some(raw) if raw.contains("Edg/") => "edge",
        Some(raw) if raw.contains("Chrome/") || raw.contains("CriOS/") => "chrome",
        Some(raw) if raw.contains("Firefox/") => "firefox",
        Some(raw) if raw.contains("Safari/") => "safari",
        Some(_) => "other",
    };
    let user_agent_major = raw_user_agent.and_then(|raw| {
        ["Edg/", "Chrome/", "CriOS/", "Firefox/", "Version/"]
            .iter()
            .find_map(|marker| {
                raw.split_once(marker)
                    .and_then(|(_, version)| version.split('.').next())
                    .and_then(|major| {
                        let digits = major
                            .chars()
                            .take_while(char::is_ascii_digit)
                            .collect::<String>();
                        digits.parse::<u16>().ok()
                    })
            })
    });
    let sec_fetch_site = match value("sec-fetch-site")
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        None => "missing",
        Some("same-origin") => "same_origin",
        Some("same-site") => "same_site",
        Some("cross-site") => "cross_site",
        Some("none") => "none",
        Some(_) => "other",
    };

    AudioRequestHeaderSummary {
        header_names,
        origin: site_category(value("origin")).into(),
        referer: site_category(value("referer")).into(),
        user_agent_family: user_agent_family.into(),
        user_agent_major,
        content_type: content_type.into(),
        content_length: value("content-length").and_then(|raw| raw.parse::<u64>().ok()),
        accept: accept.into(),
        accept_encoding: accept_encoding.into(),
        cookie_present: present("cookie"),
        authorization_present: present("authorization"),
        sec_ch_ua_present: present("sec-ch-ua"),
        sec_ch_ua_mobile_present: present("sec-ch-ua-mobile"),
        sec_ch_ua_platform_present: present("sec-ch-ua-platform"),
        sec_fetch_site: sec_fetch_site.into(),
        x_goog_visitor_id_present: present("x-goog-visitor-id"),
        x_goog_authuser_present: present("x-goog-authuser"),
    }
}

fn sanitized_query_number(parsed: &url::Url, name: &str) -> Option<u64> {
    parsed
        .query_pairs()
        .find_map(|(key, value)| (key == name).then(|| value.parse::<u64>().ok()).flatten())
}

fn sanitized_ump_request_context(parsed: &url::Url) -> String {
    let numeric = |name| {
        sanitized_query_number(parsed, name)
            .map_or_else(|| "none".to_string(), |value| value.to_string())
    };
    let pot_present = parsed.query_pairs().any(|(key, _)| key == "pot");
    format!(
        "rn={} rbuf={} clen={} pot_present={pot_present}",
        numeric("rn"),
        numeric("rbuf"),
        numeric("clen")
    )
}

fn sanitized_content_type(value: &str) -> String {
    let media_type = value.split(';').next().unwrap_or("unknown").trim();
    if media_type.len() <= 80
        && media_type
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"!#$&^_.+-/*".contains(&byte))
    {
        media_type.to_ascii_lowercase()
    } else {
        "other".to_string()
    }
}

fn stream_http_method(parsed: &url::Url) -> reqwest::Method {
    let _ = parsed;
    reqwest::Method::GET
}

fn stream_bridge_error(error: &str) -> String {
    let (code, message, retryable) = if error.starts_with("audio_stream_url_body") {
        ("audio_stream_url_body", "O YouTube Music retornou uma URL como texto no lugar dos bytes de áudio. A resolução do stream pode estar desatualizada; tente novamente para resolver um stream novo.", true)
    } else if error.starts_with("audio_http_ump") {
        ("audio_stream_ump_rejected", "O YouTube Music rejeitou a solicitação do stream de áudio (UMP/HTTP 400). Resolva um stream novo e tente novamente; os detalhes da resposta foram ocultados por segurança.", true)
    } else if error.starts_with("audio_http") {
        ("audio_stream_http_error", "A entrega do stream de áudio falhou via HTTP. Tente novamente para resolver um stream novo.", true)
    } else if error.starts_with("audio_stream_content_type")
        || error.starts_with("audio_stream_empty_body")
    {
        ("audio_stream_invalid_response", "A resposta do stream não contém mídia de áudio válida. Tente novamente; se persistir, verifique a sessão do YouTube Music.", true)
    } else {
        ("audio_stream_resolution_error", "Não foi possível iniciar o stream de áudio. Tente novamente e, se persistir, reconecte a conta YouTube Music.", true)
    };
    serde_json::json!({
        "code": code,
        "operation": "play_audio_url",
        "message": message,
        "correlationId": "audio-stream",
        "retryable": retryable,
        "details": { "reason": error.split_whitespace().next().unwrap_or("stream_error") }
    })
    .to_string()
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
