use std::{any::type_name, marker::PhantomData};

use bevy::{
    ecs::system::{Resource, SystemParam},
    prelude::Res,
};

pub trait UserStateTrait: Resource {
    type Index: Send + Sync + 'static;
    type Param<'w>: SystemParam + Send + Sync;
    type Output;
    fn get_state<'w: 'a, 'a>(param: &'a Self::Param<'w>, index: Self::Index) -> &'a Self::Output;
}

pub struct UserState<T: Resource, Index: Send + Sync + 'static = usize>(PhantomData<(T, Index)>)
where
    usize: From<Index>;

impl<T: Resource, Index: Send + Sync + 'static> UserStateTrait for UserState<T, Index>
where
    usize: From<Index>,
{
    type Index = Index;
    type Param<'w> = Res<'w, Vec<T>>;
    type Output = T;
    fn get_state<'w: 'a, 'a>(param: &'a Self::Param<'w>, index: Self::Index) -> &'a Self::Output {
        let index = usize::from(index);
        param.get(index).unwrap_or_else(|| {
            panic!(
                "Could not find state in `{}` at index {index}, vector length is {}.",
                type_name::<Self::Param<'w>>(),
                param.len()
            )
        })
    }
}

impl UserStateTrait for () {
    type Index = ();
    type Param<'w> = ();
    type Output = ();
    fn get_state<'w: 'a, 'a>(_param: &Self::Param<'w>, _index: Self::Index) -> &'a Self::Output {
        &()
    }
}
