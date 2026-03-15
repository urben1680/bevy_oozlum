use bevy::prelude::*;
use bevy_oozlum::prelude::*;

mod rev_command;
mod rev_condiiton;
mod rev_hook;
mod rev_logs_drain;
mod rev_logs_overwrite;
mod rev_observer;
mod rev_world;

pub fn plugin(app: &mut App) {
    app.add_plugins((
        rev_command::plugin::<1>,
        rev_condiiton::plugin::<2>,
        rev_hook::plugin::<3>,
        rev_logs_drain::plugin::<4>,
        rev_logs_overwrite::plugin::<5>,
        rev_observer::plugin::<6>,
        rev_world::plugin::<7>,
    ))
    .rev_configure_sets(
        RevUpdate,
        (
            Row::<1>.rev_after(Row::<2>),
            (Row::<3>, Row::<4>, Row::<5>).rev_chain(),
            Row::<6>.rev_before_ignore_deferred(Row::<7>),
        ),
    );
}

#[derive(SystemSet, Copy, Clone, Debug, Hash, PartialEq, Eq)]
struct Row<const ROW: u64>;
