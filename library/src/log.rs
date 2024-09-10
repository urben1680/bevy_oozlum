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

use std::{collections::VecDeque, fmt::Debug, iter::FusedIterator};

use bevy::reflect::Reflect;

pub mod packed_uint;
mod rare_transition;
mod rare_transitions;
mod rare_value;
mod rare_values;
mod transition;
mod transitions;
mod value;
mod values;

use packed_uint::PackedUSize;
pub use rare_transition::RareTransitionLog;
pub use rare_transitions::RareTransitionsLog;
pub use rare_value::RareValueLog;
pub use transition::TransitionLog;
pub use transitions::TransitionsLog;
pub use value::ValueLog;
pub use values::ValuesLog;

use crate::meta::RevMeta;

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Reflect)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct WithTimestamp<T = ()> {
    pub data: T,
    pub logged_at: PackedUSize,
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

#[derive(Debug, Clone, PartialEq, Reflect)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
struct RareData<T> {
    data: T,
    /// If `T` is a transiton, then these are the skips before the transition.
    ///
    /// If `T` is a value, then these are the skips after the value.
    skips: PackedUSize,
}

impl<T> RareData<T> {
    fn len(&self) -> usize {
        let skips: usize = self.skips.into();
        skips + 1 // `self.data` adds to the len 
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Reflect)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
struct WithAmount<U, Amount> {
    entry: U,
    amount: Amount,
}

impl<U, Amount> WithAmount<U, Amount>
where
    Amount: TryFrom<usize, Error: Debug> + Into<usize> + Copy,
{
    fn zero(entry: U) -> Self {
        let amount = 0usize
            .try_into()
            .expect("expects 0 to be representable by Amount");
        WithAmount { entry, amount }
    }
    fn amount(&self) -> usize {
        self.amount.into()
    }
}

const BACKWARD_EXPECT_MSG: &'static str = "self.index should always be <= the log len, so reducing it without underflow is expected to result in a valid index into the log";
