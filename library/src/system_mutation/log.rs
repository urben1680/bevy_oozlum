use std::{collections::VecDeque, marker::PhantomData, mem::MaybeUninit, num::Wrapping};

use bevy::{ecs::system::Resource, prelude::Component};

use crate::Ticks;

#[derive(Component)]
pub(super) struct Log<Entry: LogEntryTrait<Transition>, Transition: Resource, Marker: Resource> {
    //size: 5*usize + n * (Transition + u16)
    pub(super) entry_index: usize,
    pub(super) entries: VecDeque<Entry>, //todo: should be allowed to grow past capacity if this ensures that max log len is reachable
    p: PhantomData<(Transition, Marker)>,
}

pub(super) struct LogEntryWithState<Transition: Resource> {
    pub(super) transition: MaybeUninit<Transition>,
    pub(super) time_stamp: Wrapping<Ticks>,
    pub(super) state_index: usize,
}

pub(super) struct LogEntry<Transition: Resource> {
    pub(super) transition: MaybeUninit<Transition>,
    pub(super) time_stamp: Wrapping<Ticks>,
}

pub(super) trait LogEntryTrait<Transition: Resource> {
    fn new(time_stamp: Wrapping<Ticks>, state_index: usize) -> Self;
    fn state_index(&self) -> usize;
    unsafe fn transition(&self) -> &Transition;
    fn time_stamp(&self) -> Wrapping<Ticks>;
    fn set_transition(&mut self, transition: Transition);
    //fn set_time_stamp(&mut self, time_stamp: Wrapping<Ticks>);
}

impl<Transition: Resource> LogEntryTrait<Transition> for LogEntry<Transition> {
    fn new(time_stamp: Wrapping<Ticks>, _state_index: usize) -> Self {
        Self {
            transition: MaybeUninit::uninit(),
            time_stamp,
        }
    }
    fn state_index(&self) -> usize {
        Default::default()
    }
    unsafe fn transition(&self) -> &Transition {
        &self.transition.assume_init_ref()
    }
    fn time_stamp(&self) -> Wrapping<Ticks> {
        self.time_stamp
    }
    fn set_transition(&mut self, transition: Transition) {
        self.transition.write(transition);
    }
}

impl<Transition: Resource> LogEntryTrait<Transition> for LogEntryWithState<Transition> {
    fn new(time_stamp: Wrapping<Ticks>, state_index: usize) -> Self {
        Self {
            transition: MaybeUninit::uninit(),
            time_stamp,
            state_index,
        }
    }
    fn state_index(&self) -> usize {
        self.state_index
    }
    unsafe fn transition(&self) -> &Transition {
        &self.transition.assume_init_ref()
    }
    fn time_stamp(&self) -> Wrapping<Ticks> {
        self.time_stamp
    }
    fn set_transition(&mut self, transition: Transition) {
        self.transition.write(transition);
    }
}
