use bevy::{
    input::{InputSystems, keyboard::Key},
    prelude::*,
};
use bevy_oozlum::prelude::*;

use crate::{GameState, MAX_PAST_LEN, ROWS};

pub fn plugin(app: &mut App) {
    #[cfg(feature = "ci_mode")]
    let system = system.before(InputSystems);
    #[cfg(not(feature = "ci_mode"))]
    let system = system.after(InputSystems);

    app.add_systems(
        // read and use inputs before RevUpdate
        PreUpdate, system,
    )
    .add_systems(
        // reset the tracked digit inputs after systems in RevSystems read them
        RevUpdate,
        reset.after(RevSystems),
    )
    .insert_resource(JustPressed([false; ROWS]));
}

// Store which digit was pressed as ButtonInput may be cleared before FixedUpdate runs
#[derive(Resource)]
pub struct JustPressed([bool; ROWS]);

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
    mut commands: Commands,
) {
    if input.just_pressed(KeyCode::Escape) {
        exit.write(AppExit::Success);
        return;
    }

    if *state.get() != GameState::Running {
        // only allow Esc when the game is over
        return;
    }

    // RevQueue is used to control how/if RevUpdate is run
    if input.just_pressed(KeyCode::ArrowUp) {
        commands.queue(RevQueue::RunNotLog);
    } else if input.just_pressed(KeyCode::ArrowDown) {
        commands.queue(RevQueue::Pause);
    } else if input.just_pressed(KeyCode::ArrowLeft) {
        commands.queue(RevQueue::RunForwardLog);
    } else if input.just_pressed(KeyCode::ArrowRight) {
        commands.queue(RevQueue::RunBackwardLog);
    } else if input.just_pressed(KeyCode::Backspace) {
        // One can also pause after clearing with RevQueue::ClearThenPause.
        // Beware that this instantly loses all tossed waste that was not undone yet.
        commands.queue(RevQueue::ClearThenRunNotLog);
    }

    // The maximum past length can be adjusted at any time and has an effect the next time RevUpdate
    // is about to be run.
    if input.pressed(KeyCode::Enter) && meta.is_running_not_log() {
        let max_past_len = meta.past_len().saturating_sub(1);
        meta.set_max_past_len(max_past_len);
    } else if input.just_released(KeyCode::Enter) {
        meta.set_max_past_len(MAX_PAST_LEN);
    }

    for (index, pressed) in just_pressed.0.iter_mut().enumerate() {
        let row = index + 1;
        if digit_input.just_pressed(Key::Character(row.to_string().into())) {
            *pressed = true;
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
