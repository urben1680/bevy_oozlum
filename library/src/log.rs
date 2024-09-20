//! todo: intro
//!
//! # log or no log at all
//!
//! A log is needed if the context during [`BackwardLog`] or [`ForwardLog`] is not enough to determine
//! how and if a mutation has to be undone/redone. If all a system does is to increase or decrease the
//! velocity of a thing in free fall, this does not need to log at all.
//!
//! Other mutations to the components could happen in other systems, but then this system is not concerned
//! as long these other systems are reversible as well.
//!
//! ```
//! use bevy::prelude::*;
//!
//! #[derive(Component)]
//! struct Velocity(i32);
//!
//! #[derive(Component)]
//! struct Acceleration(i32);
//!
//! #[derive(Component)]
//! struct FreeFall;
//!
//! fn velocity_system(query: Query<(&mut Velocity, &mut Acceleration), With<FreeFall>>, meta: Res<RevMeta>) {
//!     match meta.direction() {
//!         Direction::Forward | Direction::ForwardLog => {
//!             for (velocity, acceleration) in query.iter_mut() {
//!                 velocity.0 += acceleration.0;
//!                 acceleration.0 += 1;
//!             }
//!         },
//!         Direction::BackwardLog => {
//!             for (velocity, acceleration) in query.iter_mut() {
//!                 acceleration.0 -= 1;
//!                 velocity.0 -= acceleration.0;
//!             }
//!         }
//!     }
//! }
//! ```
//!
//! However, situations where information is lost with a mutation, reversible systems do not work without logs.
//!
//! ```
//! use bevy::prelude::*;
//!
//! #[derive(Component)]
//! struct Velocity(i32);
//!
//! #[derive(Component)]
//! struct VerticalPosition(i32);
//!
//! fn position_system(query: Query<(&mut Velocity, &mut VerticalPosition)>, meta: Res<RevMeta>) {
//!     match meta.direction() {
//!         Direction::Forward => {
//!             for (velocity, position) in query.iter_mut() {
//!                 position.0 -= velocity.0;
//!                 if position.0 <= 0 {
//!                     // 0 is the height of the ground, stop falling
//!                     position.0 = 0;
//!                     velocity.0 = 0;
//!                 }
//!             }
//!         },
//!         _ => todo!("How to undo/redo changes? Which values did the components have?")
//!     }
//! }
//! ```
//!
//! # Variants
//!
//! ## `Rare` / non-`Rare` logs
//!
//! It is simple if entities are iterated that are always mutated when encountered. Then the mutation
//! takes place and the log is updated. And during [`BackwardLog`] or [`ForwardLog`] the log is accessed
//! to undo/redo the mutation.
//!
//! If that is the scenario, non-`Rare` log variants can be used.
//!
//! ```
//! use bevy::prelude::*;
//! use rand::prelude::*;
//!
//! #[derive(Component)]
//! struct MyVal(StateLog<i32>);
//!
//! fn rand_system(query: Query<&mut MyVal>, meta: Res<RevMeta>) {
//!     match meta.direction() {
//!         Direction::Forward => {
//!             for val in query.iter_mut() {
//!                 val.0.push_present(rand::random());
//!                 val.0.pop_past_by_len(meta.past_len());
//!                 println!("{}", val.0.get());
//!             }
//!         },
//!         Direction::ForwardLog => {
//!             for val in query.iter_mut() {
//!                 val.0.forward_log().expect("not past log end");
//!                 println!("{}", val.0.get());
//!             }
//!         },
//!         Direction::BackwardLog => {
//!             for val in query.iter_mut() {
//!                 val.0.backward_log().expect("not past log end");
//!                 println!("{}", val.0.get());
//!             }
//!         }
//!     }
//! }
//! ```
//!
//! If however mutations do not always happen, constantly pushing the same data into the log is wasteful.
//!
//! There are a few solutions for this situation.
//!
//! ### `Rare` logs
//!
//! Rare log variants take an `Option<T>` in their `push_present` or, if the logs take multiple values
//! (see TODO), do have the following behavior if 0 values are pushed.
//!
//! Receiving no value in the `push_present` method increases an internal counter without actually increasing
//! the number of entries in the internal collection. That way the less often values are pushed, the more
//! space-efficient the log is.
//!
//! ```
//! use bevy::prelude::*;
//! use rand::prelude::*;
//!
//! #[derive(Component)]
//! struct MyVal(RareStateLog<i32>);
//!
//! fn rand_system(query: Query<&mut MyVal>, meta: Res<RevMeta>) {
//!     match meta.direction() {
//!         Direction::Forward => {
//!             for val in query.iter_mut() {
//!                 let optional_val = rand::random::<bool>().then(rand::random);
//!                 val.0.push_present(optional_val);
//!                 val.0.pop_past_by_len(meta.past_len());
//!                 println!("{}", val.0.get());
//!             }
//!         },
//!         Direction::ForwardLog => {
//!             for val in query.iter_mut() {
//!                 val.0.forward_log().expect("not past log end");
//!                 println!("{}", val.0.get());
//!             }
//!         },
//!         Direction::BackwardLog => {
//!             for val in query.iter_mut() {
//!                 val.0.backward_log().expect("not past log end");
//!                 println!("{}", val.0.get());
//!             }
//!         }
//!     }
//! }
//! ```
//!
//! ### Updating by context
//!
//! Ideally the context contains the information if mutations should be done. But this context must be
//! available during [`BackwardLog`] and [`ForwardLog`] as well.
//!
//! ```
//! use bevy::prelude::*;
//! use rand::prelude::*;
//!
//! #[derive(Component)]
//! struct MyVal(StateLog<i32>);
//!
//! #[derive(Resource)]
//! struct UpdateRand(Entity); // set in a different reversible system
//!
//! fn rand_system(query: Query<&mut MyVal>, update_entity: Res<UpdateRand>, meta: Res<RevMeta>) {
//!     let Some(val) = query.get_mut(update_entity.0) else {
//!         return;
//!     }
//!     match meta.direction(update_entity.0) {
//!         Direction::Forward => {
//!             val.0.push_present(rand::random());
//!             val.0.pop_past_by_len(meta.past_len());
//!         },
//!         Direction::ForwardLog => {
//!             val.0.forward_log().expect("not past log end");
//!         },
//!         Direction::BackwardLog => {
//!             val.0.backward_log().expect("not past log end");
//!         }
//!     }
//!     println!("{}", val.0.get());
//! }
//! ```
//!
//! ### Other memory advantages of `Rare` logs
//!
//! Rare log variants are supposed to need less memory, but they fulfill this role in other situations
//! as well. The `None` push might not be an absence of a value but be treated as a value as well.
//!
//! For example, if one wants to log `bool` and `false` occurs much more often than `true`, one can use a
//! [`RareStateLog<()>`] where `false` maps to `None` and `true` to `Some(())`.
//!
//! ## Amount of values per push
//!
//! todo
//!
//! ## `State` or `Transition` logs
//!
//! todo        
//!
//! [`Forward`]: crate::meta::Direction::Forward
//! [`ForwardLog`]: crate::meta::Direction::ForwardLog
//! [`BackwardLog`]: crate::meta::Direction::BackwardLog

use std::{
    collections::VecDeque,
    fmt::{Debug, Display},
    iter::FusedIterator,
};

use bevy::reflect::{std_traits::ReflectDefault, Reflect};

#[cfg(feature = "serde")]
use bevy::reflect::{ReflectDeserialize, ReflectSerialize};

mod rare_state;
mod rare_states;
mod rare_transition;
mod rare_transitions;
mod state;
mod states;
mod transition;
mod transitions;

pub use rare_state::RareStateLog;
pub use rare_states::RareStatesLog;
pub use rare_transition::RareTransitionLog;
pub use rare_transitions::RareTransitionsLog;
pub use state::StateLog;
pub use states::StatesLog;
pub use transition::TransitionLog;
pub use transitions::TransitionsLog;

use crate::meta::RevMeta;

#[cfg(not(any(
    feature = "time_bytes_1",
    feature = "time_bytes_2",
    feature = "time_bytes_3",
    feature = "time_bytes_4",
    feature = "time_bytes_5",
    feature = "time_bytes_6",
    feature = "time_bytes_7",
    feature = "time_bytes_8",
)))]
const TIME_BYTES: usize = PackedUSize::BYTES;

#[cfg(feature = "time_bytes_1")]
const TIME_BYTES: usize = 1;

#[cfg(feature = "time_bytes_2")]
const TIME_BYTES: usize = 2;

#[cfg(feature = "time_bytes_3")]
const TIME_BYTES: usize = 3;

#[cfg(feature = "time_bytes_4")]
const TIME_BYTES: usize = 4;

#[cfg(feature = "time_bytes_5")]
const TIME_BYTES: usize = 5;

#[cfg(feature = "time_bytes_6")]
const TIME_BYTES: usize = 6;

#[cfg(feature = "time_bytes_7")]
const TIME_BYTES: usize = 7;

#[cfg(feature = "time_bytes_8")]
const TIME_BYTES: usize = 8;

#[derive(Clone, Copy, Reflect, PartialEq, Eq)]
#[reflect(Default, Debug)]
#[cfg_attr(feature = "serde", reflect(Serialize, Deserialize))]
pub struct PackedTime([u8; Self::BYTES]);

impl PackedTime {
    pub const BYTES: usize = if TIME_BYTES > PackedUSize::BYTES {
        PackedUSize::BYTES
    } else {
        TIME_BYTES
    };
    pub const MIN: Self = Self([u8::MIN; Self::BYTES]);
    pub const MAX: Self = Self([u8::MAX; Self::BYTES]);
    pub const MAX_USIZE: usize = {
        let bits = Self::BYTES as u32 * 8;
        let shift = if bits <= usize::BITS {
            usize::BITS - bits
        } else {
            0
        };
        usize::MAX >> shift
    };
    pub(crate) fn from_internal(time: usize) -> Self {
        time.try_into().unwrap_or_else(|_| panic!("{time} does not fit into {} bytes, \
            cannot map this value to `PackedTime`. If a log that contains `WithTimestamp` \
            is loaded while RevMeta is created with an offset from the last run, make use
            of the `reduce_timestamps` method of the log as well. If this is not the issue, \
            this is an internal bug.", Self::BYTES))
    }
    pub(crate) fn from_user(time: usize) -> Self {
        time.try_into().unwrap_or_else(|_| panic!("{time} does not fit into {} bytes, \
            cannot map this value to `PackedTime`, consider to increase the `time_bytes_*` \
            feature to a higher amount of bytes to store this value.", Self::BYTES))
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for PackedTime {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        Into::<usize>::into(*self).serialize(serializer)
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for PackedTime {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        match usize::deserialize(deserializer) {
            Ok(time) => match Self::try_from(time) {
                Ok(this) => Ok(this),
                Err(USizeTooLarge) => Err(serde::de::Error::custom(format!(
                    "{time} does not fit into {} bytes, cannot map this value to `PackedTime` \
                    on this machine, increase the `time_bytes_*` feature of the reversible_systems \
                    crate to the value of the source where this value was serialized",
                    Self::BYTES,
                ))),
            },
            Err(err) => Err(err),
        }
    }
}

impl Debug for PackedTime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Debug::fmt(&usize::from(*self), f)
    }
}

impl Display for PackedTime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&usize::from(*self), f)
    }
}

impl Default for PackedTime {
    fn default() -> Self {
        Self::MIN
    }
}

impl From<PackedTime> for usize {
    fn from(value: PackedTime) -> Self {
        usize::from_ne_bytes(value.0)
    }
}

impl TryFrom<usize> for PackedTime {
    type Error = USizeTooLarge;
    fn try_from(value: usize) -> Result<Self, Self::Error> {
        if value <= Self::MAX_USIZE {
            let mut i = value.to_le_bytes().into_iter();
            Ok(Self(std::array::from_fn(|_| i.next().unwrap_or(0))))
        } else {
            Err(USizeTooLarge)
        }
    }
}

#[derive(Clone, Copy, Reflect, PartialEq, Eq)]
#[reflect(Default, Debug)]
#[cfg_attr(feature = "serde", reflect(Serialize, Deserialize))]
pub struct PackedUSize([u8; Self::BYTES]);

impl PackedUSize {
    pub const BYTES: usize = usize::BITS as usize / 8;
    pub const MIN: Self = Self([u8::MIN; Self::BYTES]);
    pub const MAX: Self = Self([u8::MAX; Self::BYTES]);
}

#[cfg(feature = "serde")]
impl serde::Serialize for PackedUSize {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        Into::<usize>::into(*self).serialize(serializer)
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for PackedUSize {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        usize::deserialize(deserializer).map(Into::into)
    }
}

impl Debug for PackedUSize {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Debug::fmt(&usize::from(*self), f)
    }
}

impl Display for PackedUSize {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&usize::from(*self), f)
    }
}

impl Default for PackedUSize {
    fn default() -> Self {
        Self::MIN
    }
}

impl From<PackedUSize> for usize {
    fn from(value: PackedUSize) -> Self {
        usize::from_ne_bytes(value.0)
    }
}

impl From<usize> for PackedUSize {
    fn from(value: usize) -> Self {
        Self(value.to_ne_bytes())
    }
}

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
    pub fn push_back(&mut self, value: T) {
        self.0.push_back(value);
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
pub struct AmountErr<I, U> {
    pub values: I,
    pub entry: U,
    pub pushed_amount: usize,
    pub max_amount: usize,
}

impl<I, U> AmountErr<I, U> {
    fn new<T: WithAmount>(values: I, entry: U, pushed_amount: usize) -> Self {
        let max_amount = <T as WithAmount>::MAX;
        let max_amount = <T as WithAmount>::amount_to_usize(max_amount);
        Self {
            values,
            entry,
            pushed_amount,
            max_amount,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ValueEntry<T, U> {
    pub value: T,
    pub entry: U,
}

impl<'a, T: Iterator, U> IntoIterator for &'a mut ValueEntry<T, U> {
    type IntoIter = &'a mut T;
    type Item = T::Item;
    fn into_iter(self) -> Self::IntoIter {
        &mut self.value
    }
}

/// Call `update` of a log with this struct up to one time per reversible frame.
///
/// This will enable a cleanup strategy where entries are forgotten that are older than the global log start.
#[derive(Clone, Copy, PartialEq, Eq, Reflect)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct WithTimestamp<T = ()> {
    pub value: T,
    pub(crate) logged_at: PackedTime,
}

impl<T: Debug> Debug for WithTimestamp<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(std::any::type_name::<Self>())
            .field("value", &self.value)
            .field("logged_at", &usize::from(self.logged_at))
            .finish()
    }
}

impl<T> WithTimestamp<T> {
    pub fn new(value: T, logged_at: usize) -> Self {
        Self {
            value,
            logged_at: PackedTime::from_user(logged_at),
        }
    }
    pub fn logged_at(&self) -> usize {
        self.logged_at.into()
    }
}

impl<T: Default> From<usize> for WithTimestamp<T> {
    fn from(logged_at: usize) -> Self {
        Self {
            value: T::default(),
            logged_at: PackedTime::from_user(logged_at),
        }
    }
}

impl<T: Default> From<&RevMeta> for WithTimestamp<T> {
    fn from(meta: &RevMeta) -> Self {
        Self {
            value: T::default(),
            logged_at: PackedTime::from_internal(meta.now()),
        }
    }
}

#[derive(Clone, PartialEq, Reflect)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
struct RareValue<T> {
    value: T,
    /// If `T` is a transiton, then these are the skips before the transition.
    ///
    /// If `T` is a value, then these are the skips after the value.
    skips: PackedTime,
}

impl<T: Debug> Debug for RareValue<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(std::any::type_name::<Self>())
            .field("value", &self.value)
            .field("skips", &usize::from(self.skips))
            .finish()
    }
}

impl<T> RareValue<T> {
    fn len(&self) -> usize {
        usize::from(self.skips) + 1 // `self.data` adds to the len
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Reflect)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
struct EntryAmount<U, A> {
    entry: U,
    amount: A,
}

impl<U, A: Copy> EntryAmount<U, A> {
    fn zero<T: WithAmount<Amount = A>>(entry: U) -> Self {
        Self {
            entry,
            amount: <T as WithAmount>::MIN,
        }
    }
    fn amount<T: WithAmount<Amount = A>>(&self) -> usize {
        <T as WithAmount>::amount_to_usize(self.amount)
    }
}

const INDEX_OOB: &'static str = "self.index should always be <= the deque len, so successfully reducing it without underflow is expected to result in a valid index into the log which is not the case here";

/// unwrap bad, https://github.com/rust-lang/rust/issues/61695
fn into_ok<T>(result: Result<T, std::convert::Infallible>) -> T {
    match result {
        Ok(ok) => ok,
        Err(err) => match err {},
    }
}

#[doc(hidden)]
#[derive(Debug, Clone, Copy)]
pub struct USizeTooLarge;

#[doc(hidden)]
pub trait NotUSize {} // remove with const generic expressions, then bound on AMOUNT_BYTES > 0
impl NotUSize for PackedUSize {}
impl<const AMOUNT_BYTES: usize> NotUSize for [u8; AMOUNT_BYTES] {}

pub trait WithAmount {
    #[cfg(feature = "serde")]
    type Amount: Debug
        + Copy
        + Clone
        + Send
        + Sync
        + 'static
        + serde::Serialize
        + for<'de> serde::Deserialize<'de>;
    #[cfg(not(feature = "serde"))]
    type Amount: Debug + Copy + Clone + Send + Sync + 'static;
    type Err;
    const MIN: Self::Amount;
    const MAX: Self::Amount;
    fn amount_to_usize(value: Self::Amount) -> usize;
    fn usize_to_amount(value: usize) -> Result<Self::Amount, Self::Err>;
}

macro_rules! impl_with_amount {
    ($Log: ident) => {
        impl<T, U> crate::log::WithAmount for $Log<T, U, 0> {
            type Amount = usize;
            type Err = std::convert::Infallible;
            const MIN: Self::Amount = usize::MIN;
            const MAX: Self::Amount = usize::MAX;
            fn amount_to_usize(value: Self::Amount) -> usize {
                value
            }
            fn usize_to_amount(value: usize) -> Result<Self::Amount, Self::Err> {
                return Ok(value);

                // use this scope to hide the `target` module
                // from other macro calls in the same scope

                #[cfg(target_pointer_width = "16")]
                mod target {
                    use super::{impl_with_amount, $Log};

                    impl_with_amount!($Log, 1);
                    impl_with_amount!($Log, 2, Infallible);
                    impl_with_amount!($Log, 3, Infallible);
                    impl_with_amount!($Log, 4, Infallible);
                    impl_with_amount!($Log, 5, Infallible);
                    impl_with_amount!($Log, 6, Infallible);
                    impl_with_amount!($Log, 7, Infallible);
                    impl_with_amount!($Log, 8, Infallible);
                }

                #[cfg(target_pointer_width = "32")]
                mod target {
                    use super::{impl_with_amount, $Log};

                    impl_with_amount!($Log, 1);
                    impl_with_amount!($Log, 2);
                    impl_with_amount!($Log, 3);
                    impl_with_amount!($Log, 4, Infallible);
                    impl_with_amount!($Log, 5, Infallible);
                    impl_with_amount!($Log, 6, Infallible);
                    impl_with_amount!($Log, 7, Infallible);
                    impl_with_amount!($Log, 8, Infallible);
                }

                #[cfg(target_pointer_width = "64")]
                mod target {
                    use super::{impl_with_amount, $Log};

                    impl_with_amount!($Log, 1);
                    impl_with_amount!($Log, 2);
                    impl_with_amount!($Log, 3);
                    impl_with_amount!($Log, 4);
                    impl_with_amount!($Log, 5);
                    impl_with_amount!($Log, 6);
                    impl_with_amount!($Log, 7);
                    impl_with_amount!($Log, 8, Infallible);
                }
            }
        }
    };
    ($Log: ident, $AMOUNT_BYTES: literal) => {
        impl<T, U> crate::log::WithAmount for $Log<T, U, $AMOUNT_BYTES> {
            type Amount = [u8; $AMOUNT_BYTES];
            type Err = crate::log::USizeTooLarge;
            const MIN: Self::Amount = [u8::MIN; $AMOUNT_BYTES];
            const MAX: Self::Amount = [u8::MAX; $AMOUNT_BYTES];
            fn amount_to_usize(value: Self::Amount) -> usize {
                let mut i = value.into_iter();
                usize::from_le_bytes(std::array::from_fn(|_| i.next().unwrap_or(0)))
            }
            fn usize_to_amount(value: usize) -> Result<Self::Amount, Self::Err> {
                let shift = usize::BITS.saturating_sub($AMOUNT_BYTES as u32 * 8);
                let max = usize::MAX >> shift;
                if value <= max {
                    let mut i = value.to_le_bytes().into_iter();
                    Ok(std::array::from_fn(|_| i.next().unwrap_or(0)))
                } else {
                    Err(crate::log::USizeTooLarge)
                }
            }
        }
    };
    ($Log: ident, $AMOUNT_BYTES: literal, Infallible) => {
        impl<T, U> crate::log::WithAmount for $Log<T, U, $AMOUNT_BYTES> {
            type Amount = crate::log::PackedUSize;
            type Err = std::convert::Infallible;
            const MIN: Self::Amount = crate::log::PackedUSize::MIN;
            const MAX: Self::Amount = crate::log::PackedUSize::MAX;
            fn amount_to_usize(value: Self::Amount) -> usize {
                value.into()
            }
            fn usize_to_amount(value: usize) -> Result<Self::Amount, Self::Err> {
                Ok(value.into())
            }
        }
    };
}

use impl_with_amount;
