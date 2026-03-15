use bevy::{ecs::error::Result, prelude::*};
use bevy_oozlum::prelude::*;

use crate::{Waste, control::JustPressed, rows::Row};

pub fn plugin<const ROW: u64>(app: &mut App) {
    // Use rev_add_systems for reversible systems.
    app.rev_add_systems(RevUpdate, system::<ROW>.rev_in_set(Row(ROW)));
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
        RevDirection::Forward { .. } => {
            if !input.get(ROW) {
                return Ok(());
            }

            // TransitionLog and TransitionsLog need the past length to shorten the log if needed.
            // This can either be MetaPastLen from RevDirection (if it is updated exactly once every
            // time RevUpdate) or the value returned by UpdateLog::forward_past_len.
            // UpdateLog keeps track how long the past needs to be to keep it as small as possible
            // without running out-of-log when fully going backwards.
            let past_len = pressed_log.forward_past_len(&meta);

            let entity = commands.spawn(waste).id();

            // The spawned entity is logged in TransitionLog.
            spawn_log.forward_push(&meta, past_len, entity);
        }
        RevDirection::BackwardLog if pressed_log.backward_log(&meta) => {
            // At undo the spawned entity gets despawned.
            let entity = spawn_log.backward_log(&meta)?;
            commands.entity(*entity).despawn();
        }
        RevDirection::ForwardLog if pressed_log.forward_log(&meta) => {
            // At redo a new entity with the Waste component is spawned.
            // This could be problematic if you expect the original Entity ID to become valid again.
            // Because of this rev_spawn is instead disabled at undo and enabled at redo again.
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
