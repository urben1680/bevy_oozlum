use bevy::prelude::*;
use bevy_oozlum::prelude::*;

use crate::{Waste, control::JustPressed, rows::Row};

pub fn plugin<const ROW: u64>(app: &mut App) {
    // Use rev_add_systems for reversible systems.
    app.rev_add_systems(
        RevUpdate,
        system::<ROW>
            // Use rev_run_if to make any condition reversible.
            // It will executed during RevDirection::Forward and the output is logged and used at
            // any other RevDirection.
            .rev_run_if(condition::<ROW>)
            .rev_in_set(Row(ROW)),
    );
}

// Note how the condition has no concept of reversible logic, it is one of the few things that do
// not need to be designed specifically for reversible systems.
fn condition<const ROW: u64>(input: Res<JustPressed>) -> bool {
    input.get(ROW)
}

fn system<const ROW: u64>(meta: Res<RevMeta>, mut commands: Commands) {
    if let Some(meta_past_len) = meta.get_meta_past_len() {
        // MetaPastLen is like a token to prove that methods needing it are called during
        // RevDirection::Forward. Because of this it should not be stored past that.

        // As Commands::spawn, this spawns an entity.
        // If this is undone, the entity is at first disabled and later fully despawned if the
        // redo becomes impossible.
        commands.rev_spawn(
            meta_past_len,
            Waste {
                row: ROW,
                tossed_at: meta.now(),
            },
        );
    }
}
