impl serde::Serialize for AnyValue {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.value.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.trace.idx.AnyValue", len)?;
        if let Some(v) = self.value.as_ref() {
            match v {
                any_value::Value::StringValueRef(v) => {
                    struct_ser.serialize_field("stringValueRef", v)?;
                }
                any_value::Value::BoolValue(v) => {
                    struct_ser.serialize_field("boolValue", v)?;
                }
                any_value::Value::DoubleValue(v) => {
                    struct_ser.serialize_field("doubleValue", v)?;
                }
                any_value::Value::IntValue(v) => {
                    #[allow(clippy::needless_borrow)]
                    #[allow(clippy::needless_borrows_for_generic_args)]
                    struct_ser.serialize_field("intValue", ToString::to_string(&v).as_str())?;
                }
                any_value::Value::BytesValue(v) => {
                    #[allow(clippy::needless_borrow)]
                    #[allow(clippy::needless_borrows_for_generic_args)]
                    struct_ser.serialize_field("bytesValue", pbjson::private::base64::encode(&v).as_str())?;
                }
                any_value::Value::ArrayValue(v) => {
                    struct_ser.serialize_field("arrayValue", v)?;
                }
                any_value::Value::KeyValueList(v) => {
                    struct_ser.serialize_field("keyValueList", v)?;
                }
            }
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for AnyValue {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "stringValueRef",
            "boolValue",
            "doubleValue",
            "intValue",
            "bytesValue",
            "arrayValue",
            "keyValueList",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            StringValueRef,
            BoolValue,
            DoubleValue,
            IntValue,
            BytesValue,
            ArrayValue,
            KeyValueList,
            __SkipField__,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl serde::de::Visitor<'_> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "stringValueRef" => Ok(GeneratedField::StringValueRef),
                            "boolValue" => Ok(GeneratedField::BoolValue),
                            "doubleValue" => Ok(GeneratedField::DoubleValue),
                            "intValue" => Ok(GeneratedField::IntValue),
                            "bytesValue" => Ok(GeneratedField::BytesValue),
                            "arrayValue" => Ok(GeneratedField::ArrayValue),
                            "keyValueList" => Ok(GeneratedField::KeyValueList),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = AnyValue;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.trace.idx.AnyValue")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<AnyValue, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut value__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::StringValueRef => {
                            if value__.is_some() {
                                return Err(serde::de::Error::duplicate_field("stringValueRef"));
                            }
                            value__ = map_.next_value::<::std::option::Option<::pbjson::private::NumberDeserialize<_>>>()?.map(|x| any_value::Value::StringValueRef(x.0));
                        }
                        GeneratedField::BoolValue => {
                            if value__.is_some() {
                                return Err(serde::de::Error::duplicate_field("boolValue"));
                            }
                            value__ = map_.next_value::<::std::option::Option<_>>()?.map(any_value::Value::BoolValue);
                        }
                        GeneratedField::DoubleValue => {
                            if value__.is_some() {
                                return Err(serde::de::Error::duplicate_field("doubleValue"));
                            }
                            value__ = map_.next_value::<::std::option::Option<::pbjson::private::NumberDeserialize<_>>>()?.map(|x| any_value::Value::DoubleValue(x.0));
                        }
                        GeneratedField::IntValue => {
                            if value__.is_some() {
                                return Err(serde::de::Error::duplicate_field("intValue"));
                            }
                            value__ = map_.next_value::<::std::option::Option<::pbjson::private::NumberDeserialize<_>>>()?.map(|x| any_value::Value::IntValue(x.0));
                        }
                        GeneratedField::BytesValue => {
                            if value__.is_some() {
                                return Err(serde::de::Error::duplicate_field("bytesValue"));
                            }
                            value__ = map_.next_value::<::std::option::Option<::pbjson::private::BytesDeserialize<_>>>()?.map(|x| any_value::Value::BytesValue(x.0));
                        }
                        GeneratedField::ArrayValue => {
                            if value__.is_some() {
                                return Err(serde::de::Error::duplicate_field("arrayValue"));
                            }
                            value__ = map_.next_value::<::std::option::Option<_>>()?.map(any_value::Value::ArrayValue)
;
                        }
                        GeneratedField::KeyValueList => {
                            if value__.is_some() {
                                return Err(serde::de::Error::duplicate_field("keyValueList"));
                            }
                            value__ = map_.next_value::<::std::option::Option<_>>()?.map(any_value::Value::KeyValueList)
;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(AnyValue {
                    value: value__,
                })
            }
        }
        deserializer.deserialize_struct("datadog.trace.idx.AnyValue", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for ArrayValue {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.values.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.trace.idx.ArrayValue", len)?;
        if !self.values.is_empty() {
            struct_ser.serialize_field("values", &self.values)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for ArrayValue {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "values",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Values,
            __SkipField__,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl serde::de::Visitor<'_> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "values" => Ok(GeneratedField::Values),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = ArrayValue;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.trace.idx.ArrayValue")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<ArrayValue, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut values__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Values => {
                            if values__.is_some() {
                                return Err(serde::de::Error::duplicate_field("values"));
                            }
                            values__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(ArrayValue {
                    values: values__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.trace.idx.ArrayValue", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for ContainerDebug {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.error_ref != 0 {
            len += 1;
        }
        if self.latency_ms != 0 {
            len += 1;
        }
        if self.was_buffered {
            len += 1;
        }
        if self.buffer_ms != 0 {
            len += 1;
        }
        if self.buffer_eviction_reason_ref != 0 {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.trace.idx.ContainerDebug", len)?;
        if self.error_ref != 0 {
            struct_ser.serialize_field("errorRef", &self.error_ref)?;
        }
        if self.latency_ms != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("latencyMs", ToString::to_string(&self.latency_ms).as_str())?;
        }
        if self.was_buffered {
            struct_ser.serialize_field("wasBuffered", &self.was_buffered)?;
        }
        if self.buffer_ms != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("bufferMs", ToString::to_string(&self.buffer_ms).as_str())?;
        }
        if self.buffer_eviction_reason_ref != 0 {
            struct_ser.serialize_field("bufferEvictionReasonRef", &self.buffer_eviction_reason_ref)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for ContainerDebug {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "errorRef",
            "latencyMs",
            "wasBuffered",
            "bufferMs",
            "bufferEvictionReasonRef",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            ErrorRef,
            LatencyMs,
            WasBuffered,
            BufferMs,
            BufferEvictionReasonRef,
            __SkipField__,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl serde::de::Visitor<'_> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "errorRef" => Ok(GeneratedField::ErrorRef),
                            "latencyMs" => Ok(GeneratedField::LatencyMs),
                            "wasBuffered" => Ok(GeneratedField::WasBuffered),
                            "bufferMs" => Ok(GeneratedField::BufferMs),
                            "bufferEvictionReasonRef" => Ok(GeneratedField::BufferEvictionReasonRef),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = ContainerDebug;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.trace.idx.ContainerDebug")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<ContainerDebug, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut error_ref__ = None;
                let mut latency_ms__ = None;
                let mut was_buffered__ = None;
                let mut buffer_ms__ = None;
                let mut buffer_eviction_reason_ref__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::ErrorRef => {
                            if error_ref__.is_some() {
                                return Err(serde::de::Error::duplicate_field("errorRef"));
                            }
                            error_ref__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::LatencyMs => {
                            if latency_ms__.is_some() {
                                return Err(serde::de::Error::duplicate_field("latencyMs"));
                            }
                            latency_ms__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::WasBuffered => {
                            if was_buffered__.is_some() {
                                return Err(serde::de::Error::duplicate_field("wasBuffered"));
                            }
                            was_buffered__ = Some(map_.next_value()?);
                        }
                        GeneratedField::BufferMs => {
                            if buffer_ms__.is_some() {
                                return Err(serde::de::Error::duplicate_field("bufferMs"));
                            }
                            buffer_ms__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::BufferEvictionReasonRef => {
                            if buffer_eviction_reason_ref__.is_some() {
                                return Err(serde::de::Error::duplicate_field("bufferEvictionReasonRef"));
                            }
                            buffer_eviction_reason_ref__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(ContainerDebug {
                    error_ref: error_ref__.unwrap_or_default(),
                    latency_ms: latency_ms__.unwrap_or_default(),
                    was_buffered: was_buffered__.unwrap_or_default(),
                    buffer_ms: buffer_ms__.unwrap_or_default(),
                    buffer_eviction_reason_ref: buffer_eviction_reason_ref__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.trace.idx.ContainerDebug", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for KeyValue {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.key != 0 {
            len += 1;
        }
        if self.value.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.trace.idx.KeyValue", len)?;
        if self.key != 0 {
            struct_ser.serialize_field("key", &self.key)?;
        }
        if let Some(v) = self.value.as_ref() {
            struct_ser.serialize_field("value", v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for KeyValue {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "key",
            "value",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Key,
            Value,
            __SkipField__,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl serde::de::Visitor<'_> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "key" => Ok(GeneratedField::Key),
                            "value" => Ok(GeneratedField::Value),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = KeyValue;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.trace.idx.KeyValue")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<KeyValue, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut key__ = None;
                let mut value__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Key => {
                            if key__.is_some() {
                                return Err(serde::de::Error::duplicate_field("key"));
                            }
                            key__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Value => {
                            if value__.is_some() {
                                return Err(serde::de::Error::duplicate_field("value"));
                            }
                            value__ = map_.next_value()?;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(KeyValue {
                    key: key__.unwrap_or_default(),
                    value: value__,
                })
            }
        }
        deserializer.deserialize_struct("datadog.trace.idx.KeyValue", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for KeyValueList {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.key_values.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.trace.idx.KeyValueList", len)?;
        if !self.key_values.is_empty() {
            struct_ser.serialize_field("keyValues", &self.key_values)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for KeyValueList {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "keyValues",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            KeyValues,
            __SkipField__,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl serde::de::Visitor<'_> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "keyValues" => Ok(GeneratedField::KeyValues),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = KeyValueList;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.trace.idx.KeyValueList")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<KeyValueList, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut key_values__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::KeyValues => {
                            if key_values__.is_some() {
                                return Err(serde::de::Error::duplicate_field("keyValues"));
                            }
                            key_values__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(KeyValueList {
                    key_values: key_values__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.trace.idx.KeyValueList", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for Span {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.service_ref != 0 {
            len += 1;
        }
        if self.name_ref != 0 {
            len += 1;
        }
        if self.resource_ref != 0 {
            len += 1;
        }
        if self.span_id != 0 {
            len += 1;
        }
        if self.parent_id != 0 {
            len += 1;
        }
        if self.start != 0 {
            len += 1;
        }
        if self.duration != 0 {
            len += 1;
        }
        if self.error {
            len += 1;
        }
        if !self.attributes.is_empty() {
            len += 1;
        }
        if self.type_ref != 0 {
            len += 1;
        }
        if !self.links.is_empty() {
            len += 1;
        }
        if !self.events.is_empty() {
            len += 1;
        }
        if self.env_ref != 0 {
            len += 1;
        }
        if self.version_ref != 0 {
            len += 1;
        }
        if self.component_ref != 0 {
            len += 1;
        }
        if self.kind != 0 {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.trace.idx.Span", len)?;
        if self.service_ref != 0 {
            struct_ser.serialize_field("serviceRef", &self.service_ref)?;
        }
        if self.name_ref != 0 {
            struct_ser.serialize_field("nameRef", &self.name_ref)?;
        }
        if self.resource_ref != 0 {
            struct_ser.serialize_field("resourceRef", &self.resource_ref)?;
        }
        if self.span_id != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("spanID", ToString::to_string(&self.span_id).as_str())?;
        }
        if self.parent_id != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("parentID", ToString::to_string(&self.parent_id).as_str())?;
        }
        if self.start != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("start", ToString::to_string(&self.start).as_str())?;
        }
        if self.duration != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("duration", ToString::to_string(&self.duration).as_str())?;
        }
        if self.error {
            struct_ser.serialize_field("error", &self.error)?;
        }
        if !self.attributes.is_empty() {
            struct_ser.serialize_field("attributes", &self.attributes)?;
        }
        if self.type_ref != 0 {
            struct_ser.serialize_field("typeRef", &self.type_ref)?;
        }
        if !self.links.is_empty() {
            struct_ser.serialize_field("links", &self.links)?;
        }
        if !self.events.is_empty() {
            struct_ser.serialize_field("events", &self.events)?;
        }
        if self.env_ref != 0 {
            struct_ser.serialize_field("envRef", &self.env_ref)?;
        }
        if self.version_ref != 0 {
            struct_ser.serialize_field("versionRef", &self.version_ref)?;
        }
        if self.component_ref != 0 {
            struct_ser.serialize_field("componentRef", &self.component_ref)?;
        }
        if self.kind != 0 {
            let v = SpanKind::try_from(self.kind)
                .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", self.kind)))?;
            struct_ser.serialize_field("kind", &v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for Span {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "serviceRef",
            "nameRef",
            "resourceRef",
            "spanID",
            "parentID",
            "start",
            "duration",
            "error",
            "attributes",
            "typeRef",
            "links",
            "events",
            "envRef",
            "versionRef",
            "componentRef",
            "kind",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            ServiceRef,
            NameRef,
            ResourceRef,
            SpanId,
            ParentId,
            Start,
            Duration,
            Error,
            Attributes,
            TypeRef,
            Links,
            Events,
            EnvRef,
            VersionRef,
            ComponentRef,
            Kind,
            __SkipField__,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl serde::de::Visitor<'_> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "serviceRef" => Ok(GeneratedField::ServiceRef),
                            "nameRef" => Ok(GeneratedField::NameRef),
                            "resourceRef" => Ok(GeneratedField::ResourceRef),
                            "spanID" => Ok(GeneratedField::SpanId),
                            "parentID" => Ok(GeneratedField::ParentId),
                            "start" => Ok(GeneratedField::Start),
                            "duration" => Ok(GeneratedField::Duration),
                            "error" => Ok(GeneratedField::Error),
                            "attributes" => Ok(GeneratedField::Attributes),
                            "typeRef" => Ok(GeneratedField::TypeRef),
                            "links" => Ok(GeneratedField::Links),
                            "events" => Ok(GeneratedField::Events),
                            "envRef" => Ok(GeneratedField::EnvRef),
                            "versionRef" => Ok(GeneratedField::VersionRef),
                            "componentRef" => Ok(GeneratedField::ComponentRef),
                            "kind" => Ok(GeneratedField::Kind),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = Span;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.trace.idx.Span")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<Span, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut service_ref__ = None;
                let mut name_ref__ = None;
                let mut resource_ref__ = None;
                let mut span_id__ = None;
                let mut parent_id__ = None;
                let mut start__ = None;
                let mut duration__ = None;
                let mut error__ = None;
                let mut attributes__ = None;
                let mut type_ref__ = None;
                let mut links__ = None;
                let mut events__ = None;
                let mut env_ref__ = None;
                let mut version_ref__ = None;
                let mut component_ref__ = None;
                let mut kind__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::ServiceRef => {
                            if service_ref__.is_some() {
                                return Err(serde::de::Error::duplicate_field("serviceRef"));
                            }
                            service_ref__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::NameRef => {
                            if name_ref__.is_some() {
                                return Err(serde::de::Error::duplicate_field("nameRef"));
                            }
                            name_ref__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::ResourceRef => {
                            if resource_ref__.is_some() {
                                return Err(serde::de::Error::duplicate_field("resourceRef"));
                            }
                            resource_ref__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::SpanId => {
                            if span_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("spanID"));
                            }
                            span_id__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::ParentId => {
                            if parent_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("parentID"));
                            }
                            parent_id__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Start => {
                            if start__.is_some() {
                                return Err(serde::de::Error::duplicate_field("start"));
                            }
                            start__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Duration => {
                            if duration__.is_some() {
                                return Err(serde::de::Error::duplicate_field("duration"));
                            }
                            duration__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Error => {
                            if error__.is_some() {
                                return Err(serde::de::Error::duplicate_field("error"));
                            }
                            error__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Attributes => {
                            if attributes__.is_some() {
                                return Err(serde::de::Error::duplicate_field("attributes"));
                            }
                            attributes__ = Some(
                                map_.next_value::<std::collections::HashMap<::pbjson::private::NumberDeserialize<u32>, _>>()?
                                    .into_iter().map(|(k,v)| (k.0, v)).collect()
                            );
                        }
                        GeneratedField::TypeRef => {
                            if type_ref__.is_some() {
                                return Err(serde::de::Error::duplicate_field("typeRef"));
                            }
                            type_ref__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Links => {
                            if links__.is_some() {
                                return Err(serde::de::Error::duplicate_field("links"));
                            }
                            links__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Events => {
                            if events__.is_some() {
                                return Err(serde::de::Error::duplicate_field("events"));
                            }
                            events__ = Some(map_.next_value()?);
                        }
                        GeneratedField::EnvRef => {
                            if env_ref__.is_some() {
                                return Err(serde::de::Error::duplicate_field("envRef"));
                            }
                            env_ref__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::VersionRef => {
                            if version_ref__.is_some() {
                                return Err(serde::de::Error::duplicate_field("versionRef"));
                            }
                            version_ref__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::ComponentRef => {
                            if component_ref__.is_some() {
                                return Err(serde::de::Error::duplicate_field("componentRef"));
                            }
                            component_ref__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Kind => {
                            if kind__.is_some() {
                                return Err(serde::de::Error::duplicate_field("kind"));
                            }
                            kind__ = Some(map_.next_value::<SpanKind>()? as i32);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(Span {
                    service_ref: service_ref__.unwrap_or_default(),
                    name_ref: name_ref__.unwrap_or_default(),
                    resource_ref: resource_ref__.unwrap_or_default(),
                    span_id: span_id__.unwrap_or_default(),
                    parent_id: parent_id__.unwrap_or_default(),
                    start: start__.unwrap_or_default(),
                    duration: duration__.unwrap_or_default(),
                    error: error__.unwrap_or_default(),
                    attributes: attributes__.unwrap_or_default(),
                    type_ref: type_ref__.unwrap_or_default(),
                    links: links__.unwrap_or_default(),
                    events: events__.unwrap_or_default(),
                    env_ref: env_ref__.unwrap_or_default(),
                    version_ref: version_ref__.unwrap_or_default(),
                    component_ref: component_ref__.unwrap_or_default(),
                    kind: kind__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.trace.idx.Span", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for SpanEvent {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.time != 0 {
            len += 1;
        }
        if self.name_ref != 0 {
            len += 1;
        }
        if !self.attributes.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.trace.idx.SpanEvent", len)?;
        if self.time != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("time", ToString::to_string(&self.time).as_str())?;
        }
        if self.name_ref != 0 {
            struct_ser.serialize_field("nameRef", &self.name_ref)?;
        }
        if !self.attributes.is_empty() {
            struct_ser.serialize_field("attributes", &self.attributes)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for SpanEvent {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "time",
            "nameRef",
            "attributes",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Time,
            NameRef,
            Attributes,
            __SkipField__,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl serde::de::Visitor<'_> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "time" => Ok(GeneratedField::Time),
                            "nameRef" => Ok(GeneratedField::NameRef),
                            "attributes" => Ok(GeneratedField::Attributes),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = SpanEvent;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.trace.idx.SpanEvent")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<SpanEvent, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut time__ = None;
                let mut name_ref__ = None;
                let mut attributes__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Time => {
                            if time__.is_some() {
                                return Err(serde::de::Error::duplicate_field("time"));
                            }
                            time__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::NameRef => {
                            if name_ref__.is_some() {
                                return Err(serde::de::Error::duplicate_field("nameRef"));
                            }
                            name_ref__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Attributes => {
                            if attributes__.is_some() {
                                return Err(serde::de::Error::duplicate_field("attributes"));
                            }
                            attributes__ = Some(
                                map_.next_value::<std::collections::HashMap<::pbjson::private::NumberDeserialize<u32>, _>>()?
                                    .into_iter().map(|(k,v)| (k.0, v)).collect()
                            );
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(SpanEvent {
                    time: time__.unwrap_or_default(),
                    name_ref: name_ref__.unwrap_or_default(),
                    attributes: attributes__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.trace.idx.SpanEvent", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for SpanKind {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let variant = match self {
            Self::Unspecified => "SPAN_KIND_UNSPECIFIED",
            Self::Internal => "SPAN_KIND_INTERNAL",
            Self::Server => "SPAN_KIND_SERVER",
            Self::Client => "SPAN_KIND_CLIENT",
            Self::Producer => "SPAN_KIND_PRODUCER",
            Self::Consumer => "SPAN_KIND_CONSUMER",
        };
        serializer.serialize_str(variant)
    }
}
impl<'de> serde::Deserialize<'de> for SpanKind {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "SPAN_KIND_UNSPECIFIED",
            "SPAN_KIND_INTERNAL",
            "SPAN_KIND_SERVER",
            "SPAN_KIND_CLIENT",
            "SPAN_KIND_PRODUCER",
            "SPAN_KIND_CONSUMER",
        ];

        struct GeneratedVisitor;

        impl serde::de::Visitor<'_> for GeneratedVisitor {
            type Value = SpanKind;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(formatter, "expected one of: {:?}", &FIELDS)
            }

            fn visit_i64<E>(self, v: i64) -> std::result::Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                i32::try_from(v)
                    .ok()
                    .and_then(|x| x.try_into().ok())
                    .ok_or_else(|| {
                        serde::de::Error::invalid_value(serde::de::Unexpected::Signed(v), &self)
                    })
            }

            fn visit_u64<E>(self, v: u64) -> std::result::Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                i32::try_from(v)
                    .ok()
                    .and_then(|x| x.try_into().ok())
                    .ok_or_else(|| {
                        serde::de::Error::invalid_value(serde::de::Unexpected::Unsigned(v), &self)
                    })
            }

            fn visit_str<E>(self, value: &str) -> std::result::Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                match value {
                    "SPAN_KIND_UNSPECIFIED" => Ok(SpanKind::Unspecified),
                    "SPAN_KIND_INTERNAL" => Ok(SpanKind::Internal),
                    "SPAN_KIND_SERVER" => Ok(SpanKind::Server),
                    "SPAN_KIND_CLIENT" => Ok(SpanKind::Client),
                    "SPAN_KIND_PRODUCER" => Ok(SpanKind::Producer),
                    "SPAN_KIND_CONSUMER" => Ok(SpanKind::Consumer),
                    _ => Err(serde::de::Error::unknown_variant(value, FIELDS)),
                }
            }
        }
        deserializer.deserialize_any(GeneratedVisitor)
    }
}
impl serde::Serialize for SpanLink {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.trace_id.is_empty() {
            len += 1;
        }
        if self.span_id != 0 {
            len += 1;
        }
        if !self.attributes.is_empty() {
            len += 1;
        }
        if self.tracestate_ref != 0 {
            len += 1;
        }
        if self.flags != 0 {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.trace.idx.SpanLink", len)?;
        if !self.trace_id.is_empty() {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("traceID", pbjson::private::base64::encode(&self.trace_id).as_str())?;
        }
        if self.span_id != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("spanID", ToString::to_string(&self.span_id).as_str())?;
        }
        if !self.attributes.is_empty() {
            struct_ser.serialize_field("attributes", &self.attributes)?;
        }
        if self.tracestate_ref != 0 {
            struct_ser.serialize_field("tracestateRef", &self.tracestate_ref)?;
        }
        if self.flags != 0 {
            struct_ser.serialize_field("flags", &self.flags)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for SpanLink {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "traceID",
            "spanID",
            "attributes",
            "tracestateRef",
            "flags",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            TraceId,
            SpanId,
            Attributes,
            TracestateRef,
            Flags,
            __SkipField__,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl serde::de::Visitor<'_> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "traceID" => Ok(GeneratedField::TraceId),
                            "spanID" => Ok(GeneratedField::SpanId),
                            "attributes" => Ok(GeneratedField::Attributes),
                            "tracestateRef" => Ok(GeneratedField::TracestateRef),
                            "flags" => Ok(GeneratedField::Flags),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = SpanLink;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.trace.idx.SpanLink")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<SpanLink, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut trace_id__ = None;
                let mut span_id__ = None;
                let mut attributes__ = None;
                let mut tracestate_ref__ = None;
                let mut flags__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::TraceId => {
                            if trace_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("traceID"));
                            }
                            trace_id__ = 
                                Some(map_.next_value::<::pbjson::private::BytesDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::SpanId => {
                            if span_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("spanID"));
                            }
                            span_id__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Attributes => {
                            if attributes__.is_some() {
                                return Err(serde::de::Error::duplicate_field("attributes"));
                            }
                            attributes__ = Some(
                                map_.next_value::<std::collections::HashMap<::pbjson::private::NumberDeserialize<u32>, _>>()?
                                    .into_iter().map(|(k,v)| (k.0, v)).collect()
                            );
                        }
                        GeneratedField::TracestateRef => {
                            if tracestate_ref__.is_some() {
                                return Err(serde::de::Error::duplicate_field("tracestateRef"));
                            }
                            tracestate_ref__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Flags => {
                            if flags__.is_some() {
                                return Err(serde::de::Error::duplicate_field("flags"));
                            }
                            flags__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(SpanLink {
                    trace_id: trace_id__.unwrap_or_default(),
                    span_id: span_id__.unwrap_or_default(),
                    attributes: attributes__.unwrap_or_default(),
                    tracestate_ref: tracestate_ref__.unwrap_or_default(),
                    flags: flags__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.trace.idx.SpanLink", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for TraceChunk {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.priority != 0 {
            len += 1;
        }
        if self.origin_ref != 0 {
            len += 1;
        }
        if !self.attributes.is_empty() {
            len += 1;
        }
        if !self.spans.is_empty() {
            len += 1;
        }
        if self.dropped_trace {
            len += 1;
        }
        if !self.trace_id.is_empty() {
            len += 1;
        }
        if self.sampling_mechanism != 0 {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.trace.idx.TraceChunk", len)?;
        if self.priority != 0 {
            struct_ser.serialize_field("priority", &self.priority)?;
        }
        if self.origin_ref != 0 {
            struct_ser.serialize_field("originRef", &self.origin_ref)?;
        }
        if !self.attributes.is_empty() {
            struct_ser.serialize_field("attributes", &self.attributes)?;
        }
        if !self.spans.is_empty() {
            struct_ser.serialize_field("spans", &self.spans)?;
        }
        if self.dropped_trace {
            struct_ser.serialize_field("droppedTrace", &self.dropped_trace)?;
        }
        if !self.trace_id.is_empty() {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("traceID", pbjson::private::base64::encode(&self.trace_id).as_str())?;
        }
        if self.sampling_mechanism != 0 {
            struct_ser.serialize_field("samplingMechanism", &self.sampling_mechanism)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for TraceChunk {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "priority",
            "originRef",
            "attributes",
            "spans",
            "droppedTrace",
            "traceID",
            "samplingMechanism",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Priority,
            OriginRef,
            Attributes,
            Spans,
            DroppedTrace,
            TraceId,
            SamplingMechanism,
            __SkipField__,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl serde::de::Visitor<'_> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "priority" => Ok(GeneratedField::Priority),
                            "originRef" => Ok(GeneratedField::OriginRef),
                            "attributes" => Ok(GeneratedField::Attributes),
                            "spans" => Ok(GeneratedField::Spans),
                            "droppedTrace" => Ok(GeneratedField::DroppedTrace),
                            "traceID" => Ok(GeneratedField::TraceId),
                            "samplingMechanism" => Ok(GeneratedField::SamplingMechanism),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = TraceChunk;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.trace.idx.TraceChunk")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<TraceChunk, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut priority__ = None;
                let mut origin_ref__ = None;
                let mut attributes__ = None;
                let mut spans__ = None;
                let mut dropped_trace__ = None;
                let mut trace_id__ = None;
                let mut sampling_mechanism__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Priority => {
                            if priority__.is_some() {
                                return Err(serde::de::Error::duplicate_field("priority"));
                            }
                            priority__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::OriginRef => {
                            if origin_ref__.is_some() {
                                return Err(serde::de::Error::duplicate_field("originRef"));
                            }
                            origin_ref__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Attributes => {
                            if attributes__.is_some() {
                                return Err(serde::de::Error::duplicate_field("attributes"));
                            }
                            attributes__ = Some(
                                map_.next_value::<std::collections::HashMap<::pbjson::private::NumberDeserialize<u32>, _>>()?
                                    .into_iter().map(|(k,v)| (k.0, v)).collect()
                            );
                        }
                        GeneratedField::Spans => {
                            if spans__.is_some() {
                                return Err(serde::de::Error::duplicate_field("spans"));
                            }
                            spans__ = Some(map_.next_value()?);
                        }
                        GeneratedField::DroppedTrace => {
                            if dropped_trace__.is_some() {
                                return Err(serde::de::Error::duplicate_field("droppedTrace"));
                            }
                            dropped_trace__ = Some(map_.next_value()?);
                        }
                        GeneratedField::TraceId => {
                            if trace_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("traceID"));
                            }
                            trace_id__ = 
                                Some(map_.next_value::<::pbjson::private::BytesDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::SamplingMechanism => {
                            if sampling_mechanism__.is_some() {
                                return Err(serde::de::Error::duplicate_field("samplingMechanism"));
                            }
                            sampling_mechanism__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(TraceChunk {
                    priority: priority__.unwrap_or_default(),
                    origin_ref: origin_ref__.unwrap_or_default(),
                    attributes: attributes__.unwrap_or_default(),
                    spans: spans__.unwrap_or_default(),
                    dropped_trace: dropped_trace__.unwrap_or_default(),
                    trace_id: trace_id__.unwrap_or_default(),
                    sampling_mechanism: sampling_mechanism__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.trace.idx.TraceChunk", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for TracerPayload {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.strings.is_empty() {
            len += 1;
        }
        if self.container_id_ref != 0 {
            len += 1;
        }
        if self.language_name_ref != 0 {
            len += 1;
        }
        if self.language_version_ref != 0 {
            len += 1;
        }
        if self.tracer_version_ref != 0 {
            len += 1;
        }
        if self.runtime_id_ref != 0 {
            len += 1;
        }
        if self.env_ref != 0 {
            len += 1;
        }
        if self.hostname_ref != 0 {
            len += 1;
        }
        if self.app_version_ref != 0 {
            len += 1;
        }
        if !self.attributes.is_empty() {
            len += 1;
        }
        if !self.chunks.is_empty() {
            len += 1;
        }
        if self.container_debug.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.trace.idx.TracerPayload", len)?;
        if !self.strings.is_empty() {
            struct_ser.serialize_field("strings", &self.strings)?;
        }
        if self.container_id_ref != 0 {
            struct_ser.serialize_field("containerIDRef", &self.container_id_ref)?;
        }
        if self.language_name_ref != 0 {
            struct_ser.serialize_field("languageNameRef", &self.language_name_ref)?;
        }
        if self.language_version_ref != 0 {
            struct_ser.serialize_field("languageVersionRef", &self.language_version_ref)?;
        }
        if self.tracer_version_ref != 0 {
            struct_ser.serialize_field("tracerVersionRef", &self.tracer_version_ref)?;
        }
        if self.runtime_id_ref != 0 {
            struct_ser.serialize_field("runtimeIDRef", &self.runtime_id_ref)?;
        }
        if self.env_ref != 0 {
            struct_ser.serialize_field("envRef", &self.env_ref)?;
        }
        if self.hostname_ref != 0 {
            struct_ser.serialize_field("hostnameRef", &self.hostname_ref)?;
        }
        if self.app_version_ref != 0 {
            struct_ser.serialize_field("appVersionRef", &self.app_version_ref)?;
        }
        if !self.attributes.is_empty() {
            struct_ser.serialize_field("attributes", &self.attributes)?;
        }
        if !self.chunks.is_empty() {
            struct_ser.serialize_field("chunks", &self.chunks)?;
        }
        if let Some(v) = self.container_debug.as_ref() {
            struct_ser.serialize_field("containerDebug", v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for TracerPayload {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "strings",
            "containerIDRef",
            "languageNameRef",
            "languageVersionRef",
            "tracerVersionRef",
            "runtimeIDRef",
            "envRef",
            "hostnameRef",
            "appVersionRef",
            "attributes",
            "chunks",
            "containerDebug",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Strings,
            ContainerIdRef,
            LanguageNameRef,
            LanguageVersionRef,
            TracerVersionRef,
            RuntimeIdRef,
            EnvRef,
            HostnameRef,
            AppVersionRef,
            Attributes,
            Chunks,
            ContainerDebug,
            __SkipField__,
        }
        impl<'de> serde::Deserialize<'de> for GeneratedField {
            fn deserialize<D>(deserializer: D) -> std::result::Result<GeneratedField, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                struct GeneratedVisitor;

                impl serde::de::Visitor<'_> for GeneratedVisitor {
                    type Value = GeneratedField;

                    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                        write!(formatter, "expected one of: {:?}", &FIELDS)
                    }

                    #[allow(unused_variables)]
                    fn visit_str<E>(self, value: &str) -> std::result::Result<GeneratedField, E>
                    where
                        E: serde::de::Error,
                    {
                        match value {
                            "strings" => Ok(GeneratedField::Strings),
                            "containerIDRef" => Ok(GeneratedField::ContainerIdRef),
                            "languageNameRef" => Ok(GeneratedField::LanguageNameRef),
                            "languageVersionRef" => Ok(GeneratedField::LanguageVersionRef),
                            "tracerVersionRef" => Ok(GeneratedField::TracerVersionRef),
                            "runtimeIDRef" => Ok(GeneratedField::RuntimeIdRef),
                            "envRef" => Ok(GeneratedField::EnvRef),
                            "hostnameRef" => Ok(GeneratedField::HostnameRef),
                            "appVersionRef" => Ok(GeneratedField::AppVersionRef),
                            "attributes" => Ok(GeneratedField::Attributes),
                            "chunks" => Ok(GeneratedField::Chunks),
                            "containerDebug" => Ok(GeneratedField::ContainerDebug),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = TracerPayload;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.trace.idx.TracerPayload")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<TracerPayload, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut strings__ = None;
                let mut container_id_ref__ = None;
                let mut language_name_ref__ = None;
                let mut language_version_ref__ = None;
                let mut tracer_version_ref__ = None;
                let mut runtime_id_ref__ = None;
                let mut env_ref__ = None;
                let mut hostname_ref__ = None;
                let mut app_version_ref__ = None;
                let mut attributes__ = None;
                let mut chunks__ = None;
                let mut container_debug__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Strings => {
                            if strings__.is_some() {
                                return Err(serde::de::Error::duplicate_field("strings"));
                            }
                            strings__ = Some(map_.next_value()?);
                        }
                        GeneratedField::ContainerIdRef => {
                            if container_id_ref__.is_some() {
                                return Err(serde::de::Error::duplicate_field("containerIDRef"));
                            }
                            container_id_ref__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::LanguageNameRef => {
                            if language_name_ref__.is_some() {
                                return Err(serde::de::Error::duplicate_field("languageNameRef"));
                            }
                            language_name_ref__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::LanguageVersionRef => {
                            if language_version_ref__.is_some() {
                                return Err(serde::de::Error::duplicate_field("languageVersionRef"));
                            }
                            language_version_ref__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::TracerVersionRef => {
                            if tracer_version_ref__.is_some() {
                                return Err(serde::de::Error::duplicate_field("tracerVersionRef"));
                            }
                            tracer_version_ref__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::RuntimeIdRef => {
                            if runtime_id_ref__.is_some() {
                                return Err(serde::de::Error::duplicate_field("runtimeIDRef"));
                            }
                            runtime_id_ref__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::EnvRef => {
                            if env_ref__.is_some() {
                                return Err(serde::de::Error::duplicate_field("envRef"));
                            }
                            env_ref__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::HostnameRef => {
                            if hostname_ref__.is_some() {
                                return Err(serde::de::Error::duplicate_field("hostnameRef"));
                            }
                            hostname_ref__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::AppVersionRef => {
                            if app_version_ref__.is_some() {
                                return Err(serde::de::Error::duplicate_field("appVersionRef"));
                            }
                            app_version_ref__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Attributes => {
                            if attributes__.is_some() {
                                return Err(serde::de::Error::duplicate_field("attributes"));
                            }
                            attributes__ = Some(
                                map_.next_value::<std::collections::HashMap<::pbjson::private::NumberDeserialize<u32>, _>>()?
                                    .into_iter().map(|(k,v)| (k.0, v)).collect()
                            );
                        }
                        GeneratedField::Chunks => {
                            if chunks__.is_some() {
                                return Err(serde::de::Error::duplicate_field("chunks"));
                            }
                            chunks__ = Some(map_.next_value()?);
                        }
                        GeneratedField::ContainerDebug => {
                            if container_debug__.is_some() {
                                return Err(serde::de::Error::duplicate_field("containerDebug"));
                            }
                            container_debug__ = map_.next_value()?;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(TracerPayload {
                    strings: strings__.unwrap_or_default(),
                    container_id_ref: container_id_ref__.unwrap_or_default(),
                    language_name_ref: language_name_ref__.unwrap_or_default(),
                    language_version_ref: language_version_ref__.unwrap_or_default(),
                    tracer_version_ref: tracer_version_ref__.unwrap_or_default(),
                    runtime_id_ref: runtime_id_ref__.unwrap_or_default(),
                    env_ref: env_ref__.unwrap_or_default(),
                    hostname_ref: hostname_ref__.unwrap_or_default(),
                    app_version_ref: app_version_ref__.unwrap_or_default(),
                    attributes: attributes__.unwrap_or_default(),
                    chunks: chunks__.unwrap_or_default(),
                    container_debug: container_debug__,
                })
            }
        }
        deserializer.deserialize_struct("datadog.trace.idx.TracerPayload", FIELDS, GeneratedVisitor)
    }
}
