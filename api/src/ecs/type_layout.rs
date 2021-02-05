use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub enum TypeLayout {
    Struct { name: String, layout: StructLayout },
    Enum { name: String, layout: EnumLayout },
}

impl TypeLayout {
    pub fn unit(name: String) -> Self {
        TypeLayout::Struct {
            name,
            layout: StructLayout { fields: vec![] },
        }
    }
}

pub trait IntoTypeLayout {
    fn layout() -> TypeLayout;
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructLayout {
    fields: Vec<(String, TypeLayout)>,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnumLayout {
    variants: Vec<(String, TypeLayout)>,
}

macro_rules! layout_primitive {
    ($ident:tt) => {
        impl IntoTypeLayout for $ident {
            fn layout() -> TypeLayout {
                TypeLayout::unit(stringify!($ident).to_owned())
            }
        }
    };
    [$($ident:tt),* $(,)?] => {
        $(layout_primitive!($ident);)*
    };
}

layout_primitive![
    (),
    u8,
    i8,
    u16,
    i16,
    u32,
    i32,
    u64,
    i64,
    u128,
    i128,
    f32,
    f64,
    char,
    bool,
];