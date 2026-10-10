use super::*;

use alloc::{
    collections::vec_deque::Drain,
    string::{String, ToString},
    vec::Vec,
};
use core::{num::NonZeroU64, str::Chars};

use crate::meta::{RevMeta, RevQueue};

#[derive(Debug, Default)]
pub(super) struct Logs<T> {
    drop_drain: T,
    past_drain_iter: T,
    future_drain_iter: T,
    past_future_drain_iter: T,
    future_past_drain_iter: T,
    all_drain_iter: T,
    past_all_drain_iter: T,
    future_all_drain_iter: T,
    past_drain_for_each: T,
    future_drain_for_each: T,
    past_future_drain_for_each: T,
    future_past_drain_for_each: T,
    all_drain_for_each: T,
    past_all_drain_for_each: T,
    future_all_drain_for_each: T,
}

impl<T> Logs<T> {
    fn iter_mut(&mut self) -> impl Iterator<Item = &mut T> {
        [
            &mut self.drop_drain,
            &mut self.past_drain_iter,
            &mut self.future_drain_iter,
            &mut self.past_future_drain_iter,
            &mut self.future_past_drain_iter,
            &mut self.all_drain_iter,
            &mut self.past_all_drain_iter,
            &mut self.future_all_drain_iter,
            &mut self.past_drain_for_each,
            &mut self.future_drain_for_each,
            &mut self.past_future_drain_for_each,
            &mut self.future_past_drain_for_each,
            &mut self.all_drain_for_each,
            &mut self.past_all_drain_for_each,
            &mut self.future_all_drain_for_each,
        ]
        .into_iter()
    }
}

impl TransitionDrain<'_, char> {
    pub(super) fn assert_past_iter(&mut self, expected: &[char]) -> &mut Self {
        self.assert_past_inner(expected, |iter| iter.collect::<Vec<_>>())
    }

    pub(super) fn assert_past_for_each(&mut self, expected: &[char]) -> &mut Self {
        self.assert_past_inner(expected, |iter| {
            let mut actual = Vec::new();
            iter.for_each(|item| actual.push(item));
            actual
        })
    }

    pub(super) fn assert_past_inner(
        &mut self,
        expected: &[char],
        collect: impl FnOnce(Drain<char>) -> Vec<char>,
    ) -> &mut Self {
        let iter = self.past();
        assert_eq!(iter.size_hint().0, expected.len());
        let actual = collect(iter);
        assert_eq!(actual, expected);
        if !expected.is_empty() {
            let iter = self.past();
            assert_eq!(iter.size_hint().0, 0);
            assert_eq!(iter.count(), 0);
        }
        self
    }

    pub(super) fn assert_future_iter(&mut self, expected: &[char]) -> &mut Self {
        self.assert_future_inner(expected, |iter| iter.collect::<Vec<_>>())
    }

    pub(super) fn assert_future_for_each(&mut self, expected: &[char]) -> &mut Self {
        self.assert_future_inner(expected, |iter| {
            let mut actual = Vec::new();
            iter.for_each(|item| actual.push(item));
            actual
        })
    }

    pub(super) fn assert_future_inner(
        &mut self,
        expected: &[char],
        collect: impl FnOnce(Drain<char>) -> Vec<char>,
    ) -> &mut Self {
        let iter = self.future();
        assert_eq!(iter.len(), expected.len());
        let actual = collect(iter);
        assert_eq!(actual, expected);
        if !expected.is_empty() {
            let iter = self.future();
            assert_eq!(iter.len(), 0);
            assert_eq!(iter.count(), 0);
        }
        self
    }

    pub(super) fn assert_all_iter(&mut self, past: &[char], future: &[char]) -> &mut Self {
        self.assert_all_inner(past, future, |iter| iter.collect::<Vec<_>>())
    }

    pub(super) fn assert_all_for_each(&mut self, past: &[char], future: &[char]) -> &mut Self {
        self.assert_all_inner(past, future, |iter| {
            let mut actual = Vec::new();
            iter.for_each(|item| actual.push(item));
            actual
        })
    }

    pub(super) fn assert_all_inner(
        &mut self,
        past: &[char],
        future: &[char],
        collect: impl FnOnce(DrainAll<char>) -> Vec<char>,
    ) -> &mut Self {
        let drain_len_sum = past.len() + future.len();
        let iter = self.all();
        assert_eq!(iter.len(), drain_len_sum);
        let actual = collect(iter);
        let expected = past
            .iter()
            .cloned()
            .chain(future.iter().cloned())
            .collect::<Vec<_>>();
        assert_eq!(actual, expected);
        if drain_len_sum != 0 {
            let iter = self.all();
            assert_eq!(iter.len(), 0);
            assert_eq!(iter.count(), 0);
        }
        self
    }
}

impl Logs<TransitionLog<char>> {
    pub(super) fn assert_forward_transition(
        &mut self,
        meta: &RevMeta,
        max_past_len: impl Into<NonZeroU64> + Copy,
        past_drain: &[char],
        future_drain: &[char],
        push: char,
    ) {
        self.drop_drain.forward_push(meta, max_past_len, push);

        self.past_drain_iter
            .forward_push(meta, max_past_len, push)
            .assert_past_iter(past_drain);

        self.future_drain_iter
            .forward_push(meta, max_past_len, push)
            .assert_future_iter(future_drain);

        self.past_future_drain_iter
            .forward_push(meta, max_past_len, push)
            .assert_past_iter(past_drain)
            .assert_future_iter(future_drain)
            .assert_all_iter(&[], &[]);

        self.future_past_drain_iter
            .forward_push(meta, max_past_len, push)
            .assert_future_iter(future_drain)
            .assert_past_iter(past_drain)
            .assert_all_iter(&[], &[]);

        self.all_drain_iter
            .forward_push(meta, max_past_len, push)
            .assert_all_iter(past_drain, future_drain)
            .assert_past_iter(&[])
            .assert_future_iter(&[]);

        self.past_all_drain_iter
            .forward_push(meta, max_past_len, push)
            .assert_past_iter(past_drain)
            .assert_all_iter(&[], future_drain)
            .assert_future_iter(&[]);

        self.future_all_drain_iter
            .forward_push(meta, max_past_len, push)
            .assert_future_iter(future_drain)
            .assert_all_iter(past_drain, &[])
            .assert_future_iter(&[]);

        self.past_drain_for_each
            .forward_push(meta, max_past_len, push)
            .assert_past_for_each(past_drain);

        self.future_drain_for_each
            .forward_push(meta, max_past_len, push)
            .assert_future_for_each(future_drain);

        self.past_future_drain_for_each
            .forward_push(meta, max_past_len, push)
            .assert_past_for_each(past_drain)
            .assert_future_for_each(future_drain)
            .assert_all_for_each(&[], &[]);

        self.future_past_drain_for_each
            .forward_push(meta, max_past_len, push)
            .assert_future_for_each(future_drain)
            .assert_past_for_each(past_drain)
            .assert_all_for_each(&[], &[]);

        self.all_drain_for_each
            .forward_push(meta, max_past_len, push)
            .assert_all_for_each(past_drain, future_drain)
            .assert_past_for_each(&[])
            .assert_future_for_each(&[]);

        self.past_all_drain_for_each
            .forward_push(meta, max_past_len, push)
            .assert_past_for_each(past_drain)
            .assert_all_for_each(&[], future_drain)
            .assert_future_for_each(&[]);

        self.future_all_drain_for_each
            .forward_push(meta, max_past_len, push)
            .assert_future_for_each(future_drain)
            .assert_all_for_each(past_drain, &[])
            .assert_future_for_each(&[]);
    }

    #[track_caller]
    pub(super) fn assert_forward_log_transition(
        &mut self,
        meta: &RevMeta,
        expected: Result<char, ()>,
    ) {
        let logs = self.iter_mut();
        match expected {
            Ok(expected) => {
                for log in logs {
                    let actual = log.forward_log(meta).map(|char| *char);
                    assert_eq!(actual, Ok(expected));
                }
            }
            Err(()) => {
                for log in logs {
                    assert_eq!(log.forward_log(meta), Err(OutOfLog::caller()));
                }
            }
        }
    }

    #[track_caller]
    pub(super) fn assert_backward_log_transition(
        &mut self,
        meta: &RevMeta,
        expected: Result<char, ()>,
    ) {
        let logs = self.iter_mut();
        match expected {
            Ok(expected) => {
                for log in logs {
                    let actual = log.backward_log(meta).map(|char| *char);
                    assert_eq!(actual, Ok(expected));
                }
            }
            Err(()) => {
                for log in logs {
                    assert_eq!(log.backward_log(meta), Err(OutOfLog::caller()));
                }
            }
        }
    }
}

impl Logs<TransitionsLog<char, char>> {
    pub(super) fn assert_forward_transitions(
        &mut self,
        meta: &RevMeta,
        max_past_len: impl Into<NonZeroU64> + Copy,
        past_drain: &[(&'static str, char)],
        future_drain: &[(&'static str, char)],
        (transitions, update): (&'static str, char),
    ) {
        self.drop_drain
            .forward_extend_with(meta, max_past_len, transitions.chars(), update);

        self.past_drain_iter
            .forward_extend_with(meta, max_past_len, transitions.chars(), update)
            .assert_past_iter(past_drain);

        self.future_drain_iter
            .forward_extend_with(meta, max_past_len, transitions.chars(), update)
            .assert_future_iter(future_drain);

        self.past_future_drain_iter
            .forward_extend_with(meta, max_past_len, transitions.chars(), update)
            .assert_past_iter(past_drain)
            .assert_future_iter(future_drain)
            .assert_all_iter(&[], &[]);

        self.future_past_drain_iter
            .forward_extend_with(meta, max_past_len, transitions.chars(), update)
            .assert_future_iter(future_drain)
            .assert_past_iter(past_drain)
            .assert_all_iter(&[], &[]);

        self.all_drain_iter
            .forward_extend_with(meta, max_past_len, transitions.chars(), update)
            .assert_all_iter(past_drain, future_drain);

        self.past_all_drain_iter
            .forward_extend_with(meta, max_past_len, transitions.chars(), update)
            .assert_past_iter(past_drain)
            .assert_all_iter(&[], future_drain)
            .assert_future_iter(&[]);

        self.future_all_drain_iter
            .forward_extend_with(meta, max_past_len, transitions.chars(), update)
            .assert_future_iter(future_drain)
            .assert_all_iter(past_drain, &[])
            .assert_future_iter(&[]);

        self.past_drain_for_each
            .forward_extend_with(meta, max_past_len, transitions.chars(), update)
            .assert_past_for_each(past_drain);

        self.future_drain_for_each
            .forward_extend_with(meta, max_past_len, transitions.chars(), update)
            .assert_future_for_each(future_drain);

        self.past_future_drain_for_each
            .forward_extend_with(meta, max_past_len, transitions.chars(), update)
            .assert_past_for_each(past_drain)
            .assert_future_for_each(future_drain)
            .assert_all_for_each(&[], &[]);

        self.future_past_drain_for_each
            .forward_extend_with(meta, max_past_len, transitions.chars(), update)
            .assert_future_for_each(future_drain)
            .assert_past_for_each(past_drain)
            .assert_all_for_each(&[], &[]);

        self.all_drain_for_each
            .forward_extend_with(meta, max_past_len, transitions.chars(), update)
            .assert_all_for_each(past_drain, future_drain);

        self.past_all_drain_for_each
            .forward_extend_with(meta, max_past_len, transitions.chars(), update)
            .assert_past_for_each(past_drain)
            .assert_all_for_each(&[], future_drain)
            .assert_future_for_each(&[]);

        self.future_all_drain_for_each
            .forward_extend_with(meta, max_past_len, transitions.chars(), update)
            .assert_future_for_each(future_drain)
            .assert_all_for_each(past_drain, &[])
            .assert_future_for_each(&[]);
    }

    #[track_caller]
    pub(super) fn assert_forward_log_transitions(
        &mut self,
        meta: &RevMeta,
        expected: Result<(&'static str, char), ()>,
    ) {
        let logs = self.iter_mut();
        match expected {
            Ok((transitions, update)) => {
                for log in logs {
                    let actual = log.forward_log(meta).map(TransitionsLogIterMut::into_tuple);
                    assert_eq!(actual, Ok((transitions.to_string(), update)));
                }
            }
            Err(()) => {
                for log in logs {
                    assert_eq!(
                        log.forward_log(meta).map(TransitionsLogIterMut::into_tuple),
                        Err(OutOfLog::caller())
                    );
                }
            }
        }
    }

    #[track_caller]
    pub(super) fn assert_backward_log_transitions(
        &mut self,
        meta: &RevMeta,
        expected: Result<(&'static str, char), ()>,
    ) {
        let logs = self.iter_mut();
        match expected {
            Ok((transitions, update)) => {
                for log in logs {
                    let actual = log
                        .backward_log(meta)
                        .map(TransitionsLogIterMut::into_tuple);
                    assert_eq!(actual, Ok((transitions.to_string(), update)));
                }
            }
            Err(()) => {
                for log in logs {
                    assert_eq!(
                        log.backward_log(meta)
                            .map(TransitionsLogIterMut::into_tuple),
                        Err(OutOfLog::caller())
                    );
                }
            }
        }
    }
}

impl TransitionsDrain<'_, char, char, Chars<'_>> {
    pub(super) fn assert_past_iter(&mut self, expected: &[(&'static str, char)]) -> &mut Self {
        self.assert_past_inner(expected, |iter| iter.iter_into_tuples())
    }

    pub(super) fn assert_past_for_each(&mut self, expected: &[(&'static str, char)]) -> &mut Self {
        self.assert_past_inner(expected, |iter| iter.for_each_into_tuples())
    }

    pub(super) fn assert_past_inner(
        &mut self,
        expected: &[(&'static str, char)],
        collect: impl FnOnce(
            TransitionsDrainIters<Drain<char>, Drain<TransitionsLogUpdate<char>>, char>,
        ) -> Vec<(String, char)>,
    ) -> &mut Self {
        let iter = self.past();
        let len = expected
            .iter()
            .map(|(s, _)| s.chars().count())
            .sum::<usize>();
        assert_eq!(iter.transitions.size_hint().0, len);
        assert_eq!(iter.updates.size_hint().0, expected.len());
        let actual = collect(iter);
        let expected = expected
            .iter()
            .map(|(transitions, update)| (transitions.to_string(), *update))
            .collect::<Vec<_>>();
        assert_eq!(actual, expected);
        if !expected.is_empty() {
            let iter = self.past();
            assert_eq!(iter.transitions.size_hint().0, 0);
            assert_eq!(iter.transitions.count(), 0);
            assert_eq!(iter.updates.size_hint().0, 0);
            assert_eq!(iter.updates.count(), 0);
        }
        self
    }

    pub(super) fn assert_future_iter(&mut self, expected: &[(&'static str, char)]) -> &mut Self {
        self.assert_future_inner(expected, |iter| iter.iter_into_tuples())
    }

    pub(super) fn assert_future_for_each(
        &mut self,
        expected: &[(&'static str, char)],
    ) -> &mut Self {
        self.assert_future_inner(expected, |iter| iter.for_each_into_tuples())
    }

    pub(super) fn assert_future_inner(
        &mut self,
        expected: &[(&'static str, char)],
        collect: impl FnOnce(
            TransitionsDrainIters<Drain<char>, Drain<TransitionsLogUpdate<char>>, char>,
        ) -> Vec<(String, char)>,
    ) -> &mut Self {
        let iter = self.future();
        let len = expected
            .iter()
            .map(|(s, _)| s.chars().count())
            .sum::<usize>();
        assert_eq!(iter.transitions.len(), len);
        assert_eq!(iter.updates.len(), expected.len());
        let actual = collect(iter);
        let expected = expected
            .iter()
            .map(|(transitions, update)| (transitions.to_string(), *update))
            .collect::<Vec<_>>();
        assert_eq!(actual, expected);
        if !expected.is_empty() {
            let iter = self.future();
            assert_eq!(iter.transitions.len(), 0);
            assert_eq!(iter.transitions.count(), 0);
            assert_eq!(iter.updates.len(), 0);
            assert_eq!(iter.updates.count(), 0);
        }
        self
    }

    pub(super) fn assert_all_iter(
        &mut self,
        past: &[(&'static str, char)],
        future: &[(&'static str, char)],
    ) -> &mut Self {
        self.assert_all_inner(past, future, |iter| iter.iter_into_tuples())
    }

    pub(super) fn assert_all_for_each(
        &mut self,
        past: &[(&'static str, char)],
        future: &[(&'static str, char)],
    ) -> &mut Self {
        self.assert_all_inner(past, future, |iter| iter.for_each_into_tuples())
    }

    pub(super) fn assert_all_inner(
        &mut self,
        past: &[(&'static str, char)],
        future: &[(&'static str, char)],
        collect: impl FnOnce(
            TransitionsDrainIters<DrainAll<char>, DrainAll<TransitionsLogUpdate<char>>, char>,
        ) -> Vec<(String, char)>,
    ) -> &mut Self {
        let drain_sum_len = past.len() + future.len();
        let iter = self.all();
        let past_transitions_len = past.iter().map(|(s, _)| s.chars().count()).sum::<usize>();
        let future_transitions_len = future.iter().map(|(s, _)| s.chars().count()).sum::<usize>();
        assert_eq!(
            iter.transitions.len(),
            past_transitions_len + future_transitions_len
        );
        assert_eq!(iter.updates.len(), drain_sum_len);
        assert_eq!(iter.past_transitions_len(), past_transitions_len);
        assert_eq!(iter.past_updates_len(), past.len());
        let actual = collect(iter);
        let expected = past
            .iter()
            .cloned()
            .chain(future.iter().cloned())
            .map(|(transitions, update)| (transitions.to_string(), update))
            .collect::<Vec<_>>();
        assert_eq!(actual, expected);
        if drain_sum_len != 0 {
            let iter = self.all();
            assert_eq!(iter.transitions.len(), 0);
            assert_eq!(iter.updates.len(), 0);
            assert_eq!(iter.past_transitions_len(), 0);
            assert_eq!(iter.past_updates_len(), 0);
            assert_eq!(iter.transitions.count(), 0);
            assert_eq!(iter.updates.count(), 0);
        }
        self
    }
}

impl<TI, UI> TransitionsDrainIters<TI, UI, char>
where
    TI: Iterator<Item = char>,
    UI: Iterator<Item = TransitionsLogUpdate<char>>,
{
    fn iter_into_tuples(mut self) -> Vec<(String, char)> {
        let mut v = Vec::new();
        while let Some((transitions, update)) = self.next_log_entry() {
            v.push((transitions.collect(), update))
        }
        v
    }
    fn for_each_into_tuples(mut self) -> Vec<(String, char)> {
        let mut v = Vec::new();
        self.for_each_log_entry(|transitions, update| v.push((transitions.collect(), update)));
        v
    }
}

impl<'a> TransitionsLogIterMut<'a, char, char> {
    fn into_tuple(self) -> (String, char) {
        let update = *self.update;
        (self.map(|char| *char).collect(), update)
    }
}

struct MetaAndLogs {
    meta: RevMeta,
    updates: UpdateLog,
    transition_logs: Logs<TransitionLog<char>>,
    transitions_logs: Logs<TransitionsLog<char, char>>,
}

struct Entries {
    past_drain: Vec<(&'static str, char)>,
    future_drain: Vec<(&'static str, char)>,
    push: (&'static str, char),
}

fn entries<const N: usize, const M: usize>(
    past_drain: [(&'static str, char); N],
    future_drain: [(&'static str, char); M],
    push: (&'static str, char),
) -> Entries {
    Entries {
        past_drain: past_drain.into(),
        future_drain: future_drain.into(),
        push,
    }
}

impl MetaAndLogs {
    fn new(max_past_len: u64) -> Self {
        Self {
            meta: RevMeta::new(max_past_len),
            updates: UpdateLog::new(),
            transition_logs: Logs::default(),
            transitions_logs: Logs::default(),
        }
    }
    fn forward<const N: usize>(&mut self, entries: [Entries; N], clear: bool) {
        let queue = if clear {
            Some(RevQueue::ClearThenRunNotLog)
        } else {
            Some(RevQueue::RunNotLog)
        };
        self.meta.update_ref(queue, true, |meta, _| {
            for Entries {
                past_drain,
                future_drain,
                push,
            } in entries
            {
                let past_len = self.updates.forward_past_len(meta);
                let past_transition = past_drain
                    .iter()
                    .map(|(_, update)| *update)
                    .collect::<Vec<_>>();
                let future_transition = future_drain
                    .iter()
                    .map(|(_, update)| *update)
                    .collect::<Vec<_>>();
                self.transition_logs.assert_forward_transition(
                    meta,
                    past_len,
                    &past_transition,
                    &future_transition,
                    push.1,
                );
                self.transitions_logs.assert_forward_transitions(
                    meta,
                    past_len,
                    &past_drain,
                    &future_drain,
                    push,
                );
            }
        });
    }
    fn forward_log<const N: usize>(&mut self, entries: [(&'static str, char); N]) {
        self.meta
            .update_ref(Some(RevQueue::RunForwardLog), true, |meta, _| {
                let mut entries = entries.into_iter();
                while self.updates.forward_log(meta).unwrap() {
                    let entry = entries.by_ref().next().unwrap();
                    self.transition_logs
                        .assert_forward_log_transition(meta, Ok(entry.1));
                    self.transitions_logs
                        .assert_forward_log_transitions(meta, Ok(entry));
                }
                assert_eq!(entries.len(), 0);
            });
    }
    fn backward_log<const N: usize>(&mut self, entries: [(&'static str, char); N]) {
        self.meta
            .update_ref(Some(RevQueue::RunBackwardLog), true, |meta, _| {
                let mut entries = entries.into_iter();
                while self.updates.backward_log(meta).unwrap() {
                    let entry = entries.by_ref().next().unwrap();
                    self.transition_logs
                        .assert_backward_log_transition(meta, Ok(entry.1));
                    self.transitions_logs
                        .assert_backward_log_transitions(meta, Ok(entry));
                }
                assert_eq!(entries.len(), 0);
            });
    }
}

pub(super) mod transitions_presets {
    pub const A: (&str, char) = ("a", 'A');
    pub const B: (&str, char) = ("bb", 'B');
    pub const C: (&str, char) = ("ccc", 'C');
    pub const D: (&str, char) = ("dddd", 'D');
    pub const E: (&str, char) = ("eeeee", 'E');
    pub const F: (&str, char) = ("ffffff", 'F');
    pub const G: (&str, char) = ("ggggggg", 'G');
    pub const H: (&str, char) = ("hhhhhhhh", 'H');
    pub const I: (&str, char) = ("iiiiiiiii", 'I');
    pub const J: (&str, char) = ("jjjjjjjjjj", 'J');
    pub const K: (&str, char) = ("kkkkkkkkkkk", 'K');
    pub const L: (&str, char) = ("llllllllllll", 'L');
    pub const M: (&str, char) = ("mmmmmmmmmmmmm", 'M');
    pub const N: (&str, char) = ("nnnnnnnnnnnnnn", 'N');
    pub const O: (&str, char) = ("ooooooooooooooo", 'O');
    pub const P: (&str, char) = ("pppppppppppppppp", 'P');
    pub const Q: (&str, char) = ("qqqqqqqqqqqqqqqqq", 'Q');
    pub const R: (&str, char) = ("rrrrrrrrrrrrrrrrrr", 'R');
}

#[test]
fn traverses_logs() {
    use transitions_presets::*;

    let mut meta_and_logs = MetaAndLogs::new(4);

    meta_and_logs.forward([entries([], [], A)], false);
    meta_and_logs.forward([entries([], [], B), entries([], [], C)], false);
    meta_and_logs.forward(
        [entries([], [], D), entries([], [], E), entries([], [], F)],
        false,
    );
    meta_and_logs.forward(
        [
            entries([], [], G),
            entries([], [], H),
            entries([], [], I),
            entries([], [], J),
        ],
        false,
    );
    meta_and_logs.forward(
        [
            entries([A], [], K),
            entries([], [], L),
            entries([], [], M),
            entries([], [], N),
            entries([], [], O),
        ],
        false,
    );
    meta_and_logs.forward([entries([B, C], [], P)], false);

    meta_and_logs.backward_log([P]);
    meta_and_logs.backward_log([O, N, M, L, K]);
    meta_and_logs.backward_log([J, I, H, G]);
    meta_and_logs.backward_log([F, E, D]);

    meta_and_logs.forward_log([D, E, F]);
    meta_and_logs.forward_log([G, H, I, J]);
    meta_and_logs.forward_log([K, L, M, N, O]);
    meta_and_logs.forward_log([P]);

    meta_and_logs.backward_log([P]);
    meta_and_logs.backward_log([O, N, M, L, K]);

    meta_and_logs.forward([entries([], [K, L, M, N, O, P], Q)], false);
    meta_and_logs.forward([entries([D, E, F, G, H, I, J, Q], [], R)], true);
}

#[test]
fn behaves_like_meta_minus_gaps() {
    use transitions_presets::*;

    let mut meta_and_logs = MetaAndLogs::new(3);

    meta_and_logs.forward([entries([], [], A)], false);
    meta_and_logs.forward([], false);
    meta_and_logs.forward([entries([], [], B)], false);
    // from here on RevMeta::past_len is not growing anymore, it stays at 3 and instead RevMeta::past_end grows
    meta_and_logs.forward([entries([A], [], C)], false);
    meta_and_logs.forward([entries([], [], D)], false);
    meta_and_logs.forward([entries([B], [], E)], false);
    meta_and_logs.forward([], false);
    meta_and_logs.forward([], false);
    meta_and_logs.forward([entries([C, D, E], [], F)], false);
    meta_and_logs.forward([entries([], [], G)], false);
    meta_and_logs.forward([entries([], [], H)], false);
    meta_and_logs.forward([entries([F], [], I)], false);
    meta_and_logs.forward([entries([G], [], J)], false);
}
