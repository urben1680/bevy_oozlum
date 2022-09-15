use bevy::ecs::system::SystemParam;

pub trait Derived: SystemParam{
    /*
    - replaces `Entity` with `PresentEntity` or, if not present in query, adds `Without<EntityDespawned>`
    - sensitive to marked query

    - alternative, implementing struct consists of systemparams and might also contain worldquery items
    -- straight-forward as user params
    -- enforce rules: PresentEntity, no commands, no events (both only for state changes)
    --- Braucht zusätzlichen type für StateChange, Filter


    types:
    - State (S)
    - Transition (T)

    - User Params System (US) -> UP
    - User Params (UP)

    - User Params + State Change Params (CP) -> UP + SP
    - State Change Params (SP)

    - Log Only
    */
}

