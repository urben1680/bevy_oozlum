//! `bevy_oozlum` is a crate for [`bevy`] to write reversible systems and schedules and to control
//! them by mutating a central resource, [`RevMeta`](crate::meta::RevMeta).
//!
//! "Oozlum" is a mythical bird that is able to fly backwards.
//!
//! This crate is experimential and may be discontinued at any time.
//!
//! # Example
//!
//! ```
//! use bevy::prelude::*;
//! use bevy_oozlum::prelude::*;
//!
//! fn main() {
//!     App::new()
//!         .add_plugins((MinimalPlugins, RevPlugin::default()))
//!         .rev_add_systems(RevUpdate, hello_world_system)
//!         .init_resource::<Time::<Fixed>>()
//!         .run();
//!  }
//!  
//!  fn hello_world_system(meta: Res<RevMeta>) {
//!     if meta.running_direction().is_forward() {
//!         println!("hello world!");
//!     } else {
//!         println!("!dlrow olleh");
//!     }
//!  }
//! ```
//!
//! # Features
//!
//! This crate aims to support reversible variants of most of the `bevy` API. Most prominently:
//!
//! - Reversible system scheduling and set configurations
//! - Reversible commands
//!
//! Systems themselves however need to be actively written to have reversible logic in mind.
//! Passing existing systems from other crates to this crate's API is most likely not enough.
//!
//! Hooks and observers are in principial possible to be reversible as long they are just as systems
//! written that way. The most easy way to do that is to just queue reversible commands and let the
//! reversible sync points of the scheduling handle that. If needed,
//! [`RevOp`](crate::undo_redo::RevOp) can help to identify if and which reversible operation is
//! happening in the context of a hook or observer.
//!
//! # Limitations
//!
//! Currently some features do not have a reversible variant available.
//!
//! ## Change detection
//!
//! It is impossible to revert component or world change ticks to a previous value when undoing an
//! update. It is also insufficient to just log what component is changed at a non-log update to
//! undo/redo it at a log update. This is because the change during this log update impossible to
//! not cause change detection themselves unless very careful and isolated handling.
//!
//! This crate offers no alternative and the user needs to invent their own solution that fits their
//! use case.
//!
//! ## Relationships / Bundle Effects
//!
//! Relationships are not supported due the large API surface and current lack of dynamic inspection
//! to make any support feasable in the scope of this crate.
//!
//! ## Dynamic commands
//!
//! Inserting and removing components via reflection is not supported because they are out-of-scope.
//!
//! ## Manual sync point configuration
//!
//! Changing a schedule's `auto_insert_apply_deferred` is not compatible with reversible scheduling
//! and should not be done.
//!
//! ## Misc
//!
//! The following APIs have no reversible variants. This list may be incomplete.
//!
//! - `clone_with` (out of scope)
//! - `insert_batch` (current lack of a more optimal algorithm to inserts-in-a-loop)
//!
//! # Cargo Features
//!
//! | feature        | description                                | default feature |
//! | -------------- | ------------------------------------------ | --------------- |
//! | `bevy_app`     | `App` related features                     | yes             |
//! | `bevy_reflect` | Reflection derives on resources/components | yes             |

/*
TODO:

- update README
- https://github.com/rust-lang/rust/issues/133253

Enhancements:
- reduce todo!() and //todo
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
