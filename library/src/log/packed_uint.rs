//! This module contains unsigned integer types that all have an alignment of `1`.
//!
//! When reversible logs are used extensively and the global log spans over many frames,
//! padding bytes might make up a significant portion of the app's memory usage.
//!
//! Padding bytes are added by the compiler if types with different alignments are combined
//! into a new type. For example logging a `(usize, u8)` may consume the memory of two usizes.
//! See [Rust's type layout](https://doc.rust-lang.org/reference/type-layout.html).
//!
//! If a `(PackedUSize, u8)` is stored instead, no additional padding bytes are added,
//! allowing storing more log entries into the same space at a small performance cost.
//!
//! If these integers are combined with a more complex `T`, consider to apply comparable
//! approaches to lower the alignment like `#[repr(packed)]`.
//!
//! Besides the tighter packing, uncommon integer sizes in this module (like [`PackedU24`])
//! allow picking the minimal amount of bytes for the desired number range.
//!
//! This module contains no `PackedU8` because `u8` already has an alignment of `1`.
//!
//! This module also contains no integer types larger than [`PackedU56`]/[`PackedUSize`]
//! because the intended usage here are integers that can be transformed from and to `usize`,
//! mainly to be used as the `Amount` generic of log types that store multiple transitions/values
//! per push. These types usually have a [`PackedUSize`] as the default `Amount` type to work
//! well in more cases where a non-ZST per-push type `U` is specified.
//!
//! To opt out of this strategy, for example if `U` remains `()`, the generic `Amount` can be
//! manually set to `usize`.

use std::{fmt::Debug, iter::Sum, ops::Add};

use bevy::reflect::{std_traits::ReflectDefault, Reflect};

#[cfg(feature = "serde")]
use bevy::reflect::prelude::{ReflectDeserialize, ReflectSerialize};

const USIZE_BYTES: usize = usize::BITS as usize / 8;

/// A packed variant of `u16` to be used for example as the `Amount` generic of log structs.
///
/// Other than `u16` this type has an alignment of `1` so less padding is added to structs
/// that combine `PackedU16` with other types that have an alignment of `1` themselves.
#[derive(Clone, Copy, PartialEq, Eq, Default, Reflect)]
#[reflect(Default, Debug)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    reflect(Serialize, Deserialize)
)]
pub struct PackedU16([u8; 2]);

/// A packed, unsigned 24 bit integer to be used for example as the `Amount` generic of log structs.
///
/// This type has an alignment of `1` and can store values from `0` to up to `16777215`.
#[derive(Clone, Copy, PartialEq, Eq, Default, Reflect)]
#[reflect(Default, Debug)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    reflect(Serialize, Deserialize)
)]
pub struct PackedU24([u8; 3]);

/// A packed variant of `u32` to be used for example as the `Amount` generic of log structs.
///
/// Other than `u32` this type has an alignment of `1` so less padding is added to structs
/// that combine `PackedU32` with other types that have an alignment less than `4` themselves.
#[derive(Clone, Copy, PartialEq, Eq, Default, Reflect)]
#[reflect(Default, Debug)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    reflect(Serialize, Deserialize)
)]
pub struct PackedU32([u8; 4]);

/// A packed, unsigned 40 bit integer to be used for example as the `Amount` generic of log structs.
///
/// This type has an alignment of `1` and can store values from `0` to up to `1099511627775`.
#[derive(Clone, Copy, PartialEq, Eq, Default, Reflect)]
#[reflect(Default, Debug)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    reflect(Serialize, Deserialize)
)]
pub struct PackedU40([u8; 5]);

/// A packed, unsigned 48 bit integer to be used for example as the `Amount` generic of log structs.
///
/// This type has an alignment of `1` and can store values from `0` to up to `281474976710655`.
#[derive(Clone, Copy, PartialEq, Eq, Default, Reflect)]
#[reflect(Default, Debug)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    reflect(Serialize, Deserialize)
)]
pub struct PackedU48([u8; 6]);

/// A packed, unsigned 56 bit integer to be used for example as the `Amount` generic of log structs.
///
/// This type has an alignment of `1` and can store values from `0` to up to `72057594037927935`.
#[derive(Clone, Copy, PartialEq, Eq, Default, Reflect)]
#[reflect(Default, Debug)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    reflect(Serialize, Deserialize)
)]
pub struct PackedU56([u8; 7]);

/// A packed variant of `usize` to be used for example as the `Amount` generic of log structs.
///
/// Other than `usize` this type has an alignment of `1` so less padding is added to structs
/// that combine `PackedUSize` with other types that have an alignment less than `8` themselves.
#[derive(Clone, Copy, PartialEq, Eq, Default, Reflect)]
#[reflect(Default, Debug)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    reflect(Serialize, Deserialize)
)]
pub struct PackedUSize([u8; USIZE_BYTES]);

fn resize_le_bytes<const M: usize, const N: usize>(le_bytes: [u8; M]) -> [u8; N] {
    let mut i = le_bytes.iter().copied().chain(std::iter::repeat(0));
    std::array::from_fn(|_| i.next().unwrap())
}

fn le_bytes_to_usize<const N: usize>(le_bytes: [u8; N]) -> usize {
    usize::from_le_bytes(resize_le_bytes(le_bytes))
}

fn try_usize_to_le_bytes<const N: usize, Out>(
    value: usize,
    map: impl FnOnce([u8; N]) -> Out,
) -> Result<Out, usize> {
    let limit = if N < USIZE_BYTES {
        (1usize << (8 * N)) - 1
    } else {
        usize::MAX
    };

    if value <= limit {
        Ok(map(resize_le_bytes(value.to_le_bytes())))
    } else {
        Err(value)
    }
}

impl Into<usize> for PackedU16 {
    fn into(self) -> usize {
        le_bytes_to_usize(self.0)
    }
}

/// Converts `PackedU24` to `usize`.
///
/// As this type can only be constructed from `usize` and is otherwise immutable,
/// it is expected that the inner value does not exceed `usize::MAX`.
///
/// If dispite this that is not the case, this conversation is lossy and higher bits are truncated.
impl Into<usize> for PackedU24 {
    fn into(self) -> usize {
        le_bytes_to_usize(self.0)
    }
}

/// Converts `PackedU32` to `usize`.
///
/// As this type can only be constructed from `usize` and is otherwise immutable,
/// it is expected that the inner value does not exceed `usize::MAX`.
///
/// If dispite this that is not the case, this conversation is lossy and higher bits are truncated.
impl Into<usize> for PackedU32 {
    fn into(self) -> usize {
        le_bytes_to_usize(self.0)
    }
}

/// Converts `PackedU40` to `usize`.
///
/// As this type can only be constructed from `usize` and is otherwise immutable,
/// it is expected that the inner value does not exceed `usize::MAX`.
///
/// If dispite this that is not the case, this conversation is lossy and higher bits are truncated.
impl Into<usize> for PackedU40 {
    fn into(self) -> usize {
        le_bytes_to_usize(self.0)
    }
}

/// Converts `PackedU48` to `usize`.
///
/// As this type can only be constructed from `usize` and is otherwise immutable,
/// it is expected that the inner value does not exceed `usize::MAX`.
///
/// If dispite this that is not the case, this conversation is lossy and higher bits are truncated.
impl Into<usize> for PackedU48 {
    fn into(self) -> usize {
        le_bytes_to_usize(self.0)
    }
}

/// Converts `PackedU56` to `usize`.
///
/// As this type can only be constructed from `usize` and is otherwise immutable,
/// it is expected that the inner value does not exceed `usize::MAX`.
///
/// If dispite this that is not the case, this conversation is lossy and higher bits are truncated.
impl Into<usize> for PackedU56 {
    fn into(self) -> usize {
        le_bytes_to_usize(self.0)
    }
}

impl Into<usize> for PackedUSize {
    fn into(self) -> usize {
        usize::from_le_bytes(self.0)
    }
}

impl TryFrom<usize> for PackedU16 {
    type Error = usize;
    fn try_from(value: usize) -> Result<Self, Self::Error> {
        try_usize_to_le_bytes(value, Self)
    }
}

impl TryFrom<usize> for PackedU24 {
    type Error = usize;
    fn try_from(value: usize) -> Result<Self, Self::Error> {
        try_usize_to_le_bytes(value, Self)
    }
}

impl TryFrom<usize> for PackedU32 {
    type Error = usize;
    fn try_from(value: usize) -> Result<Self, Self::Error> {
        try_usize_to_le_bytes(value, Self)
    }
}

impl TryFrom<usize> for PackedU40 {
    type Error = usize;
    fn try_from(value: usize) -> Result<Self, Self::Error> {
        try_usize_to_le_bytes(value, Self)
    }
}

impl TryFrom<usize> for PackedU48 {
    type Error = usize;
    fn try_from(value: usize) -> Result<Self, Self::Error> {
        try_usize_to_le_bytes(value, Self)
    }
}

impl TryFrom<usize> for PackedU56 {
    type Error = usize;
    fn try_from(value: usize) -> Result<Self, Self::Error> {
        try_usize_to_le_bytes(value, Self)
    }
}

impl From<usize> for PackedUSize {
    fn from(value: usize) -> Self {
        Self(value.to_le_bytes())
    }
}

impl PartialEq<usize> for PackedUSize {
    fn eq(&self, other: &usize) -> bool {
        let as_usize: usize = (*self).into();
        as_usize.eq(other)
    }
}

impl PartialOrd<usize> for PackedUSize {
    fn partial_cmp(&self, other: &usize) -> Option<std::cmp::Ordering> {
        let as_usize: usize = (*self).into();
        as_usize.partial_cmp(other)
    }
}

impl Sum<PackedUSize> for usize {
    fn sum<I: Iterator<Item = PackedUSize>>(iter: I) -> Self {
        iter.map(Into::<usize>::into).sum()
    }
}

impl Add for PackedUSize {
    type Output = usize;
    fn add(self, rhs: Self) -> Self::Output {
        let self_usize: usize = self.into();
        let rhs_usize: usize = rhs.into();
        self_usize + rhs_usize
    }
}

impl Debug for PackedU16 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PackedU16({})", Into::<usize>::into(*self))
    }
}

impl Debug for PackedU24 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PackedU24({})", Into::<usize>::into(*self))
    }
}

impl Debug for PackedU32 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PackedU32({})", Into::<usize>::into(*self))
    }
}

impl Debug for PackedU40 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PackedU40({})", Into::<usize>::into(*self))
    }
}

impl Debug for PackedU48 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PackedU48({})", Into::<usize>::into(*self))
    }
}

impl Debug for PackedU56 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PackedU56({})", Into::<usize>::into(*self))
    }
}

impl Debug for PackedUSize {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "PackedUSize({})", Into::<usize>::into(*self))
    }
}

impl PackedU16 {
    pub const MIN: Self = Self([0; 2]);
}

impl PackedU24 {
    pub const MIN: Self = Self([0; 3]);
}

impl PackedU32 {
    pub const MIN: Self = Self([0; 4]);
}

impl PackedU40 {
    pub const MIN: Self = Self([0; 5]);
}

impl PackedU48 {
    pub const MIN: Self = Self([0; 6]);
}

impl PackedU56 {
    pub const MIN: Self = Self([0; 7]);
}

impl PackedUSize {
    pub const MIN: Self = Self([0; USIZE_BYTES]);
}
