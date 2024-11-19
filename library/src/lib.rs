/*
TODO:

- add license
- find new name
-- catchier and more unique to not use reversible-systems/schedules for comparable crates
-- "systems" in reverse: smetsys, can be altered to smet-sys or something
-- "schedules" in reverse: seludehcs (ew)
- should still be descriptive
-- rev..?
-- bevy_smetsys
-- bevy_yveb

Enhancements:
- reduce todo!() and //todo
- RareInitNoneLog tests
- pipe config tests
-- dummy systems in a_then_b einfügen und dummy system auch als ordnungssystem nutzen
-- dummy.pipe(sys) und sys.pipe(dummy) variationen
- impl Error for relevant structs

Docs
- documentations
-- point out determinism aspects of methods
-- log contract (always valid, may go further into the past)
-- check-logged-at should not be used as the sole shortening mechanism or else logs can grow larger than desired

ISSUES/DISCUSSIONS:
- Trigger::event_mut
-- mutations on the event are not reflectable in the log
- reversible change detection (copy over to new repo)
- analyze test schedule::non_exclusive_then_exclusive_ignore_deferred, consider revamping test strategy
- reversible entity commands, link to https://github.com/bevyengine/bevy/issues/15350 as blocker
- manual sync point configuration
-- apply_deferred
-- ScheduleBuildSettings::auto_insert_apply_deferred
- examples
*/

use std::{fmt::Debug, hash::Hash};

use bevy::{
    ecs::{component::Tick, schedule::ScheduleLabel},
    reflect::Reflect,
};

#[cfg(feature = "serde")]
use bevy::reflect::{ReflectDeserialize, ReflectSerialize};

pub mod app;
pub mod commands;
pub mod hook;
pub mod log;
pub mod meta;
pub mod observer;
pub mod schedule;
pub mod world;

/// Contains all extension traits `as _` and common types.
pub mod prelude {
    pub use crate::app::{RevApp as _, RevSystemsPlugin};
    pub use crate::commands::{RevCommands as _, RevEntityCommands as _};
    pub use crate::hook::HookDirection;
    pub use crate::meta::{RevDirection, RevMeta, VerifyingRevMeta};
    pub use crate::observer::RevEvent;
    pub use crate::schedule::{
        IntoRevSystemConfigs as _, IntoRevSystemSetConfigs as _, RevSchedule as _,
    };
    pub use crate::world::RevWorld as _;
    pub use crate::world::{RevDeferredWorld as _, RevEntityWorldMut as _, RevWorld as _};
    pub use crate::{PackedRevFrame, RevFrame, RevUpdate};
}

#[derive(Clone, Copy, PartialEq, Eq, Reflect, Hash)]
#[reflect(Debug)]
#[cfg_attr(feature = "serde", reflect(Serialize, Deserialize))]
pub struct RevFrame(u32);

impl From<RevFrame> for u32 {
    fn from(value: RevFrame) -> Self {
        value.0
    }
}

impl RevFrame {
    const fn new(value: u32) -> Self {
        debug_assert!(value <= PackedRevFrame::MAX_AS_U32);
        Self(value)
    }
    const fn wrapping_add(self, value: u32) -> Self {
        Self(self.0.wrapping_add(value) & PackedRevFrame::MAX_AS_U32)
    }
    const fn wrapping_sub(self, value: u32) -> Self {
        Self(self.0.wrapping_sub(value) & PackedRevFrame::MAX_AS_U32)
    }
    const fn first_half(self) -> bool {
        self.0 <= PackedRevFrame::MAX_AS_U32 / 2
    }
}

impl Debug for RevFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for RevFrame {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        PackedRevFrame::from(*self).serialize(serializer)
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for RevFrame {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        PackedRevFrame::deserialize(deserializer).map(Into::into)
    }
}

#[cfg(feature = "packed_rev_frame_size_1")]
const PACKED_REV_FRAME_SIZE: usize = 1;

#[cfg(feature = "packed_rev_frame_size_2")]
const PACKED_REV_FRAME_SIZE: usize = 2;

#[cfg(feature = "packed_rev_frame_size_3")]
const PACKED_REV_FRAME_SIZE: usize = 3;

#[cfg(not(any(
    feature = "packed_rev_frame_size_1",
    feature = "packed_rev_frame_size_2",
    feature = "packed_rev_frame_size_3",
)))]
const PACKED_REV_FRAME_SIZE: usize = 4;

#[derive(Clone, Copy, Reflect, PartialEq, Eq, Hash)]
#[reflect(Debug)]
#[cfg_attr(feature = "serde", reflect(Serialize, Deserialize))]
pub struct PackedRevFrame([u8; PACKED_REV_FRAME_SIZE]);

impl PackedRevFrame {
    const MAX_AS_U32: u32 = {
        let bits = PACKED_REV_FRAME_SIZE * 8;
        let shift = 32 - bits;
        u32::MAX >> shift
    };
}

impl From<PackedRevFrame> for u32 {
    fn from(value: PackedRevFrame) -> Self {
        let mut i = value.0.into_iter();
        u32::from_le_bytes(std::array::from_fn(|_| i.next().unwrap_or(0)))
    }
}

impl Debug for PackedRevFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        u32::from(*self).fmt(f)
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for PackedRevFrame {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        u32::from(*self).serialize(serializer)
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for PackedRevFrame {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value: u32 = u32::deserialize(deserializer)?;
        if value <= Self::MAX_AS_U32 {
            let mut i = value.to_le_bytes().into_iter();
            Ok(Self(std::array::from_fn(|_| i.next().unwrap_or(0))))
        } else {
            Err(serde::de::Error::custom(format!(
                // todo update after changing to u32
                "{value} does not fit into {} bytes, cannot map this value to `PackedRevFrame` \
                on this machine, increase the `time_bytes_*` feature of the reversible_systems \
                crate to the value of the source where this value was serialized or, if the \
                source does not use that feature, change that to a value low enough to be \
                supported by all machines",
                PACKED_REV_FRAME_SIZE,
            )))
        }
    }
}

impl From<PackedRevFrame> for RevFrame {
    fn from(value: PackedRevFrame) -> Self {
        RevFrame(u32::from_le_bytes(value.0))
    }
}

impl From<RevFrame> for PackedRevFrame {
    fn from(value: RevFrame) -> Self {
        // RevFrame is only constructed from u32 <= PackedRevFrame::MAX_AS_U32
        let mut i = value.0.to_le_bytes().into_iter();
        Self(std::array::from_fn(|_| i.next().unwrap_or(0)))
    }
}

impl PartialEq<RevFrame> for PackedRevFrame {
    fn eq(&self, other: &RevFrame) -> bool {
        let this: RevFrame = (*self).into();
        this.eq(other)
    }
}

impl PartialEq<PackedRevFrame> for RevFrame {
    fn eq(&self, other: &PackedRevFrame) -> bool {
        let other: RevFrame = (*other).into();
        self.eq(&other)
    }
}

#[derive(ScheduleLabel, Copy, Clone, Debug, Hash, PartialEq, Eq)]
pub struct RevUpdate;

/// reference: Tick::check_tick
// todo: move to module this is using if there ends up being only one
fn check_tick(this: &mut Tick, change_tick: Tick) {
    let age = change_tick.get().wrapping_sub(this.get());
    if age > Tick::MAX.get() {
        *this = Tick::new(change_tick.get().wrapping_sub(Tick::MAX.get()));
    }
}

macro_rules! error_per_flag {
    ($flag:expr, $($arg:tt)+) => ({
        if !*$flag {
            bevy::utils::tracing::error!($($arg)+);
            *$flag = true;
        }
        core::default::Default::default()
    });
}

use error_per_flag;
