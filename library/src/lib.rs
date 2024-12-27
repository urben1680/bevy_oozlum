/*
TODO:

- add license
- find new name
-- catchier and more unique to not use reversible-systems/schedules for comparable crates
-- "systems" in reverse: smetsys, can be altered to smet-sys or something
-- "schedules" in reverse: seludehcs (ew)
- should still be descriptive
-- rev..?
-- bevy_smetsys
-- bevy_yveb

Enhancements:
- reduce todo!() and //todo
- impl Error for relevant structs/enums via thiserror
- #[inline]s
- unify wording: reduce_logged_at / check_logged_at
- last_run tests
- examples
-- folder next to src
--- how to have cargo check look at it?
--- add CI
-- hooks https://github.com/bevyengine/bevy/blob/main/examples/ecs/component_hooks.rs
-- observers https://github.com/bevyengine/bevy/blob/main/examples/ecs/observers.rs
-- ecs, commands https://github.com/bevyengine/bevy/blob/main/examples/ecs/ecs_guide.rs

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
- rare_init_none
*/

use std::{fmt::Debug, hash::Hash};

use bevy::ecs::schedule::ScheduleLabel;

pub mod app;
pub mod frame;
pub mod log;
pub mod meta;
pub mod schedule;
pub mod undo_redo;

/// Contains all extension traits `as _` and common types.
pub mod prelude {
    pub use crate::app::{RevApp as _, RevSystemsPlugin};
    pub use crate::frame::{PackedRevFrame, RevFrame, RevLastRun};
    pub use crate::meta::{CheckLoggedAt, RevDirection, RevMeta};
    pub use crate::schedule::{
        IntoRevSystemConfigs as _, IntoRevSystemSetConfigs as _, RevSchedule as _,
    };
    pub use crate::undo_redo::{BuffersUndoRedo as _, RevCommands as _};
    pub use crate::RevUpdate;
}

#[derive(ScheduleLabel, Copy, Clone, Debug, Hash, PartialEq, Eq)]
pub struct RevUpdate;

macro_rules! error_per_flag {
    ($flag:expr, $($arg:tt)+) => ({
        if !*$flag {
            bevy::utils::tracing::error!($($arg)+);
            *$flag = true;
        }
        core::default::Default::default()
    });
}

use error_per_flag;
