//! Tool parameter schemas: a small hand-rolled JSON-Schema subset. The
//! builtin tools take flat string/integer/boolean/array arguments, so a
//! tiny owned type documents the shape exactly and serializes to the
//! wire form providers expect, with no schema-derivation dependency.

use serde_json::{Map, Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Schema {
    Str,
    Int,
    Bool,
    StrList,
    Object {
        desc: &'static str,
        props: &'static [Prop],
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Prop {
    pub name: &'static str,
    pub ty: Schema,
    pub required: bool,
    pub desc: &'static str,
}

impl Schema {
    /// The object form used by every builtin tool.
    pub fn object(desc: &'static str, props: &'static [Prop]) -> Schema {
        Schema::Object { desc, props }
    }

    pub fn to_value(&self) -> Value {
        match self {
            Schema::Str => json!({ "type": "string" }),
            Schema::Int => json!({ "type": "integer" }),
            Schema::Bool => json!({ "type": "boolean" }),
            Schema::StrList => json!({ "type": "array", "items": { "type": "string" } }),
            Schema::Object { desc, props } => {
                let mut properties = Map::new();
                let mut required = Vec::new();
                for prop in *props {
                    let mut field = prop.ty.to_value();
                    if let Value::Object(map) = &mut field {
                        map.insert("description".to_string(), Value::from(prop.desc));
                    }
                    properties.insert(prop.name.to_string(), field);
                    if prop.required {
                        required.push(Value::from(prop.name));
                    }
                }
                json!({
                    "type": "object",
                    "description": desc,
                    "properties": Value::Object(properties),
                    "required": required,
                    "additionalProperties": false,
                })
            }
        }
    }
}
