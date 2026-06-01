#![allow(missing_docs)]

use bevy::prelude::*;
use bevy_oozlum::prelude::*;

// This example shows jumping to a specific frame in the log instead of always the log end.
// We also suppress going further backward if the oldest marked frame is reached or there is none.

fn main() {
    App::new()
        .add_plugins((
            RevPlugin.set_max_past_len(9),
            DefaultPlugins.set(render::window_plugin()),
            render::plugin,
        ))
        .add_systems(PreUpdate, input_system)
        .add_systems(RevUpdate, update_frame_log.after(RevSystems))
        .init_resource::<FrameLog>()
        .insert_resource(Time::<Fixed>::from_seconds(0.33))
        .run();
}

#[derive(Resource, Default)]
struct FrameLog {
    // frames that can be jumped to
    marked_frames: Vec<u64>,

    // signal next frame to be added to marked_frames, is ignored during log directions
    mark_next_frame: bool,

    // is true if marked_frames contains a frame that is older than meta.now()
    allow_backward: bool,
}

fn update_frame_log(meta: Res<RevMeta>, mut frame_log: ResMut<FrameLog>, mut commands: Commands) {
    // take and thus reset mark_next_frame
    let mark = std::mem::take(&mut frame_log.mark_next_frame);

    if meta.is_running_not_log() {
        // remove out-of-log frames
        frame_log
            .marked_frames
            .retain(|frame| meta.past_contains(*frame));

        // any remaining marked frame would be less then meta.now(), allowing going backward to it
        frame_log.allow_backward = !frame_log.marked_frames.is_empty();

        // mark frame if desired
        if mark {
            frame_log.marked_frames.push(meta.now());
        }

        return;
    }

    if let Ok(index) = frame_log
        .marked_frames
        .binary_search(&meta.now_after_running())
    {
        // if we reached the oldest marked frame, forbid going further backward
        frame_log.allow_backward = index != 0;

        // pause at this marked frame
        commands.queue(RevQueue::Pause);
    } else if meta.is_running_forward_log() {
        // going forward in log implies we are allowed to go back to the previous frame again
        frame_log.allow_backward = true;
    }
}

fn input_system(
    input: Res<ButtonInput<KeyCode>>,
    mut frame_log: ResMut<FrameLog>,
    mut commands: Commands,
) {
    if input.just_pressed(KeyCode::ArrowRight) {
        commands.queue(RevQueue::RunNotLog);
    } else if input.just_pressed(KeyCode::ArrowLeft) {
        commands.queue(RevQueue::Pause);
    } else if input.just_pressed(KeyCode::ArrowDown) {
        commands.queue(RevQueue::RunForwardLog);
    } else if input.just_pressed(KeyCode::ArrowUp) && frame_log.allow_backward {
        commands.queue(RevQueue::RunBackwardLog);
    } else if input.just_pressed(KeyCode::Space) {
        frame_log.mark_next_frame = true;
    } else if input.just_pressed(KeyCode::Escape) {
        commands.write_message(AppExit::Success);
    }
}

mod render;
