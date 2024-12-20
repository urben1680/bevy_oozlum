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
//!         _ => ("How to undo/redo changes? Which values did the components have?")
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
    collections::{vec_deque::Drain, TryReserveError, VecDeque},
    fmt::Debug,
    iter::FusedIterator,
    ops::Deref,
};

use bevy::{log::error, reflect::Reflect, utils::all_tuples};

mod init_none;
mod rare_state;
mod rare_states;
mod rare_transition;
mod rare_transitions;
#[cfg(feature = "serde")]
mod serde_with;
mod state;
mod states;
mod transition;
mod transitions;

pub use init_none::InitNoneLog;
pub use rare_state::RareStateLog;
pub use rare_states::RareStatesLog;
pub use rare_transition::RareTransitionLog;
pub use rare_transitions::RareTransitionsLog;
#[cfg(feature = "serde")]
pub use serde_with::{logless_state, logless_with_capacity, with_capacity};
pub use state::StateLog;
pub use states::StatesLog;
pub use transition::TransitionLog;
pub use transitions::TransitionsLog;

#[derive(Debug, Clone, PartialEq)]
pub struct OutOfLog;

/// A `&mut VecDeque<T>` wrapper that does not expose methods which remove from the deque.
pub struct LogMut<'a, T>(&'a mut VecDeque<T>);

impl<'a, T> LogMut<'a, T> {
    pub fn append(&mut self, other: &mut VecDeque<T>) {
        self.0.append(other);
    }
    pub fn push_back(&mut self, value: T) {
        self.0.push_back(value);
    }
    pub fn reserve(&mut self, additional: usize) {
        self.0.reserve(additional)
    }
    pub fn reserve_exact(&mut self, additional: usize) {
        self.0.reserve_exact(additional)
    }
    pub fn try_reserve(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.0.try_reserve(additional)
    }
    pub fn try_reserve_exact(&mut self, additional: usize) -> Result<(), TryReserveError> {
        self.0.try_reserve_exact(additional)
    }
    pub fn shrink_to(&mut self, min_capacity: usize) {
        self.0.shrink_to(min_capacity)
    }
    pub fn shrink_to_fit(&mut self) {
        self.0.shrink_to_fit()
    }
}

impl<'a, T> Deref for LogMut<'a, T> {
    type Target = VecDeque<T>;
    fn deref(&self) -> &Self::Target {
        self.0
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

pub struct AmountErr<I, Log: WithAmount> {
    pub values: I,
    pub entry: Log::Entry,
    pub pushed_amount: usize,
    // () or Infallible, enabling `let Ok(ok) = result;` syntax
    // https://github.com/rust-lang/rust-analyzer/issues/18334
    _error: Log::Err,
}

#[allow(private_bounds)]
impl<I, Log: WithAmountInternal> AmountErr<I, Log> {
    // taking &self makes it easier to call this method
    pub fn max_amount(&self) -> usize {
        Log::amount_to_usize(Log::MAX)
    }
}

// makes unwrap possible without requiring additional Debug bounds everywhere
#[allow(private_bounds)]
impl<I, Log: WithAmountInternal> Debug for AmountErr<I, Log> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(std::any::type_name::<Self>())
            .field("pushed_amount", &self.pushed_amount)
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
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))] // todo: manual impl with usize as intermediate skips value
struct RareValue<T> {
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
    skips_ne: [u8; USIZE_BYTES as usize],
}

impl<T: Debug> Debug for RareValue<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(std::any::type_name::<Self>())
            .field("value", &self.value)
            .field("skips", &self.skips())
            .finish()
    }
}

impl<T> RareValue<T> {
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

/// Draining iterator used by some methods of rare logs.
///
/// See [`vec_deque::Drain`](Drain), that is wrapped here, for further information.
#[derive(Debug)]
// Nameable alternative to `Map<Drain<RareValue<T>>, impl FnMut(RareValue<T>) -> T>`
pub struct RareDrain<'a, T>(Drain<'a, RareValue<T>>);

impl<T> Iterator for RareDrain<'_, T> {
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

impl<T> DoubleEndedIterator for RareDrain<'_, T> {
    #[inline]
    fn next_back(&mut self) -> Option<T> {
        self.0.next_back().map(|rare| rare.value)
    }
}

impl<T> ExactSizeIterator for RareDrain<'_, T> {}

impl<T> FusedIterator for RareDrain<'_, T> {}

// Private bounds need no documentation because the `drain_future`
// methods which return this type document these bounds themselves.
#[allow(private_bounds)]
#[derive(Debug, Clone, Reflect)]
pub struct EntryAmount<Log: WithAmountInternal> {
    pub entry: Log::Entry,
    amount: Log::Amount,
}

#[allow(private_bounds)]
impl<Log: WithAmountInternal> EntryAmount<Log> {
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
impl<Log: WithAmountInternal<Entry: serde::Serialize>> serde::Serialize for EntryAmount<Log> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        (&self.entry, self.amount()).serialize(serializer)
    }
}

#[cfg(feature = "serde")]
impl<'de, Log: WithAmountInternal<Entry: serde::Deserialize<'de>>> serde::Deserialize<'de>
    for EntryAmount<Log>
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

fn index_oob() -> OutOfLog {
    error!("self.index should always be <= the deque len, so successfully reducing \
        it without underflow is expected to result in a valid index into the log which is not the case here, \
        the log is in an invalid state before calling the current method, this is a crate bug"
    );
    OutOfLog
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

impl<Log: WithAmountInternal<Entry: LoggedAt>> LoggedAt for EntryAmount<Log> {
    fn logged_at(&self) -> RevFrame {
        self.entry.logged_at()
    }
}

impl<T: LoggedAt> LoggedAt for RareValue<T> {
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

use crate::{PackedRevFrame, RevFrame};

#[cfg(test)]
mod test {
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
                            $log.truncate_future_drain_past_by_logged_at($meta).collect()
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
                            $log.truncate_future_drain_past_by_logged_at($meta).collect()
                        }
                        _ => unreachable!(),
                    };
                    ((!actual_popped.is_empty()).then_some(actual_popped), None)
                }
            }
        };
    }

    pub(super) use shorten_strategy;
}
