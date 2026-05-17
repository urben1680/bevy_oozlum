use bevy::{prelude::*, window::WindowResolution};
use bevy_oozlum::{prelude::*, schedule::run_rev_update};
use std::{fmt::Write, mem::take};

// This example shows jumping to a specific frame in the log instead of always the log end

fn main() {
    App::new()
        .add_plugins((
            RevPlugin.set_max_past_len(9),
            DefaultPlugins.set(window_plugin()),
        ))
        .add_systems(Startup, setup)
        .add_systems(PreUpdate, control)
        .add_systems(RevUpdate, update_frame_log.after(RevSystems))
        .add_systems(FixedUpdate, render.after(run_rev_update))
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

fn window_plugin() -> WindowPlugin {
    WindowPlugin {
        primary_window: Some(Window {
            resizable: false,
            resolution: WindowResolution::new(430, 550),
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);
    commands.spawn((
        Text::default(),
        TextLayout::no_wrap(),
        Node {
            flex_direction: FlexDirection::Column,
            align_self: AlignSelf::Center,
            justify_self: JustifySelf::Center,
            align_items: AlignItems::Center,
            ..Default::default()
        },
    ));
}

fn render(meta: Res<RevMeta>, frame_log: Res<FrameLog>, mut text: Single<&mut Text>) -> Result {
    text.clear();

    writeln!(
        &mut text.0,
        "UP: backward log, pause at end
DOWN: forward log, pause at end
RIGHT: exit log and resume
LEFT: pause
ESCAPE: close"
    )?;

    if meta
        .get_ran_direction()
        .is_some_and(RevDirection::is_not_log)
    {
        write!(&mut text.0, "SPACE: mark frame to pause at")?;
    }

    match meta.get_ran_direction() {
        None => writeln!(&mut text.0, "\n\nPause\n\n-- log start --"),
        Some(direction) => writeln!(&mut text.0, "\n\n{direction}\n\n-- log start --"),
    }?;

    for frame in meta.past_end()..=meta.future_end() {
        let marked = match frame_log.marked_frames.binary_search(&frame).is_ok() {
            false => ' ',
            true => 'X',
        };
        let now = match frame == meta.now() {
            false => ' ',
            true => '<',
        };
        writeln!(&mut text.0, "{frame:05} {marked} {now}")?;
    }

    for _ in meta.len()..10 {
        writeln!(&mut text.0)?;
    }

    write!(&mut text.0, "-- log end --")?;

    Ok(())
}
