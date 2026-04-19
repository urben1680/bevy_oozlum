use bevy::{
    ecs::{entity_disabling::Disabled, error::Result},
    prelude::*,
};
use bevy_oozlum::prelude::*;

use crate::{Waste, control::JustPressed};

pub fn plugin<const ROW: u64>(app: &mut App) {
    // Use rev_add_systems for reversible systems.
    app.rev_add_systems(RevUpdate, system::<ROW>);
}

fn system<const ROW: u64>(
    input: Res<JustPressed>,
    meta: Res<RevMeta>,
    mut pressed_log: Local<UpdateLog>,
    mut spawn_log: Local<TransitionLog<Entity>>,
    mut commands: Commands,
) -> Result {
    match meta.running_direction() {
        RevDirection::NotLog(_) => {
            if !input.get(ROW) {
                return Ok(());
            }

            // TransitionLog and TransitionsLog need the past length to shorten the log if needed.
            // This can either be NotLog from RevDirection (if the log is updated exactly once every
            // time RevUpdate runs) or the value returned by UpdateLog::forward_past_len.
            // UpdateLog keeps track how long the past needs to be to keep it as small as possible
            // without running out-of-log when fully going backwards.
            let past_len = pressed_log.forward_past_len(&meta);

            // Note that this is no rev_spawn but a regular spawn, we want to handle this manually.
            let entity = commands
                .spawn(Waste {
                    row: ROW,
                    tossed_at: meta.now(),
                })
                .id();

            // The spawned entity is logged in TransitionLog. With the push the log also returns
            // draining iterators of the entities that got out of log now. This is the case for
            // spawns that are further in the past than past_len or are in a future segment that is
            // now overwritten.
            // The first is handled by the despawn_lost_waste system in main.rs but the latter
            // must be handled here. These entities get despawned.
            let mut drains = spawn_log.forward_push(&meta, past_len, entity);
            for entity in drains.future() {
                commands.entity(entity).despawn();
            }
        }
        RevDirection::BackwardLog if pressed_log.backward_log(&meta) => {
            // At undo the spawned entity gets disabled so it does not get rendered.
            // The more common rev_spawn does it the same way, though with the RevDespawned
            // component to not collide with other code that uses Disabled.
            let entity = spawn_log.backward_log(&meta)?;
            commands.entity(*entity).insert(Disabled);

            // **NOTE** it may be problematic to use commands during RevDirection::BackwardLog for
            // reversible logic because this will not be applied *before* the system like a proper
            // rev_spawn used at RevDirection::NotLog.
            // This here may lead to unexpected ordering problems among other systems. Systems like
            // this should instead modify existing components or resources at most, not using
            // commands.
        }
        RevDirection::ForwardLog if pressed_log.forward_log(&meta) => {
            // At redo the entity gets enabled again.
            let entity = spawn_log.forward_log(&meta)?;
            commands.entity(*entity).remove::<Disabled>();
        }
        _ => {}
    }

    // The backward_log and forward_log methods of TransitionLog and TransitionsLog can fail if the
    // log cannot go further. This should not happen if the log is used correctly but it is still a
    // good idea to return the result instead of crashing the app with a panic.
    Ok(())
}
