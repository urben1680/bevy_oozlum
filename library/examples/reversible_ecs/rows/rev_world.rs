use bevy::prelude::*;
use bevy_oozlum::prelude::*;

use crate::{TossedAt, Waste, control::JustPressed, rows::Row};

pub fn plugin<const ROW: u64>(app: &mut App) {
    // the system itself is not really reversible as it has no backward logic, besides being noop,
    // but needs to be added as such for the queued commands to be reversible
    app.rev_add_systems(RevUpdate, system::<ROW>.rev_in_set(Row::<ROW>));
}

fn system<const ROW: u64>(world: &mut World) {
    if !world.resource::<JustPressed>().get::<ROW>() {
        return;
    }
    let meta = world.resource::<RevMeta>();
    if let RevDirection::Forward { meta_past_len } = meta.running_direction() {
        world.rev_spawn(meta_past_len, (TossedAt(meta.now()), Waste { row: ROW }));
    }
}
