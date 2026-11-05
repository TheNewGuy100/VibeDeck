//! Pure planning for the byte-range UMP mode: read the `sidx` index of a fragmented MP4 audio
//! format, map a playback time to a segment, and split what must be fetched into requests.
//! No network and no provider text. Seek is "init bytes, then segments from the target on";
//! the engine's UMP assembler already writes by byte range, so a gap after the init is fine.

use super::read_ump_varint;

const MAX_REFERENCES: usize = 20_000;
const MAX_RANGE_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Segment {
    pub start: u64,
    pub size: u64,
    pub start_ms: u64,
    pub dur_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SegmentIndex {
    pub segments: Vec<Segment>,
    /// Inclusive last byte of the initialization data plus the index box.
    pub header_end: u64,
    /// Content length: one past the last byte of the last segment.
    pub total: u64,
}

fn be32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(at..at + 4)?.try_into().ok()?))
}
fn be64(b: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_be_bytes(b.get(at..at + 8)?.try_into().ok()?))
}

/// `bytes` starts at the `sidx` box, whose first byte sits at file offset `box_offset`.
pub fn parse_sidx(bytes: &[u8], box_offset: u64) -> Result<SegmentIndex, &'static str> {
    let size = be32(bytes, 0).ok_or("sidx_short")? as u64;
    if bytes.get(4..8) != Some(b"sidx") || size < 32 || size as usize > bytes.len() {
        return Err("sidx_not_found");
    }
    let version = bytes[8];
    let timescale = be32(bytes, 16).ok_or("sidx_short")? as u64;
    if timescale == 0 {
        return Err("sidx_timescale_zero");
    }
    let (earliest, first_offset, mut at) = match version {
        0 => (be32(bytes, 20).ok_or("sidx_short")? as u64, be32(bytes, 24).ok_or("sidx_short")? as u64, 28),
        1 => (be64(bytes, 20).ok_or("sidx_short")?, be64(bytes, 28).ok_or("sidx_short")?, 36),
        _ => return Err("sidx_version_unsupported"),
    };
    let count = u16::from_be_bytes(bytes.get(at + 2..at + 4).ok_or("sidx_short")?.try_into().unwrap()) as usize;
    at += 4;
    if count == 0 || count > MAX_REFERENCES || at + count * 12 > size as usize {
        return Err("sidx_reference_count_rejected");
    }
    let box_end = box_offset + size;
    let mut start = box_end + first_offset;
    let mut ticks = earliest;
    let mut segments = Vec::with_capacity(count);
    for i in 0..count {
        let word = be32(bytes, at + i * 12).ok_or("sidx_short")?;
        if word >> 31 == 1 {
            return Err("sidx_hierarchical_unsupported");
        }
        let referenced = u64::from(word & 0x7fff_ffff);
        let duration = u64::from(be32(bytes, at + i * 12 + 4).ok_or("sidx_short")?);
        if referenced == 0 {
            return Err("sidx_empty_reference");
        }
        segments.push(Segment {
            start,
            size: referenced,
            start_ms: ticks * 1000 / timescale,
            dur_ms: duration * 1000 / timescale,
        });
        start += referenced;
        ticks += duration;
    }
    Ok(SegmentIndex { segments, header_end: box_end.saturating_sub(1), total: start })
}

/// Index of the segment that contains `ms` (the last one when `ms` is past the end).
pub fn segment_at(index: &SegmentIndex, ms: u64) -> usize {
    index
        .segments
        .iter()
        .rposition(|s| s.start_ms <= ms)
        .unwrap_or(0)
}

/// Inclusive byte ranges to fetch, in order. `with_header` adds `0..=header_end` first (needed at
/// the start and after a seek). The body is split into chunks of at most `chunk` bytes; `max_ranges`
/// bounds how many are planned now so the caller can ask again as playback advances.
pub fn plan_ranges(
    index: &SegmentIndex,
    from_segment: usize,
    with_header: bool,
    chunk: u64,
    max_ranges: usize,
) -> Vec<(u64, u64)> {
    let chunk = chunk.clamp(16 * 1024, MAX_RANGE_BYTES);
    let mut out = Vec::new();
    if with_header {
        out.push((0, index.header_end));
    }
    let Some(first) = index.segments.get(from_segment) else { return out };
    let mut at = if with_header && from_segment == 0 { index.header_end + 1 } else { first.start };
    while at < index.total && out.len() < max_ranges {
        let end = (at + chunk - 1).min(index.total - 1);
        out.push((at, end));
        at = end + 1;
    }
    out
}

/// Concatenated payload of media parts (id 21) of one UMP response, header-id byte removed.
pub fn ump_media_bytes(body: &[u8]) -> Vec<u8> {
    let (mut out, mut offset) = (Vec::new(), 0usize);
    while let Ok(Some((id, after_id))) = read_ump_varint(body, offset) {
        let Ok(Some((len, start))) = read_ump_varint(body, after_id) else { break };
        let end = start.saturating_add(len as usize);
        if end > body.len() {
            break;
        }
        if id == 21 && len > 0 {
            out.extend_from_slice(&body[start + 1..end]);
        }
        offset = end;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A v0 `sidx` with the given (size, duration) references, timescale 1000.
    fn sidx(refs: &[(u32, u32)], first_offset: u32) -> Vec<u8> {
        let size = 32 + refs.len() * 12;
        let mut b = Vec::new();
        b.extend_from_slice(&(size as u32).to_be_bytes());
        b.extend_from_slice(b"sidx");
        b.extend_from_slice(&[0, 0, 0, 0]);
        b.extend_from_slice(&1u32.to_be_bytes());
        b.extend_from_slice(&1000u32.to_be_bytes());
        b.extend_from_slice(&0u32.to_be_bytes());
        b.extend_from_slice(&first_offset.to_be_bytes());
        b.extend_from_slice(&0u16.to_be_bytes());
        b.extend_from_slice(&(refs.len() as u16).to_be_bytes());
        for (s, d) in refs {
            b.extend_from_slice(&s.to_be_bytes());
            b.extend_from_slice(&d.to_be_bytes());
            b.extend_from_slice(&0x9000_0000u32.to_be_bytes());
        }
        b
    }

    fn index() -> SegmentIndex {
        // init+index occupy bytes 0..=811 (sidx box of 68 bytes at offset 744); media follows.
        parse_sidx(&sidx(&[(1000, 5000), (2000, 5000), (500, 2500)], 0), 744).unwrap()
    }

    #[test]
    fn sidx_gives_contiguous_segments_with_times() {
        let i = index();
        assert_eq!(i.header_end, 811);
        assert_eq!(i.segments[0], Segment { start: 812, size: 1000, start_ms: 0, dur_ms: 5000 });
        assert_eq!(i.segments[1], Segment { start: 1812, size: 2000, start_ms: 5000, dur_ms: 5000 });
        assert_eq!(i.segments[2].start_ms, 10_000);
        assert_eq!(i.total, 4312);
    }

    #[test]
    fn broken_or_hostile_boxes_are_refused() {
        assert_eq!(parse_sidx(&[0; 8], 0).unwrap_err(), "sidx_not_found");
        let mut hier = sidx(&[(100, 1000)], 0);
        hier[32] |= 0x80;
        assert_eq!(parse_sidx(&hier, 0).unwrap_err(), "sidx_hierarchical_unsupported");
        let mut zero_scale = sidx(&[(100, 1000)], 0);
        zero_scale[16..20].copy_from_slice(&0u32.to_be_bytes());
        assert_eq!(parse_sidx(&zero_scale, 0).unwrap_err(), "sidx_timescale_zero");
        assert_eq!(parse_sidx(&sidx(&[(0, 1000)], 0), 0).unwrap_err(), "sidx_empty_reference");
    }

    #[test]
    fn time_maps_to_the_containing_segment() {
        let i = index();
        assert_eq!(segment_at(&i, 0), 0);
        assert_eq!(segment_at(&i, 4999), 0);
        assert_eq!(segment_at(&i, 5000), 1);
        assert_eq!(segment_at(&i, 999_999), 2);
    }

    #[test]
    fn start_plan_begins_after_the_header_and_covers_the_whole_file() {
        let i = index();
        let plan = plan_ranges(&i, 0, true, 16 * 1024, 100);
        assert_eq!(plan, vec![(0, 811), (812, 4311)]);
        let small = plan_ranges(&i, 0, true, 16 * 1024, 1);
        assert_eq!(small, vec![(0, 811)]);
    }

    #[test]
    fn seek_plan_is_header_then_target_segment_onwards_without_a_gap_in_ranges() {
        let i = index();
        let plan = plan_ranges(&i, 1, true, 16 * 1024, 100);
        assert_eq!(plan, vec![(0, 811), (1812, 4311)]);
        let last = plan_ranges(&i, 2, false, 16 * 1024, 100);
        assert_eq!(last, vec![(3812, 4311)]);
        assert!(plan_ranges(&i, 9, false, 16 * 1024, 100).is_empty());
    }

    #[test]
    fn large_files_are_split_into_bounded_chunks() {
        let big = SegmentIndex {
            segments: vec![Segment { start: 100, size: 1_000_000, start_ms: 0, dur_ms: 60_000 }],
            header_end: 99,
            total: 1_000_100,
        };
        let plan = plan_ranges(&big, 0, true, 256 * 1024, 100);
        assert_eq!(plan.first(), Some(&(0, 99)));
        assert!(plan[1..].iter().all(|(a, b)| b - a < 256 * 1024));
        assert_eq!(plan.last().unwrap().1, 1_000_099);
        assert!(plan.windows(2).all(|w| w[1].0 == w[0].1 + 1));
    }

    #[test]
    fn media_bytes_are_extracted_from_ump_without_the_header_id_byte() {
        // part 20 (len 1), part 21 (len 4: id byte + 3 media bytes), part 21 (len 2: id + 1)
        let body = [20u8, 1, 0, 21, 4, 0, 1, 2, 3, 21, 2, 0, 9];
        assert_eq!(ump_media_bytes(&body), vec![1, 2, 3, 9]);
        assert!(ump_media_bytes(&[21, 200]).is_empty());
    }
}
