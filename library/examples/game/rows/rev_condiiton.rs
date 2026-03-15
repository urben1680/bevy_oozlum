use bevy::prelude::*;
use bevy_oozlum::prelude::*;

use crate::{TossedAt, Waste, control::JustPressed, rows::Row};

pub fn plugin<const ROW: u64>(app: &mut App) {
    app.rev_add_systems(
        RevUpdate,
        system::<ROW>
            .rev_run_if(condition::<ROW>)
            .rev_in_set(Row::<ROW>),
    );
}

fn condition<const ROW: u64>(input: Res<JustPressed>) -> bool {
    input.get::<ROW>()
}

fn system<const ROW: u64>(meta: Res<RevMeta>, mut commands: Commands) {
    if let RevDirection::Forward { meta_past_len } = meta.running_direction() {
        commands.rev_spawn(meta_past_len, (TossedAt(meta.now()), Waste { row: ROW }));
    }
}
