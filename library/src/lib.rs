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
- transition log error tests

Enhancements:
- reduce todo!() and //todo and unwrap (in favor of expect)
- #[inline]s
- track_location and bevy_reflect feature (both are not documented?), rename feature serde -> serialize
- delete unused types
- integrate BuffersUndoRedo in new Rev wrappers, find good way to support DeferredWorld
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
-- log contract (always valid, may go further into the past)
-- check-logged-at should not be used as the sole shortening mechanism or else logs can grow larger than desired
-- docs for private UndoRedo types
-- point out additional conditions to not panic/return err and how some are only needed in observers/hooks
-- remind in Apis with HasEffect bound to use App::register_non_entity_buffer

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
- RevBundleEffect
- RevRelationship
-- support via Observers:
--- if added during NOT_LOG to a buffer entity, Relationship uses non-buffer-entity UndoRedo
- rev_insert_batch
-- backup components one by one
-- insert closure for each is noop
- batch insert

example broken, first write tests for spawn_despawn
*/

pub mod app;
pub mod log;
pub mod meta;
pub mod schedule;
pub mod undo_redo;

/// Contains all extension traits `as _` and common types.
pub mod prelude {
    pub use crate::app::{RevApp as _, RevPlugin};
    pub use crate::meta::{RevDirection, RevMeta};
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
#[cfg(test)]
fn panic_on_error_events() {
    use bevy::log::{
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
