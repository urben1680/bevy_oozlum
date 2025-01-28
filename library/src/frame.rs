use std::fmt::Debug;

use bevy::reflect::Reflect;

#[cfg(feature = "serde")]
use bevy::reflect::{ReflectDeserialize, ReflectSerialize};

#[derive(Copy, Clone, Reflect, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    reflect(Serialize, Deserialize)
)]
pub struct RevFrame(pub u64);

impl RevFrame {
    pub(crate) const fn increase(self) -> Self {
        Self(self.0 + 1)
    }
    pub(crate) const fn decrease(self) -> Self {
        Self(self.0 - 1)
    }
}
