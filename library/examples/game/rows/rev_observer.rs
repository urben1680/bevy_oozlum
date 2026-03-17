use bevy::prelude::*;
use bevy_oozlum::{meta::MetaPastLen, prelude::*};

use crate::{Waste, control::JustPressed};

pub fn plugin<const ROW: u64>(app: &mut App) {
    // Use rev_add_systems for reversible systems.
    app.rev_add_systems(RevUpdate, system::<ROW>)
        .add_observer(observer);
}

#[derive(Event)]
struct WasteEvent {
    meta_past_len: MetaPastLen,
    waste: Waste,
}

fn system<const ROW: u64>(input: Res<JustPressed>, meta: Res<RevMeta>, mut commands: Commands) {
    if input.get(ROW)
        && let Some(meta_past_len) = meta.get_meta_past_len()
    {
        // MetaPastLen is like a token to prove that methods needing it are called during
        // RevDirection::Forward. Because of this it should not be stored past that.

        commands.trigger(WasteEvent {
            meta_past_len,
            waste: Waste {
                row: ROW,
                tossed_at: meta.now(),
            },
        });
    }
}

fn observer(event: On<WasteEvent>, mut commands: Commands) {
    let WasteEvent {
        meta_past_len,
        waste,
    } = *event;

    // As Commands::spawn, this spawns an entity.
    // If this is undone, the entity is at first disabled and later fully despawned if the redo
    // becomes unreachable.
    commands.rev_spawn(meta_past_len, waste);
}
