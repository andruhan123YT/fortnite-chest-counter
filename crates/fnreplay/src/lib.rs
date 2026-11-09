//! Reader for Fortnite `.replay` files.
//!
//! ```no_run
//! use fnreplay::{GroupSpec, Decoder, Schema, Visitor, PropertyUpdate};
//!
//! struct Names(Vec<String>);
//! impl Visitor for Names {
//!     fn on_update(&mut self, u: &PropertyUpdate<'_>) {
//!         if let Some(name) = u.get("PlayerNamePrivate").and_then(|v| v.as_str()) {
//!             self.0.push(name.to_string());
//!         }
//!     }
//! }
//!
//! let schema = Schema::new(vec![GroupSpec::new("PlayerState")
//!     .paths(["/Script/FortniteGame.FortPlayerStateAthena"])
//!     .prop("PlayerNamePrivate", Decoder::String)]);
//! let bytes = std::fs::read("match.replay").unwrap();
//! let mut names = Names(Vec::new());
//! fnreplay::parse(&bytes, &schema, &mut names).unwrap();
//! ```

pub mod bits;
pub mod container;
mod names;
mod net;
pub mod paths;
pub mod schema;

pub use bits::{Rotator, Vec3};
pub use container::{read_info, MatchStats, ReplayHeader, ReplayInfo, TeamStats};
pub use net::{ActorDespawn, ActorSpawn, PropertyUpdate, StreamStats, Visitor};
pub use schema::{Decoder, GroupSpec, RepMovement, Schema, Value, FAST_ARRAY_ELEMENT};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("not a replay file")]
    NotAReplay,
    #[error("the match is still in progress (the replay has no encryption key yet)")]
    Incomplete,
    #[error("corrupt replay: {0}")]
    Corrupt(&'static str),
    #[error("decompression failed: {0}")]
    Decompress(String),
    #[error("unsupported replay format: {0}")]
    Unsupported(&'static str),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Everything known about a replay besides what the visitor collected.
#[derive(Debug, Clone)]
pub struct Replay {
    pub info: ReplayInfo,
    pub header: ReplayHeader,
    pub team_stats: Option<TeamStats>,
    pub match_stats: Option<MatchStats>,
    pub stream: StreamStats,
    /// Data chunks that had to be abandoned midway.
    pub broken_chunks: u32,
    pub stopped_early: bool,
}

impl Replay {
    /// Share of packets that decoded cleanly. A sharp drop usually means a game update
    /// changed the network format.
    pub fn packet_health(&self) -> f64 {
        if self.stream.packets == 0 {
            return 0.0;
        }
        1.0 - self.stream.failed_packets as f64 / self.stream.packets as f64
    }
}

pub fn parse<V: Visitor>(file: &[u8], schema: &Schema, visitor: &mut V) -> Result<Replay> {
    let c = container::read_container(file)?;
    let versions = c.header.versions();
    if !versions.has_level_streaming_fixes() {
        return Err(Error::Unsupported("replays without level streaming fixes"));
    }

    let mut team_stats = None;
    let mut match_stats = None;
    for event in &c.event_chunks {
        let wanted = matches!(event.metadata.as_str(), "AthenaMatchTeamStats" | "AthenaMatchStats");
        if !wanted {
            continue;
        }
        let Ok(plain) = container::decrypt(&c.info, file, event.offset, event.length) else { continue };
        match event.metadata.as_str() {
            "AthenaMatchTeamStats" => team_stats = container::read_team_stats(&plain),
            _ => match_stats = container::read_match_stats(&plain),
        }
    }

    let mut stream = net::NetStream::new(schema, versions);
    let mut broken_chunks = 0;
    let mut stopped_early = false;
    let total = c.data_chunks.len().max(1);
    for (i, chunk) in c.data_chunks.iter().enumerate() {
        if visitor.should_stop() {
            stopped_early = true;
            break;
        }
        let plain = container::decrypt(&c.info, file, chunk.offset, chunk.length)?;
        let data = container::decompress(&c.info, plain)?;
        let mut r = bits::BitReader::new(&data, versions);
        while !r.at_end() {
            if stream.read_frame(&mut r, visitor).is_err() {
                broken_chunks += 1;
                break;
            }
        }
        visitor.on_progress((i + 1) as f32 / total as f32);
    }

    Ok(Replay {
        info: c.info,
        header: c.header,
        team_stats,
        match_stats,
        stream: stream.stats,
        broken_chunks,
        stopped_early,
    })
}
