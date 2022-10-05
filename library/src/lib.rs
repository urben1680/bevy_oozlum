use std::{marker::PhantomData, num::Wrapping};

use bevy::{
    ecs::{
        query::{QueryItem, WorldQuery},
        system::SystemParam,
    },
    prelude::{Component, Entity, Query, ResMut, Without},
};

pub mod commands;
pub mod controller;
pub mod event;
//pub mod system_mutation;

pub const MAX_LOG_INDEX: Ticks = Ticks::MAX;
pub const MAX_LOG_INDEX_USIZE: usize = MAX_LOG_INDEX as usize;
pub const LOG_LEN: usize = MAX_LOG_INDEX_USIZE + 1;
pub const DEFAULT_TIME_STEP: f64 = 0.02;
pub const FORGET_SYNC_SENDER_CAPACITY: usize = 1024;
pub const DELAYED_COMMANDS_TICKS_CAPACITY: usize = Ticks::MAX as usize >> 1; //jumping from morning to evening
pub const DELAYED_COMMANDS_SYNC_SENDER_CAPACITY: usize = 1024;

/// Type that stores the ticks systems work by.
/// MAX value is also the limit how many ticks can be logged.
/// Timestamps are stored as `std::num::Wrapping<Ticks>`.
pub type Ticks = u16;

pub trait TicksRelative {
    fn ticks_ago(&self, reference: Wrapping<Ticks>) -> Ticks;
    fn ticks_from_now(&self, reference: Wrapping<Ticks>) -> Ticks;
    fn further_in_the_future(&self, other: Wrapping<Ticks>, reference: Wrapping<Ticks>) -> bool;
    fn further_in_the_past(&self, other: Wrapping<Ticks>, reference: Wrapping<Ticks>) -> bool;
    fn in_between(&self, earlier: Wrapping<Ticks>, later: Wrapping<Ticks>) -> bool;
}

impl TicksRelative for Wrapping<Ticks> {
    fn ticks_ago(&self, time_stamp: Self) -> Ticks {
        (time_stamp - self).0
    }
    fn ticks_from_now(&self, time_stamp: Self) -> Ticks {
        (self - time_stamp).0
    }
    fn further_in_the_future(&self, other: Self, reference: Self) -> bool {
        self.ticks_from_now(reference) > other.ticks_from_now(reference)
    }
    fn further_in_the_past(&self, other: Self, reference: Self) -> bool {
        self.ticks_ago(reference) > other.ticks_ago(reference)
    }
    fn in_between(&self, earlier: Wrapping<Ticks>, later: Wrapping<Ticks>) -> bool {
        later.further_in_the_future(*self, earlier)
    }
}

/// Component that should be always queried in `Query`s (instead of `Entity`).
#[derive(WorldQuery)]
pub struct PresentEntity {
    pub entity: Entity,
    filter: Without<DespawnedEntity>,
}

/// Buffer component/resource that contains despawned data so it can be recovered.
pub struct Despawned<T>(pub T);

impl<T: Component> Component for Despawned<T> {
    type Storage = <T as Component>::Storage;
}

/// Flag Component to mark an `Entity` as despawned.
#[derive(Component)]
pub struct DespawnedEntity;

/*
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        assert!(true);
    }
}
*/
mod bevy_discord {

    use bevy::{ecs::{query::{QueryItem, WorldQuery},system::SystemParam},prelude::{Component, Query, ResMut}};

#[derive(Component)]
struct Log;
struct PerSystem;
struct PerEntity<Q: WorldQuery + 'static, const BATCH_SIZE: usize = 0>(std::marker::PhantomData<Q>);

trait LogPosition {
    type Params<'w, 's>: SystemParam;
    type UserParams<'w, 'a, S: SystemParam + Send + Sync + 'a>;
    fn mutate<
        'w, 's, 'a,
        S: SystemParam + Send + Sync + 'a,
        FN: Fn(&mut Log, &mut Self::UserParams<'w, 'a, S>) + Send + Sync + Copy,
    >(params: Self::Params<'w, 's>, s: S, f: FN);
}

impl LogPosition for PerSystem {
    type Params<'w, 's> = ResMut<'w, Log>;
    type UserParams<'w, 'a, S: SystemParam + Send + Sync + 'a> = S;
    fn mutate<
        'w, 's, 'a,
        S: SystemParam + Send + Sync + 'a,
        FN: Fn(&mut Log, &mut Self::UserParams<'w, 'a, S>) + Send + Sync + Copy,
    >(mut params: Self::Params<'w, 's>, mut s: S, f: FN) {
        f(&mut params, &mut s);
    }
}

impl<Q: WorldQuery, const BATCH_SIZE: usize> LogPosition for PerEntity<Q, BATCH_SIZE> {
    type Params<'w, 's> = Query<'w, 's, (Q, &'static mut Log)>;
    type UserParams<'w, 'a, S: SystemParam + Send + Sync + 'a> = (&'a S, QueryItem<'w, Q>);
    fn mutate<
        'w, 's, 'a,
        S: SystemParam + Send + Sync + 'a,
        FN: Fn(&mut Log, &mut Self::UserParams<'w, 'a, S>) + Send + Sync + Copy,
    >(mut params: Self::Params<'w, 's>, s: S, f: FN) {
        if BATCH_SIZE == 0 {
            params.for_each_mut(|(item, mut log)| {
                f(&mut log, &mut (&s, item));
            });
        } else {
            params.par_for_each_mut(BATCH_SIZE, |(item, mut log)| {
                f(&mut log, &mut (&s, item));
            });
        }
    }
}
}
