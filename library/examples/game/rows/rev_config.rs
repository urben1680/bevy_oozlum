use bevy::prelude::*;
use bevy_oozlum::prelude::*;

use crate::{Waste, control::JustPressed};

pub fn plugin<const ROW: u64>(app: &mut App) {
    // Use rev_add_systems for reversible systems.
    app.rev_add_systems(
        RevUpdate,
        (system1, system2::<ROW>)
            // During RevDirection::Backward, the systems are executed in reverse, effects from sync
            // points are undone before the system that originally queued them runs. This is only
            // true for reversible commands however.
            .rev_chain()
            // Use rev_in_set to add reversible systems to a set you intend to configure reversibly
            // as well.
            .rev_in_set(RowSystems),
    )
    .rev_configure_sets(
        RevUpdate,
        RowSystems
            // Use rev_run_if to make any condition reversible.
            // It will executed during RevDirection::Forward and the output is logged and used at
            // any other RevDirection.
            .rev_run_if(condition::<ROW>),
    );
}

#[derive(SystemSet, Debug, Copy, Clone, Hash, PartialEq, Eq)]
struct RowSystems;

// Note how the condition has no concept of reversible logic, it is one of the few things that do
// not need to be designed specifically for reversible systems.
fn condition<const ROW: u64>(input: Res<JustPressed>) -> bool {
    input.get(ROW)
}

#[derive(Resource, PartialEq, Debug)]
enum Signal {
    DoSpawn { at: u64 },
    DidSpawn { at: u64 },
}

fn system1(meta: Res<RevMeta>, signal: Option<Res<Signal>>, mut commands: Commands) {
    match meta.running_direction() {
        RevDirection::Forward { .. } => {
            commands.insert_resource(Signal::DoSpawn { at: meta.now() });
        }
        RevDirection::BackwardLog => {
            // Because the systems only run when you pressed the row number, that is also true
            // backwards: it only runs when that specific frame is undone.

            // When Meta::now at forward is n, it is n-1 when the frame n is undone. To compensate
            // this, we add 1 back to the value here, because that is the frame system1 spawned the
            // entity at.
            assert_eq!(*signal.unwrap(), Signal::DidSpawn { at: meta.now() + 1 });
        }
        _ => {}
    }
}

fn system2<const ROW: u64>(meta: Res<RevMeta>, mut signal: ResMut<Signal>, mut commands: Commands) {
    match meta.running_direction() {
        RevDirection::Forward { meta_past_len } => {
            assert_eq!(*signal, Signal::DoSpawn { at: meta.now() });

            // MetaPastLen is like a token to prove that methods needing it are called during
            // RevDirection::Forward. Because of this it should not be stored past that.

            // As Commands::spawn, this spawns an entity.
            // If this is undone, the entity is at first disabled and later fully despawned if the redo
            // becomes unreachable.
            commands.rev_spawn(
                meta_past_len,
                Waste {
                    row: ROW,
                    tossed_at: meta.now(),
                },
            );
        }
        RevDirection::BackwardLog => {
            *signal = Signal::DidSpawn { at: meta.now() + 1 };
        }
        _ => {}
    }
}
