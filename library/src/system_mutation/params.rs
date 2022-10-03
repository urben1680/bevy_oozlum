/*
NOTES

AdvanceFast:
Sollte nur aufgerufen werden wenn bisheriger Fortschritt eingeholt wurde.
Das Log müsste wissen welcher timestamp derzeit vorliegt. Das kann es noch nicht und würde 2 bytes extra speicher benötigen
Alternativ kann man den Kontext betrachten:
Die Funktion gibt drei mögliche Ergebnisse wieder:
1. Das Ziel Timestamp wurde erreicht, es wird eh kein neuer aufruf stattfinden
2. Eine Transition wurde erreicht, der aktuelle timestamp ergibt sich mit dem neuen entry
3. Es wurde nur ein step getan (default impl)
Um programmatisch klar den dritten Fall zu erkennen, lohnt sich vielleicht eine weitere Trait variation. Damit verdoppeln sich die Traits auf 8:
- entity logs ja/nein
- states ja/nein
- fast forward ja/nein (würde auch advance/revert log end ermöglichen oder nicht)

-> Trait hell

Mögliche Alternativen:

Elterntrait
type State: (), State<T> -> legt fest ob log entry index enthält
type Query: (), Query<T, const BATCH_SIZE: usize = 0> -> legt funktionsparameter fest
const Fast: bool -> legt umgang in fast kontext fest, default impl versichern das Wert

alternative zum boolean:

advance_up_to hat die parameter PARAMS (struct) und 2 time stamps.
PARAMS könnte eine union sein und die funktionen können mit einem default aufgerufen werden
*/

use std::{
    any::{type_name, TypeId},
    collections::VecDeque,
    marker::PhantomData,
    mem::{needs_drop, MaybeUninit},
    num::Wrapping,
};

use bevy::{
    ecs::{
        query::{QueryItem, WorldQuery},
        system::{Resource, SystemParam},
    },
    prelude::{Component, Mut, ParallelCommands, Query, Res, ResMut, Without},
};

use crate::{commands::NextCommands, controller::Controller, DespawnedEntity, Ticks, MAX_LOG_LEN};

type Params<'w, 'a, T> = ReversibleParams<
    'w,
    'a,
    <T as ReversibleComponents>::State,
    <T as ReversibleComponents>::Params,
    <T as ReversibleComponents>::LogPosition,
>;
type ParamsTransition<'w, 'a, T> = ReversibleParamsTransition<
    'w,
    'a,
    <T as ReversibleComponents>::State,
    <T as ReversibleComponents>::Params,
    <T as ReversibleComponents>::LogPosition,
    <T as ReversibleComponents>::Transition,
>;

pub trait ReversibleComponents: Send + Sync + Sized + 'static {
    type State: UserStateTrait; //() or UserState<T, usize>
    type Params: SystemParam;
    type LogPosition: LogPositionTrait; //`PerSystem` or `PerEntity<Q, 0>`
    type Transition: Send + Sync + 'static;
    const INITIAL_LOG_CAPACITY: usize = MAX_LOG_LEN;
    const LOG_CAPACITY_GROWTH: usize = 1;
    const FAST_FUNCTIONS: bool = false;
    fn next_transition(
        params: &mut Params<Self>,
        now: Wrapping<Ticks>,
    ) -> Option<NextTransition<<Self::State as UserStateTrait>::Index, Self::Transition, Self>>;
    fn advance(params: &mut Params<Self>, now: Wrapping<Ticks>);
    fn revert(params: &mut Params<Self>, now: Wrapping<Ticks>);
    fn advance_up_to(params: &mut Params<Self>, now: Wrapping<Ticks>, target: Wrapping<Ticks>) {
        Self::fast_function_error("advance_up_to");
        #[allow(clippy::no_effect)]
        (params, now, target);
    }
    fn revert_down_to(params: &mut Params<Self>, now: Wrapping<Ticks>, target: Wrapping<Ticks>) {
        Self::fast_function_error("revert_down_to");
        #[allow(clippy::no_effect)]
        (params, now, target);
    }
    fn advance_transition(params: &mut ParamsTransition<Self>, now: Wrapping<Ticks>) {
        #[allow(clippy::no_effect)]
        (params, now);
    }
    fn revert_transition(params: &mut ParamsTransition<Self>, now: Wrapping<Ticks>) {
        #[allow(clippy::no_effect)]
        (params, now);
    }
}

trait ReversibleComponentsImplemented: ReversibleComponents {
    fn fast_function_error(fn_name: &'static str) {
        if Self::FAST_FUNCTIONS {
            panic!(
                "`FAST_FUNCTIONS` should be `false` if `{fn_name}` is not implemented for type `{}`.",
                type_name::<Self>()
            )
        } else {
            unreachable!(
                "`{fn_name}` for type `{}` should not be called as `FAST_FUNCTIONS` is set to `false`.",
                type_name::<Self>()
            )
        }
    }
    fn eager_functions() -> Option<(
        fn(&mut Params<Self>, Wrapping<Ticks>, Wrapping<Ticks>),
        fn(&mut Params<Self>, Wrapping<Ticks>, Wrapping<Ticks>),
    )> {
        let default_advance_called = &mut false;
        let default_revert_called = &mut false;
        let mut check = ReversibleParams(DefaultImplCheckWrapper {
            default_advance_called,
            default_revert_called,
            inner: Default::default(),
        });
        //funktioniert nicht, user-implimentierte funktion verursacht UB bei ReversibleParamsInner::default()
        Self::advance_up_to(&mut check, Default::default(), Default::default());
        Self::revert_down_to(&mut check, Default::default(), Default::default());
        match (default_advance_called, default_revert_called) {
            (false, false) => Some((Self::advance_up_to, Self::revert_down_to)),
            (false, true) => panic!(
                "Forgot to implement `revert_down_to` for `{}`.",
                type_name::<Self>()
            ),
            (true, false) => panic!(
                "Forgot to implement `advance_up_to` for `{}`.",
                type_name::<Self>()
            ),
            (true, true) => None,
        }
    }
}

impl<T: ReversibleComponents> ReversibleComponentsImplemented for T {}

pub struct ReversibleParams<
    'w,
    'a,
    UserState: UserStateTrait,
    UserParams: SystemParam,
    UserQuery: LogPositionTrait,
>(DefaultImplCheckWrapper<'a, ReversibleParamsInner<'w, 'a, UserState, UserParams, UserQuery>>);

pub struct ReversibleParamsTransition<
    'w,
    'a,
    UserState: UserStateTrait,
    UserParams: SystemParam,
    UserQuery: LogPositionTrait,
    UserTransition,
> {
    past_state: MaybeUninit<&'a UserState>,
    future_state: MaybeUninit<&'a UserState>,
    params_mut: MaybeUninit<&'a mut UserParams>,
    params: MaybeUninit<&'a UserParams>,
    query_items: MaybeUninit<&'a mut UserQuery::QueryItem<'w>>,
    transition: MaybeUninit<&'a UserTransition>,
}

struct DefaultImplCheckWrapper<'a, T> {
    default_advance_called: &'a mut bool,
    default_revert_called: &'a mut bool,
    inner: T,
}

impl<'a, T> DefaultImplCheckWrapper<'a, T> {
    fn default_check<const FORWARD: bool>(&mut self) {
        *self.default_advance_called |= FORWARD;
        *self.default_revert_called |= !FORWARD;
    }
}

struct ReversibleParamsInner<
    'w,
    'a,
    UserState: UserStateTrait,
    UserParams: SystemParam,
    UserQuery: LogPositionTrait,
> {
    state: MaybeUninit<&'a UserState>,
    params_mut: MaybeUninit<&'a mut UserParams>,
    params: MaybeUninit<&'a UserParams>,
    query_items: MaybeUninit<&'a mut UserQuery::QueryItem<'w>>,
}

impl<'w, 'a, UserState: UserStateTrait, UserParams: SystemParam, UserQuery: LogPositionTrait>
    Default for ReversibleParamsInner<'w, 'a, UserState, UserParams, UserQuery>
{
    fn default() -> Self {
        Self {
            state: MaybeUninit::zeroed(),
            params_mut: MaybeUninit::zeroed(),
            params: MaybeUninit::zeroed(),
            query_items: MaybeUninit::zeroed(),
        }
    }
}

struct ReversibleParamsTransitionInner<
    'w,
    'a,
    UserState: UserStateTrait,
    UserParams: SystemParam,
    UserQuery: LogPositionTrait,
    UserTransition,
> {
    past_state: MaybeUninit<&'a UserState>,
    future_state: MaybeUninit<&'a UserState>,
    params_mut: MaybeUninit<&'a mut UserParams>,
    params: MaybeUninit<&'a UserParams>,
    query_items: MaybeUninit<&'a mut UserQuery::QueryItem<'w>>,
    transition: MaybeUninit<&'a UserTransition>,
}

impl<
        'w,
        'a,
        UserState: UserStateTrait,
        UserParams: SystemParam,
        UserQuery: LogPositionTrait,
        UserTransition,
    > Default
    for ReversibleParamsTransitionInner<'w, 'a, UserState, UserParams, UserQuery, UserTransition>
{
    fn default() -> Self {
        Self {
            past_state: MaybeUninit::zeroed(),
            future_state: MaybeUninit::zeroed(),
            params_mut: MaybeUninit::zeroed(),
            params: MaybeUninit::zeroed(),
            query_items: MaybeUninit::zeroed(),
            transition: MaybeUninit::zeroed(),
        }
    }
}

struct UserState<T: Resource, Index: Send + Sync + 'static = usize>(PhantomData<(T, Index)>)
where
    usize: From<Index>;

pub trait UserStateTrait: Resource {
    type Index: Send + Sync + 'static;
    type Param<'w>: SystemParam;
    type Output;
    fn get_state<'w: 'a, 'a>(param: &'a Self::Param<'w>, index: Self::Index) -> &'a Self::Output;
}

pub(super) type OutLogOnly<'w, 'a, T> = (&'w Controller, &'a mut Log<T>);

pub trait LogPositionTrait: 'static {
    type QueryItem<'w>;
    type In<'w: 's, 's, Other: SystemParam + Send + Sync + 'w, T: ReversibleComponents>: SystemParam;
    type Out<'w: 's, 's, 'a, Other: SystemParam + Send + Sync + 'w, T: ReversibleComponents>;
    type InLogOnly<'w: 's, 's, T: ReversibleComponents>: SystemParam;
    fn mutate<'w: 's, 's, Other: SystemParam + Send + Sync, T: ReversibleComponents>(
        params: &'w mut Self::In<'w, 's, Other, T>,
        f: fn(Self::Out<'w, 's, '_, Other, T>),
    );
    fn mutate_log<'w: 's, 's, T: ReversibleComponents>(
        params: &'w mut Self::InLogOnly<'w, 's, T>,
        f: fn(OutLogOnly<'w, '_, T>),
    );
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
    type In<'w: 's, 's, Other: SystemParam + Send + Sync + 'w, T: ReversibleComponents> =
        (Other, Res<'w, Controller>, ResMut<'w, Log<T>>);
    type Out<'w: 's, 's, 'a, Other: SystemParam + Send + Sync + 'w, T: ReversibleComponents> =
        (&'w mut Other, &'w Controller, &'a mut Log<T>);
    type InLogOnly<'w: 's, 's, T: ReversibleComponents> = (Res<'w, Controller>, ResMut<'w, Log<T>>);
    fn mutate<'w: 's, 's, Other: SystemParam + Send + Sync + 'w, T: ReversibleComponents>(
        params: &'w mut Self::In<'w, 's, Other, T>,
        f: fn(Self::Out<'w, 's, '_, Other, T>),
    ) {
        f((&mut params.0, &params.1, &mut params.2));
    }
    fn mutate_log<'w: 's, 's, T: ReversibleComponents>(
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
    type In<'w: 's, 's, Other: SystemParam + Send + Sync + 'w, T: ReversibleComponents> = (
        Other,
        Res<'w, Controller>,
        Query<'w, 's, (Q, &'static mut Log<T>), Without<DespawnedEntity>>,
    );
    type Out<'w: 's, 's, 'a, Other: SystemParam + Send + Sync + 'w, T: ReversibleComponents> = (
        &'w Other,
        &'w Controller,
        &'a mut Log<T>,
        Self::QueryItem<'w>,
    );
    type InLogOnly<'w: 's, 's, T: ReversibleComponents> =
        (Res<'w, Controller>, Query<'w, 's, &'static mut Log<T>>);
    fn mutate<'w: 's, 's, Other: SystemParam + Send + Sync + 'w, T: ReversibleComponents>(
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
    fn mutate_log<'w: 's, 's, T: ReversibleComponents>(
        params: &'w mut Self::InLogOnly<'w, 's, T>,
        f: fn(OutLogOnly<'w, '_, T>),
    ) {
        Self::apply_mutate(&mut params.1, |mut log: Mut<'w, Log<T>>| {
            f((&params.0, &mut *log))
        });
    }
}

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

pub struct NextTransition<Index: Send + Sync + 'static, Transition, Marker: Send + Sync + 'static> {
    pub(super) next_state_index: Index,
    pub(super) transition: Transition,
    pub(super) commands: Option<NextCommands<Marker>>,
}

#[derive(Component)]
pub struct Log<T: ReversibleComponents> {
    pub(super) entry_index: usize,
    pub(super) entries: VecDeque<LogEntry<T>>,
}

pub(super) struct LogEntry<T: ReversibleComponents> {
    pub(super) transition: MaybeUninit<T::Transition>,
    pub(super) time_stamp: Wrapping<Ticks>,
    pub(super) state_index: <T::State as UserStateTrait>::Index,
}
