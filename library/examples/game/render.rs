use bevy::{
    prelude::*,
    window::{EnabledButtons, WindowResolution},
};
use bevy_oozlum::meta::{RevDirection, RevMeta};

use std::fmt::Write;

use crate::{CURRENT_BEVY_VERSION, MAX_PAST_LEN, Stats, TossedAt, WINNING_BEVY_VERSION, Waste};

pub fn window_plugin<const ROWS: usize>() -> WindowPlugin {
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

pub fn plugin<const ROWS: usize>(app: &mut App) {
    app.add_systems(Startup, setup)
        .add_systems(FixedUpdate, render::<ROWS>.after(RevMeta::run_rev_update));
}

fn setup(mut commands: Commands) {
    // UI camera
    commands.spawn(Camera2d);
    // Text with one section
    commands.spawn((
        // Accepts a `String` or any type that converts into a `String`, such as `&str`
        Text::default(),
        // Set the justification of the Text
        TextLayout::new_with_no_wrap(),
        // Set the style of the Node itself.
        Node {
            flex_direction: FlexDirection::Column,
            align_self: AlignSelf::Center,
            justify_self: JustifySelf::Center,
            align_items: AlignItems::Center,
            ..Default::default()
        },
        TextFont {
            weight: FontWeight::EXTRA_LIGHT,
            ..Default::default()
        },
    ));
}

// todo: refactor with write! blocks
fn render<const ROWS: usize>(
    meta: Res<RevMeta>,
    time_and_counts: Res<Stats>,
    waste: Query<(&Waste, &TossedAt)>,
    mut text: Single<&mut Text>,
    mut last_future_end: Local<Option<u64>>,
) {
    if meta.paused() {
        return;
    }

    text.clear();

    let wave = |phase: u64| "`-._,~'".chars().cycle().skip(7 - phase as usize % 7);

    let row_future: String = wave(meta.future_end())
        .take(meta.future_len() as usize)
        .collect();

    let row_past: String = wave(meta.future_end())
        .skip((meta.future_len() + 1) as usize % 7)
        .take(meta.past_len() as usize + 1)
        .collect();
    let mut past_rows: [String; ROWS] = std::array::from_fn(|_| row_past.clone());

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
    let padding = " ".repeat(padding_cols);

    for (&waste, &tossed_at) in waste.iter() {
        let Some(index) = meta.now().checked_sub(tossed_at.0) else {
            panic!(
                "now {} < tossed_at {} of row {}",
                meta.now(),
                tossed_at.0,
                waste.row
            );
        };
        let index = index as usize;
        past_rows
            .get_mut(waste.row as usize - 1)
            .unwrap()
            .replace_range(index..=index, "#");
    }

    let mut bevy_version = format!("{:04}", time_and_counts.score + CURRENT_BEVY_VERSION);
    bevy_version.insert(3, '.');
    bevy_version.insert(1, '.');

    writeln!(
        &mut text.0,
        "Let's waste the time 'til Bevy 1.0 by tossing said waste into the ocean!"
    )
    .unwrap();
    writeln!(
        &mut text.0,
        "No worry, it's okay as long you undo it. Just don't wait for too long..."
    )
    .unwrap();
    writeln!(&mut text.0).unwrap();
    writeln!(
        &mut text.0,
        "                         It is Bevy {bevy_version} now!                         "
    )
    .unwrap();
    writeln!(&mut text.0).unwrap();

    for (i, past_row) in past_rows.into_iter().enumerate() {
        writeln!(&mut text.0, "{padding}{row_future}{}{past_row}", i + 1).unwrap();
    }

    let total = waste.iter().len() + time_and_counts.lost as usize;
    let lost = time_and_counts.lost.min(10) as usize;
    let lost_bar: String = "#"
        .repeat(lost)
        .chars()
        .chain(wave(meta.now()).take(10 - lost))
        .collect();

    let marker1 =
        "                                                             <- future | past ->"
            .chars()
            .skip(MAX_PAST_LEN as usize - (padding_cols + meta.future_len() as usize))
            .take(MAX_PAST_LEN as usize)
            .collect::<String>();
    let marker2 = "                                                                      now"
        .chars()
        .skip(MAX_PAST_LEN as usize - (padding_cols + meta.future_len() as usize))
        .take(MAX_PAST_LEN as usize)
        .collect::<String>();

    writeln!(&mut text.0, "{marker1}").unwrap();
    writeln!(&mut text.0, "{marker2}").unwrap();
    writeln!(&mut text.0).unwrap();

    if meta.get_ran_direction().is_some_and(RevDirection::is_log) || lost == 10 {
        writeln!(
            &mut text.0,
            "                ({total:03}, lost: {lost_bar})          ESC: close"
        )
        .unwrap();
    } else {
        writeln!(
            &mut text.0,
            "1-7: toss waste ({total:03}, lost: {lost_bar})          ESC: close"
        )
        .unwrap();
    }

    let blink = (meta.now() / 8) % 2 == 0;
    if lost < 10 {
        if time_and_counts.score + CURRENT_BEVY_VERSION < WINNING_BEVY_VERSION {
            writeln!(
                &mut text.0,
                "LEFT: forward log, pause at end                  UP: exit log and resume"
            )
            .unwrap();
            writeln!(
                &mut text.0,
                "RIGHT: backward log, pause at end                DOWN: pause"
            )
            .unwrap();
            writeln!(
                &mut text.0,
                "ENTER (hold): reduce past length                 BACKSPACE: clear log"
            )
            .unwrap();
        } else {
            writeln!(&mut text.0).unwrap();
            if blink {
                writeln!(
                    &mut text.0,
                    "                    Yay, Bevy 1.0 is there! YOU WON!"
                )
                .unwrap();
            } else {
                writeln!(&mut text.0).unwrap();
            }
        }
    } else {
        writeln!(&mut text.0).unwrap();
        if blink {
            writeln!(
                &mut text.0,
                "You left too much waste behind that you can no longer recover. GAME OVER"
            )
            .unwrap();
        } else {
            writeln!(&mut text.0).unwrap();
        }
    }

    writeln!(&mut text.0).unwrap();
    writeln!(
        &mut text.0,
        "meta.past_end()   == {:05}                       meta.past_len()   == {:02}",
        meta.past_end(),
        meta.past_len()
    )
    .unwrap();
    writeln!(
        &mut text.0,
        "meta.now()        == {:05}                       meta.len()        == {:02}",
        meta.now(),
        meta.len()
    )
    .unwrap();
    writeln!(
        &mut text.0,
        "meta.future_end() == {:05}                       meta.future_len() == {:02}",
        meta.future_end(),
        meta.future_len()
    )
    .unwrap();
}
