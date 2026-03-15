use std::num::NonZeroU64;

use bevy::{input::keyboard::Key, prelude::*};
use bevy_oozlum::prelude::*;

use crate::{GameState, MAX_PAST_LEN, ROWS};

pub fn plugin(app: &mut App) {
    app.add_systems(
        // read and use inputs before RevUpdate
        PreUpdate, system,
    )
    .add_systems(
        // reset the tracked digit inputs after systems in RevSystems read them
        RevUpdate,
        reset.after(RevSystems),
    )
    .insert_resource(JustPressed([false; ROWS].into()));
}

// Store which digit was pressed as ButtonInput may be cleared before FixedUpdate runs
#[derive(Resource)]
pub struct JustPressed(Box<[bool]>);

impl JustPressed {
    pub fn get(&self, row: u64) -> bool {
        self.0.get(row as usize - 1).copied().unwrap_or(false)
    }
}

fn system(
    input: Res<ButtonInput<KeyCode>>,
    digit_input: Res<ButtonInput<Key>>,
    mut exit: MessageWriter<AppExit>,
    state: Res<State<GameState>>,
    mut meta: ResMut<RevMeta>,
    mut just_pressed: ResMut<JustPressed>,
) {
    if input.just_pressed(KeyCode::Escape) {
        exit.write(AppExit::Success);
        return;
    }

    if *state.get() != GameState::Running {
        // only allow Esc when the game is over
        return;
    }

    // RevMeta::set_queue is used to control how RevUpdate is run
    if input.just_pressed(KeyCode::ArrowUp) {
        meta.set_queue(RevQueue::RunForward);
    } else if input.just_pressed(KeyCode::ArrowDown) {
        meta.set_queue(RevQueue::Pause);
    } else if input.just_pressed(KeyCode::ArrowLeft) {
        meta.set_queue(RevQueue::RunForwardLog);
    } else if input.just_pressed(KeyCode::ArrowRight) {
        meta.set_queue(RevQueue::RunBackwardLog);
    } else if input.just_pressed(KeyCode::Backspace) {
        // One can also pause after clearing with RevQueue::ClearThenPause.
        // Beware that this instantly loses all tossed waste that was not undone yet
        meta.set_queue(RevQueue::ClearThenRunForward);
    }

    // The maximum past length can be adjusted at any time and has an effect the next time RevUpdate
    // is about to be run.
    if input.pressed(KeyCode::Enter) {
        let max_past_len = meta.past_len().saturating_sub(1).max(1);
        meta.set_max_past_len(NonZeroU64::new(max_past_len).unwrap());
    } else if input.just_released(KeyCode::Enter) {
        meta.set_max_past_len(NonZeroU64::new(MAX_PAST_LEN).unwrap());
    }

    for row in 1..=ROWS {
        if digit_input.just_pressed(Key::Character(row.to_string().into())) {
            just_pressed.0[row - 1] = true;
        }
    }
}

fn reset(mut digits: ResMut<JustPressed>) {
    digits
        .bypass_change_detection()
        .0
        .iter_mut()
        .for_each(|pressed| *pressed = false);
}
