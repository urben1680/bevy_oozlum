/*
TODO:

- add license
- find new name
-- revy
-- brevy
-- bevyveb
-- bevy_yveb
-- bevy_revsys
-- bevy_revsched
-- bevy_smetsys

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

#[cfg(test)]
mod test {
    use bevy::{
        log::{
            tracing_subscriber::{
                layer::{Context, SubscriberExt},
                registry,
                util::SubscriberInitExt,
                Layer,
            },
            Level,
        },
        utils::tracing::{dispatcher::get_default, Event, Subscriber},
    };

    /// Make `error!` and `error_once!` cause panics.
    pub(crate) fn panic_on_error_events() {
        struct PanicOnError;
        impl<S: Subscriber> Layer<S> for PanicOnError {
            fn on_event(&self, event: &Event, _ctx: Context<S>) {
                if *event.metadata().level() == Level::ERROR {
                    panic!("{event:#?}")
                }
            }
        }
        if registry().with(PanicOnError).try_init().is_err() {
            get_default(|subscriber| {
                assert!(subscriber.downcast_ref::<PanicOnError>().is_some());
            })
        }
    }
}
