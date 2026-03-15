use bevy::{
    ecs::error::Result,
    prelude::*,
    window::{EnabledButtons, WindowResolution},
};
use bevy_oozlum::prelude::*;

use std::{fmt::Write, iter::repeat_n};

use crate::{CURRENT_BEVY_VERSION, GameState, MAX_PAST_LEN, ROWS, Stats, Waste};

// Not many comments here as this is not really a showcase of the crate's functionalities

pub fn window_plugin() -> WindowPlugin {
    WindowPlugin {
        primary_window: Some(Window {
            resizable: false,
            resolution: WindowResolution::new(965, 455 + ROWS as u32 * 20),
            enabled_buttons: EnabledButtons {
                minimize: false,
                maximize: false,
                close: false,
            },
            ..Default::default()
        }),
        ..Default::default()
    }
}

pub fn plugin(app: &mut App) {
    app.add_systems(Startup, setup)
        .add_systems(FixedUpdate, render::<ROWS>.after(RevMeta::run_rev_update));
}

fn setup(mut commands: Commands) {
    commands.spawn(Camera2d);
    commands.spawn((
        Text(String::with_capacity(73 * 23)),
        TextLayout::new_with_no_wrap(),
        Node {
            flex_direction: FlexDirection::Column,
            align_self: AlignSelf::Center,
            justify_self: JustifySelf::Center,
            align_items: AlignItems::Center,
            ..Default::default()
        },
    ));
}

fn render<const ROWS: usize>(
    meta: Res<RevMeta>,
    state: Res<State<GameState>>,
    time_and_counts: Res<Stats>,
    waste: Query<&Waste>,
    mut text: Single<&mut Text>,
    mut last_future_end: Local<Option<u64>>,
) -> Result {
    if meta.paused() {
        return Ok(());
    }

    text.clear();

    let padding_cols = match *last_future_end {
        Some(frame) => {
            let mut padding = frame.wrapping_sub(meta.future_end());
            if padding > MAX_PAST_LEN {
                padding = 0;
                *last_future_end = Some(meta.future_end());
            }
            padding as usize
        }
        None => {
            *last_future_end = Some(meta.future_end());
            0
        }
    };

    let bevy_version = time_and_counts.score + CURRENT_BEVY_VERSION;
    writeln!(
        &mut text.0,
        "Let's waste the time 'til Bevy 1.0 by tossing said waste into the ocean!
No worry, it's okay as long you undo it. Just don't wait for too long...
\n                         It is Bevy {v1}.{v2:02}.{v3} now!\n",
        v1 = bevy_version / 1000,
        v2 = bevy_version % 1000 / 10,
        v3 = bevy_version % 10
    )?;

    let len_to_waves = text.0.len();

    let wave_iter = |offset: u64| "`-._,~'".chars().cycle().skip(7 - offset as usize % 7);

    for _ in 0..ROWS {
        for _ in 0..padding_cols {
            write!(&mut text.0, " ")?;
        }
        for c in wave_iter(meta.future_end())
            .clone()
            .take(meta.len() as usize + 1)
        {
            write!(&mut text.0, "{c}")?;
        }
        writeln!(&mut text.0)?;
    }

    let row_index = |row, past_offset| {
        len_to_waves
        + padding_cols
        + meta.future_len() as usize
        + 1 // digit char of the row
        + (row - 1) * (
            meta.len() as usize
            + padding_cols
            + 2 // 1x digit char of the row + 1x char for \n
        )
        + past_offset
    };

    for &Waste { row, tossed_at } in waste.iter() {
        let index = row_index(row as usize, (meta.now() - tossed_at) as usize);
        // SAFETY:
        // all characters including line breaks are ASCII and random bytes being replaced by the
        // ASCII character '#' will keep the text's utf8 encoding valid
        unsafe {
            text.0.as_bytes_mut()[index] = b'#';
        }
    }

    for row in 1..=ROWS {
        let index = row_index(row, 0) - 1;
        // SAFETY:
        // all characters including line breaks are ASCII and random bytes being replaced by the
        // ASCII characters '1' to '9' will keep the text's utf8 encoding valid
        unsafe {
            text.0.as_bytes_mut()[index] = b'0' + row.min(9) as u8;
        }
    }

    let future_past_marker = repeat_n(' ', 61)
        .chain("<- future | past ->".chars())
        .skip(MAX_PAST_LEN as usize - (padding_cols + meta.future_len() as usize))
        .take(MAX_PAST_LEN as usize);

    for c in future_past_marker {
        write!(&mut text.0, "{c}")?;
    }
    writeln!(&mut text.0)?;

    let now_marker = repeat_n(' ', 70)
        .chain("now".chars())
        .skip(MAX_PAST_LEN as usize - (padding_cols + meta.future_len() as usize))
        .take(MAX_PAST_LEN as usize);

    for c in now_marker {
        write!(&mut text.0, "{c}")?;
    }
    writeln!(&mut text.0, "\n")?;

    let total = waste.iter().len() + time_and_counts.lost as usize;
    let lost = time_and_counts.lost.min(10) as usize;

    if *state.get() == GameState::Running
        && meta
            .get_ran_direction()
            .is_some_and(RevDirection::is_not_log)
    {
        write!(&mut text.0, "1-{ROWS}: toss waste ")?;
    } else {
        write!(&mut text.0, "                ")?;
    }

    write!(&mut text.0, "({total:03}, lost: ")?;

    for _ in 0..lost {
        write!(&mut text.0, "#")?;
    }

    for c in wave_iter(meta.now()).clone().take(10 - lost) {
        write!(&mut text.0, "{c}")?;
    }

    writeln!(&mut text.0, ")          ESC: close")?;

    match state.get() {
        GameState::Running => writeln!(
            &mut text.0,
            "LEFT: forward log, pause at end                  UP: exit log and resume
RIGHT: backward log, pause at end                DOWN: pause
ENTER (hold): reduce past length                 BACKSPACE: clear log"
        )?,
        GameState::Won if (meta.now() / 60) % 2 == 0 => {
            writeln!(
                &mut text.0,
                "\n                    Yay, Bevy 1.0 is there! YOU WON!"
            )?;
        }
        GameState::Lost if (meta.now() / 9) % 2 == 0 => {
            writeln!(
                &mut text.0,
                "\nYou left too much waste behind that you can no longer recover. GAME OVER"
            )?;
        }
        _ => writeln!(&mut text.0, "\n")?, // pulse!
    }

    writeln!(
        &mut text.0,
"\nmeta.past_end()   == {past_end:05}                       meta.past_len()   == {past_len:02}
meta.now()        == {now:05}                       meta.len()        == {len:02}
meta.future_end() == {future_end:05}                       meta.future_len() == {future_len:02}",
        past_end = meta.past_end(),
        past_len = meta.past_len(),
        now = meta.now(),
        len = meta.len(),
        future_end = meta.future_end(),
        future_len = meta.future_len(),
    )?;

    Ok(())
}
