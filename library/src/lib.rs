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
- impl Error for relevant structs/enums via thiserror
- #[inline]s
- examples
-- folder next to src
--- how to have cargo check look at it?
--- add CI
-- hooks https://github.com/bevyengine/bevy/blob/main/examples/ecs/component_hooks.rs
-- observers https://github.com/bevyengine/bevy/blob/main/examples/ecs/observers.rs
-- ecs, commands https://github.com/bevyengine/bevy/blob/main/examples/ecs/ecs_guide.rs

Docs
- documentations
-- point out determinism aspects of methods
-- log contract (always valid, may go further into the past)
-- check-logged-at should not be used as the sole shortening mechanism or else logs can grow larger than desired

ISSUES/DISCUSSIONS:
- reversible change detection (copy over to new repo)
- analyze test schedule::non_exclusive_then_exclusive_ignore_deferred, consider revamping test strategy
- reversible entity commands, link to https://github.com/bevyengine/bevy/issues/15350 as blocker
- manual sync point configuration
-- apply_deferred
-- ScheduleBuildSettings::auto_insert_apply_deferred
- rare_init_none
*/

use std::{fmt::Debug, hash::Hash};

use bevy::{ecs::schedule::ScheduleLabel, reflect::Reflect};

#[cfg(feature = "serde")]
use bevy::reflect::{ReflectDeserialize, ReflectSerialize};

pub mod app;
pub mod commands;
pub mod log;
pub mod meta;
pub mod schedule;
//pub mod world;

/// Contains all extension traits `as _` and common types.
pub mod prelude {
    pub use crate::app::{RevApp as _, RevSystemsPlugin};
    pub use crate::commands::{BuffersUndoRedo as _, RevCommands as _, UndoRedoBuffer};
    pub use crate::meta::{RevDirection, RevMeta, VerifyingRevMeta};
    pub use crate::schedule::{
        IntoRevSystemConfigs as _, IntoRevSystemSetConfigs as _, RevSchedule as _,
    };
    pub use crate::{PackedRevFrame, RevFrame, RevUpdate};
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

const REV_FRAME_AS_U32_MAX: u32 = {
    let bits = PACKED_REV_FRAME_SIZE * 8;
    let shift = 32 - bits;
    u32::MAX >> shift
};

#[derive(Clone, Copy, PartialEq, Eq, Reflect, Hash)]
#[reflect(Debug)]
#[cfg_attr(feature = "serde", reflect(Serialize, Deserialize))]
pub struct RevFrame(u32);

#[derive(Clone, Copy, Reflect, PartialEq, Eq, Hash)]
#[reflect(Debug)]
#[cfg_attr(feature = "serde", reflect(Serialize, Deserialize))]
pub struct PackedRevFrame([u8; PACKED_REV_FRAME_SIZE]);

impl RevFrame {
    #[cfg(test)]
    const fn checked_new(value: u32) -> Self {
        assert!(value <= REV_FRAME_AS_U32_MAX);
        Self(value)
    }
    const fn wrapping_add(self, value: u32) -> Self {
        Self(self.0.wrapping_add(value) & REV_FRAME_AS_U32_MAX)
    }
    const fn wrapping_sub(self, value: u32) -> Self {
        Self(self.0.wrapping_sub(value) & REV_FRAME_AS_U32_MAX)
    }
    const fn first_half(self) -> bool {
        self.0 <= REV_FRAME_AS_U32_MAX / 2
    }
}

impl From<RevFrame> for u32 {
    fn from(value: RevFrame) -> Self {
        value.0
    }
}

impl From<PackedRevFrame> for u32 {
    fn from(value: PackedRevFrame) -> Self {
        RevFrame::from(value).0
    }
}

impl From<PackedRevFrame> for RevFrame {
    fn from(value: PackedRevFrame) -> Self {
        Self(u32::from_le_bytes(resize_le_bytes(value.0)))
    }
}

impl From<RevFrame> for PackedRevFrame {
    fn from(value: RevFrame) -> Self {
        Self(resize_le_bytes(value.0.to_le_bytes()))
    }
}

/// Assumes cut-off bytes, if any, are `0`.
#[inline(always)]
fn resize_le_bytes<const N: usize, const M: usize>(arr: [u8; N]) -> [u8; M] {
    let mut i = arr.into_iter();
    std::array::from_fn(|_| i.next().unwrap_or(0))
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

impl Debug for RevFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl Debug for PackedRevFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        u32::from(*self).fmt(f)
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for RevFrame {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        PackedRevFrame::from(*self).serialize(serializer)
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for PackedRevFrame {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        u32::from(*self).serialize(serializer)
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for RevFrame {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        PackedRevFrame::deserialize(deserializer).map(Into::into)
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for PackedRevFrame {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value: u32 = u32::deserialize(deserializer)?;
        if value <= REV_FRAME_AS_U32_MAX {
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

#[derive(ScheduleLabel, Copy, Clone, Debug, Hash, PartialEq, Eq)]
pub struct RevUpdate;

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
