/*
TODO:

Features:
- entity commands, standard rev commands
-- postponed due to required components + disabled entities + moving components
- add license
- reversible versions of World::observe / App::observe
- Schedule::ignore_ambiguity

Enhancements:
- reduce todo!() and //todo
- RareStates tests
- longer log tests for RareTransition and RareState
- tests of other log methods like clear variants
- config tests
- make doctests work
- mod/use/pub cleanup, make prelude
- serde-with tests
- observer tests
- hook -> observer -> commands reversible order test
-- commands should also trigger hooks here
-- commands shouls also call observer here
-- a second sync point aftwerwards should be scheduled to assert nothing ist postponed into it
- use serde from bevy reexport
- seal log structs (pub after appearing in pub interface)
- thiserror for error types
- reflect behind feature flag
- use upcoming param verification for system (params)
- RevMetaWithVerify
-- tests
-- own submodule
- rename Direction to RevDirecton to not clash with non-crate types with that name

Docs
- examples
- documentations, besonders mit informationen welche Methoden für deterministische Logik geeignet ist

UNSUPPORTED:

- Change Detection
-- would be very complex and would rely heavily on implemention details of bevy
-- might be more easy to do if Ticks needed no wrapping, like with proposed u64 Ticks
- IntoSystemConfigs::distributive_run_if
-- incompatible with how internals work
- Schedule::set_apply_final_deferred
-- default behavior is critical for assumptions of backward schedule
- Schedule::graph_mut
-- why not?
- ScheduleBuildSettings::auto_insert_apply_deferred
-- see unsupported Schedule::set_apply_final_deferred
- Trigger::event_mut
-- mutations on the event are not reflectable in the log
- EntityCommands/EntityWorldMut
-- missing features on bevy's side
*/

use std::hash::Hash;

use bevy::{
    app::{FixedUpdate, Plugin},
    ecs::schedule::{InternedScheduleLabel, InternedSystemSet, IntoSystemConfigs, ScheduleLabel},
};

use commands::RevCommandBuffer;
use meta::RevMeta;

pub mod app;
pub mod commands;
pub mod log;
pub mod meta;
pub mod schedule;
pub mod set_configs;
pub mod system_configs;
pub mod world;

/// Contains important extension traits `as _`, [`RevMeta`] and [`Direction`].
pub mod prelude {
    pub use crate::app::RevApp as _;
    pub use crate::commands::RevCommands as _;
    pub use crate::meta::{RevMeta, Direction};
    pub use crate::set_configs::IntoRevSystemSetConfigs as _;
    pub use crate::system_configs::IntoRevSystemConfigs as _;
    pub use crate::world::RevWorld as _;
}

/// Should not be pub to not add invalid settings (see unsupported Schedule settings)
#[derive(ScheduleLabel, Copy, Clone, Debug, Hash, PartialEq, Eq)]
struct ForwardSchedule(InternedScheduleLabel);

/// Should not be pub to not add invalid settings (see unsupported Schedule settings)
#[derive(ScheduleLabel, Copy, Clone, Debug, Hash, PartialEq, Eq)]
struct BackwardSchedule(InternedScheduleLabel);

#[derive(ScheduleLabel, Copy, Clone, Debug, Hash, PartialEq, Eq)]
pub struct RevUpdate;

pub struct RevSystemsPlugin {
    pub rev_meta: Option<RevMeta>,
    pub add_rev_meta_sys_in: Option<(InternedScheduleLabel, Option<InternedSystemSet>)>,
}

impl Default for RevSystemsPlugin {
    fn default() -> Self {
        Self {
            rev_meta: Some(RevMeta::default()),
            add_rev_meta_sys_in: Some((FixedUpdate.intern(), None)),
        }
    }
}

impl Plugin for RevSystemsPlugin {
    fn build(&self, app: &mut bevy::prelude::App) {
        app.register_type::<RevMeta>()
            // needs to be manually inserted because first accessor might be a hook which cannot insert it
            .init_resource::<RevCommandBuffer>();

        if let Some(rev_meta) = &self.rev_meta {
            app.insert_resource(rev_meta.clone());
        }

        match self.add_rev_meta_sys_in {
            Some((schedule, None)) => app.add_systems(schedule, RevMeta::update_world),
            Some((schedule, Some(set))) => {
                app.add_systems(schedule, RevMeta::update_world.in_set(set))
            }
            None => app,
        };
    }
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

pub(crate) use error_per_flag;
