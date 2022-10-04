use std::marker::PhantomData;

use bevy::{
    ecs::{
        query::{QueryItem, WorldQuery},
        system::SystemParam,
    },
    prelude::{Mut, Query, Res, ResMut, Without, ParallelCommands},
};

use crate::{controller::Controller, DespawnedEntity};

use super::{Log, ReversibleSystem, params::{Params, ParamsTransition}};

pub(super) type OutLogOnly<'w, 'a, T> = (&'w Controller, &'a mut Log<T>);

pub trait LogPositionTrait: 'static {
    type QueryItem<'w>;
    type In<'w: 's, 's, Other: SystemParam + Send + Sync + 'w, T: ReversibleSystem>: SystemParam;
    type Out<'w: 's, 's, 'a, Other: SystemParam + Send + Sync + 'w, T: ReversibleSystem>;
    type InLogOnly<'w: 's, 's, T: ReversibleSystem>: SystemParam;
    type UserParams: UserParamContainer;
    /*
    fn mutate<'w: 's, 's, Other: SystemParam + Send + Sync, T: ReversibleSystem>(
        params: &'w mut Self::In<'w, 's, Other, T>,
        f: fn(Self::Out<'w, 's, '_, Other, T>, ),
    );
    */
    fn mutate<'w: 's, 's, T: ReversibleSystem>(
        params: Self::In::<'w, 's, (T::Params, ParallelCommands)>,
        f: fn(Self::UserParams, Self::Out<'w, 's, '_, (T::Params, ParallelCommands), T>)
    );
    fn mutate_log<'w: 's, 's, T: ReversibleSystem>(
        params: Self::In::<'w, 's, T::Params>,
        f: fn(Self::UserParams, Self::Out<'w, 's, '_, T::Params, T>)
    );
    fn mutate_log_only<'w: 's, 's, T: ReversibleSystem>(
        params: &'w mut Self::InLogOnly<'w, 's, T>,
        f: fn(OutLogOnly<'w, '_, T>),
    );
}

pub trait UserParamContainer{
    fn params<'w: 'a, 'a, T: ReversibleSystem>(&mut self) -> Params<'w, 'a, T>;
    fn params_transition<'w: 'a, 'a, T: ReversibleSystem>(&mut self) -> ParamsTransition<'w, 'a, T>;
}

pub struct PerSystem;
pub struct PerEntity<Q: WorldQuery + 'static, const PAR_ITER_BATCH_SIZE: usize = 0>(PhantomData<Q>);

impl<Q: WorldQuery + 'static, const PAR_ITER_BATCH_SIZE: usize> PerEntity<Q, PAR_ITER_BATCH_SIZE> {
    fn apply_mutate<
        'w: 's,
        's,
        T: WorldQuery,
        F: WorldQuery,
        FN: Fn(QueryItem<'w, T>) + Send + Sync + Clone,
    >(
        query: &'w mut Query<'w, 's, T, F>,
        f: FN,
    ) {
        if PAR_ITER_BATCH_SIZE == 0 {
            query.for_each_mut(f);
        } else {
            query.par_for_each_mut(PAR_ITER_BATCH_SIZE, f);
        }
    }
}

impl LogPositionTrait for PerSystem {
    type QueryItem<'w> = ();
    type In<'w: 's, 's, Other: SystemParam + Send + Sync + 'w, T: ReversibleSystem> =
        (Other, Res<'w, Controller>, ResMut<'w, Log<T>>);
    type Out<'w: 's, 's, 'a, Other: SystemParam + Send + Sync + 'w, T: ReversibleSystem> =
        (&'w mut Other, &'w Controller, &'a mut Log<T>);
    type InLogOnly<'w: 's, 's, T: ReversibleSystem> = (Res<'w, Controller>, ResMut<'w, Log<T>>);
    fn mutate<'w: 's, 's, Other: SystemParam + Send + Sync + 'w, T: ReversibleSystem>(
        params: &'w mut Self::In<'w, 's, Other, T>,
        f: fn(Self::Out<'w, 's, '_, Other, T>),
    ) {
        f((&mut params.0, &params.1, &mut params.2));
    }
    fn mutate_log_only<'w: 's, 's, T: ReversibleSystem>(
        params: &'w mut Self::InLogOnly<'w, 's, T>,
        f: fn(OutLogOnly<'w, '_, T>),
    ) {
        f((&params.0, &mut *params.1));
    }
}

impl<Q: WorldQuery + 'static, const PAR_ITER_BATCH_SIZE: usize> LogPositionTrait
    for PerEntity<Q, PAR_ITER_BATCH_SIZE>
{
    type QueryItem<'w> = QueryItem<'w, Q>;
    type In<'w: 's, 's, Other: SystemParam + Send + Sync + 'w, T: ReversibleSystem> = (
        Other,
        Res<'w, Controller>,
        Query<'w, 's, (Q, &'static mut Log<T>), Without<DespawnedEntity>>,
    );
    type Out<'w: 's, 's, 'a, Other: SystemParam + Send + Sync + 'w, T: ReversibleSystem> = (
        &'w Other,
        &'w Controller,
        &'a mut Log<T>,
        Self::QueryItem<'w>,
    );
    type InLogOnly<'w: 's, 's, T: ReversibleSystem> =
        (Res<'w, Controller>, Query<'w, 's, &'static mut Log<T>>);
    fn mutate<'w: 's, 's, Other: SystemParam + Send + Sync + 'w, T: ReversibleSystem>(
        params: &'w mut Self::In<'w, 's, Other, T>,
        f: fn(Self::Out<'w, 's, '_, Other, T>),
    ) {
        Self::apply_mutate(
            &mut params.2,
            |(item, mut log): (Self::QueryItem<'w>, Mut<'w, Log<T>>)| {
                f((&params.0, &params.1, &mut *log, item))
            },
        );
    }
    fn mutate_log_only<'w: 's, 's, T: ReversibleSystem>(
        params: &'w mut Self::InLogOnly<'w, 's, T>,
        f: fn(OutLogOnly<'w, '_, T>),
    ) {
        Self::apply_mutate(&mut params.1, |mut log: Mut<'w, Log<T>>| {
            f((&params.0, &mut *log))
        });
    }
}
