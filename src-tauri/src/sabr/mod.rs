//! Native SABR/UMP client for YouTube Music audio.
//!
//! VibeDeck issues the ABR media requests itself instead of relying on a page
//! or webview to drive playback. The protocol is internal and undocumented;
//! field numbers and part ids below come from a pinned third-party
//! reverse-engineered schema and may change without notice.
//!
//! Nothing in this module logs or persists media URLs, tokens, signatures,
//! cookies, media bytes, or track identifiers.

#[cfg(debug_assertions)]
pub(crate) mod bootstrap_probe;
#[cfg(all(debug_assertions, feature = "bootstrap-inspection"))]
pub(crate) mod bootstrap_script_probe;
#[cfg(debug_assertions)]
pub(crate) mod bootstrap_structure_probe;
#[cfg(all(windows, debug_assertions, feature = "native-playback-trial"))]
pub(crate) mod native_trial;
pub(crate) mod player_script;
pub mod proto;
#[cfg(debug_assertions)]
pub(crate) mod request_shape;
#[cfg(debug_assertions)]
pub(crate) mod script_probe;
pub(crate) mod url_resolution;

#[cfg(debug_assertions)]
pub mod client_survey;
#[cfg(all(windows, debug_assertions, feature = "native-playback-trial"))]
pub mod native_media_probe;
#[cfg(all(windows, debug_assertions, feature = "native-playback-trial"))]
pub mod native_playback_probe;
#[cfg(any(debug_assertions, feature = "native-range-playback"))]
pub mod range_plan;
#[cfg(any(debug_assertions, feature = "native-range-playback"))]
pub mod range_policy;
#[cfg(debug_assertions)]
pub mod range_probe;
#[cfg(any(debug_assertions, feature = "native-range-playback"))]
pub mod url_resolver;
#[cfg(debug_assertions)]
pub mod session_probe;
#[cfg(debug_assertions)]
pub mod transport_probe;

use proto::{Field, Reader, Writer};

/// `WEB_REMIX` client id used by music.youtube.com.
pub const CLIENT_NAME_WEB_REMIX: i32 = 67;

/// `enabled_track_types_bitfield` value that requests audio only.
const TRACK_TYPE_AUDIO: i32 = 1;

// UMP part ids that the SABR client understands.
const PART_MEDIA_HEADER: u32 = 20;
const PART_MEDIA: u32 = 21;
const PART_NEXT_REQUEST_POLICY: u32 = 35;
const PART_SABR_REDIRECT: u32 = 43;
const PART_SABR_ERROR: u32 = 44;
const PART_SABR_SEEK: u32 = 45;
const PART_RELOAD_PLAYER_RESPONSE: u32 = 46;
const PART_SABR_CONTEXT_UPDATE: u32 = 57;
const PART_STREAM_PROTECTION_STATUS: u32 = 58;
const PART_SABR_CONTEXT_SENDING_POLICY: u32 = 59;
const PART_END_OF_TRACK: u32 = 62;

/// A protobuf `misc.FormatId`, used to identify one audio representation.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FormatId {
    pub itag: i32,
    pub last_modified: u64,
    pub xtags: String,
}

impl FormatId {
    pub fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::new();
        writer.int32_field(1, self.itag);
        writer.uint64_field(2, self.last_modified);
        writer.string_field(3, &self.xtags);
        writer.into_vec()
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, String> {
        let mut format = FormatId::default();
        let mut reader = Reader::new(bytes);
        while let Some((field, value)) = reader.next_field()? {
            match (field, value) {
                (1, Field::Varint(itag)) => format.itag = itag as i32,
                (2, Field::Varint(last_modified)) => format.last_modified = last_modified,
                (3, Field::Bytes(xtags)) => {
                    format.xtags = String::from_utf8_lossy(xtags).to_string();
                }
                _ => {}
            }
        }
        Ok(format)
    }
}

/// One audio representation offered by the player response.
#[derive(Clone, Debug)]
pub struct AudioFormat {
    pub id: FormatId,
    pub mime_type: String,
    pub bitrate: u64,
    pub audio_quality: String,
    pub sample_rate: u32,
    pub channels: u32,
    pub approx_duration_ms: u64,
    pub content_length: u64,
    pub init_range: Option<(u64, u64)>,
}

impl AudioFormat {
    fn quality_rank(&self) -> u8 {
        match self.audio_quality.as_str() {
            "AUDIO_QUALITY_ULTRALOW" => 0,
            "AUDIO_QUALITY_LOW" => 1,
            "AUDIO_QUALITY_MEDIUM" => 2,
            "AUDIO_QUALITY_HIGH" => 3,
            _ => 2,
        }
    }
}

/// Failure categories for the player-response bootstrap. Only these fixed
/// codes are logged; provider payloads and URLs are never recorded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootstrapError {
    PlayabilityNotOk,
    MissingStreamingData,
    MissingStreamingUrl,
    MissingUstreamerConfig,
    MissingAudioFormats,
}

impl BootstrapError {
    pub fn code(self) -> &'static str {
        match self {
            BootstrapError::PlayabilityNotOk => "playability_not_ok",
            BootstrapError::MissingStreamingData => "missing_streaming_data",
            BootstrapError::MissingStreamingUrl => "missing_server_abr_url",
            BootstrapError::MissingUstreamerConfig => "missing_ustreamer_config",
            BootstrapError::MissingAudioFormats => "missing_audio_formats",
        }
    }
}

/// Everything needed to start a native SABR session. `streaming_url` and
/// `po_token` are credentials for the media CDN and stay in memory.
#[derive(Clone, Debug)]
pub struct SabrBootstrap {
    pub video_id: String,
    pub streaming_url: String,
    pub ustreamer_config: Vec<u8>,
    pub audio_formats: Vec<AudioFormat>,
    pub po_token: Option<Vec<u8>>,
    pub client_version: String,
    pub duration_ms: i64,
}

/// Raw Innertube player JSON, not the normalized YouTube.js representation.
/// Shared by bootstrap and diagnostics so both agree on usable config bytes.
pub fn player_ustreamer_config(payload: &serde_json::Value) -> Option<Vec<u8>> {
    payload
        .pointer("/playerConfig/mediaCommonConfig/mediaUstreamerRequestConfig/videoPlaybackUstreamerConfig")
        .and_then(|value| value.as_str())
        .and_then(proto::decode_base64)
        .filter(|config| !config.is_empty())
}

impl SabrBootstrap {
    /// Extracts the SABR bootstrap from a `youtubei/v1/player` response.
    pub fn from_player_payload(
        payload: &serde_json::Value,
        video_id: &str,
        client_version: &str,
    ) -> Result<Self, BootstrapError> {
        let status = payload
            .pointer("/playabilityStatus/status")
            .and_then(|value| value.as_str())
            .unwrap_or_default();
        if status != "OK" {
            return Err(BootstrapError::PlayabilityNotOk);
        }

        let streaming = payload
            .get("streamingData")
            .ok_or(BootstrapError::MissingStreamingData)?;

        let streaming_url = streaming
            .get("serverAbrStreamingUrl")
            .and_then(|value| value.as_str())
            .filter(|url| url.starts_with("https://"))
            .ok_or(BootstrapError::MissingStreamingUrl)?;

        let ustreamer_config =
            player_ustreamer_config(payload).ok_or(BootstrapError::MissingUstreamerConfig)?;

        let mut audio_formats = Vec::new();
        if let Some(formats) = streaming
            .get("adaptiveFormats")
            .and_then(|value| value.as_array())
        {
            for format in formats {
                let Some(mime_type) = format.get("mimeType").and_then(|value| value.as_str())
                else {
                    continue;
                };
                if !mime_type.starts_with("audio/") {
                    continue;
                }
                let Some(itag) = format.get("itag").and_then(|value| value.as_i64()) else {
                    continue;
                };
                let itag = i32::try_from(itag).unwrap_or_default();
                if itag <= 0 {
                    continue;
                }
                let Some(last_modified) = json_u64(format.get("lastModified")).filter(|v| *v > 0)
                else {
                    continue;
                };
                let init_range = format.get("initRange").and_then(|range| {
                    let start = json_u64(range.get("start"))?;
                    let end = json_u64(range.get("end"))?;
                    Some((start, end))
                });
                audio_formats.push(AudioFormat {
                    id: FormatId {
                        itag,
                        last_modified,
                        xtags: format
                            .get("xtags")
                            .and_then(|value| value.as_str())
                            .unwrap_or_default()
                            .to_string(),
                    },
                    mime_type: mime_type.to_string(),
                    bitrate: format
                        .get("bitrate")
                        .and_then(|value| value.as_u64())
                        .unwrap_or_default(),
                    audio_quality: format
                        .get("audioQuality")
                        .and_then(|value| value.as_str())
                        .unwrap_or_default()
                        .to_string(),
                    sample_rate: format
                        .get("sampleRate")
                        .and_then(|value| value.as_str())
                        .and_then(|value| value.parse().ok())
                        .unwrap_or_default(),
                    channels: format
                        .get("audioChannels")
                        .and_then(|value| value.as_u64())
                        .unwrap_or_default() as u32,
                    approx_duration_ms: json_u64(format.get("approxDurationMs"))
                        .unwrap_or_default(),
                    content_length: json_u64(format.get("contentLength")).unwrap_or_default(),
                    init_range,
                });
            }
        }
        if audio_formats.is_empty() {
            return Err(BootstrapError::MissingAudioFormats);
        }
        // Highest advertised audio quality wins; bitrate breaks ties.
        audio_formats.sort_by_key(|format| (format.quality_rank(), format.bitrate));

        let po_token = payload
            .pointer("/serviceIntegrityDimensions/poToken")
            .or_else(|| payload.pointer("/streamingData/serviceIntegrityDimensions/poToken"))
            .and_then(|value| value.as_str())
            .and_then(proto::decode_base64)
            .filter(|token| !token.is_empty());

        let duration_ms = payload
            .pointer("/videoDetails/lengthSeconds")
            .and_then(|value| value.as_str())
            .and_then(|value| value.parse::<f64>().ok())
            .map(|seconds| (seconds * 1000.0).round() as i64)
            .unwrap_or_default();

        Ok(Self {
            video_id: video_id.to_string(),
            streaming_url: streaming_url.to_string(),
            ustreamer_config,
            audio_formats,
            po_token,
            client_version: client_version.to_string(),
            duration_ms,
        })
    }

    /// The preferred audio representation (last item after sorting).
    pub fn selected_audio(&self) -> &AudioFormat {
        self.audio_formats
            .last()
            .expect("bootstrap always contains at least one audio format")
    }
}

fn json_u64(value: Option<&serde_json::Value>) -> Option<u64> {
    match value? {
        serde_json::Value::Number(number) => number.as_u64(),
        serde_json::Value::String(text) => text.parse().ok(),
        _ => None,
    }
}

/// A protobuf `video_streaming.BufferedRange`.
#[derive(Clone, Debug, Default)]
pub struct BufferedRange {
    pub format: FormatId,
    pub start_time_ms: i64,
    pub duration_ms: i64,
    pub start_segment_index: i32,
    pub end_segment_index: i32,
}

impl BufferedRange {
    fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::new();
        writer.bytes_field(1, &self.format.encode());
        writer.int64_field(2, self.start_time_ms);
        writer.int64_field(3, self.duration_ms);
        writer.int32_field(4, self.start_segment_index);
        writer.int32_field(5, self.end_segment_index);
        writer.into_vec()
    }
}

/// Mutable per-request playback state, mirroring the browser's `ClientAbrState`.
#[derive(Clone, Debug)]
pub struct AbrRequestState {
    /// Formats confirmed initialized by response metadata, not merely selected.
    /// The coordinator must clear this list when initialization is reset.
    pub initialized_formats: Vec<FormatId>,
    pub player_time_ms: i64,
    pub elapsed_wall_time_ms: i64,
    pub time_since_last_seek_ms: i64,
    pub time_since_last_action_ms: i64,
    pub buffered_ranges: Vec<BufferedRange>,
    pub playback_cookie: Option<Vec<u8>>,
    pub sabr_contexts: Vec<(i32, Vec<u8>)>,
    pub unsent_contexts: Vec<i32>,
    pub bandwidth_estimate: Option<u64>,
    pub playback_rate: f32,
    pub visibility: i32,
    pub enabled_track_types: i32,
    /// Debug experiment: encode the request the way the web player was observed to
    /// (no field 16; `ClientInfo` with fields 1, 16, 17, 18, 19). Off keeps the reference format.
    pub browser_shape: bool,
}

impl AbrRequestState {
    pub fn new(player_time_ms: i64) -> Self {
        Self {
            initialized_formats: Vec::new(),
            player_time_ms,
            elapsed_wall_time_ms: 0,
            time_since_last_seek_ms: 0,
            time_since_last_action_ms: 0,
            buffered_ranges: Vec::new(),
            playback_cookie: None,
            sabr_contexts: Vec::new(),
            unsent_contexts: Vec::new(),
            bandwidth_estimate: None,
            playback_rate: 1.0,
            visibility: 1,
            enabled_track_types: TRACK_TYPE_AUDIO,
            browser_shape: false,
        }
    }

    fn encode_client_abr_state(&self) -> Vec<u8> {
        let mut writer = Writer::new();
        writer.int64_field(28, self.player_time_ms);
        writer.int64_field(29, self.time_since_last_seek_ms);
        writer.int32_field(34, self.visibility);
        writer.float_field(35, self.playback_rate);
        writer.int64_field(36, self.elapsed_wall_time_ms);
        writer.int64_field(39, self.time_since_last_action_ms);
        writer.int32_field(40, self.enabled_track_types);
        if let Some(bandwidth) = self.bandwidth_estimate {
            writer.int64_field(23, bandwidth as i64);
        }
        writer.into_vec()
    }
}

/// Encodes a `video_streaming.VideoPlaybackAbrRequest` for one audio track.
pub fn build_abr_request(bootstrap: &SabrBootstrap, state: &AbrRequestState) -> Vec<u8> {
    let audio = bootstrap.selected_audio();
    let audio_id = audio.id.encode();

    let mut request = Writer::new();
    request.bytes_field(1, &state.encode_client_abr_state());
    for format in &state.initialized_formats {
        request.bytes_field(2, &format.encode());
    }
    for range in &state.buffered_ranges {
        request.bytes_field(3, &range.encode());
    }
    request.bytes_field(5, &bootstrap.ustreamer_config);
    if !state.browser_shape {
        request.bytes_field(16, &audio_id);
    }
    request.bytes_field(19, &encode_streamer_context(bootstrap, state));
    request.into_vec()
}

fn encode_streamer_context(bootstrap: &SabrBootstrap, state: &AbrRequestState) -> Vec<u8> {
    let mut client_info = Writer::new();
    // Optional locale/device fields are omitted instead of inventing values.
    // Pinned reference factory supplies only client identity/version here.
    if state.browser_shape {
        // Field numbers observed in the web player; the values are assumptions of this
        // experiment (hl, os name, os version), not captured data.
        client_info.string_field(1, "en");
    }
    client_info.int32_field(16, CLIENT_NAME_WEB_REMIX);
    client_info.string_field(17, &bootstrap.client_version);
    if state.browser_shape {
        client_info.string_field(18, "Windows");
        client_info.string_field(19, "10.0");
    }

    let mut context = Writer::new();
    context.message_field(1, &client_info);
    if let Some(token) = &bootstrap.po_token {
        context.bytes_field(2, token);
    }
    if let Some(cookie) = &state.playback_cookie {
        context.bytes_field(3, cookie);
    }
    for (kind, value) in &state.sabr_contexts {
        let mut entry = Writer::new();
        entry.int32_field(1, *kind);
        entry.bytes_field(2, value);
        context.message_field(5, &entry);
    }
    for kind in &state.unsent_contexts {
        context.int32_field(6, *kind);
    }
    context.into_vec()
}

/// `video_streaming.NextRequestPolicy` guidance for the next request.
#[derive(Clone, Debug, Default)]
pub struct NextRequestPolicy {
    pub target_audio_readahead_ms: Option<i32>,
    pub target_video_readahead_ms: Option<i32>,
    pub max_time_since_last_request_ms: Option<i32>,
    pub backoff_time_ms: Option<i32>,
    pub min_audio_readahead_ms: Option<i32>,
    pub min_video_readahead_ms: Option<i32>,
    pub playback_cookie: Option<Vec<u8>>,
}

/// `video_streaming.SabrContextUpdate`, echoed back while it stays active.
#[derive(Clone, Debug)]
pub struct SabrContextUpdate {
    pub kind: i32,
    pub value: Vec<u8>,
    pub send_by_default: bool,
    pub keep_existing: bool,
}

/// Media-header metadata of one segment in a response.
#[derive(Clone, Debug, Default)]
pub struct SegmentInfo {
    pub header_id: u8,
    pub itag: i32,
    pub is_initialization: bool,
    pub segment_num: i32,
    pub start_ms: i64,
    pub duration_ms: i64,
    pub byte_start: i64,
    pub byte_length: i64,
    pub media_bytes: usize,
    pub media_end: bool,
}

impl SegmentInfo {
    pub fn end_ms(&self) -> i64 {
        if self.start_ms > 0 || self.duration_ms > 0 {
            self.start_ms.saturating_add(self.duration_ms)
        } else {
            0
        }
    }
}

/// Control information decoded from one SABR response body.
#[derive(Clone, Debug, Default)]
pub struct SabrControl {
    pub part_counts: Vec<(u32, u32)>,
    pub media_bytes: usize,
    pub segments: Vec<SegmentInfo>,
    pub next_request_policy: Option<NextRequestPolicy>,
    pub redirect_url: Option<String>,
    pub error_code: Option<i32>,
    pub seek_to_ms: Option<i64>,
    pub reload_token: Option<Vec<u8>>,
    pub stream_protection_status: Option<i32>,
    pub stream_protection_max_retries: Option<i32>,
    pub context_updates: Vec<SabrContextUpdate>,
    pub start_contexts: Vec<i32>,
    pub stop_contexts: Vec<i32>,
    pub discard_contexts: Vec<i32>,
    pub end_of_track: bool,
}

impl SabrControl {
    fn count_part(&mut self, part_type: u32) {
        if let Some((_, count)) = self
            .part_counts
            .iter_mut()
            .find(|(kind, _)| *kind == part_type)
        {
            *count += 1;
        } else {
            self.part_counts.push((part_type, 1));
        }
    }

    /// Latest audio segment end time seen in this response, if the media
    /// headers declared one.
    pub fn buffered_until_ms(&self) -> Option<i64> {
        self.segments
            .iter()
            .filter(|segment| !segment.is_initialization && segment.itag != 0)
            .map(SegmentInfo::end_ms)
            .filter(|end| *end > 0)
            .max()
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

/// Parses the control parts of one SABR response. Media bytes are only
/// counted; their content is passed to the audio engine untouched.
pub fn parse_response_parts(body: &[u8]) -> Result<SabrControl, String> {
    let mut control = SabrControl::default();
    let mut segment_index_by_header: Vec<(u8, usize)> = Vec::new();
    let mut offset = 0;
    while offset < body.len() {
        let Some((part_type, after_type)) = read_ump_varint(body, offset)? else {
            break;
        };
        let Some((part_size, after_size)) = read_ump_varint(body, after_type)? else {
            break;
        };
        let start = after_size;
        let end = start
            .checked_add(part_size as usize)
            .ok_or_else(|| "Invalid UMP part length".to_string())?;
        let Some(payload) = body.get(start..end) else {
            // A truncated part means the response ended early; keep what was
            // already decoded instead of failing the whole session.
            break;
        };
        offset = end;
        control.count_part(part_type);

        match part_type {
            PART_MEDIA_HEADER => {
                if let Ok(segment) = parse_media_header(payload) {
                    if let Some((_, index)) = segment_index_by_header
                        .iter()
                        .find(|(header, _)| *header == segment.header_id)
                    {
                        control.segments[*index] = segment;
                    } else {
                        segment_index_by_header.push((segment.header_id, control.segments.len()));
                        control.segments.push(segment);
                    }
                }
            }
            PART_MEDIA => {
                control.media_bytes += payload.len();
                if let Some(header_id) = payload.first() {
                    if let Some((_, index)) = segment_index_by_header
                        .iter()
                        .find(|(header, _)| header == header_id)
                    {
                        control.segments[*index].media_bytes += payload.len();
                    }
                }
            }
            PART_NEXT_REQUEST_POLICY => {
                control.next_request_policy = Some(parse_next_request_policy(payload));
            }
            PART_SABR_REDIRECT => {
                if let Some(url) = parse_single_string(payload, 1) {
                    if is_allowed_media_url(&url) {
                        control.redirect_url = Some(url);
                    }
                }
            }
            PART_SABR_ERROR => {
                control.error_code = parse_single_varint(payload, 2).map(|value| value as i32);
            }
            PART_SABR_SEEK => {
                let mut reader = Reader::new(payload);
                let mut time = 0i64;
                let mut timescale = 1000i64;
                while let Some((field, value)) = reader.next_field()? {
                    match (field, value) {
                        (1, Field::Varint(seek)) => time = seek as i64,
                        (2, Field::Varint(scale)) => timescale = (scale as i64).max(1),
                        _ => {}
                    }
                }
                control.seek_to_ms = Some(time.saturating_mul(1000) / timescale);
            }
            PART_RELOAD_PLAYER_RESPONSE => {
                if let Some(params) = parse_single_bytes(payload, 1) {
                    control.reload_token =
                        parse_single_bytes(params, 1).map(|token| token.to_vec());
                }
            }
            PART_STREAM_PROTECTION_STATUS => {
                control.stream_protection_status =
                    parse_single_varint(payload, 1).map(|value| value as i32);
                control.stream_protection_max_retries =
                    parse_single_varint(payload, 2).map(|value| value as i32);
            }
            PART_SABR_CONTEXT_UPDATE => {
                control
                    .context_updates
                    .extend(parse_context_updates(payload));
            }
            PART_SABR_CONTEXT_SENDING_POLICY => {
                let (start, stop, discard) = parse_context_sending_policy(payload);
                control.start_contexts.extend(start);
                control.stop_contexts.extend(stop);
                control.discard_contexts.extend(discard);
            }
            PART_END_OF_TRACK => control.end_of_track = true,
            _ => {}
        }
    }
    Ok(control)
}

fn parse_media_header(payload: &[u8]) -> Result<SegmentInfo, String> {
    let mut segment = SegmentInfo::default();
    let mut reader = Reader::new(payload);
    while let Some((field, value)) = reader.next_field()? {
        match (field, value) {
            (1, Field::Varint(header_id)) => segment.header_id = header_id as u8,
            (3, Field::Varint(itag)) => segment.itag = itag as i32,
            (6, Field::Varint(byte_start)) => segment.byte_start = byte_start as i64,
            (8, Field::Varint(is_init)) => segment.is_initialization = is_init != 0,
            (9, Field::Varint(segment_num)) => segment.segment_num = segment_num as i32,
            (11, Field::Varint(start_ms)) => segment.start_ms = start_ms as i64,
            (12, Field::Varint(duration_ms)) => segment.duration_ms = duration_ms as i64,
            (14, Field::Varint(length)) => segment.byte_length = length as i64,
            _ => {}
        }
    }
    Ok(segment)
}

fn parse_next_request_policy(payload: &[u8]) -> NextRequestPolicy {
    let mut policy = NextRequestPolicy::default();
    let mut reader = Reader::new(payload);
    while let Ok(Some((field, value))) = reader.next_field() {
        match (field, value) {
            (1, Field::Varint(value)) => policy.target_audio_readahead_ms = Some(value as i32),
            (2, Field::Varint(value)) => policy.target_video_readahead_ms = Some(value as i32),
            (3, Field::Varint(value)) => policy.max_time_since_last_request_ms = Some(value as i32),
            (4, Field::Varint(value)) => policy.backoff_time_ms = Some(value as i32),
            (5, Field::Varint(value)) => policy.min_audio_readahead_ms = Some(value as i32),
            (6, Field::Varint(value)) => policy.min_video_readahead_ms = Some(value as i32),
            (7, Field::Bytes(cookie)) => policy.playback_cookie = Some(cookie.to_vec()),
            _ => {}
        }
    }
    policy
}

fn parse_context_updates(payload: &[u8]) -> Vec<SabrContextUpdate> {
    let mut update = SabrContextUpdate {
        kind: 0,
        value: Vec::new(),
        send_by_default: false,
        keep_existing: false,
    };
    let mut reader = Reader::new(payload);
    while let Ok(Some((field, value))) = reader.next_field() {
        match (field, value) {
            (1, Field::Varint(kind)) => update.kind = kind as i32,
            (3, Field::Bytes(bytes)) => update.value = bytes.to_vec(),
            (4, Field::Varint(send)) => update.send_by_default = send != 0,
            (5, Field::Varint(policy)) => update.keep_existing = policy == 2,
            _ => {}
        }
    }
    if update.kind == 0 || update.value.is_empty() {
        return Vec::new();
    }
    vec![update]
}

fn parse_context_sending_policy(payload: &[u8]) -> (Vec<i32>, Vec<i32>, Vec<i32>) {
    let mut start = Vec::new();
    let mut stop = Vec::new();
    let mut discard = Vec::new();
    let mut reader = Reader::new(payload);
    while let Ok(Some((field, value))) = reader.next_field() {
        let target = match field {
            1 => &mut start,
            2 => &mut stop,
            3 => &mut discard,
            _ => continue,
        };
        match value {
            Field::Varint(value) => target.push(value as i32),
            Field::Bytes(bytes) => {
                // Tolerate a packed encoding as well; proto2 does not require it.
                let mut offset = 0;
                while let Some((value, consumed)) = proto::decode_varint(&bytes[offset..]) {
                    offset += consumed;
                    target.push(value as i32);
                }
            }
            _ => {}
        }
    }
    (start, stop, discard)
}

fn parse_single_varint(payload: &[u8], wanted: u32) -> Option<u64> {
    let mut reader = Reader::new(payload);
    while let Some((field, value)) = reader.next_field().ok()? {
        if field == wanted {
            return value.as_u64();
        }
    }
    None
}

fn parse_single_bytes<'a>(payload: &'a [u8], wanted: u32) -> Option<&'a [u8]> {
    let mut reader = Reader::new(payload);
    while let Some((field, value)) = reader.next_field().ok()? {
        if field == wanted {
            return value.as_bytes();
        }
    }
    None
}

fn parse_single_string(payload: &[u8], wanted: u32) -> Option<String> {
    let mut reader = Reader::new(payload);
    while let Some((field, value)) = reader.next_field().ok()? {
        if field == wanted {
            return value.as_str().map(|text| text.to_string());
        }
    }
    None
}

/// Redirects are only followed to HTTPS media hosts, so a malformed or
/// unexpected redirect can never forward the session to another origin.
fn is_allowed_media_url(url: &str) -> bool {
    url::Url::parse(url)
        .ok()
        .filter(|parsed| parsed.scheme() == "https")
        .and_then(|parsed| parsed.host_str().map(|host| host.to_string()))
        .is_some_and(|host| host == "googlevideo.com" || host.ends_with(".googlevideo.com"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn player_payload() -> serde_json::Value {
        serde_json::json!({
            "playabilityStatus": { "status": "OK" },
            "videoDetails": { "lengthSeconds": "231" },
            "serviceIntegrityDimensions": { "poToken": "dG9rZW4=" },
            "playerConfig": {
                "mediaCommonConfig": {
                    "mediaUstreamerRequestConfig": {
                        "videoPlaybackUstreamerConfig": "AQID"
                    }
                }
            },
            "streamingData": {
                "serverAbrStreamingUrl": "https://rr1---sn.example.googlevideo.com/videoplayback?sabr=1&cpn=abc",
                "adaptiveFormats": [
                    {
                        "itag": 140,
                        "mimeType": "audio/mp4; codecs=\"mp4a.40.2\"",
                        "bitrate": 129000,
                        "audioQuality": "AUDIO_QUALITY_MEDIUM",
                        "sampleRate": "44100",
                        "audioChannels": 2,
                        "approxDurationMs": "231000",
                        "contentLength": "3720000",
                        "lastModified": "1700000000000000",
                        "initRange": { "start": "0", "end": "906" }
                    },
                    {
                        "itag": 251,
                        "mimeType": "audio/webm; codecs=\"opus\"",
                        "bitrate": 160000,
                        "audioQuality": "AUDIO_QUALITY_HIGH",
                        "sampleRate": "48000",
                        "audioChannels": 2,
                        "lastModified": "1700000000000001"
                    },
                    {
                        "itag": 135,
                        "mimeType": "video/mp4",
                        "bitrate": 500000
                    }
                ]
            }
        })
    }

    #[test]
    fn bootstrap_parses_audio_formats_and_credentials() {
        let payload = player_payload();
        let bootstrap = SabrBootstrap::from_player_payload(&payload, "abc", "1.2026").unwrap();
        assert_eq!(bootstrap.audio_formats.len(), 2);
        assert_eq!(bootstrap.selected_audio().id.itag, 251);
        assert_eq!(bootstrap.ustreamer_config, vec![1, 2, 3]);
        assert_eq!(bootstrap.po_token.as_deref(), Some(b"token".as_slice()));
        assert_eq!(bootstrap.duration_ms, 231_000);
        assert!(bootstrap.streaming_url.contains("sabr=1"));
    }

    #[test]
    fn bootstrap_reports_specific_missing_field_codes() {
        let mut payload = player_payload();
        payload["streamingData"]
            .as_object_mut()
            .unwrap()
            .remove("serverAbrStreamingUrl");
        assert_eq!(
            SabrBootstrap::from_player_payload(&payload, "abc", "1")
                .unwrap_err()
                .code(),
            "missing_server_abr_url"
        );

        let mut payload = player_payload();
        payload["playerConfig"]["mediaCommonConfig"]["mediaUstreamerRequestConfig"]
            .as_object_mut()
            .unwrap()
            .remove("videoPlaybackUstreamerConfig");
        assert_eq!(
            SabrBootstrap::from_player_payload(&payload, "abc", "1")
                .unwrap_err()
                .code(),
            "missing_ustreamer_config"
        );
    }

    #[test]
    fn ustreamer_config_rejects_missing_invalid_and_misplaced_values() {
        for value in [
            serde_json::Value::Null,
            serde_json::json!(42),
            serde_json::json!(""),
            serde_json::json!("!"),
        ] {
            let mut payload = player_payload();
            payload["playerConfig"]["mediaCommonConfig"]["mediaUstreamerRequestConfig"]
                ["videoPlaybackUstreamerConfig"] = value;
            // The old, incorrect location must not mask an invalid raw config.
            payload["streamingData"]["videoPlaybackUstreamerConfig"] = serde_json::json!("AQID");
            assert!(player_ustreamer_config(&payload).is_none());
            assert_eq!(
                SabrBootstrap::from_player_payload(&payload, "synthetic", "1").unwrap_err(),
                BootstrapError::MissingUstreamerConfig
            );
        }
        for parent in [
            "/playerConfig",
            "/playerConfig/mediaCommonConfig",
            "/playerConfig/mediaCommonConfig/mediaUstreamerRequestConfig",
        ] {
            let mut payload = player_payload();
            *payload.pointer_mut(parent).unwrap() = serde_json::Value::Null;
            assert!(player_ustreamer_config(&payload).is_none());
        }
    }

    #[test]
    fn request_encodes_abr_state_formats_and_streamer_context() {
        let payload = player_payload();
        let bootstrap = SabrBootstrap::from_player_payload(&payload, "abc", "1.2026").unwrap();
        let mut state = AbrRequestState::new(42_000);
        state.playback_cookie = Some(vec![9, 9]);
        state.sabr_contexts = vec![(3, vec![7, 7])];
        let request = build_abr_request(&bootstrap, &state);

        let mut reader = Reader::new(&request);
        let mut fields = Vec::new();
        let mut abr_state = Vec::new();
        let mut context = Vec::new();
        while let Some((field, value)) = reader.next_field().unwrap() {
            fields.push(field);
            match field {
                1 => abr_state = value.as_bytes().unwrap().to_vec(),
                19 => context = value.as_bytes().unwrap().to_vec(),
                _ => {}
            }
        }
        assert_eq!(fields, vec![1, 5, 16, 19]);

        let mut abr_reader = Reader::new(&abr_state);
        let mut player_time = None;
        while let Some((field, value)) = abr_reader.next_field().unwrap() {
            if field == 28 {
                player_time = value.as_i64();
            }
        }
        assert_eq!(player_time, Some(42_000));

        let mut context_reader = Reader::new(&context);
        let mut has_client_info = false;
        let mut has_po_token = false;
        let mut has_cookie = false;
        let mut has_context = false;
        while let Some((field, value)) = context_reader.next_field().unwrap() {
            match field {
                1 => {
                    let mut client = Reader::new(value.as_bytes().unwrap());
                    let mut fields = Vec::new();
                    while let Some((field, value)) = client.next_field().unwrap() {
                        fields.push(field);
                        match field {
                            16 => assert_eq!(value.as_u64(), Some(CLIENT_NAME_WEB_REMIX as u64)),
                            17 => {
                                assert_eq!(value.as_str(), Some(bootstrap.client_version.as_str()))
                            }
                            _ => panic!("must not fabricate locale/device context"),
                        }
                    }
                    assert_eq!(fields, vec![16, 17]);
                    has_client_info = true;
                }
                2 => has_po_token = true,
                3 => has_cookie = true,
                5 => has_context = true,
                _ => {}
            }
        }
        assert!(has_client_info && has_po_token && has_cookie && has_context);
    }

    #[test]
    fn browser_shape_drops_field_16_and_fills_client_info_like_the_web_player() {
        let payload = player_payload();
        let mut bootstrap = SabrBootstrap::from_player_payload(&payload, "abc", "1.2026").unwrap();
        bootstrap.po_token = None;
        let mut state = AbrRequestState::new(0);
        state.browser_shape = true;
        let request = build_abr_request(&bootstrap, &state);

        let mut reader = Reader::new(&request);
        let mut fields = Vec::new();
        let mut context = Vec::new();
        while let Some((field, value)) = reader.next_field().unwrap() {
            fields.push(field);
            if field == 19 {
                context = value.as_bytes().unwrap().to_vec();
            }
        }
        assert_eq!(fields, vec![1, 5, 19]);

        let mut context_reader = Reader::new(&context);
        let mut context_fields = Vec::new();
        let mut client_fields = Vec::new();
        while let Some((field, value)) = context_reader.next_field().unwrap() {
            context_fields.push(field);
            if field == 1 {
                let mut client = Reader::new(value.as_bytes().unwrap());
                while let Some((field, _)) = client.next_field().unwrap() {
                    client_fields.push(field);
                }
            }
        }
        assert_eq!(context_fields, vec![1]);
        assert_eq!(client_fields, vec![1, 16, 17, 18, 19]);
    }

    #[test]
    fn format_id_bytes_match_pinned_schema_defaults_and_utf8() {
        let mut id = FormatId {
            itag: 140,
            last_modified: 300,
            xtags: String::new(),
        };
        // Fields 1 and 2 only; absent/empty xtags has no field 3.
        assert_eq!(id.encode(), vec![0x08, 0x8c, 0x01, 0x10, 0xac, 0x02]);
        assert_eq!(FormatId::decode(&id.encode()).unwrap(), id);
        id.xtags = "é".into();
        assert_eq!(
            id.encode(),
            vec![0x08, 0x8c, 0x01, 0x10, 0xac, 0x02, 0x1a, 0x02, 0xc3, 0xa9]
        );
        assert_eq!(FormatId::decode(&id.encode()).unwrap(), id);
        assert!(FormatId::default().encode().is_empty());
    }

    #[test]
    fn initialization_ids_follow_confirmed_state_not_selection() {
        let bootstrap =
            SabrBootstrap::from_player_payload(&player_payload(), "synthetic", "1").unwrap();
        let mut state = AbrRequestState::new(0);
        for initialized in [false, true, false] {
            if initialized {
                state
                    .initialized_formats
                    .push(bootstrap.selected_audio().id.clone());
            } else {
                state.initialized_formats.clear();
            }
            let bytes = build_abr_request(&bootstrap, &state);
            let mut reader = Reader::new(&bytes);
            let mut init_ids = Vec::new();
            let mut selected_ids = Vec::new();
            while let Some((field, value)) = reader.next_field().unwrap() {
                match field {
                    2 => init_ids.push(FormatId::decode(value.as_bytes().unwrap()).unwrap()),
                    16 => selected_ids.push(FormatId::decode(value.as_bytes().unwrap()).unwrap()),
                    1 => {
                        let mut abr = Reader::new(value.as_bytes().unwrap());
                        while let Some((field, _)) = abr.next_field().unwrap() {
                            assert_ne!(field, 28, "zero player time is omitted");
                        }
                    }
                    17 | 18 => panic!("audio-only request must not select video/captions"),
                    _ => {}
                }
            }
            assert_eq!(init_ids, state.initialized_formats);
            assert_eq!(selected_ids, vec![bootstrap.selected_audio().id.clone()]);
        }
    }

    #[test]
    fn bootstrap_filters_unusable_last_modified_and_preserves_xtags() {
        for invalid in [
            serde_json::Value::Null,
            serde_json::json!("bad"),
            serde_json::json!(0),
            serde_json::json!("0"),
            serde_json::json!(-1),
            serde_json::json!(1.5),
            serde_json::json!("18446744073709551616"),
        ] {
            let mut payload = player_payload();
            for format in payload["streamingData"]["adaptiveFormats"]
                .as_array_mut()
                .unwrap()
            {
                format["lastModified"] = invalid.clone();
            }
            assert_eq!(
                SabrBootstrap::from_player_payload(&payload, "synthetic", "1").unwrap_err(),
                BootstrapError::MissingAudioFormats
            );
        }
        let mut payload = player_payload();
        payload["streamingData"]["adaptiveFormats"][1]
            .as_object_mut()
            .unwrap()
            .remove("lastModified");
        payload["streamingData"]["adaptiveFormats"][0]["lastModified"] = serde_json::json!(300);
        for xtags in [None, Some(""), Some("synthetic-tags")] {
            if let Some(value) = xtags {
                payload["streamingData"]["adaptiveFormats"][0]["xtags"] = serde_json::json!(value);
            }
            let bootstrap = SabrBootstrap::from_player_payload(&payload, "synthetic", "1").unwrap();
            assert_eq!(bootstrap.audio_formats.len(), 1);
            assert_eq!(bootstrap.selected_audio().id.last_modified, 300);
            assert_eq!(
                bootstrap.selected_audio().id.xtags,
                xtags.unwrap_or_default()
            );
        }
    }

    #[cfg(test)]
    fn push_ump_varint(out: &mut Vec<u8>, value: u32) {
        if value < 0x80 {
            out.push(value as u8);
        } else if value < 0x4000 {
            out.push(0x80 | (value & 0x3f) as u8);
            out.push((value >> 6) as u8);
        } else if value < 0x20_0000 {
            out.push(0xc0 | (value & 0x1f) as u8);
            out.push((value >> 5) as u8);
            out.push((value >> 13) as u8);
        } else if value < 0x1000_0000 {
            out.push(0xe0 | (value & 0x0f) as u8);
            out.push((value >> 4) as u8);
            out.push((value >> 12) as u8);
            out.push((value >> 20) as u8);
        } else {
            out.push(0xf0);
            out.extend_from_slice(&value.to_le_bytes());
        }
    }

    fn push_part(out: &mut Vec<u8>, part_type: u32, payload: &[u8]) {
        push_ump_varint(out, part_type);
        push_ump_varint(out, payload.len() as u32);
        out.extend_from_slice(payload);
    }

    #[test]
    fn parses_control_parts_and_segment_metadata() {
        let mut media_header = Writer::new();
        media_header.int32_field(1, 7);
        media_header.int32_field(3, 140);
        media_header.bool_field(8, false);
        media_header.int32_field(9, 2);
        media_header.int64_field(11, 10_000);
        media_header.int64_field(12, 160_000);
        media_header.int64_field(14, 150_000);

        let mut policy = Writer::new();
        policy.int32_field(1, 15_000);
        policy.int32_field(4, 250);
        let mut cookie = Writer::new();
        cookie.int32_field(8, 140);
        policy.bytes_field(7, &cookie.into_vec());

        let mut redirect = Writer::new();
        redirect.string_field(
            1,
            "https://rr2---sn.example.googlevideo.com/videoplayback?sabr=1",
        );

        let mut protection = Writer::new();
        protection.int32_field(1, 2);
        protection.int32_field(2, 3);

        let mut context_update = Writer::new();
        context_update.int32_field(1, 5);
        context_update.bytes_field(3, &[1, 2, 3]);
        context_update.bool_field(4, true);

        let mut sending = Writer::new();
        sending.int32_field(1, 5);
        sending.int32_field(2, 6);

        let mut body = Vec::new();
        push_part(&mut body, PART_MEDIA_HEADER, &media_header.into_vec());
        push_part(&mut body, PART_MEDIA, &[7, 1, 2, 3, 4]);
        push_part(&mut body, PART_NEXT_REQUEST_POLICY, &policy.into_vec());
        push_part(&mut body, PART_SABR_REDIRECT, &redirect.into_vec());
        push_part(
            &mut body,
            PART_STREAM_PROTECTION_STATUS,
            &protection.into_vec(),
        );
        push_part(
            &mut body,
            PART_SABR_CONTEXT_UPDATE,
            &context_update.into_vec(),
        );
        push_part(
            &mut body,
            PART_SABR_CONTEXT_SENDING_POLICY,
            &sending.into_vec(),
        );
        push_part(&mut body, PART_END_OF_TRACK, &[]);

        let control = parse_response_parts(&body).unwrap();
        assert_eq!(control.media_bytes, 5);
        assert_eq!(control.segments.len(), 1);
        let segment = &control.segments[0];
        assert_eq!(segment.header_id, 7);
        assert_eq!(segment.itag, 140);
        assert_eq!(segment.segment_num, 2);
        assert_eq!(segment.end_ms(), 170_000);
        assert_eq!(segment.media_bytes, 5);

        let policy = control.next_request_policy.as_ref().unwrap();
        assert_eq!(policy.target_audio_readahead_ms, Some(15_000));
        assert_eq!(policy.backoff_time_ms, Some(250));
        assert!(policy.playback_cookie.is_some());

        assert_eq!(
            control.redirect_url.as_deref(),
            Some("https://rr2---sn.example.googlevideo.com/videoplayback?sabr=1")
        );
        assert_eq!(control.stream_protection_status, Some(2));
        assert_eq!(control.stream_protection_max_retries, Some(3));
        assert_eq!(control.context_updates.len(), 1);
        assert_eq!(control.start_contexts, vec![5]);
        assert_eq!(control.stop_contexts, vec![6]);
        assert!(control.end_of_track);
        assert_eq!(control.buffered_until_ms(), Some(170_000));
    }

    #[test]
    fn ignores_redirects_outside_googlevideo() {
        let mut redirect = Writer::new();
        redirect.string_field(1, "https://attacker.example/videoplayback");
        let mut body = Vec::new();
        push_part(&mut body, PART_SABR_REDIRECT, &redirect.into_vec());
        let control = parse_response_parts(&body).unwrap();
        assert!(control.redirect_url.is_none());
    }

    #[test]
    fn tolerates_truncated_response_body() {
        let mut body = Vec::new();
        push_part(&mut body, PART_STREAM_PROTECTION_STATUS, &[0x08, 0x02]);
        body.extend_from_slice(&[20, 0x40, 0x01]); // truncated header
        let control = parse_response_parts(&body).unwrap();
        assert_eq!(control.stream_protection_status, Some(2));
    }
}
