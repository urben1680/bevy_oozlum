mod log;
mod per_entity;
mod per_system;

#[cfg(test)]
pub(crate) mod test;

use std::{fmt::Debug, marker::PhantomData};

use bevy::{
    ecs::system::{Resource, SystemParam},
    prelude::Res,
};

use crate::commands::ReversibleCommand;


pub trait StateOption: Resource {
    type Index: Send + Sync + Copy + Debug + 'static;
    type Param<'w>: SystemParam + Send + Sync;
    type Output;
    /// Returns the element at given index or else the length of the internal vector if  `index` is out of range.
    fn get_state<'w: 'a, 'a>(
        param: &'a Self::Param<'w>,
        index: Self::Index,
    ) -> Result<&'a Self::Output, usize>;
}

pub struct State<T: Resource, Index: Send + Sync + 'static = usize>(PhantomData<(T, Index)>)
where
    usize: From<Index>;

pub struct StateCollection<T> {
    pub resource: Vec<T>,
}

impl<T: Resource, Index: Send + Sync + Copy + Debug + 'static> StateOption for State<T, Index>
where
    usize: From<Index>,
{
    type Index = Index;
    type Param<'w> = Res<'w, StateCollection<T>>;
    type Output = T;
    fn get_state<'w: 'a, 'a>(
        param: &'a Self::Param<'w>,
        index: Self::Index,
    ) -> Result<&'a Self::Output, usize> {
        let index = usize::from(index);
        param.resource.get(index).ok_or(param.resource.len())
    }
}

impl StateOption for () {
    type Index = ();
    type Param<'w> = ();
    type Output = ();
    fn get_state<'w: 'a, 'a>(
        _param: &Self::Param<'w>,
        _index: Self::Index,
    ) -> Result<&'a Self::Output, usize> {
        Ok(&())
    }
}

pub struct NextTransition<State: StateOption, Transition> {
    pub next_state_index: State::Index,
    pub transition: Transition,
    pub commands: Vec<Box<dyn ReversibleCommand>>,
}
