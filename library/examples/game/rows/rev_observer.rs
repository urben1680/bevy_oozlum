use bevy::prelude::*;
use bevy_oozlum::{meta::MetaPastLen, prelude::*};

use crate::{TossedAt, Waste, control::JustPressed, rows::Row};

pub fn plugin<const ROW: u64>(app: &mut App) {
    // the system itself is not really reversible as it has no backward logic, besides being noop,
    // but needs to be added as such for the queued commands to be reversible
    app.rev_add_systems(RevUpdate, system::<ROW>.rev_in_set(Row::<ROW>))
        .add_observer(observer::<ROW>);
}

#[derive(Event)]
struct WasteEvent {
    meta_past_len: MetaPastLen,
    tossed_at: TossedAt,
}

fn system<const ROW: u64>(input: Res<JustPressed>, meta: Res<RevMeta>, mut commands: Commands) {
    if input.get::<ROW>()
        && let RevDirection::Forward { meta_past_len } = meta.running_direction()
    {
        commands.trigger(WasteEvent {
            meta_past_len,
            tossed_at: TossedAt(meta.now()),
        });
    }
}

fn observer<const ROW: u64>(event: On<WasteEvent>, mut commands: Commands) {
    let WasteEvent {
        meta_past_len,
        tossed_at,
    } = *event;
    commands.rev_spawn(meta_past_len, (tossed_at, Waste { row: ROW }));
}
