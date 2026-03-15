use bevy::prelude::*;
use bevy_oozlum::prelude::*;

use crate::{Waste, control::JustPressed, rows::Row};

pub fn plugin<const ROW: u64>(app: &mut App) {
    // Use rev_add_systems for reversible systems.
    app.rev_add_systems(RevUpdate, system::<ROW>.rev_in_set(Row(ROW)));
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
        world.rev_spawn(
            meta_past_len,
            Waste {
                row: ROW,
                tossed_at: meta.now(),
            },
        );
    }
}
