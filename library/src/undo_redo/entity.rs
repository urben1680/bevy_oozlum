use std::{any::TypeId, marker::PhantomData};

use bevy::{
    ecs::{
        bundle::{Bundle, BundleFromComponents},
        component::{Component, ComponentId},
        entity::{Entity, EntityClonerBuilder},
        hierarchy::ChildOf,
        observer::TriggerTargets,
        relationship::{Relationship, RelationshipTarget},
    },
    ptr::OwningPtr,
};

use super::*;

pub trait RevEntityWorldMut<'w> {
    /// Reversible version of [`EntityWorldMut::insert`].
    fn rev_insert<T: Bundle>(&mut self, bundle: T) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::insert_if_new`].
    fn rev_insert_if_new<T: Bundle>(&mut self, bundle: T) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::insert_by_id`].
    ///
    /// # Safety
    ///
    /// - [`ComponentId`] must be from the same world as [`EntityWorldMut`]
    /// - [`OwningPtr`] must be a valid reference to the type represented by [`ComponentId`]
    unsafe fn rev_insert_by_id(
        &mut self,
        component_id: ComponentId,
        component: OwningPtr<'_>,
    ) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::insert_by_ids`].
    ///
    /// # Safety
    ///
    /// - Each [`ComponentId`] must be from the same world as [`EntityWorldMut`]
    /// - Each [`OwningPtr`] must be a valid reference to the type represented by [`ComponentId`]
    unsafe fn rev_insert_by_ids<'a, I: Iterator<Item = OwningPtr<'a>>>(
        &mut self,
        component_ids: &[ComponentId],
        iter_components: I,
    ) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::remove`].
    fn rev_remove<T: Bundle>(&mut self) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::remove_with_requires`].
    fn rev_remove_with_requires<T: Bundle>(&mut self) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::take`].
    fn rev_take<T: Bundle + BundleFromComponents + Clone>(&mut self) -> Option<T>;

    /// Reversible version of [`EntityWorldMut::retain`].
    fn rev_retain<T: Bundle>(&mut self) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::remove_by_id`].
    fn rev_remove_by_id(&mut self, component_id: ComponentId) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::remove_by_ids`].
    fn rev_remove_by_ids(&mut self, component_ids: &[ComponentId]) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::clear`].
    fn rev_clear(&mut self) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::despawn`].
    ///
    /// Note that this despawns the entity not now but later when this action goes out of log.
    ///
    /// Until then the entity is disabled via the [`DespawnAtOutOfLog`] component.
    fn rev_despawn(self);

    /// Reversible version of [`EntityWorldMut::is_despawned`].
    fn rev_is_despawned(&self) -> bool;

    /// Reversible version of [`EntityWorldMut::clone_and_spawn`].
    fn rev_clone_and_spawn(&mut self) -> Entity;

    /// Reversible version of [`EntityWorldMut::clone_and_spawn_with`].
    fn rev_clone_and_spawn_with(
        &mut self,
        config: impl FnOnce(&mut EntityClonerBuilder) + Send + Sync + 'static,
    ) -> Entity;

    /// Reversible version of [`EntityWorldMut::clone_components`].
    fn rev_clone_components<B: Bundle>(&mut self, target: Entity) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::move_components`].
    fn rev_move_components<B: Bundle>(&mut self, target: Entity) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::with_related`].
    fn rev_with_related<R: Relationship>(
        &mut self,
        func: impl FnOnce(&mut RevRelatedSpawner<R>),
    ) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::add_related`].
    fn rev_add_related<R: Relationship>(&mut self, related: &[Entity]) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::add_one_related`].
    fn rev_add_one_related<R: Relationship>(&mut self, entity: Entity) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::despawn_related`].
    fn rev_despawn_related<S: RelationshipTarget>(&mut self) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::insert_recursive`].
    fn rev_insert_recursive<S: RelationshipTarget>(
        &mut self,
        bundle: impl Bundle + Clone,
    ) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::remove_recursive`].
    fn rev_remove_recursive<S: RelationshipTarget, B: Bundle>(&mut self) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::with_children`].
    fn rev_with_children(
        &mut self,
        func: impl FnOnce(&mut RevRelatedSpawner<ChildOf>),
    ) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::add_children`].
    fn rev_add_children(&mut self, children: &[Entity]) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::add_child`].
    fn rev_add_child(&mut self, child: Entity) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::with_child`].
    fn rev_with_child(&mut self, bundle: impl Bundle) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::entry`].
    fn rev_entry<'a, T: Component>(&'a mut self) -> RevEntry<'w, 'a, T>;

    fn buffer_components(
        &mut self,
        at: BufferAt,
        components: Vec<ComponentId>,
    ) -> Option<EntityRef>;

    fn buffer_components_cached(
        &mut self,
        key: impl Hash + 'static,
        components: impl FnOnce(&mut World) -> (BufferAt, Vec<ComponentId>),
    ) -> Option<EntityRef>;
}

impl<'w> RevEntityWorldMut<'w> for EntityWorldMut<'w> {
    fn rev_insert<T: Bundle>(&mut self, bundle: T) -> &mut Self {
        let archetype_id = self.archetype().id();
        self.buffer_components_cached(
            unique_for_location!(archetype_id, TypeId::of::<T>()),
            |world: &mut World| {
                let bundle_id = world.register_bundle::<T>().id();
                maybe_overwrite_insert(world, bundle_id, archetype_id)
            },
        );
        self.insert(bundle)
    }

    fn rev_insert_if_new<T: Bundle>(&mut self, bundle: T) -> &mut Self {
        // Bundle explicit:  A(2), B(2), C(2)
        // Bundle required:                    D(2), E(2)

        // Entity before:    A(1), B(1),             E(1)
        // Entity after:     A(1), B(1), C(2), D(2), E(1)

        // Buffer at undo:               C(2), D(2)

        let archetype_id = self.archetype().id();
        self.buffer_components_cached(
            unique_for_location!(archetype_id, TypeId::of::<T>()),
            |world| {
                let bundle_id = world.register_bundle::<T>().id();
                let archetype = world.archetypes().get(archetype_id).unwrap();
                let components = world
                    .bundles()
                    .get(bundle_id)
                    .unwrap()
                    .contributed_components()
                    .iter()
                    .copied()
                    .filter(|component_id| !archetype.contains(*component_id))
                    .collect();
                (BufferAt::Undo, components)
            },
        );
        self.insert_if_new(bundle)
    }

    unsafe fn rev_insert_by_id(
        &mut self,
        component_id: ComponentId,
        component: OwningPtr<'_>,
    ) -> &mut Self {
        self.rev_insert_by_ids(&[component_id], [component].into_iter())
    }

    unsafe fn rev_insert_by_ids<'a, I: Iterator<Item = OwningPtr<'a>>>(
        &mut self,
        component_ids: &[ComponentId],
        iter_components: I,
    ) -> &mut Self {
        let archetype_id = self.archetype().id();
        let bundle_id = unsafe {
            // SAFETY: registering a bundle does not change the entity's location
            self.world_mut().register_dynamic_bundle(component_ids).id()
        };
        self.buffer_components_cached(
            unique_for_location!(archetype_id, bundle_id),
            |world: &mut World| maybe_overwrite_insert(world, bundle_id, archetype_id),
        );
        self.insert_by_ids(component_ids, iter_components)
    }

    fn rev_remove<T: Bundle>(&mut self) -> &mut Self {
        let archetype_id = self.archetype().id();
        self.buffer_components_cached(
            unique_for_location!(archetype_id, TypeId::of::<T>()),
            |world| {
                let bundle_id = world.register_bundle::<T>().id();
                let bundle = world.bundles().get(bundle_id).unwrap();
                let archetype = world.archetypes().get(archetype_id).unwrap();
                let components: Vec<_> = bundle
                    .explicit_components()
                    .iter()
                    .filter(|component_id| archetype.contains(**component_id))
                    .copied()
                    .collect();
                (BufferAt::Now, components)
            },
        );
        self
    }

    fn rev_remove_with_requires<T: Bundle>(&mut self) -> &mut Self {
        let archetype_id = self.archetype().id();
        self.buffer_components_cached(
            unique_for_location!(archetype_id, TypeId::of::<T>()),
            |world| {
                let bundle_id = world.register_bundle::<T>().id();
                let bundle = world.bundles().get(bundle_id).unwrap();
                let archetype = world.archetypes().get(archetype_id).unwrap();
                let components: Vec<_> = bundle
                    .contributed_components()
                    .iter()
                    .filter(|component_id| archetype.contains(**component_id))
                    .copied()
                    .collect();
                (BufferAt::Now, components)
            },
        );
        self
    }

    fn rev_take<T: Bundle + BundleFromComponents + Clone>(&mut self) -> Option<T> {
        let value = self.take::<T>()?;
        self.insert(value.clone()).rev_remove::<T>();
        Some(value)
    }

    fn rev_retain<T: Bundle>(&mut self) -> &mut Self {
        let archetype_id = self.archetype().id();
        self.buffer_components_cached(
            unique_for_location!(archetype_id, TypeId::of::<T>()),
            |world| {
                let contributed_components: HashSet<_> = world
                    .register_bundle::<T>()
                    .contributed_components()
                    .iter()
                    .copied()
                    .collect();
                let components = world
                    .archetypes()
                    .get(archetype_id)
                    .unwrap()
                    .components()
                    .filter(|component_id| !contributed_components.contains(component_id))
                    .collect();
                (BufferAt::Now, components)
            },
        );
        self
    }

    fn rev_remove_by_id(&mut self, component_id: ComponentId) -> &mut Self {
        if !self.contains_id(component_id) {
            return self;
        }
        self.buffer_components(BufferAt::Now, vec![component_id]);
        self
    }

    fn rev_remove_by_ids(&mut self, component_ids: &[ComponentId]) -> &mut Self {
        let components = component_ids
            .components()
            .filter(|component_id| self.archetype().contains(*component_id))
            .collect();
        self.buffer_components(BufferAt::Now, components);
        self
    }

    fn rev_clear(&mut self) -> &mut Self {
        let archetype_id = self.archetype().id();
        self.buffer_components_cached(unique_for_location!(archetype_id), |world| {
            let components = world
                .archetypes()
                .get(archetype_id)
                .unwrap()
                .components()
                .collect();
            (BufferAt::Now, components)
        });
        self
    }

    fn rev_despawn(self) {
        rev_despawn_inner(self);
    }

    fn rev_is_despawned(&self) -> bool {
        self.contains::<DespawnAtOutOfLog>()
    }

    fn rev_clone_and_spawn(&mut self) -> Entity {
        let meta = self.get_resource::<RevMeta>().expect("todo");
        let marker = DespawnAtOutOfLog::new(meta);
        let entity = self.clone_and_spawn();
        self.buffer_undo_redo(UndoRedoSwap(RevDespawnSingle { entity, marker }));
        entity
    }

    fn rev_clone_and_spawn_with(
        &mut self,
        config: impl FnOnce(&mut EntityClonerBuilder) + Send + Sync + 'static,
    ) -> Entity {
        let meta = self.get_resource::<RevMeta>().expect("todo");
        let marker = DespawnAtOutOfLog::new(meta);
        let entity = self.clone_and_spawn_with(config);
        self.buffer_undo_redo(UndoRedoSwap(RevDespawnSingle { entity, marker }));
        entity
    }

    fn rev_clone_components<B: Bundle>(&mut self, target: Entity) -> &mut Self {
        let archetype_id = self
            .world()
            .entities()
            .get(target)
            .expect("todo")
            .archetype_id;
        self.world_scope(|world| {
            world.buffer_components_cached(
                target,
                unique_for_location!(archetype_id, TypeId::of::<B>()),
                |world| {
                    let bundle_id = world.register_bundle::<B>().id();
                    maybe_overwrite_insert(&world, bundle_id, archetype_id)
                },
            );
        });
        self.clone_components::<B>(target)
    }

    fn rev_move_components<B: Bundle>(&mut self, target: Entity) -> &mut Self {
        // todo: when moving no longer requires Clone, replace this logic with non-Clone approach
        self.rev_clone_components::<B>(target).rev_remove::<B>()
    }

    fn rev_with_related<R: Relationship>(
        &mut self,
        func: impl FnOnce(&mut RevRelatedSpawner<R>),
    ) -> &mut Self {
        let parent = self.id();
        self.world_scope(|world| {
            func(&mut RevRelatedSpawner::new(world, parent));
        });
        self
    }

    fn rev_add_related<R: Relationship>(&mut self, related: &[Entity]) -> &mut Self {
        let id = self.id();
        self.world_scope(|world| {
            for related in related {
                world.entity_mut(*related).rev_insert(R::from(id));
            }
        });
        self
    }

    fn rev_add_one_related<R: Relationship>(&mut self, entity: Entity) -> &mut Self {
        self.rev_add_related::<R>(&[entity])
    }

    fn rev_despawn_related<S: RelationshipTarget>(&mut self) -> &mut Self {
        if let Some(sources) = self.get::<S>() {
            let sources: Vec<_> = sources.iter().collect();
            self.world_scope(|world| {
                for entity in sources.into_iter() {
                    if let Ok(entity_mut) = world.get_entity_mut(entity) {
                        entity_mut.rev_despawn();
                    }
                }
            });
        }
        self
    }

    fn rev_insert_recursive<S: RelationshipTarget>(
        &mut self,
        bundle: impl Bundle + Clone,
    ) -> &mut Self {
        self.rev_insert(bundle.clone());
        if let Some(relationship_target) = self.get::<S>() {
            let related_vec: Vec<Entity> = relationship_target.iter().collect();
            for related in related_vec {
                self.world_scope(|world| {
                    world
                        .entity_mut(related)
                        .rev_insert_recursive::<S>(bundle.clone());
                });
            }
        }

        self
    }

    fn rev_remove_recursive<S: RelationshipTarget, B: Bundle>(&mut self) -> &mut Self {
        self.rev_remove::<B>();
        if let Some(relationship_target) = self.get::<S>() {
            let related_vec: Vec<Entity> = relationship_target.iter().collect();
            for related in related_vec {
                self.world_scope(|world| {
                    world.entity_mut(related).rev_remove_recursive::<S, B>();
                });
            }
        }

        self
    }

    fn rev_with_children(
        &mut self,
        func: impl FnOnce(&mut RevRelatedSpawner<ChildOf>),
    ) -> &mut Self {
        self.rev_with_related(func);
        self
    }

    fn rev_add_children(&mut self, children: &[Entity]) -> &mut Self {
        self.rev_add_related::<ChildOf>(children);
        self
    }

    fn rev_add_child(&mut self, child: Entity) -> &mut Self {
        self.rev_add_related::<ChildOf>(&[child])
    }

    fn rev_with_child(&mut self, bundle: impl Bundle) -> &mut Self {
        let parent = self.id();
        self.world_scope(|world| {
            world.rev_spawn((bundle, ChildOf { parent }));
        });
        self
    }

    fn rev_entry<'a, T: Component>(&'a mut self) -> RevEntry<'w, 'a, T> {
        if self.contains::<T>() {
            RevEntry::Occupied(RevOccupiedEntry {
                entity_world: self,
                _marker: PhantomData,
            })
        } else {
            RevEntry::Vacant(RevVacantEntry {
                entity_world: self,
                _marker: PhantomData,
            })
        }
    }

    fn buffer_components(
        &mut self,
        at: BufferAt,
        components: Vec<ComponentId>,
    ) -> Option<EntityRef> {
        let entity = self.id();
        if at == BufferAt::Undo {
            unsafe {
                // SAFETY: No components of this entity are buffered yet,
                // only resources are mutated and a bundle is registered.
                self.world_mut().buffer_components(entity, at, components)
            }
        } else {
            let id = self.world_scope(|world| {
                world
                    .buffer_components(entity, at, components)
                    .map(|buffer| buffer.id())
            });
            id.map(|id| self.world().entity(id))
        }
    }

    fn buffer_components_cached(
        &mut self,
        key: impl Hash + 'static,
        components: impl FnOnce(&mut World) -> (BufferAt, Vec<ComponentId>),
    ) -> Option<EntityRef> {
        let entity = self.id();
        let id = self.world_scope(|world| {
            world
                .buffer_components_cached(entity, key, components)
                .map(|buffer| buffer.id())
        });
        id.map(|id| self.world().entity(id))
    }
}

pub struct RevRelatedSpawner<'w, R: Relationship> {
    target: Entity,
    world: &'w mut World,
    _marker: PhantomData<R>,
}

impl<'w, R: Relationship> RevRelatedSpawner<'w, R> {
    /// See [`RelatedSpawner::new`](bevy::ecs::relationship::RelatedSpawner::new).
    pub fn new(world: &'w mut World, target: Entity) -> Self {
        Self {
            world,
            target,
            _marker: PhantomData,
        }
    }

    /// See [`RelatedSpawner::spawn`](bevy::ecs::relationship::RelatedSpawner::spawn).
    pub fn spawn(&mut self, bundle: impl Bundle) -> EntityWorldMut<'_> {
        self.world.spawn((R::from(self.target), bundle))
    }

    /// Reversible version of [`RelatedSpawner::spawn`](bevy::ecs::relationship::RelatedSpawner::spawn).
    pub fn rev_spawn(&mut self, bundle: impl Bundle) -> EntityWorldMut<'_> {
        self.world.rev_spawn((R::from(self.target), bundle))
    }

    /// See [`RelatedSpawner::spawn_empty`](bevy::ecs::relationship::RelatedSpawner::spawn_empty).
    pub fn spawn_empty(&mut self) -> EntityWorldMut<'_> {
        self.world.spawn(R::from(self.target))
    }

    /// Reversible version of [`RelatedSpawner::spawn_empty`](bevy::ecs::relationship::RelatedSpawner::spawn_empty).
    pub fn rev_spawn_empty(&mut self) -> EntityWorldMut<'_> {
        self.world.rev_spawn(R::from(self.target))
    }

    /// See [`RelatedSpawner::target_entity`](bevy::ecs::relationship::RelatedSpawner::target_entity).
    pub fn target_entity(&self) -> Entity {
        self.target
    }
}

pub enum RevEntry<'w, 'a, T: Component> {
    /// An occupied entry.
    Occupied(RevOccupiedEntry<'w, 'a, T>),
    /// A vacant entry.
    Vacant(RevVacantEntry<'w, 'a, T>),
}

pub struct RevOccupiedEntry<'w, 'a, T> {
    entity_world: &'a mut EntityWorldMut<'w>,
    _marker: PhantomData<T>,
}

pub struct RevVacantEntry<'w, 'a, T> {
    entity_world: &'a mut EntityWorldMut<'w>,
    _marker: PhantomData<T>,
}

impl<'w, 'a, T: Component> RevEntry<'w, 'a, T> {
    /// See [`Entry::target_entity`](bevy::ecs::world::Entry::insert_entry).
    pub fn insert_entry(self, component: T) -> RevOccupiedEntry<'w, 'a, T> {
        match self {
            RevEntry::Occupied(mut entry) => {
                entry.insert(component);
                entry
            }
            RevEntry::Vacant(entry) => entry.insert(component),
        }
    }

    /// Reversible version of [`Entry::target_entity`](bevy::ecs::world::Entry::insert_entry).
    pub fn rev_insert_entry(self, component: T) -> RevOccupiedEntry<'w, 'a, T> {
        match self {
            RevEntry::Occupied(mut entry) => {
                entry.rev_insert(component);
                entry
            }
            RevEntry::Vacant(entry) => entry.rev_insert(component),
        }
    }

    /// See [`Entry::or_insert`](bevy::ecs::world::Entry::or_insert).
    pub fn or_insert(self, default: T) -> RevOccupiedEntry<'w, 'a, T> {
        match self {
            RevEntry::Occupied(entry) => entry,
            RevEntry::Vacant(entry) => entry.insert(default),
        }
    }

    /// Reversible version of [`Entry::or_insert`](bevy::ecs::world::Entry::or_insert).
    pub fn rev_or_insert(self, default: T) -> RevOccupiedEntry<'w, 'a, T> {
        match self {
            RevEntry::Occupied(entry) => entry,
            RevEntry::Vacant(entry) => entry.rev_insert(default),
        }
    }

    /// See [`Entry::or_insert_with`](bevy::ecs::world::Entry::or_insert_with).
    pub fn or_insert_with<F: FnOnce() -> T>(self, default: F) -> RevOccupiedEntry<'w, 'a, T> {
        match self {
            RevEntry::Occupied(entry) => entry,
            RevEntry::Vacant(entry) => entry.insert(default()),
        }
    }

    /// Reversible version of [`Entry::or_insert_with`](bevy::ecs::world::Entry::or_insert_with).
    pub fn rev_or_insert_with<F: FnOnce() -> T>(self, default: F) -> RevOccupiedEntry<'w, 'a, T> {
        match self {
            RevEntry::Occupied(entry) => entry,
            RevEntry::Vacant(entry) => entry.rev_insert(default()),
        }
    }
}

impl<'w, 'a, T: Component + Default> RevEntry<'w, 'a, T> {
    /// See [`Entry::or_insert_with`](bevy::ecs::world::Entry::or_default).
    pub fn or_default(self) -> RevOccupiedEntry<'w, 'a, T> {
        match self {
            RevEntry::Occupied(entry) => entry,
            RevEntry::Vacant(entry) => entry.insert(Default::default()),
        }
    }

    /// Reversible version of [`Entry::or_insert_with`](bevy::ecs::world::Entry::or_default).
    pub fn rev_or_default(self) -> RevOccupiedEntry<'w, 'a, T> {
        match self {
            RevEntry::Occupied(entry) => entry,
            RevEntry::Vacant(entry) => entry.rev_insert(Default::default()),
        }
    }
}

impl<'w, 'a, T: Component> RevOccupiedEntry<'w, 'a, T> {
    /// See [`OccupiedEntry::or_insert_with`](bevy::ecs::world::OccupiedEntry::insert).
    pub fn insert(&mut self, component: T) {
        self.entity_world.insert(component);
    }

    /// Reversible version of [`OccupiedEntry::or_insert_with`](bevy::ecs::world::OccupiedEntry::insert).
    pub fn rev_insert(&mut self, component: T) {
        self.entity_world.rev_insert(component);
    }

    /// See [`OccupiedEntry::take`](bevy::ecs::world::OccupiedEntry::take).
    pub fn take(self) -> T {
        // This shouldn't panic because if we have an OccupiedEntry the component must exist.
        self.entity_world.take().unwrap()
    }
}

impl<'w, 'a, T: Component + Clone> RevOccupiedEntry<'w, 'a, T> {
    /// Reversible version of [`OccupiedEntry::take`](bevy::ecs::world::OccupiedEntry::take).
    pub fn rev_take(self) -> T {
        // This shouldn't panic because if we have an OccupiedEntry the component must exist.
        self.entity_world.rev_take().unwrap()
    }
}

impl<'w, 'a, T: Component> RevVacantEntry<'w, 'a, T> {
    /// See [`VacantEntry::take`](bevy::ecs::world::VacantEntry::insert).
    pub fn insert(self, component: T) -> RevOccupiedEntry<'w, 'a, T> {
        self.entity_world.insert(component);
        RevOccupiedEntry {
            entity_world: self.entity_world,
            _marker: PhantomData,
        }
    }

    /// Reversible version of [`VacantEntry::take`](bevy::ecs::world::VacantEntry::insert).
    pub fn rev_insert(self, component: T) -> RevOccupiedEntry<'w, 'a, T> {
        self.entity_world.rev_insert(component);
        RevOccupiedEntry {
            entity_world: self.entity_world,
            _marker: PhantomData,
        }
    }
}

// todo: dont forget RevRelatedSpawnerCommands

fn maybe_overwrite_insert(
    world: &World,
    bundle_id: BundleId,
    archetype_id: ArchetypeId,
) -> (BufferAt, Vec<ComponentId>) {
    // Bundle explicit:  A(2), B(2), C(2)
    // Bundle required:                    D(2), E(2)

    // Entity before:    A(1), B(1),             E(1)
    // Entity after:     A(2), B(2), C(2), D(2), E(1)

    // Buffer 1:         A(1), B(1), C(*), D(*)        *if any appear at redo
    // Buffer 2 at undo: A(2), B(2), C(2), D(2)

    let bundle = world.bundles().get(bundle_id).unwrap();
    let archetype = world.archetypes().get(archetype_id).unwrap();
    let components = bundle
        .explicit_components()
        .iter()
        .chain(
            bundle
                .required_components()
                .iter()
                .filter(|component_id| !archetype.contains(**component_id)),
        )
        .copied()
        .collect();
    let overwrites = bundle
        .explicit_components()
        .iter()
        .any(|component_id| archetype.contains(*component_id));
    let at = if overwrites {
        BufferAt::NowAndUndo
    } else {
        BufferAt::Undo
    };
    (at, components)
}
