use bevy::{prelude::*, window::WindowResolution};
use bevy_oozlum::{prelude::*, schedule::run_rev_update};
use std::fmt::Write;

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
    marked_frames: Vec<u64>,
    mark_next_frame: bool,
}

fn update_frame_log(mut meta: ResMut<RevMeta>, mut frame_log: ResMut<FrameLog>) {
    match meta.running_direction() {
        RevDirection::NotLog(_) => {
            frame_log
                .marked_frames
                .retain(|frame| meta.contains(*frame));
            if frame_log.mark_next_frame {
                frame_log.marked_frames.push(meta.now());
            }
        }
        RevDirection::BackwardLog => {
            let now = meta.now() - 1;
            if frame_log.marked_frames.binary_search(&now).is_ok() {
                meta.set_queue(RevQueue::Pause);
            }
        }
        RevDirection::ForwardLog => {
            if frame_log.marked_frames.binary_search(&meta.now()).is_ok() {
                meta.set_queue(RevQueue::Pause);
            }
        }
    }

    frame_log.mark_next_frame = false;
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

    let space = match meta.get_ran_direction().is_none_or(RevDirection::is_log) {
        false => "SPACE: mark frame to pause at",
        true => "",
    };

    writeln!(
        &mut text.0,
        "UP: backward log, pause at end
DOWN: forward log, pause at end
RIGHT: exit log and resume
LEFT: pause
ESCAPE: close
{space}
"
    )?;

    match meta.get_ran_direction() {
        None => writeln!(&mut text.0, "Pause\n\n-- log start --"),
        Some(direction) => writeln!(&mut text.0, "{direction}\n\n-- log start --"),
    }?;

    for frame in meta.past_end()..=meta.future_end() {
        let now = match frame == meta.now() {
            false => ' ',
            true => '<',
        };
        let marked = match frame_log.marked_frames.binary_search(&frame).is_ok() {
            false => ' ',
            true => 'X',
        };
        writeln!(&mut text.0, "{frame:05} {marked} {now}")?;
    }

    for _ in meta.len()..10 {
        writeln!(&mut text.0)?;
    }

    write!(&mut text.0, "-- log end --")?;

    Ok(())
}
