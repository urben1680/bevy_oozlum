use std::{collections::VecDeque, fmt::Debug, num::NonZeroUsize};

use library::{log::*, prelude::*};

use serde::{Deserialize, Serialize};

fn main() {
    let len = 100;
    let modulo = 7;
    let mut meta = RevMeta::new(NonZeroUsize::new(len + 1), 0, false);
    let mut last_frame_where_modulo_eq_zero =
        LastFrameWhereModuloEqZero::with_capacity(0, len, modulo);

    let plot = get_plot(&mut meta, &mut last_frame_where_modulo_eq_zero, len);
    println!("fwd     {plot}");

    meta.queue_log(0).unwrap();
    let plot = get_plot(&mut meta, &mut last_frame_where_modulo_eq_zero, len);
    println!("bwd log {plot}");

    meta.queue_log(len as u64).unwrap();
    let plot = get_plot(&mut meta, &mut last_frame_where_modulo_eq_zero, len);
    println!("fwd log {plot}");
}

fn get_plot(
    meta: &mut RevMeta,
    last_frame_where_modulo_eq_zero: &mut LastFrameWhereModuloEqZero,
    len: usize,
) -> String {
    let mut plot = VecDeque::with_capacity(len + 1);
    let frame = last_frame_where_modulo_eq_zero.get();
    let character = get_character(frame, meta);
    plot.push_back(character);
    for _ in 0..len {
        meta.update(|meta| {
            let frame = last_frame_where_modulo_eq_zero.update_and_get(meta);
            let character = get_character(frame, meta);
            if meta.direction().is_forward() {
                plot.push_back(character);
            } else {
                plot.push_front(character);
            }
        });
    }
    plot.into_iter().collect()
}

fn get_character(frame: u64, meta: &RevMeta) -> char {
    match frame == meta.now() {
        true => '|',
        false => '.',
    }
}

#[derive(Serialize, Deserialize)]
struct LastFrameWhereModuloEqZero {
    modulo: u8,
    state_for_transition_logs: u64,

    #[serde(with = "logless_with_capacity")]
    dense_state: DenseStateLog<u64>,
    #[serde(with = "logless_with_capacity")]
    scoped_state: DenseStateLog<u64>,

    #[serde(with = "logless_with_capacity")]
    dense_transition: DenseTransitionLog<u8>,
    #[serde(with = "logless_with_capacity")]
    scoped_transition: DenseTransitionLog<u8>,

    #[serde(with = "logless_with_capacity")]
    sparse_state: SparseStateLog<u64>,
    #[serde(with = "logless_with_capacity")]
    sparse_transition: SparseTransitionLog<u8>,

    #[serde(with = "logless_with_capacity")]
    frame_transition: FrameTransitionLog,
}

impl LastFrameWhereModuloEqZero {
    fn with_capacity(state: u64, capacity: usize, modulo: u8) -> Self {
        assert_eq!(state % modulo as u64, 0);
        let scoped_capacity = capacity / modulo as usize;
        Self {
            modulo,
            state_for_transition_logs: state,

            dense_state: DenseStateLog::with_capacity(state, capacity),
            scoped_state: DenseStateLog::with_capacity(state, scoped_capacity),

            dense_transition: DenseTransitionLog::with_capacity(capacity),
            scoped_transition: DenseTransitionLog::with_capacity(scoped_capacity),

            sparse_state: SparseStateLog::with_capacity(state, scoped_capacity),
            sparse_transition: SparseTransitionLog::with_capacity(scoped_capacity),

            frame_transition: FrameTransitionLog::with_capacity(scoped_capacity),
        }
    }
    fn update_and_get(&mut self, meta: &RevMeta) -> u64 {
        let modulo = self.modulo as u64;
        let expected_result = modulo * (meta.now() / modulo);
        /*
        return expected_result;
         */
        match meta.direction() {
            RevDirection::NOT_LOG => {
                let now = meta.now();
                let delta: u8 = (now - self.state_for_transition_logs).try_into().unwrap();
                let past_len = meta.past_len() as usize;
                let update = now % modulo == 0;

                self.dense_state.push_and_pop_past(
                    past_len,
                    update
                        .then_some(now)
                        .unwrap_or(self.state_for_transition_logs),
                );

                self.dense_transition
                    .push_and_pop_past(past_len, update.then_some(delta).unwrap_or(0));

                self.sparse_state
                    .push_and_pop_past(past_len, update.then_some(now));

                self.sparse_transition
                    .push_and_pop_past(past_len, update.then_some(delta));

                if update {
                    let scoped_past_len = self.frame_transition.push_and_get_past_len(&meta);
                    assert_eq!(scoped_past_len, past_len / modulo as usize);

                    self.scoped_state.push_and_drain_past(scoped_past_len, now);
                    self.scoped_transition
                        .push_and_drain_past(scoped_past_len, delta);

                    self.state_for_transition_logs = now;
                }
            }
            RevDirection::FORWARD_LOG => {
                let mut states = vec![];
                let mut transitions = vec![];

                self.dense_state.forward_log().unwrap();
                states.push(*self.dense_state);

                transitions.push(*self.dense_transition.forward_log().unwrap());

                let expect_forward_log = self.frame_transition.forward_log(&meta);
                assert_eq!(expect_forward_log, meta.now() % modulo == 0);

                let state_changed = self.sparse_state.forward_log().unwrap();
                assert_eq!(state_changed, expect_forward_log);
                states.push(*self.sparse_state);

                let transition = self.sparse_transition.forward_log().unwrap().copied();
                assert_eq!(transition.is_some(), expect_forward_log);
                transitions.push(transition.unwrap_or(0));

                if expect_forward_log {
                    self.scoped_state.forward_log().unwrap();
                    states.push(*self.scoped_state);

                    transitions.push(*self.scoped_transition.forward_log().unwrap());
                }

                let transition = assert_equality_get(transitions);
                states.push(self.state_for_transition_logs + transition as u64);
                self.state_for_transition_logs = assert_equality_get(states);
            }
            RevDirection::BackwardLog => {
                let mut states = vec![];
                let mut transitions = vec![];

                self.dense_state.backward_log().unwrap();
                states.push(*self.dense_state);

                transitions.push(*self.dense_transition.backward_log().unwrap());

                let expect_backward_log = self.frame_transition.backward_log(&meta);
                assert_eq!(expect_backward_log, (meta.now() + 1) % modulo == 0);

                let state_changed = self.sparse_state.backward_log().unwrap();
                assert_eq!(state_changed, expect_backward_log);
                states.push(*self.sparse_state);

                let transition = self.sparse_transition.backward_log().unwrap().copied();
                assert_eq!(transition.is_some(), expect_backward_log);
                transitions.push(transition.unwrap_or(0));

                if expect_backward_log {
                    self.scoped_state.backward_log().unwrap();
                    states.push(*self.scoped_state);

                    transitions.push(*self.scoped_transition.backward_log().unwrap());
                }

                let transition = assert_equality_get(transitions);
                states.push(self.state_for_transition_logs - transition as u64);
                self.state_for_transition_logs = assert_equality_get(states);
            }
        }

        assert_eq!(self.state_for_transition_logs, expected_result);

        self.state_for_transition_logs
    }
    fn get(&self) -> u64 {
        assert_equality_get(vec![
            *self.dense_state,
            *self.scoped_state,
            *self.sparse_state,
            self.state_for_transition_logs,
        ])
    }
}

fn assert_equality_get<T: Ord + Debug>(mut v: Vec<T>) -> T {
    v.sort();
    assert_eq!(v.first(), v.last());
    v.pop().unwrap()
}
