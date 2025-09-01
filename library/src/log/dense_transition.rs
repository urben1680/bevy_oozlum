use core::fmt::Debug;
use std::collections::{
    TryReserveError, VecDeque,
    vec_deque::{Drain, Iter},
};

use crate::{log::{past_len::WithPastLenLog, PastLenBackwardLog, PastLenForwardLog, PreLogUpdate}, meta::{PreLogUpdateState, RevMeta}};

use super::{INDEX_OOB, OutOfLog};

#[derive(Debug)]
pub struct DenseTransitionLog<T> {
    transitions: VecDeque<T>,
    index: usize,
    pre_update: PreLogUpdateState
}

impl<T> Default for DenseTransitionLog<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> DenseTransitionLog<T> {
    pub const fn new() -> Self {
        Self {
            transitions: VecDeque::new(),
            index: 0,
            pre_update: PreLogUpdateState::new()
        }
    }
    pub fn push_and_pop_past(&mut self, max_past_len: usize, transition: T) -> Option<T> {
        self.transitions.truncate(self.index);
        self.transitions.push_back(transition);
        if self.index >= max_past_len {
            self.transitions.pop_front()
        } else {
            self.index = self.transitions.len();
            None
        }
    }
    pub fn push_and_drain_past(&mut self, max_past_len: usize, transition: T) -> Drain<T> {
        self.transitions.truncate(self.index);
        self.transitions.push_back(transition);
        let to_drain = self.transitions.len().saturating_sub(max_past_len);
        self.index = self.transitions.len() - to_drain;
        self.transitions.drain(..to_drain)
    }
    pub(super) fn push_and_iter_to_drain_past(
        &mut self,
        max_past_len: usize,
        transition: T,
    ) -> Iter<T> {
        self.transitions.truncate(self.index);
        self.transitions.push_back(transition);
        let to_drain = self.transitions.len().saturating_sub(max_past_len);
        self.index = self.transitions.len() - to_drain;
        self.transitions.range(..to_drain)
    }
    pub(super) fn drain_past(&mut self, to_drain: usize) -> Drain<T> {
        self.transitions.drain(..to_drain)
    }
    fn truncate_future(&mut self) {
        self.transitions.truncate(self.index);
    }
    fn drain_future(&mut self) -> Drain<T> {
        self.transitions.drain(self.index..)
    }
    fn clear(&mut self) {
        self.transitions.clear();
        self.index = 0;
    }
    pub fn backward_log(&mut self) -> Result<&mut T, OutOfLog> {
        let index = self.index.checked_sub(1).ok_or(OutOfLog)?;
        let transition = self.transitions.get_mut(index).expect(INDEX_OOB);
        self.index = index;
        Ok(transition)
    }
    pub fn forward_log(&mut self) -> Result<&mut T, OutOfLog> {
        self.transitions
            .get_mut(self.index)
            .inspect(|_| self.index += 1)
            .ok_or(OutOfLog)
    }
    pub fn pre_update(&mut self, meta: &RevMeta) {
        match self.pre_update.check(meta) {
            PreLogUpdate::Clear => {
                self.transitions.clear();
                self.index = 0;
            },
            PreLogUpdate::TruncateOrDrainFuture => self.transitions.truncate(self.index),
            PreLogUpdate::Nothing => {}
        }
    }
    pub fn pre_update_drain_past(&mut self, meta: &RevMeta) -> Drain<T> {
        match self.pre_update.check(meta) {
            PreLogUpdate::Clear => {
                self.transitions.truncate(self.index);
                self.index = 0;
                return self.transitions.drain(..);
            },
            PreLogUpdate::TruncateOrDrainFuture => self.transitions.truncate(self.index),
            PreLogUpdate::Nothing => {}
        }
        self.transitions.drain(..0)
    }
    pub fn pre_update_drain_future(&mut self, meta: &RevMeta) -> Drain<T> {
        match self.pre_update.check(meta) {
            PreLogUpdate::Clear => {
                self.transitions.drain(..self.index);
                self.index = 0;
                self.transitions.drain(..)
            },
            PreLogUpdate::TruncateOrDrainFuture => {
                self.transitions.drain(self.index..)
            },
            PreLogUpdate::Nothing => self.transitions.drain(..0)
        }
    }
}

/*
Api brainstorming:

idealerweise nehmen die logs RevMeta an und ermöglichen drainen, truncaten, etc
aber transition logs geben bei log Methoden auch eine referenz zum log EIntrag zurück
vielleicht builder style?

my_log.pre_update(&meta); // truncate
my_log.pre_update(&meta).drain_past() ... // drain
my_log.pre_update(&meta).drain_future() ... // drain

oder

my_log.pre_update_truncate(&meta);
let drain = my_log.pre_update_drain(&meta);
for _ in drain.past() { // .next()
for _ in drain.future() { // .next_back()

oder

my_log.pre_update_truncate(&meta);
for _ in my_log.pre_update_drain(&meta, LogDrain::Future)
for _ in my_log.pre_update_drain(&meta, LogDrain::Past)

welche typen gibt es?

NOT_LOG:
IN:  push via T, Option<T> oder FnOnce(LogMut)
OUT: drainer via methods, truncate via drop

LOG:
In: -
OUT: state with drain/get?
             | truncate future | clear
forward log  | panic           | panic
backward log | drain + value   | panic

PastLenLog in die logs integrieren?


*/

#[cfg(test)]
mod test {
/* 
    use super::*;

    struct Logs(Vec<[DenseTransitionLog<char>; 2]>);

    impl Logs {
        fn new() -> Self {
            Self(vec![Default::default()])
        }
        fn forward(
            &mut self,
            max_past_len: usize,
            push: char,
            expected_transitions_len: usize,
            expected_pop: Option<char>,
        ) {
            for [log1, log2] in self.0.iter_mut() {
                let before = log1.clone();
                let actual_pop = log1.push_and_pop_past(max_past_len, push);
                assert_eq!(
                    log1.transitions_len(),
                    expected_transitions_len,
                    "\nbefore: {before:#?}\nafter: {log1:#?}"
                );
                assert_eq!(
                    actual_pop, expected_pop,
                    "\nbefore: {before:#?}\nafter: {log1:#?}"
                );

                let before = log2.clone();
                let actual_drain: Vec<_> = log2.push_and_drain_past(max_past_len, push).collect();
                assert_eq!(
                    log2.transitions_len(),
                    expected_transitions_len,
                    "\nbefore: {before:#?}\nafter: {log2:#?}"
                );
                assert_eq!(
                    actual_drain.as_slice(),
                    expected_pop.as_slice(),
                    "\nbefore: {before:#?}\nafter: {log2:#?}"
                );
            }
        }
        fn forward_log(&mut self, expected_transition: Result<char, OutOfLog>) {
            for log in self.0.iter_mut().flatten() {
                let before = log.clone();
                let actual_transition = log.forward_log().cloned();
                assert_eq!(
                    actual_transition, expected_transition,
                    "\nbefore: {before:#?}\nafter: {log:#?}"
                );
            }
        }
        fn backward_log(&mut self, expected_transition: Result<char, OutOfLog>) {
            for log in self.0.iter_mut().flatten() {
                let before = log.clone();
                let actual_transition = log.backward_log().cloned();
                assert_eq!(
                    actual_transition, expected_transition,
                    "\nbefore: {before:#?}\nafter: {log:#?}"
                );
            }
        }
        fn drain_future(&mut self, expected_future: Vec<char>, expected_transitions_len: usize) {
            self.0 = std::mem::take(&mut self.0)
                .into_iter()
                .flatten()
                .map(|mut log| {
                    let before = log.clone();
                    let actual_future: Vec<_> = log.drain_future().collect();
                    assert_eq!(
                        log.transitions_len(),
                        expected_transitions_len,
                        "\nbefore: {before:#?}\nafter: {log:#?}"
                    );
                    assert_eq!(
                        actual_future, expected_future,
                        "\nbefore: {before:#?}\nafter: {log:#?}"
                    );
                    [before, log]
                })
                .collect();
        }
    }

    #[test]
    fn log_traversal_works() {
        let mut logs = Logs::new();
        logs.forward(2, 'a', 1, None);
        logs.forward(2, 'b', 2, None);
        // shortened log
        logs.forward(2, 'c', 2, Some('a'));

        logs.backward_log(Ok('c'));
        logs.backward_log(Ok('b'));
        // out of log, no mutations happend to the logs here
        logs.backward_log(Err(OutOfLog));

        logs.forward_log(Ok('b'));
        logs.forward_log(Ok('c'));
        // nothing ever logged past 'c', no mutations happend to the logs here
        logs.forward_log(Err(OutOfLog));

        logs.backward_log(Ok('c'));
        logs.backward_log(Ok('b'));

        logs.drain_future(vec!['b', 'c'], 0);

        // all entries are truncated as they are in the future
        logs.forward(2, 'd', 1, None);
    }
    */
}
