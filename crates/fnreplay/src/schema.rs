//! Declares which replicated classes and properties the stream reader should decode.
//!
//! Everything not listed here is skipped bit-exactly, because every property payload is
//! length-prefixed on the wire.

use crate::bits::{BitReader, Rotator, Vec3};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Decoder {
    Bit,
    U8,
    U16,
    U32,
    I32,
    F32,
    IntPacked,
    String,
    FName,
    /// The whole payload (up to 32 bits) as an unsigned integer. Handy for enums and
    /// bytes whose exact width is not worth hardcoding.
    UInt,
    PackedVector {
        scale: f64,
        max_bits: u32,
    },
    RotationShort,
    /// `FRepMovement`. Quantization levels are 0 = whole units, 1 = one decimal,
    /// 2 = two decimals, matching `EVectorQuantization`.
    Movement {
        location_level: u32,
        rotation_short: bool,
        velocity_level: u32,
    },
    /// `FUniqueNetIdRepl`: a player's online id, such as an Epic account id, as lowercase
    /// hex (or the plain string for ids sent unencoded).
    NetId,
    /// Counts as a change but is not decoded.
    Skip,
    /// A fast array field of a `_ClassNetCache` group. Every changed element is reported
    /// as its own update of the owning class, with the element id under
    /// [`FAST_ARRAY_ELEMENT`] and its properties decoded by the owning class's spec.
    FastArray,
}

/// Property name that carries a fast array element's id.
pub const FAST_ARRAY_ELEMENT: &str = "@element";

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct RepMovement {
    pub location: Vec3,
    pub rotation: Rotator,
    pub linear_velocity: Vec3,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    None,
    Bool(bool),
    UInt(u32),
    Int(i32),
    Float(f32),
    Str(String),
    Vector(Vec3),
    Rotator(Rotator),
    Movement(RepMovement),
}

impl Value {
    pub fn as_bool(&self) -> Option<bool> {
        match *self {
            Value::Bool(b) => Some(b),
            Value::UInt(v) => Some(v != 0),
            _ => None,
        }
    }

    pub fn as_u32(&self) -> Option<u32> {
        match *self {
            Value::UInt(v) => Some(v),
            Value::Int(v) => u32::try_from(v).ok(),
            Value::Bool(b) => Some(u32::from(b)),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_movement(&self) -> Option<&RepMovement> {
        match self {
            Value::Movement(m) => Some(m),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct PropSpec {
    pub name: String,
    pub decoder: Decoder,
}

#[derive(Debug, Clone, Default)]
pub struct GroupSpec {
    /// Caller-facing name, reported back in every event.
    pub name: String,
    /// Full class paths as they appear in the net field export table.
    pub paths: Vec<String>,
    /// Match `paths` as substrings instead of exact names.
    pub partial_path: bool,
    /// Static (level-placed) actor names after suffix cleanup, e.g. `Tiered_Chest_Athena`.
    pub static_actor_ids: Vec<String>,
    /// Names of stably named subobjects (default components such as
    /// `AbilitySystemComponent`) that replicate with this class's layout. Their content
    /// blocks carry the object, not its class, so the name is all there is to match on.
    pub subobject_names: Vec<String>,
    pub props: Vec<PropSpec>,
}

impl GroupSpec {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), ..Default::default() }
    }

    pub fn paths<I: IntoIterator<Item = S>, S: Into<String>>(mut self, paths: I) -> Self {
        self.paths = paths.into_iter().map(Into::into).collect();
        self
    }

    pub fn partial_path(mut self) -> Self {
        self.partial_path = true;
        self
    }

    pub fn static_actors<I: IntoIterator<Item = S>, S: Into<String>>(mut self, ids: I) -> Self {
        self.static_actor_ids = ids.into_iter().map(Into::into).collect();
        self
    }

    pub fn subobjects<I: IntoIterator<Item = S>, S: Into<String>>(mut self, names: I) -> Self {
        self.subobject_names = names.into_iter().map(Into::into).collect();
        self
    }

    pub fn prop(mut self, name: impl Into<String>, decoder: Decoder) -> Self {
        self.props.push(PropSpec { name: name.into(), decoder });
        self
    }

    pub(crate) fn prop_index(&self, name: &str) -> Option<usize> {
        self.props.iter().position(|p| p.name == name)
    }
}

#[derive(Debug, Clone, Default)]
pub struct Schema {
    pub groups: Vec<GroupSpec>,
}

impl Schema {
    pub fn new(groups: Vec<GroupSpec>) -> Self {
        Self { groups }
    }

    /// Finds the spec for an exported class path. First match wins, in declaration order.
    pub(crate) fn spec_for_path(&self, path: &str) -> Option<usize> {
        self.groups
            .iter()
            .position(|g| g.paths.iter().any(|p| if g.partial_path { path.contains(p.as_str()) } else { path == p }))
    }
}

/// `FUniqueNetIdRepl::NetSerialize`: a flags byte (bit 0 encoded, bit 1 empty, top five
/// bits the id type, 31 meaning its name follows), then the encoded bytes with their
/// length, or a string when not encoded.
fn read_net_id(r: &mut BitReader<'_>) -> Option<String> {
    const ENCODED: u8 = 1;
    const EMPTY: u8 = 2;
    const TYPE_BY_NAME: u8 = 31;
    let flags = r.read_u8();
    if flags & ENCODED != 0 && flags & EMPTY != 0 {
        return None;
    }
    if flags >> 3 == TYPE_BY_NAME {
        r.read_string();
    }
    let id = if flags & ENCODED != 0 {
        let len = usize::from(r.read_u8());
        r.read_bytes(len).iter().map(|b| format!("{b:02x}")).collect()
    } else {
        r.read_string()
    };
    (!id.is_empty() && !r.is_error()).then_some(id)
}

pub(crate) fn decode(decoder: Decoder, r: &mut BitReader<'_>) -> Value {
    let value = match decoder {
        Decoder::Bit => Value::Bool(r.read_bit()),
        Decoder::U8 => Value::UInt(u32::from(r.read_u8())),
        Decoder::U16 => Value::UInt(u32::from(r.read_u16())),
        Decoder::U32 => Value::UInt(r.read_u32()),
        Decoder::I32 => Value::Int(r.read_i32()),
        Decoder::F32 => Value::Float(r.read_f32()),
        Decoder::IntPacked => Value::UInt(r.read_int_packed()),
        Decoder::String => Value::Str(r.read_string()),
        Decoder::FName => Value::Str(r.read_fname()),
        Decoder::UInt => {
            let bits = r.bits_left().min(32) as u32;
            Value::UInt(r.read_bits_u32(bits))
        }
        Decoder::PackedVector { scale, max_bits } => Value::Vector(r.read_packed_vector(scale, max_bits)),
        Decoder::RotationShort => Value::Rotator(r.read_rotation_short()),
        Decoder::Movement { location_level, rotation_short, velocity_level } => {
            // Trailing fields vary between engine versions; a valid location is worth
            // keeping even when they do not line up.
            return read_rep_movement(r, location_level, rotation_short, velocity_level)
                .map_or(Value::None, Value::Movement);
        }
        Decoder::NetId => read_net_id(r).map_or(Value::None, Value::Str),
        Decoder::Skip | Decoder::FastArray => Value::None,
    };
    if r.is_error() {
        Value::None
    } else {
        value
    }
}

fn read_rep_movement(
    r: &mut BitReader<'_>,
    location_level: u32,
    rotation_short: bool,
    velocity_level: u32,
) -> Option<RepMovement> {
    let engine = r.versions.engine_network;
    let _simulated_physic_sleep = r.read_bit();
    let rep_physics = r.read_bit();
    let (server_frame, server_handle) =
        if engine >= 25 && engine != 26 { (r.read_bit(), r.read_bit()) } else { (false, false) };

    let packed = |r: &mut BitReader<'_>, level: u32| r.read_packed_vector(10f64.powi(level as i32), 24 + 3 * level);

    let location = packed(r, location_level);
    if r.is_error() {
        return None;
    }
    let mut movement = RepMovement { location, ..Default::default() };
    movement.rotation = if rotation_short { r.read_rotation_short() } else { r.read_rotation() };
    movement.linear_velocity = packed(r, velocity_level);
    if rep_physics {
        packed(r, velocity_level);
    }
    if server_frame {
        r.read_int_packed();
    }
    if server_handle {
        r.read_int_packed();
    }
    if engine >= 35 && r.read_bit() {
        packed(r, velocity_level);
    }
    if r.is_error() {
        movement.rotation = Default::default();
        movement.linear_velocity = Default::default();
    }
    Some(movement)
}
