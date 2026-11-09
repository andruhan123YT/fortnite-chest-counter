//! The `.replay` file container: file info, chunk table, encryption and compression.

use aes::cipher::{generic_array::GenericArray, BlockDecrypt, KeyInit};
use aes::Aes256;

use crate::bits::{BitReader, NetVersions};
use crate::{Error, Result};

const FILE_MAGIC: u32 = 0x1CA2_E27F;
const HEADER_MAGIC: u32 = 0x2CF5_A13D;
/// .NET ticks (100 ns since 0001-01-01) at the Unix epoch.
const UNIX_EPOCH_TICKS: u64 = 621_355_968_000_000_000;

/// The unencrypted file preamble. Cheap to read and enough to tell whether a match
/// has finished: the encryption key is only written once the player leaves the match.
#[derive(Debug, Clone)]
pub struct ReplayInfo {
    pub file_version: u32,
    pub length_ms: u32,
    pub network_version: u32,
    pub changelist: u32,
    pub friendly_name: String,
    pub is_live: bool,
    /// Milliseconds since the Unix epoch. Fortnite writes the PC's local wall-clock time
    /// here as if it were UTC, so treat it as a naive local timestamp.
    pub timestamp_ms: Option<i64>,
    pub is_compressed: bool,
    pub is_encrypted: bool,
    pub encryption_key: Vec<u8>,
}

impl ReplayInfo {
    /// True when the payload can be decrypted, i.e. the match is over.
    pub fn is_complete(&self) -> bool {
        !self.is_encrypted || (!self.encryption_key.is_empty() && self.encryption_key.iter().any(|&b| b != 0))
    }
}

#[derive(Debug, Clone, Default)]
pub struct ReplayHeader {
    pub network_version: u32,
    pub network_checksum: u32,
    pub engine_network_version: u32,
    pub game_network_protocol_version: u32,
    /// Unique per recording; stable across file renames and copies.
    pub guid: Option<[u8; 16]>,
    pub patch: u16,
    pub changelist: u32,
    pub branch: String,
    pub major: Option<u32>,
    pub minor: Option<u32>,
    pub level_names_and_times: Vec<(String, u32)>,
    pub flags: u32,
    pub game_specific_data: Vec<String>,
}

impl ReplayHeader {
    pub fn versions(&self) -> NetVersions {
        NetVersions { network: self.network_version, engine_network: self.engine_network_version, flags: self.flags }
    }

    pub fn guid_hex(&self) -> Option<String> {
        self.guid.map(|g| g.iter().map(|b| format!("{b:02x}")).collect())
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DataChunk {
    pub start_ms: u32,
    pub end_ms: u32,
    pub offset: usize,
    pub length: usize,
}

#[derive(Debug, Clone)]
pub struct EventChunk {
    pub id: String,
    pub group: String,
    pub metadata: String,
    pub start_ms: u32,
    pub end_ms: u32,
    pub offset: usize,
    pub length: usize,
}

#[derive(Debug, Clone)]
pub struct Container {
    pub info: ReplayInfo,
    pub header: ReplayHeader,
    pub data_chunks: Vec<DataChunk>,
    pub event_chunks: Vec<EventChunk>,
}

/// Parsed `AthenaMatchTeamStats` event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TeamStats {
    pub placement: u32,
    pub total_players: u32,
}

/// Parsed `AthenaMatchStats` event (the recording player's end-of-match stats).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MatchStats {
    pub accuracy: f32,
    pub assists: u32,
    pub eliminations: u32,
    pub weapon_damage: u32,
    pub other_damage: u32,
    pub revives: u32,
    pub damage_taken: u32,
    pub damage_to_structures: u32,
    pub materials_gathered: u32,
    pub materials_used: u32,
    pub total_traveled: u32,
}

pub fn read_info(file: &[u8]) -> Result<ReplayInfo> {
    let mut r = BitReader::new(file, NetVersions::default());
    read_info_from(&mut r)
}

fn read_info_from(r: &mut BitReader<'_>) -> Result<ReplayInfo> {
    if r.read_u32() != FILE_MAGIC {
        return Err(Error::NotAReplay);
    }
    let file_version = r.read_u32();
    if file_version >= 7 {
        let custom_versions = r.read_i32().max(0) as usize;
        r.skip_bytes(custom_versions * 20);
    }
    let length_ms = r.read_u32();
    let network_version = r.read_u32();
    let changelist = r.read_u32();
    let friendly_name = r.read_string();
    let is_live = r.read_bool32();
    let timestamp_ms = (file_version >= 3).then(|| {
        let ticks = r.read_u64();
        (ticks.saturating_sub(UNIX_EPOCH_TICKS) / 10_000) as i64
    });
    let is_compressed = file_version >= 2 && r.read_bool32();
    let (is_encrypted, encryption_key) = if file_version >= 6 {
        let encrypted = r.read_bool32();
        let key_len = r.read_u32() as usize;
        if key_len > 64 {
            return Err(Error::Corrupt("encryption key length"));
        }
        (encrypted, r.read_bytes(key_len))
    } else {
        (false, Vec::new())
    };
    if r.is_error() {
        return Err(Error::Corrupt("file info"));
    }
    Ok(ReplayInfo {
        file_version,
        length_ms,
        network_version,
        changelist,
        friendly_name,
        is_live,
        timestamp_ms,
        is_compressed,
        is_encrypted,
        encryption_key,
    })
}

pub fn read_container(file: &[u8]) -> Result<Container> {
    let mut r = BitReader::new(file, NetVersions::default());
    let info = read_info_from(&mut r)?;
    if !info.is_complete() {
        return Err(Error::Incomplete);
    }

    let mut header = None;
    let mut data_chunks = Vec::new();
    let mut event_chunks = Vec::new();

    while r.bits_left() >= 64 {
        let chunk_type = r.read_u32();
        let chunk_size = r.read_i32();
        let Some(limit) = usize::try_from(chunk_size).ok().and_then(|size| r.push_limit(size * 8)) else {
            break;
        };
        match chunk_type {
            0 => header = Some(read_header(&mut r)?),
            1 => {
                let (start_ms, end_ms) = if info.file_version >= 4 { (r.read_u32(), r.read_u32()) } else { (0, 0) };
                let length = r.read_u32() as usize;
                if info.file_version >= 6 {
                    r.skip_bytes(4);
                }
                data_chunks.push(DataChunk { start_ms, end_ms, offset: r.pos() / 8, length });
            }
            3 => {
                let id = r.read_string();
                let group = r.read_string();
                let metadata = r.read_string();
                let start_ms = r.read_u32();
                let end_ms = r.read_u32();
                let length = r.read_u32() as usize;
                event_chunks.push(EventChunk { id, group, metadata, start_ms, end_ms, offset: r.pos() / 8, length });
            }
            _ => {}
        }
        r.pop_limit(limit);
    }

    let header = header.ok_or(Error::Corrupt("missing header chunk"))?;
    Ok(Container { info, header, data_chunks, event_chunks })
}

fn read_header(r: &mut BitReader<'_>) -> Result<ReplayHeader> {
    if r.read_u32() != HEADER_MAGIC {
        return Err(Error::Corrupt("header magic"));
    }
    let mut h = ReplayHeader { network_version: r.read_u32(), ..Default::default() };
    if h.network_version >= 19 {
        let custom_versions = r.read_i32().max(0) as usize;
        r.skip_bytes(custom_versions * 20);
    }
    h.network_checksum = r.read_u32();
    h.engine_network_version = r.read_u32();
    h.game_network_protocol_version = r.read_u32();
    if h.network_version >= 12 {
        let bytes = r.read_bytes(16);
        h.guid = bytes.try_into().ok();
    }
    if h.network_version >= 11 {
        r.skip_bytes(4);
        h.patch = r.read_u16();
        h.changelist = r.read_u32();
        h.branch = r.read_string();
        (h.major, h.minor) = parse_branch(&h.branch);
    } else {
        h.changelist = r.read_u32();
    }
    if h.network_version >= 18 {
        r.skip_bytes(12);
    }
    let levels = r.read_u32() as usize;
    for _ in 0..levels.min(4096) {
        let name = r.read_string();
        let time = r.read_u32();
        h.level_names_and_times.push((name, time));
    }
    if h.network_version >= 9 {
        h.flags = r.read_u32();
    }
    let game_specific = r.read_u32() as usize;
    for _ in 0..game_specific.min(4096) {
        h.game_specific_data.push(r.read_string());
    }
    if r.is_error() {
        return Err(Error::Corrupt("header"));
    }
    Ok(h)
}

/// Extracts the season from a branch name like `++Fortnite+Release-43.10`.
fn parse_branch(branch: &str) -> (Option<u32>, Option<u32>) {
    let Some(rest) = branch.split("Release-").nth(1) else {
        return (None, None);
    };
    let mut parts = rest.split('.');
    let number =
        |s: Option<&str>| s.and_then(|s| s.chars().take_while(char::is_ascii_digit).collect::<String>().parse().ok());
    (number(parts.next()), number(parts.next()))
}

/// Returns the plaintext of a chunk: AES-256-ECB with PKCS#7 padding when encrypted.
pub fn decrypt(info: &ReplayInfo, file: &[u8], offset: usize, length: usize) -> Result<Vec<u8>> {
    let end = offset.checked_add(length).filter(|&e| e <= file.len()).ok_or(Error::Corrupt("chunk bounds"))?;
    let mut data = file[offset..end].to_vec();
    if !info.is_encrypted {
        return Ok(data);
    }
    let cipher = Aes256::new_from_slice(&info.encryption_key).map_err(|_| Error::Corrupt("encryption key"))?;
    let whole_blocks = data.len() / 16 * 16;
    for block in data[..whole_blocks].as_chunks_mut::<16>().0 {
        cipher.decrypt_block(GenericArray::from_mut_slice(block));
    }
    data.truncate(whole_blocks);
    strip_pkcs7(&mut data);
    Ok(data)
}

fn strip_pkcs7(data: &mut Vec<u8>) {
    let Some(&pad) = data.last() else { return };
    let pad = pad as usize;
    if (1..=16).contains(&pad) && pad <= data.len() && data[data.len() - pad..].iter().all(|&b| b as usize == pad) {
        data.truncate(data.len() - pad);
    }
}

/// Inflates an Oodle-compressed data chunk (`[decompressed size][compressed size][bytes]`).
pub fn decompress(info: &ReplayInfo, plain: Vec<u8>) -> Result<Vec<u8>> {
    if !info.is_compressed {
        return Ok(plain);
    }
    if plain.len() < 8 {
        return Err(Error::Corrupt("compressed chunk header"));
    }
    let decompressed_size = i32::from_le_bytes(plain[0..4].try_into().unwrap_or_default());
    let compressed_size = i32::from_le_bytes(plain[4..8].try_into().unwrap_or_default());
    let (Ok(decompressed_size), Ok(compressed_size)) =
        (usize::try_from(decompressed_size), usize::try_from(compressed_size))
    else {
        return Err(Error::Corrupt("compressed chunk sizes"));
    };
    if decompressed_size > 512 * 1024 * 1024 {
        return Err(Error::Corrupt("decompressed size"));
    }
    let input = plain.get(8..8 + compressed_size).ok_or(Error::Corrupt("compressed chunk bounds"))?;
    let mut output = vec![0u8; decompressed_size];
    let written = oozextract::Extractor::new()
        .read_from_slice(input, &mut output)
        .map_err(|e| Error::Decompress(format!("{e:?}")))?;
    output.truncate(written);
    Ok(output)
}

pub fn read_team_stats(plain: &[u8]) -> Option<TeamStats> {
    let mut r = BitReader::new(plain, NetVersions::default());
    r.skip_bytes(4);
    let placement = r.read_u32();
    let total_players = r.read_u32();
    (!r.is_error()).then_some(TeamStats { placement, total_players })
}

pub fn read_match_stats(plain: &[u8]) -> Option<MatchStats> {
    let mut r = BitReader::new(plain, NetVersions::default());
    r.skip_bytes(4);
    let stats = MatchStats {
        accuracy: r.read_f32(),
        assists: r.read_u32(),
        eliminations: r.read_u32(),
        weapon_damage: r.read_u32(),
        other_damage: r.read_u32(),
        revives: r.read_u32(),
        damage_taken: r.read_u32(),
        damage_to_structures: r.read_u32(),
        materials_gathered: r.read_u32(),
        materials_used: r.read_u32(),
        total_traveled: r.read_u32(),
    };
    (!r.is_error()).then_some(stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aes::cipher::BlockEncrypt;

    #[test]
    fn parses_branch() {
        assert_eq!(parse_branch("++Fortnite+Release-43.10"), (Some(43), Some(10)));
        assert_eq!(parse_branch("++Fortnite+Main"), (None, None));
    }

    #[test]
    fn decrypts_and_strips_padding() {
        let key = [7u8; 32];
        let cipher = Aes256::new_from_slice(&key).unwrap();
        let mut payload = b"hello replay".to_vec();
        payload.extend(std::iter::repeat_n(4u8, 4));
        cipher.encrypt_block(GenericArray::from_mut_slice(&mut payload));
        let info = ReplayInfo {
            file_version: 6,
            length_ms: 0,
            network_version: 0,
            changelist: 0,
            friendly_name: String::new(),
            is_live: false,
            timestamp_ms: None,
            is_compressed: false,
            is_encrypted: true,
            encryption_key: key.to_vec(),
        };
        assert_eq!(decrypt(&info, &payload, 0, 16).unwrap(), b"hello replay");
    }

    #[test]
    fn zero_key_means_incomplete() {
        let mut info = ReplayInfo {
            file_version: 6,
            length_ms: 0,
            network_version: 0,
            changelist: 0,
            friendly_name: String::new(),
            is_live: true,
            timestamp_ms: None,
            is_compressed: true,
            is_encrypted: true,
            encryption_key: vec![0; 32],
        };
        assert!(!info.is_complete());
        info.encryption_key[3] = 1;
        assert!(info.is_complete());
    }
}
