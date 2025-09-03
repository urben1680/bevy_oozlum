use std::{
    collections::{VecDeque, vec_deque::Drain},
    error::Error,
    fmt::{Debug, Display},
    iter::FusedIterator,
};

mod transition;
mod transitions;
mod past_len;

pub use transition::TransitionLog;
pub use transitions::TransitionsLog;

pub use past_len::PastLenBackwardLog;
pub use past_len::PastLenForwardLog;
pub use past_len::PastLenLog;
pub use past_len::PastLenNotLog;
pub use past_len::PreUpdateVariant;

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct OutOfLog;

impl Display for OutOfLog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "a log was traversed beyond it's bounds or it was attempted to queue RevMeta to a frame outside the log"
        )
    }
}

impl Error for OutOfLog {}

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

#[derive(Clone, PartialEq)]
struct SparseValue<T> {
    value: T,
    /// If `T` is a state, then these are the skips _after_ the state.
    ///
    /// If `T` is a transiton, then these are the skips _before_ the transition.
    skips_ne: [u8; USIZE_BYTES],
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
#[derive(Debug, Clone)]
pub struct EntryAmount<U> {
    pub entry: U,

    /// The amount of transitions of an update. This can be useful to chunk them.
    ///
    /// # Examples
    ///
    /// With `log` being [`&mut DenseStatesLog`](DenseStatesLog) and [`DenseStatesLog::drain_future`] returning the
    /// draining iterators, do this to chunk them by updates:
    ///
    /// ```
    /// # let mut log = library::log::DenseStatesLog::<i32, (), 1>::new([0], ());
    /// let (mut future_states, future_entry_amounts) = log.drain_future();
    /// ```
    ///
    /// Now iterate...
    /// ```
    /// # let mut log = library::log::DenseStatesLog::<i32, (), 1>::new([0], ());
    /// # let (mut future_states, future_entry_amounts) = log.drain_future();
    /// for entry_amount in future_entry_amounts {
    ///     let entry = entry_amount.entry;
    ///     for future_states in future_states.by_ref().take(entry_amount.amount()) {
    ///         // logic
    ///     }
    /// }
    /// ```
    ///
    /// ...or collect them:
    /// ```
    /// # let mut log = library::log::DenseStatesLog::<i32, (), 1>::new([0], ());
    /// # let (mut future_states, future_entry_amounts) = log.drain_future();
    /// let updates: Vec<(Vec<_>, _)> = future_entry_amounts.map(|entry_amount| (
    ///     future_states.by_ref().take(entry_amount.amount()).collect(),
    ///     entry_amount.entry
    /// )).collect();
    /// ```
    pub amount: usize,
}

#[derive(Copy, Clone)]
struct AmountArray<const AMOUNT_BYTES: usize>([u8; AMOUNT_BYTES]);

impl<U> EntryAmount<U> {
    const fn zero(entry: U) -> Self {
        Self {
            entry,
            amount: 0,
        }
    }
    fn new(entry: U, amount: usize) -> Self {
        Self {
            entry,
            amount,
        }
    }
}

const INDEX_OOB: &'static str = "self.index should always be <= the deque len, so successfully reducing \
    it without underflow is expected to result in a valid index into the log but this is not the case here, \
    the log was in an invalid state before calling the current method, this is a crate bug or the log was \
    deserialized with invalid data";
