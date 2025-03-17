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
- track_location and bevy_reflect feature (both are not documented?), rename serde -> serialize
- deprecate Finalize
-- pub BufferEntity(u64) component
-- buffer despawn system
-- no first-party cleanup API
-- new tests
- all init_resource to plugin
- assert buffer components remain empty for component
- sync points before exclusive systems need to be reflected
- migration tracking issue
- reversible commands traits of:
-- Commands
-- EntityCommands
-- RelatedSpawnerCommands
-- EntityEntryCommands
-- ChildSpawnerCommands
- check if EntityWorldMut with relations need to handle children rev_despawn

Docs
- documentations
-- point out determinism aspects of methods
-- log contract (always valid, may go further into the past)
-- check-logged-at should not be used as the sole shortening mechanism or else logs can grow larger than desired

ISSUES/DISCUSSIONS:
- reversible change detection (copy over to new repo)
- analyze test schedule::non_exclusive_then_exclusive_ignore_deferred, consider revamping test strategy
- manual sync point configuration
-- apply_deferred
-- ScheduleBuildSettings::auto_insert_apply_deferred
- more compact FrameTransitionLog
-- VecDeque<u8> with variable len entries
-- has to provide the same api
- not supported:
-- EntityWorldMut::clone_with because EntityClonerBuilder is not offering reads on which components are cloned
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
    pub use crate::schedule::{
        forward_set, IntoRevSystemConfigs as _, IntoRevSystemSetConfigs as _, RevSchedule as _,
        RevSystemsSet, RevUpdate,
    };
    pub use crate::undo_redo::{
        unique_for_location, BuffersUndoRedo as _, DespawnAtOutOfLog, RevCommands as _,
        RevEntityWorldMut as _, RevWorld as _, UndoRedoBuffer, UndoRedoDirection, UndoRedoSwap,
    };
}
