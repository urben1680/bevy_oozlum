use bevy::{
    ecs::{lifecycle::HookContext, world::DeferredWorld},
    prelude::*,
};
use bevy_oozlum::prelude::*;

use crate::{Waste, control::JustPressed};

pub fn plugin<const ROW: u64>(app: &mut App) {
    // Use rev_add_systems for reversible systems.
    app.rev_add_systems(RevUpdate, system::<ROW>)
        .world_mut()
        .register_component_hooks::<Waste>()
        .on_insert(on_insert::<ROW>);
}

fn system<const ROW: u64>(input: Res<JustPressed>, meta: Res<RevMeta>, mut commands: Commands) {
    if input.get(ROW) && meta.running_direction().is_not_log() {
        // Note that this is no rev_spawn but a regular spawn
        commands.spawn(Waste {
            row: ROW,
            tossed_at: meta.now(),
        });
    }
}

fn on_insert<const ROW: u64>(mut world: DeferredWorld, context: HookContext) {
    let meta = world.resource::<RevMeta>();
    let Some(meta_past_len) = meta.get_meta_past_len() else {
        return;
    };

    // MetaPastLen is like a token to prove that methods needing it are called during
    // RevDirection::Forward. Because of this it should not be stored past that.

    let waste = *world.get::<Waste>(context.entity).unwrap();
    if waste.row != ROW {
        // Every row spawns with Waste so we filter for the specific row here
        return;
    }

    // Reversible logic is set here.
    // rev_mark_spawned can be used if the actual spawn is not in your source and you need to make
    // the spawn reversible. The boolean argument defines if children without linked spawn should
    // also be included in the reversible spawn. That may be the case if all of them were spawned
    // along it.
    world
        .commands()
        .rev_mark_spawned(meta_past_len, context.entity, false);
}
