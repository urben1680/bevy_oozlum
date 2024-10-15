/*
TODO:

Features:
- entity commands, standard rev commands
-- postponed due to required components + disabled entities + moving components
- add license
- reversible versions of World::observe / App::observe

Enhancements:
- reduce todo!() and //todo
- RareStates tests
- longer log tests for RareTransition and RareState
- tests of other log methods like clear variants
- config tests
- make doctests work
- mod/use/pub cleanup
- serde-with tests
- observer tests
- commands -> hook -> observer -> reversible order test
-- a second sync point aftwerwards should be scheduled to assert nothing ist postponed into it
-- test uses masks for systems that are const generic
- use serde from bevy reexport
- seal log structs (pub after appearing in pub interface)
- reflect behind feature flag
- use upcoming param verification for system (params)
- RevMetaWithVerify
-- tests
- LoggedAt module
- Andere Observer Strategie:
-- Wrapping Logik
--- die Timestamps werden nicht reduziert, nur bei jeden bevorstehenden overflow
    ruft der observer drain_past_by_timestamp auf damit ältere logs nicht wieder im
    log range auftauchen
--- ermöglicht den ganzen PackedTime range zu nutzen ohne jeden tick reduzieren zu müssen
---- stimmt nicht, wenn end_inclusive+1==start ist, muss weiterhin ticklich reduziert werden
----- neue strategie: log.drain_past_by_logged_at achtet auch auf now, wenn das kleiner als
----- das ende vom log ist, aber größer als der anfang, wird das ganze log geleert
----- funktioniert das denn? start kann 1 sein, end max-1, wenn das event nur bei 0 gefeuert
----- wird, passiert hier nichts
----- alternative: log range ist capped by PackedTime::MAX / 2
--- verringert API, macht logs auch ohne RevMeta stabiler
- forward/backward keine schedules sondern system sets mit run_if, benötigt dann kein RevSchedule

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
    ecs::{
        component::Tick,
        schedule::{
            InternedScheduleLabel, InternedSystemSet, IntoSystemConfigs, ScheduleLabel, SystemSet,
        },
    },
    reflect::Reflect,
};

use commands::RevCommandBuffer;
use log::PackedRevFrame;
use meta::RevMeta;

pub mod app;
pub mod commands;
pub mod log;
pub mod meta;
pub mod schedule;
pub mod set_configs;
pub mod system_configs;
pub mod world;

/// Contains important extension traits `as _`, [`RevMeta`] and [`RevDirection`].
pub mod prelude {
    pub use crate::app::RevApp as _;
    pub use crate::commands::RevCommands as _;
    pub use crate::meta::{RevDirection, RevMeta};
    pub use crate::set_configs::IntoRevSystemSetConfigs as _;
    pub use crate::system_configs::IntoRevSystemConfigs as _;
    pub use crate::world::RevWorld as _;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Reflect)]
pub struct RevFrame(usize);

impl Into<usize> for RevFrame {
    fn into(self) -> usize {
        self.0
    }
}

impl RevFrame {
    const fn new(value: usize) -> Self {
        debug_assert!(value <= PackedRevFrame::MAX_USIZE);
        Self(value)
    }
    const fn wrapping_add(self, value: usize) -> Self {
        let mut value = self.0.wrapping_add(value);
        if value > PackedRevFrame::MAX_USIZE {
            value -= PackedRevFrame::MAX_USIZE;
        }
        Self(value)
    }
    const fn wrapping_sub(self, value: usize) -> Self {
        let mut value = self.0.wrapping_sub(value);
        if value > PackedRevFrame::MAX_USIZE {
            value -= PackedRevFrame::MAX_USIZE;
        }
        Self(value)
    }
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

#[derive(SystemSet, Copy, Clone, Debug, Hash, PartialEq, Eq)]
struct BackwardCmdsSys(InternedSystemSet);

#[derive(SystemSet, Copy, Clone, Debug, Hash, PartialEq, Eq)]
struct BackwardSys(InternedSystemSet);

/// reference: Tick::check_tick
fn check_tick(this: &mut Tick, change_tick: Tick) {
    let age = change_tick.get().wrapping_sub(this.get());
    if age > Tick::MAX.get() {
        *this = Tick::new(change_tick.get().wrapping_sub(Tick::MAX.get()));
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

use error_per_flag;
