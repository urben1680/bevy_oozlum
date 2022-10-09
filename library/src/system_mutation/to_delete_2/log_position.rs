use std::marker::PhantomData;

use bevy::{
    ecs::{
        query::{QueryItem, WorldQuery},
        system::SystemParam,
    },
    prelude::{Mut, ParallelCommands, Query, Res, ResMut, Without},
};

use crate::{controller::Controller, DespawnedEntity};

use super::{state::UserStateTrait, Log, ReversibleSystem};

pub trait LogPositionTrait: 'static {
    type In<'w: 's, 's, T: ReversibleSystem>: SystemParam;
    type InLogOnly<'w: 's, 's, T: ReversibleSystem>: SystemParam;
    type Out<'w: 'a, 'a, T: ReversibleSystem>;
    type QueryItem<'w>;
    fn mutate<
        'w: 's + 'a,
        's,
        'a,
        T: ReversibleSystem,
        FN: Fn(
            &mut Log<T>,
            &mut Self::Out<'w, 'a, T>,
        ) + Send + Sync + Clone
    >(
        params: Self::In<'w, 's, T>,
        f: FN,
    );
    /*
    fn mutate_log_only<
        'w: 'a, 
        's: 'a,
        'a,
        T: ReversibleSystem,
        FN: for<'b> Fn(
            &'b mut Log<T>,
        ) + Send + Sync + Clone
    >(
        log: &'a mut Self::InLogOnly<'w, 's, T>,
        f: FN,
    );
    */
}

pub struct PerSystem;
pub struct PerEntity<Q: WorldQuery + 'static, const PAR_ITER_BATCH_SIZE: usize = 0>(PhantomData<Q>);

/*
impl<Q: WorldQuery + 'static, const PAR_ITER_BATCH_SIZE: usize> PerEntity<Q, PAR_ITER_BATCH_SIZE> {
    fn for_each_mut<
        'w: 's,
        's: 'a,
        'a,'w,
        T: WorldQuery,
        F: WorldQuery,
        FN: Fn (QueryItem<'a, T>) + Send + Sync + Clone,
    >(
        mut query: Query<'w, 's, T, F>,
        f: FN,
    ) {
        if PAR_ITER_BATCH_SIZE == 0 {
            query.for_each_mut::<'a, FN>(f);
        } else {
            query.par_for_each_mut::<'a, FN>(PAR_ITER_BATCH_SIZE, f);
        }
    }
}
*/

/*
impl LogPositionTrait for PerSystem {
    type In<'w: 's, 's, T: ReversibleSystem> = (T::Params, ResMut<'w, Log<T>>);
    type InLogOnly<'w: 's, 's, T: ReversibleSystem> = ResMut<'w, Log<T>>;
    type Out<'w: 's, 's, T: ReversibleSystem> = T::Params;
    type QueryItem<'w> = ();
    fn mutate<'w: 's, 's, T: ReversibleSystem>(
        controller: Res<'w, Controller>,
        commands: ParallelCommands,
        states: <T::State as UserStateTrait>::Param<'w>,
        params: &'w mut Self::In<'w, 's, T>,
        f: fn(
            &Controller,
            &ParallelCommands,
            &<T::State as UserStateTrait>::Param<'w>,
            &mut Log<T>,
            &mut Self::Out<'w, 's, T>,
        ),
    ) {
        f(
            &controller,
            &commands,
            &states,
            &mut *params.1,
            &mut params.0,
        );
    }
    fn mutate_log<'w: 's, 's, T: ReversibleSystem>(
        controller: Res<'w, Controller>,
        states: <T::State as UserStateTrait>::Param<'w>,
        params: &'w mut Self::In<'w, 's, T>,
        f: fn(
            &Controller,
            &<T::State as UserStateTrait>::Param<'w>,
            &mut Log<T>,
            &mut Self::Out<'w, 's, T>,
        ),
    ) {
        f(&controller, &states, &mut *params.1, &mut params.0);
    }
    fn mutate_log_only<'w: 's, 's, T: ReversibleSystem>(
        controller: Res<'w, Controller>,
        params: &'w mut Self::InLogOnly<'w, 's, T>,
        f: fn(&Controller, &mut Log<T>),
    ) {
        f(&controller, &mut *params);
    }
}
*/

impl<Q: WorldQuery + 'static, const PAR_ITER_BATCH_SIZE: usize> LogPositionTrait
    for PerEntity<Q, PAR_ITER_BATCH_SIZE>
{
    type In<'w: 's, 's, T: ReversibleSystem> = (
        T::Params,
        Query<'w, 's, (Q, &'static mut Log<T>), Without<DespawnedEntity>>,
    );
    type InLogOnly<'w: 's, 's, T: ReversibleSystem> = Query<'w, 's, &'static mut Log<T>>;
    type Out<'w: 'a, 'a, T: ReversibleSystem> = (&'a T::Params, Self::QueryItem<'w>);
    type QueryItem<'w> = QueryItem<'w, Q>;
    fn mutate<
        'w: 's + 'a,
        's,
        'a,
        T: ReversibleSystem,
        FN: Fn(
            &mut Log<T>,
            &mut Self::Out<'w, 'a, T>,
        ) + Send + Sync + Clone
    >(
        params: Self::In<'w, 's, T>,
        f: FN,
    ) {
        let (params, mut query) = params;
        if PAR_ITER_BATCH_SIZE == 0 {
            query.for_each_mut(|(item, mut log)|{
                f(
                    &mut *log,
                    &mut (&params, item),
                )
            });
        } else {
            query.par_for_each_mut(PAR_ITER_BATCH_SIZE, |(item, mut log)|{
                f(
                    &mut *log,
                    &mut (&params, item),
                )
            });
        }

/*
        Self::for_each_mut::<'w, 's, 'a, (Q, &'static mut Log<T>), Without<DespawnedEntity>, _>(
            params.1,
            |(item, mut log): (Self::QueryItem<'w>, Mut<'w, Log<T>>)| {
                f(
                    &controller,
                    &commands,
                    &states,
                    &mut log,
                    &mut (&params.0, item),
                );
            },
        );
        */
    }
    /*
    fn mutate_log_only<
        'w: 'a, 
        's: 'a,
        'a,
        T: ReversibleSystem,
        FN: for<'b> Fn(
            &'b mut Log<T>,
        ) + Send + Sync + Clone + 'a
    >(
        log: &'a mut Self::InLogOnly<'w, 's, T>,
        f: FN,
    ){
        let f = |mut log: Mut<'w, Log<T>>| {
            f(&mut *log);
        };
        if PAR_ITER_BATCH_SIZE == 0 {
            log.for_each_mut(f);
        } else {
            log.par_for_each_mut(PAR_ITER_BATCH_SIZE, f);
        }
        /*
        Self::for_each_mut::<'w, 's, 'a, &'static mut Log<T>, (), _>(
            log, 
            |mut log: Mut<'w, Log<T>>| {
                f(&controller, &mut log);
            }
        );
        */
    }
    */
}