use std::fmt::Debug;

use library::{log::*, prelude::*};

use serde::{Deserialize, Serialize};

fn main() {
    let len = 100;
    let modulo = 7;
    let mut meta = RevMeta::new(None, 0, false);
    let mut logs = Logs::with_capacity(len, modulo);

    println!("fwd     {}", get_plot(&mut meta, &mut logs, len, modulo));
    meta.queue_log(0).unwrap();
    println!("bwd log {}", get_plot(&mut meta, &mut logs, len, modulo));
    meta.queue_log(len as u64).unwrap();
    println!("fwd log {}", get_plot(&mut meta, &mut logs, len, modulo));
}

fn get_plot(meta: &mut RevMeta, logs: &mut Logs, len: usize, modulo: u8) -> String {
    let mut plot = "?".repeat(len + 1);
    for _ in 0..len {
        meta.update(|meta| {
            let frame = logs.get_last_frame_where_module_eq_zero(meta, modulo);
            let character = match frame == meta.now() {
                true => "|",
                false => ".",
            };
            let i = meta.now() as usize;
            plot.replace_range(i..=i, character);
        });
    }
    plot
}

#[derive(Serialize, Deserialize)]
struct Logs {
    last_frame_where_modulo_eq_zero: u64,

    #[serde(with = "logless_with_capacity")]
    dense_state: DenseStateLog<u64>,
    #[serde(with = "logless_with_capacity")]
    scoped_state: DenseStateLog<u64>,

    #[serde(with = "logless_with_capacity")]
    sparse_state: SparseStateLog<u64>,
    #[serde(with = "logless_with_capacity")]
    sparse_transition: SparseTransitionLog<u8>,

    #[serde(with = "logless_with_capacity")]
    frame_transition: FrameTransitionLog,
    #[serde(with = "logless_with_capacity")]
    dense_transition: DenseTransitionLog<u8>,
    #[serde(with = "logless_with_capacity")]
    scoped_transition: DenseTransitionLog<u8>,
}

impl Logs {
    fn with_capacity(capacity: usize, modulo: u8) -> Self {
        let scoped_capacity = capacity / modulo as usize;
        Self {
            last_frame_where_modulo_eq_zero: 0,
            dense_state: DenseStateLog::with_capacity(0, capacity),
            dense_transition: DenseTransitionLog::with_capacity(capacity),

            sparse_state: SparseStateLog::with_capacity(0, scoped_capacity),
            sparse_transition: SparseTransitionLog::with_capacity(scoped_capacity),
            scoped_state: DenseStateLog::with_capacity(0, scoped_capacity),
            scoped_transition: DenseTransitionLog::with_capacity(scoped_capacity),
            frame_transition: FrameTransitionLog::with_capacity(scoped_capacity),
        }
    }
    fn get_last_frame_where_module_eq_zero(&mut self, meta: &RevMeta, modulo: u8) -> u64 {
        let modulo = modulo as u64;
        let expected_result = modulo * (meta.now() / modulo);
        /*
        return expected_result;
         */
        match meta.direction() {
            RevDirection::NOT_LOG => {
                let now = meta.now();
                let delta: u8 = (now - self.last_frame_where_modulo_eq_zero)
                    .try_into()
                    .unwrap();
                let past_len = meta.past_len() as usize;
                let update = now % modulo == 0;

                self.dense_state.push_and_pop_past(
                    past_len,
                    update
                        .then_some(now)
                        .unwrap_or(self.last_frame_where_modulo_eq_zero),
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

                    self.last_frame_where_modulo_eq_zero = now;
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
                states.push(self.last_frame_where_modulo_eq_zero + transition as u64);
                self.last_frame_where_modulo_eq_zero = assert_equality_get(states);
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
                states.push(self.last_frame_where_modulo_eq_zero - transition as u64);
                self.last_frame_where_modulo_eq_zero = assert_equality_get(states);
            }
        }

        assert_eq!(self.last_frame_where_modulo_eq_zero, expected_result);

        self.last_frame_where_modulo_eq_zero
    }
}

fn assert_equality_get<T: Ord + Debug>(mut v: Vec<T>) -> T {
    v.sort();
    assert_eq!(v.first(), v.last());
    v.pop().unwrap()
}
