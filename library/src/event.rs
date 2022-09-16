use std::{collections::VecDeque, marker::PhantomData};

use bevy::{ecs::{event::{Event, EventId}, system::{SystemParam, Resource}}, prelude::{Local, Res}};

/// see https://github.com/bevyengine/bevy/blob/main/crates/bevy_ecs/src/event.rs#L157
#[derive(Debug)]
struct EventSequence<E: Event> {
    events: Vec<EventInstance<E>>,
    start_event_count: usize,
}

/// see https://github.com/bevyengine/bevy/blob/main/crates/bevy_ecs/src/event.rs#L51
#[derive(Debug)]
struct EventInstance<E: Event> {
    pub event_id: EventId<E>,
    pub event: E,
}

/// see https://github.com/bevyengine/bevy/blob/main/crates/bevy_ecs/src/event.rs#L325
#[derive(Debug)]
pub struct ManualEventReader<E: Event> {
    last_event_count: usize,
    _marker: PhantomData<E>,
}

/// see https://github.com/bevyengine/bevy/blob/main/crates/bevy_ecs/src/event.rs#L188
#[derive(SystemParam)]
pub struct EventReader<'w, 's, E: Event> {
    reader: Local<'s, ManualEventReader<E>>,
    events: Res<'w, Events<E>>,
}

#[derive(Debug)]
pub struct Events<E: Event> {
    /* bevy variant:
    /// Holds the oldest still active events.
    /// Note that a.start_event_count + a.len() should always === events_b.start_event_count.
    events_a: EventSequence<E>,
    /// Holds the newer events.
    events_b: EventSequence<E>,
    event_count: usize,
    */
    log: VecDeque<EventSequence<E>>,
    log_index: usize,
    event_count: usize, //?
}