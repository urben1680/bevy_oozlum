[![License](https://img.shields.io/badge/license-MIT%2FApache-blue.svg)](https://github.com/urben1680/bevy_oozlum#license)
[![Crates.io](https://img.shields.io/crates/v/bevy_oozlum.svg)](https://crates.io/crates/bevy_oozlum)
[![Docs](https://docs.rs/bevy_oozlum/badge.svg)](https://docs.rs/bevy_oozlum/latest/bevy/)

# ![Bevy Oozlum](https://raw.githubusercontent.com/urben1680/bevy_oozlum/refs/heads/main/logo.png)

Bevy Oozlum is a crate for [Bevy](https://bevy.org/) to write reversible systems, commands and schedules. It may be useful to implement rewind features in a game that run as smoothly as the 
normal gameplay.

"Oozlum" is a mythical bird that is able to fly backwards.

## Examples

```rs
use bevy::prelude::*;
use bevy_oozlum::prelude::*;

// reversible logic happens in commands
fn rev_system_1(not_log: NotLog, mut commands: Commands) {
    commands.queue(|_: &mut World| println!("hello world!"));
    commands.as_rev(not_log).queue_undo_redo(|_: &mut World, direction| {
        match direction {
            UndoRedoDirection::Undo => println!("!dlrow olleh (log)"),
            UndoRedoDirection::Redo => println!("hello world! (log)"),
        }
    });
}

// reversible logic happens in system
fn rev_system_2(meta: Res<RevMeta>) {
    match meta.running_direction() {
        RevDirection::NotLog(_) => println!("hello world!"),
        RevDirection::BackwardLog => println!("!dlrow olleh (log)"),
        RevDirection::ForwardLog => println!("hello world! (log)")
    }
}

// reversible rev_* variants of numerous bevy API
app.rev_add_systems(
    RevUpdate, // main reversible schedule
    (rev_system_1, rev_system_2).rev_chain() // reversed during RevDirection::BackwardLog
);

// control how and if RevUpdate is ran
fn input_system(
    keyboard_input: Res<ButtonInput<KeyCode>>,
    mut meta: ResMut<RevMeta>
) {
    if keyboard_input.pressed(KeyCode::ArrowUp) {
        meta.set_queue(RevQueue::RunNotLog);
        println!("queue forward, truncates too-old past frames and all future frames");
    } else if keyboard_input.pressed(KeyCode::ArrowDown) {
        meta.set_queue(RevQueue::Pause);
        println!("queue pause, will not run RevUpdate until unpaused");
    } else if keyboard_input.pressed(KeyCode::ArrowLeft) {
        meta.set_queue(RevQueue::RunBackwardLog);
        println!("queue backward log, reverts logged frames, pauses at past end");
    } else if keyboard_input.pressed(KeyCode::ArrowRight) {
        meta.set_queue(RevQueue::RunForwardLog);
        println!("queue forward log, advances logged frames, pauses at future end");
    }
}
```

An example game is available that showcases the most important API additions. See the documentation to learn more.

## Warning

This crate is experimential and may be discontinued at any time.

## Supported bevy version

| Bevy Oozlum | Bevy |
| - | - |
| 0.1.0-rc.1 | =0.19.0-rc.1 |

## License

This crate aligns with bevy's licensing:

* MIT License ([LICENSE-MIT](LICENSE-MIT) or [http://opensource.org/licenses/MIT](http://opensource.org/licenses/MIT))
* Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or [http://www.apache.org/licenses/LICENSE-2.0](http://www.apache.org/licenses/LICENSE-2.0))
