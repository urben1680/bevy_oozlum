/*
events are buffered for two frames
a state log would fit

*/

//reference
type Reader<'w, 's, E> = EventReader<'w, 's, E>;
type Writer<'w, 's, E> = EventWriter<'w, E>;
type Events<E> = bevy::prelude::Events<E>;
type ManualReader<E> = ManualEventReader<E>;

use std::marker::PhantomData;

use bevy::{
    ecs::{
        event::{EventId, ManualEventReader},
        system::SystemParam,
    },
    prelude::{Event, EventReader, EventWriter, Local, Res, ResMut, Resource},
};

#[derive(SystemParam)]
pub struct RevEventReader<'w, 's, E: Event> {
    reader: Local<'s, ManualEventReader<E>>,
    events: Res<'w, RevEvents<E>>,
}

#[derive(SystemParam)]
pub struct RevEventWriter<'w, E: Event> {
    events: ResMut<'w, RevEvents<E>>,
}

#[derive(Resource)]
pub struct RevEvents<E: Event> {
    /// Holds the oldest still active events.
    /// Note that `a.start_event_count + a.len()` should always be equal to `events_b.start_event_count`.
    events_a: RevEventSequence<E>,
    /// Holds the newer events.
    events_b: RevEventSequence<E>,
    event_count: usize,
}

pub struct RevManualEventReader<E: Event> {
    last_event_count: usize,
    _marker: PhantomData<E>,
}

struct RevEventSequence<E: Event> {
    events: Vec<RevEventInstance<E>>,
    start_event_count: usize,
}

struct RevEventInstance<E: Event> {
    pub event_id: EventId<E>,
    pub event: E,
}
