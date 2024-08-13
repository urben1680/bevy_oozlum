use std::{
    convert::Infallible,
    fmt::Debug,
    num::NonZeroUsize,
    ops::{Deref, DerefMut},
};

use bevy::ecs::{
    change_detection::{DetectChanges, DetectChangesMut},
    component::{Component, Tick},
    query::WorldQuery,
    system::ParallelCommands,
    world::Mut,
};

/*
TODO:

- generall todo!()
- value logs
- log tests
*/

/*
Ticks Umgang:

- Deques sollen mit Ticks erkennen wann sie verringert werden sollen
- Rev Schedules sollen mit World::last_change_tick_scope laufen
- Meta hat ein Tick log, in log wird die schedule dann mit jeweiligen start tick ausgelöst
- change detection:

NOT LOG:
start: Tick X
comp_a wird bei X+1 verändert
sys_a findet changed comp_a bei Tick X+2, nicht comp_b
comp_b wird bei X+3 verändert

BACKWARD LOG
comp_b wird bei X verändert, Tick wird von X+3 auf X geändert
sys_a findet comp_b  bei X+1 FEHLER

LoggedFilter hat auch ein problem wenn nach log-backward non-log forward kommt
und ein in der zukunft verändertes component als changed wahrgenommen wird

der tick wird hochgezählt, das läss sich nicht ändern

tick | event
-----+--------------------- FORWARD
 011 | sys_a ändert comp_a: 001 -> 011
 012 | sys_b finded changed comp_a weil 002 < 011 < 012
 013 | sys_c ändert comp_c: 003 -> 013
-----+--------------------- BACKWARD LOG
 014 | sys_c ändert comp_c: 013 -> 003
 015 | sys_b findet changed comp_a weil im FilterLog
 016

*/

/*

Meta Steuerung

beliebig lange loggen zu lassen ist unrealistisch

man sollte eher eine maximale Länge festlegen und möglichst viele log punkte setzen -> RareTransition


*/

//mod commands;
pub mod log;
//pub mod match_log;
//pub mod eager;
//pub mod meta_old2;
//pub mod meta_new;
pub mod meta;
//mod parallel_queue;
//pub mod plugin;
//pub mod rev;
//pub mod rev_filter;
//pub mod rev_mut;
//pub mod rev_frame_meta;
//mod reversible_configs;
//pub mod transition;
pub mod app;
pub mod commands;

#[cfg(test)]
mod test {
    use bevy::{
        app::{App, Update},
        prelude::{IntoSystemSetConfigs, Schedule, SystemSet, World},
        utils::default,
    };

    #[test]
    fn can_config_system_set() {
        use bevy::prelude::{IntoSystem, System};

        fn sys_fn() {}

        let sys = IntoSystem::into_system(sys_fn);
        let sets = sys.default_system_sets();
        assert_eq!(sets.len(), 1);
        let set = sets[0];

        let mut schedule = Schedule::default();
        schedule
            .add_systems(sys_fn)
            .configure_sets(set.ambiguous_with_all());
        let _ = schedule.initialize(&mut World::new());
    }

    #[test]
    fn set_in_self() {
        #[derive(SystemSet, Copy, Clone, Debug, Hash, PartialEq, Eq)]
        struct MySet;
        let mut schedule = Schedule::default();
        schedule.configure_sets(MySet.in_set(MySet));
        let _ = schedule.initialize(&mut World::new());
    }
}

/*
#[cfg(all(not(feature = "log_len_u16"), not(feature = "log_len_u32")))]
mod log_len {
    pub type LogLen = u8;
    //pub type NonZeroLogLen = std::num::NonZeroU8;
}

#[cfg(all(
    feature = "log_len_u16",
    not(feature = "log_len_u32"),
    not(target_pointer_width = "8")
))]
mod log_len {
    pub type LogLen = u16;
    //pub type NonZeroLogLen = std::num::NonZeroU16;
}

#[cfg(all(
    not(feature = "log_len_u16"),
    feature = "log_len_u32",
    not(target_pointer_width = "8"),
    not(target_pointer_width = "16")
))]
mod log_len {
    pub type LogLen = u32;
    //pub type NonZeroLogLen = std::num::NonZeroU32;
}

pub use log_len::LogLen;
*/

/*
RPITIT für Rev System

impl ReversibleSystem for Foo {
    fn system<Revy: RevySchedule>(self) -> impl IntoSystem<(), (), Marker> { //Marker???
        |bar: ResMut<Bar>| {
            bar.update::<Revy>()
        }
        // oder:
        fn system(bar: ResMut<Bar>) {
            bar.update::<Revy>()
        }
        system
    }
}

*/
