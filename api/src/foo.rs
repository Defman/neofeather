use std::collections::HashMap;

use serde::{Deserialize, Serialize, de::DeserializeOwned};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum QueryAccess {
    None,
    Read(TypeLayout, String),
    Write(TypeLayout, String),
    Optional(Box<QueryAccess>),
    With(TypeLayout, Box<QueryAccess>),
    Without(TypeLayout, Box<QueryAccess>),
    Union(Vec<QueryAccess>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TypeLayout {
    Struct {
        name: String,
        layout: StructLayout
    },
    Enum {
        name: String,
        layout: EnumLayout
    },
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructLayout {
    fields: HashMap<String, TypeLayout>
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnumLayout {
    variants: HashMap<String, TypeLayout>
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterSystem {
    queires: Vec<QueryAccess>,
    resources: Vec<TypeLayout>,
}

// (&Immutable_Component, &mut Component)
// Can be constructed from &[u8]
pub trait QuillQuery: Serialize + DeserializeOwned {}

pub struct Query<Q: QuillQuery> {
    entities: Vec<Q>
}