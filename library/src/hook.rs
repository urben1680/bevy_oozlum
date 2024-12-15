use bevy::{
    ecs::{
        component::ComponentId,
        entity::Entity,
        system::Resource,
        world::{DeferredWorld, World},
    },
    utils::HashMap,
};

use crate::meta::{RevDirection, RevMeta};

use super::commands::{buffer_rev_command, RevCommandInit};

/// The direction the current hook is triggered at.
///
/// # Triggers //todo: put documentation into variant docs
///
/// | variant | description |
/// | - | - |
/// | `NotLog` | Triggered by reversible systems in the forward schedule (non-log). This follows bevy's hook logic. |
/// | `ForwardLog` | Triggered by reversible systems in the forward schedule (log). This **does not** follow bevy's hook logic and instead is a reversible command. Still this is triggered right after `IndeterministicForward`, see it's description. |
/// | `BackwardLog` | Triggered by reversible systems in the Backward schedule (log). This **does not** follow bevy's hook logic and instead is a reversible command. For example the `on_remove` hook with this indicates no remove but an undone remove. |
/// | `IndeterministicForward` |
///
/// Reversible systems that trigger a hook during non-log
///
/// Note that reversible logic should done at `Forward` and `BackwardLog`.
/// The indeterministic variants are triggered by the hook itself a
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HookDirection {
    /// Triggered by reversible systems in the forward schedule. This follows bevy's hook logic.
    Forward { log: bool },
    /// Triggered by reversible systems in the Backward schedule (log). This **does not** follow
    /// bevy's hook logic and instead is a reversible command. For example, the `on_remove` hook
    /// with this indicates no remove but an insert. If an `on_insert` hook was registered as well,
    /// it will be triggered right before this one with `IndeterministicBackward`.
    BackwardLog,
    /// Triggered by reversible systems in the forward schedule. This follows bevy's hook logic.
    /// Reversible logic should not be done with this variant.
    IndeterministicForwardLog,
    /// Triggered by reversible systems in the backward schedule. This follows bevy's hook logic.
    /// Reversible logic should not be done with this variant.
    IndeterministicBackwardLog,
    /// Triggered at any point outside reversible systems.
    /// Reversible logic should not be done with this variant.
    NonReversibleSchedule,
}

impl From<RevDirection> for HookDirection {
    fn from(value: RevDirection) -> Self {
        match value {
            RevDirection::NotLog => Self::NotLog,
            RevDirection::ForwardLog => Self::ForwardLog,
            RevDirection::BackwardLog => Self::BackwardLog,
        }
    }
}

impl TryFrom<HookDirection> for RevDirection {
    type Error = Option<RevDirection>;
    fn try_from(value: HookDirection) -> Result<Self, Self::Error> {
        match value {
            HookDirection::NotLog => Ok(RevDirection::NotLog),
            HookDirection::ForwardLog => Ok(RevDirection::ForwardLog),
            HookDirection::BackwardLog => Ok(RevDirection::BackwardLog),
            HookDirection::IndeterministicForwardLog => Err(Some(RevDirection::ForwardLog)),
            HookDirection::IndeterministicBackwardLog => Err(Some(RevDirection::BackwardLog)),
            HookDirection::NonReversibleSchedule => Err(None),
        }
    }
}

impl HookDirection {
    #[allow(non_upper_case_globals)]
    pub const NotLog: Self = Self::Forward { log: false };
    #[allow(non_upper_case_globals)]
    pub const ForwardLog: Self = Self::Forward { log: true };
    fn get_in_hook(world: &DeferredWorld) -> Self {
        match world
            .get_resource::<RevMeta>()
            .and_then(RevMeta::get_direction)
        {
            Some(RevDirection::NotLog) => Self::Forward { log: false },
            Some(RevDirection::ForwardLog) => Self::IndeterministicForwardLog,
            Some(RevDirection::BackwardLog) => Self::IndeterministicBackwardLog,
            None => Self::NonReversibleSchedule,
        }
    }
}

pub struct RevComponentHooks<'a> {
    pub(crate) world: &'a mut World,
    pub(crate) component: ComponentId,
}

#[derive(Resource, Default)]
struct ComponentHooksResource {
    hooks: HashMap<ComponentId, ComponentHookEntry>,
}

#[derive(Default)]
struct ComponentHookEntry {
    on_add: Option<RevComponentHook>,
    on_insert: Option<RevComponentHook>,
    on_replace: Option<RevComponentHook>,
    on_remove: Option<RevComponentHook>,
}

pub type RevComponentHook =
    for<'w> fn(_: HookDirection, _: DeferredWorld<'w>, _: Entity, _: ComponentId);

impl<'a> RevComponentHooks<'a> {
    pub fn on_add(&mut self, hook: RevComponentHook) -> &mut Self {
        let mut resource = self
            .world
            .get_resource_or_insert_with(ComponentHooksResource::default);
        let hooks = resource.hooks.entry(self.component).or_default();
        if hooks.on_add.is_some() {
            todo!()
        }
        hooks.on_add = Some(hook);

        self.world
            .register_component_hooks_by_id(self.component)
            .expect("todo")
            .on_add(|mut world, entity, component| {
                let direction = HookDirection::get_in_hook(&world);
                if direction == (HookDirection::Forward { log: false }) {
                    buffer_rev_command(
                        &mut world,
                        HookCommand {
                            entity,
                            component,
                            variant: HookVariant::OnAdd,
                        },
                    )
                }
                // todo: filter on parking entities
                world
                    .get_resource::<ComponentHooksResource>()
                    .expect("todo")
                    .hooks
                    .get(&component)
                    .expect("todo")
                    .on_add
                    .expect("todo")(direction, world, entity, component);
            });

        self
    }
    pub fn on_insert(&mut self, hook: RevComponentHook) -> &mut Self {
        let mut resource = self
            .world
            .get_resource_or_insert_with(ComponentHooksResource::default);
        let hooks = resource.hooks.entry(self.component).or_default();
        if hooks.on_insert.is_some() {
            todo!()
        }
        hooks.on_insert = Some(hook);

        self.world
            .register_component_hooks_by_id(self.component)
            .expect("todo")
            .on_insert(|mut world, entity, component| {
                let direction = HookDirection::get_in_hook(&world);
                if direction == (HookDirection::Forward { log: false }) {
                    buffer_rev_command(
                        &mut world,
                        HookCommand {
                            entity,
                            component,
                            variant: HookVariant::OnInsert,
                        },
                    )
                }
                world
                    .get_resource::<ComponentHooksResource>()
                    .expect("todo")
                    .hooks
                    .get(&component)
                    .expect("todo")
                    .on_insert
                    .expect("todo")(direction, world, entity, component);
            });

        self
    }
    pub fn on_replace(&mut self, hook: RevComponentHook) -> &mut Self {
        let mut resource = self
            .world
            .get_resource_or_insert_with(ComponentHooksResource::default);
        let hooks = resource.hooks.entry(self.component).or_default();
        if hooks.on_replace.is_some() {
            todo!()
        }
        hooks.on_replace = Some(hook);

        self.world
            .register_component_hooks_by_id(self.component)
            .expect("todo")
            .on_replace(|mut world, entity, component| {
                let direction = HookDirection::get_in_hook(&world);
                if direction == (HookDirection::Forward { log: false }) {
                    buffer_rev_command(
                        &mut world,
                        HookCommand {
                            entity,
                            component,
                            variant: HookVariant::OnReplace,
                        },
                    )
                }
                world
                    .get_resource::<ComponentHooksResource>()
                    .expect("todo")
                    .hooks
                    .get(&component)
                    .expect("todo")
                    .on_replace
                    .expect("todo")(direction, world, entity, component);
            });

        self
    }
    pub fn on_remove(&mut self, hook: RevComponentHook) -> &mut Self {
        let mut resource = self
            .world
            .get_resource_or_insert_with(ComponentHooksResource::default);
        let hooks = resource.hooks.entry(self.component).or_default();
        if hooks.on_remove.is_some() {
            todo!()
        }
        hooks.on_remove = Some(hook);

        self.world
            .register_component_hooks_by_id(self.component)
            .expect("todo")
            .on_remove(|mut world, entity, component| {
                let direction = HookDirection::get_in_hook(&world);
                if direction == (HookDirection::Forward { log: false }) {
                    buffer_rev_command(
                        &mut world,
                        HookCommand {
                            entity,
                            component,
                            variant: HookVariant::OnRemove,
                        },
                    )
                }
                world
                    .get_resource::<ComponentHooksResource>()
                    .expect("todo")
                    .hooks
                    .get(&component)
                    .expect("todo")
                    .on_remove
                    .expect("todo")(direction, world, entity, component);
            });

        self
    }
    pub fn try_on_add(&mut self, hook: RevComponentHook) -> Option<&mut Self> {
        todo!()
    }
    pub fn try_on_insert(&mut self, hook: RevComponentHook) -> Option<&mut Self> {
        todo!()
    }
    pub fn try_on_replace(&mut self, hook: RevComponentHook) -> Option<&mut Self> {
        todo!()
    }
    pub fn try_on_remove(&mut self, hook: RevComponentHook) -> Option<&mut Self> {
        todo!()
    }
}

struct HookCommand {
    entity: Entity,
    component: ComponentId,
    variant: HookVariant,
}

enum HookVariant {
    OnAdd,
    OnInsert,
    OnReplace,
    OnRemove,
}

impl HookCommand {
    fn undo_redo(&self, world: DeferredWorld, undo: bool) {
        let direction = if undo {
            HookDirection::BackwardLog
        } else {
            HookDirection::Forward { log: true }
        };
        let hooks = world
            .get_resource::<ComponentHooksResource>()
            .expect("todo")
            .hooks
            .get(&self.component)
            .expect("todo");
        let hook = match self.variant {
            HookVariant::OnAdd => hooks.on_add,
            HookVariant::OnInsert => hooks.on_insert,
            HookVariant::OnReplace => hooks.on_replace,
            HookVariant::OnRemove => hooks.on_remove,
        };
        hook.expect("todo")(direction, world.into(), self.entity, self.component)
    }
}

impl RevCommandInit for HookCommand {
    fn undo(&mut self, world: &mut World) {
        self.undo_redo(world.into(), true)
    }
    fn redo(&mut self, world: &mut World) {
        self.undo_redo(world.into(), false)
    }
}
