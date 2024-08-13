use std::{
    any::{type_name, TypeId},
    borrow::Cow,
    marker::PhantomData,
    sync::{atomic::{AtomicBool, Ordering}, Arc, Mutex, RwLock, RwLockReadGuard},
};

use bevy::{
    app::FixedUpdate,
    ecs::{
        archetype::ArchetypeComponentId,
        component::{ComponentId, Tick},
        intern::Interned,
        query::Access,
        schedule::{
            InternedSystemSet, IntoSystemConfigs, ScheduleLabel, SystemConfig, SystemConfigs,
            SystemSet,
        },
        system::{IntoSystem, Res, Resource, System, SystemBuffer, SystemMeta, SystemParam},
        world::{unsafe_world_cell::UnsafeWorldCell, DeferredWorld, FromWorld, World},
    },
    prelude::{Condition, IntoSystemSet, IntoSystemSetConfigs, Local, ReadOnlySystem},
    utils::Parallel,
};

use crate::{
    log::{LimitLen, RareTransitionLog, TransitionsLog},
    meta::{Direction, RevMeta},
};

/* ┌┐└┘─│
                ForwardSchedule<_>   BackwardSchedule<_>
system T
┌──────────────────────────────────┐   ┌────────────┐
│ArcSystem<T>.pipe(CommandsForward)│ → │ sync point │
└──────────────────────────────────┘   └────────────┘
---------------------------------------------------------------------------
      ┌BackwardSet(TypeId::of<T>())┐   ┌────────────┐   ┌BackwardSet(TypeId::of<T>())┐
      │    CommandsBackward<T>     │ → │ sync point │ → │        ArcSystem<T>        │
      └────────────────────────────┘   └────────────┘   └────────────────────────────┘

Sind hooks/observers auch so reversible?
Sie sind eigentlich nicht Teil der systeme, können darüber also auch nicht in ihrer Reihenfolge gesteuert werden.
Aber wenn sie auch in den nächsten sync points angewandt werden, können ihre Effekte trotzdem durch spezielle Varianten geloggt werden.
Undo/Redo müssten dann als commands umgesetzt werden
*/
/*
Design user api

app.add_rev_systems(FixedUpdate, (a, b).chain().in_set(Foo))

*/









#[cfg(test)]
mod test {
    use bevy::ecs::{
        schedule::{IntoSystemConfigs, Schedule},
        system::{IntoSystem, System},
        world::World,
    };
    /*
    use super::{BackwardSet, RevCommands, ReversibleConfigs};

    #[test]
    fn orders_relative_to_backward_commands_system() {
        let mut world = World::new();

        let system1 = |_: RevCommands| {};
        let system2 = IntoSystem::into_system(|| {});
        let config = ReversibleConfigs::new(&mut world, system1).backward;
        let set = BackwardSet(system2.type_id());

        let mut schedule = Schedule::default();
        schedule
            .add_systems(config)
            .add_systems(system2.before(set))
            .initialize(&mut world)
            .unwrap();
    }
    */
}
