use bevy::{ecs::error::Result, prelude::*};
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
    let waste = Waste {
        row: ROW,
        tossed_at: meta.now(),
    };
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
            let entity = commands.spawn(waste).id();

            // The spawned entity is logged in TransitionLog.
            spawn_log.forward_push(&meta, past_len, entity);
        }
        RevDirection::BackwardLog if pressed_log.backward_log(&meta) => {
            // At undo the spawned entity gets despawned.
            let entity = spawn_log.backward_log(&meta)?;
            commands.entity(*entity).despawn();

            // **NOTE** it may be problematic to use commands during RevDirection::BackwardLog for
            // reversible logic because this will not be applied *before* the system like a proper
            // rev_spawn used at RevDirection::NotLog.
            // This here may lead to unexpected ordering problems among other systems. Systems like
            // this should instead modify existing components or resources at most, not using
            // commands.
        }
        RevDirection::ForwardLog if pressed_log.forward_log(&meta) => {
            // At redo a new entity with the Waste component is spawned.
            // This could be problematic if you expect the original Entity ID to become valid again.
            // Because of this the rev_spawn command is instead disabling the entity at undo and
            // enabling it at redo again until it can finally be despawned.
            let entity = spawn_log.forward_log(&meta)?;
            *entity = commands.spawn(waste).id();
        }
        _ => {}
    }

    // The backward_log and forward_log methods of TransitionLog and TransitionsLog can fail if the
    // log cannot go further. This should not happen if the log is used correctly but it is still a
    // good idea to return the result instead of crashing the app with a panic.
    Ok(())
}
