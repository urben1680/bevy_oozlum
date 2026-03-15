use bevy::prelude::*;
use bevy_oozlum::prelude::*;

use crate::{TossedAt, Waste, control::JustPressed, rows::Row};

pub fn plugin<const ROW: u64>(app: &mut App) {
    // the system itself is not really reversible as it has no backward logic, besides being noop,
    // but needs to be added as such for the queued commands to be reversible
    app.rev_add_systems(RevUpdate, system::<ROW>.rev_in_set(Row::<ROW>));
}

fn system<const ROW: u64>(
    input: Res<JustPressed>,
    meta: Res<RevMeta>,
    mut pressed_log: Local<UpdateLog>,
    mut spawn_log: Local<TransitionLog<Entity>>,
    mut commands: Commands,
) {
    let bundle = (TossedAt(meta.now()), Waste { row: ROW });
    match meta.running_direction() {
        RevDirection::Forward { .. } => {
            if !input.get::<ROW>() {
                return;
            }

            let past_len = pressed_log.forward_past_len(&meta);
            let entity = commands.spawn(bundle).id();
            spawn_log.forward_push(&meta, past_len, entity);
        }
        RevDirection::BackwardLog if pressed_log.backward_log(&meta) => {
            let entity = spawn_log.backward_log(&meta).unwrap();
            commands.entity(*entity).despawn();
        }
        RevDirection::ForwardLog if pressed_log.forward_log(&meta) => {
            let entity = spawn_log.forward_log(&meta).unwrap();
            *entity = commands.spawn(bundle).id();
        }
        _ => {}
    }
}
