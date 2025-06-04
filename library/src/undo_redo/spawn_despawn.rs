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
pub struct DespawnAtUndo(NonLogNow);

impl BundleEffect for DespawnAtUndo {
    fn apply(self, entity: &mut EntityWorldMut) {
        let id = entity.id();
        let marker = DisabledToDespawn::for_spawn_despawn(self.0.0);
        entity.buffer_undo_redo(
            self.0,
            Spawn {
                spawned: [id],
                marker,
            },
        );
        let components = entity.archetype().components().collect::<Vec<_>>();
        entity
            .resource::<RevRelationship>()
            .clone()
            .buffer(entity, &components, self.0, false);
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

pub(super) struct Spawn<I> {
    pub(super) spawned: I,
    pub(super) marker: DisabledToDespawn,
}

impl<I: Send + 'static> UndoRedo for Spawn<I>
where
    I: AsRef<[Entity]>,
{
    fn undo(&mut self, world: &mut World) {
        world.insert_batch(
            self.spawned
                .as_ref()
                .into_iter()
                .rev()
                .map(|entity| (*entity, self.marker)),
        );
    }
    fn redo(&mut self, world: &mut World) {
        let id = world.component_id::<DisabledToDespawn>().expect("todo");
        for entity in self.spawned.as_ref().into_iter() {
            world.entity_mut(*entity).remove_by_id(id);
        }
    }
}

#[track_caller]
pub(super) fn rev_despawn_inner<'w>(
    mut entity_mut: EntityWorldMut<'w>,
    now: NonLogNow,
) -> Result<(), RevEntityError> {
    let entity = entity_mut.id();
    if entity_mut.is_despawned() {
        return match entity_mut.world().get_entity(entity) {
            Err(err) => Err(RevEntityError::EntityDoesNotExistError(err)),
            Ok(_) => unreachable!(),
        };
    }
    if let Some(marker) = entity_mut.get::<DisabledToDespawn>() {
        return Err(RevEntityError::EntityRevDespawnedError(
            EntityRevDespawnedError::new(entity, *marker),
        ));
    }

    let marker = DisabledToDespawn::for_spawn_despawn(now.0);
    let entity = entity_mut.id();
    let children = entity_mut
        .get::<Children>()
        .map(|children| RelationshipTarget::iter(children).collect::<Vec<Entity>>())
        .filter(|children| !children.is_empty());

    entity_mut.world_scope(|world| {
        let component_id = world
            .component_id::<DisabledToDespawn>()
            .unwrap_or_else(|| {
                warn!(
                    "the Component to reversibly mark entities as despawned is not known to the world, \
                    it is registered now but fulfills no disabling functionality as it should, \
                    make sure to add the {} plugin to the application",
                    type_name::<RevSystemsPlugin>()
                );
                world.register_component::<DisabledToDespawn>()
            });
        let Some(children) = children else {
            return rev_despawn_single(world, now, entity, marker);
        };
        let mut entities = [entity].into_iter().collect();
        for child in children {
            collect_children(world, child, component_id, &mut entities);
        }
        if entities.len() < 2 {
            return rev_despawn_single(world, now, entity, marker);
        }

        let mut undo_redo = RevDespawnHierarchy {
            entities: entities.into_iter().collect(),
            marker,
        };
        undo_redo.redo(world);
        world.buffer_undo_redo(now, undo_redo);
    });

    Ok(())
}

fn collect_children(
    world: &World,
    entity: Entity,
    component_id: ComponentId,
    entities: &mut EntityHashSet,
) {
    let entity_mut = world.entity(entity);
    if entity_mut.contains_id(component_id) {
        return;
    }
    if !entities.insert(entity) {
        return;
    }
    let Some(children) = entity_mut.get::<Children>() else {
        return;
    };
    for &child in children {
        collect_children(world, child, component_id, entities);
    }
}

fn rev_despawn_single(
    world: &mut World,
    now: NonLogNow,
    entity: Entity,
    marker: DisabledToDespawn,
) {
    let mut undo_redo = RevDespawnSingle { entity, marker };
    undo_redo.redo(world);
    world.buffer_undo_redo(now, undo_redo);
}

pub(super) struct RevDespawnSingle {
    pub(super) entity: Entity,
    pub(super) marker: DisabledToDespawn,
}

impl UndoRedo for RevDespawnSingle {
    fn undo(&mut self, world: &mut World) {
        world.entity_mut(self.entity).remove::<DisabledToDespawn>();
    }
    fn redo(&mut self, world: &mut World) {
        world.entity_mut(self.entity).insert(self.marker);
    }
}

struct RevDespawnHierarchy {
    entities: Arc<[Entity]>,
    marker: DisabledToDespawn,
}

impl UndoRedo for RevDespawnHierarchy {
    fn undo(&mut self, world: &mut World) {
        let component_id = world.component_id::<DisabledToDespawn>().expect("todo");
        let mut commands = world.commands();
        for &entity in self.entities.iter().rev() {
            commands.entity(entity).remove_by_id(component_id);
        }
        world.flush();
    }
    fn redo(&mut self, world: &mut World) {
        struct Iter {
            entities: Arc<[Entity]>,
            index: usize,
            marker: DisabledToDespawn,
        }

        impl Iterator for Iter {
            type Item = (Entity, DisabledToDespawn);
            fn next(&mut self) -> Option<Self::Item> {
                self.entities.get(self.index).map(|entity| {
                    self.index += 1;
                    (*entity, self.marker)
                })
            }
            fn size_hint(&self) -> (usize, Option<usize>) {
                let len = self.len();
                (len, Some(len))
            }
        }

        impl ExactSizeIterator for Iter {
            fn len(&self) -> usize {
                self.entities.len() - self.index
            }
        }

        impl FusedIterator for Iter {}

        world.insert_batch(Iter {
            entities: self.entities.clone(),
            index: 0,
            marker: self.marker,
        })
    }
}

#[cfg(test)]
mod test {
    /*
    use super::*;

    fn despawn_at_undo_try_new() {
        assert_eq!(
            PrecomputedDespawnAtUndo::try_new(&RevMeta::new(None, 0, false)).err(),
            Some(RevDirectionMismatch {
                actual: None
            })
        );
        assert_eq!(
            PrecomputedDespawnAtUndo::try_new(&RevDirection::FORWARD_LOG.to_meta(0, 1, 1)).err(),
            Some(RevDirectionMismatch {
                actual: Some(RevDirection::FORWARD_LOG)
            })
        );
        assert_eq!(
            PrecomputedDespawnAtUndo::try_new(&RevDirection::BackwardLog.to_meta(0, 0, 1)).err(),
            Some(RevDirectionMismatch {
                actual: Some(RevDirection::BackwardLog)
            })
        );
        assert_eq!(
            PrecomputedDespawnAtUndo::try_new(&RevDirection::NOT_LOG.to_meta(0, 1, 1)).err(),
            None
        );
    }

    #[test]
    fn rev_spawn_spawns() {
        let mut world = setup();
        world.spawn_empty().despawn(); // have a free entity in the world to be reused
        let entity_mut = world.rev_spawn((Explicit1(10), children![Explicit1(20)]));
        let entity2 = *entity_mut
            .get::<Children>()
            .unwrap()
            .into_iter()
            .next()
            .unwrap();
        let entity1 = entity_mut.id();
        let mut buffer = world.remove_resource::<UndoRedoBuffer>().unwrap();

        let [ref1, ref2] = world.entity([entity1, entity2]);

        assert_eq!(ref1.get(), Some(&Explicit1(10)));
        assert_eq!(ref1.get(), Some(&Required1(0)));
        assert_eq!(ref1.rev_is_despawned(), false);

        assert_eq!(ref2.get(), Some(&Explicit1(20)));
        assert_eq!(ref2.get(), Some(&Required1(0)));
        assert_eq!(ref2.rev_is_despawned(), false);

        buffer.undo(&mut world);
        let [ref1, ref2] = world.entity([entity1, entity2]);

        assert_eq!(ref1.rev_is_despawned(), true);
        assert_eq!(ref2.rev_is_despawned(), true);

        buffer.redo(&mut world);
        let [ref1, ref2] = world.entity([entity1, entity2]);

        assert_eq!(ref1.get(), Some(&Explicit1(10)));
        assert_eq!(ref1.get(), Some(&Required1(0)));
        assert_eq!(ref1.rev_is_despawned(), false);

        assert_eq!(ref2.get(), Some(&Explicit1(20)));
        assert_eq!(ref2.get(), Some(&Required1(0)));
        assert_eq!(ref2.rev_is_despawned(), false);
    }
     */
}
