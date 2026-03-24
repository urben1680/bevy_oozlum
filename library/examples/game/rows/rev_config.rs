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
            // It will executed during RevDirection::NotLog and the output is logged and used at
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
        RevDirection::NotLog(_) => {
            commands.insert_resource(Signal::DoSpawn { at: meta.now() });
        }
        RevDirection::BackwardLog => {
            // Because the systems only run when you pressed the row number, that is also true
            // backwards: it only runs when that specific frame is undone.

            assert_eq!(*signal.unwrap(), Signal::DidSpawn { at: meta.now() });
        }
        _ => {}
    }
}

fn system2<const ROW: u64>(meta: Res<RevMeta>, mut signal: ResMut<Signal>, mut commands: Commands) {
    match meta.running_direction() {
        RevDirection::NotLog(not_log) => {
            assert_eq!(*signal, Signal::DoSpawn { at: meta.now() });

            // NotLog is like a token to prove that methods needing it are called during
            // RevDirection::NotLog. Because of this it should not be stored past that.

            // As Commands::spawn, this spawns an entity.
            // If this is undone, the entity is at first disabled and later fully despawned if the redo
            // becomes unreachable.
            commands.rev_spawn(
                not_log,
                Waste {
                    row: ROW,
                    tossed_at: meta.now(),
                },
            );
        }
        RevDirection::BackwardLog => {
            *signal = Signal::DidSpawn { at: meta.now() };
        }
        _ => {}
    }
}
