use super::*;

pub(super) fn pre_insert<T: Bundle>(
    world: &mut World,
    entity: Entity,
    archetype_id: ArchetypeId,
    insert_mode: InsertMode,
) {
    match insert_mode {
        InsertMode::Replace => world.buffer_components_cached(
            entity,
            unique_for_location!(archetype_id, PhantomData::<T>),
            |world: &mut World| {
                let bundle_id = world.register_bundle::<T>().id();
                insert_maybe_overwrite(world, bundle_id, archetype_id)
            },
        ),
        InsertMode::Keep => world.buffer_components_cached(
            entity,
            unique_for_location!(archetype_id, PhantomData::<T>),
            |world| {
                let bundle_id = world.register_bundle::<T>().id();
                insert_no_overwrite(&world, bundle_id, archetype_id)
            },
        ),
    };
}

pub(super) fn insert_maybe_overwrite(
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

pub(super) fn insert_no_overwrite(
    world: &World,
    bundle_id: BundleId,
    archetype_id: ArchetypeId,
) -> (BufferAt, Vec<ComponentId>) {
    // Bundle explicit:  A(2), B(2), C(2)
    // Bundle required:                    D(2), E(2)

    // Entity before:    A(1), B(1),             E(1)
    // Entity after:     A(1), B(1), C(2), D(2), E(1)

    // Buffer at undo:               C(2), D(2)

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
}

#[derive(Resource, Default)]
struct NonEntityBufferRes(HashMap<ComponentId, fn(&mut World, Entity, BufferAt)>);

fn non_entity_buffer(world: &mut World, entity: Entity, at: BufferAt, components: &[ComponentId]) {
    if !world.contains_resource::<NonEntityBufferRes>() {
        return;
    }
    world.resource_scope(|world, non_entity_buffers: Mut<NonEntityBufferRes>| {
        for component in components.iter() {
            if let Some(c) = non_entity_buffers.0.get(component) {
                c(world, entity, at);
            }
        }
    })
}

pub(crate) fn register_non_entity_buffer<T: Component>(world: &mut World) {
    struct NonEntityBuffer<T: Component> {
        entity: Entity,
        component: Option<T>,
    }

    impl<T: Component> UndoRedo for NonEntityBuffer<T> {
        fn undo(&mut self, world: &mut World) {
            let mut entity = world.entity_mut(self.entity);
            if T::Mutability::MUTABLE {
                let component = unsafe {
                    // SAFETY: this if branch asserts the component is mutable
                    entity.get_mut_assume_mutable::<T>()
                };
                match component {
                    Some(mut c1) => match self.component.as_mut() {
                        Some(c2) => core::mem::swap(&mut *c1, c2),
                        None => self.component = entity.take::<T>(),
                    },
                    None => {
                        if let Some(c2) = self.component.take() {
                            entity.insert(c2);
                        }
                    }
                }
            } else {
                match entity.take::<T>() {
                    Some(mut c1) => match self.component.as_mut() {
                        Some(c2) => {
                            core::mem::swap(&mut c1, c2);
                            entity.insert(c1);
                        }
                        None => self.component = Some(c1),
                    },
                    None => {
                        if let Some(c2) = self.component.take() {
                            entity.insert(c2);
                        }
                    }
                }
            }
        }
        fn redo(&mut self, world: &mut World) {
            self.undo(world);
        }
    }

    let component_id = world.register_component::<T>();
    world.get_resource_or_init::<NonEntityBufferRes>().0.insert(
        component_id,
        |world, entity, at| {
            let mut component = None;
            if matches!(at, BufferAt::Now | BufferAt::NowAndUndo) {
                component = world.entity_mut(entity).take::<T>();
            }
            let undo_redo = NonEntityBuffer { entity, component };
            world.buffer_undo_redo(undo_redo);
        },
    );
}

// todo: consider make this a RevWorld method
pub(super) fn buffer_bundle(
    world: &mut World,
    entity: Entity,
    at: BufferAt,
    bundle: BundleId,
) -> Option<Entity> {
    let mut buffer = BundleBuffer::new(world, entity, bundle);
    match at {
        BufferAt::Now => {
            let entities = buffer.toggle_state(world);
            let components = buffer.get_component_ids(world);
            non_entity_buffer(world, entity, at, &components);
            let out = buffer.move_bundle(world, entities, &components);
            world.buffer_undo_redo(buffer);
            Some(out)
        }
        BufferAt::Undo => {
            let components = buffer.get_component_ids(world);
            non_entity_buffer(world, entity, at, &components);
            world.buffer_undo_redo(buffer);
            None
        }
        BufferAt::NowAndUndo => {
            let at_undo = buffer.clone(); // no buffer entity set yet so each spawns their own
            let entities = buffer.toggle_state(world);
            let components = buffer.get_component_ids(world);
            non_entity_buffer(world, entity, at, &components);
            let out = buffer.move_bundle(world, entities, &components);
            world.buffer_undo_redo(buffer).buffer_undo_redo(at_undo);
            Some(out)
        }
    }
}

#[derive(Clone)]
struct BundleBuffer {
    bundle: BundleId,
    entity: Entity,
    state: BufferState,
}

#[derive(Clone)]
enum BufferState {
    Unspawned(DespawnAtOutOfLog),
    Empty(Entity),
    Filled(Entity),
}

struct BundleEntities {
    target: Entity,
    source: Entity,
    buffer: Entity,
}

impl BundleBuffer {
    fn new(world: &World, entity: Entity, bundle: BundleId) -> Self {
        let meta = world
            .get_resource::<RevMeta>()
            .expect(RevMeta::EXPECT_IN_WORLD);
        let marker = DespawnAtOutOfLog::new(meta);
        Self {
            bundle,
            entity,
            state: BufferState::Unspawned(marker),
        }
    }
    fn toggle_state(&mut self, world: &mut World) -> BundleEntities {
        match self.state {
            BufferState::Unspawned(marker) => {
                let buffer = world.spawn(marker).id();
                self.state = BufferState::Filled(buffer);
                BundleEntities {
                    target: buffer,
                    source: self.entity,
                    buffer,
                }
            }
            BufferState::Filled(buffer) => {
                self.state = BufferState::Empty(buffer);
                BundleEntities {
                    target: self.entity,
                    source: buffer,
                    buffer,
                }
            }
            BufferState::Empty(buffer) => {
                self.state = BufferState::Filled(buffer);
                BundleEntities {
                    target: buffer,
                    source: self.entity,
                    buffer,
                }
            }
        }
    }
    fn get_component_ids(&self, world: &World) -> Box<[ComponentId]> {
        world
            .bundles()
            .get(self.bundle)
            .expect("todo")
            .explicit_components()
            .into()
    }
    fn move_bundle(
        &mut self,
        world: &mut World,
        entities: BundleEntities,
        components: &[ComponentId],
    ) -> Entity {
        let progress_res = world.buffer_components_in_progress();
        if !progress_res {
            world.insert_resource(BufferComponentsInProgress);
        }
        EntityCloner::build(world)
            .deny_all()
            .move_components(true)
            .without_required_components(|builder| {
                builder.allow_by_ids(components.iter().copied());
            })
            .clone_entity(entities.source, entities.target);
        if !progress_res {
            world.remove_resource::<BufferComponentsInProgress>();
        }
        entities.buffer
    }
}

impl UndoRedo for BundleBuffer {
    fn undo(&mut self, world: &mut World) {
        let entities = self.toggle_state(world);
        let components = self.get_component_ids(world);
        self.move_bundle(world, entities, &components);
    }
    fn redo(&mut self, world: &mut World) {
        self.undo(world);
    }
}

#[derive(Resource)]
pub(crate) struct BufferComponentsInProgress;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BufferAt {
    /// The components will be buffered now which removes them from the entity.
    ///
    /// When this is undone, the components are moved back into the entity from the buffer.
    ///
    /// Redoing this results in buffering and removing them from the entity again.
    ///
    /// This variant is useful as reversible removals of these components.
    ///
    /// **Make sure to not manually remove the components and solely use this buffering.**
    Now,
    /// The components will be buffered when this action is undone, which then removed them
    /// from the entity. Until then they remain at the entity.
    ///
    /// When this id redone, the components are moved back into the entity from the buffer.
    ///
    /// This variant is useful to make accompanied insertions of these components _without_
    /// overwrites reversible.
    Undo,
    /// Combines [`BufferAt::Now`] and [`BufferAt::Undo`], utilizing two separate buffers.
    ///
    /// This variant is useful to make accompanied insertions of these components _with_
    /// overwrites reversible.
    ///
    /// **Make sure to do such insertions right after and not before this buffering.**
    NowAndUndo,
}

// todo: replace this with register_dynamic_bundle when moving components no longer requires cloning
pub(super) fn components_to_bundle(world: &mut World, components: &[ComponentId]) -> BundleId {
    #[derive(Resource, Default)]
    struct CheckedClonable(HashSet<ComponentId>);

    let mut checked = world
        .remove_resource::<CheckedClonable>()
        .unwrap_or_default();
    for &component_id in components {
        if checked.0.insert(component_id) {
            if let Some(component_info) = world.components().get_info(component_id) {
                if component_info.clone_behavior() == &ComponentCloneBehavior::Ignore {
                    bevy::log::error!(
                        "Component {} is unclonable, it's insert, remove or overwrite will \
                        not be reversible, see https://github.com/bevyengine/bevy/issues/18079",
                        component_info.name()
                    );
                }
            }
        }
    }
    world.insert_resource(checked);

    world.register_dynamic_bundle(components).id()
}

#[macro_export]
macro_rules! unique_for_location {
    ($($hashable: expr),*) => {
        // extra scope to keep `UniquePerInvoke`s isolated
        {
            struct UniquePerInvoke;
            (core::any::TypeId::of::<UniquePerInvoke>(), $($hashable,)*)
        }
    }
}

pub(super) use unique_for_location;
