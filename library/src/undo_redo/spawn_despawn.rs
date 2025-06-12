use std::panic::Location;

use crate::meta::NonLogNow;

use super::*;

/// todo, mention register_non_entity_buffer
#[derive(Component, Clone, Copy, Debug, Eq, Ord)]
#[component(immutable)]
pub struct DisabledToDespawn {
    added_frame: u64,
    added_location: MaybeLocation<Option<&'static Location<'static>>>,
}

impl PartialEq for DisabledToDespawn {
    fn eq(&self, other: &Self) -> bool {
        self.added_frame.eq(&other.added_frame)
    }
}

impl PartialOrd for DisabledToDespawn {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.added_frame.partial_cmp(&other.added_frame)
    }
}

impl DisabledToDespawn {
    pub(crate) fn for_buffer(added_frame: u64) -> Self {
        Self {
            added_frame,
            added_location: MaybeLocation::new(None),
        }
    }
    #[track_caller]
    pub(crate) fn for_spawn_despawn(added_frame: u64) -> Self {
        Self {
            added_frame,
            added_location: MaybeLocation::new_with(|| Some(Location::caller())),
        }
    }
    pub fn added_frame(self) -> u64 {
        self.added_frame
    }
    pub fn added_location(self) -> MaybeLocation<Option<&'static Location<'static>>> {
        self.added_location
    }
}

pub trait RevIsDespawned {
    fn rev_is_despawned(&self) -> bool;
}

impl RevIsDespawned for EntityRef<'_> {
    fn rev_is_despawned(&self) -> bool {
        self.contains::<DisabledToDespawn>()
    }
}

impl<B: Bundle> RevIsDespawned for EntityRefExcept<'_, B> {
    fn rev_is_despawned(&self) -> bool {
        self.contains::<DisabledToDespawn>()
    }
}

impl RevIsDespawned for FilteredEntityRef<'_> {
    fn rev_is_despawned(&self) -> bool {
        self.contains::<DisabledToDespawn>()
    }
}

impl RevIsDespawned for EntityMut<'_> {
    fn rev_is_despawned(&self) -> bool {
        self.contains::<DisabledToDespawn>()
    }
}

impl<B: Bundle> RevIsDespawned for EntityMutExcept<'_, B> {
    fn rev_is_despawned(&self) -> bool {
        self.contains::<DisabledToDespawn>()
    }
}

impl RevIsDespawned for FilteredEntityMut<'_> {
    fn rev_is_despawned(&self) -> bool {
        self.contains::<DisabledToDespawn>()
    }
}

impl RevIsDespawned for EntityWorldMut<'_> {
    fn rev_is_despawned(&self) -> bool {
        self.contains::<DisabledToDespawn>()
    }
}

/// todo
pub struct DespawnAtUndo(pub NonLogNow);

impl BundleEffect for DespawnAtUndo {
    fn apply(self, entity: &mut EntityWorldMut) {
        let mut resource = entity
            .world_scope(World::remove_resource::<RevRelationship>)
            .expect("todo");
        let _ = resource.try_despawn(entity, self.0, false);
        entity.world_scope(|world| world.insert_resource(resource));
    }
}

unsafe impl Bundle for DespawnAtUndo {
    fn get_component_ids(components: &Components, ids: &mut impl FnMut(Option<ComponentId>)) {
        <() as Bundle>::get_component_ids(components, ids);
    }

    fn register_required_components(
        components: &mut ComponentsRegistrator,
        required_components: &mut RequiredComponents,
    ) {
        <() as Bundle>::register_required_components(components, required_components);
    }

    fn component_ids(components: &mut ComponentsRegistrator, ids: &mut impl FnMut(ComponentId)) {
        <() as Bundle>::component_ids(components, ids);
    }
}

impl DynamicBundle for DespawnAtUndo {
    type Effect = Self;

    fn get_components(self, func: &mut impl FnMut(StorageType, OwningPtr<'_>)) -> Self::Effect {
        <() as DynamicBundle>::get_components((), func);
        self
    }
}

#[cfg(test)]
mod test {
    use super::*;

    // todo
}
