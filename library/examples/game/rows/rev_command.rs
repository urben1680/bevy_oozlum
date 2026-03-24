use bevy::prelude::*;
use bevy_oozlum::prelude::*;

use crate::{Waste, control::JustPressed};

pub fn plugin<const ROW: u64>(app: &mut App) {
    // Use rev_add_systems for reversible systems.
    app.rev_add_systems(RevUpdate, system::<ROW>);
}

fn system<const ROW: u64>(input: Res<JustPressed>, meta: Res<RevMeta>, mut commands: Commands) {
    if input.get(ROW)
        && let Some(not_log) = meta.get_not_log()
    {
        // NotLog is like a token to prove that methods needing it are called during
        // RevDirection::Forward. Because of this it should not be stored past that.

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
