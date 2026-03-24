use bevy::{ecs::schedule::ScheduleLabel, prelude::*};
use bevy_oozlum::prelude::*;

use crate::{Waste, control::JustPressed};

pub fn plugin<const ROW: u64>(app: &mut App) {
    // Use rev_add_systems for reversible systems.
    app.rev_add_systems(RevUpdate, system1)
        .rev_add_systems(RevUpdateInner, system2::<ROW>);
}

#[derive(ScheduleLabel, Copy, Clone, Debug, Hash, PartialEq, Eq)]
struct RevUpdateInner;

fn system1(not_log: NotLog, mut commands: Commands) {
    // NotLog is like a token to prove that methods needing it are called during
    // RevDirection::NotLog. Because of this it should not be stored past that.

    commands.rev_run_schedule(not_log, RevUpdateInner);
}

fn system2<const ROW: u64>(input: Res<JustPressed>, meta: Res<RevMeta>, mut commands: Commands) {
    if input.get(ROW)
        && let Some(not_log) = meta.get_not_log()
    {
        // As Commands::spawn, this spawns an entity.
        // If this is undone, the entity is at first disabled and later fully despawned if the redo
        // becomes unreachable.
        commands.rev_spawn(
            not_log,
            Waste {
                row: ROW,
                tossed_at: meta.now(),
            },
        );
    }
}
