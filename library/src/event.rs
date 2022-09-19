use std::{collections::VecDeque, marker::PhantomData, slice::Iter};

use bevy::{ecs::{event::Event, system::SystemParam}, prelude::{ResMut, Res, Events, EventWriter}};

use crate::MAX_LOG_LEN_USIZE;

pub type ReversibleEventWriter<'w, 's, T> = EventWriter<'w, 's, T>;

#[derive(SystemParam)]
pub struct ReversibleEventReader<'w, 's, T: Event>{
    reader: Res<'w, ReversibleEvent<T>>,
    #[system_param(ignore)]
    p: PhantomData<&'s ()>
}

impl<'w, 's, T: Event> ReversibleEventReader<'w, 's, T>{
    pub fn iter(&self) -> Iter<T>{
        self
            .reader
            .log
            .get(self.reader.log_index)
            .unwrap()
            .iter()
    }
}

pub struct ReversibleEvent<T: Event>{
    log: VecDeque<Vec<T>>,
    log_index: usize,
}

impl<T: Event> ReversibleEvent<T>{
    pub(super) fn system_clear_buffer(mut buffer: ResMut<Events<T>>){
        buffer.clear();
    }
    pub(super) fn system_forward(mut reader: ResMut<Self>, mut buffer: ResMut<Events<T>>){
        if reader.log.len() == MAX_LOG_LEN_USIZE{
            let oldest = reader.log.front_mut().unwrap();
            oldest.clear();
            oldest.extend(buffer.drain());
            reader.log.rotate_left(1);
        } else {
            reader.log.push_back(buffer.drain().collect());
            reader.log_index += 1;
        }
    }
    pub(super) fn system_forward_log(mut reader: ResMut<Self>){
        reader.log_index += 1;
    }
    pub(super) fn system_backward_log(mut reader: ResMut<Self>){
        reader.log_index -= 1;
    }
}