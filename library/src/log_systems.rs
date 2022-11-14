pub mod log;
pub mod per_entity;
pub mod per_system;

#[cfg(test)]
pub(crate) mod test;

use std::{fmt::Debug, marker::PhantomData};

use bevy::{
    ecs::system::{Resource, SystemParam},
    prelude::Res,
};

use crate::commands::ReversibleCommand;

pub trait StateOption {
    type Index: Send + Sync + Copy + Debug + 'static;
    type Param<'w>: SystemParam + Send + Sync;
    type Output;
    /// Returns the element at given index or else the length of the internal vector if  `index` is out of range.
    fn get_state<'w: 'a, 'a>(states: &'a Self::Param<'w>, index: Self::Index) -> &'a Self::Output;
}

pub struct State<T: Resource, Index: Send + Sync + 'static = usize>(PhantomData<(T, Index)>)
where
    usize: From<Index>;

#[derive(Resource)]
pub struct StateCollection<T> {
    //todo versioning, const param?
    pub resource: Vec<T>,
}

impl<T: Resource, Index: Send + Sync + Copy + Debug + 'static> StateOption for State<T, Index>
where
    usize: From<Index>,
{
    type Index = Index;
    type Param<'w> = Res<'w, StateCollection<T>>;
    type Output = T;
    fn get_state<'w: 'a, 'a>(states: &'a Self::Param<'w>, index: Self::Index) -> &'a Self::Output {
        &states.resource[usize::from(index)]
    }
}

impl StateOption for () {
    type Index = ();
    type Param<'w> = ();
    type Output = ();
    fn get_state<'w: 'a, 'a>(_states: &Self::Param<'w>, _index: Self::Index) -> &'a Self::Output {
        &()
    }
}

pub struct NextTransition<State: StateOption, Transition> {
    pub next_state_index: State::Index,
    pub transition: Transition,
    pub commands: Vec<Box<dyn ReversibleCommand>>,
}
