use std::num::NonZeroU64;

use bevy::{input::keyboard::Key, prelude::*};
use bevy_oozlum::prelude::*;

use crate::{GameState, MAX_PAST_LEN};

pub fn plugin<const ROWS: usize>(app: &mut App) {
    app.add_systems(Update, system::<ROWS>.before(RevSystems))
        .add_systems(
            RevUpdate,
            cleanup
                .run_if(resource_changed::<JustPressed>)
                .after(RevSystems),
        )
        .insert_resource(JustPressed([false; ROWS].into()));
}

#[derive(Resource)]
pub struct JustPressed(Box<[bool]>);

impl JustPressed {
    pub fn get<const ROW: u64>(&self) -> bool {
        self.0.get(ROW as usize - 1).copied().unwrap_or(false)
    }
}

fn system<const ROWS: usize>(
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
        return;
    }

    if input.just_pressed(KeyCode::ArrowUp) {
        meta.set_queue(RevQueue::RunForward);
    } else if input.just_pressed(KeyCode::ArrowDown) {
        meta.set_queue(RevQueue::Pause);
    } else if input.just_pressed(KeyCode::ArrowLeft) {
        meta.set_queue(RevQueue::RunForwardLog);
    } else if input.just_pressed(KeyCode::ArrowRight) {
        meta.set_queue(RevQueue::RunBackwardLog);
    } else if input.just_pressed(KeyCode::Backspace) {
        meta.set_queue(RevQueue::ClearThenRunForward);
    } else if input.pressed(KeyCode::Enter) {
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

fn cleanup(mut digits: ResMut<JustPressed>) {
    digits
        .bypass_change_detection()
        .0
        .iter_mut()
        .for_each(|pressed| *pressed = false);
}
