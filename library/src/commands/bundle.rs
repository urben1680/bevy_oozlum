/*
- components can register other components as required
- required components need to specify a constructor like Default
- existing required components are not overwritten
- so mimicking a subset of Bundle is not enough here if a required component is added along it
- a untyped take method is needed https://github.com/bevyengine/bevy/issues/15350

Required Components PR https://github.com/bevyengine/bevy/pull/14791
- wait with impl here until that PR lands (likely next bevy version)
- world.bundled() -> &Bundles
- bundles.get_id(TypeId) -> bundles.get(id) -> Option<&BundleInfo>
-- if None, use remove::<Bundle> on a dummy entity so the bundle gets registered
- info.contributed_components -> &[ComponentId]

MVP:
insert:
- 1. bundle must not add required components
-- user must provide a bundle that includes all required components minus those that are existing in the entity
- 2. components must not be overwritten unless all are overwritten so typed take method can be used
-- user must first issue a remove command if the entity already contains a subset of components
remove:
- 1. bundle must not remove required components
-- user must provide a bundle that includes all required components
- 2. all components for the entity must exist
spawn/despawn
- unsupported until entity disabling https://github.com/bevyengine/bevy/issues/11090

helper trait that can be implemented on third party bundles with an associated
type that includes required components, then only each 2nd rule must be followed
*/
