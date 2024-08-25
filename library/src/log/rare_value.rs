use core::fmt::Debug;
use std::{
    cmp::Ordering,
    collections::{vec_deque::Drain, TryReserveError, VecDeque},
};

use bevy::ecs::{component::Component, system::Resource};

use crate::meta::RevMeta;

use super::{
    NPerFrame, OutOfLog, Packed, RareData, WithAmount, WithTimestamp, BACKWARD_EXPECT_MSG,
};

#[derive(Debug, Clone, Component, Resource)]
pub struct RareValueLog<T> {
    /// RareData.skips represents the number of None pushes after the value in the struct
    values: VecDeque<RareData<T>>,
    present: RareData<T>,
    future_index: usize,
    skips: usize,
    skips_max: usize,
    len: usize,
}

impl<T> From<T> for RareValueLog<T> {
    fn from(present: T) -> Self {
        Self::new(present)
    }
}

impl<T> RareValueLog<T> {
    pub const fn new(present: T) -> Self {
        Self {
            values: VecDeque::new(),
            present: RareData {
                data: present,
                skips: Packed(0),
            },
            future_index: 0,
            skips: 0,
            skips_max: 0,
            len: 0,
        }
    }
}
