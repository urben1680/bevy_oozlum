use std::{
    collections::{vec_deque::Drain, VecDeque},
    fmt::Debug,
    iter::FusedIterator,
    ops::{Deref, DerefMut},
};

use bevy::{log::error, reflect::Reflect, utils::all_tuples};

use crate::{
    frame::{PackedRevFrame, RevFrame, RevFrameGen, RevFrameNew},
    meta::RevMeta,
    resize_ne_bytes,
};

#[cfg(feature = "serde")]
mod serde_with;

/*
mod init_none;
mod rare_state;
mod rare_states;
mod rare_transition;
mod rare_transitions;
mod state;
mod states;
mod transition;
mod transitions;
*/

mod dense_state;
mod dense_states;
mod dense_transition;
mod dense_transitions;
mod framed_state;
mod framed_states;
mod framed_transition;
mod framed_transitions;
mod sparse_state;
mod sparse_states;
mod sparse_transition;
mod sparse_transitions;

#[cfg(feature = "serde")]
pub use serde_with::{logless_state, logless_with_capacity, with_capacity};

/*
pub use init_none::InitNoneLog;
pub use rare_state::RareStateLog;
pub use rare_states::RareStatesLog;
pub use rare_transition::RareTransitionLog;
pub use rare_transitions::RareTransitionsLog;
pub use state::StateLog;
pub use states::StatesLog;
pub use transition::TransitionLog;
pub use transitions::TransitionsLog;
*/

pub use dense_state::DenseStateLog;
pub use dense_states::DenseStatesLog;
pub use dense_transition::DenseTransitionLog;
pub use dense_transitions::DenseTransitionsLog;

pub use framed_state::FramedStateLog;

pub use sparse_state::SparseStateLog;
pub use sparse_states::SparseStatesLog;
pub use sparse_transition::SparseTransitionLog;
pub use sparse_transitions::SparseTransitionsLog;

#[derive(Debug, Clone, PartialEq)]
pub struct OutOfLog;

/// A `&mut VecDeque<T>` wrapper that does not expose methods which remove from the deque.
pub struct LogMut<'a, T>(&'a mut VecDeque<T>);

impl<'a, T> LogMut<'a, T> {
    pub fn append(&mut self, other: &mut VecDeque<T>) {
        self.0.append(other);
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

pub struct SparseLogMut<'a, T, U> {
    values: &'a mut VecDeque<T>,
    entry: &'a mut Option<U>,
}

impl<'a, T, U> SparseLogMut<'a, T, U> {
    pub fn push(self, entry: U) -> LogMut<'a, T> {
        *self.entry = Some(entry);
        LogMut(self.values)
    }
}

pub struct AmountErrOld<I, Log: WithAmount> {
    pub values: I,
    pub entry: Log::Entry,
    pub pushed_amount: usize,
    // () or Infallible, enabling `let Ok(ok) = result;` syntax
    // https://github.com/rust-lang/rust-analyzer/issues/18334
    _error: Log::Err,
}

#[allow(private_bounds)]
impl<I, Log: WithAmountInternal> AmountErrOld<I, Log> {
    // taking &self makes it easier to call this method
    pub fn max_amount(&self) -> usize {
        Log::amount_to_usize(Log::MAX)
    }
}

// makes unwrap possible without requiring additional Debug bounds everywhere
#[allow(private_bounds)]
impl<I, Log: WithAmountInternal> Debug for AmountErrOld<I, Log> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(std::any::type_name::<Self>())
            .field("pushed_amount", &self.pushed_amount)
            .field("max_amount", &self.max_amount())
            .finish_non_exhaustive()
    }
}

pub struct AmountErr<I, U, const AMOUNT_BYTES: usize> {
    pub values: I,
    pub entry_amount: EntryAmount<U, AMOUNT_BYTES>,
}

impl<I, U, const AMOUNT_BYTES: usize> AmountErr<I, U, AMOUNT_BYTES> {
    pub const MAX_AMOUNT: usize = usize::MAX >> ((USIZE_BYTES - AMOUNT_BYTES) * 8);
    // easier to call with &self during error handling
    pub fn max_amount(&self) -> usize {
        Self::MAX_AMOUNT
    }
}

// makes unwrap possible without requiring additional Debug bounds everywhere
#[allow(private_bounds)]
impl<I, U, const AMOUNT_BYTES: usize> Debug for AmountErr<I, U, AMOUNT_BYTES> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(std::any::type_name::<Self>())
            .field("pushed_amount", &self.entry_amount.amount())
            .field("max_amount", &self.max_amount())
            .finish_non_exhaustive()
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

/// Draining iterator used by some methods of framed logs.
///
/// The frame that the value was logged at is intentionally hidden here as the frame may be invalid.
///
/// See [`vec_deque::Drain`](Drain), that is wrapped here, for further information.
#[derive(Debug)]
// Nameable alternative to `Map<Drain<ValueLoggedAt<T>>, impl FnMut(ValueLoggedAt<T>) -> T>`
pub struct FramedDrain<'a, T>(Drain<'a, ValueLoggedAt<T>>);

impl<T> Iterator for FramedDrain<'_, T> {
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

impl<T> DoubleEndedIterator for FramedDrain<'_, T> {
    #[inline]
    fn next_back(&mut self) -> Option<T> {
        self.0.next_back().map(|rare| rare.value)
    }
}

impl<T> ExactSizeIterator for FramedDrain<'_, T> {}

impl<T> FusedIterator for FramedDrain<'_, T> {}

// Private bounds need no documentation because the `drain_future`
// methods which return this type document these bounds themselves.
#[allow(private_bounds)]
#[derive(Debug, Clone, Reflect)]
pub struct EntryAmountOld<Log: WithAmountInternal> {
    pub entry: Log::Entry,
    amount: Log::Amount,
}

#[allow(private_bounds)]
impl<Log: WithAmountInternal> EntryAmountOld<Log> {
    const fn zero(entry: <Log as WithAmount>::Entry) -> Self {
        Self {
            entry,
            amount: <Log as WithAmountInternal>::MIN,
        }
    }
    // todo: doc example with Iterator::take
    pub fn amount(&self) -> usize {
        <Log as WithAmountInternal>::amount_to_usize(self.amount)
    }
}

#[cfg(feature = "serde")]
impl<Log: WithAmountInternal<Entry: serde::Serialize>> serde::Serialize for EntryAmountOld<Log> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        (&self.entry, self.amount()).serialize(serializer)
    }
}

#[cfg(feature = "serde")]
impl<'de, Log: WithAmountInternal<Entry: serde::Deserialize<'de>>> serde::Deserialize<'de>
    for EntryAmountOld<Log>
{
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let (entry, amount) =
            <(Log::Entry, usize) as serde::Deserialize<'de>>::deserialize(deserializer)?;
        match <Log as WithAmountInternal>::usize_to_amount(amount) {
            Ok(amount) => Ok(Self { entry, amount }),
            Err(_) => Err(serde::de::Error::custom("todo")),
        }
    }
}

/// `EntryAmount` is usually encountered in draining methods of logs with multiple states/transitions per update,
/// for example [`DenseStatesLog`] which will be behind the `log` variable in the following code snippets.
///
/// These methods return two draining iterators, the first with the states/transitions of all updates that are
/// drained, and the second with `EntryAmount`, containing the entry type `U` of the log (if specified) and the
/// amount of states/transitions per update, returned by the [`amount`](Self::amount) method.
#[derive(Debug, Clone, Reflect)]
pub struct EntryAmount<U, const AMOUNT_BYTES: usize = USIZE_BYTES> {
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

    /// Returns the amount of states/transitions of an update.
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
    /// Now iterate the updates:
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
    /// Or collect the updates:
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

#[derive(Debug, Clone, Reflect)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
struct ValueLoggedAt<T> {
    value: T,
    logged_at: PackedRevFrame,
}

impl<T> ValueLoggedAt<T> {
    fn new(meta: &RevMeta, value: T) -> Self {
        Self {
            value,
            logged_at: meta.present_world_state().into(),
        }
    }
    fn logged_at(&self) -> PackedRevFrame {
        self.logged_at
    }
    fn into_inner(self) -> T {
        self.value
    }
}

impl<T> Deref for ValueLoggedAt<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl<T> DerefMut for ValueLoggedAt<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.value
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

/* 
/// See [`VecDeque::partition_point`], is limited to the first `max` entries.
fn partition_point<T: LoggedAt>(deque: &VecDeque<T>, max: usize, meta: &RevMeta) -> usize {
    let (mut front, mut back) = deque.as_slices();
    let front_max = front.len().min(max);
    let back_max = back.len().min(max - front_max);
    front = &front[..front_max];
    back = &back[..back_max];

    let pred =
        |entry: &T| meta.present_world_state() - entry.logged_at() > meta.past_world_states();

    if back.first().map(|v| pred(v)) == Some(true) {
        back.partition_point(pred) + front.len()
    } else {
        front.partition_point(pred)
    }
}
*/

#[derive(Clone, Copy, Debug, Reflect)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
struct FramedMeta {
    last_push: RevFrameNew,
    deque_elems_of_this_frame: usize,
}

impl FramedMeta {
    fn new(meta: &RevMeta) -> Self {
        Self {
            last_push: meta.present_world_state(),
            deque_elems_of_this_frame: 0,
        }
    }
    fn push_and_len_to_drain_past<T>(
        &mut self,
        meta: &RevMeta,
        deque: &mut VecDeque<ValueLoggedAt<T>>,
        present_state: Option<&mut ValueLoggedAt<T>>,
        index: &mut usize,
        value: T,
    ) -> usize {
        // update `deque` and `deque_elems_of_this_frame`
        deque.truncate(*index);
        let mut value = ValueLoggedAt::new(meta, value);
        match (present_state, self.last_push == meta.present_world_state()) {
            (Some(state), true) => {
                value = core::mem::replace(state, value);
                self.deque_elems_of_this_frame += 1;
            }
            (Some(state), _) => {
                value = core::mem::replace(state, value);
                // `value` is not of this frame, pushing it does not set this to 1
                self.deque_elems_of_this_frame = 0;
            }
            (None, true) => {
                self.deque_elems_of_this_frame += 1;
            }
            (None, _) => {
                self.deque_elems_of_this_frame = 1;
            }
        }
        deque.push_back(value);

        // find amount of past entries out of log
        let max_drain = deque.len() - self.deque_elems_of_this_frame;
        let mut to_drain = max_drain;
        if meta.past_contains(self.last_push) {
            // used `VecDeque::partition_point` source with an upper limit `max_drain`
            let (mut front, mut back) = deque.as_slices();
            let front_max = front.len().min(max_drain);
            let back_max = back.len().min(max_drain - front_max);
            front = &front[..front_max];
            back = &back[..back_max];

            let now_packed = meta.present_world_state().without_generation();
            let past_frames = meta.past_world_states();
            let pred = |value: &ValueLoggedAt<T>| now_packed - value.logged_at() > past_frames;
            if let Some(true) = back.first().map(|v| pred(v)) {
                to_drain = back[1..].partition_point(pred) + front.len() + 1;
            } else {
                to_drain = front.partition_point(pred);
            }
        }

        // update values for after the draining
        self.last_push = meta.present_world_state();
        *index = deque.len() - to_drain;

        to_drain
    }
    fn forward_log(&mut self, frame: PackedRevFrame) {
        self.last_push = self.last_push.of_future_packed(frame);
        self.deque_elems_of_this_frame = 0;
    }
    fn backward_log(&mut self, frame: PackedRevFrame) {
        self.last_push = self.last_push.of_past_packed(frame);
        self.deque_elems_of_this_frame = 0;
    }
}

/// Logged types that contain the information when these were logged, for example
/// by containing [`RevFrame`] or the more compact [`PackedRevFrame`] from
/// [`RevMeta::present_world_state`](crate::meta::RevMeta::present_world_state).
pub trait LoggedAt {
    fn logged_at(&self) -> RevFrame;
}

impl LoggedAt for RevFrame {
    fn logged_at(&self) -> RevFrame {
        *self
    }
}

impl LoggedAt for PackedRevFrame {
    fn logged_at(&self) -> RevFrame {
        RevFrame((*self).into())
    }
}

impl<Log: WithAmountInternal<Entry: LoggedAt>> LoggedAt for EntryAmountOld<Log> {
    fn logged_at(&self) -> RevFrame {
        self.entry.logged_at()
    }
}

impl<T: LoggedAt> LoggedAt for SparseValue<T> {
    fn logged_at(&self) -> RevFrame {
        self.value.logged_at()
    }
}

macro_rules! impl_logged_at {
    ($($T: ident),*) => {
        impl<$($T,)* U: LoggedAt> LoggedAt for ($($T,)* U) {
            fn logged_at(&self) -> RevFrame {
                #[allow(non_snake_case, unused_variables)]
                let ($($T,)* logged_at) = self;
                logged_at.logged_at()
            }
        }
    };
}

all_tuples!(impl_logged_at, 1, 20, T);

trait NotUSize {} // remove if bounds on const generics (> 0) or type inequality (!= usize) stabilizes
impl<const AMOUNT_BYTES: usize> NotUSize for [u8; AMOUNT_BYTES] {}

trait WithAmountInternal: WithAmount {
    type Amount: Debug + Copy + Clone + Send + Sync + 'static;
    const MIN: Self::Amount;
    const MAX: Self::Amount;
    fn amount_to_usize(value: Self::Amount) -> usize;
    fn usize_to_amount(value: usize) -> Result<Self::Amount, Self::Err>;
}

#[derive(Debug, Clone, Copy)]
pub struct AmountOverflow;

pub trait WithAmount {
    type Err: Debug;
    #[doc(hidden)]
    type Entry; // including this type here simplifies `AmountErr` and `EntryAmount`
}

macro_rules! doc_with_amount {
    (struct) => {
        "
        
        The const generic parameter `AMOUNT_BYTES` makes it possible to reduce the memory usage of this log:
        
        - **unspecified or `0`**: the amount of values per push is stored as `usize` and only infallable
          methods and functions can be used.
        - **in `1..size_of::<usize>()`**: the amount of values per push is stored as an `[u8; AMOUNT_BYTES]` and
          only the fallible methods and functions can be used. This has the benefit to consume less memory per push.
          This allows storing up to `2^AMOUNT_BYTES - 1` values per push.
        - **in `size_of::<usize>()..=8`**: the amount of values per push is stored as a `[u8; size_of::<usize>()]`
          and both the infallible and fallible (which never fail) methods and functions can be used. It may be
          helpful to still use the fallible methods in case that the application runs on machines with different
          pointer widths and only for some of them the conversation is fallible. That makes the code more agnostic
          for the target machine.
          
        The latter two cases have the additional benefit that the byte array has an alignment of `1` which
        may add less or no padding along a non-ZST `U` of this struct."
    };
    (impl) => {
        doc_with_amount!(concat, "unspecified or in `0..=8`")
    };
    (impl where NotUsize) => {
        doc_with_amount!(concat, "in `1..=8`")
    };
    (impl where Infallible) => {
        doc_with_amount!(concat, "unspecified or `0` or in `size_of::<usize>()..=8`")
    };
    (concat, $text: literal) => {
        std::concat!(
            "These methods are implemented with the const generic parameter `AMOUNT_BYTES` being",
            $text,
            doc_with_amount!(ref struct)
        )
    };
    (try 0) => {
        std::concat!(
            "Implements only infallible methods and functions",
            doc_with_amount!(ref struct)
        )
    };
    (try) => {
        std::concat!(
            "Implements both fallible and infallible methods and functions. If `AMOUNT_BYTES` is not less than
            `size_of::<usize>()`, the fallible methods and functions never return `Err`.",
            doc_with_amount!(ref struct)
        )
    };
    (ref struct) => {
        ".
        
        See the struct documentation for further detail on `AMOUNT_BYTES`."
    };
}

use doc_with_amount;

macro_rules! impl_with_amount {
    ($Log: ident) => {
        #[doc = crate::log::doc_with_amount!(try 0)]
        impl<T, U> crate::log::WithAmount for $Log<T, U, 0> {
            type Err = std::convert::Infallible;
            type Entry = U;
        }

        impl<T, U> crate::log::WithAmountInternal for $Log<T, U, 0> {
            type Amount = usize;
            const MIN: Self::Amount = usize::MIN;
            const MAX: Self::Amount = usize::MAX;
            fn amount_to_usize(value: Self::Amount) -> usize {
                value
            }
            fn usize_to_amount(value: usize) -> Result<Self::Amount, Self::Err> {
                Ok(value)
            }
        }

        #[cfg(target_pointer_width = "16")]
        const _: () = {
            impl_with_amount!($Log, 1);
            impl_with_amount!($Log, 2, Infallible);
            impl_with_amount!($Log, 3, Infallible);
            impl_with_amount!($Log, 4, Infallible);
            impl_with_amount!($Log, 5, Infallible);
            impl_with_amount!($Log, 6, Infallible);
            impl_with_amount!($Log, 7, Infallible);
            impl_with_amount!($Log, 8, Infallible);
        };

        #[cfg(target_pointer_width = "32")]
        const _: () = {
            impl_with_amount!($Log, 1);
            impl_with_amount!($Log, 2);
            impl_with_amount!($Log, 3);
            impl_with_amount!($Log, 4, Infallible);
            impl_with_amount!($Log, 5, Infallible);
            impl_with_amount!($Log, 6, Infallible);
            impl_with_amount!($Log, 7, Infallible);
            impl_with_amount!($Log, 8, Infallible);
        };

        #[cfg(target_pointer_width = "64")]
        const _: () = {
            impl_with_amount!($Log, 1);
            impl_with_amount!($Log, 2);
            impl_with_amount!($Log, 3);
            impl_with_amount!($Log, 4);
            impl_with_amount!($Log, 5);
            impl_with_amount!($Log, 6);
            impl_with_amount!($Log, 7);
            impl_with_amount!($Log, 8, Infallible);
        };
    };
    ($Log: ident, $AMOUNT_BYTES: literal) => {
        #[doc = crate::log::doc_with_amount!(try)]
        impl<T, U> crate::log::WithAmount for $Log<T, U, $AMOUNT_BYTES> {
            type Err = crate::log::AmountOverflow;
            type Entry = U;
        }

        impl<T, U> crate::log::WithAmountInternal for $Log<T, U, $AMOUNT_BYTES> {
            type Amount = [u8; $AMOUNT_BYTES];
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
                    Err(crate::log::AmountOverflow)
                }
            }
        }
    };
    ($Log: ident, $AMOUNT_BYTES: literal, Infallible) => {
        #[doc = crate::log::doc_with_amount!(try)]
        impl<T, U> crate::log::WithAmount for $Log<T, U, $AMOUNT_BYTES> {
            type Err = std::convert::Infallible;
            type Entry = U;
        }

        impl<T, U> crate::log::WithAmountInternal for $Log<T, U, $AMOUNT_BYTES> {
            type Amount = [u8; crate::log::USIZE_BYTES as usize];
            const MIN: Self::Amount = [u8::MIN; crate::log::USIZE_BYTES as usize];
            const MAX: Self::Amount = [u8::MAX; crate::log::USIZE_BYTES as usize];
            fn amount_to_usize(value: Self::Amount) -> usize {
                usize::from_ne_bytes(value)
            }
            fn usize_to_amount(value: usize) -> Result<Self::Amount, Self::Err> {
                Ok(value.to_ne_bytes())
            }
        }
    };
}

use impl_with_amount;

#[cfg(test)]
mod test {
    pub(super) fn collect_pop_result<I1: Iterator<Item = char>, I2: Iterator<Item = char>>(
        actual_pop: Result<Option<ValueEntry<I1, char>>, AmountErr<I2, char, 1>>,
    ) -> Result<Option<(Vec<char>, char)>, (Vec<char>, char)> {
        match actual_pop {
            Ok(None) => Ok(None),
            Ok(Some(value_entry)) => Ok(Some((value_entry.value.collect(), value_entry.entry))),
            Err(err) => Err((err.values.collect(), err.entry_amount.entry)),
        }
    }

    pub(super) fn collect_drain_result<
        I1: ExactSizeIterator<Item = char>,
        I2: Iterator<Item = EntryAmount<char, 1>>,
        I3: Iterator<Item = char>,
    >(
        actual_drain: Result<(I1, I2), AmountErr<I3, char, 1>>,
    ) -> Result<Vec<(Vec<char>, char)>, (Vec<char>, char)> {
        match actual_drain {
            Ok(ok) => Ok(collect_drain(ok)),
            Err(err) => Err((err.values.collect(), err.entry_amount.entry)),
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

    #[derive(Debug, Clone, Copy)]
    pub(super) enum ShortenStrategy {
        PopPastByLen,
        DrainPastByLen,
        PopPastByLoggedAt,
        DrainPastByLoggedAt,
    }

    impl ShortenStrategy {
        pub(super) const VARIANTS: [Self; 4] = [
            Self::PopPastByLen,
            Self::DrainPastByLen,
            Self::PopPastByLoggedAt,
            Self::DrainPastByLoggedAt,
        ];
    }

    macro_rules! shorten_strategy {
        // single value per log entry
        ($log: expr, $meta: expr, $strategy: expr, $len: expr, $before: expr, $after_push: expr) => {
            match $strategy {
                ShortenStrategy::PopPastByLen => $log.pop_past_by_len($len as usize),
                ShortenStrategy::PopPastByLoggedAt => $log.pop_past_by_logged_at($meta),
                ShortenStrategy::DrainPastByLen | ShortenStrategy::DrainPastByLoggedAt => {
                    let mut actual_popped: Vec<_> = match $strategy {
                        ShortenStrategy::DrainPastByLen => {
                            $log.drain_past_by_len($len as usize).collect()
                        }
                        ShortenStrategy::DrainPastByLoggedAt => {
                            $log.drain_past_by_logged_at($meta).collect()
                        }
                        _ => unreachable!(),
                    };
                    assert!(
                        actual_popped.len() <= 1,
                        "\nmeta: {:#?}\nbefore: {:#?}\nafter_push: {:#?}\nafter_pop: {:#?}\npopped: {actual_popped:#?}",
                        $meta, $before, $after_push, $log
                    );
                    actual_popped.pop()
                }
            }.map(|(value, logged_at)| (value, u32::from(logged_at)))
        };
        // multiple values per log entry
        ($log: expr, $meta: expr, $strategy: expr, $len: expr) => {
            match $strategy {
                ShortenStrategy::PopPastByLen => $log
                    .pop_past_by_len($len as usize)
                    .map(|value_entry| (
                        value_entry.value.collect::<Vec<_>>(),
                        u32::from(value_entry.entry),
                    ))
                    .unzip(),
                ShortenStrategy::PopPastByLoggedAt => $log
                    .pop_past_by_logged_at($meta)
                    .map(|value_entry| (
                        value_entry.value.collect::<Vec<_>>(),
                        u32::from(value_entry.entry),
                    ))
                    .unzip(),
                ShortenStrategy::DrainPastByLen | ShortenStrategy::DrainPastByLoggedAt => {
                    let actual_popped: Vec<_> = match $strategy {
                        ShortenStrategy::DrainPastByLen => {
                            $log.drain_past_by_len($len as usize).collect()
                        }
                        ShortenStrategy::DrainPastByLoggedAt => {
                            $log.drain_past_by_logged_at($meta).collect()
                        }
                        _ => unreachable!(),
                    };
                    ((!actual_popped.is_empty()).then_some(actual_popped), None)
                }
            }
        };
    }

    pub(super) use shorten_strategy;

    use super::{AmountErr, EntryAmount, ValueEntry};
}
