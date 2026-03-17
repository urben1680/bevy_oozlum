use bevy::prelude::*;
use bevy_oozlum::prelude::*;

// Every row does more or less the same:
//
// RevDirection::Forward => an entity is spawned with Waste
// RevDirection::BackwardLog => the entity is either disabled or despawned
// RevDirection::ForwardLog => the entity is either enabled or respawned

mod rev_command; // utilizes reversible commands in the most simple form
mod rev_condition; // utilizes rev_run_if
mod rev_hook; // reversible commands from hooks
mod rev_logs_drain; // reversilbe logic using log types, utilizes drain to clean up
mod rev_logs_mut; // reversilbe logic using log types, utilizes mutation of log entries
mod rev_observer; // reversible commands from observers
mod rev_world; // reversible exclusive systems

pub fn plugin(app: &mut App) {
    app.add_plugins((
        rev_command::plugin::<1>,
        rev_condition::plugin::<2>,
        rev_hook::plugin::<3>,
        rev_logs_drain::plugin::<4>,
        rev_logs_mut::plugin::<5>,
        rev_observer::plugin::<6>,
        rev_world::plugin::<7>,
    ))
    // It does not really matter in which order the row systems are executed, but one can use all
    // vanilla ordering configurations on systems and sets with a rev_* prefixed variant.
    // During RevDirection::Backward, the systems are executed in reverse, effects from sync points
    // are undone before the system that originally queued them. This is only true for reversible
    // commands however.
    .rev_configure_sets(
        RevUpdate,
        (
            Row(1).rev_after(Row(2)),
            (Row(3), Row(4), Row(5)).rev_chain(),
            Row(6).rev_before_ignore_deferred(Row(7)),
        ),
    );
}

// Each row adds their system to this set to showcase reversible ordering above
#[derive(SystemSet, Copy, Clone, Debug, Hash, PartialEq, Eq)]
struct Row(u64);
