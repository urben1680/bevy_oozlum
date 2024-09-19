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

use std::{collections::VecDeque, fmt::Debug, iter::FusedIterator};

use bevy::reflect::Reflect;

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

type PackedUSize = [u8; usize::BITS as usize / 8];

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
    logged_at: PackedUSize,
}

impl<T: Debug> Debug for WithTimestamp<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(std::any::type_name::<Self>())
            .field("value", &self.value)
            .field("logged_at", &usize::from_ne_bytes(self.logged_at))
            .finish()
    }
}

impl<T> WithTimestamp<T> {
    pub fn new(value: T, logged_at: usize) -> Self {
        Self {
            value,
            logged_at: logged_at.to_ne_bytes(),
        }
    }
    pub fn logged_at(&self) -> usize {
        usize::from_ne_bytes(self.logged_at)
    }
}

impl<T: Default> From<usize> for WithTimestamp<T> {
    fn from(logged_at: usize) -> Self {
        Self {
            value: T::default(),
            logged_at: logged_at.to_ne_bytes(),
        }
    }
}

impl<T: Default> From<&RevMeta> for WithTimestamp<T> {
    fn from(meta: &RevMeta) -> Self {
        Self {
            value: T::default(),
            logged_at: meta.now().to_ne_bytes(),
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
    skips: PackedUSize,
}

impl<T: Debug> Debug for RareValue<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(std::any::type_name::<Self>())
            .field("value", &self.value)
            .field("skips", &usize::from_ne_bytes(self.skips))
            .finish()
    }
}

impl<T> RareValue<T> {
    fn len(&self) -> usize {
        usize::from_ne_bytes(self.skips) + 1 // `self.data` adds to the len
    }
    fn skips(&self) -> usize {
        usize::from_ne_bytes(self.skips)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Reflect)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
struct EntryAmount<U, A> {
    entry: U,
    amount: A,
}

impl<U, A: Copy> EntryAmount<U, A> {
    fn amount<T: WithAmount<Amount = A>>(&self) -> usize {
        <T as WithAmount>::amount_to_usize(self.amount)
    }
}

const INDEX_OOB: &'static str = "self.index should always be <= the deque len, so successfully reducing it without underflow is expected to result in a valid index into the log which is not the case here";

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
    const ZERO: Self::Amount;
    fn amount_to_usize(value: Self::Amount) -> usize;
    fn usize_to_amount(value: usize) -> Result<Self::Amount, Self::Err>;
}

macro_rules! impl_with_amount {
    ($Log: ident) => {
        impl_with_amount!($Log, 0);

        #[cfg(target_pointer_width = "16")]
        mod target_16 {
            use super::{impl_with_amount, $Log, WithAmount};

            impl_with_amount!($Log, 1);
            impl_with_amount!($Log, 2, Infallible);
        }

        #[cfg(target_pointer_width = "32")]
        mod target_32 {
            use super::{impl_with_amount, $Log, WithAmount};

            impl_with_amount!($Log, 1);
            impl_with_amount!($Log, 2);
            impl_with_amount!($Log, 3);
            impl_with_amount!($Log, 4, Infallible);
        }

        #[cfg(target_pointer_width = "64")]
        mod target_64 {
            use super::{impl_with_amount, $Log, WithAmount};
            
            impl_with_amount!($Log, 1);
            impl_with_amount!($Log, 2);
            impl_with_amount!($Log, 3);
            impl_with_amount!($Log, 4);
            impl_with_amount!($Log, 5);
            impl_with_amount!($Log, 6);
            impl_with_amount!($Log, 7);
            impl_with_amount!($Log, 8, Infallible);
        }
    };
    ($Log: ident, 0) => {
        impl<T, U> WithAmount for $Log<T, U, 0> {
            type Amount = usize;
            type Err = std::convert::Infallible;
            const ZERO: Self::Amount = 0;
            fn amount_to_usize(value: Self::Amount) -> usize {
                value
            }
            fn usize_to_amount(value: usize) -> Result<Self::Amount, Self::Err> {
                Ok(value)
            }
        }
    };
    ($Log: ident, $AMOUNT_BYTES: literal) => {
        impl<T, U> WithAmount for $Log<T, U, $AMOUNT_BYTES> {
            type Amount = [u8; $AMOUNT_BYTES];
            type Err = usize;
            const ZERO: Self::Amount = [0; $AMOUNT_BYTES];
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
                    Err(Self::amount_to_usize([u8::MAX; $AMOUNT_BYTES]))
                }
            }
        }
    };
    ($Log: ident, $AMOUNT_BYTES: literal, Infallible) => {
        impl<T, U> WithAmount for $Log<T, U, $AMOUNT_BYTES> {
            type Amount = [u8; $AMOUNT_BYTES];
            type Err = std::convert::Infallible;
            const ZERO: Self::Amount = [0; $AMOUNT_BYTES];
            fn amount_to_usize(value: Self::Amount) -> usize {
                let mut i = value.into_iter();
                usize::from_ne_bytes(std::array::from_fn(|_| i.next().unwrap_or(0)))
            }
            fn usize_to_amount(value: usize) -> Result<Self::Amount, Self::Err> {
                let mut i = value.to_ne_bytes().into_iter();
                Ok(std::array::from_fn(|_| i.next().unwrap_or(0)))
            }
        }
    };
}

use impl_with_amount;
