//! # Log structures
//!
//! The logs in this module can be used for custom standalone loggings but offer additional methods to keep their length at a
//! minimum if they are combined with [`OnePerFrame`], [`NPerFrame`] and [`WithTimestamp`]. The following table shows the
//! individual scenarios where each of the combinations are suggested if memory usage is a concern.
//!
//! If performance is more important, it might make more sense to use [`TransitionLog<WithTimestamp<Option<T>>>`] for example
//! than [`RareTransitionLog<OnePerFrame<T>>`]. Both are updated with `Option<T>`, but the first stores them more compact,
//! depending on how rarely a `Some` occurs. But this comes with a little overhead at accessing the logged data again.
//!
//! # Available logs and use cases
//!
//! | data per push        | pushes per frame | suggested log struct |
//! |----------------------|------------------|----------------------|
//! | 1                    | constant         | [`TransitionLog<T>`] |
//! | 1                    | varying          | [`TransitionLog<WithTimestamp<T>>`] |
//! | 0 or rarely 1        | constant         | [`RareTransitionLog<T>`] |
//! | 0 or rarely 1        | varying          | [`RareTransitionLog<WithTimestamp<T>>`] |
//! | varying              | constant         | [`TransitionsLog<T, U, Amount>`] |
//! | varying              | varying          | [`TransitionsLog<T, WithTimestamp<U>, Amount>`] |
//! | 0 or rarely varying  | constant         | [`RareTransitionsLog<T,U>`] |
//! | 0 or rarely varying  | varying          | [`RareTransitionsLog<T, WithTimestamp<U>>`] |
//!
//! - For constant `M` amounts of data per push, refer to "data per push": 1 and `[T; M]` as the logged type.
//! - `U` is an optional `Copy` type that can be stored with each push of multiple data. Default is `()`.
//! - `Amount` is the integer type to store the amount of data in each push, allowing memory optimizations. Default is `usize`.
//!
//! # Considerations and alternatives
//!
//! When storing `bool` or other types that can represent two states and one state occurs much more frequently, it makes sense
//! to use [`RareTransitionLog<()>`] and map the `Option<()>` into the desired type where `None` is the more
//! frequent state of the type.
//!
//! If the goal is to just sometimes push into the log, it might be benefitial if the call of `log.push_presence(value)` itself
//! is what happens not every frame instead of wrapping the value into an `Option` while using a [`RareTransitionLog`]. This
//! becomes possible if the condition for this decision is also available during log logic where `log.forward_log()` or
//! `log.backward_log()` must or must not be called. Then [`TransitionLog`] can be used instead which is less memory-consuming and
//! has less logic overhead.

use std::{collections::VecDeque, fmt::Debug, iter::{FusedIterator, Sum}, ops::Add};

mod rare_transition;
mod rare_transitions;
mod rare_value;
mod rare_values;
mod transition;
mod transitions;
mod value;
mod values;

use bevy::reflect::Reflect;
pub use rare_transition::RareTransitionLog;
pub use rare_transitions::RareTransitionsLog;
pub use rare_value::RareValueLog;
pub use transition::TransitionLog;
pub use transitions::TransitionsLog;
pub use value::ValueLog;
pub use values::ValuesLog;

use crate::meta::RevMeta;

use bevy::reflect::std_traits::ReflectDefault;

#[cfg(feature = "serde")]
use bevy::reflect::prelude::{ReflectSerialize, ReflectDeserialize};

pub trait LogIter<'a, T>:
    Iterator<Item = T> + DoubleEndedIterator + ExactSizeIterator + FusedIterator
{
}

impl<T, I: Iterator<Item = T> + DoubleEndedIterator + ExactSizeIterator + FusedIterator>
    LogIter<'_, T> for I
{
}

#[derive(Debug, Clone, PartialEq)]
pub struct OutOfLog;

pub struct LogMut<'a, T>(&'a mut VecDeque<T>);

impl<'a, T> LogMut<'a, T> {
    pub fn append(&mut self, other: &mut VecDeque<T>) {
        self.0.append(other);
    }
    pub fn push_back(&mut self, data: T) {
        self.0.push_back(data);
    }
}

impl<'a, T> Extend<&'a T> for LogMut<'a, T>
where
    T: 'a + Copy,
{
    fn extend<I: IntoIterator<Item = &'a T>>(&mut self, iter: I) {
        self.0.extend(iter);
    }
}

impl<'a, T> Extend<T> for LogMut<'a, T> {
    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        self.0.extend(iter);
    }
}

#[derive(Debug)]
pub struct AmountErr<I, U, Amount: TryFrom<usize> = usize> {
    pub data: I,
    pub entry: U,
    pub err: Amount::Error,
}

#[derive(Debug, Clone)]
pub struct DataEntry<T, U> {
    pub data: T,
    pub entry: U,
}

impl<'a, T: Iterator, U> IntoIterator for &'a mut DataEntry<T, U> {
    type IntoIter = &'a mut T;
    type Item = T::Item;
    fn into_iter(self) -> Self::IntoIter {
        &mut self.data
    }
}

/// Call `update` of a log with this struct up to one time per reversible frame.
///
/// This will enable a cleanup strategy where entries are forgotten that are older than the global log start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize)
)]
pub struct WithTimestamp<T = ()> {
    pub data: T,
    pub logged_at: PackedUSize, // todo: type-alias [u8;?] depending on pointer width to deprecate Packed
}

impl<T: Default> From<usize> for WithTimestamp<T> {
    fn from(logged_at: usize) -> Self {
        Self {
            data: T::default(),
            logged_at: logged_at.into(),
        }
    }
}

impl<T: Default> From<&RevMeta> for WithTimestamp<T> {
    fn from(meta: &RevMeta) -> Self {
        Self {
            data: T::default(),
            logged_at: meta.now().into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize)
)]
struct RareData<T> {
    data: T,
    /// If `T` is a transiton, then this is the skips before the transition.
    ///
    /// If `T` is a value, then this is the skips after the value
    skips: PackedUSize, // todo: type-alias [u8;?] depending on pointer width to deprecate Packed
}

impl<T> RareData<T> {
    fn len(&self) -> usize {
        let skips: usize = self.skips.into();
        skips + 1
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize)
)]
struct WithAmount<U = (), Amount = PackedUSize> {
    entry: U,
    amount: Amount,
}

impl<U, Amount> WithAmount<U, Amount>
where
    Amount: TryFrom<usize, Error: Debug> + Into<usize> + Default + Copy,
{
    fn zero(entry: U) -> Self {
        let amount = 0usize
            .try_into()
            .expect("expects 0 to be representable by Amount");
        WithAmount {
            entry,
            amount,
        }
    }
    fn amount(&self) -> usize {
        self.amount.into()
    }
}

const BACKWARD_EXPECT_MSG: &'static str = "self.index should always be <= the log len, so reducing it without underflow is expected to result in a valid index into the log";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Reflect)]
#[reflect(Default)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    reflect(Serialize, Deserialize)
)]
pub struct PackedU8([u8;1]);

#[cfg(any(
    target_pointer_width = "32",
    target_pointer_width = "64"
))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Reflect)]
#[reflect(Default)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    reflect(Serialize, Deserialize)
)]
pub struct PackedU16([u8;2]);

#[cfg(any(
    target_pointer_width = "32",
    target_pointer_width = "64"
))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Reflect)]
#[reflect(Default)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    reflect(Serialize, Deserialize)
)]
pub struct PackedU24([u8;3]);

#[cfg(target_pointer_width = "64")]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Reflect)]
#[reflect(Default)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    reflect(Serialize, Deserialize)
)]
pub struct PackedU32([u8;4]);

#[cfg(target_pointer_width = "64")]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Reflect)]
#[reflect(Default)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    reflect(Serialize, Deserialize)
)]
pub struct PackedU40([u8;5]);

#[cfg(target_pointer_width = "64")]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Reflect)]
#[reflect(Default)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    reflect(Serialize, Deserialize)
)]
pub struct PackedU48([u8;6]);

#[cfg(target_pointer_width = "64")]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Reflect)]
#[reflect(Default)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    reflect(Serialize, Deserialize)
)]
pub struct PackedU56([u8;7]);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Reflect)]
#[reflect(Default)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    reflect(Serialize, Deserialize)
)]
pub struct PackedUSize([u8; (usize::BITS as usize) / 8]);

/// # Panics
/// Panics if `N` > `size_of::<usize>()`
fn le_bytes_to_usize<const N: usize>(le_bytes: [u8; N]) -> usize {
    let mut bytes = 0usize.to_le_bytes();
    let slice = &mut bytes[..N];
    slice.copy_from_slice(&le_bytes);
    usize::from_le_bytes(bytes)
}

impl Into<usize> for PackedU8 {
    fn into(self) -> usize {
        le_bytes_to_usize(self.0)
    }
}

#[cfg(any(
    target_pointer_width = "32",
    target_pointer_width = "64"
))]
impl Into<usize> for PackedU16 {
    fn into(self) -> usize {
        le_bytes_to_usize(self.0)
    }
}

#[cfg(any(
    target_pointer_width = "32",
    target_pointer_width = "64"
))]
impl Into<usize> for PackedU24 {
    fn into(self) -> usize {
        le_bytes_to_usize(self.0)
    }
}

#[cfg(target_pointer_width = "64")]
impl Into<usize> for PackedU32 {
    fn into(self) -> usize {
        le_bytes_to_usize(self.0)
    }
}

#[cfg(target_pointer_width = "64")]
impl Into<usize> for PackedU40 {
    fn into(self) -> usize {
        le_bytes_to_usize(self.0)
    }
}

#[cfg(target_pointer_width = "64")]
impl Into<usize> for PackedU48 {
    fn into(self) -> usize {
        le_bytes_to_usize(self.0)
    }
}

#[cfg(target_pointer_width = "64")]
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

fn try_usize_to_le_bytes<const N: usize, Out>(value: usize, map: impl FnOnce([u8; N]) -> Out) -> Result<Out, usize> {
    let limit = if N < std::mem::size_of::<usize>() {
        (1usize << (8 * N)) - 1
    } else {
        usize::MAX
    };

    if value <= limit {
        let bytes = value.to_le_bytes();
        let arr = std::array::from_fn(|i| *bytes.get(i).unwrap_or(&0));
        Ok(map(arr))
    } else {
        Err(value)
    }
}

impl TryFrom<usize> for PackedU8 {
    type Error = usize;
    fn try_from(value: usize) -> Result<Self, Self::Error> {
        try_usize_to_le_bytes(value, Self)
    }
}

#[cfg(any(
    target_pointer_width = "32",
    target_pointer_width = "64"
))]
impl TryFrom<usize> for PackedU16 {
    type Error = usize;
    fn try_from(value: usize) -> Result<Self, Self::Error> {
        try_usize_to_le_bytes(value, Self)
    }
}

#[cfg(any(
    target_pointer_width = "32",
    target_pointer_width = "64"
))]
impl TryFrom<usize> for PackedU24 {
    type Error = usize;
    fn try_from(value: usize) -> Result<Self, Self::Error> {
        try_usize_to_le_bytes(value, Self)
    }
}

#[cfg(target_pointer_width = "64")]
impl TryFrom<usize> for PackedU32 {
    type Error = usize;
    fn try_from(value: usize) -> Result<Self, Self::Error> {
        try_usize_to_le_bytes(value, Self)
    }
}

#[cfg(target_pointer_width = "64")]
impl TryFrom<usize> for PackedU40 {
    type Error = usize;
    fn try_from(value: usize) -> Result<Self, Self::Error> {
        try_usize_to_le_bytes(value, Self)
    }
}

#[cfg(target_pointer_width = "64")]
impl TryFrom<usize> for PackedU48 {
    type Error = usize;
    fn try_from(value: usize) -> Result<Self, Self::Error> {
        try_usize_to_le_bytes(value, Self)
    }
}

#[cfg(target_pointer_width = "64")]
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
        iter.map(|this| {
            let this: usize = this.into();
            this
        })
        .sum()
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

impl PackedU8 {
    pub const MIN: Self = Self([u8::MIN; 1]);
    pub const MAX: Self = Self([u8::MAX; 1]);
}

#[cfg(any(
    target_pointer_width = "32",
    target_pointer_width = "64"
))]
impl PackedU16 {
    pub const MIN: Self = Self([u8::MIN; 2]);
    pub const MAX: Self = Self([u8::MAX; 2]);
}

#[cfg(any(
    target_pointer_width = "32",
    target_pointer_width = "64"
))]
impl PackedU24 {
    pub const MIN: Self = Self([u8::MIN; 3]);
    pub const MAX: Self = Self([u8::MAX; 3]);
}

#[cfg(target_pointer_width = "64")]
impl PackedU32 {
    pub const MIN: Self = Self([u8::MIN; 4]);
    pub const MAX: Self = Self([u8::MAX; 4]);
}

#[cfg(target_pointer_width = "64")]
impl PackedU40 {
    pub const MIN: Self = Self([u8::MIN; 5]);
    pub const MAX: Self = Self([u8::MAX; 5]);
}

#[cfg(target_pointer_width = "64")]
impl PackedU48 {
    pub const MIN: Self = Self([u8::MIN; 6]);
    pub const MAX: Self = Self([u8::MAX; 6]);
}

#[cfg(target_pointer_width = "64")]
impl PackedU56 {
    pub const MIN: Self = Self([u8::MIN; 7]);
    pub const MAX: Self = Self([u8::MAX; 7]);
}

impl PackedUSize {
    pub const MIN: Self = Self([u8::MIN; (usize::BITS as usize) / 8]);
    pub const MAX: Self = Self([u8::MAX; (usize::BITS as usize) / 8]);
}


/* 
#[derive(Debug, Clone, Copy, Reflect)]
#[reflect(Debug)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    reflect(Serialize, Deserialize)
)]
struct PackedUInt<const N: usize>([u8; N]) where Self: TryFrom<usize> + Serialize; 

impl<const N: usize> Default for PackedUInt<N> where Self: TryFrom<usize> {
    fn default() -> Self {
        Self([0; N])
    }
}

impl<const N: usize> Into<usize> for PackedUInt<N> where Self: TryFrom<usize> {
    fn into(self) -> usize {
        let mut bytes = 0usize.to_le_bytes();
        let slice = &mut bytes[..N];
        slice.copy_from_slice(&self.0);
        usize::from_le_bytes(bytes)
    }
}

impl TryFrom<usize> for PackedUInt<1> {
    type Error = usize;
    fn try_from(value: usize) -> Result<Self, Self::Error> {
        match u8::try_from(value) {
            Ok(byte) => Ok(Self([byte])),
            Err(_) => Err(value)
        }
    }
}

#[cfg(target_pointer_width = "16")]
impl From<usize> for PackedUInt<2> {
    fn from(value: usize) -> Self {
        let [b0, b1] = value.to_be_bytes();
        Self([b0, b1])
    }
}

#[cfg(not(target_pointer_width = "16"))]
impl TryFrom<usize> for PackedUInt<2> {
    type Error = usize;
    fn try_from(value: usize) -> Result<Self, Self::Error> {
        match value.to_be_bytes() {
            [b0, b1, 0, ..] => Ok(Self([b0, b1])),
            _ => Err(value)
        }        
    }
}
*/





/* 
pub trait PackedUInt: TryFrom<usize, Error: Debug> + TryInto<usize, Error: Debug> + Default + Copy {
    #[cfg(feature = "serde")]
    type Bytes: Reflect + serde::Serialize + for<'a> Deserialize<'a>; //correct bound?
    #[cfg(not(feature = "serde"))]
    type Bytes: Reflect;
    fn to_le_bytes(this: Self) -> Self::Bytes;
    fn from_le_bytes(bytes: Self::Bytes) -> Self;
}

impl PackedUInt for u8 {
    type Bytes = u8;
    fn to_le_bytes(this: Self) -> Self::Bytes {
        this
    }
    fn from_le_bytes(bytes: Self::Bytes) -> Self {
        bytes
    }
}

impl PackedUInt for u16 {
    type Bytes = [u8; 2];
    fn to_le_bytes(this: Self) -> Self::Bytes {
        this.to_le_bytes()
    }
    fn from_le_bytes(bytes: Self::Bytes) -> Self {
        Self::from_le_bytes(bytes)
    }
}

impl PackedUInt for u16 {
    type Bytes = [u8; 2];
    fn to_le_bytes(this: Self) -> Self::Bytes {
        this.to_le_bytes()
    }
    fn from_le_bytes(bytes: Self::Bytes) -> Self {
        Self::from_le_bytes(bytes)
    }
}
*/