use bevy::prelude::*;

// Every row does more or less the same:
//
// RevDirection::Forward => an entity is spawned with Waste
// RevDirection::BackwardLog => the entity is either disabled or despawned
// RevDirection::ForwardLog => the entity is either enabled or respawned

mod rev_command; // reversible command from a simple system
mod rev_config; // reversible schedule configuration
mod rev_hook; // reversible command from hook
mod rev_logs_drain; // reversible logic using log types, utilizes drain to clean up
mod rev_logs_mut; // reversible logic using log types, utilizes mutation of log entries
mod rev_observer; // reversible command from observer
mod rev_schedule; // running other schedules during RevUpdate

pub fn plugin(app: &mut App) {
    app.add_plugins((
        rev_command::plugin::<1>,
        rev_config::plugin::<2>,
        rev_hook::plugin::<3>,
        rev_logs_drain::plugin::<4>,
        rev_logs_mut::plugin::<5>,
        rev_observer::plugin::<6>,
        rev_schedule::plugin::<7>,
    ));
}
