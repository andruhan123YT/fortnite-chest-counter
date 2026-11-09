//! Little-endian bit stream reader with Unreal `FBitReader` semantics.
//!
//! Reads never panic. Running past the active limit sets a sticky error flag and yields
//! zeroes, which is how the engine itself reports a desynchronized stream.

use crate::names;

/// Versions from the replay header that change the wire format.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NetVersions {
    pub network: u32,
    pub engine_network: u32,
    pub flags: u32,
}

impl NetVersions {
    pub fn has_level_streaming_fixes(&self) -> bool {
        self.flags & 2 != 0
    }

    pub fn has_game_specific_frame_data(&self) -> bool {
        self.flags & 8 != 0
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vec3 {
    pub const ZERO: Vec3 = Vec3 { x: 0.0, y: 0.0, z: 0.0 };

    pub fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    pub fn scaled(self, factor: f64) -> Self {
        Self::new(self.x * factor, self.y * factor, self.z * factor)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Rotator {
    pub pitch: f64,
    pub yaw: f64,
    pub roll: f64,
}

/// A nested read window. Must be closed with [`BitReader::pop_limit`].
#[must_use = "a pushed limit has to be popped"]
#[derive(Debug)]
pub struct Limit(usize);

#[derive(Debug, Clone, Copy)]
struct Scope {
    outer_end: usize,
    end: usize,
}

pub struct BitReader<'a> {
    data: &'a [u8],
    pos: usize,
    end: usize,
    scopes: Vec<Scope>,
    error: bool,
    pub versions: NetVersions,
}

impl<'a> BitReader<'a> {
    pub fn new(data: &'a [u8], versions: NetVersions) -> Self {
        Self::with_bits(data, data.len() * 8, versions)
    }

    pub fn with_bits(data: &'a [u8], bits: usize, versions: NetVersions) -> Self {
        Self { data, pos: 0, end: bits.min(data.len() * 8), scopes: Vec::new(), error: false, versions }
    }

    pub fn data(&self) -> &'a [u8] {
        self.data
    }

    pub fn pos(&self) -> usize {
        self.pos
    }

    pub fn set_pos(&mut self, pos: usize) {
        self.pos = pos.min(self.end);
    }

    pub fn end(&self) -> usize {
        self.end
    }

    pub fn at_end(&self) -> bool {
        self.pos >= self.end
    }

    pub fn bits_left(&self) -> usize {
        self.end.saturating_sub(self.pos)
    }

    pub fn can_read(&self, bits: usize) -> bool {
        self.pos.checked_add(bits).is_some_and(|p| p <= self.end)
    }

    pub fn is_error(&self) -> bool {
        self.error
    }

    pub fn set_error(&mut self) {
        self.error = true;
    }

    pub fn clear_error(&mut self) {
        self.error = false;
    }

    /// Restricts reads to the next `bits` bits. Returns `None` if the window would
    /// overrun the current one.
    pub fn push_limit(&mut self, bits: usize) -> Option<Limit> {
        if !self.can_read(bits) {
            return None;
        }
        self.scopes.push(Scope { outer_end: self.end, end: self.pos + bits });
        self.end = self.pos + bits;
        Some(Limit(self.scopes.len() - 1))
    }

    /// Skips whatever is left of the window, restores the enclosing one and returns
    /// whether the window was left in an error state. Any windows opened inside it and
    /// never closed are discarded too.
    pub fn pop_limit(&mut self, limit: Limit) -> bool {
        let Some(scope) = self.scopes.get(limit.0).copied() else {
            return true;
        };
        self.scopes.truncate(limit.0);
        self.pos = scope.end;
        self.end = scope.outer_end;
        std::mem::take(&mut self.error)
    }

    fn fail<T: Default>(&mut self) -> T {
        self.error = true;
        self.pos = self.end;
        T::default()
    }

    /// Reads up to 64 bits starting at an arbitrary bit position. The caller has
    /// already checked the bounds.
    fn peek(&self, count: u32) -> u64 {
        debug_assert!(count <= 64);
        if count == 0 {
            return 0;
        }
        let byte = self.pos >> 3;
        let shift = (self.pos & 7) as u32;
        let mut buf = [0u8; 9];
        let avail = (self.data.len() - byte).min(9);
        buf[..avail].copy_from_slice(&self.data[byte..byte + avail]);
        let lo = u64::from_le_bytes([buf[0], buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7]]);
        let mut value = lo >> shift;
        if shift > 0 {
            value |= u64::from(buf[8]) << (64 - shift);
        }
        if count == 64 {
            value
        } else {
            value & ((1u64 << count) - 1)
        }
    }

    pub fn read_bit(&mut self) -> bool {
        if self.pos >= self.end {
            self.error = true;
            return false;
        }
        let bit = (self.data[self.pos >> 3] >> (self.pos & 7)) & 1 == 1;
        self.pos += 1;
        bit
    }

    pub fn read_bits_u64(&mut self, count: u32) -> u64 {
        if count > 64 || !self.can_read(count as usize) {
            return self.fail();
        }
        let value = self.peek(count);
        self.pos += count as usize;
        value
    }

    pub fn read_bits_u32(&mut self, count: u32) -> u32 {
        if count > 32 {
            return self.fail();
        }
        self.read_bits_u64(count) as u32
    }

    pub fn skip_bits(&mut self, count: usize) {
        if !self.can_read(count) {
            self.fail::<()>();
            return;
        }
        self.pos += count;
    }

    pub fn skip_bytes(&mut self, count: usize) {
        self.skip_bits(count.saturating_mul(8));
    }

    /// `FArchive::SerializeInt`: a value in `[0, max)` stored in as few bits as needed.
    pub fn read_serialized_int(&mut self, max: u32) -> u32 {
        let mut value: u32 = 0;
        let mut mask: u32 = 1;
        while mask != 0 && u64::from(value) + u64::from(mask) < u64::from(max) {
            if self.read_bit() {
                value |= mask;
            }
            if self.error {
                return 0;
            }
            mask <<= 1;
        }
        value
    }

    /// `FArchive::SerializeIntPacked`: 7 bits per byte, low bit flags continuation.
    pub fn read_int_packed(&mut self) -> u32 {
        let mut value: u32 = 0;
        for index in 0..5 {
            let byte = self.read_u8();
            if self.error {
                return 0;
            }
            value = value.wrapping_add(u32::from(byte >> 1).wrapping_shl(7 * index));
            if byte & 1 == 0 {
                break;
            }
        }
        value
    }

    pub fn read_u8(&mut self) -> u8 {
        self.read_bits_u64(8) as u8
    }

    pub fn read_u16(&mut self) -> u16 {
        self.read_bits_u64(16) as u16
    }

    pub fn read_u32(&mut self) -> u32 {
        self.read_bits_u64(32) as u32
    }

    pub fn read_i32(&mut self) -> i32 {
        self.read_u32() as i32
    }

    pub fn read_u64(&mut self) -> u64 {
        self.read_bits_u64(64)
    }

    pub fn read_f32(&mut self) -> f32 {
        f32::from_bits(self.read_u32())
    }

    pub fn read_f64(&mut self) -> f64 {
        f64::from_bits(self.read_u64())
    }

    /// A 32-bit `UBOOL` as written by `FArchive << bool`.
    pub fn read_bool32(&mut self) -> bool {
        self.read_u32() & 1 == 1
    }

    pub fn read_bytes(&mut self, count: usize) -> Vec<u8> {
        if !self.can_read(count.saturating_mul(8)) {
            return self.fail();
        }
        if self.pos & 7 == 0 {
            let start = self.pos >> 3;
            self.pos += count * 8;
            return self.data[start..start + count].to_vec();
        }
        (0..count).map(|_| self.read_u8()).collect()
    }

    /// Copies `count` bits into a fresh, byte-aligned buffer.
    pub fn read_bits_to_vec(&mut self, count: usize) -> Vec<u8> {
        if !self.can_read(count) {
            return self.fail();
        }
        let mut out = vec![0u8; count.div_ceil(8)];
        let mut written = 0;
        while written < count {
            let chunk = (count - written).min(64) as u32;
            let value = self.peek(chunk);
            self.pos += chunk as usize;
            let bytes = value.to_le_bytes();
            let start = written / 8;
            let len = (chunk as usize).div_ceil(8);
            out[start..start + len].copy_from_slice(&bytes[..len]);
            written += chunk as usize;
        }
        out
    }

    /// `FString` serialization: signed length, negative for UTF-16, null-terminated.
    pub fn read_string(&mut self) -> String {
        let len = self.read_i32();
        if self.error || len == 0 {
            return String::new();
        }
        if len < 0 {
            let units = len.unsigned_abs() as usize;
            if !self.can_read(units.saturating_mul(16)) {
                return self.fail();
            }
            let bytes = self.read_bytes(units * 2);
            let mut chars: Vec<u16> = bytes.as_chunks::<2>().0.iter().map(|&c| u16::from_le_bytes(c)).collect();
            chars.pop();
            return String::from_utf16_lossy(&chars).trim().to_string();
        }
        let len = len as usize;
        if !self.can_read(len.saturating_mul(8)) {
            return self.fail();
        }
        let mut bytes = self.read_bytes(len);
        bytes.pop();
        String::from_utf8_lossy(&bytes).into_owned()
    }

    pub fn read_fname(&mut self) -> String {
        let hardcoded = self.read_bit();
        self.read_fname_body(hardcoded)
    }

    /// Same as [`Self::read_fname`] but with the "hardcoded" flag stored as a full byte.
    pub fn read_fname_byte(&mut self) -> String {
        let hardcoded = self.read_u8() != 0;
        self.read_fname_body(hardcoded)
    }

    fn read_fname_body(&mut self, hardcoded: bool) -> String {
        if hardcoded {
            let index = if self.versions.engine_network < 6 { self.read_u32() } else { self.read_int_packed() };
            return names::hardcoded_name(index).unwrap_or_default().to_string();
        }
        let name = self.read_string();
        self.skip_bytes(4);
        name
    }

    pub fn read_vector3d(&mut self) -> Vec3 {
        if self.versions.engine_network < 23 {
            Vec3::new(f64::from(self.read_f32()), f64::from(self.read_f32()), f64::from(self.read_f32()))
        } else {
            Vec3::new(self.read_f64(), self.read_f64(), self.read_f64())
        }
    }

    pub fn read_packed_vector(&mut self, scale: f64, max_bits: u32) -> Vec3 {
        if self.versions.engine_network >= 23 {
            self.read_quantized_vector(scale)
        } else {
            self.read_packed_vector_legacy(scale, max_bits)
        }
    }

    /// UE5 `WritePackedVector`: a 7-bit header holding the per-component bit count and
    /// whether the value was pre-multiplied by `scale`. Zero bits means raw floats.
    fn read_quantized_vector(&mut self, scale: f64) -> Vec3 {
        let header = self.read_serialized_int(1 << 7);
        if self.error {
            return Vec3::ZERO;
        }
        let component_bits = header & 63;
        let scaled = header >> 6 != 0;

        if component_bits > 0 {
            let mut component = || {
                let raw = self.read_bits_u64(component_bits);
                let sign = 1u64 << (component_bits - 1);
                let value = (raw ^ sign).wrapping_sub(sign) as i64 as f64;
                if scaled {
                    value / scale
                } else {
                    value
                }
            };
            let v = Vec3::new(component(), component(), component());
            return if self.error { Vec3::ZERO } else { v };
        }

        if scaled {
            Vec3::new(self.read_f64(), self.read_f64(), self.read_f64())
        } else {
            Vec3::new(f64::from(self.read_f32()), f64::from(self.read_f32()), f64::from(self.read_f32()))
        }
    }

    fn read_packed_vector_legacy(&mut self, scale: f64, max_bits: u32) -> Vec3 {
        let bits = self.read_serialized_int(max_bits);
        if self.error || bits > 29 {
            return Vec3::ZERO;
        }
        let bias = 1i64 << (bits + 1);
        let max = 1u32 << (bits + 2);
        let dx = i64::from(self.read_serialized_int(max));
        let dy = i64::from(self.read_serialized_int(max));
        let dz = i64::from(self.read_serialized_int(max));
        if self.error {
            return Vec3::ZERO;
        }
        Vec3::new((dx - bias) as f64 / scale, (dy - bias) as f64 / scale, (dz - bias) as f64 / scale)
    }

    pub fn read_rotation(&mut self) -> Rotator {
        self.read_rotator(8)
    }

    pub fn read_rotation_short(&mut self) -> Rotator {
        self.read_rotator(16)
    }

    fn read_rotator(&mut self, bits: u32) -> Rotator {
        let steps = f64::from(1u32 << bits);
        let axis = |r: &mut Self| if r.read_bit() { f64::from(r.read_bits_u32(bits)) * 360.0 / steps } else { 0.0 };
        let pitch = axis(self);
        let yaw = axis(self);
        let roll = axis(self);
        if self.error {
            return Rotator::default();
        }
        Rotator { pitch, yaw, roll }
    }
}

/// An owned, growable bit buffer used to reassemble partial bunches.
#[derive(Debug, Clone, Default)]
pub struct BitBuf {
    pub data: Vec<u8>,
    pub bits: usize,
}

impl BitBuf {
    pub fn from_bytes(data: Vec<u8>, bits: usize) -> Self {
        Self { data, bits }
    }

    pub fn append(&mut self, src: &[u8], count: usize) {
        let count = count.min(src.len() * 8);
        if self.bits.is_multiple_of(8) {
            self.data.truncate(self.bits / 8);
            self.data.extend_from_slice(&src[..count.div_ceil(8)]);
        } else {
            self.data.resize((self.bits + count).div_ceil(8), 0);
            for i in 0..count {
                let bit = (src[i >> 3] >> (i & 7)) & 1;
                let at = self.bits + i;
                if bit == 1 {
                    self.data[at >> 3] |= 1 << (at & 7);
                } else {
                    self.data[at >> 3] &= !(1 << (at & 7));
                }
            }
        }
        self.bits += count;
    }

    pub fn reader(&self, versions: NetVersions) -> BitReader<'_> {
        BitReader::with_bits(&self.data, self.bits, versions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reader(data: &[u8]) -> BitReader<'_> {
        BitReader::new(data, NetVersions { engine_network: 45, network: 19, flags: 0 })
    }

    #[test]
    fn reads_unaligned_bits() {
        let data = [0b1010_1101u8, 0b0000_0001];
        let mut r = reader(&data);
        assert!(r.read_bit());
        assert_eq!(r.read_bits_u32(3), 0b110);
        assert_eq!(r.read_bits_u32(5), 0b11010);
        assert_eq!(r.bits_left(), 7);
    }

    #[test]
    fn overrun_sets_error_and_returns_zero() {
        let data = [0xFFu8];
        let mut r = reader(&data);
        assert_eq!(r.read_u16(), 0);
        assert!(r.is_error());
        assert!(r.at_end());
    }

    #[test]
    fn int_packed_round_trip() {
        // 300 = 0b1_0010_1100 -> low 7 bits 0x2C with continuation, then 0x02.
        let data = [(0x2C << 1) | 1, 0x02 << 1];
        let mut r = reader(&data);
        assert_eq!(r.read_int_packed(), 300);
        assert!(!r.is_error());
    }

    #[test]
    fn serialized_int_uses_minimal_bits() {
        let data = [0b0000_0101u8];
        let mut r = reader(&data);
        assert_eq!(r.read_serialized_int(8), 5);
        assert_eq!(r.pos(), 3);
    }

    #[test]
    fn limits_nest_and_skip_remainder() {
        let data = [0u8; 4];
        let mut r = reader(&data);
        let outer = r.push_limit(20).unwrap();
        r.read_bits_u32(4);
        let inner = r.push_limit(8).unwrap();
        assert_eq!(r.bits_left(), 8);
        assert!(r.push_limit(9).is_none());
        assert!(!r.pop_limit(inner));
        assert_eq!(r.pos(), 12);
        assert_eq!(r.bits_left(), 8);
        r.read_u16();
        assert!(r.pop_limit(outer));
        assert_eq!(r.pos(), 20);
        assert_eq!(r.bits_left(), 12);
    }

    #[test]
    fn strings_ascii_and_utf16() {
        let mut data = vec![];
        data.extend_from_slice(&4i32.to_le_bytes());
        data.extend_from_slice(b"abc\0");
        data.extend_from_slice(&(-3i32).to_le_bytes());
        for c in "\u{42F}z\0".encode_utf16() {
            data.extend_from_slice(&c.to_le_bytes());
        }
        let mut r = reader(&data);
        assert_eq!(r.read_string(), "abc");
        assert_eq!(r.read_string(), "\u{42F}z");
        assert!(r.at_end());
    }

    #[test]
    fn quantized_vector_sign_extends() {
        // header: 6 bits count = 4, scaled flag = 1 -> 0b1_000100, 7 bits.
        // components: -1 (0b1111), 2 (0b0010), -8 (0b1000).
        let mut bits: Vec<bool> = Vec::new();
        let header = 0b100_0100u32;
        for i in 0..7 {
            bits.push(header >> i & 1 == 1);
        }
        for v in [0b1111u32, 0b0010, 0b1000] {
            for i in 0..4 {
                bits.push(v >> i & 1 == 1);
            }
        }
        let mut data = vec![0u8; bits.len().div_ceil(8)];
        for (i, b) in bits.iter().enumerate() {
            if *b {
                data[i / 8] |= 1 << (i % 8);
            }
        }
        let mut r = reader(&data);
        let v = r.read_packed_vector(10.0, 0);
        assert_eq!(v, Vec3::new(-0.1, 0.2, -0.8));
    }

    #[test]
    fn bitbuf_appends_unaligned() {
        let mut buf = BitBuf::from_bytes(vec![0b0000_0101], 3);
        buf.append(&[0b0000_0011], 2);
        assert_eq!(buf.bits, 5);
        assert_eq!(buf.data[0], 0b0001_1101);
        let mut r = buf.reader(NetVersions::default());
        assert_eq!(r.read_bits_u32(5), 0b11101);
    }

    #[test]
    fn copies_bits_to_vec() {
        let data = [0xABu8, 0xCD, 0xEF];
        let mut r = reader(&data);
        r.read_bits_u32(4);
        let out = r.read_bits_to_vec(12);
        assert_eq!(out, vec![0xDA, 0x0C]);
    }
}
