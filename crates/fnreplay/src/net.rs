//! The demo network stream: frames, packets, bunches, channels and property replication.
//!
//! This mirrors the receive path of `UDemoNetDriver` closely enough to follow actor
//! channels and decode the properties requested by a [`Schema`]. Remote procedure calls
//! and fast-array deltas are skipped.

use std::collections::{HashMap, HashSet};

use crate::bits::{BitBuf, BitReader, NetVersions, Vec3};
use crate::paths::{clean_static_id_suffix, remove_path_prefix};
use crate::schema::{decode, Decoder, Schema, Value, FAST_ARRAY_ELEMENT};

const MAX_PACKET_BYTES: i32 = 2048;
const MAX_BUNCH_BITS: u32 = 2048 * 8;
const MAX_GUIDS_PER_BUNCH: i32 = 2048;
const MAX_LOAD_OBJECT_DEPTH: u32 = 16;
const MAX_FAST_ARRAY_CHANGES: i32 = 1024;
/// `HISTORY_FAST_ARRAY_DELTA_STRUCT`: custom delta properties start with a flag bit.
const FAST_ARRAY_DELTA_STRUCT_ENGINE: u32 = 19;
/// Replay player controllers carry extra data after the actor spawn header.
const REPLAY_PLAYER_CONTROLLERS: &[&str] = &["BP_ReplayPC_Athena_C"];
/// From this engine network version the replay controller's spawn header is followed by
/// 9 bytes instead of 1: the net player index plus 8 more bytes, all zero in every v43
/// replay seen so far. Verified on engine net 45; 36 and older use the single byte. The
/// exact version in between where it grew is unknown.
const REPLAY_PC_LONG_TRAILER_ENGINE: u32 = 45;

/// Receives decoded data while a replay is parsed.
pub trait Visitor {
    fn on_update(&mut self, _update: &PropertyUpdate<'_>) {}
    fn on_despawn(&mut self, _despawn: &ActorDespawn<'_>) {}
    fn on_actor_spawn(&mut self, _spawn: &ActorSpawn<'_>) {}
    /// Called for every replicated field the stream declares, for all classes, including
    /// those the schema does not ask for. Meant for exploring a new game version.
    fn on_net_field_export(&mut self, _group_path: &str, _handle: u32, _name: &str) {}
    /// Called between data chunks with the fraction of chunks done.
    fn on_progress(&mut self, _fraction: f32) {}
    /// Return true to stop parsing early.
    fn should_stop(&self) -> bool {
        false
    }
}

/// One replication update of a tracked actor: only the properties that changed.
#[derive(Debug)]
pub struct PropertyUpdate<'a> {
    pub time: f32,
    pub channel: u32,
    pub actor_guid: u32,
    /// Index into [`Schema::groups`].
    pub group: usize,
    pub group_path: &'a str,
    /// Full name of a level-placed actor; `None` for spawned actors.
    pub static_actor_id: Option<&'a str>,
    /// Class path of a spawned actor; `None` for level-placed ones.
    pub archetype_path: Option<&'a str>,
    pub props: &'a [(&'a str, Value)],
    object_paths: &'a HashMap<u32, String>,
}

impl PropertyUpdate<'_> {
    /// Path of an object referenced by a property, when the stream exported it.
    pub fn object_path(&self, guid: u32) -> Option<&str> {
        self.object_paths.get(&guid).map(String::as_str)
    }

    pub fn get(&self, name: &str) -> Option<&Value> {
        self.props.iter().find(|(n, _)| *n == name).map(|(_, v)| v)
    }

    pub fn changed(&self, name: &str) -> bool {
        self.props.iter().any(|(n, _)| *n == name)
    }
}

/// A tracked actor was destroyed (not merely dropped from relevancy).
#[derive(Debug)]
pub struct ActorDespawn<'a> {
    pub time: f32,
    pub channel: u32,
    pub actor_guid: u32,
    pub group: usize,
    pub group_path: &'a str,
    pub static_actor_id: Option<&'a str>,
    pub archetype_path: Option<&'a str>,
}

/// An actor channel opened. Emitted for every actor, tracked or not.
#[derive(Debug)]
pub struct ActorSpawn<'a> {
    pub time: f32,
    pub channel: u32,
    pub actor_guid: u32,
    pub archetype_path: Option<&'a str>,
    pub location: Option<Vec3>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StreamStats {
    pub frames: u64,
    pub packets: u64,
    /// Packets abandoned because the bit stream desynchronized.
    pub failed_packets: u64,
    pub bunches: u64,
    pub updates: u64,
}

#[derive(Debug, Clone, Copy)]
struct ActorRef {
    guid: u32,
    archetype: Option<u32>,
}

#[derive(Debug, Default)]
struct Channel {
    actor: Option<ActorRef>,
}

#[derive(Debug)]
struct ExportGroup {
    path: String,
    /// `path` without its package, used for archetype matching.
    short_path: String,
    num_exports: u32,
    spec: usize,
    /// Net field handle -> property index in the spec.
    exports: HashMap<u32, usize>,
    /// The matching `_ClassNetCache` group (RPCs and custom delta properties), if tracked.
    class_net_cache: Option<usize>,
}

#[derive(Debug)]
struct ExternalData {
    handle: u32,
    payload: Vec<u8>,
}

#[derive(Debug, Clone, Copy)]
struct BunchMeta {
    time: f32,
    channel: u32,
    sequence: u32,
    open: bool,
    close: bool,
    close_reason: u32,
    reliable: bool,
    has_exports: bool,
    has_must_be_mapped_guids: bool,
    partial_initial: bool,
    partial_final: bool,
}

#[derive(Debug)]
struct PartialBunch {
    meta: BunchMeta,
    buf: BitBuf,
    pos: usize,
}

#[derive(Debug)]
pub(crate) struct FrameError;

pub(crate) struct NetStream<'s> {
    schema: &'s Schema,
    versions: NetVersions,
    channels: HashMap<u32, Channel>,
    ignored: HashSet<u32>,
    in_reliable: u32,
    in_packet_id: u32,
    partial: Option<PartialBunch>,
    guid_paths: HashMap<u32, String>,
    groups: Vec<ExportGroup>,
    group_by_path: HashMap<String, usize>,
    group_by_index: HashMap<u32, usize>,
    static_groups: HashMap<String, usize>,
    subobject_groups: HashMap<String, usize>,
    archetype_groups: HashMap<u32, usize>,
    external: HashMap<u32, ExternalData>,
    /// Path of every export group seen, tracked or not, by its path index.
    export_paths: HashMap<u32, String>,
    /// Field bindings shared by classes with the same spec and layout size. Fortnite
    /// exports a field once for the whole family of chest classes, so a child class
    /// often replicates a field before (or without) exporting it under its own path.
    shared_exports: HashMap<(usize, u32), HashMap<u32, usize>>,
    pub(crate) stats: StreamStats,
}

impl<'s> NetStream<'s> {
    pub(crate) fn new(schema: &'s Schema, versions: NetVersions) -> Self {
        Self {
            schema,
            versions,
            channels: HashMap::new(),
            ignored: HashSet::new(),
            in_reliable: 0,
            in_packet_id: 0,
            partial: None,
            guid_paths: HashMap::new(),
            groups: Vec::new(),
            group_by_path: HashMap::new(),
            group_by_index: HashMap::new(),
            static_groups: HashMap::new(),
            subobject_groups: HashMap::new(),
            archetype_groups: HashMap::new(),
            shared_exports: HashMap::new(),
            external: HashMap::new(),
            export_paths: HashMap::new(),
            stats: StreamStats::default(),
        }
    }

    /// Reads one demo frame. An error means the chunk can no longer be followed.
    pub(crate) fn read_frame(&mut self, r: &mut BitReader<'_>, visitor: &mut impl Visitor) -> Result<(), FrameError> {
        let v = self.versions;
        if v.network >= 6 {
            r.read_i32();
        }
        let time = r.read_f32();
        if v.network >= 10 {
            self.read_net_field_exports(r, visitor);
            self.read_net_export_guids(r);
        }
        if !v.has_level_streaming_fixes() {
            return Err(FrameError);
        }
        let streaming_levels = r.read_int_packed();
        for _ in 0..streaming_levels {
            r.read_string();
            if r.is_error() {
                return Err(FrameError);
            }
        }
        r.skip_bytes(8);
        self.read_external_data(r);
        if v.has_game_specific_frame_data() {
            let skip = r.read_u64();
            r.skip_bytes(usize::try_from(skip).unwrap_or(usize::MAX));
        }
        if r.is_error() {
            return Err(FrameError);
        }
        self.stats.frames += 1;

        loop {
            r.read_int_packed();
            let size = r.read_i32();
            if r.is_error() || !(0..=MAX_PACKET_BYTES).contains(&size) {
                return Err(FrameError);
            }
            if size == 0 {
                return Ok(());
            }
            let limit = r.push_limit(size as usize * 8).ok_or(FrameError)?;
            self.read_raw_packet(r, time, visitor);
            r.pop_limit(limit);
        }
    }

    /// Packets end with a single 1 bit after the payload; everything past it is padding.
    fn read_raw_packet(&mut self, r: &mut BitReader<'_>, time: f32, visitor: &mut impl Visitor) {
        let last_byte = r.data()[r.end() / 8 - 1];
        if last_byte == 0 {
            self.stats.failed_packets += 1;
            return;
        }
        let payload_bits = r.bits_left() - 1 - last_byte.leading_zeros() as usize;
        let Some(limit) = r.push_limit(payload_bits) else { return };
        self.in_packet_id = self.in_packet_id.wrapping_add(1);
        self.stats.packets += 1;
        let ok = self.read_packet(r, time, visitor);
        let had_error = r.pop_limit(limit);
        if !ok || had_error {
            self.stats.failed_packets += 1;
        }
    }

    fn read_packet(&mut self, r: &mut BitReader<'_>, time: f32, visitor: &mut impl Visitor) -> bool {
        let engine = self.versions.engine_network;
        while !r.at_end() {
            if engine < 8 {
                r.skip_bits(1);
            }
            let control = r.read_bit();
            let open = control && r.read_bit();
            let close = control && r.read_bit();
            let close_reason = if close { r.read_serialized_int(15) } else { 0 };
            let _replication_paused = r.read_bit();
            let reliable = r.read_bit();
            let channel = r.read_int_packed();
            let has_exports = r.read_bit();
            let has_must_be_mapped_guids = r.read_bit();
            let partial = r.read_bit();
            let sequence = if reliable {
                self.in_reliable.wrapping_add(1)
            } else if partial {
                self.in_packet_id
            } else {
                0
            };
            let (partial_initial, partial_final) = if partial {
                let initial = r.read_bit();
                if engine >= 36 {
                    let _partial_custom_exports_final = r.read_bit();
                }
                (initial, r.read_bit())
            } else {
                (false, false)
            };
            if reliable || open {
                let _channel_name = r.read_fname();
            }
            let bunch_bits = r.read_serialized_int(MAX_BUNCH_BITS) as usize;
            if r.is_error() {
                return false;
            }
            self.stats.bunches += 1;

            let meta = BunchMeta {
                time,
                channel,
                sequence,
                open,
                close,
                close_reason,
                reliable,
                has_exports,
                has_must_be_mapped_guids,
                partial_initial,
                partial_final,
            };

            if self.ignored.contains(&channel) {
                r.skip_bits(bunch_bits);
                if close {
                    self.close_channel(channel);
                }
                continue;
            }

            let known = self.channels.contains_key(&channel);
            let accept = known || reliable || (open && (close || partial));

            if partial {
                let buf = BitBuf::from_bytes(r.read_bits_to_vec(bunch_bits), bunch_bits);
                if r.is_error() {
                    return false;
                }
                let mut br = buf.reader(self.versions);
                if has_exports {
                    self.receive_net_guid_bunch(&mut br, visitor);
                }
                let pos = br.pos();
                if !accept {
                    continue;
                }
                self.channels.entry(channel).or_default();
                self.receive_partial(meta, buf, pos, visitor);
                continue;
            }

            let Some(limit) = r.push_limit(bunch_bits) else { return false };
            if has_exports {
                self.receive_net_guid_bunch(r, visitor);
            }
            if accept {
                self.channels.entry(channel).or_default();
                if reliable {
                    self.in_reliable = sequence;
                }
                self.receive_sequenced_bunch(r, &meta, visitor);
            }
            if r.pop_limit(limit) {
                return false;
            }
        }
        true
    }

    fn receive_partial(&mut self, meta: BunchMeta, buf: BitBuf, pos: usize, visitor: &mut impl Visitor) {
        if meta.reliable {
            self.in_reliable = meta.sequence;
        }

        if meta.partial_initial {
            if let Some(existing) = &self.partial {
                if existing.meta.reliable {
                    // An unfinished reliable bunch is never dropped for a newer one.
                    return;
                }
            }
            self.partial = Some(PartialBunch { meta, buf, pos });
            return;
        }

        let Some(mut pending) = self.partial.take() else { return };
        let sequence_ok = if pending.meta.reliable {
            meta.sequence == pending.meta.sequence.wrapping_add(1)
        } else {
            meta.sequence == pending.meta.sequence || meta.sequence == pending.meta.sequence.wrapping_add(1)
        };
        if !sequence_ok || pending.meta.reliable != meta.reliable {
            self.partial = Some(pending);
            return;
        }

        let bits_left = buf.bits.saturating_sub(pos);
        if !meta.has_exports && bits_left > 0 {
            let mut br = buf.reader(self.versions);
            br.set_pos(pos);
            let tail = br.read_bits_to_vec(bits_left);
            pending.buf.append(&tail, bits_left);
        }
        if !meta.has_exports && !meta.partial_final && !bits_left.is_multiple_of(8) {
            self.partial = Some(pending);
            return;
        }
        pending.meta.sequence = meta.sequence;

        if !meta.partial_final {
            self.partial = Some(pending);
            return;
        }
        if meta.has_exports {
            self.partial = Some(pending);
            return;
        }

        let mut assembled = pending.meta;
        assembled.close = meta.close;
        assembled.close_reason = meta.close_reason;
        let mut br = pending.buf.reader(self.versions);
        br.set_pos(pending.pos);
        self.receive_sequenced_bunch(&mut br, &assembled, visitor);
    }

    fn receive_sequenced_bunch(&mut self, r: &mut BitReader<'_>, meta: &BunchMeta, visitor: &mut impl Visitor) {
        if meta.has_must_be_mapped_guids {
            let count = r.read_u16();
            for _ in 0..count {
                r.read_int_packed();
            }
        }
        self.process_bunch(r, meta, visitor);

        if meta.close {
            if meta.close_reason == 0 {
                self.emit_despawn(meta, visitor);
            }
            self.close_channel(meta.channel);
        }
    }

    fn close_channel(&mut self, channel: u32) {
        self.ignored.remove(&channel);
        self.channels.remove(&channel);
    }

    fn emit_despawn(&mut self, meta: &BunchMeta, visitor: &mut impl Visitor) {
        let Some(actor) = self.channels.get(&meta.channel).and_then(|c| c.actor) else { return };
        let (group, static_actor_id) = if is_dynamic(actor.guid) {
            (actor.archetype.and_then(|a| self.group_for_archetype(a)), None)
        } else {
            self.group_for_static_actor(actor.guid)
        };
        let Some(group) = group else { return };
        let g = &self.groups[group];
        visitor.on_despawn(&ActorDespawn {
            time: meta.time,
            channel: meta.channel,
            actor_guid: actor.guid,
            group: g.spec,
            group_path: &g.path,
            static_actor_id: static_actor_id.as_deref(),
            archetype_path: actor.archetype.and_then(|a| self.guid_paths.get(&a)).map(String::as_str),
        });
    }

    fn process_bunch(&mut self, r: &mut BitReader<'_>, meta: &BunchMeta, visitor: &mut impl Visitor) {
        let Some(channel) = self.channels.get(&meta.channel) else { return };

        let actor = match channel.actor {
            Some(actor) => actor,
            None => {
                if !meta.open {
                    return;
                }
                let guid = self.load_object(r, false, 0);
                if r.at_end() && is_dynamic(guid) {
                    return;
                }
                let mut actor = ActorRef { guid, archetype: None };
                let mut location = None;
                if is_dynamic(guid) {
                    actor.archetype = Some(self.load_object(r, false, 0));
                    if self.versions.engine_network >= 5 {
                        self.load_object(r, false, 0);
                    }
                    location = Some(read_conditional_vector(r, Vec3::ZERO));
                    if r.read_bit() {
                        r.read_rotation_short();
                    }
                    read_conditional_vector(r, Vec3::new(1.0, 1.0, 1.0));
                    read_conditional_vector(r, Vec3::ZERO);
                }
                if let Some(channel) = self.channels.get_mut(&meta.channel) {
                    channel.actor = Some(actor);
                }
                let archetype_path = actor.archetype.and_then(|a| self.guid_paths.get(&a)).map(String::as_str);
                visitor.on_actor_spawn(&ActorSpawn {
                    time: meta.time,
                    channel: meta.channel,
                    actor_guid: guid,
                    archetype_path,
                    location,
                });
                if archetype_path.is_some_and(|p| REPLAY_PLAYER_CONTROLLERS.contains(&p)) {
                    let trailer = if self.versions.engine_network >= REPLAY_PC_LONG_TRAILER_ENGINE { 9 } else { 1 };
                    r.skip_bytes(trailer);
                }
                actor
            }
        };

        while !r.at_end() {
            let block = self.read_content_block_header(r, actor);
            if block.deleted {
                continue;
            }
            let payload_bits = r.read_int_packed() as usize;
            let limit = if payload_bits > 0 {
                match r.push_limit(payload_bits) {
                    Some(limit) => Some(limit),
                    None => {
                        r.set_error();
                        return;
                    }
                }
            } else {
                None
            };
            if r.is_error() {
                if let Some(limit) = limit {
                    r.pop_limit(limit);
                }
                r.set_error();
                return;
            }
            if let Some(limit) = limit {
                if block.rep_object != 0 {
                    self.receive_replicator(r, meta, actor, block, visitor);
                }
                if r.pop_limit(limit) {
                    r.set_error();
                    return;
                }
            }
        }
    }

    fn read_content_block_header(&mut self, r: &mut BitReader<'_>, actor: ActorRef) -> ContentBlock {
        let has_rep_layout = r.read_bit();
        let is_actor = r.read_bit();
        if is_actor {
            let rep_object = actor.archetype.filter(|&a| a != 0).unwrap_or(actor.guid);
            return ContentBlock { has_rep_layout, is_actor, rep_object, deleted: false };
        }

        let net_guid = self.load_object(r, false, 0);
        if r.read_bit() {
            // Stably named sub-object: its own guid identifies the class.
            return ContentBlock { has_rep_layout, is_actor, rep_object: net_guid, deleted: false };
        }

        let mut delete = false;
        let mut serialize_class = true;
        if self.versions.engine_network >= 30 && r.read_bit() {
            delete = true;
            serialize_class = false;
            r.skip_bits(8);
        }
        let mut class_guid = 0;
        if serialize_class {
            class_guid = self.load_object(r, false, 0);
            delete = class_guid == 0;
        }
        if delete {
            return ContentBlock { has_rep_layout, is_actor, rep_object: net_guid, deleted: true };
        }
        if self.versions.engine_network >= 18 && !r.read_bit() {
            self.load_object(r, false, 0);
        }
        ContentBlock { has_rep_layout, is_actor, rep_object: class_guid, deleted: false }
    }

    fn receive_replicator(
        &mut self,
        r: &mut BitReader<'_>,
        meta: &BunchMeta,
        actor: ActorRef,
        block: ContentBlock,
        visitor: &mut impl Visitor,
    ) {
        let (group, static_actor_id) = if is_dynamic(actor.guid) {
            (self.group_for_archetype(block.rep_object), None)
        } else {
            self.group_for_static_actor(block.rep_object)
        };
        if std::env::var("DBGPS").is_ok()
            && !block.is_actor
            && actor.archetype.and_then(|a| self.guid_paths.get(&a)).is_some_and(|p| p.contains("PlayerState"))
        {
            eprintln!(
                "PSB found={} rep={} path={:?} layout={}",
                group.is_some(),
                block.rep_object,
                self.guid_paths.get(&block.rep_object),
                block.has_rep_layout
            );
        }
        let Some(group) = group else {
            if block.is_actor {
                self.ignored.insert(meta.channel);
            }
            return;
        };
        if block.has_rep_layout {
            self.receive_properties(r, meta, actor, group, static_actor_id.as_deref(), visitor);
        }
        if let Some(cache) = self.groups[group].class_net_cache {
            self.receive_class_net_cache(r, meta, actor, group, cache, static_actor_id.as_deref(), visitor);
        }
    }

    /// RPCs and custom delta properties follow the replicated properties. Only fast arrays
    /// the schema asks for are decoded; the rest is skipped by its length. Errors in here
    /// are contained: this data is optional and must not fail the packet.
    #[allow(clippy::too_many_arguments)]
    fn receive_class_net_cache(
        &mut self,
        r: &mut BitReader<'_>,
        meta: &BunchMeta,
        actor: ActorRef,
        group: usize,
        cache: usize,
        static_actor_id: Option<&str>,
        visitor: &mut impl Visitor,
    ) {
        let Some(window) = r.push_limit(r.bits_left()) else { return };
        let max_handle = self.groups[cache].num_exports.max(2);
        let schema = self.schema;
        let cache_spec = &schema.groups[self.groups[cache].spec];
        while !r.at_end() {
            let handle = r.read_serialized_int(max_handle);
            let num_bits = r.read_int_packed() as usize;
            if r.is_error() {
                break;
            }
            let Some(field) = r.push_limit(num_bits) else { break };
            let wanted = self.groups[cache].exports.get(&handle).map(|&p| cache_spec.props[p].decoder);
            if std::env::var("DBGCNC").is_ok() {
                eprintln!(
                    "CNC t={} g={} h={handle} bits={num_bits} max={max_handle} wanted={wanted:?} left={}",
                    meta.time,
                    self.groups[cache].path,
                    r.bits_left()
                );
            }
            if wanted == Some(Decoder::FastArray) {
                self.receive_fast_array(r, meta, actor, group, static_actor_id, visitor);
            }
            r.pop_limit(field);
        }
        r.pop_limit(window);
    }

    /// `FFastArraySerializer` delta: header, deleted ids, then each changed element with
    /// its properties in the owning class's handle space.
    fn receive_fast_array(
        &mut self,
        r: &mut BitReader<'_>,
        meta: &BunchMeta,
        actor: ActorRef,
        group: usize,
        static_actor_id: Option<&str>,
        visitor: &mut impl Visitor,
    ) {
        if self.versions.engine_network >= FAST_ARRAY_DELTA_STRUCT_ENGINE {
            r.read_bit();
        }
        let _array_key = r.read_i32();
        let _base_key = r.read_i32();
        if std::env::var("DBGCNC").is_ok() {
            eprintln!("FA start left={}", r.bits_left());
        }
        let deletes = r.read_i32();
        let changed = r.read_i32();
        let counts = 0..=MAX_FAST_ARRAY_CHANGES;
        if r.is_error() || !counts.contains(&deletes) || !counts.contains(&changed) {
            return;
        }
        r.skip_bits(deletes as usize * 32);
        let schema = self.schema;
        for _ in 0..changed {
            let g = &self.groups[group];
            let spec = &schema.groups[g.spec];
            let element = r.read_i32();
            let mut props: Vec<(&str, Value)> = vec![(FAST_ARRAY_ELEMENT, Value::Int(element))];
            loop {
                let handle = r.read_int_packed();
                if r.is_error() {
                    return;
                }
                if handle == 0 {
                    break;
                }
                let num_bits = r.read_int_packed() as usize;
                let Some(&prop) = g.exports.get(&(handle - 1)) else {
                    r.skip_bits(num_bits);
                    continue;
                };
                let Some(limit) = r.push_limit(num_bits) else { return };
                let value = decode(spec.props[prop].decoder, r);
                r.pop_limit(limit);
                props.push((&spec.props[prop].name, value));
            }
            if props.len() == 1 {
                continue;
            }
            self.stats.updates += 1;
            visitor.on_update(&PropertyUpdate {
                time: meta.time,
                channel: meta.channel,
                actor_guid: actor.guid,
                group: g.spec,
                group_path: &g.path,
                static_actor_id,
                archetype_path: actor.archetype.and_then(|a| self.guid_paths.get(&a)).map(String::as_str),
                props: &props,
                object_paths: &self.guid_paths,
            });
        }
    }

    fn receive_properties(
        &mut self,
        r: &mut BitReader<'_>,
        meta: &BunchMeta,
        actor: ActorRef,
        group_index: usize,
        static_actor_id: Option<&str>,
        visitor: &mut impl Visitor,
    ) {
        let group = &self.groups[group_index];
        let spec = &self.schema.groups[group.spec];
        let mut props: Vec<(&str, Value)> = Vec::new();

        // Property checksum flag.
        r.skip_bits(1);
        loop {
            let handle = r.read_int_packed();
            if r.is_error() {
                return;
            }
            if handle == 0 {
                break;
            }
            let handle = handle - 1;
            if handle >= group.num_exports {
                return;
            }
            let num_bits = r.read_int_packed() as usize;
            if num_bits == 0 {
                continue;
            }
            let Some(prop) = self.export_for(group_index, handle) else {
                r.skip_bits(num_bits);
                continue;
            };
            let Some(limit) = r.push_limit(num_bits) else { return };
            let value = decode(spec.props[prop].decoder, r);
            r.pop_limit(limit);
            props.push((&spec.props[prop].name, value));
        }

        if props.is_empty() {
            return;
        }
        if let Some(external) = self.external.remove(&actor.guid) {
            if let Some(&prop) = group.exports.get(&external.handle) {
                let mut er = BitReader::new(&external.payload, self.versions);
                let name = spec.props[prop].name.as_str();
                let value = decode(spec.props[prop].decoder, &mut er);
                // External data carries the authoritative value and replaces the inline one.
                match props.iter_mut().find(|(n, _)| *n == name) {
                    Some(slot) => slot.1 = value,
                    None => props.push((name, value)),
                }
            }
        }

        self.stats.updates += 1;
        visitor.on_update(&PropertyUpdate {
            time: meta.time,
            channel: meta.channel,
            actor_guid: actor.guid,
            group: group.spec,
            group_path: &group.path,
            static_actor_id,
            archetype_path: actor.archetype.and_then(|a| self.guid_paths.get(&a)).map(String::as_str),
            props: &props,
            object_paths: &self.guid_paths,
        });
    }

    fn group_for_static_actor(&self, guid: u32) -> (Option<usize>, Option<String>) {
        let Some(path) = self.guid_paths.get(&guid) else { return (None, None) };
        let group = self.static_groups.get(clean_static_id_suffix(path)).copied();
        (group, Some(path.clone()))
    }

    fn group_for_archetype(&mut self, guid: u32) -> Option<usize> {
        if let Some(&group) = self.archetype_groups.get(&guid) {
            return Some(group);
        }
        let path = self.guid_paths.get(&guid)?;
        let group = match self.subobject_groups.get(path) {
            Some(&group) => group,
            None => self.groups.iter().position(|g| path.contains(g.short_path.as_str()))?,
        };
        self.archetype_groups.insert(guid, group);
        Some(group)
    }

    /// `UPackageMapClient::InternalLoadObject`. Exported paths are cached by guid.
    fn load_object(&mut self, r: &mut BitReader<'_>, exporting: bool, depth: u32) -> u32 {
        if depth > MAX_LOAD_OBJECT_DEPTH {
            return 0;
        }
        let guid = r.read_int_packed();
        if guid == 0 || r.is_error() {
            return guid;
        }
        if guid == 1 || exporting {
            let flags = r.read_u8();
            if flags & 1 != 0 {
                self.load_object(r, true, depth + 1);
                let path = r.read_string();
                if flags & 4 != 0 {
                    r.read_u32();
                }
                if exporting && !r.is_error() {
                    self.guid_paths.insert(guid, remove_path_prefix(&path).to_string());
                }
            }
        }
        guid
    }

    fn receive_net_guid_bunch(&mut self, r: &mut BitReader<'_>, visitor: &mut impl Visitor) {
        if r.read_bit() {
            self.receive_net_field_exports_compat(r, visitor);
            return;
        }
        let count = r.read_i32();
        if count > MAX_GUIDS_PER_BUNCH {
            return;
        }
        for _ in 0..count.max(0) {
            self.load_object(r, true, 0);
            if r.is_error() {
                return;
            }
        }
    }

    fn receive_net_field_exports_compat(&mut self, r: &mut BitReader<'_>, visitor: &mut impl Visitor) {
        let count = r.read_u32();
        for _ in 0..count {
            let path_index = r.read_int_packed();
            let declared = if r.read_bit() {
                let path = r.read_string();
                let num_exports = r.read_u32();
                Some((path, num_exports))
            } else {
                None
            };
            let field = read_net_field_export(r);
            if r.is_error() {
                return;
            }
            self.on_field_export(path_index, declared, field, true, visitor);
        }
    }

    fn read_net_field_exports(&mut self, r: &mut BitReader<'_>, visitor: &mut impl Visitor) {
        let count = r.read_int_packed();
        for _ in 0..count {
            let path_index = r.read_int_packed();
            let exported = r.read_int_packed() == 1;
            let declared = if exported {
                let path = r.read_string();
                let num_exports = r.read_int_packed();
                Some((path, num_exports))
            } else {
                None
            };
            let field = read_net_field_export(r);
            if r.is_error() {
                return;
            }
            self.on_field_export(path_index, declared, field, false, visitor);
        }
    }

    fn on_field_export(
        &mut self,
        path_index: u32,
        declared: Option<(String, u32)>,
        field: Option<(u32, String)>,
        check_bounds: bool,
        visitor: &mut impl Visitor,
    ) {
        let group = match declared {
            Some((path, num_exports)) => {
                self.export_paths.insert(path_index, path.clone());
                self.find_or_register_group(path, path_index, num_exports)
            }
            None => self.group_by_index.get(&path_index).copied(),
        };
        let Some((handle, name)) = field else { return };
        if let Some(path) = self.export_paths.get(&path_index) {
            visitor.on_net_field_export(path, handle, &name);
        }
        if let Some(group) = group {
            if !check_bounds || handle < self.groups[group].num_exports {
                self.bind_export(group, handle, &name);
            }
        }
    }

    fn find_or_register_group(&mut self, path: String, path_index: u32, num_exports: u32) -> Option<usize> {
        if let Some(&group) = self.group_by_path.get(&path) {
            return Some(group);
        }
        let spec = self.schema.spec_for_path(&path)?;
        let index = self.groups.len();
        self.groups.push(ExportGroup {
            short_path: remove_path_prefix(&path).to_string(),
            path: path.clone(),
            num_exports,
            spec,
            exports: HashMap::new(),
            class_net_cache: None,
        });
        if let Some(&base) = path.strip_suffix("_ClassNetCache").and_then(|b| self.group_by_path.get(b)) {
            self.groups[base].class_net_cache = Some(index);
        } else if let Some(&cache) = self.group_by_path.get(&format!("{path}_ClassNetCache")) {
            self.groups[index].class_net_cache = Some(cache);
        }
        self.group_by_path.insert(path, index);
        self.group_by_index.insert(path_index, index);
        for id in &self.schema.groups[spec].static_actor_ids {
            self.static_groups.insert(id.clone(), index);
        }
        for name in &self.schema.groups[spec].subobject_names {
            self.subobject_groups.insert(name.clone(), index);
        }
        Some(index)
    }

    /// The spec property behind a field handle: the group's own export, or the same
    /// handle exported by a sibling class with an identical layout.
    fn export_for(&self, group: usize, handle: u32) -> Option<usize> {
        let g = &self.groups[group];
        g.exports
            .get(&handle)
            .or_else(|| self.shared_exports.get(&(g.spec, g.num_exports)).and_then(|m| m.get(&handle)))
            .copied()
    }

    fn bind_export(&mut self, group: usize, handle: u32, name: &str) {
        let g = &self.groups[group];
        let spec_index = g.spec;
        let num_exports = g.num_exports;
        if let Some(prop) = self.schema.groups[spec_index].prop_index(name) {
            self.groups[group].exports.insert(handle, prop);
            self.shared_exports.entry((spec_index, num_exports)).or_default().insert(handle, prop);
        }
    }

    fn read_net_export_guids(&mut self, r: &mut BitReader<'_>) {
        let count = r.read_int_packed();
        for _ in 0..count {
            let size = r.read_i32();
            let Some(limit) = usize::try_from(size).ok().and_then(|s| r.push_limit(s * 8)) else {
                r.set_error();
                return;
            };
            self.load_object(r, true, 0);
            r.pop_limit(limit);
        }
    }

    /// Out-of-band property values (player names among them) attached to an actor and
    /// applied with its next property update.
    fn read_external_data(&mut self, r: &mut BitReader<'_>) {
        loop {
            let num_bits = r.read_int_packed();
            if num_bits == 0 || r.is_error() {
                return;
            }
            let guid = r.read_int_packed();
            let num_bytes = (num_bits / 8) as usize;
            let handle = u32::from(r.read_u8());
            r.read_u8();
            r.read_u8();
            let payload = r.read_bytes(num_bytes.saturating_sub(3));
            if r.is_error() {
                return;
            }
            self.external.insert(guid, ExternalData { handle, payload });
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct ContentBlock {
    has_rep_layout: bool,
    is_actor: bool,
    rep_object: u32,
    deleted: bool,
}

fn is_dynamic(guid: u32) -> bool {
    guid > 0 && guid & 1 == 0
}

fn read_conditional_vector(r: &mut BitReader<'_>, default: Vec3) -> Vec3 {
    if !r.read_bit() {
        return default;
    }
    let quantize = r.versions.engine_network < 13 || r.read_bit();
    if quantize {
        r.read_packed_vector(10.0, 24)
    } else {
        r.read_vector3d()
    }
}

fn read_net_field_export(r: &mut BitReader<'_>) -> Option<(u32, String)> {
    if r.read_u8() == 0 {
        return None;
    }
    let handle = r.read_int_packed();
    let _checksum = r.read_u32();
    let engine = r.versions.engine_network;
    let name = if engine < 9 {
        let name = r.read_string();
        r.read_string();
        name
    } else if engine < 10 {
        r.read_string()
    } else {
        r.read_fname_byte()
    };
    Some((handle, name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{Decoder, GroupSpec};

    struct Quiet;
    impl Visitor for Quiet {}

    fn declare(stream: &mut NetStream<'_>, index: u32, path: &str, size: u32, field: (u32, &str)) {
        let declared = Some((path.to_string(), size));
        stream.on_field_export(index, declared, Some((field.0, field.1.to_string())), false, &mut Quiet);
    }

    #[test]
    fn child_classes_share_field_exports_of_the_same_layout() {
        let schema = Schema::new(vec![GroupSpec::new("Chest")
            .paths(["/Game/Base.Base_C", "/Game/Child.Child_C", "/Game/Other.Other_C"])
            .prop("bAlreadySearched", Decoder::Bit)
            .prop("SearchingPawn", Decoder::IntPacked)]);
        let mut stream = NetStream::new(&schema, NetVersions::default());
        declare(&mut stream, 1, "/Game/Base.Base_C", 82, (80, "SearchingPawn"));
        declare(&mut stream, 2, "/Game/Child.Child_C", 82, (4, "RemoteRole"));
        declare(&mut stream, 3, "/Game/Other.Other_C", 86, (4, "RemoteRole"));
        let (base, child, other) = (stream.group_by_index[&1], stream.group_by_index[&2], stream.group_by_index[&3]);
        let searching_pawn = schema.groups[0].prop_index("SearchingPawn");
        assert_eq!(stream.export_for(base, 80), searching_pawn);
        assert_eq!(stream.export_for(child, 80), searching_pawn);
        // A different layout size is a different layout: no sharing.
        assert_eq!(stream.export_for(other, 80), None);
    }
}
