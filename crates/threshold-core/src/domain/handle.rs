use std::{borrow::Cow, error::Error, fmt};

use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Handle {
    Entity(u64),
    Fact(u64),
    WorldEvent(u64),
    Goal(u64),
    NewNpc(u64),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HandleError;

impl Handle {
    pub fn parse(input: &str) -> Result<Handle, HandleError> {
        let (prefix, digits) = input.split_at_checked(1).ok_or(HandleError)?;
        if digits.is_empty()
            || !digits.bytes().all(|byte| byte.is_ascii_digit())
            || (digits.len() > 1 && digits.starts_with('0'))
        {
            return Err(HandleError);
        }
        let id = digits.parse().map_err(|_| HandleError)?;

        match prefix {
            "E" => Ok(Self::Entity(id)),
            "F" => Ok(Self::Fact(id)),
            "W" => Ok(Self::WorldEvent(id)),
            "G" => Ok(Self::Goal(id)),
            "N" => Ok(Self::NewNpc(id)),
            _ => Err(HandleError),
        }
    }
}

impl fmt::Display for Handle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (prefix, id) = match self {
            Self::Entity(id) => ('E', id),
            Self::Fact(id) => ('F', id),
            Self::WorldEvent(id) => ('W', id),
            Self::Goal(id) => ('G', id),
            Self::NewNpc(id) => ('N', id),
        };
        write!(f, "{prefix}{id}")
    }
}

impl fmt::Display for HandleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid handle")
    }
}

impl Error for HandleError {}

impl Serialize for Handle {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Handle {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(de::Error::custom)
    }
}

impl JsonSchema for Handle {
    fn schema_name() -> Cow<'static, str> {
        "Handle".into()
    }

    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        String::json_schema(generator)
    }
}
