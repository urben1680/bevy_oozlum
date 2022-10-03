use std::{
    any::{type_name, TypeId},
    collections::VecDeque,
    marker::PhantomData,
    mem::{ManuallyDrop, MaybeUninit},
    num::Wrapping,
    ops::Index,
};

use bevy::{
    ecs::{
        query::{Fetch, QueryItem, WorldQuery},
        schedule::IntoSystemDescriptor,
        system::{Resource, StaticSystemParam, SystemParam, SystemParamFetch, SystemParamItem},
    },
    prelude::{
        App, Commands, Component, EventWriter, ParallelCommands, Query, Res, ResMut, System,
        Without,
    },
};

use crate::{
    commands::{CommandsScope, NextCommands, ReversibleCommands},
    controller::Controller,
    DespawnedEntity, Ticks, MAX_LOG_LEN,
};

//mod component_mutation;
//mod generic_composition;
mod log;
mod next_transition;
mod params;
//mod resource_mutation;

//pub use resource_mutation::*;
//pub use component_mutation::*;

pub struct ReversibleSystemContainer<
    'w,
    's,
    Params: SystemParam,
    ParamsOnlyLog: SystemParam,
    Commands: CommandsScope<'w, 's>,
> {
    pub advance: fn(Params, Res<'w, Controller>, Commands),
    pub advance_timestamp: fn(Params, Res<'w, Controller>, Commands), //commands must be applied later when returning timestamp is reached
    pub advance_log: fn(Params, Res<'w, Controller>),
    pub advance_log_end: fn(Params, Res<'w, Controller>),
    pub revert_log: fn(Params, Res<'w, Controller>),
    pub revert_log_end: fn(Params, Res<'w, Controller>),
    pub log_end: fn(ParamsOnlyLog, Res<'w, Controller>),
    pub log_age_check: fn(ParamsOnlyLog, Res<'w, Controller>), //check if timestamp of oldest entry is equal to current time stamp to drop just when time stamp was raised in controller
    p: PhantomData<&'s ()>,
}
