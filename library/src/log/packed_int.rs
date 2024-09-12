use std::{
    cmp::Ordering,
    fmt::Debug,
    marker::PhantomData,
    ops::{Add, AddAssign, RangeInclusive, Sub, SubAssign},
};

use bevy::reflect::{std_traits::ReflectDefault, Reflect};

#[cfg(feature = "serde")]
use bevy::reflect::{ReflectDeserialize, ReflectSerialize};

/// Wraps a `[u8; N]` that contains the output of `T::to_le_bytes()` where `T` is an integer primitive
/// and the wrapped value is small enough to fit into a potenitally smaller byte array than the type's
/// size is usually.
/// 
/// # Conversations
///
/// This wrapper implements `TryFrom<T>` and `Into<T>` among other traits, though variants with the same
/// size as their `T` also implement the infallible `From<T>`. Exceptions are the `PackedInt<usize, SIZE>`
/// and `PackedInt<isize, SIZE>` types that always only implement `TryFrom<T>`. If the `From` implementation
/// is desired, use the [`PackedUSize`] / [`PackedISize`] structs.
/// 
/// These namings also exist for other integer types which, other than the two mentioned, are type aliases.
/// 
/// # Value ranges
///
/// The value range follow the pattern of the primitive integers:
/// - signed: `-2^((SIZE-1)*8)` to `2^((SIZE-1)*8)` exclusively
/// - unsigned: `0` to `2^(SIZE*8)` exclusively
/// 
/// On machines with a pointer width less than 64 bit, `PackedInt<usize, SIZE>` / `PackedInt<isize, SIZE>`
/// and `SIZE` larger than the integer's size, the range still is limited to the range of their respective
/// integer types on these machines. The excessive internal bytes contain unspecified values that are not
/// read.
/// 
/// # Motivations
///
/// The benefit of this type is to safe memory if not the whole range of `T` is needed and also to pack
/// values with less padding along other types in a collection as all variants have an alignment of 1.
///
/// The main use-case is to offer alternative types for the `Amount` generic of...
/// - [`ValuesLog`](crate::log::ValuesLog)
/// - [`RareValuesLog`](crate::log::RareValuesLog)
/// - [`TransitionsLog`](crate::log::TransitionsLog)
/// - [`RareTransitionsLog`](crate::log::RareTransitionsLog)
///
/// ... where one can use `PackedInt<usize, SIZE>` there if one knows that never more than `(2^(SIZE*8))-1`
/// values/transitions are pushed. This can significantly reduce the memory usage of these logs if the
/// value/transition type itself is small as well.
///
/// By default these logs use [`PackedUSize`] to reduce padding with a non-ZST type picked for their generic
/// parameter `U`. To opt-out of all these memory optimizations, for example because `U` remains unused, one
/// can set `Amount` to `usize`.
#[derive(Reflect)]
#[reflect(Default, PartialEq)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    reflect(Serialize, Deserialize)
)]
pub struct PackedInt<T, const SIZE: usize>
where
    for<'a> [u8; SIZE]: SerdeBound<'a>,
{
    bytes: [u8; SIZE],
    #[reflect(ignore)]
    _p: PhantomData<fn([u8; SIZE]) -> T>,
}

/// Implements `From<u16>`. See [`Packed`].
pub type PackedU16 = PackedInt<u16, 2>;
/// Implements `From<i16>`. See [`Packed`].
pub type PackedI16 = PackedInt<i16, 2>;
/// Implements `From<u32>`. See [`Packed`].
pub type PackedU32 = PackedInt<u32, 4>;
/// Implements `From<i32>`. See [`Packed`].
pub type PackedI32 = PackedInt<i32, 4>;
/// Implements `From<u64>`. See [`Packed`].
pub type PackedU64 = PackedInt<u64, 8>;
/// Implements `From<i64>`. See [`Packed`].
pub type PackedI64 = PackedInt<i64, 8>;
/// Implements `From<u128>`. See [`Packed`].
pub type PackedU128 = PackedInt<u128, 16>;
/// Implements `From<i128>`. See [`Packed`].
pub type PackedI128 = PackedInt<i128, 16>;

#[doc(hidden)]
#[cfg(feature = "serde")]
pub trait SerdeBound<'de>: serde::Serialize + serde::Deserialize<'de> {}

#[doc(hidden)]
#[cfg(not(feature = "serde"))]
pub trait LimitLen<'de> {}

impl SerdeBound<'_> for [u8; 1] {}
impl SerdeBound<'_> for [u8; 2] {}
impl SerdeBound<'_> for [u8; 3] {}
impl SerdeBound<'_> for [u8; 4] {}
impl SerdeBound<'_> for [u8; 5] {}
impl SerdeBound<'_> for [u8; 6] {}
impl SerdeBound<'_> for [u8; 7] {}
impl SerdeBound<'_> for [u8; 8] {}
impl SerdeBound<'_> for [u8; 9] {}
impl SerdeBound<'_> for [u8; 10] {}
impl SerdeBound<'_> for [u8; 11] {}
impl SerdeBound<'_> for [u8; 12] {}
impl SerdeBound<'_> for [u8; 13] {}
impl SerdeBound<'_> for [u8; 14] {}
impl SerdeBound<'_> for [u8; 15] {}
impl SerdeBound<'_> for [u8; 16] {}

/// Error type of `PackedInt::<T, SIZE>::try_from` that is returned when the integer value does not fit into
/// `SIZE` bytes.
#[derive(Debug, PartialEq)]
pub struct ToPackedErr<T> {
    pub value_out_of_range: T,
    pub range: RangeInclusive<T>,
}

impl<T, const SIZE: usize> PackedInt<T, SIZE>
where
    for<'a> [u8; SIZE]: SerdeBound<'a>,
{
    pub const ZERO: Self = Self {
        bytes: [0; SIZE],
        _p: PhantomData,
    };
}

impl<T, const SIZE: usize> Default for PackedInt<T, SIZE>
where
    for<'a> [u8; SIZE]: SerdeBound<'a>,
{
    fn default() -> Self {
        Self::ZERO
    }
}

impl<T, const SIZE: usize> Clone for PackedInt<T, SIZE>
where
    for<'a> [u8; SIZE]: SerdeBound<'a>,
{
    fn clone(&self) -> Self {
        Self {
            bytes: self.bytes.clone(),
            _p: PhantomData,
        }
    }
}

impl<T, const SIZE: usize> Copy for PackedInt<T, SIZE>
where
    for<'a> [u8; SIZE]: SerdeBound<'a> {}

impl<T, const SIZE: usize> PartialEq for PackedInt<T, SIZE>
where
    for<'a> [u8; SIZE]: SerdeBound<'a>,
{
    fn eq(&self, rhs: &Self) -> bool {
        self.bytes.eq(&rhs.bytes)
    }
}

impl<T, const SIZE: usize> Eq for PackedInt<T, SIZE>
where
    for<'a> [u8; SIZE]: SerdeBound<'a> {}

impl<T: PartialOrd + From<Self>, const SIZE: usize> PartialOrd for PackedInt<T, SIZE>
where
    for<'a> [u8; SIZE]: SerdeBound<'a>,
{
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        let this: T = (*self).into();
        let other: T = (*other).into();
        this.partial_cmp(&other)
    }
}

impl<T: Ord + From<Self>, const SIZE: usize> Ord for PackedInt<T, SIZE>
where
    for<'a> [u8; SIZE]: SerdeBound<'a> {
    fn cmp(&self, other: &Self) -> Ordering {
        let this: T = (*self).into();
        let other: T = (*other).into();
        this.cmp(&other)
    }
}

impl<T: Add + From<Self>, const SIZE: usize> Add<Self> for PackedInt<T, SIZE>
where
    for<'a> [u8; SIZE]: SerdeBound<'a>,
{
    type Output = <T as Add>::Output;
    fn add(self, rhs: Self) -> Self::Output {
        let this: T = self.into();
        let rhs: T = rhs.into();
        this + rhs
    }
}

impl<T: Add + From<Self>, const SIZE: usize> Add<T> for PackedInt<T, SIZE>
where
    for<'a> [u8; SIZE]: SerdeBound<'a>,
{
    type Output = <T as Add>::Output;
    fn add(self, rhs: T) -> Self::Output {
        let this: T = self.into();
        this + rhs
    }
}

impl<T: Sub + From<Self>, const SIZE: usize> Sub<T> for PackedInt<T, SIZE>
where
    for<'a> [u8; SIZE]: SerdeBound<'a>,
{
    type Output = <T as Sub>::Output;
    fn sub(self, rhs: T) -> Self::Output {
        let this: T = self.into();
        this - rhs
    }
}

impl<T: Sub + From<Self>, const SIZE: usize> Sub<Self> for PackedInt<T, SIZE>
where
    for<'a> [u8; SIZE]: SerdeBound<'a>,
{
    type Output = <T as Sub>::Output;
    fn sub(self, rhs: Self) -> Self::Output {
        let this: T = self.into();
        let rhs: T = rhs.into();
        this - rhs
    }
}

macro_rules! packed_from_impl {
    ($integer: ident, $size: expr, TryFrom) => {
        impl TryFrom<$integer> for PackedInt<$integer, $size> {
            type Error = ToPackedErr<$integer>;
            fn try_from(value: $integer) -> Result<Self, Self::Error> {
                let shift = $integer::BITS.saturating_sub($size as u32 * 8);
                let range = ($integer::MIN >> shift)..=($integer::MAX >> shift);
                if range.contains(&value) {
                    let mut i = value.to_le_bytes().into_iter();
                    let bytes = std::array::from_fn(|_| i.next().unwrap_or(0));
                    Ok(Self {
                        bytes,
                        _p: PhantomData,
                    })
                } else {
                    Err(ToPackedErr {
                        value_out_of_range: value,
                        range,
                    })
                }
            }
        }
    };
    ($integer: ident, $size: expr, InfallibleTryFrom) => {
        impl TryFrom<$integer> for PackedInt<$integer, $size> {
            type Error = std::convert::Infallible;
            fn try_from(value: $integer) -> Result<Self, Self::Error> {
                const _: () = if $size < core::mem::size_of::<$integer>() {
                    panic!("InfallibleTryFrom impl not possible, Packed size too small")
                };
                let mut i = value.to_le_bytes().into_iter();
                let bytes = std::array::from_fn(|_| i.next().unwrap_or(0));
                Ok(Self {
                    bytes,
                    _p: PhantomData,
                })
            }
        }
    };
    ($integer: ident, $size: expr, From) => {
        impl From<$integer> for PackedInt<$integer, $size> {
            fn from(value: $integer) -> Self {
                Self {
                    bytes: value.to_le_bytes(),
                    _p: PhantomData,
                }
            }
        }
    };
}

macro_rules! packed_impl {
    ($integer: ident, $signed: tt, $size: expr, $fallible: ident) => {
        packed_from_impl!($integer, $size, $fallible);

        impl From<PackedInt<$integer, $size>> for $integer {
            fn from(packed: PackedInt<$integer, $size>) -> Self {
                let mut tail = 0;
                if $signed && packed.bytes[$size - 1] > 127 {
                    tail = 255;
                }
                let mut i = packed.bytes.into_iter();
                Self::from_le_bytes(std::array::from_fn(|_| i.next().unwrap_or(tail)))
            }
        }

        impl std::fmt::Debug for PackedInt<$integer, $size> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                let value: $integer = (*self).into();
                value.fmt(f)
            }
        }

        impl PartialEq<$integer> for PackedInt<$integer, $size> {
            fn eq(&self, other: &$integer) -> bool {
                let value: $integer = (*self).into();
                value.eq(other)
            }
        }

        impl PartialEq<PackedInt<$integer, $size>> for $integer {
            fn eq(&self, other: &PackedInt<$integer, $size>) -> bool {
                let other: $integer = (*other).into();
                self.eq(&other)
            }
        }

        impl PartialOrd<$integer> for PackedInt<$integer, $size> {
            fn partial_cmp(&self, other: &$integer) -> Option<Ordering> {
                let value: $integer = (*self).into();
                value.partial_cmp(other)
            }
        }

        impl PartialOrd<PackedInt<$integer, $size>> for $integer {
            fn partial_cmp(&self, other: &PackedInt<$integer, $size>) -> Option<Ordering> {
                let other: $integer = (*other).into();
                self.partial_cmp(&other)
            }
        }

        impl std::iter::Sum<PackedInt<$integer, $size>> for $integer {
            fn sum<I: Iterator<Item = PackedInt<$integer, $size>>>(iter: I) -> Self {
                iter.map(Into::<$integer>::into).sum()
            }
        }

        impl Add<PackedInt<$integer, $size>> for $integer {
            type Output = $integer;
            fn add(self, rhs: PackedInt<$integer, $size>) -> Self::Output {
                let rhs: $integer = rhs.into();
                self + rhs
            }
        }

        impl AddAssign<PackedInt<$integer, $size>> for $integer {
            fn add_assign(&mut self, rhs: PackedInt<$integer, $size>) {
                let rhs: $integer = rhs.into();
                *self += rhs;
            }
        }

        impl Sub<PackedInt<$integer, $size>> for $integer {
            type Output = $integer;
            fn sub(self, rhs: PackedInt<$integer, $size>) -> Self::Output {
                let rhs: $integer = rhs.into();
                self - rhs
            }
        }

        impl SubAssign<PackedInt<$integer, $size>> for $integer {
            fn sub_assign(&mut self, rhs: PackedInt<$integer, $size>) {
                let rhs: $integer = rhs.into();
                *self -= rhs;
            }
        }
    };
}

macro_rules! packed_size_impl {
    ($structname: ident, $integer: ident) => {
        /// Packed variant of the integer with the same range and size but an alignment of 1.
        /// 
        /// This type behaves like [`Packed`] variants with the integer's size, though this one specifies this
        /// size internally and supports the infallible `From<integer>` conversation for easier usage.
        /// 
        /// For more information see [`Packed`].
        #[derive(Clone, Copy, PartialEq, Eq, Reflect)]
        #[reflect(Default, PartialEq)]
        #[cfg_attr(
            feature = "serde",
            derive(serde::Serialize, serde::Deserialize),
            reflect(Serialize, Deserialize)
        )]
        pub struct $structname([u8; $integer::BITS as usize / 8]);

        impl $structname
        {
            pub const ZERO: Self = Self([0; $integer::BITS as usize / 8]);
        }

        impl Default for $structname
        {
            fn default() -> Self {
                Self::ZERO
            }
        }

        impl Add<Self> for $structname
        {
            type Output = $integer;
            fn add(self, rhs: Self) -> Self::Output {
                let this: $integer = self.into();
                let rhs: $integer = rhs.into();
                this + rhs
            }
        }

        impl Add<$integer> for $structname
        {
            type Output = $integer;
            fn add(self, rhs: $integer) -> Self::Output {
                let this: $integer = self.into();
                this + rhs
            }
        }

        impl Sub<$integer> for $structname
        {
            type Output = $integer;
            fn sub(self, rhs: $integer) -> Self::Output {
                let this: $integer = self.into();
                this - rhs
            }
        }

        impl Sub<$structname> for $structname
        {
            type Output = $integer;
            fn sub(self, rhs: Self) -> Self::Output {
                let this: $integer = self.into();
                let rhs: $integer = rhs.into();
                this - rhs
            }
        }

        impl From<$integer> for $structname {
            fn from(value: $integer) -> Self {
                Self(value.to_le_bytes())
            }
        }

        impl From<$structname> for $integer {
            fn from(packed: $structname) -> Self {
                Self::from_le_bytes(packed.0)
            }
        }

        impl std::fmt::Debug for $structname {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                let value: $integer = (*self).into();
                value.fmt(f)
            }
        }

        impl PartialEq<$integer> for $structname {
            fn eq(&self, other: &$integer) -> bool {
                let value: $integer = (*self).into();
                value.eq(other)
            }
        }

        impl PartialEq<$structname> for $integer {
            fn eq(&self, other: &$structname) -> bool {
                let other: $integer = (*other).into();
                self.eq(&other)
            }
        }

        impl PartialOrd for $structname {
            fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
                let value: $integer = (*self).into();
                let other: $integer = (*other).into();
                value.partial_cmp(&other)
            }
        }

        impl PartialOrd<$integer> for $structname {
            fn partial_cmp(&self, other: &$integer) -> Option<Ordering> {
                let value: $integer = (*self).into();
                value.partial_cmp(other)
            }
        }

        impl PartialOrd<$structname> for $integer {
            fn partial_cmp(&self, other: &$structname) -> Option<Ordering> {
                let other: $integer = (*other).into();
                self.partial_cmp(&other)
            }
        }

        impl Ord for $structname {
            fn cmp(&self, other: &Self) -> Ordering {
                let value: $integer = (*self).into();
                let other: $integer = (*other).into();
                value.cmp(&other)
            }
        }

        impl std::iter::Sum<$structname> for $integer {
            fn sum<I: Iterator<Item = $structname>>(iter: I) -> Self {
                iter.map(Into::<$integer>::into).sum()
            }
        }

        impl Add<$structname> for $integer {
            type Output = $integer;
            fn add(self, rhs: $structname) -> Self::Output {
                let rhs: $integer = rhs.into();
                self + rhs
            }
        }

        impl AddAssign<$structname> for $integer {
            fn add_assign(&mut self, rhs: $structname) {
                let rhs: $integer = rhs.into();
                *self += rhs;
            }
        }

        impl Sub<$structname> for $integer {
            type Output = $integer;
            fn sub(self, rhs: $structname) -> Self::Output {
                let rhs: $integer = rhs.into();
                self - rhs
            }
        }

        impl SubAssign<$structname> for $integer {
            fn sub_assign(&mut self, rhs: $structname) {
                let rhs: $integer = rhs.into();
                *self -= rhs;
            }
        }
    }
}

packed_impl!(u16, false, 1, TryFrom);
packed_impl!(i16, true, 1, TryFrom);
packed_impl!(u16, false, 2, From);
packed_impl!(i16, true, 2, From);

packed_impl!(u32, false, 1, TryFrom);
packed_impl!(i32, true, 1, TryFrom);
packed_impl!(u32, false, 2, TryFrom);
packed_impl!(i32, true, 2, TryFrom);
packed_impl!(u32, false, 3, TryFrom);
packed_impl!(i32, true, 3, TryFrom);
packed_impl!(u32, false, 4, From);
packed_impl!(i32, true, 4, From);

packed_impl!(u64, false, 1, TryFrom);
packed_impl!(i64, true, 1, TryFrom);
packed_impl!(u64, false, 2, TryFrom);
packed_impl!(i64, true, 2, TryFrom);
packed_impl!(u64, false, 3, TryFrom);
packed_impl!(i64, true, 3, TryFrom);
packed_impl!(u64, false, 4, TryFrom);
packed_impl!(i64, true, 4, TryFrom);
packed_impl!(u64, false, 5, TryFrom);
packed_impl!(i64, true, 5, TryFrom);
packed_impl!(u64, false, 6, TryFrom);
packed_impl!(i64, true, 6, TryFrom);
packed_impl!(u64, false, 7, TryFrom);
packed_impl!(i64, true, 7, TryFrom);
packed_impl!(u64, false, 8, From);
packed_impl!(i64, true, 8, From);

packed_size_impl!(PackedUSize, usize);
packed_size_impl!(PackedISize, isize);

#[cfg(target_pointer_width = "16")]
mod pointer_16 {
    use super::*;
    packed_impl!(usize, false, 1, TryFrom);
    packed_impl!(isize, true, 1, TryFrom);
    packed_impl!(usize, false, 2, InfallibleTryFrom);
    packed_impl!(isize, true, 2, InfallibleTryFrom);
    packed_impl!(usize, false, 3, InfallibleTryFrom);
    packed_impl!(isize, true, 3, InfallibleTryFrom);
    packed_impl!(usize, false, 4, InfallibleTryFrom);
    packed_impl!(isize, true, 4, InfallibleTryFrom);
    packed_impl!(usize, false, 5, InfallibleTryFrom);
    packed_impl!(isize, true, 5, InfallibleTryFrom);
    packed_impl!(usize, false, 6, InfallibleTryFrom);
    packed_impl!(isize, true, 6, InfallibleTryFrom);
    packed_impl!(usize, false, 7, InfallibleTryFrom);
    packed_impl!(isize, true, 7, InfallibleTryFrom);
    packed_impl!(usize, false, 8, InfallibleTryFrom);
    packed_impl!(isize, true, 8, InfallibleTryFrom);
}

#[cfg(target_pointer_width = "32")]
mod pointer_32 {
    use super::*;
    packed_impl!(usize, false, 1, TryFrom);
    packed_impl!(isize, true, 1, TryFrom);
    packed_impl!(usize, false, 2, TryFrom);
    packed_impl!(isize, true, 2, TryFrom);
    packed_impl!(usize, false, 3, TryFrom);
    packed_impl!(isize, true, 3, TryFrom);
    packed_impl!(usize, false, 4, InfallibleTryFrom);
    packed_impl!(isize, true, 4, InfallibleTryFrom);
    packed_impl!(usize, false, 5, InfallibleTryFrom);
    packed_impl!(isize, true, 5, InfallibleTryFrom);
    packed_impl!(usize, false, 6, InfallibleTryFrom);
    packed_impl!(isize, true, 6, InfallibleTryFrom);
    packed_impl!(usize, false, 7, InfallibleTryFrom);
    packed_impl!(isize, true, 7, InfallibleTryFrom);
    packed_impl!(usize, false, 8, InfallibleTryFrom);
    packed_impl!(isize, true, 8, InfallibleTryFrom);
}

#[cfg(target_pointer_width = "64")]
mod pointer_64 {
    use super::*;
    packed_impl!(usize, false, 1, TryFrom);
    packed_impl!(isize, true, 1, TryFrom);
    packed_impl!(usize, false, 2, TryFrom);
    packed_impl!(isize, true, 2, TryFrom);
    packed_impl!(usize, false, 3, TryFrom);
    packed_impl!(isize, true, 3, TryFrom);
    packed_impl!(usize, false, 4, TryFrom);
    packed_impl!(isize, true, 4, TryFrom);
    packed_impl!(usize, false, 5, TryFrom);
    packed_impl!(isize, true, 5, TryFrom);
    packed_impl!(usize, false, 6, TryFrom);
    packed_impl!(isize, true, 6, TryFrom);
    packed_impl!(usize, false, 7, TryFrom);
    packed_impl!(isize, true, 7, TryFrom);
    packed_impl!(usize, false, 8, InfallibleTryFrom);
    packed_impl!(isize, true, 8, InfallibleTryFrom);
}

packed_impl!(u128, false, 1, TryFrom);
packed_impl!(i128, true, 1, TryFrom);
packed_impl!(u128, false, 2, TryFrom);
packed_impl!(i128, true, 2, TryFrom);
packed_impl!(u128, false, 3, TryFrom);
packed_impl!(i128, true, 3, TryFrom);
packed_impl!(u128, false, 4, TryFrom);
packed_impl!(i128, true, 4, TryFrom);
packed_impl!(u128, false, 5, TryFrom);
packed_impl!(i128, true, 5, TryFrom);
packed_impl!(u128, false, 6, TryFrom);
packed_impl!(i128, true, 6, TryFrom);
packed_impl!(u128, false, 7, TryFrom);
packed_impl!(i128, true, 7, TryFrom);
packed_impl!(u128, false, 8, TryFrom);
packed_impl!(i128, true, 8, TryFrom);
packed_impl!(u128, false, 9, TryFrom);
packed_impl!(i128, true, 9, TryFrom);
packed_impl!(u128, false, 10, TryFrom);
packed_impl!(i128, true, 10, TryFrom);
packed_impl!(u128, false, 11, TryFrom);
packed_impl!(i128, true, 11, TryFrom);
packed_impl!(u128, false, 12, TryFrom);
packed_impl!(i128, true, 12, TryFrom);
packed_impl!(u128, false, 13, TryFrom);
packed_impl!(i128, true, 13, TryFrom);
packed_impl!(u128, false, 14, TryFrom);
packed_impl!(i128, true, 14, TryFrom);
packed_impl!(u128, false, 15, TryFrom);
packed_impl!(i128, true, 15, TryFrom);
packed_impl!(u128, false, 16, From);
packed_impl!(i128, true, 16, From);
