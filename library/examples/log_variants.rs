use std::{fmt::Debug, io::stdout, num::NonZeroU64, time::Duration};

use bevy::{
    app::App,
    ecs::{component::ComponentId, world::DeferredWorld},
    prelude::*,
};

use crossterm::{cursor::*, terminal::*, ExecutableCommand};

use library::{
    log::{
        DenseStateLog, DenseTransitionLog, FrameTransitionLog, SparseStateLog, SparseTransitionLog, logless_with_capacity
    },
    prelude::*,
};

fn main() {
    let len = 100;
    let mut meta = RevMeta::new(None, 0, false);
    let modulo = 7;
    let mut logs = Logs::with_capacity(len, modulo);

    let mut buffer = "?".repeat(len + 1);
    for _ in 0..len {
        meta.update(|meta| {
            let frame = logs.get_last_frame_where_module_eq_zero(meta, modulo);
            update_buffer(&mut buffer, frame, meta);
        });
    }
    println!("fwd     {buffer}");

    meta.queue_log(0).unwrap();

    let mut buffer = "?".repeat(len + 1);
    for _ in 0..len {
        meta.update(|meta| {
            let frame = logs.get_last_frame_where_module_eq_zero(meta, modulo);
            update_buffer(&mut buffer, frame, meta);
        });
    }
    println!("bwd log {buffer}");
    
    meta.queue_log(len as u64).unwrap();

    let mut buffer = "?".repeat(len + 1);
    for _ in 0..len {
        meta.update(|meta| {
            let frame = logs.get_last_frame_where_module_eq_zero(meta, modulo);
            update_buffer(&mut buffer, frame, meta);
        });
    }
    println!("fwd log {buffer}");
}

fn update_buffer(buffer: &mut String, frame: u64, meta: &RevMeta) {
    let c = if frame == meta.now() {
        "|"
    } else {
        "."
    };
    let i = meta.now() as usize;
    buffer.replace_range(i..=i, &c);
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Logs {
    last_frame_where_modulo_eq_zero: u64,

    #[serde(with = "logless_with_capacity")]
    dense_state: DenseStateLog<u64>,
    #[serde(with = "logless_with_capacity")]
    scoped_state: DenseStateLog<u64>,

    #[serde(with = "logless_with_capacity")]
    sparse_state: SparseStateLog<u64>,

    #[serde(with = "logless_with_capacity")]
    dense_transition: DenseTransitionLog<u8>,
    #[serde(with = "logless_with_capacity")]
    scoped_transition: DenseTransitionLog<u8>,

    #[serde(with = "logless_with_capacity")]
    sparse_transition: SparseTransitionLog<u8>,

    #[serde(with = "logless_with_capacity")]
    frame_transition: FrameTransitionLog,
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
            frame_transition: FrameTransitionLog::with_capacity(scoped_capacity)
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
                    self.scoped_transition.push_and_drain_past(scoped_past_len, delta);

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

        assert_eq!(self.last_frame_where_modulo_eq_zero, expected_result, "{meta:#?}");

        self.last_frame_where_modulo_eq_zero
    }
    fn clear(&mut self, capacity: usize, modulo: u8) {
        let scoped_capacity = capacity / modulo as usize;

        self.last_frame_where_modulo_eq_zero = 0;
        self.dense_state.clear();
        self.dense_transition.clear();
        self.sparse_state.clear();
        self.sparse_transition.clear();
        self.scoped_state.clear();
        self.scoped_transition.clear();
        self.frame_transition.clear();

        self.dense_state.states_reserve_exact(capacity);
        self.dense_transition.transitions_reserve_exact(capacity);

        self.sparse_state.states_reserve_exact(scoped_capacity);
        self.sparse_transition.transitions_reserve_exact(scoped_capacity);
        self.scoped_state.states_reserve_exact(scoped_capacity);
        self.scoped_transition.transitions_reserve_exact(scoped_capacity);
        self.frame_transition.frames_reserve_exact(scoped_capacity);
    }
}

fn assert_equality_get<T: Ord + Debug>(mut v: Vec<T>) -> T {
    v.sort();
    assert_eq!(v.first(), v.last());
    v.pop().unwrap()
}
