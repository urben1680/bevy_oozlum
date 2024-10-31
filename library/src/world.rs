use bevy::ecs::world::World;

#[derive(Clone, Copy, Debug)]
pub struct ScheduleMissing;

pub trait RevWorld {}

impl RevWorld for World {}
