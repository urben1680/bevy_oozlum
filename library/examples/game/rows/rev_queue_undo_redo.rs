use bevy::prelude::*;
use bevy_oozlum::{prelude::*, undo_redo::UndoRedo};

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
        // RevDirection::NotLog. Because of this it should not be stored past that.

        // As Commands::spawn_empty, this spawns an empty entity.
        // If this is undone, the entity is at first disabled and later fully despawned if the redo
        // becomes unreachable for RevDirection::BackwardLog.
        let entity = commands.rev_spawn_empty(not_log).id();

        // There are two methods to know: queue_undo_redo and redo_and_queue.
        //
        // The first is used to define undo/redo logic to happen and store it in the system state so
        // it can queue the undo or redo logic, depending on the current RevDirection.
        //
        // The second does the same, but also immediately applies the redo logic at the next
        // sync-point. This is useful when this does the same as what we would need to do via a
        // command ourselves here. And as that is the case (we want to insert the Waste component)
        // we use redo_and_queue.
        commands.redo_and_queue(
            not_log,
            InsertRemoveWaste {
                entity,
                waste: Waste {
                    row: ROW,
                    tossed_at: meta.now(),
                },
            },
        );
    }
}

struct InsertRemoveWaste {
    entity: Entity,
    waste: Waste,
}

// Note that this is often inferior to EntityCommands.rev_insert when it comes to bundles of
// multiple components and required components. Still you may want to use custom UndoRedo
// implementations for operations this crate does not cover.
impl UndoRedo for InsertRemoveWaste {
    fn undo(&mut self, world: &mut World) {
        // RevFetch includes an additional check for the entity to not be reversibly despawned.
        // This is needed because as long the despawn can be undone, the entity is only disabled.
        //
        // In this case this should never fail, as the undo of rev_spawn_empty would come *after*
        // this code executes. Generally, RevFetch is more interesting for code outside of
        // reversible systems.
        world
            .get_entity_mut(RevFetch(self.entity))
            .unwrap()
            .remove::<Waste>();
    }
    fn redo(&mut self, world: &mut World) {
        world.entity_mut(RevFetch(self.entity)).insert(self.waste);
    }
}
