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
//! | 1                    | 1                | [`TransitionLog<OnePerFrame<T>>`] |
//! | 1                    | const `N`        | [`TransitionLog<NPerFrame<N, T>>`] |
//! | 1                    | varying          | [`TransitionLog<WithTimestamp<T>>`] |
//! | 0 or rarely 1        | 1                | [`RareTransitionLog<OnePerFrame<T>>`] |
//! | 0 or rarely 1        | const `N`        | [`RareTransitionLog<NPerFrame<N, T>>`] |
//! | 0 or rarely 1        | varying          | [`RareTransitionLog<WithTimestamp<T>>`] |
//! | varying              | 1                | [`TransitionsLog<T, OnePerFrame<U>, Amount>`] |
//! | varying              | const `N`        | [`TransitionsLog<T, NPerFrame<N, U>, Amount>`] |
//! | varying              | varying          | [`TransitionsLog<T, WithTimestamp<U>, Amount>`] |
//! | 0 or rarely varying  | 1                | [`RareTransitionsLog<T, OnePerFrame<U>>`] |
//! | 0 or rarely varying  | const `N`        | [`RareTransitionsLog<T, NPerFrame<N, U>>`] |
//! | 0 or rarely varying  | varying          | [`RareTransitionsLog<T, WithTimestamp<U>>`] |
//!
//! - For constant `M` amounts of data per push, refer to "data per push": 1 and `[T; M]` as the logged type.
//! - `U` is an optional `Copy` type that can be stored with each push of multiple data. Default is `()`.
//! - `Amount` is the integer type to store the amount of data in each push, allowing memory optimizations. Default is `usize`.
//!
//! # Considerations and alternatives
//!
//! When storing `bool` or other types that can represent two states and one state occurs much more frequently, it makes sense
//! to use [`RareTransitionLog<OnePerFrame<()>>`] and map the `Option<()>` into the desired type where `None` is the more
//! frequent state of the type.
//!
//! If the goal is to just sometimes push into the log, it might be benefitial if the call of `log.push_presence(value)` itself
//! is what happens not every frame instead of wrapping the value into an `Option` while using a [`RareTransitionLog`]. This
//! becomes possible if the condition for this decision is also available during log logic where `log.forward_log()` or
//! `log.backward_log()` must or must not be called. Then [`TransitionLog`] can be used instead which is less memory-consuming and
//! has less logic overhead.

use std::{
    collections::{vec_deque::Drain, VecDeque},
    fmt::Debug,
    num::NonZeroUsize,
};

// Überlegungen für eigentliche value logs:
// - machen für Values/RareValues auch Sinn wenn man zB damit einen Vec befüllen will
// - brauchen einen initialen Wert, desswegen können sie kein Default implementieren
// -- initialer Wert von Values/RareValues ist eine VecDeque, transitions_capacity bei with_capacity entfällt dann
// - andere logik bei backward_log
// - andere logik bei forward, <start anstatt <=start
// - andere logik bei forward_log wenn mit indices anders umgegangen werden muss
// - front_value methoden machen für diese Sinn
// - typen hier drunter brauchen allgemeinere Namen
// - ~~get methode um ohne update an das value zu kommen, erspart es das value gesondert zu halten~~

// jetzt wo man mut transition erhält, lässt sich damit value nachbilden? via mem::swap
/*
val = 10;
---forward:
log.push(value);
value = 20;
---backward log:
entry = log.backward();
std::mem::swap(value, entry);
value == 10
---forward log:
entry = log.forward();
std::mem::swap(value, entry);
value == 20

das klappt nicht bei Values weil die anzahl unterschiedlich ist

alternative: es wird nur der index/range gespeichert wo im log das value ist, unnötig bei value log

*/

// Alternative zu WithTImestamp: forward limitiert anhand von meta log len, zu bevorzugen wenn jedes frame geupdated wird

// todo: pop_front_by_... anstelle von forward methoden, impl reflect

// für mehrere updates per frame: TransitionsLog, aber nicht der ganze range sondern einzelne darin

// neue log art für mehrere updates pro entry wo U: PartialEq

mod rare_transition;
mod rare_transitions;
mod rare_value;
mod rare_values;
mod transition;
mod transitions;
mod value;
mod values;

pub use rare_transition::RareTransitionLog;
pub use rare_transitions::RareTransitionsLog;
pub use transition::TransitionLog;
pub use transitions::TransitionsLog;

use crate::meta::RevMeta;

// A `#[repr(packed)]` wrapper for `Copy` types that may be larger than single bytes but should not bloat parent
// structs with other, smaller fields without having these struct to be packed themselves to prevent `T`'s padding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(packed)]
pub struct Packed<T: Copy>(pub T);

impl<T: Copy> From<T> for Packed<T> {
    fn from(value: T) -> Self {
        Self(value)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct OutOfLog;

#[derive(Debug, Clone, PartialEq)]
pub struct RareData<T> {
    pub data: T,
    pub skips_before_value: Packed<usize>,
}

impl<T> RareData<T> {
    pub fn len(&self) -> NonZeroUsize {
        unsafe {
            // SAFETY: +1 ensures to never be zero
            NonZeroUsize::new_unchecked(self.skips_before_value.0 + 1) // self.data adds to the len
        }
    }
}

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
pub struct AmountErr<'a, T, U, Amount: TryFrom<usize> = usize> {
    pub data: Drain<'a, T>,
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
pub struct WithTimestamp<T = ()> {
    pub data: T,
    pub logged_at: Packed<usize>,
}

impl<T: Default> From<usize> for WithTimestamp<T> {
    fn from(logged_at: usize) -> Self {
        Self {
            data: T::default(),
            logged_at: Packed(logged_at),
        }
    }
}

impl<T: Default> From<&RevMeta> for WithTimestamp<T> {
    fn from(meta: &RevMeta) -> Self {
        Self {
            data: T::default(),
            logged_at: Packed(meta.now()),
        }
    }
}

pub type OnePerFrame<T = ()> = NPerFrame<1, T>;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct NPerFrame<const N: usize, T = ()>(pub T);

impl<const N: usize, T> From<T> for NPerFrame<N, T> {
    fn from(value: T) -> Self {
        Self(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(packed)]
pub struct WithAmount<U = (), Amount: Copy = usize> {
    pub entry: U,
    pub amount: Packed<Amount>,
}

fn amount_to_usize<Amount: TryInto<usize, Error: Debug>>(amount: Amount) -> usize {
    amount
        .try_into()
        .expect("a logged Amount value was converted from usize, so it is expected to be convertible back into usize")
}

const BACKWARD_EXPECT_MSG: &'static str = "self.index should always be <= the log len, so reducing it without underflow is expected to result in a valid index into the log";
