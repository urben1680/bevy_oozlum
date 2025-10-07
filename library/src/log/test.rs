use super::*;

use core::{num::NonZeroU64, str::Chars};

use crate::meta::{RevDirection, RevMeta, RevQueue};

#[derive(Debug, Default)]
pub(super) struct Logs<T> {
    drop_drain: T,
    past_drain: T,
    future_drain: T,
    past_future_drain: T,
    future_past_drain: T,
    all_drain: T,
    past_all_drain: T,
    future_all_drain: T,
}

impl TransitionDrain<'_, char> {
    pub(super) fn assert_past<const N: usize>(&mut self, expected: [char; N]) -> &mut Self {
        let iter = self.past();
        assert_eq!(iter.len(), N);
        let actual = iter.collect::<Vec<_>>();
        assert_eq!(actual, expected);
        if N != 0 {
            let iter = self.past();
            assert_eq!(iter.len(), 0);
            assert_eq!(iter.count(), 0);
        }
        self
    }

    pub(super) fn assert_future<const N: usize>(&mut self, expected: [char; N]) -> &mut Self {
        let iter = self.future();
        assert_eq!(iter.len(), N);
        let actual = iter.collect::<Vec<_>>();
        assert_eq!(actual, expected);
        if N != 0 {
            let iter = self.future();
            assert_eq!(iter.len(), 0);
            assert_eq!(iter.count(), 0);
        }
        self
    }

    pub(super) fn assert_all<const N: usize, const M: usize>(
        &mut self,
        past: [char; N],
        future: [char; M],
    ) -> &mut Self {
        let iter = self.all();
        assert_eq!(iter.len(), N + M);
        let actual = iter.collect::<Vec<_>>();
        let expected = past.into_iter().chain(future).collect::<Vec<_>>();
        assert_eq!(actual, expected);
        if N + M != 0 {
            let iter = self.all();
            assert_eq!(iter.len(), 0);
            assert_eq!(iter.count(), 0);
        }
        self
    }
}

impl Logs<TransitionLog<char>> {
    pub(super) fn assert_forward_transition<const N: usize, const M: usize>(
        &mut self,
        meta: &RevMeta,
        max_past_len: u64,
        past_drain: [char; N],
        future_drain: [char; M],
        push: char,
    ) {
        self.drop_drain.push(meta, max_past_len, push).unwrap();

        self.past_drain
            .push(meta, max_past_len, push)
            .unwrap()
            .assert_past(past_drain);

        self.future_drain
            .push(meta, max_past_len, push)
            .unwrap()
            .assert_future(future_drain);

        self.past_future_drain
            .push(meta, max_past_len, push)
            .unwrap()
            .assert_past(past_drain)
            .assert_future(future_drain)
            .assert_all([], []);

        self.future_past_drain
            .push(meta, max_past_len, push)
            .unwrap()
            .assert_future(future_drain)
            .assert_past(past_drain)
            .assert_all([], []);

        self.all_drain
            .push(meta, max_past_len, push)
            .unwrap()
            .assert_all(past_drain, future_drain)
            .assert_past([])
            .assert_future([]);

        self.past_all_drain
            .push(meta, max_past_len, push)
            .unwrap()
            .assert_past(past_drain)
            .assert_all([], future_drain)
            .assert_future([]);

        self.future_all_drain
            .push(meta, max_past_len, push)
            .unwrap()
            .assert_future(future_drain)
            .assert_all(past_drain, [])
            .assert_future([]);
    }

    #[track_caller]
    pub(super) fn assert_forward_log_transition(
        &mut self,
        meta: &RevMeta,
        expected: Result<char, ()>,
    ) {
        let logs = [
            &mut self.drop_drain,
            &mut self.past_drain,
            &mut self.future_drain,
            &mut self.past_future_drain,
            &mut self.future_past_drain,
            &mut self.all_drain,
            &mut self.past_all_drain,
            &mut self.future_all_drain,
        ]
        .into_iter()
        .enumerate();

        match expected {
            Ok(expected) => {
                for (i, log) in logs {
                    let actual = log.forward_log(meta).map(|char| *char);
                    assert_eq!(actual, Ok(expected), "{i}");
                }
            }
            Err(()) => {
                for (i, log) in logs {
                    assert_eq!(log.forward_log(meta), Err(OutOfLog::caller()), "{i}");
                    log.clear_poison();
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
        match expected {
            Ok(expected) => {
                for (i, log) in [
                    &mut self.drop_drain,
                    &mut self.past_drain,
                    &mut self.future_drain,
                    &mut self.past_future_drain,
                    &mut self.future_past_drain,
                    &mut self.all_drain,
                    &mut self.past_all_drain,
                    &mut self.future_all_drain,
                ]
                .into_iter()
                .enumerate()
                {
                    let actual = log.backward_log(meta).map(|char| *char);
                    assert_eq!(actual, Ok(expected), "{i}");
                }
            }
            Err(()) => {
                for log in [&mut self.drop_drain, &mut self.future_drain] {
                    assert_eq!(log.backward_log(meta), Err(OutOfLog::caller()));
                    log.clear_poison();
                }

                for (i, log) in [
                    &mut self.past_drain,
                    &mut self.past_future_drain,
                    &mut self.future_past_drain,
                    &mut self.all_drain,
                    &mut self.past_all_drain,
                    &mut self.future_all_drain,
                ]
                .into_iter()
                .enumerate()
                {
                    match log.backward_log(meta) {
                        Ok(expected) => {
                            let expected = *expected;
                            assert_eq!(log.backward_log(meta), Err(OutOfLog::caller()), "{i}");
                            log.clear_poison();

                            // undo Ok
                            let actual = log.forward_log(meta).map(|char| *char);
                            assert_eq!(actual, Ok(expected), "{i}");
                        }
                        Err(out_of_log) => {
                            assert_eq!(out_of_log, OutOfLog::caller(), "{i}");
                            log.clear_poison();
                        }
                    }
                }
            }
        }
    }
}

impl Logs<TransitionsLog<char, char>> {
    pub(super) fn assert_forward_transitions<const N: usize, const M: usize>(
        &mut self,
        meta: &RevMeta,
        max_past_len: u64,
        past_drain: [(String, char); N],
        future_drain: [(String, char); M],
        (transitions, update): (String, char),
    ) {
        self.drop_drain
            .extend_with(meta, max_past_len, transitions.chars(), update)
            .unwrap();

        self.past_drain
            .extend_with(meta, max_past_len, transitions.chars(), update)
            .unwrap()
            .assert_past(past_drain.clone());

        self.future_drain
            .extend_with(meta, max_past_len, transitions.chars(), update)
            .unwrap()
            .assert_future(future_drain.clone());

        self.past_future_drain
            .extend_with(meta, max_past_len, transitions.chars(), update)
            .unwrap()
            .assert_past(past_drain.clone())
            .assert_future(future_drain.clone())
            .assert_all([], []);

        self.future_past_drain
            .extend_with(meta, max_past_len, transitions.chars(), update)
            .unwrap()
            .assert_future(future_drain.clone())
            .assert_past(past_drain.clone())
            .assert_all([], []);

        self.all_drain
            .extend_with(meta, max_past_len, transitions.chars(), update)
            .unwrap()
            .assert_all(past_drain.clone(), future_drain.clone());

        self.past_all_drain
            .extend_with(meta, max_past_len, transitions.chars(), update)
            .unwrap()
            .assert_past(past_drain.clone())
            .assert_all([], future_drain.clone())
            .assert_future([]);

        self.future_all_drain
            .extend_with(meta, max_past_len, transitions.chars(), update)
            .unwrap()
            .assert_future(future_drain)
            .assert_all(past_drain, [])
            .assert_future([]);
    }

    #[track_caller]
    pub(super) fn assert_forward_log_transitions(
        &mut self,
        meta: &RevMeta,
        expected: Result<(String, char), ()>,
    ) {
        let logs = [
            &mut self.drop_drain,
            &mut self.past_drain,
            &mut self.future_drain,
            &mut self.past_future_drain,
            &mut self.future_past_drain,
            &mut self.all_drain,
            &mut self.past_all_drain,
            &mut self.future_all_drain,
        ];
        match expected {
            Ok(expected) => {
                for log in logs {
                    let actual = log.forward_log(meta).map(TransitionsLogIterMut::to_tuple);
                    assert_eq!(actual, Ok(expected.clone()));
                }
            }
            Err(()) => {
                for log in logs {
                    assert_eq!(
                        log.forward_log(meta).map(TransitionsLogIterMut::to_tuple),
                        Err(OutOfLog::caller())
                    );
                    log.clear_poison();
                }
            }
        }
    }

    #[track_caller]
    pub(super) fn assert_backward_log_transitions(
        &mut self,
        meta: &RevMeta,
        expected: Result<(String, char), ()>,
    ) {
        match expected {
            Ok(expected) => {
                for log in [
                    &mut self.drop_drain,
                    &mut self.past_drain,
                    &mut self.future_drain,
                    &mut self.past_future_drain,
                    &mut self.future_past_drain,
                    &mut self.all_drain,
                    &mut self.past_all_drain,
                    &mut self.future_all_drain,
                ] {
                    let actual = log.backward_log(meta).map(TransitionsLogIterMut::to_tuple);
                    assert_eq!(actual, Ok(expected.clone()));
                }
            }
            Err(()) => {
                for log in [&mut self.drop_drain, &mut self.future_drain] {
                    assert_eq!(
                        log.backward_log(meta).map(TransitionsLogIterMut::to_tuple),
                        Err(OutOfLog::caller())
                    );
                    log.clear_poison();
                }

                for (i, log) in [
                    &mut self.past_drain,
                    &mut self.past_future_drain,
                    &mut self.future_past_drain,
                    &mut self.all_drain,
                    &mut self.past_all_drain,
                    &mut self.future_all_drain,
                ]
                .into_iter()
                .enumerate()
                {
                    match log.backward_log(meta) {
                        Ok(expected) => {
                            let expected = expected.to_tuple();
                            assert_eq!(
                                log.backward_log(meta).map(TransitionsLogIterMut::to_tuple),
                                Err(OutOfLog::caller()),
                                "{i}"
                            );
                            log.clear_poison();

                            // undo Ok
                            let actual = log.forward_log(meta).map(TransitionsLogIterMut::to_tuple);
                            assert_eq!(actual, Ok(expected), "{i}");
                        }
                        Err(out_of_log) => {
                            assert_eq!(out_of_log, OutOfLog::caller(), "{i}");
                            log.clear_poison();
                        }
                    }
                }
            }
        }
    }
}

impl TransitionsDrain<'_, char, char, Chars<'_>> {
    pub(super) fn assert_past<const N: usize>(
        &mut self,
        expected: [(String, char); N],
    ) -> &mut Self {
        let iter = self.past();
        let len = expected
            .iter()
            .map(|(s, _)| s.chars().count())
            .sum::<usize>();
        assert_eq!(iter.transitions.len(), len);
        assert_eq!(iter.updates.len(), N);
        let actual = iter.to_tuples();
        assert_eq!(actual, expected);
        if N != 0 {
            let iter = self.past();
            assert_eq!(iter.transitions.len(), 0);
            assert_eq!(iter.transitions.count(), 0);
            assert_eq!(iter.updates.len(), 0);
            assert_eq!(iter.updates.count(), 0);
        }
        self
    }
    pub(super) fn assert_future<const N: usize>(
        &mut self,
        expected: [(String, char); N],
    ) -> &mut Self {
        let iter = self.future();
        let len = expected
            .iter()
            .map(|(s, _)| s.chars().count())
            .sum::<usize>();
        assert_eq!(iter.transitions.len(), len);
        assert_eq!(iter.updates.len(), N);
        let actual = iter.to_tuples();
        assert_eq!(actual, expected);
        if N != 0 {
            let iter = self.future();
            assert_eq!(iter.transitions.len(), 0);
            assert_eq!(iter.transitions.count(), 0);
            assert_eq!(iter.updates.len(), 0);
            assert_eq!(iter.updates.count(), 0);
        }
        self
    }
    pub(super) fn assert_all<const N: usize, const M: usize>(
        &mut self,
        past: [(String, char); N],
        future: [(String, char); M],
    ) -> &mut Self {
        let iter = self.all();
        let len = past
            .iter()
            .chain(future.iter())
            .map(|(s, _)| s.chars().count())
            .sum::<usize>();
        assert_eq!(iter.transitions.len(), len);
        assert_eq!(iter.updates.len(), N + M);
        let actual = iter.to_tuples();
        let expected = past.into_iter().chain(future).collect::<Vec<_>>();
        assert_eq!(actual, expected);
        if N + M != 0 {
            let iter = self.all();
            assert_eq!(iter.transitions.len(), 0);
            assert_eq!(iter.transitions.count(), 0);
            assert_eq!(iter.updates.len(), 0);
            assert_eq!(iter.updates.count(), 0);
        }
        self
    }
}

impl<TI, UI> TransitionsDrainIters<TI, UI, char>
where
    TI: ExactSizeIterator<Item = char>,
    UI: ExactSizeIterator<Item = TransitionsLogUpdate<char>>,
{
    fn to_tuples(mut self) -> Vec<(String, char)> {
        let mut v = Vec::new();
        while let Some((transitions, update)) = self.next_log_entry() {
            v.push((transitions.collect(), update))
        }
        v
    }
}

impl<'a> TransitionsLogIterMut<'a, char, char> {
    fn to_tuple(self) -> (String, char) {
        let update = *self.update;
        (self.map(|char| *char).collect(), update)
    }
}

static EMPTY: &[char] = &[];
static A: &[char] = &['a'];
static AB: &[char] = &['a', 'b'];
static AC: &[char] = &['a', 'c'];
static ABC: &[char] = &['a', 'b', 'c'];
static B: &[char] = &['b'];
static BC: &[char] = &['b', 'c'];
static C: &[char] = &['c'];

#[derive(Debug, Clone)]
struct GapTest {
    drained: &'static [char],
    kept: &'static [char],
    buffer: &'static [char],
}

impl GapTest {
    fn gap_clear() -> Self {
        Self {
            drained: ABC,
            kept: EMPTY,
            buffer: EMPTY,
        }
    }
    fn gap_empty(drained: &'static [char], buffer: &'static [char]) -> Self {
        Self {
            drained,
            kept: EMPTY,
            buffer,
        }
    }
    fn gap_a() -> Self {
        Self {
            drained: BC,
            kept: A,
            buffer: EMPTY,
        }
    }
    fn gap_b() -> Self {
        Self {
            drained: C,
            kept: EMPTY,
            buffer: AB,
        }
    }
    fn gap_c() -> Self {
        Self {
            drained: A,
            kept: C,
            buffer: B,
        }
    }
    fn gap_ab() -> Self {
        Self {
            drained: C,
            kept: AB,
            buffer: EMPTY,
        }
    }
    fn gap_bc() -> Self {
        Self {
            drained: EMPTY,
            kept: BC,
            buffer: A,
        }
    }
    fn gap_abc() -> Self {
        Self {
            drained: EMPTY,
            kept: ABC,
            buffer: EMPTY,
        }
    }
}

#[test]
fn drain_all_iterator_works() {
    let tests = [
        (
            GapRange::new_offset_one(0, 0),
            GapTest::gap_empty(ABC, EMPTY),
        ),
        (GapRange::new_offset_one(0, 1), GapTest::gap_a()),
        (GapRange::new_offset_one(0, 2), GapTest::gap_ab()),
        (GapRange::new_offset_one(0, 3), GapTest::gap_abc()),
        (GapRange::new_clear(0), GapTest::gap_clear()),
        (GapRange::new_offset_one(1, 1), GapTest::gap_empty(BC, A)),
        (GapRange::new_offset_one(1, 2), GapTest::gap_b()),
        (GapRange::new_offset_one(1, 3), GapTest::gap_bc()),
        (GapRange::new_clear(1), GapTest::gap_clear()),
        (GapRange::new_offset_one(2, 2), GapTest::gap_empty(AC, B)),
        (GapRange::new_offset_one(2, 3), GapTest::gap_c()),
        (GapRange::new_clear(2), GapTest::gap_clear()),
        (GapRange::new_offset_one(3, 3), GapTest::gap_empty(AB, C)),
        (GapRange::new_clear(3), GapTest::gap_clear()),
    ];

    for (i, (mut gap_range, test)) in tests.into_iter().enumerate() {
        let mut deque = ABC.iter().cloned().collect::<VecDeque<_>>();
        let mut gap_buffer = Default::default();
        let drain_all = DrainAll::new(&mut deque, &mut gap_range, &mut gap_buffer);
        let updated_gap = gap_range.clone();

        let drained = drain_all.collect::<Vec<_>>();

        assert_eq!(deque, test.kept, "#{i}");
        assert_eq!(drained, test.drained, "#{i}");
        assert_eq!(&*gap_buffer, test.buffer, "#{i}");

        let drain_all = DrainAll::new(&mut deque, &mut gap_range, &mut gap_buffer);

        let drained = drain_all.collect::<Vec<_>>();
        assert_eq!(deque, test.kept, "#{i}");
        assert_eq!(drained, [], "#{i}");
        assert_eq!(&*gap_buffer, test.buffer, "#{i}");

        assert_eq!(gap_range.start, updated_gap.start, "#{i}");
        assert_eq!(gap_range.end, updated_gap.end, "#{i}");
        prepend(&mut gap_buffer, &mut deque);
        assert!(deque.iter().is_sorted(), "#{i}");
    }
}

struct MetaAndLogs {
    meta: RevMeta,
    updates: UpdateLog,
    transition_logs: Logs<TransitionLog<char>>,
    transitions_logs: Logs<TransitionsLog<char, char>>,
}

// todo
