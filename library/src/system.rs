use std::marker::PhantomData;

use bevy::{ecs::{system::SystemParam, query::{WorldQuery, QueryItem}, event::Event}, prelude::{Query, Component, Without, Commands}};

use crate::{PresentEntity, DespawnedEntity, commands::{ReversibleCommand, ReversibleCommands}};

pub trait Derived: SystemParam{
    /*
    - replaces `Entity` with `PresentEntity` or, if not present in query, adds `Without<EntityDespawned>`
    - sensitive to marked query

    - alternative, implementing struct consists of systemparams and might also contain worldquery items
    -- straight-forward as user params
    -- enforce rules: PresentEntity, no commands, no events (both only for state changes)
    --- Braucht zusätzlichen type für StateChange, Filter


    types:
    - State (S)
    - Transition (T)

    - User Params System (US) -> UP
    - User Params (UP)

    - State Change Params System (CS) -> CP
    - State Change Params (CP)

    - Log Only System (LS)
    */
}

trait DraftQuery: WorldQuery + Sized{
    type SystemParams: SystemParam;
    type Transition;
    fn system(mut query: Query<(Self, &mut Log), Without<DespawnedEntity>>, other: Self::SystemParams){
        query.for_each_mut(|(user_items, log)| 
            Self::user_system(user_items, &other)
        )
    }
    fn system_change(mut query: Query<(Self, &mut Log), Without<DespawnedEntity>>, other: Self::SystemParams, commands: Commands){
        let commands = &mut ReversibleCommands(commands);
        query.for_each_mut(|(user_items, log)| 
            if let Some(change) = Self::user_system_change(user_items, &other){
                change
                    .commands
                    .into_iter()
                    .for_each(|command| commands.add(command));
            }
        )
    }
    fn user_system(stuff: QueryItem<Self>, other: &Self::SystemParams);
    fn user_system_change(stuff: QueryItem<Self>, other: &Self::SystemParams) -> Option<Change<Self::Transition>>;
}


#[derive(Component)]
struct Log;

struct Change<T>{
    transition: T,
    commands: Vec<Box<dyn ReversibleCommand>>,
    //event vectors?
}

