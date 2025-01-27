use std::{fmt::Debug, hash::Hash, ops::Sub};

use bevy::reflect::Reflect;

#[cfg(feature = "serde")]
use bevy::reflect::{ReflectDeserialize, ReflectSerialize};

use crate::resize_ne_bytes;

#[cfg(all(
    feature = "packed_rev_frame_size_1",
    not(feature = "packed_rev_frame_size_2"),
    not(feature = "packed_rev_frame_size_3"),
))]
const PACKED_REV_FRAME_SIZE: usize = 1;

#[cfg(all(
    not(feature = "packed_rev_frame_size_1"),
    feature = "packed_rev_frame_size_2",
    not(feature = "packed_rev_frame_size_3"),
))]
const PACKED_REV_FRAME_SIZE: usize = 2;

#[cfg(all(
    not(feature = "packed_rev_frame_size_1"),
    not(feature = "packed_rev_frame_size_2"),
    feature = "packed_rev_frame_size_3",
))]
const PACKED_REV_FRAME_SIZE: usize = 3;

#[cfg(not(any(
    feature = "packed_rev_frame_size_1",
    feature = "packed_rev_frame_size_2",
    feature = "packed_rev_frame_size_3",
)))]
const PACKED_REV_FRAME_SIZE: usize = 4;

/// Maximum value a frame can be internally, can be used as a bitmask that is 1 for all relevant bits.
pub(crate) const REV_FRAME_AS_U32_MAX: u32 = {
    let bits = PACKED_REV_FRAME_SIZE * 8;
    let shift = 32 - bits;
    u32::MAX >> shift
};

#[derive(Clone, Copy, PartialEq, Eq, Reflect, Hash)]
#[reflect(Debug)]
#[cfg_attr(feature = "serde", reflect(Serialize, Deserialize))]
pub struct RevFrame(pub(crate) u32);

#[derive(Clone, Copy, Reflect, PartialEq, Eq, Hash)]
#[reflect(Debug)]
#[cfg_attr(feature = "serde", reflect(Serialize, Deserialize))]
pub struct PackedRevFrame([u8; PACKED_REV_FRAME_SIZE]);

impl RevFrame {
    #[cfg(test)]
    pub(crate) const fn checked_new(value: u32) -> Self {
        assert!(value <= REV_FRAME_AS_U32_MAX);
        Self(value)
    }
    pub(crate) const fn wrapping_add(self, value: u32) -> Self {
        Self(self.0.wrapping_add(value) & REV_FRAME_AS_U32_MAX)
    }
    pub(crate) const fn wrapping_sub(self, value: u32) -> Self {
        Self(self.0.wrapping_sub(value) & REV_FRAME_AS_U32_MAX)
    }
    pub(crate) const fn first_half(self) -> bool {
        self.0 <= REV_FRAME_AS_U32_MAX / 2
    }
}

impl Sub for RevFrame {
    type Output = u32;
    fn sub(self, rhs: Self) -> Self::Output {
        self.0.wrapping_sub(rhs.0) & REV_FRAME_AS_U32_MAX
    }
}

impl Sub<PackedRevFrame> for RevFrame {
    type Output = u32;
    fn sub(self, rhs: PackedRevFrame) -> Self::Output {
        self - RevFrame::from(rhs)
    }
}

impl Sub for PackedRevFrame {
    type Output = u32;
    fn sub(self, rhs: Self) -> Self::Output {
        RevFrame::from(self) - RevFrame::from(rhs)
    }
}

impl Sub<RevFrame> for PackedRevFrame {
    type Output = u32;
    fn sub(self, rhs: RevFrame) -> Self::Output {
        RevFrame::from(self) - rhs
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
        Self(u32::from_ne_bytes(resize_ne_bytes(value.0)))
    }
}

impl From<RevFrame> for PackedRevFrame {
    fn from(value: RevFrame) -> Self {
        Self(resize_ne_bytes(value.0.to_ne_bytes()))
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

#[derive(Copy, Clone, Reflect, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    reflect(Serialize, Deserialize)
)]
pub struct RevFrameGen(u64);

#[derive(Copy, Clone, Reflect, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    reflect(Serialize, Deserialize)
)]
pub struct RevFrameNew(u64);

impl RevFrameNew {
    pub(crate) const GENERATION_STEP: u64 = (REV_FRAME_AS_U32_MAX as u64) + 1;
    pub const fn from_raw(raw: u64) -> Self {
        Self(raw)
    }
    pub fn from_parts(frame: PackedRevFrame, generation: RevFrameGen) -> Self {
        let frame = u64::from_ne_bytes(resize_ne_bytes(frame.0));
        Self(frame & generation.0)
    }
    pub(crate) const fn increase_frame(self) -> Self {
        Self(self.0 + 1)
    }
    pub(crate) const fn decrease_frame(self) -> Self {
        Self(self.0 - 1)
    }
    pub(crate) const fn increase_generation(self) -> Self {
        Self(self.0 + Self::GENERATION_STEP)
    }
    pub(crate) const fn sub_by_frames(self, rhs: u32) -> Self {
        Self(self.0 - rhs as u64)
    }
    pub(crate) const fn first_of_gen(self) -> bool {
        self.0 & (REV_FRAME_AS_U32_MAX as u64) == 0
    }
    pub(crate) const fn frame_raw(self) -> u32 {
        (self.0 & (REV_FRAME_AS_U32_MAX as u64)) as u32
    }
    pub const fn to_generation(self) -> RevFrameGen {
        RevFrameGen(self.generation_raw())
    }
    pub fn without_generation(self) -> PackedRevFrame {
        PackedRevFrame(resize_ne_bytes(self.0.to_ne_bytes()))
    }
    pub(crate) const fn generation_raw(self) -> u64 {
        self.0 & !(REV_FRAME_AS_U32_MAX as u64)
    }
    pub(crate) const fn raw(self) -> u64 {
        self.0
    }
    pub(crate) fn of_future_packed(self, frame: PackedRevFrame) -> Self {
        let lhs_frame = self.0 & (REV_FRAME_AS_U32_MAX as u64);
        let rhs_frame = u64::from_ne_bytes(resize_ne_bytes(frame.0));
        let mut generation = self.generation_raw();
        if rhs_frame < lhs_frame {
            generation += Self::GENERATION_STEP;
        }
        Self(rhs_frame & generation)
    }
    pub(crate) fn of_past_packed(self, frame: PackedRevFrame) -> Self {
        let lhs_frame = self.0 & (REV_FRAME_AS_U32_MAX as u64);
        let rhs_frame = u64::from_ne_bytes(resize_ne_bytes(frame.0));
        let mut generation = self.generation_raw();
        if rhs_frame > lhs_frame {
            generation -= Self::GENERATION_STEP;
        }
        Self(rhs_frame & generation)
    }
}

impl From<RevFrameNew> for PackedRevFrame {
    fn from(value: RevFrameNew) -> Self {
        Self(resize_ne_bytes(value.0.to_ne_bytes()))
    }
}

impl From<RevFrameNew> for RevFrameGen {
    fn from(value: RevFrameNew) -> Self {
        value.to_generation()
    }
}

impl Sub for RevFrameNew {
    type Output = u32;
    fn sub(self, rhs: Self) -> Self::Output {
        self.frame_raw() - rhs.frame_raw()
    }
}
