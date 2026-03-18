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
//! # return;
//!     App::new()
//!         .add_plugins((DefaultPlugins, RevPlugin::default()))
//!         .rev_add_systems(RevUpdate, rev_system)
//!         .add_systems(Update, input_system)
//!         .init_resource::<Time::<Fixed>>()
//!         .run();
//! }
//!
//! fn rev_system(meta: Res<RevMeta>, mut commands: Commands) {
//!     match meta.running_direction() {
//!         RevDirection::Forward { meta_past_len } => {
//!             println!("rev sys: hello world!");
//!
//!             commands.queue(move |world: &mut World| {
//!                 println!("rev cmd: hello world!");
//!                 
//!                 world.buffer_undo_redo(meta_past_len, |_: &mut World, direction| {
//!                     match direction {
//!                         UndoRedoDirection::Redo => println!("rev cmd: hello world! (log)"),
//!                         UndoRedoDirection::Undo => println!("rev cmd: !dlrow olleh (log)")
//!                     }
//!                 })
//!             })
//!         },
//!         RevDirection::ForwardLog => println!("rev sys: hello world! (log)"),
//!         RevDirection::BackwardLog => println!("rev sys: !dlrow olleh (log)")
//!     }
//! }
//!
//! fn input_system(
//!     keyboard_input: Res<ButtonInput<KeyCode>>,
//!     mut meta: ResMut<RevMeta>
//! ) {
//!     if keyboard_input.pressed(KeyCode::ArrowUp) {
//!         meta.set_queue(RevQueue::RunForward);
//!     } else if keyboard_input.pressed(KeyCode::ArrowDown) {
//!         meta.set_queue(RevQueue::Pause);
//!     } else if keyboard_input.pressed(KeyCode::ArrowRight) {
//!         meta.set_queue(RevQueue::RunForwardLog);
//!     } else if keyboard_input.pressed(KeyCode::ArrowLeft) {
//!         meta.set_queue(RevQueue::RunBackwardLog);
//!     }
//! }
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

Docs
- no reversible change detection (copy over to new repo)
- no manual sync point configuration
-- ScheduleBuildSettings::auto_insert_apply_deferred
- subset of EntityWorld/EntityCommands as scope limit
- make fake variadics docs work
- docs for private UndoRedo types

ISSUES/DISCUSSIONS:
- feature track_update_logs to opt-out
- no_std
- RevBundle::rev_insert_inner out of trait
- schedule::set_base_sets should not need to chain forward/backward configs

*/
// todo: deny
#![deny(rustdoc::broken_intra_doc_links)] // works only in cargo doc --no-deps
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
        BuffersUndoRedo as _, IsRevDespawned as _, RevCommands as _, RevEntityCommands as _,
        RevEntityEntryCommands as _, RevEntityWorldMut as _, RevRelatedSpawnerCommands as _,
        RevWorld as _, UndoRedo, UndoRedoDirection,
    };
}

/// Make `error!` and `error_once!` cause panics.
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
