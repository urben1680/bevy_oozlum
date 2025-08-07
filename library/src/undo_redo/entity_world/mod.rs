use std::marker::PhantomData;

use bevy::ecs::{
    bundle::{Bundle, BundleFromComponents, InsertMode}, change_detection::MaybeLocation, component::Component, entity::{Entity, EntityClonerBuilder, EntityHashSet, OptIn, OptOut}, hierarchy::{ChildOf, Children}, relationship::{Relationship, RelationshipSourceCollection, RelationshipTarget}, world::{EntityWorldMut, World}
};

use crate::{meta::NonLogNow, undo_redo::EntityRevDespawnedError};

use super::{rev_spawn_finish, try_rev_clear, try_rev_insert, try_rev_remove, try_rev_retain, BuffersUndoRedo, RevDespawnCleaner, RevDespawnedBy, RevWorld, Spawn, Take, UndoRedo, UndoRedoSwap};

//#[cfg(test)]
//mod test;

// todo: mirror trait non-pub *WithCaller and use it for RevEntityWorldMut and RevEntityCommands
pub trait RevEntityWorldMut<'w> {
    fn redo_and_buffer(&mut self, now: NonLogNow, undo_redo: impl UndoRedo);

    // the methods here are purposely sorted alphabetically to make it easily comparable to bevy's docs
    // unmentioned methods are either
    // a) unrelated to reversible structural changes OR
    // b) deprecated in bevy OR
    // c) missed by accident!

    /// Reversible version of [`EntityWorldMut::add_child`].
    fn rev_add_child(&mut self, now: NonLogNow, child: Entity) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::add_children`].
    fn rev_add_children(&mut self, now: NonLogNow, children: &[Entity]) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::add_one_related`].
    fn rev_add_one_related<R: Relationship>(&mut self, now: NonLogNow, entity: Entity)
    -> &mut Self;

    /// Reversible version of [`EntityWorldMut::add_related`].
    fn rev_add_related<R: Relationship>(&mut self, now: NonLogNow, related: &[Entity])
    -> &mut Self;

    /// Reversible version of [`EntityWorldMut::clear`].
    fn rev_clear(&mut self, now: NonLogNow) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::clear_children`].
    fn rev_clear_children(&mut self, now: NonLogNow) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::clear_related`].
    fn rev_clear_related<R: Relationship>(&mut self, now: NonLogNow) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::clone_and_spawn`].
    ///
    /// Note that if `self` is in relationship with another entity, these relationship types need to be
    /// registered with [`RevApp::register_non_entity_buffer`](crate::app::RevApp::register_non_entity_buffer).
    /// Otherwise, at undo, the spawned entity will still be in relationship with the common
    /// `RelationshipTarget` despite being [temporarily despawned](DisabledToDespawn).
    fn rev_clone_and_spawn(&mut self, now: NonLogNow) -> Entity;

    /// Reversible version of [`EntityWorldMut::clone_and_spawn_with_opt_in`].
    ///
    /// Note that if `self` is in relationship with another entity, these relationship types need to be
    /// registered with [`RevApp::register_non_entity_buffer`](crate::app::RevApp::register_non_entity_buffer).
    /// Otherwise, at undo, the spawned entity will still be in relationship with the common
    /// `RelationshipTarget` despite being [temporarily despawned](DisabledToDespawn).
    fn rev_clone_and_spawn_with_opt_in(
        &mut self,
        now: NonLogNow,
        config: impl FnOnce(&mut EntityClonerBuilder<OptIn>) + Send + Sync + 'static,
    ) -> Entity;

    /// Reversible version of [`EntityWorldMut::clone_and_spawn_with_opt_out].
    ///
    /// Note that if `self` is in relationship with another entity, these relationship types need to be
    /// registered with [`RevApp::register_non_entity_buffer`](crate::app::RevApp::register_non_entity_buffer).
    /// Otherwise, at undo, the spawned entity will still be in relationship with the common
    /// `RelationshipTarget` despite being [temporarily despawned](DisabledToDespawn).
    fn rev_clone_and_spawn_with_opt_out(
        &mut self,
        now: NonLogNow,
        config: impl FnOnce(&mut EntityClonerBuilder<OptOut>) + Send + Sync + 'static,
    ) -> Entity;

    // rev_clone_components
    // out of scope

    // rev_clone_with
    // out of scope due complexity

    /// Reversible version of [`EntityWorldMut::despawn`].
    ///
    /// Note that this despawns the entity not now but later when this action goes out of log.
    fn rev_despawn(self, now: NonLogNow);

    /// Reversible version of [`EntityWorldMut::despawn_children`].
    fn rev_despawn_children(&mut self, now: NonLogNow) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::despawn_related`].
    fn rev_despawn_related<S: RelationshipTarget>(&mut self, now: NonLogNow) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::entry`].
    fn rev_entry<'a, T: Component>(&'a mut self) -> RevComponentEntry<'w, 'a, T>;

    /// Reversible version of [`EntityWorldMut::insert`].
    fn rev_insert<T: Bundle>(&mut self, now: NonLogNow, bundle: T) -> &mut Self;

    // rev_insert_by_id
    // out of scope

    // rev_insert_by_ids
    // out of scope

    // rev_insert_children
    // out of scope
    // todo: reevaluate

    /// Reversible version of [`EntityWorldMut::insert_if_new`].
    fn rev_insert_if_new<T: Bundle>(&mut self, now: NonLogNow, bundle: T) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::insert_recursive`].
    fn rev_insert_recursive<S: RelationshipTarget>(
        &mut self,
        now: NonLogNow,
        bundle: impl Bundle + Clone,
    ) -> &mut Self;

    // rev_insert_reflect
    // out of scope due complexity

    // rev_insert_reflect_with_registry
    // out of scope due complexity

    // rev_insert_related
    // out of scope
    // todo: reevaluate

    // rev_insert_with_relationship_hook_mode
    // missing EntityCloner API with RelationshipHookMode

    // rev_move_components
    // out of scope

    /// Reversible version of [`EntityWorldMut::remove`].
    fn rev_remove<T: Bundle>(&mut self, now: NonLogNow) -> &mut Self;

    // rev_remove_by_id
    // out of scope

    // rev_remove_by_ids
    // out of scope

    /// Reversible version of [`EntityWorldMut::remove_children`].
    fn rev_remove_children(&mut self, now: NonLogNow, children: &[Entity]) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::remove_recursive`].
    fn rev_remove_recursive<S: RelationshipTarget, B: Bundle>(
        &mut self,
        now: NonLogNow,
    ) -> &mut Self;

    // rev_remove_reflect
    // out of scope due complexity

    // rev_remove_reflect_with_registry
    // out of scope due complexity

    /// Reversible version of [`EntityWorldMut::remove_related`].
    fn rev_remove_related<R: Relationship>(
        &mut self,
        now: NonLogNow,
        related: &[Entity],
    ) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::remove_with_requires`].
    fn rev_remove_with_requires<T: Bundle>(&mut self, now: NonLogNow) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::replace_children`].
    fn rev_replace_children(&mut self, now: NonLogNow, related: &[Entity]) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::replace_children_with_difference`].
    fn rev_replace_children_with_difference(
        &mut self,
        now: NonLogNow,
        entities_to_unrelate: &[Entity],
        entities_to_relate: &[Entity],
        newly_related_entities: &[Entity],
    ) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::replace_related`].
    fn rev_replace_related<R: Relationship>(
        &mut self,
        now: NonLogNow,
        related: &[Entity],
    ) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::replace_related_with_difference`].
    fn rev_replace_related_with_difference<R: Relationship>(
        &mut self,
        now: NonLogNow,
        entities_to_unrelate: &[Entity],
        entities_to_relate: &[Entity],
        newly_related_entities: &[Entity],
    ) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::retain`].
    fn rev_retain<T: Bundle>(&mut self, now: NonLogNow) -> &mut Self;

    /// Reversible version of [`EntityWorldMut::take`].
    fn rev_take<'a, T: Bundle + BundleFromComponents, Out>(
        &'a mut self,
        now: NonLogNow,
        c: impl FnOnce(&T) -> Out,
    ) -> Option<Out>;

    /// Reversible version of [`EntityWorldMut::with_child`].
    fn rev_with_child(&mut self, now: NonLogNow, bundle: impl Bundle) -> &mut Self;

    // rev_with_children
    // implemented via DespawnAtUndo
    // todo: reevaluate

    /// Reversible version of [`EntityWorldMut::with_related`].
    fn rev_with_related<R: Relationship>(
        &mut self,
        now: NonLogNow,
        bundle: impl Bundle,
    ) -> &mut Self;

    // rev_with_related_entities
    // implemented via DespawnAtUndo
    // todo: reevaluate
}

impl<'w> RevEntityWorldMut<'w> for EntityWorldMut<'w> {
    fn redo_and_buffer(&mut self, now: NonLogNow, undo_redo: impl UndoRedo) {
        self.world_scope(|world| world.redo_and_buffer(now, undo_redo))
    }

    #[track_caller]
    fn rev_add_child(&mut self, now: NonLogNow, child: Entity) -> &mut Self {
        self.rev_add_one_related::<ChildOf>(now, child)
    }

    #[track_caller]
    fn rev_add_children(&mut self, now: NonLogNow, children: &[Entity]) -> &mut Self {
        self.rev_add_related::<ChildOf>(now, children)
    }

    #[track_caller]
    fn rev_add_one_related<R: Relationship>(
        &mut self,
        now: NonLogNow,
        entity: Entity,
    ) -> &mut Self {
        self.rev_add_related::<R>(now, &[entity])
    }

    #[track_caller]
    fn rev_add_related<R: Relationship>(
        &mut self,
        now: NonLogNow,
        related: &[Entity],
    ) -> &mut Self {
        let id = self.id();
        // todo: this does not pass MaybeLocation
        self.world_scope(|world| {
            for related in related {
                world.entity_mut(*related).rev_insert(now, R::from(id));
            }
        });
        self
    }

    #[track_caller]
    fn rev_clear(&mut self, now: NonLogNow) -> &mut Self {
        try_rev_clear(self, now, MaybeLocation::caller()).unwrap();
        self
    }

    #[track_caller]
    fn rev_clear_children(&mut self, now: NonLogNow) -> &mut Self {
        self.rev_clear_related::<ChildOf>(now)
    }

    #[track_caller]
    fn rev_clear_related<R: Relationship>(&mut self, now: NonLogNow) -> &mut Self {
        self.rev_remove::<R::RelationshipTarget>(now)
    }

    #[track_caller]
    fn rev_clone_and_spawn(&mut self, now: NonLogNow) -> Entity {
        self.rev_clone_and_spawn_with_opt_out(now, |_| {})
    }

    #[track_caller]
    fn rev_clone_and_spawn_with_opt_in(
        &mut self,
        now: NonLogNow,
        config: impl FnOnce(&mut EntityClonerBuilder<OptIn>) + Send + Sync + 'static,
    ) -> Entity {
        let clone = self.clone_and_spawn_with_opt_in(config);
        rev_spawn_finish(self, now, clone, MaybeLocation::caller());
        clone
    }

    #[track_caller]
    fn rev_clone_and_spawn_with_opt_out(
        &mut self,
        now: NonLogNow,
        config: impl FnOnce(&mut EntityClonerBuilder<OptOut>) + Send + Sync + 'static,
    ) -> Entity {
        let clone = self.clone_and_spawn_with_opt_out(config);
        rev_spawn_finish(self, now, clone, MaybeLocation::caller());
        clone
    }

    #[track_caller]
    fn rev_despawn(mut self, now: NonLogNow) {
        let entity = self.id();

        if let Some(location) = self.get_rev_despawned_by() {
            panic!("{}", EntityRevDespawnedError { entity, location });
        }

        let location = MaybeLocation::caller();

        self.redo_and_buffer(now, UndoRedoSwap(Spawn { entity, location }));
        self.resource_mut::<RevDespawnCleaner>()
            .log_despawn(entity, location, now);
    }

    #[track_caller]
    fn rev_despawn_children(&mut self, now: NonLogNow) -> &mut Self {
        self.rev_despawn_related::<Children>(now)
    }

    #[track_caller]
    fn rev_despawn_related<S: RelationshipTarget>(&mut self, now: NonLogNow) -> &mut Self {
        if let Some(sources) = self.get::<S>() {
            let sources: Vec<_> = sources.iter().collect();
            // todo: this does not pass MaybeLocation
            self.world_scope(|world| {
                for entity in sources.into_iter() {
                    if let Ok(entity_mut) = world.get_entity_mut(entity) {
                        entity_mut.rev_despawn(now);
                    }
                }
            });
        }
        self
    }

    fn rev_entry<'a, T: Component>(&'a mut self) -> RevComponentEntry<'w, 'a, T> {
        if self.contains::<T>() {
            RevComponentEntry::Occupied(RevOccupiedComponentEntry {
                entity_world_mut: self,
                _marker: PhantomData,
            })
        } else {
            RevComponentEntry::Vacant(RevVacantComponentEntry {
                entity_world_mut: self,
                _marker: PhantomData,
            })
        }
    }

    #[track_caller]
    fn rev_insert<T: Bundle>(&mut self, now: NonLogNow, bundle: T) -> &mut Self {
        try_rev_insert(
            self,
            bundle,
            InsertMode::Replace,
            now,
            MaybeLocation::caller(),
        )
        .unwrap();
        self
    }

    #[track_caller]
    fn rev_insert_if_new<T: Bundle>(&mut self, now: NonLogNow, bundle: T) -> &mut Self {
        try_rev_insert(self, bundle, InsertMode::Keep, now, MaybeLocation::caller()).unwrap();
        self
    }

    #[track_caller]
    fn rev_insert_recursive<S: RelationshipTarget>(
        &mut self,
        now: NonLogNow,
        bundle: impl Bundle + Clone,
    ) -> &mut Self {
        self.rev_insert(now, bundle.clone());
        if let Some(relationship_target) = self.get::<S>() {
            let related_vec: Vec<Entity> = relationship_target.iter().collect();
            for related in related_vec {
                // todo: this does not pass MaybeLocation
                self.world_scope(|world| {
                    world
                        .entity_mut(related)
                        .rev_insert_recursive::<S>(now, bundle.clone());
                });
            }
        }

        self
    }

    #[track_caller]
    fn rev_remove<T: Bundle>(&mut self, now: NonLogNow) -> &mut Self {
        try_rev_remove::<T, false>(self, now, MaybeLocation::caller()).unwrap();
        self
    }

    #[track_caller]
    fn rev_remove_children(&mut self, now: NonLogNow, children: &[Entity]) -> &mut Self {
        self.rev_remove_related::<ChildOf>(now, children)
    }

    #[track_caller]
    fn rev_remove_recursive<S: RelationshipTarget, B: Bundle>(
        &mut self,
        now: NonLogNow,
    ) -> &mut Self {
        self.rev_remove::<B>(now);
        if let Some(relationship_target) = self.get::<S>() {
            let related_vec: Vec<Entity> = relationship_target.iter().collect();
            for related in related_vec {
                // todo: this does not pass MaybeLocation
                self.world_scope(|world| {
                    world.entity_mut(related).rev_remove_recursive::<S, B>(now);
                });
            }
        }

        self
    }

    #[track_caller]
    fn rev_remove_related<R: Relationship>(
        &mut self,
        now: NonLogNow,
        related: &[Entity],
    ) -> &mut Self {
        let id = self.id();
        // todo: this does not pass MaybeLocation
        self.world_scope(|world| {
            for related in related {
                if world
                    .get::<R>(*related)
                    .is_some_and(|relationship| relationship.get() == id)
                {
                    world.entity_mut(*related).rev_remove::<R>(now);
                }
            }
        });

        self
    }

    #[track_caller]
    fn rev_remove_with_requires<T: Bundle>(&mut self, now: NonLogNow) -> &mut Self {
        try_rev_remove::<T, true>(self, now, MaybeLocation::caller()).unwrap();
        self
    }

    #[track_caller]
    fn rev_replace_children(&mut self, now: NonLogNow, related: &[Entity]) -> &mut Self {
        self.rev_replace_related::<ChildOf>(now, related)
    }

    #[track_caller]
    fn rev_replace_children_with_difference(
        &mut self,
        now: NonLogNow,
        entities_to_unrelate: &[Entity],
        entities_to_relate: &[Entity],
        newly_related_entities: &[Entity],
    ) -> &mut Self {
        self.rev_replace_related_with_difference::<ChildOf>(
            now,
            entities_to_unrelate,
            entities_to_relate,
            newly_related_entities,
        )
    }

    #[track_caller]
    fn rev_replace_related<R: Relationship>(
        &mut self,
        now: NonLogNow,
        related: &[Entity],
    ) -> &mut Self {
        struct ReplaceRelated<R: Relationship> {
            entity: Entity,
            related: Box<[Entity]>,
            undo_at: usize,
            _p: PhantomData<R>,
        }

        impl<R: Relationship> UndoRedo for ReplaceRelated<R> {
            fn undo(&mut self, world: &mut World) {
                world
                    .entity_mut(self.entity)
                    .replace_related::<R>(&self.related[self.undo_at..]);
            }
            fn redo(&mut self, world: &mut World) {
                world
                    .entity_mut(self.entity)
                    .replace_related::<R>(&self.related[..self.undo_at]);
            }
        }

        if related.is_empty() {
            self.rev_remove::<R::RelationshipTarget>(now);
            return self;
        }

        let Some(existing_relations) = self.get::<R::RelationshipTarget>() else {
            return self.rev_add_related::<R>(now, related);
        };

        let mut removed_relations = EntityHashSet::from_iter(existing_relations.iter());
        let mut related = related
            .iter()
            .copied()
            .filter(|entity| !removed_relations.remove(*entity))
            .collect::<Vec<_>>();
        let undo_at = related.len();
        related.extend(removed_relations.into_iter());

        let entity = self.id();
        self.redo_and_buffer(
            now,
            ReplaceRelated {
                entity,
                related: related.into_boxed_slice(),
                undo_at,
                _p: PhantomData::<R>,
            },
        );

        self
    }

    #[track_caller]
    fn rev_replace_related_with_difference<R: Relationship>(
        &mut self,
        now: NonLogNow,
        entities_to_unrelate: &[Entity],
        entities_to_relate: &[Entity],
        newly_related_entities: &[Entity],
    ) -> &mut Self {
        struct ReplaceRelatedWithDifference<R: Relationship> {
            entity: Entity,
            related: Box<[Entity]>,        // [R, R, S, S, N, N]
            entities_to_unrelate: usize,   // ------^
            newly_related_entities: usize, // ------------^
            _p: PhantomData<R>,
        }

        impl<R: Relationship> UndoRedo for ReplaceRelatedWithDifference<R> {
            fn undo(&mut self, world: &mut World) {
                world
                    .entity_mut(self.entity)
                    .replace_related_with_difference::<R>(
                        &self.related[self.newly_related_entities..],
                        &self.related[self.entities_to_unrelate..],
                        &self.related[..self.entities_to_unrelate],
                    );
            }
            fn redo(&mut self, world: &mut World) {
                world
                    .entity_mut(self.entity)
                    .replace_related_with_difference::<R>(
                        &self.related[..self.entities_to_unrelate],
                        &self.related[self.entities_to_unrelate..],
                        &self.related[self.newly_related_entities..],
                    );
            }
        }

        if !self.contains::<R::RelationshipTarget>() {
            self.rev_add_related::<R>(now, entities_to_relate);
            return self;
        };

        let mut staying = EntityHashSet::from_iter(entities_to_relate.iter().copied());
        for &entity in newly_related_entities {
            staying.remove(entity);
        }

        let mut related = Vec::with_capacity(entities_to_unrelate.len() + entities_to_relate.len());
        related.extend_from_slice(entities_to_unrelate);
        related.extend(staying.into_iter());
        related.extend_from_slice(newly_related_entities);

        let entity = self.id();
        self.redo_and_buffer(
            now,
            ReplaceRelatedWithDifference {
                entity,
                related: related.into_boxed_slice(),
                entities_to_unrelate: entities_to_unrelate.len(),
                newly_related_entities: entities_to_unrelate.len() + entities_to_relate.len()
                    - newly_related_entities.len(),
                _p: PhantomData::<R>,
            },
        );

        self
    }

    #[track_caller]
    fn rev_retain<T: Bundle>(&mut self, now: NonLogNow) -> &mut Self {
        try_rev_retain::<T>(self, now, MaybeLocation::caller()).unwrap();
        self
    }

    #[track_caller]
    fn rev_take<'a, T: Bundle + BundleFromComponents, Out>(
        &'a mut self,
        now: NonLogNow,
        c: impl FnOnce(&T) -> Out,
    ) -> Option<Out> {
        self.take::<T>().map(|value| {
            let out = c(&value);
            let entity = self.id();
            self.buffer_undo_redo(
                now,
                Take {
                    bundle: Some(value),
                    entity,
                },
            );
            out
        })
    }

    #[track_caller]
    fn rev_with_child(&mut self, now: NonLogNow, bundle: impl Bundle) -> &mut Self {
        self.rev_with_related::<ChildOf>(now, bundle)
    }

    #[track_caller]
    fn rev_with_related<R: Relationship>(
        &mut self,
        now: NonLogNow,
        bundle: impl Bundle,
    ) -> &mut Self {
        let parent = self.id();
        self.world_scope(|world| {
            world.rev_spawn(now, (bundle, R::from(parent)));
        });
        self
    }
}

// todo: upstream public EntityWorldMut getter for vanilla types
pub enum RevComponentEntry<'w, 'a, T: Component> {
    /// An occupied entry.
    Occupied(RevOccupiedComponentEntry<'w, 'a, T>),
    /// A vacant entry.
    Vacant(RevVacantComponentEntry<'w, 'a, T>),
}

pub struct RevOccupiedComponentEntry<'w, 'a, T> {
    entity_world_mut: &'a mut EntityWorldMut<'w>,
    _marker: PhantomData<T>,
}

pub struct RevVacantComponentEntry<'w, 'a, T> {
    entity_world_mut: &'a mut EntityWorldMut<'w>,
    _marker: PhantomData<T>,
}

impl<'w, 'a, T: Component> RevComponentEntry<'w, 'a, T> {
    /// See [`Entry::target_entity`](bevy::ecs::world::Entry::insert_entry).
    #[track_caller]
    pub fn insert_entry(self, component: T) -> RevOccupiedComponentEntry<'w, 'a, T> {
        match self {
            RevComponentEntry::Occupied(mut entry) => {
                entry.insert(component);
                entry
            }
            RevComponentEntry::Vacant(entry) => entry.insert(component),
        }
    }

    /// Reversible version of [`Entry::target_entity`](bevy::ecs::world::Entry::insert_entry).
    #[track_caller]
    pub fn rev_insert_entry(
        self,
        now: NonLogNow,
        component: T,
    ) -> RevOccupiedComponentEntry<'w, 'a, T> {
        match self {
            RevComponentEntry::Occupied(mut entry) => {
                entry.rev_insert(now, component);
                entry
            }
            RevComponentEntry::Vacant(entry) => entry.rev_insert(now, component),
        }
    }

    /// See [`Entry::or_insert`](bevy::ecs::world::Entry::or_insert).
    #[track_caller]
    pub fn or_insert(self, default: T) -> RevOccupiedComponentEntry<'w, 'a, T> {
        match self {
            RevComponentEntry::Occupied(entry) => entry,
            RevComponentEntry::Vacant(entry) => entry.insert(default),
        }
    }

    /// Reversible version of [`Entry::or_insert`](bevy::ecs::world::Entry::or_insert).
    #[track_caller]
    pub fn rev_or_insert(self, now: NonLogNow, default: T) -> RevOccupiedComponentEntry<'w, 'a, T> {
        match self {
            RevComponentEntry::Occupied(entry) => entry,
            RevComponentEntry::Vacant(entry) => entry.rev_insert(now, default),
        }
    }

    /// See [`Entry::or_insert_with`](bevy::ecs::world::Entry::or_insert_with).
    #[track_caller]
    pub fn or_insert_with<F: FnOnce() -> T>(
        self,
        default: F,
    ) -> RevOccupiedComponentEntry<'w, 'a, T> {
        match self {
            RevComponentEntry::Occupied(entry) => entry,
            RevComponentEntry::Vacant(entry) => entry.insert(default()),
        }
    }

    /// Reversible version of [`Entry::or_insert_with`](bevy::ecs::world::Entry::or_insert_with).
    #[track_caller]
    pub fn rev_or_insert_with<F: FnOnce() -> T>(
        self,
        now: NonLogNow,
        default: F,
    ) -> RevOccupiedComponentEntry<'w, 'a, T> {
        match self {
            RevComponentEntry::Occupied(entry) => entry,
            RevComponentEntry::Vacant(entry) => entry.rev_insert(now, default()),
        }
    }
}

impl<'w, 'a, T: Component + Default> RevComponentEntry<'w, 'a, T> {
    /// See [`Entry::or_insert_with`](bevy::ecs::world::Entry::or_default).
    #[track_caller]
    pub fn or_default(self) -> RevOccupiedComponentEntry<'w, 'a, T> {
        match self {
            RevComponentEntry::Occupied(entry) => entry,
            RevComponentEntry::Vacant(entry) => entry.insert(Default::default()),
        }
    }

    /// Reversible version of [`Entry::or_insert_with`](bevy::ecs::world::Entry::or_default).
    #[track_caller]
    pub fn rev_or_default(self, now: NonLogNow) -> RevOccupiedComponentEntry<'w, 'a, T> {
        match self {
            RevComponentEntry::Occupied(entry) => entry,
            RevComponentEntry::Vacant(entry) => entry.rev_insert(now, Default::default()),
        }
    }
}

impl<'w, 'a, T: Component> RevOccupiedComponentEntry<'w, 'a, T> {
    /// See [`OccupiedEntry::or_insert_with`](bevy::ecs::world::OccupiedEntry::insert).
    #[track_caller]
    pub fn insert(&mut self, component: T) {
        self.entity_world_mut.insert(component);
    }

    /// Reversible version of [`OccupiedEntry::or_insert_with`](bevy::ecs::world::OccupiedEntry::insert).
    #[track_caller]
    pub fn rev_insert(&mut self, now: NonLogNow, component: T) {
        self.entity_world_mut.rev_insert(now, component);
    }

    /// See [`OccupiedEntry::take`](bevy::ecs::world::OccupiedEntry::take).
    #[track_caller]
    pub fn take(self) -> T {
        // This shouldn't panic because if we have an OccupiedEntry the component must exist.
        self.entity_world_mut.take().unwrap()
    }

    /// Reversible version of [`OccupiedEntry::take`](bevy::ecs::world::OccupiedEntry::take).
    #[track_caller]
    pub fn rev_take<Out>(self, now: NonLogNow, c: impl FnOnce(&T) -> Out) -> Out {
        // This shouldn't panic because if we have an OccupiedEntry the component must exist.
        self.entity_world_mut.rev_take::<T, Out>(now, c).unwrap()
    }
}

impl<'w, 'a, T: Component> RevVacantComponentEntry<'w, 'a, T> {
    /// See [`VacantEntry::take`](bevy::ecs::world::VacantEntry::insert).
    #[track_caller]
    pub fn insert(self, component: T) -> RevOccupiedComponentEntry<'w, 'a, T> {
        self.entity_world_mut.insert(component);
        RevOccupiedComponentEntry {
            entity_world_mut: self.entity_world_mut,
            _marker: PhantomData,
        }
    }

    /// Reversible version of [`VacantEntry::take`](bevy::ecs::world::VacantEntry::insert).
    #[track_caller]
    pub fn rev_insert(self, now: NonLogNow, component: T) -> RevOccupiedComponentEntry<'w, 'a, T> {
        self.entity_world_mut.rev_insert(now, component);
        RevOccupiedComponentEntry {
            entity_world_mut: self.entity_world_mut,
            _marker: PhantomData,
        }
    }
}
