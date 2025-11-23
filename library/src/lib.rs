/*
TODO:

- schedule/test
-- test not only multi-thread executor
- update README
- PastLenLog -> UpdateLog, rename push methods
- StrLog
-- https://github.com/rust-lang/rust/issues/133253
- retest drain_past logic without +1 special case
-- With UpdateLog it cannot be guaranteed that the drained items were pushed at an out-of-log frame
- nach infallible non-log mutations, error enums überprüfen ob es obsolete varianten gibt

log Idee:
- Nothing works everywhere
- RemoveLog only works at non-log
- RemoveFuture works at non-log and backward log
-- backward lock could postpone it with another usize where it behaves like this is the log end
-- that way only non-log needs a draining API
-- .forward_log(meta)
-- .backward_log(meta)
-- TransitionLog
--- .push(meta, max_past_len, transition)
--- .push(meta, max_past_len, |_log| transition)
-- TransitionsLog
--- .push(meta, max_past_len, |_log| update)
- wenn bei TransitionsLog TransitionsLogMut und LogDrains kombiniert werden, braucht es keine neue API
- wenn bei TransitionLog TransitionLogMut und LogDrains erfunden und kombiniert werden
  und transition T beide als valide parameter angenommen werden, braucht es keine neue API
- UpdateLog integriert einfach pre_update in die anderen Methoden

Enhancements:
- reduce todo!() and //todo and unwrap (in favor of expect)
- #[inline]s
- despawn_single -> despawn
- missing apis:
-- EntityWorldMut::clone_with
-- EntityWorldMut::insert_reflect
-- EntityWorldMut::insert_reflect_with_registry
-- EntityWorldMut::insert_with_relationship_hook_mode
-- EntityWorldMut::remove_reflect
-- EntityWorldMut::remove_reflect_with_registry
-- ... (check Commands + friends)

Docs
- make fake variadics docs work
- check with optional features off that these still show up in docs
- documentations
-- point out determinism aspects of methods
-- docs for private UndoRedo types
-- point out additional conditions to not panic/return err and how some are only needed in observers/hooks
-- remind in Apis with HasEffect bound to use App::register_non_entity_buffer

ISSUES/DISCUSSIONS:
- reversible change detection (copy over to new repo)
- manual sync point configuration
-- ScheduleBuildSettings::auto_insert_apply_deferred
- not supported:
-- EntityWorldMut::clone_with because EntityClonerBuilder is not offering reads on which components are cloned
--- could be supported with RevEntityClonerBuilder
- RevBundleEffect
- RevRelationship
-- support via Observers:
--- if added during NOT_LOG to a buffer entity, Relationship uses non-buffer-entity UndoRedo
- rev_insert_batch
-- backup components one by one
-- insert closure for each is noop
*/

// todo: deny
#![deny(broken_intra_doc_links)] // works only in cargo doc --no-deps
#![warn(missing_docs)]

#[cfg(feature = "bevy_app")]
pub mod app;
pub mod log;
pub mod meta;
pub mod schedule;
pub mod undo_redo;

/// Contains common types and all extension traits `as _`.
pub mod prelude {
    #[cfg(feature = "bevy_app")]
    pub use crate::app::{RevApp as _, RevPlugin};
    pub use crate::log::{TransitionLog, TransitionsLog, UpdateLog};
    pub use crate::meta::{RevDirection, RevMeta, RevQueue};
    pub use crate::schedule::{
        IntoRevScheduleConfigs as _, RevSchedule as _, RevSystems, RevUpdate,
    };
    pub use crate::undo_redo::{
        BuffersUndoRedo as _, RevCommands as _, RevComponentEntry, RevDeferredWorld as _,
        RevEntityCommands as _, RevEntityWorldMut as _, RevIsDespawned as _, RevIsDespawned as _,
        RevOp, RevWorld as _, UndoRedo, UndoRedoDirection,
    };
}

/// Make `error!` and `error_once!` cause panics.
// This exists in the reversible_ecs example too, keep that in sync to this.
#[cfg(test)]
fn panic_on_error_events() {
    use bevy_log::{
        Level,
        tracing::{Event, Subscriber, dispatcher::get_default},
        tracing_subscriber::{
            Layer,
            layer::{Context, SubscriberExt},
            registry,
            util::SubscriberInitExt,
        },
    };

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
