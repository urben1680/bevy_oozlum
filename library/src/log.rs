use std::{
    collections::{
        vec_deque::{Drain, IterMut},
        VecDeque,
    },
    fmt::Debug,
    ops::Deref,
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
// - clear methoden für beide? value variante braucht dann einen neuen initial wert als parameter
// - get methode um ohne update an das value zu kommen, erspart es das value gesondert zu halten

// Alternative zu WithTImestamp: forward limitiert anhand von meta log len, zu bevorzugen wenn jedes frame geupdated wird

// todo: pop_front_by_... anstelle von forward methoden

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

#[derive(Debug, Clone, PartialEq)]
pub struct OutOfLog;

#[derive(Debug, Clone, PartialEq)]
pub struct RareData<T> {
    pub data: T,
    pub skips_before_value: usize,
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
pub struct AmountErr<'a, T, U, Amount: TryFrom<usize>> {
    pub data: Drain<'a, T>,
    pub entry: U,
    pub err: Amount::Error,
}

#[derive(Debug, Clone)]
pub struct DataEntry<T, U> {
    pub data: T,
    pub entry: U,
}

impl<'a, T, U> IntoIterator for &'a mut DataEntry<IterMut<'a, T>, U> {
    type IntoIter = &'a mut IterMut<'a, T>;
    type Item = &'a mut T;
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
    pub logged_at: usize,
}

impl<T: Default> From<usize> for WithTimestamp<T> {
    fn from(logged_at: usize) -> Self {
        Self {
            data: T::default(),
            logged_at,
        }
    }
}

impl<T: Default> From<&RevMeta> for WithTimestamp<T> {
    fn from(meta: &RevMeta) -> Self {
        Self {
            data: T::default(),
            logged_at: meta.now(),
        }
    }
}



impl<T> Deref for WithTimestamp<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

/// Configuration for logs that should be limited by their log length. This goal is
/// relaxed if it is sure a missing `pop_front` will not cause reallocations later.
///
/// Intended for logs that call their `forward` method once at every update.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LimitLen<T = ()>(T);

impl<T> Deref for LimitLen<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// For `forward` methods of [`LimitLen`] transition logs.
#[inline(always)]
fn should_pop_transition_at_push(current_len: usize, meta: &RevMeta) -> bool {
    // plus 2 because
    // - for n world states (meta.len()) there can be only n-1 transitions (+1)
    // - pushing to the log adds another entry to the len that is considered here (+1)
    // map_or default true is a consequence of the above as meta.len() would have be
    // greater than usize::MAX to return false in these cases which is impossible.
    //
    // todo: limit on RevMeta so current_len never can become that large
    current_len
        .checked_add(2)
        .map_or(true, |len| len > meta.range().len())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WithAmount<U, Amount> {
    pub entry: U,
    pub amount: Amount,
}

fn amount_to_usize<Amount: TryInto<usize, Error: Debug>>(amount: Amount) -> usize {
    amount
        .try_into()
        .expect("a logged Amount value was converted from usize, so it is expected to be convertible back into usize")
}

const BACKWARD_EXPECT_MSG: &'static str = "self.index should always be <= the log len, so reducing it without underflow is expected to result in a valid index into the log";
