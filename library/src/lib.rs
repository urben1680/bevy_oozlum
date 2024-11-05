/*
TODO:

Features:
- entity commands, standard rev commands
-- postponed due to required components + disabled entities + moving components
- add license
- reversible versions of World::observe / App::observe

Enhancements:
- reduce () and //todo
- config tests
- make doctests work
- mod/use/pub cleanup
-- promote hook and observer
- use serde from bevy reexport
- reflect behind feature flag
- RevMetaWithVerify
-- tests
- how to handle set_apply_final_deferred(false)?
-- rev meta reads buffer after run too
- more plugin constructors

- InitiallyNoneStateLog / InitiallyNoneRareStateLog
-- Rare variant
-- tests, serde_with

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

use std::{fmt::Debug, hash::Hash};

use bevy::{
    ecs::{component::Tick, schedule::ScheduleLabel},
    reflect::Reflect,
};

#[cfg(feature = "serde")]
use bevy::reflect::{ReflectDeserialize, ReflectSerialize};

use log::PackedRevFrame;

pub mod app;
pub mod commands;
pub mod hook;
pub mod log;
pub mod meta;
pub mod observer;
pub mod schedule;
pub mod world;

/// Contains important extension traits `as _`, [`RevMeta`] and [`RevDirection`].
pub mod prelude {
    pub use crate::app::RevApp as _;
    pub use crate::commands::RevCommands as _;
    pub use crate::hook::HookDirection;
    pub use crate::meta::{RevDirection, RevMeta};
    pub use crate::schedule::IntoRevSystemConfigs as _;
    pub use crate::schedule::IntoRevSystemSetConfigs as _;
    pub use crate::schedule::RevSchedule as _;
    pub use crate::world::RevDeferredWorld as _;
    pub use crate::world::RevWorld as _;
}

#[derive(Clone, Copy, PartialEq, Eq, Reflect)]
#[cfg_attr(feature = "serde", reflect(Serialize, Deserialize))]
pub struct RevFrame(usize);

impl From<RevFrame> for usize {
    fn from(value: RevFrame) -> Self {
        value.0
    }
}

impl RevFrame {
    const fn new(value: usize) -> Self {
        debug_assert!(value <= PackedRevFrame::MAX_AS_USIZE);
        Self(value)
    }
    const fn wrapping_add(self, value: usize) -> Self {
        Self(self.0.wrapping_add(value) & PackedRevFrame::MAX_AS_USIZE)
    }
    const fn wrapping_sub(self, value: usize) -> Self {
        Self(self.0.wrapping_sub(value) & PackedRevFrame::MAX_AS_USIZE)
    }
}

impl Debug for RevFrame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for RevFrame {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        PackedRevFrame::from(*self).serialize(serializer)
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for RevFrame {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        PackedRevFrame::deserialize(deserializer).map(Into::into)
    }
}

#[derive(ScheduleLabel, Copy, Clone, Debug, Hash, PartialEq, Eq)]
pub struct RevUpdate;

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
