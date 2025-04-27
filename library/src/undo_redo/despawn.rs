use std::borrow::Borrow;

use super::*;

#[derive(Component, Clone, Copy, Debug, Hash, PartialOrd, Ord, PartialEq, Eq)]
#[component(immutable)]
pub(crate) struct DespawnAtOutOfLog(u64);

impl DespawnAtOutOfLog {
    pub(crate) fn new(meta: Option<impl Borrow<RevMeta>>) -> Result<Self, DespawnAtOutOfLogErr> {
        let meta = meta.ok_or(DespawnAtOutOfLogErr::RevMetaMissing)?;
        let meta = meta.borrow();
        let running_direction = meta.get_running_direction();
        if running_direction == Some(RevDirection::NOT_LOG) {
            return Err(DespawnAtOutOfLogErr::NotRunningNonLogDirection(
                running_direction,
            ));
        }
        Ok(Self(meta.now()))
    }
}

#[derive(Debug, Clone, Copy, Hash)] //todo: Error
pub enum DespawnAtOutOfLogErr {
    RevMetaMissing,
    NotRunningNonLogDirection(Option<RevDirection>),
}

#[derive(QueryFilter)]
pub struct WithRevDespawned {
    _filter: With<DespawnAtOutOfLog>,
}

pub struct HasRevDespawned;

// SAFETY: same as Has
unsafe impl WorldQuery for HasRevDespawned {
    type Fetch<'a> = bool;
    type State = ComponentId;

    const IS_DENSE: bool = <Has<DespawnAtOutOfLog> as WorldQuery>::IS_DENSE;

    fn shrink_fetch<'wlong: 'wshort, 'wshort>(fetch: Self::Fetch<'wlong>) -> Self::Fetch<'wshort> {
        <Has<DespawnAtOutOfLog> as WorldQuery>::shrink_fetch(fetch)
    }

    unsafe fn init_fetch<'w>(
        world: bevy::ecs::world::unsafe_world_cell::UnsafeWorldCell<'w>,
        state: &Self::State,
        last_run: bevy::ecs::component::Tick,
        this_run: bevy::ecs::component::Tick,
    ) -> Self::Fetch<'w> {
        unsafe {
            // SAFETY: same as Has
            <Has<DespawnAtOutOfLog> as WorldQuery>::init_fetch(world, state, last_run, this_run)
        }
    }

    unsafe fn set_archetype<'w>(
        fetch: &mut Self::Fetch<'w>,
        state: &Self::State,
        archetype: &'w bevy::ecs::archetype::Archetype,
        table: &'w bevy::ecs::storage::Table,
    ) {
        unsafe {
            // SAFETY: same as Has
            <Has<DespawnAtOutOfLog> as WorldQuery>::set_archetype(fetch, state, archetype, table)
        }
    }

    unsafe fn set_table<'w>(
        fetch: &mut Self::Fetch<'w>,
        state: &Self::State,
        table: &'w bevy::ecs::storage::Table,
    ) {
        unsafe {
            // SAFETY: same as Has
            <Has<DespawnAtOutOfLog> as WorldQuery>::set_table(fetch, state, table)
        }
    }

    fn update_component_access(
        state: &Self::State,
        access: &mut bevy::ecs::query::FilteredAccess<ComponentId>,
    ) {
        <Has<DespawnAtOutOfLog> as WorldQuery>::update_component_access(state, access)
    }

    fn init_state(world: &mut World) -> Self::State {
        <Has<DespawnAtOutOfLog> as WorldQuery>::init_state(world)
    }

    fn get_state(components: &bevy::ecs::component::Components) -> Option<Self::State> {
        <Has<DespawnAtOutOfLog> as WorldQuery>::get_state(components)
    }

    fn matches_component_set(
        state: &Self::State,
        set_contains_id: &impl Fn(ComponentId) -> bool,
    ) -> bool {
        <Has<DespawnAtOutOfLog> as WorldQuery>::matches_component_set(state, set_contains_id)
    }

    fn set_access(state: &mut Self::State, access: &bevy::ecs::query::FilteredAccess<ComponentId>) {
        <Has<DespawnAtOutOfLog> as WorldQuery>::set_access(state, access);
    }
}

// SAFETY: same as Has
unsafe impl QueryData for HasRevDespawned {
    type ReadOnly = Self;
    type Item<'a> = bool;

    const IS_READ_ONLY: bool = true;

    fn shrink<'wlong: 'wshort, 'wshort>(item: Self::Item<'wlong>) -> Self::Item<'wshort> {
        <Has<DespawnAtOutOfLog> as QueryData>::shrink(item)
    }

    unsafe fn fetch<'w>(
        fetch: &mut Self::Fetch<'w>,
        entity: Entity,
        table_row: bevy::ecs::storage::TableRow,
    ) -> Self::Item<'w> {
        unsafe {
            // SAFETY: same as Has
            <Has<DespawnAtOutOfLog> as QueryData>::fetch(fetch, entity, table_row)
        }
    }
}

// SAFETY: same as Has
unsafe impl ReadOnlyQueryData for HasRevDespawned {}

#[derive(QueryData)]
pub struct RefRevDespawned {
    marker: &'static DespawnAtOutOfLog,
}

impl RefRevDespawnedItem<'_> {
    pub fn added_at(&self) -> u64 {
        self.marker.0
    }
}

pub trait RevIsDespawned {
    fn rev_is_despawned(&self) -> bool;
}

impl RevIsDespawned for EntityRef<'_> {
    fn rev_is_despawned(&self) -> bool {
        self.contains::<DespawnAtOutOfLog>()
    }
}

impl<B: Bundle> RevIsDespawned for EntityRefExcept<'_, B> {
    fn rev_is_despawned(&self) -> bool {
        self.contains::<DespawnAtOutOfLog>()
    }
}

impl RevIsDespawned for FilteredEntityRef<'_> {
    fn rev_is_despawned(&self) -> bool {
        self.contains::<DespawnAtOutOfLog>()
    }
}

impl RevIsDespawned for EntityMut<'_> {
    fn rev_is_despawned(&self) -> bool {
        self.contains::<DespawnAtOutOfLog>()
    }
}

impl<B: Bundle> RevIsDespawned for EntityMutExcept<'_, B> {
    fn rev_is_despawned(&self) -> bool {
        self.contains::<DespawnAtOutOfLog>()
    }
}

impl RevIsDespawned for FilteredEntityMut<'_> {
    fn rev_is_despawned(&self) -> bool {
        self.contains::<DespawnAtOutOfLog>()
    }
}

impl RevIsDespawned for EntityWorldMut<'_> {
    fn rev_is_despawned(&self) -> bool {
        self.contains::<DespawnAtOutOfLog>()
    }
}
