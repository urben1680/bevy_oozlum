use bevy::{prelude::*, window::WindowResolution};
use bevy_oozlum::{prelude::*, schedule::run_rev_update};
use std::fmt::Write;

use super::FrameLog;

// Not many comments here as this is not really a showcase of the crate's functionalities

pub fn plugin(app: &mut App) {
    app.add_systems(Startup, setup)
        .add_systems(FixedUpdate, render.after(run_rev_update));
}

pub fn window_plugin() -> WindowPlugin {
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

    if frame_log.allow_backward {
        write!(&mut text.0, "UP: backward log, pause at end")?;
    }

    writeln!(
        &mut text.0,
        "
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
