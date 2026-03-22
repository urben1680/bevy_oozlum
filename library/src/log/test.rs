use super::*;

use core::{num::NonZeroU64, str::Chars};

use crate::meta::{RevMeta, RevQueue};

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
            drained: AC,
            kept: EMPTY,
            buffer: B,
        }
    }
    fn gap_c() -> Self {
        Self {
            drained: AB,
            kept: C,
            buffer: EMPTY,
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
            drained: A,
            kept: BC,
            buffer: EMPTY,
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
        (GapRange::new(0, 0), GapTest::gap_empty(ABC, EMPTY)),
        (GapRange::new(0, 1), GapTest::gap_a()),
        (GapRange::new(0, 2), GapTest::gap_ab()),
        (GapRange::new(0, 3), GapTest::gap_abc()),
        (GapRange::new_clear(0), GapTest::gap_clear()),
        (GapRange::new(1, 1), GapTest::gap_empty(ABC, EMPTY)),
        (GapRange::new(1, 2), GapTest::gap_b()),
        (GapRange::new(1, 3), GapTest::gap_bc()),
        (GapRange::new_clear(1), GapTest::gap_clear()),
        (GapRange::new(2, 2), GapTest::gap_empty(ABC, EMPTY)),
        (GapRange::new(2, 3), GapTest::gap_c()),
        (GapRange::new_clear(2), GapTest::gap_clear()),
        (GapRange::new(3, 3), GapTest::gap_empty(ABC, EMPTY)),
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
        prepend(&mut deque, &mut gap_buffer);
        assert!(deque.iter().is_sorted(), "#{i}");
    }
}

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
    pub(super) fn assert_past(&mut self, expected: &[char]) -> &mut Self {
        let iter = self.past();
        assert_eq!(iter.size_hint().0, expected.len());
        let actual = iter.collect::<Vec<_>>();
        assert_eq!(actual, expected);
        if expected.len() != 0 {
            let iter = self.past();
            assert_eq!(iter.size_hint().0, 0);
            assert_eq!(iter.count(), 0);
        }
        self
    }

    pub(super) fn assert_future(&mut self, expected: &[char]) -> &mut Self {
        let iter = self.future();
        assert_eq!(iter.len(), expected.len());
        let actual = iter.collect::<Vec<_>>();
        assert_eq!(actual, expected);
        if expected.len() != 0 {
            let iter = self.future();
            assert_eq!(iter.len(), 0);
            assert_eq!(iter.count(), 0);
        }
        self
    }

    pub(super) fn assert_all(&mut self, past: &[char], future: &[char]) -> &mut Self {
        let drain_len_sum = past.len() + future.len();
        let iter = self.all();
        assert_eq!(iter.len(), drain_len_sum);
        let actual = iter.collect::<Vec<_>>();
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

        self.past_drain
            .forward_push(meta, max_past_len, push)
            .assert_past(past_drain);

        self.future_drain
            .forward_push(meta, max_past_len, push)
            .assert_future(future_drain);

        self.past_future_drain
            .forward_push(meta, max_past_len, push)
            .assert_past(past_drain)
            .assert_future(future_drain)
            .assert_all(&[], &[]);

        self.future_past_drain
            .forward_push(meta, max_past_len, push)
            .assert_future(future_drain)
            .assert_past(past_drain)
            .assert_all(&[], &[]);

        self.all_drain
            .forward_push(meta, max_past_len, push)
            .assert_all(past_drain, future_drain)
            .assert_past(&[])
            .assert_future(&[]);

        self.past_all_drain
            .forward_push(meta, max_past_len, push)
            .assert_past(past_drain)
            .assert_all(&[], future_drain)
            .assert_future(&[]);

        self.future_all_drain
            .forward_push(meta, max_past_len, push)
            .assert_future(future_drain)
            .assert_all(past_drain, &[])
            .assert_future(&[]);
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

        self.past_drain
            .forward_extend_with(meta, max_past_len, transitions.chars(), update)
            .assert_past(past_drain);

        self.future_drain
            .forward_extend_with(meta, max_past_len, transitions.chars(), update)
            .assert_future(future_drain);

        self.past_future_drain
            .forward_extend_with(meta, max_past_len, transitions.chars(), update)
            .assert_past(past_drain)
            .assert_future(future_drain)
            .assert_all(&[], &[]);

        self.future_past_drain
            .forward_extend_with(meta, max_past_len, transitions.chars(), update)
            .assert_future(future_drain)
            .assert_past(past_drain)
            .assert_all(&[], &[]);

        self.all_drain
            .forward_extend_with(meta, max_past_len, transitions.chars(), update)
            .assert_all(past_drain, future_drain);

        self.past_all_drain
            .forward_extend_with(meta, max_past_len, transitions.chars(), update)
            .assert_past(past_drain)
            .assert_all(&[], future_drain)
            .assert_future(&[]);

        self.future_all_drain
            .forward_extend_with(meta, max_past_len, transitions.chars(), update)
            .assert_future(future_drain)
            .assert_all(past_drain, &[])
            .assert_future(&[]);
    }

    #[track_caller]
    pub(super) fn assert_forward_log_transitions(
        &mut self,
        meta: &RevMeta,
        expected: Result<(&'static str, char), ()>,
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
            Ok((transitions, update)) => {
                for log in logs {
                    let actual = log.forward_log(meta).map(TransitionsLogIterMut::to_tuple);
                    assert_eq!(actual, Ok((transitions.to_string(), update)));
                }
            }
            Err(()) => {
                for log in logs {
                    assert_eq!(
                        log.forward_log(meta).map(TransitionsLogIterMut::to_tuple),
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
            Ok((transitions, update)) => {
                for log in logs {
                    let actual = log.backward_log(meta).map(TransitionsLogIterMut::to_tuple);
                    assert_eq!(actual, Ok((transitions.to_string(), update)));
                }
            }
            Err(()) => {
                for log in logs {
                    assert_eq!(
                        log.backward_log(meta).map(TransitionsLogIterMut::to_tuple),
                        Err(OutOfLog::caller())
                    );
                }
            }
        }
    }
}

impl TransitionsDrain<'_, char, char, Chars<'_>> {
    pub(super) fn assert_past(&mut self, expected: &[(&'static str, char)]) -> &mut Self {
        let iter = self.past();
        let len = expected
            .iter()
            .map(|(s, _)| s.chars().count())
            .sum::<usize>();
        assert_eq!(iter.transitions.size_hint().0, len);
        assert_eq!(iter.updates.size_hint().0, expected.len());
        let actual = iter.to_tuples();
        let expected = expected
            .iter()
            .map(|(transitions, update)| (transitions.to_string(), *update))
            .collect::<Vec<_>>();
        assert_eq!(actual, expected);
        if expected.len() != 0 {
            let iter = self.past();
            assert_eq!(iter.transitions.size_hint().0, 0);
            assert_eq!(iter.transitions.count(), 0);
            assert_eq!(iter.updates.size_hint().0, 0);
            assert_eq!(iter.updates.count(), 0);
        }
        self
    }
    pub(super) fn assert_future(&mut self, expected: &[(&'static str, char)]) -> &mut Self {
        let iter = self.future();
        let len = expected
            .iter()
            .map(|(s, _)| s.chars().count())
            .sum::<usize>();
        assert_eq!(iter.transitions.len(), len);
        assert_eq!(iter.updates.len(), expected.len());
        let actual = iter.to_tuples();
        let expected = expected
            .iter()
            .map(|(transitions, update)| (transitions.to_string(), *update))
            .collect::<Vec<_>>();
        assert_eq!(actual, expected);
        if expected.len() != 0 {
            let iter = self.future();
            assert_eq!(iter.transitions.len(), 0);
            assert_eq!(iter.transitions.count(), 0);
            assert_eq!(iter.updates.len(), 0);
            assert_eq!(iter.updates.count(), 0);
        }
        self
    }
    pub(super) fn assert_all(
        &mut self,
        past: &[(&'static str, char)],
        future: &[(&'static str, char)],
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
        let actual = iter.to_tuples();
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
            meta: RevMeta::new(max_past_len, false),
            updates: UpdateLog::new(),
            transition_logs: Logs::default(),
            transitions_logs: Logs::default(),
        }
    }
    fn forward<const N: usize>(&mut self, entries: [Entries; N], clear: bool) {
        let queue = if clear {
            RevQueue::ClearThenRunForward
        } else {
            RevQueue::RunForward
        };
        self.meta.set_queue(queue);
        self.meta.update_ref(Ok(true), |meta, _| {
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
        self.meta.set_queue(RevQueue::RunForwardLog);
        self.meta.update_ref(Ok(true), |meta, _| {
            let mut entries = entries.into_iter();
            while self.updates.forward_log(meta) {
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
        self.meta.set_queue(RevQueue::RunBackwardLog);
        self.meta.update_ref(Ok(true), |meta, _| {
            let mut entries = entries.into_iter();
            while self.updates.backward_log(meta) {
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
