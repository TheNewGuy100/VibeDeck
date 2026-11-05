//! Minimal protobuf wire-format support for the internal SABR protocol.
//!
//! The SABR messages are small and only use a handful of field types, so the
//! client encodes and decodes them directly instead of generating bindings.
//! Field numbers come from third-party reverse-engineered schemas and are not a
//! stable public contract.

/// Protobuf wire types used by the SABR messages.
pub const WIRE_VARINT: u32 = 0;
pub const WIRE_FIXED64: u32 = 1;
pub const WIRE_LEN: u32 = 2;
pub const WIRE_FIXED32: u32 = 5;

/// Appends protobuf fields to a byte buffer.
#[derive(Debug, Default)]
pub struct Writer {
    bytes: Vec<u8>,
}

impl Writer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn into_vec(self) -> Vec<u8> {
        self.bytes
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.bytes
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    pub fn varint(&mut self, mut value: u64) {
        while value >= 0x80 {
            self.bytes.push((value as u8) | 0x80);
            value >>= 7;
        }
        self.bytes.push(value as u8);
    }

    pub fn tag(&mut self, field: u32, wire: u32) {
        self.varint(u64::from(field) << 3 | u64::from(wire));
    }

    pub fn uint64_field(&mut self, field: u32, value: u64) {
        if value == 0 {
            return;
        }
        self.tag(field, WIRE_VARINT);
        self.varint(value);
    }

    /// Signed varints are encoded as their two's complement value, so negative
    /// numbers occupy ten bytes.
    pub fn int64_field(&mut self, field: u32, value: i64) {
        if value == 0 {
            return;
        }
        self.tag(field, WIRE_VARINT);
        self.varint(value as u64);
    }

    pub fn int32_field(&mut self, field: u32, value: i32) {
        if value == 0 {
            return;
        }
        self.tag(field, WIRE_VARINT);
        self.varint(value as i64 as u64);
    }

    pub fn bool_field(&mut self, field: u32, value: bool) {
        if !value {
            return;
        }
        self.tag(field, WIRE_VARINT);
        self.varint(1);
    }

    pub fn float_field(&mut self, field: u32, value: f32) {
        if value == 0.0 {
            return;
        }
        self.tag(field, WIRE_FIXED32);
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    pub fn bytes_field(&mut self, field: u32, value: &[u8]) {
        if value.is_empty() {
            return;
        }
        self.tag(field, WIRE_LEN);
        self.varint(value.len() as u64);
        self.bytes.extend_from_slice(value);
    }

    pub fn string_field(&mut self, field: u32, value: &str) {
        if value.is_empty() {
            return;
        }
        self.bytes_field(field, value.as_bytes());
    }

    pub fn message_field(&mut self, field: u32, message: &Writer) {
        if message.is_empty() {
            return;
        }
        self.bytes_field(field, message.as_slice());
    }
}

/// One decoded protobuf field.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Field<'a> {
    Varint(u64),
    Fixed64(u64),
    Bytes(&'a [u8]),
    Fixed32(u32),
}

impl<'a> Field<'a> {
    pub fn as_u64(self) -> Option<u64> {
        match self {
            Field::Varint(value) | Field::Fixed64(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_i64(self) -> Option<i64> {
        self.as_u64().map(|value| value as i64)
    }

    pub fn as_bool(self) -> Option<bool> {
        self.as_u64().map(|value| value != 0)
    }

    pub fn as_bytes(self) -> Option<&'a [u8]> {
        match self {
            Field::Bytes(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_str(self) -> Option<&'a str> {
        self.as_bytes()
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
    }
}

/// Sequential protobuf field reader. Unknown fields are exposed to the caller
/// so each message can decide whether it understands them.
#[derive(Debug, Clone)]
pub struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    pub fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.position)
    }

    fn read_varint(&mut self) -> Result<u64, String> {
        let (value, consumed) = decode_varint(self.bytes.get(self.position..).unwrap_or_default())
            .ok_or_else(|| "Truncated protobuf varint".to_string())?;
        self.position += consumed;
        Ok(value)
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], String> {
        let end = self
            .position
            .checked_add(length)
            .ok_or_else(|| "Invalid protobuf length".to_string())?;
        let slice = self
            .bytes
            .get(self.position..end)
            .ok_or_else(|| "Truncated protobuf field".to_string())?;
        self.position = end;
        Ok(slice)
    }

    /// Returns the next `(field_number, value)` pair, or `None` at the end of
    /// the message.
    pub fn next_field(&mut self) -> Result<Option<(u32, Field<'a>)>, String> {
        if self.position >= self.bytes.len() {
            return Ok(None);
        }
        let key = self.read_varint()?;
        let key = u32::try_from(key).map_err(|_| "Invalid protobuf field key".to_string())?;
        let field = key >> 3;
        if field == 0 {
            return Err("Invalid protobuf field number".to_string());
        }
        let wire = key & 0x07;
        let value = match wire {
            WIRE_VARINT => Field::Varint(self.read_varint()?),
            WIRE_FIXED64 => {
                let bytes = self.take(8)?;
                Field::Fixed64(u64::from_le_bytes(
                    bytes
                        .try_into()
                        .map_err(|_| "Invalid fixed64".to_string())?,
                ))
            }
            WIRE_LEN => {
                let length = self.read_varint()?;
                let length =
                    usize::try_from(length).map_err(|_| "Invalid protobuf length".to_string())?;
                Field::Bytes(self.take(length)?)
            }
            WIRE_FIXED32 => {
                let bytes = self.take(4)?;
                Field::Fixed32(u32::from_le_bytes(
                    bytes
                        .try_into()
                        .map_err(|_| "Invalid fixed32".to_string())?,
                ))
            }
            _ => return Err("Unsupported protobuf wire type".to_string()),
        };
        Ok(Some((field, value)))
    }
}

/// Decodes one base-128 varint and returns `(value, bytes_consumed)`.
pub fn decode_varint(bytes: &[u8]) -> Option<(u64, usize)> {
    let mut value = 0u64;
    for (index, byte) in bytes.iter().take(10).enumerate() {
        if index == 9 && *byte > 1 {
            return None;
        }
        value |= u64::from(byte & 0x7f) << (index * 7);
        if byte & 0x80 == 0 {
            return Some((value, index + 1));
        }
    }
    None
}

/// Decodes a standard alphabet base64 string. Returns `None` for malformed
/// input; callers treat that as a missing field instead of failing playback.
pub fn decode_base64(input: &str) -> Option<Vec<u8>> {
    fn value(byte: u8) -> Option<u32> {
        match byte {
            b'A'..=b'Z' => Some(u32::from(byte - b'A')),
            b'a'..=b'z' => Some(u32::from(byte - b'a') + 26),
            b'0'..=b'9' => Some(u32::from(byte - b'0') + 52),
            b'+' | b'-' => Some(62),
            b'/' | b'_' => Some(63),
            _ => None,
        }
    }

    let mut output = Vec::with_capacity(input.len() / 4 * 3);
    let mut buffer = 0u32;
    let mut bits = 0u32;
    for byte in input.bytes() {
        if byte == b'=' || byte.is_ascii_whitespace() {
            continue;
        }
        let digit = value(byte)?;
        buffer = (buffer << 6) | digit;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            output.push((buffer >> bits) as u8);
            buffer &= (1 << bits) - 1;
        }
    }
    Some(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_link_delimited_and_varint_fields() {
        let mut inner = Writer::new();
        inner.int32_field(1, 140);
        inner.uint64_field(2, 9_000);

        let mut outer = Writer::new();
        outer.message_field(16, &inner);

        let mut reader = Reader::new(outer.as_slice());
        let (field, value) = reader.next_field().unwrap().unwrap();
        assert_eq!(field, 16);
        let nested = value.as_bytes().unwrap();

        let mut nested_reader = Reader::new(nested);
        assert_eq!(
            nested_reader.next_field().unwrap(),
            Some((1, Field::Varint(140)))
        );
        assert_eq!(
            nested_reader.next_field().unwrap(),
            Some((2, Field::Varint(9_000)))
        );
        assert!(nested_reader.next_field().unwrap().is_none());
    }

    #[test]
    fn decodes_standard_and_url_safe_base64() {
        assert_eq!(decode_base64("aGVsbG8=").unwrap(), b"hello");
        assert_eq!(decode_base64("aGVsbG8").unwrap(), b"hello");
        assert_eq!(decode_base64("--__").unwrap().len(), 3);
        assert!(decode_base64("!!!!").is_none());
    }

    #[test]
    fn rejects_truncated_fields() {
        let bytes = [0x0a, 0x05, 0x01, 0x02];
        let mut reader = Reader::new(&bytes);
        assert!(reader.next_field().is_err());
    }
}
