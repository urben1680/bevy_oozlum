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
- test Finalize
- #[inline]s
- schedule tests
-- macro based
-- forward_set

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
