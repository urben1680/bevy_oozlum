use std::{marker::PhantomData, collections::VecDeque};

use bevy::{ecs::{system::{SystemParam, Resource}, query::{WorldQuery, QueryItem}}, prelude::{Query, Component, Without, Res, ResMut}};

use crate::{DespawnedEntity, Timestamp, commands::ReversibleCommands};


mod resource_mutation;
mod component_mutation;

pub use resource_mutation::*;
pub use component_mutation::*;

pub struct NextTransition<T>{
    next_state_index: usize,
    transition: T,
    commands: Option<Box<dyn FnOnce(&mut ReversibleCommands)>>
}

pub struct NextTransitionStateless<T>{
    transition: T,
    commands: Option<Box<dyn FnOnce(&mut ReversibleCommands)>>
}

#[derive(Component)]
pub struct Log<T: Resource, M: Resource>{
    entry_index: usize,
    entries: VecDeque<LogEntry<T>>,
    marker: PhantomData<M>
}

#[derive(Component)]
pub struct LogStateless<T: Resource, M: Resource>{
    entry_index: usize,
    entries: VecDeque<LogEntryStateless<T>>,
    marker: PhantomData<M>
}

struct LogEntry<T: Resource>{
    state_index: usize,
    transition: T,
    time_stamp: Timestamp
}

struct LogEntryStateless<T: Resource>{
    transition: T,
    time_stamp: Timestamp
}

trait LogMutation{
    //systems go here, functions are generic
}

impl<T: Resource, M: Resource> LogMutation for Log<T, M> {}
impl<T: Resource, M: Resource> LogMutation for LogStateless<T, M> {}