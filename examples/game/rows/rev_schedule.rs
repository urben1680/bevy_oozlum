use bevy::{ecs::schedule::ScheduleLabel, prelude::*};
use bevy_oozlum::prelude::*;

use crate::{Waste, control::JustPressed};

pub fn plugin<const ROW: u64>(app: &mut App) {
    // Use rev_add_systems for reversible systems.
    app.rev_add_systems(RevUpdate, system1::<ROW>)
        .rev_add_systems(MyRevSchedule, system2::<ROW>);
}

#[derive(ScheduleLabel, Copy, Clone, Debug, Hash, PartialEq, Eq)]
struct MyRevSchedule;

fn system1<const ROW: u64>(not_log: NotLog, input: Res<JustPressed>, mut commands: Commands) {
    if input.get(ROW) {
        // NotLog is like a token to prove that methods needing it are called during
        // RevDirection::NotLog. Because of this it should not be stored past that.

        commands.as_rev(not_log).rev_run_schedule(MyRevSchedule);
    }
}

fn system2<const ROW: u64>(meta: Res<RevMeta>, mut commands: Commands) {
    if let Some(not_log) = meta.get_not_log() {
        // As Commands::spawn, this spawns an entity.
        // If this is undone, the entity is at first disabled and later fully despawned if the redo
        // becomes unreachable for RevDirection::BackwardLog.
        commands.as_rev(not_log).rev_spawn(Waste {
            row: ROW,
            tossed_at: meta.now(),
        });
    }
}
