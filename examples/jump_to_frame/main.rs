use bevy::prelude::*;
use bevy_oozlum::prelude::*;
use std::mem::take;

// This example shows jumping to a specific frame in the log instead of always the log end

fn main() {
    App::new()
        .add_plugins((
            RevPlugin.set_max_past_len(9),
            DefaultPlugins.set(render::window_plugin()),
            render::plugin,
        ))
        .add_systems(PreUpdate, control)
        .add_systems(RevUpdate, update_frame_log.after(RevSystems))
        .init_resource::<FrameLog>()
        .insert_resource(Time::<Fixed>::from_seconds(0.33))
        .run();
}

#[derive(Resource, Default)]
struct FrameLog {
    // frames that can be jumped to
    marked_frames: Vec<u64>,

    // signal next frame to be added to marked_frames
    mark_next_frame: bool,
}

fn update_frame_log(mut meta: ResMut<RevMeta>, mut frame_log: ResMut<FrameLog>) {
    // take and thus reset mark_next_frame
    let mark = take(&mut frame_log.mark_next_frame);

    if meta.is_running_not_log() {
        // remove out-of-log frames
        frame_log
            .marked_frames
            .retain(|frame| meta.past_contains(*frame));

        // mark frame if desired
        if mark {
            frame_log.marked_frames.push(meta.now());
        }

        return;
    }

    let mut now = meta.now();

    if meta.is_running_backward_log() {
        // RevMeta::now is increased at the *beginning* of:
        // - RevDirection::NotLog
        // - RevDirection::ForwardLog
        //
        // Therefore, a reverse of this will decrease the value at the *end* of
        // RevDirection::BackwardLog. As we want to pause the app when the targeted world state of
        // frame N is reached, we have to account for the frame to not be updated here yet and
        // reduce it to the value at the end of this update.
        now -= 1;
    }

    if frame_log.marked_frames.binary_search(&now).is_ok() {
        meta.set_queue(RevQueue::Pause);
    }
}

fn control(
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
    } else if input.just_pressed(KeyCode::ArrowUp) {
        commands.queue(RevQueue::RunBackwardLog);
    } else if input.just_pressed(KeyCode::Space) {
        frame_log.mark_next_frame = true;
    } else if input.just_pressed(KeyCode::Escape) {
        commands.write_message(AppExit::Success);
    }
}

mod render;
