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
- blanket impl UndoRedo
- test Finalize
- #[inline]s
- examples
-- folder next to src
--- how to have cargo check look at it?
--- add CI
-- hooks https://github.com/bevyengine/bevy/blob/main/examples/ecs/component_hooks.rs
-- observers https://github.com/bevyengine/bevy/blob/main/examples/ecs/observers.rs
-- ecs, commands https://github.com/bevyengine/bevy/blob/main/examples/ecs/ecs_guide.rs
-local log that can react on DrainPastByLoggedAt
-- deprecate event? last RevMeta at trigger + count, if count is equal, call drain_past_by_logged_at, otherwise clear log
- test cfg with forward_set
- deprecate init_none, Option::get_or_* methods are enough
- expects_forward_log/backward_log for framed logs
- idea: Arc system stores system state in resource, not Mutex
-- might be incompatible with System::update_archetype_component_access as UnsafeWorldCell may only be used to read (archetype) metadata
-- register write access of dynamic resources is impossible because methods are not pub

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

mod example {}

pub mod app;
pub mod frame;
pub mod log;
pub mod meta;
pub mod schedule;
pub mod undo_redo;

/// Contains all extension traits `as _` and common types.
pub mod prelude {
    pub use crate::app::{RevApp as _, RevSystemsPlugin};
    pub use crate::frame::{PackedRevFrame, RevFrame};
    pub use crate::meta::{DrainPastByLoggedAt, RevDirection, RevMeta};
    pub use crate::schedule::{
        BackwardNoop as _, IntoRevSystemConfigs as _, IntoRevSystemSetConfigs as _,
        RevSchedule as _, RevSystemsSet, RevUpdate,
    };
    pub use crate::undo_redo::{BuffersUndoRedo as _, RevCommands as _, UndoRedoDirection};
}

/// Assumes cut-off bytes, if any, are `0`.
#[inline(always)]
fn resize_ne_bytes<const N: usize, const M: usize>(arr: [u8; N]) -> [u8; M] {
    let min = N.min(M);
    let mut result = [0; M];
    let (source, target);
    if cfg!(target_endian = "little") {
        source = &arr[..min];
        target = &mut result[..min];
    } else {
        source = &arr[N - min..];
        target = &mut result[M - min..];
    };
    target.copy_from_slice(source);
    result
}

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
