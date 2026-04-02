//! `bevy_oozlum` is a crate for [`bevy`] to write reversible systems and schedules.
//! It may be useful to implement rewind features in a game that run as smoothly as the normal
//! gameplay.
//!
//! "Oozlum" is a mythical bird that is able to fly backwards.
//!
//! **This crate is experimential and may be discontinued at any time.**
//!
//! ## Features
//!
//! This crate offers additional APIs for reversible schedules and reversible (entity) commands.
//!
//! Systems in these schedules run as usual, but also when undoing and redoing their effect. When
//! undoing them, the systems in the schedule are run in reverse order. Reversible commands are also
//! undone in reverse order before the system that originally queued them will run:
//!
//! - **forward**: (re)do system1 -> (re)do system1 commands -> (re)do system2
//! - **backward**: undo system2 -> undo system1 commands -> undo system1
//!
//! Since systems need to be designed with that in mind, using existing systems like from other
//! crates is most likely not enough. The same is true with commands and schedules that use bevy's
//! vanilla API.
//!
//! One exception are **run conditions**, those will work generally and can be written as usual.
//! Though an exception to the exception are conditions based on change ticks. These are
//! unsupported, see _Limitations_ below.
//!
//! **Hooks and observers** may or may not additionally run at log directions depending on how they
//! are triggered. Still they can be written to issue reversible commands which will work just fine.
//!
//! ## Examples
//!
//! A system that only has logic when running normally, so not backward or forward in log, can use
//! the [`NotLog`] system param. It skips the system in log phases and is needed for the reversible
//! command API that is used for this system's undo/redo logic.
//!
//! ```
//! # use bevy::prelude::*;
//! # use bevy_oozlum::prelude::*;
//! fn rev_system_1(not_log: NotLog, mut commands: Commands) {
//!     println!("hello world!");
//!     commands.queue_undo_redo(not_log, |_: &mut World, direction| {
//!         match direction {
//!             UndoRedoDirection::Undo => println!("!dlrow olleh (log)"),
//!             UndoRedoDirection::Redo => println!("hello world! (log)"),
//!         }
//!     });
//! }
//! ```
//!
//! If the system itself should handle the undo/redo logic directly and not in a command, this can
//! be done with the [`RevMeta`] resource to check in which direction the schedule is running.
//!
//! ```
//! # use bevy::prelude::*;
//! # use bevy_oozlum::prelude::*;
//! fn rev_system_2(meta: Res<RevMeta>) {
//!     match meta.running_direction() {
//!         RevDirection::NotLog(_) => println!("hello world!"),
//!         RevDirection::BackwardLog => println!("!dlrow olleh (log)"),
//!         RevDirection::ForwardLog => println!("hello world! (log)")
//!     }
//! }
//! ```
//!
//! If there is the need to maintain logs of changes inside the system, check the [`log`] module.
//!
//! Both system variants can be combined, [`RevDirection::NotLog`] contains the [`NotLog`] value
//! needed for reversible commands.
//!
//! Reversible systems are added to the app using [`rev_add_systems`]. Defining reversible orderings
//! and set configurations is also supported. All new APIs have a `rev_` prefix and otherwise mimic
//! the known methods of bevy.
//!
//! The main schedule for reversible systems is [`RevUpdate`]. One can populate other schedules with
//! reversible systems as well and call them via [`rev_run_schedule`] but should never mix
//! reversible and conventional systems in the same schedule.
//!
//! ```
//! # use bevy::prelude::*;
//! # use bevy_oozlum::prelude::*;
//! # fn rev_system_1() {}
//! # fn rev_system_2() {}
//! # let mut app = App::new();
//! app.rev_add_systems(
//!     RevUpdate,
//!     (rev_system_1, rev_system_2).rev_chain()
//! );
//! ```
//!
//! To revert or advance the world in the log of already ran frames, or in other words, to set the
//! [`RevDirection`] as seen in `rev_system_2`, the [`RevMeta`] resource is offering a [`set_queue`]
//! method.
//!
//! ```
//! # use bevy::prelude::*;
//! # use bevy_oozlum::prelude::*;
//! fn input_system(
//!     keyboard_input: Res<ButtonInput<KeyCode>>,
//!     mut meta: ResMut<RevMeta>
//! ) {
//!     if keyboard_input.pressed(KeyCode::ArrowUp) {
//!         meta.set_queue(RevQueue::RunForward);
//!         println!("queue forward, truncates past frames that get too old and all future frames");
//!     } else if keyboard_input.pressed(KeyCode::ArrowDown) {
//!         meta.set_queue(RevQueue::Pause);
//!         println!("queue pause, will not run RevUpdate until unpaused");
//!     } else if keyboard_input.pressed(KeyCode::ArrowLeft) {
//!         meta.set_queue(RevQueue::RunBackwardLog);
//!         println!("queue backward log, reverts logged frames, pauses at past end");
//!     } else if keyboard_input.pressed(KeyCode::ArrowRight) {
//!         meta.set_queue(RevQueue::RunForwardLog);
//!         println!("queue forward log, advances logged frames, pauses at future end");
//!     }
//! }
//! ```
//!
//! If one wants to jump to a specific frame in the log, the system also has to check [`now`] to
//! queue [`Pause`] when desired. These frames are enumerated in `u64`.
//!
//! ## Setup
//!
//! The [`RevPlugin`] plugin is used to set everything up:
//!
//! ```
//! # use bevy::prelude::*;
//! # use bevy_oozlum::prelude::*;
//! # let mut app = App::new();
//! app.add_plugins(
//!     RevPlugin.set_max_past_len(42)
//! )
//!
//! ```
//!
//! The plugin does the following things:
//!
//! 1. **Construct and insert [`RevMeta`]**, by default unpaused with keeping the the log for one
//!    frame. Both can be adjusted at the plugin.
//! 2. **Adds the [`run_rev_update`] system** which runs [`RevUpdate`], by default in
//!    [`FixedUpdate`]. A different schedule and optionally a system set can be specified at the
//!    plugin.
//! 3. **Register [`RevDespawned`] as a disabling component**, this is needed for reversibly
//!    (de)spawning entities which are only disabled at first. See the [`undo_redo`] module
//!    documentation for more information.
//!
//! One usually at least want to specify the maximum past length. The insertion of `RevMeta` and
//! `run_rev_update` can be suppressed entirely at the plugin as well for more custom setups.
//!
//! ## Cargo Features
//!
//! - `bevy_app`: default feature that includes the [`app`] module
//! - `bevy_reflect`: default feature that derives [`Reflect`] on components and resources
//! - `hotpatching`: Makes this crate compile while using hotpatching, not a default feature
//!
//! `std` is not used in this crate so it is `no_std` compatible, to the extend of bevy's support.
//!
//! ## Limitations
//!
//! Not everything one can do in bevy is also possible in a reversible manner with this crate. In
//! the following is a not exhaustive list of such limitations.
//!
//! - Attempting to use **change detection** in queries, resources, run conditions or other APIs
//!   that expose or work with [`Tick`]s will not work here. The mechanism behind them will be
//!   unable to differentiate between changes at non-log and log phases. Because of this it would
//!   not behave determistically.
//! - As supporting reversible **exclusive systems** would come with some footguns that are hard to
//!   detect and prevent, they are not supported and will cause panics.
//! - Reversible (entity) commands lack some methods that are available in vanilla bevy, most
//!   prominently those based on **dynamic components** or **entity cloning**. Supporting them is
//!   past the scope of this crate.
//! - Reversible commands working with **[relationships]** are generally available. If custom types
//!   are used that also contain other data next to the entity collections however, some APIs in
//!   this crate will not compile in the best case or will silently make that data unrecoverable at
//!   the worst case. This has to do with the lack of untyped API support as pointed out above.
//! - The behavior of reversible sync points is tightly embedded in this crate. Never build
//!   reversible schedules with **[`ScheduleBuildSettings::auto_insert_apply_deferred`] set to
//!   `false`**. Suppress them individually when configuring systems and sets.
//! - The `hotpatching` feature enables **hotpatching reversible systems**, but this will not be
//!   reversible automatically. One either has to manually patch to the previous/next fn pointer
//!   when undoing/redoing the frame the patch happened or clear the log while patching.
//!
//! [`bevy`]: https://bevy.org/
//! [`NotLog`]: crate::meta::NotLog
//! [`RevMeta`]: crate::meta::RevMeta
//! [`RevDirection::NotLog`]: crate::meta::RevDirection::NotLog
//! [`rev_add_systems`]: crate::app::RevApp::rev_add_systems
//! [`RevUpdate`]: crate::schedule::RevUpdate
//! [`rev_run_schedule`]: crate::undo_redo::commands::RevCommands::rev_run_schedule
//! [`RevDirection`]: crate::meta::RevDirection
//! [`set_queue`]: crate::meta::RevMeta::set_queue
//! [`now`]: crate::meta::RevMeta::now
//! [`Pause`]: crate::meta::RevQueue::Pause
//! [`RevPlugin`]: crate::app::RevPlugin
//! [`set_max_past_len`]: crate::meta::RevMeta::set_max_past_len
//! [`run_rev_update`]: crate::schedule::run_rev_update
//! [`FixedUpdate`]: bevy_app::FixedUpdate
//! [`RevDespawned`]: crate::undo_redo::RevDespawned
//! [not show up in queries]: bevy_ecs::entity_disabling
//! [reversibly despawned]: crate::undo_redo::commands::RevCommands::rev_despawn
//! [`RevFetch`]: crate::undo_redo::RevFetch
//! [`is_rev_despawned`]: crate::undo_redo::IsRevDespawned::is_rev_despawned
//! [`Reflect`]: bevy_reflect::Reflect
//! [`Tick`]: bevy_ecs::change_detection::Tick
//! [relationships]: bevy_ecs::relationship
//! [`ScheduleBuildSettings::auto_insert_apply_deferred`]: bevy_ecs::schedule::ScheduleBuildSettings::auto_insert_apply_deferred

#![no_std]
#![allow(internal_features)]
#![cfg_attr(any(docsrs, docsrs_dep), feature(rustdoc_internals))]

extern crate alloc;
/*
ISSUES/DISCUSSIONS:
- feature track_update_logs to opt-out
- crate::schedule::set_base_sets should not need to chain forward/backward configs
- benchmarks
- set MaybeLocation of component meta, https://github.com/bevyengine/bevy/issues/20494
*/

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
    pub use crate::meta::{NotLog, RevDirection, RevMeta, RevQueue};
    pub use crate::schedule::{
        IntoRevScheduleConfigs as _, RevSchedule as _, RevSystems, RevUpdate,
    };
    pub use crate::undo_redo::{
        IsRevDespawned as _, RevFetch, UndoRedoDirection,
        commands::RevCommands as _,
        entity_commands::{
            RevEntityCommands as _, RevEntityEntryCommands as _, RevRelatedSpawnerCommands as _,
        },
    };
}

/// Make `error!` and `error_once!` cause panics.
#[cfg(any(test, feature = "ci-mode"))]
#[doc(hidden)]
pub fn panic_on_error_events() {
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
