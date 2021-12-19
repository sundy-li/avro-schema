use std::{collections::HashMap, fmt};

use serde::{
    de::{MapAccess, SeqAccess, Visitor},
    Deserialize, Deserializer,
};
use serde_json::Value;

use super::*;

fn to_primitive(v: &str) -> Option<Schema> {
    use Schema::*;
    Some(match v {
        "null" => Null,
        "boolean" => Boolean,
        "string" => String,
        "bytes" => Bytes,
        "int" => Int,
        "long" => Long,
        "float" => Float,
        "double" => Double,
        _ => return None,
    })
}

fn get_type<E: serde::de::Error>(map: &mut HashMap<String, Value>) -> Result<String, E> {
    if let Some(v) = map.remove("type") {
        if let Value::String(v) = v {
            Ok(v)
        } else {
            Err(serde::de::Error::custom("type must be a string"))
        }
    } else {
        Err(serde::de::Error::missing_field("type"))
    }
}

fn as_string<E: serde::de::Error>(v: Value, helper: &str) -> Result<String, E> {
    if let Value::String(v) = v {
        Ok(v)
    } else {
        Err(serde::de::Error::custom(format!(
            "{} must be a string",
            helper
        )))
    }
}

fn remove_string<E: serde::de::Error>(
    data: &mut HashMap<String, Value>,
    key: &str,
) -> Result<Option<String>, E> {
    match data.remove(key) {
        Some(s) => as_string(s, key).map(Some),
        None => Ok(None),
    }
}

fn remove_vec_string<E: serde::de::Error>(
    data: &mut HashMap<String, Value>,
    key: &str,
) -> Result<Vec<String>, E> {
    match data.remove(key) {
        Some(s) => {
            if let Value::Array(x) = s {
                x.into_iter().map(|x| as_string(x, key)).collect()
            } else {
                Err(serde::de::Error::custom(format!(
                    "{} must be a string",
                    key
                )))
            }
        }
        None => Ok(vec![]),
    }
}

fn to_enum<E: serde::de::Error>(data: &mut HashMap<String, Value>) -> Result<Schema, E> {
    Ok(Schema::Enum(Enum {
        name: remove_string(data, "name")?
            .ok_or_else(|| serde::de::Error::custom("name is required in enum"))?,
        namespace: remove_string(data, "namespace")?,
        aliases: remove_vec_string(data, "aliases")?,
        doc: remove_string(data, "doc")?,
        symbols: remove_vec_string(data, "symbols")?,
        default: remove_string(data, "default")?,
    }))
}

fn to_map<E: serde::de::Error>(data: &mut HashMap<String, Value>) -> Result<Schema, E> {
    let item = data
        .remove("values")
        .ok_or_else(|| serde::de::Error::custom("values is required in a map"))?;
    let schema: Schema = serde_json::from_value(item).map_err(serde::de::Error::custom)?;
    Ok(Schema::Map(Box::new(schema)))
}

fn to_schema<E: serde::de::Error>(
    data: &mut HashMap<String, Value>,
    key: &str,
) -> Result<Option<Schema>, E> {
    let schema = data.remove(key);
    schema
        .map(|schema| serde_json::from_value(schema).map_err(serde::de::Error::custom))
        .transpose()
}

fn to_array<E: serde::de::Error>(data: &mut HashMap<String, Value>) -> Result<Schema, E> {
    let schema =
        to_schema(data, "items")?.ok_or_else(|| E::custom("items is required in an array"))?;
    Ok(Schema::Array(Box::new(schema)))
}

fn to_field<E: serde::de::Error>(data: Value) -> Result<Field, E> {
    serde_json::from_value(data).map_err(E::custom)
}

fn to_vec_fields<E: serde::de::Error>(
    data: &mut HashMap<String, Value>,
    key: &str,
) -> Result<Vec<Field>, E> {
    match data.remove(key) {
        Some(s) => {
            if let Value::Array(x) = s {
                x.into_iter().map(to_field).collect()
            } else {
                Err(E::custom(format!("{} must be a string", key)))
            }
        }
        None => Ok(vec![]),
    }
}

fn to_record<E: serde::de::Error>(data: &mut HashMap<String, Value>) -> Result<Schema, E> {
    Ok(Schema::Record(Record {
        name: remove_string(data, "name")?
            .ok_or_else(|| serde::de::Error::custom("name is required in enum"))?,
        namespace: remove_string(data, "namespace")?,
        aliases: remove_vec_string(data, "aliases")?,
        doc: remove_string(data, "doc")?,
        fields: to_vec_fields(data, "fields")?,
    }))
}

fn to_fixed<E: serde::de::Error>(data: &mut HashMap<String, Value>) -> Result<Schema, E> {
    let size = data
        .remove("size")
        .ok_or_else(|| serde::de::Error::custom("name is required in enum"))
        .and_then(|x| serde_json::from_value::<usize>(x).map_err(serde::de::Error::custom))?;
    Ok(Schema::Fixed(Fixed {
        name: remove_string(data, "name")?
            .ok_or_else(|| serde::de::Error::custom("name is required in enum"))?,
        namespace: remove_string(data, "namespace")?,
        aliases: remove_vec_string(data, "aliases")?,
        doc: remove_string(data, "doc")?,
        size,
    }))
}

fn to_order<E: serde::de::Error>(
    data: &mut HashMap<String, Value>,
    key: &str,
) -> Result<Option<Order>, E> {
    remove_string(data, key)?
        .map(|x| {
            Ok(match x.as_ref() {
                "ascending" => Order::Ascending,
                "descending" => Order::Descending,
                "ignore" => Order::Ignore,
                _ => {
                    return Err(serde::de::Error::custom(
                        "order can only be one of {ascending, descending, ignore}",
                    ))
                }
            })
        })
        .transpose()
}

struct SchemaVisitor {}

impl<'de> Visitor<'de> for SchemaVisitor {
    type Value = Schema;

    // Format a message stating what data this Visitor expects to receive.
    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a string, array or map describing an Avro schema")
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        to_primitive(v)
            .ok_or_else(|| serde::de::Error::custom("string must be a valid primitive Schema"))
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut vec = Vec::with_capacity(seq.size_hint().unwrap_or(0));
        while let Some(item) = seq.next_element::<Value>()? {
            let schema: Schema = serde_json::from_value(item).map_err(serde::de::Error::custom)?;
            vec.push(schema)
        }
        Ok(Schema::Union(vec))
    }

    fn visit_map<M>(self, mut access: M) -> Result<Self::Value, M::Error>
    where
        M: MapAccess<'de>,
    {
        let mut map = HashMap::<String, Value>::with_capacity(access.size_hint().unwrap_or(0));

        // While there are entries remaining in the input, add them
        // into our map.
        while let Some((key, value)) = access.next_entry()? {
            map.insert(key, value);
        }

        let (schema, type_) = get_type(&mut map).map(|x| (to_primitive(&x), x))?;

        if let Some(schema) = schema {
            Ok(schema)
        } else {
            match type_.as_ref() {
                "enum" => to_enum(&mut map),
                "map" => to_map(&mut map),
                "array" => to_array(&mut map),
                "record" => to_record(&mut map),
                "fixed" => to_fixed(&mut map),
                other => todo!("{}", other),
            }
        }
    }
}

impl<'de> Deserialize<'de> for Schema {
    fn deserialize<D>(deserializer: D) -> Result<Schema, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(SchemaVisitor {})
    }
}

struct FieldVisitor {}

impl<'de> Visitor<'de> for FieldVisitor {
    type Value = Field;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a map describing an Avro field")
    }

    fn visit_map<M>(self, mut access: M) -> Result<Self::Value, M::Error>
    where
        M: MapAccess<'de>,
    {
        let mut map = HashMap::<String, Value>::with_capacity(access.size_hint().unwrap_or(0));

        // While there are entries remaining in the input, add them
        // into our map.
        while let Some((key, value)) = access.next_entry()? {
            map.insert(key, value);
        }

        Ok(Field {
            name: remove_string(&mut map, "name")?
                .ok_or_else(|| serde::de::Error::custom("name is required in enum"))?,
            doc: remove_string(&mut map, "doc")?,
            schema: to_schema(&mut map, "type")?
                .ok_or_else(|| serde::de::Error::custom("schema is required in Field"))?,
            default: to_schema(&mut map, "default")?,
            order: to_order(&mut map, "order")?,
            aliases: remove_vec_string(&mut map, "aliases")?,
        })
    }
}

impl<'de> Deserialize<'de> for Field {
    fn deserialize<D>(deserializer: D) -> Result<Field, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_map(FieldVisitor {})
    }
}
