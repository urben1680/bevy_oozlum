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
- track_location and bevy_reflect feature (both are not documented?), rename serde -> serialize

Docs
- documentations
-- point out determinism aspects of methods
-- log contract (always valid, may go further into the past)
-- check-logged-at should not be used as the sole shortening mechanism or else logs can grow larger than desired

ISSUES/DISCUSSIONS:
- reversible change detection (copy over to new repo)
- analyze test schedule::non_exclusive_then_exclusive_ignore_deferred, consider revamping test strategy
- reversible entity commands
-- main has all that is needed but might all be too fresh and up for changes before 0.16
-- approach:
-- 1. methods that are rev_ variants of bevy's commands + entity commands
--    still are vanilla Command/EntityCommand implementors
--    ideally work with fallible commands API
--    entity disabling (unspaw, undespawn) needs to be aware if entity is already disabled
--      maybe add another marker component that requires Disabled
-- 2. extend RevCommands trait, introduce RevEntityCommands
- manual sync point configuration
-- apply_deferred
-- ScheduleBuildSettings::auto_insert_apply_deferred
- more compact FrameTransitionLog
-- VecDeque<u8> with variable len entries
-- has to provide the same api
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
        BuffersUndoRedoFinalize as _, FinalizeDirection, RevBuffers, RevCommands as _, RevDisabled,
        UndoRedoDirection,
    };
}
