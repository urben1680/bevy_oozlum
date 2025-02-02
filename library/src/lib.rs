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

Enhancements:
- reduce todo!() and //todo
- #[inline]s

Docs
- documentations
-- point out determinism aspects of methods
-- log contract (always valid, may go further into the past)
-- check-logged-at should not be used as the sole shortening mechanism or else logs can grow larger than desired

ISSUES/DISCUSSIONS:
- reversible change detection (copy over to new repo)
- analyze test schedule::non_exclusive_then_exclusive_ignore_deferred, consider revamping test strategy
- reversible entity commands, link to https://github.com/bevyengine/bevy/issues/15350 as blocker
- manual sync point configuration
-- apply_deferred
-- ScheduleBuildSettings::auto_insert_apply_deferred
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
        BuffersUndoRedo as _, FinalizeDirection, RevCommands as _, UndoRedoBuffer,
        UndoRedoDirection,
    };
}
