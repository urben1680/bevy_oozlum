use std::{collections::VecDeque, marker::PhantomData, ops::Range};

use serde::{
    de::{SeqAccess, Visitor},
    ser::SerializeSeq,
    Deserialize, Deserializer, Serialize, Serializer,
};

/// for state variants
pub mod logless_state {
    use super::*;

    #[allow(private_bounds)]
    pub fn serialize<S, T>(this: &T, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
        T: LoglessState,
    {
        this.get_logless_state().serialize(serializer)
    }

    #[allow(private_bounds)]
    pub fn deserialize<'de, D, T>(deserializer: D) -> Result<T, D::Error>
    where
        D: Deserializer<'de>,
        T: LoglessState,
    {
        Deserialize::deserialize(deserializer).and_then(|deserialized| {
            T::from_logless_state(deserialized).map_err(|message| serde::de::Error::custom(message))
        })
    }
}

/// for all variants
pub mod with_capacity {
    use super::*;

    #[allow(private_bounds)]
    pub fn serialize<S, T>(this: &T, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
        T: WithCapacity,
    {
        this.get_with_capacity().serialize(serializer)
    }

    #[allow(private_bounds)]
    pub fn deserialize<'de, D, T>(deserializer: D) -> Result<T, D::Error>
    where
        D: Deserializer<'de>,
        T: WithCapacity,
    {
        Deserialize::deserialize(deserializer).and_then(|deserialized| {
            T::from_with_capacity(deserialized).map_err(|message| serde::de::Error::custom(message))
        })
    }
}

/// for all variants
pub mod logless_with_capacity {
    use super::*;

    #[allow(private_bounds)]
    pub fn serialize<S, T>(this: &T, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
        T: LoglessWithCapacity,
    {
        this.get_logless_with_capacity().serialize(serializer)
    }

    #[allow(private_bounds)]
    pub fn deserialize<'de, D, T>(deserializer: D) -> Result<T, D::Error>
    where
        D: Deserializer<'de>,
        T: LoglessWithCapacity,
    {
        Deserialize::deserialize(deserializer).and_then(|deserialized| {
            T::from_logless_with_capacity(deserialized)
                .map_err(|message| serde::de::Error::custom(message))
        })
    }
}

pub(super) trait LoglessState: Sized {
    type Se<'se>: Serialize
    where
        Self: 'se;
    type De: for<'de> Deserialize<'de>;
    fn get_logless_state(&self) -> Self::Se<'_>;
    fn from_logless_state(logless_state: Self::De) -> Result<Self, String>;
}

pub(super) trait WithCapacity: Sized {
    type Se<'se>: Serialize
    where
        Self: 'se;
    type De: for<'de> Deserialize<'de>;
    fn get_with_capacity(&self) -> Self::Se<'_>;
    fn from_with_capacity(with_capacity: Self::De) -> Result<Self, String>;
}

pub(super) trait LoglessWithCapacity: Sized {
    type Se<'se>: Serialize
    where
        Self: 'se;
    type De: for<'de> Deserialize<'de>;
    fn get_logless_with_capacity(&self) -> Self::Se<'_>;
    fn from_logless_with_capacity(logless_with_capacity: Self::De) -> Result<Self, String>;
}

/// Serializes to a sequence.
pub(super) struct WithRange<'a, T> {
    pub(super) deque: &'a VecDeque<T>,
    pub(super) range: Range<usize>,
}

/// Serializes and Deserializes into one usize and a sequence `T`. The usize contains the capacity of `T`.
pub(super) struct WithCapacityWrapper<T>(pub(super) T);

impl<'a, T: Serialize> Serialize for WithRange<'a, T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut seq = serializer.serialize_seq(Some(self.range.len()))?;
        for state in self.deque.range(self.range.clone()) {
            seq.serialize_element(state)?;
        }
        seq.end()
    }
}

impl<T: Serialize> Serialize for WithCapacityWrapper<&'_ VecDeque<T>> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(self.0.len() + 1))?;
        seq.serialize_element(&self.0.capacity())?;
        for element in self.0 {
            seq.serialize_element(element)?;
        }
        seq.end()
    }
}

impl<T: Serialize> Serialize for WithCapacityWrapper<WithRange<'_, T>> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(self.0.range.len() + 1))?;
        seq.serialize_element(&self.0.deque.capacity())?;
        for element in self.0.deque.range(self.0.range.clone()) {
            seq.serialize_element(element)?;
        }
        seq.end()
    }
}

impl<'de, T> Deserialize<'de> for WithCapacityWrapper<VecDeque<T>>
where
    T: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct WithCapacityVisitor<T>(PhantomData<T>);

        impl<'de, T: Deserialize<'de>> Visitor<'de> for WithCapacityVisitor<T> {
            type Value = WithCapacityWrapper<VecDeque<T>>;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                write!(
                    formatter,
                    "a sequence of one usize followed by a number of {}",
                    std::any::type_name::<T>()
                )
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let Some(capacity) = seq.next_element()? else {
                    return Err(serde::de::Error::custom("expected usize for capacity"));
                };

                let mut values = VecDeque::<T>::with_capacity(capacity);

                while let Some(value) = seq.next_element()? {
                    values.push_back(value);
                }

                Ok(WithCapacityWrapper(values))
            }
        }

        deserializer.deserialize_seq(WithCapacityVisitor(PhantomData))
    }
}

#[cfg(test)]
mod test {
    use std::collections::VecDeque;

    use super::{WithCapacityWrapper, WithRange};

    #[test]
    fn with_capacity_wrapper() {
        let mut deque = VecDeque::with_capacity(100);
        let chars = ['a', 'b', 'c'];
        deque.extend(chars);

        let ser1 = WithCapacityWrapper(&deque);
        let ser2 = WithCapacityWrapper(WithRange {
            deque: &deque,
            range: 0..3,
        });

        let json1 = serde_json::to_string(&ser1).unwrap();
        let json2 = serde_json::to_string(&ser2).unwrap();

        let json_expected = format!("[{},\"a\",\"b\",\"c\"]", deque.capacity());
        assert_eq!(json1, json_expected);
        assert_eq!(json2, json_expected);

        let des1: WithCapacityWrapper<VecDeque<char>> = serde_json::from_str(&json1).unwrap();
        let des2: WithCapacityWrapper<VecDeque<char>> = serde_json::from_str(&json2).unwrap();

        assert_eq!(des1.0, chars);
        assert_eq!(des2.0, chars);

        assert_eq!(des1.0.capacity(), deque.capacity());
        assert_eq!(des2.0.capacity(), deque.capacity());
    }
}
