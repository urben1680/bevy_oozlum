use bevy::prelude::*;
use bevy_oozlum::prelude::*;

use crate::{Waste, control::JustPressed};

pub fn plugin<const ROW: u64>(app: &mut App) {
    // Use rev_add_systems for reversible systems.
    app.rev_add_systems(RevUpdate, system::<ROW>);
}

fn system<const ROW: u64>(world: &mut World) {
    if !world.resource::<JustPressed>().get(ROW) {
        return;
    }
    let meta = world.resource::<RevMeta>();
    if let Some(meta_past_len) = meta.get_meta_past_len() {
        // MetaPastLen is like a token to prove that methods needing it are called during
        // RevDirection::Forward. Because of this it should not be stored past that.

        // World and EntityWorldMut has mostly all reversible functionality as Commands and
        // EntityCommands.
        // If this is undone, the entity is at first disabled and later fully despawned if the redo
        // becomes unreachable.
        world.rev_spawn(
            meta_past_len,
            Waste {
                row: ROW,
                tossed_at: meta.now(),
            },
        );
    }
}
