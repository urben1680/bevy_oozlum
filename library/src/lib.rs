#![deny(rust_2018_idioms)]
#![feature(generic_associated_types)]

use std::{num::Wrapping, ops::RangeInclusive};

use bevy::prelude::Component;

pub mod commands;
pub mod controller;
pub mod log_systems;

pub const DEFAULT_TIME_STEP: f64 = 0.02;
pub const LOG_ONLY_PAR_ITER_BATCH_SIZE: usize = 0;

/// Type that stores the ticks systems work by.
/// MAX value is also the limit how many ticks can be logged.
/// Timestamps are stored as `std::num::Wrapping<Ticks>`.
/// Must be a smaller integer type than `usize`.
pub type Ticks = u16;

#[derive(Clone, Copy, Debug, Default)]
pub struct ToTimeStamp {
    pub to_time_stamp: Wrapping<Ticks>,
    pub delta_abs: Ticks,
}

impl ToTimeStamp {
    fn to_future(now: Wrapping<Ticks>, to: Wrapping<Ticks>) -> Self {
        Self {
            to_time_stamp: to,
            delta_abs: (to - now).0,
        }
    }
    fn to_past(now: Wrapping<Ticks>, to: Wrapping<Ticks>) -> Self {
        Self {
            to_time_stamp: to,
            delta_abs: (now - to).0,
        }
    }
}

pub trait TicksRelative {
    fn ticks_ago(&self, reference: Wrapping<Ticks>) -> Ticks;
    fn ticks_from_now(&self, reference: Wrapping<Ticks>) -> Ticks;
    fn further_in_the_future(&self, other: Wrapping<Ticks>, reference: Wrapping<Ticks>) -> bool;
    fn further_in_the_past(&self, other: Wrapping<Ticks>, reference: Wrapping<Ticks>) -> bool;
    fn in_range(&self, range: &RangeInclusive<Wrapping<Ticks>>) -> bool;
}

impl TicksRelative for Wrapping<Ticks> {
    fn ticks_ago(&self, time_stamp: Self) -> Ticks {
        (time_stamp - self).0
    }
    fn ticks_from_now(&self, time_stamp: Self) -> Ticks {
        (self - time_stamp).0
    }
    fn further_in_the_future(&self, than: Self, reference: Self) -> bool {
        self.ticks_from_now(reference) > than.ticks_from_now(reference)
    }
    fn further_in_the_past(&self, other: Self, reference: Self) -> bool {
        self.ticks_ago(reference) > other.ticks_ago(reference)
    }
    fn in_range(&self, range: &RangeInclusive<Wrapping<Ticks>>) -> bool {
        !self.further_in_the_future(*range.end(), *range.start()) &&
        !self.further_in_the_past(*range.start(), *range.end())
    }
}

/// Buffer component/resource that contains despawned data so it can be recovered.
pub struct Despawned<T>(pub T);

impl<T: Component> Component for Despawned<T> {
    type Storage = <T as Component>::Storage;
}

/// Flag Component to mark an `Entity` as despawned.
#[derive(Component)]
pub struct DespawnedEntity;
