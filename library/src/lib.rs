/*
TODO:

- add license
-- dual license MIT/Apache-2.0 like bevy https://github.com/bevyengine/bevy/issues/2373
- find new name
-- revy
-- brevy
-- bevyveb
-- bevy_yveb
-- bevy_revsys
-- bevy_revsched
-- bevy_smetsys
-- bevy_oozlum (mythical bird that flies backwards)
- schedule/test
-- reflect on fix of https://github.com/bevyengine/bevy/issues/17828
-- test not only multi-thread executor

Enhancements:
- reduce todo!() and //todo and unwrap (in favor of expect)
- #[inline]s
- track_location and bevy_reflect feature (both are not documented?), rename feature serde -> serialize
- schedule tests with ApplyDeferred
- schedule tests with mixed chain + chain_ignore_deferred
- reversible commands traits of:
-- Commands
-- EntityCommands
-- RelatedSpawnerCommands
-- EntityEntryCommands
-- ChildSpawnerCommands

Docs
- make fake variadics docs work
- check with optional features off that these still show up in docs
- documentations
-- point out determinism aspects of methods
-- log contract (always valid, may go further into the past)
-- check-logged-at should not be used as the sole shortening mechanism or else logs can grow larger than desired

ISSUES/DISCUSSIONS:
- reversible change detection (copy over to new repo)
- manual sync point configuration
-- ScheduleBuildSettings::auto_insert_apply_deferred
- more compact FrameTransitionLog
-- VecDeque<u8> with variable len entries
-- has to provide the same api
- not supported:
-- EntityWorldMut::clone_with because EntityClonerBuilder is not offering reads on which components are cloned
--- could be supported with RevEntityClonerBuilder
*/

pub mod app;
pub mod log;
pub mod meta;
pub mod schedule;
pub mod undo_redo;

/// Contains all extension traits `as _` and common types.
pub mod prelude {
    pub use crate::app::{RevApp as _, RevSystemsPlugin};
    pub use crate::meta::{RevDirection, RevMeta};
    pub use crate::schedule::{RevSchedule as _, RevSystemsSet, RevUpdate};
    pub use crate::undo_redo::{
        unique_for_location, BuffersUndoRedo as _, RevCommands as _, RevEntityWorldMut as _,
        RevWorld as _, UndoRedoBuffer, UndoRedoDirection, UndoRedoSwap,
    };
}

#[cfg(test)]
mod test {
    use bevy::prelude::*;

    #[derive(PartialEq, Debug)]
    enum Entry {
        System(usize),
        SyncPoint(usize),
    }

    #[derive(Resource, Default)]
    struct Log(Vec<Entry>);

    fn system<const N: usize>(mut res: ResMut<Log>, mut commands: Commands) {
        res.0.push(Entry::System(N));
        commands.queue(|world: &mut World| world.resource_mut::<Log>().0.push(Entry::SyncPoint(N)));
    }

    fn generate_log(reinsert_build_settings: bool) -> Vec<Entry> {
        let mut world = World::new();
        let mut schedule = Schedule::new(Update);
        schedule.add_systems((system::<1>, system::<2>).chain_ignore_deferred());
        if reinsert_build_settings {
            let settings = schedule.get_build_settings();
            schedule.set_build_settings(settings);
        }
        schedule.initialize(&mut world).unwrap();
        world.init_resource::<Log>();
        schedule.run(&mut world);
        world.remove_resource::<Log>().unwrap().0
    }

    #[test]
    fn test() {
        let log1 = generate_log(false);
        let log2 = generate_log(true);
        assert_eq!(log1, log2);
    }
}
