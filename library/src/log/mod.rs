use std::{
    collections::{vec_deque::Drain, VecDeque},
    error::Error,
    fmt::{Debug, Display},
    iter::FusedIterator,
};

use bevy::{log::error, reflect::Reflect};

#[cfg(feature = "serde")]
mod serde_with;

mod dense_state;
mod dense_states;
mod dense_transition;
mod dense_transitions;

mod frame_transition;

mod sparse_state;
mod sparse_states;
mod sparse_transition;
mod sparse_transitions;

#[cfg(feature = "serde")]
pub use serde_with::{logless_state, logless_with_capacity, with_capacity};

pub use dense_state::DenseStateLog;
pub use dense_states::DenseStatesLog;
pub use dense_transition::DenseTransitionLog;
pub use dense_transitions::DenseTransitionsLog;

pub use frame_transition::FrameTransitionLog;
pub use frame_transition::FrameTransitionLogError;

pub use sparse_state::SparseStateLog;
pub use sparse_states::SparseStatesLog;
pub use sparse_transition::SparseTransitionLog;
pub use sparse_transitions::SparseTransitionsLog;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OutOfLog;

impl Display for OutOfLog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "a log was traversed beyond it's bounds or it was attempted to queue RevMeta to a frame outside the log")
    }
}

impl Error for OutOfLog {}

pub struct PushedTooMany<I: ExactSizeIterator, U, const AMOUNT_BYTES: usize> {
    pub values: I,
    pub entry: U,
}

impl<I: ExactSizeIterator, U, const AMOUNT_BYTES: usize> PushedTooMany<I, U, AMOUNT_BYTES> {
    pub const MAX_AMOUNT: usize = usize::MAX >> ((USIZE_BYTES - AMOUNT_BYTES) * 8);
    // easier to call with &self during error handling
    pub fn max_amount(&self) -> usize {
        Self::MAX_AMOUNT
    }
}

// makes unwrap possible without requiring additional Debug bounds everywhere
impl<I: ExactSizeIterator, U, const AMOUNT_BYTES: usize> Debug
    for PushedTooMany<I, U, AMOUNT_BYTES>
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(std::any::type_name::<Self>())
            .field("pushed_amount", &self.values.len())
            .field("max_amount", &self.max_amount())
            .finish_non_exhaustive()
    }
}

impl<I: ExactSizeIterator, U, const AMOUNT_BYTES: usize> Display
    for PushedTooMany<I, U, AMOUNT_BYTES>
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "attempted to push {} values into a log that support only {} values per update",
            self.values.len(),
            Self::MAX_AMOUNT
        )
    }
}

impl<I: ExactSizeIterator, U, const AMOUNT_BYTES: usize> Error
    for PushedTooMany<I, U, AMOUNT_BYTES>
{
}

/// A `&mut VecDeque<T>` wrapper that does not expose methods which remove from the deque.
pub struct LogMut<'a, T>(&'a mut VecDeque<T>);

impl<'a, T> LogMut<'a, T> {
    pub fn append(&mut self, other: &mut VecDeque<T>) {
        self.0.append(other);
    }
    pub fn push(&mut self, value: T) {
        self.0.push_back(value);
    }
}

impl<'a, T> Extend<T> for LogMut<'a, T> {
    fn extend<I: IntoIterator<Item = T>>(&mut self, iter: I) {
        self.0.extend(iter);
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

#[derive(Debug, Clone, PartialEq, Eq)]
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

const USIZE_BYTES: usize = usize::BITS as usize / 8;

#[derive(Clone, PartialEq, Reflect)]
struct SparseValue<T> {
    value: T,
    /// If `T` is a state, then these are the skips _after_ the state.
    ///
    /// If `T` is a transiton, then these are the skips _before_ the transition.
    ///
    /// This is not a `PackedRevFrame` because skips may be sub-frames and sum up to larger values.
    /// Instead, this is usize's native byte representation to reduce the alignment of this field.
    ///
    /// This value never gets reduced by `pop`/`drain_past_by_len` to be consistent with the behavior
    /// of `pop`/`drain_past_by_logged_at` which cannot interpret these skips as frames as pointed out.
    skips_ne: [u8; USIZE_BYTES],
}

#[cfg(feature = "serde")]
impl<T: serde::Serialize> serde::Serialize for SparseValue<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        (&self.value, &self.skips()).serialize(serializer)
    }
}

#[cfg(feature = "serde")]
impl<'de, T: serde::Deserialize<'de>> serde::Deserialize<'de> for SparseValue<T> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let (value, skips) = <(T, usize) as serde::Deserialize<'de>>::deserialize(deserializer)?;
        let rare_value = Self::new(value, skips);
        // skips == rare_value.skips() is always true, if this platform's usize lacks the bytes to store the
        // serialized value, that information was lost at usize::deserialize already and needs to be handled by serde
        Ok(rare_value)
    }
}

impl<T: Debug> Debug for SparseValue<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(std::any::type_name::<Self>())
            .field("value", &self.value)
            .field("skips", &self.skips())
            .finish()
    }
}

impl<T> SparseValue<T> {
    fn new(value: T, skips: usize) -> Self {
        Self {
            value,
            skips_ne: usize::to_ne_bytes(skips),
        }
    }
    fn len(&self) -> usize {
        self.skips() + 1 // `self.data` adds to the len
    }
    fn skips(&self) -> usize {
        usize::from_ne_bytes(self.skips_ne)
    }
}

/// Draining iterator used by some methods of sparse logs.
///
/// See [`vec_deque::Drain`](Drain), that is wrapped here, for further information.
#[derive(Debug)]
// Nameable alternative to `Map<Drain<SparseValue<T>>, impl FnMut(SparseValue<T>) -> T>`
pub struct SparseDrain<'a, T>(Drain<'a, SparseValue<T>>);

impl<T> Iterator for SparseDrain<'_, T> {
    type Item = T;

    #[inline]
    fn next(&mut self) -> Option<T> {
        self.0.next().map(|rare| rare.value)
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
    }
}

impl<T> DoubleEndedIterator for SparseDrain<'_, T> {
    #[inline]
    fn next_back(&mut self) -> Option<T> {
        self.0.next_back().map(|rare| rare.value)
    }
}

impl<T> ExactSizeIterator for SparseDrain<'_, T> {}

impl<T> FusedIterator for SparseDrain<'_, T> {}

/// Assumes cut-off bytes, if any, are more significant than the existing ones and are `0`.
#[inline(always)]
fn resize_ne_bytes<const N: usize, const M: usize>(arr: [u8; N]) -> [u8; M] {
    let min = N.min(M);
    let mut result = [0; M];
    let (source, target);
    if cfg!(target_endian = "little") {
        source = &arr[..min];
        target = &mut result[..min];
    } else {
        source = &arr[N - min..];
        target = &mut result[M - min..];
    };
    target.copy_from_slice(source);
    result
}

/// `EntryAmount` is usually encountered in draining methods of logs with multiple states/transitions per update,
/// for example [`DenseStatesLog`] which will be behind the `log` variable in the following code snippets.
///
/// These methods return two draining iterators, the first with the states/transitions of all updates that are
/// drained, and the second with `EntryAmount`, containing the entry type `U` of the log (if specified) and the
/// amount of states/transitions per update, returned by the [`amount`](Self::amount) method.
#[derive(Debug, Clone, Reflect)]
pub struct EntryAmount<U, const AMOUNT_BYTES: usize> {
    pub entry: U,
    amount: [u8; AMOUNT_BYTES],
}

impl<U, const AMOUNT_BYTES: usize> EntryAmount<U, AMOUNT_BYTES> {
    const fn zero(entry: U) -> Self {
        Self {
            entry,
            amount: [0; AMOUNT_BYTES],
        }
    }
    fn new(entry: U, amount: usize) -> Self {
        Self {
            entry,
            amount: resize_ne_bytes(amount.to_ne_bytes()),
        }
    }

    /// Returns the amount of states/transitions of an update. This can be useful to chunk them.
    ///
    /// # Examples
    ///
    /// With `log` being [`&mut DenseStatesLog`](DenseStatesLog) and [`DenseStatesLog::drain_future`] returning the
    /// draining iterators, do this to slice them by updates:
    ///
    /// ```
    /// # let mut log = library::log::DenseStatesLog::new([0], 0);
    /// let (mut future_states, future_entry_amounts) = log.drain_future();
    /// ```
    ///
    /// Now iterate...
    /// ```
    /// # let mut log = library::log::DenseStatesLog::new([0], 0);
    /// # let (mut future_states, future_entry_amounts) = log.drain_future();
    /// for entry_amount in future_entry_amounts {
    ///     for future_states in future_states.by_ref().take(entry_amount.amount()) {
    ///         let entry = entry_amount.entry;
    ///         // logic
    ///     }
    /// }
    /// ```
    ///
    /// ...or collect them:
    /// ```
    /// # let mut log = library::log::DenseStatesLog::new([0], 0);
    /// # let (mut future_states, future_entry_amounts) = log.drain_future();
    /// let updates: Vec<(Vec<_>, _)> = future_entry_amounts.map(|entry_amount| (
    ///     future_states.by_ref().take(entry_amount.amount()).collect(),
    ///     entry_amount.entry
    /// )).collect();
    /// ```
    pub fn amount(&self) -> usize {
        usize::from_ne_bytes(resize_ne_bytes(self.amount))
    }
}

#[cfg(feature = "serde")]
impl<U: serde::Serialize, const AMOUNT_BYTES: usize> serde::Serialize
    for EntryAmount<U, AMOUNT_BYTES>
{
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        (&self.entry, self.amount()).serialize(serializer)
    }
}

#[cfg(feature = "serde")]
impl<'de, U: serde::Deserialize<'de>, const AMOUNT_BYTES: usize> serde::Deserialize<'de>
    for EntryAmount<U, AMOUNT_BYTES>
{
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let (entry, amount) = <(U, usize) as serde::Deserialize<'de>>::deserialize(deserializer)?;
        let entry_amount = Self::new(entry, amount);
        if amount == entry_amount.amount() {
            Ok(entry_amount)
        } else {
            Err(serde::de::Error::custom("todo"))
        }
    }
}

fn index_oob() -> OutOfLog {
    error!("self.index should always be <= the deque len, so successfully reducing \
        it without underflow is expected to result in a valid index into the log which is not the case here, \
        the log is in an invalid state before calling the current method, this is a crate bug"
    );
    OutOfLog
}

#[cfg(test)]
mod test {
    use super::{EntryAmount, PushedTooMany, ValueEntry};

    pub(super) fn collect_pop_result<
        I1: Iterator<Item = char>,
        I2: ExactSizeIterator<Item = char>,
    >(
        actual_pop: Result<Option<ValueEntry<I1, char>>, PushedTooMany<I2, char, 1>>,
    ) -> Result<Option<(Vec<char>, char)>, (Vec<char>, char)> {
        match actual_pop {
            Ok(None) => Ok(None),
            Ok(Some(value_entry)) => Ok(Some((value_entry.value.collect(), value_entry.entry))),
            Err(err) => Err((err.values.collect(), err.entry)),
        }
    }

    pub(super) fn collect_drain_result<
        I1: ExactSizeIterator<Item = char>,
        I2: Iterator<Item = EntryAmount<char, 1>>,
        I3: ExactSizeIterator<Item = char>,
    >(
        actual_drain: Result<(I1, I2), PushedTooMany<I3, char, 1>>,
    ) -> Result<Vec<(Vec<char>, char)>, (Vec<char>, char)> {
        match actual_drain {
            Ok(ok) => Ok(collect_drain(ok)),
            Err(err) => Err((err.values.collect(), err.entry)),
        }
    }

    pub(super) fn collect_drain<
        I1: ExactSizeIterator<Item = char>,
        I2: Iterator<Item = EntryAmount<char, 1>>,
    >(
        (mut values, entry_amounts): (I1, I2),
    ) -> Vec<(Vec<char>, char)> {
        let collected = entry_amounts
            .map(|entry_amount| {
                let amount = entry_amount.amount();
                let values: Vec<_> = values.by_ref().take(amount).collect();
                assert_eq!(values.len(), amount);
                (values, entry_amount.entry)
            })
            .collect();
        assert_eq!(values.len(), 0);
        collected
    }
}
