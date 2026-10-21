impl serde::Serialize for AgentPayload {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.host_name.is_empty() {
            len += 1;
        }
        if !self.env.is_empty() {
            len += 1;
        }
        if !self.tracer_payloads.is_empty() {
            len += 1;
        }
        if !self.tags.is_empty() {
            len += 1;
        }
        if !self.agent_version.is_empty() {
            len += 1;
        }
        if self.target_tps != 0. {
            len += 1;
        }
        if self.error_tps != 0. {
            len += 1;
        }
        if self.rare_sampler_enabled {
            len += 1;
        }
        if !self.idx_tracer_payloads.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.trace.AgentPayload", len)?;
        if !self.host_name.is_empty() {
            struct_ser.serialize_field("hostName", &self.host_name)?;
        }
        if !self.env.is_empty() {
            struct_ser.serialize_field("env", &self.env)?;
        }
        if !self.tracer_payloads.is_empty() {
            struct_ser.serialize_field("tracerPayloads", &self.tracer_payloads)?;
        }
        if !self.tags.is_empty() {
            struct_ser.serialize_field("tags", &self.tags)?;
        }
        if !self.agent_version.is_empty() {
            struct_ser.serialize_field("agentVersion", &self.agent_version)?;
        }
        if self.target_tps != 0. {
            struct_ser.serialize_field("targetTPS", &self.target_tps)?;
        }
        if self.error_tps != 0. {
            struct_ser.serialize_field("errorTPS", &self.error_tps)?;
        }
        if self.rare_sampler_enabled {
            struct_ser.serialize_field("rareSamplerEnabled", &self.rare_sampler_enabled)?;
        }
        if !self.idx_tracer_payloads.is_empty() {
            struct_ser.serialize_field("idxTracerPayloads", &self.idx_tracer_payloads)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for AgentPayload {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "hostName",
            "env",
            "tracerPayloads",
            "tags",
            "agentVersion",
            "targetTPS",
            "errorTPS",
            "rareSamplerEnabled",
            "idxTracerPayloads",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            HostName,
            Env,
            TracerPayloads,
            Tags,
            AgentVersion,
            TargetTps,
            ErrorTps,
            RareSamplerEnabled,
            IdxTracerPayloads,
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
                            "hostName" => Ok(GeneratedField::HostName),
                            "env" => Ok(GeneratedField::Env),
                            "tracerPayloads" => Ok(GeneratedField::TracerPayloads),
                            "tags" => Ok(GeneratedField::Tags),
                            "agentVersion" => Ok(GeneratedField::AgentVersion),
                            "targetTPS" => Ok(GeneratedField::TargetTps),
                            "errorTPS" => Ok(GeneratedField::ErrorTps),
                            "rareSamplerEnabled" => Ok(GeneratedField::RareSamplerEnabled),
                            "idxTracerPayloads" => Ok(GeneratedField::IdxTracerPayloads),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = AgentPayload;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.trace.AgentPayload")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<AgentPayload, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut host_name__ = None;
                let mut env__ = None;
                let mut tracer_payloads__ = None;
                let mut tags__ = None;
                let mut agent_version__ = None;
                let mut target_tps__ = None;
                let mut error_tps__ = None;
                let mut rare_sampler_enabled__ = None;
                let mut idx_tracer_payloads__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::HostName => {
                            if host_name__.is_some() {
                                return Err(serde::de::Error::duplicate_field("hostName"));
                            }
                            host_name__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Env => {
                            if env__.is_some() {
                                return Err(serde::de::Error::duplicate_field("env"));
                            }
                            env__ = Some(map_.next_value()?);
                        }
                        GeneratedField::TracerPayloads => {
                            if tracer_payloads__.is_some() {
                                return Err(serde::de::Error::duplicate_field("tracerPayloads"));
                            }
                            tracer_payloads__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Tags => {
                            if tags__.is_some() {
                                return Err(serde::de::Error::duplicate_field("tags"));
                            }
                            tags__ = Some(
                                map_.next_value::<std::collections::HashMap<_, _>>()?
                            );
                        }
                        GeneratedField::AgentVersion => {
                            if agent_version__.is_some() {
                                return Err(serde::de::Error::duplicate_field("agentVersion"));
                            }
                            agent_version__ = Some(map_.next_value()?);
                        }
                        GeneratedField::TargetTps => {
                            if target_tps__.is_some() {
                                return Err(serde::de::Error::duplicate_field("targetTPS"));
                            }
                            target_tps__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::ErrorTps => {
                            if error_tps__.is_some() {
                                return Err(serde::de::Error::duplicate_field("errorTPS"));
                            }
                            error_tps__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::RareSamplerEnabled => {
                            if rare_sampler_enabled__.is_some() {
                                return Err(serde::de::Error::duplicate_field("rareSamplerEnabled"));
                            }
                            rare_sampler_enabled__ = Some(map_.next_value()?);
                        }
                        GeneratedField::IdxTracerPayloads => {
                            if idx_tracer_payloads__.is_some() {
                                return Err(serde::de::Error::duplicate_field("idxTracerPayloads"));
                            }
                            idx_tracer_payloads__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(AgentPayload {
                    host_name: host_name__.unwrap_or_default(),
                    env: env__.unwrap_or_default(),
                    tracer_payloads: tracer_payloads__.unwrap_or_default(),
                    tags: tags__.unwrap_or_default(),
                    agent_version: agent_version__.unwrap_or_default(),
                    target_tps: target_tps__.unwrap_or_default(),
                    error_tps: error_tps__.unwrap_or_default(),
                    rare_sampler_enabled: rare_sampler_enabled__.unwrap_or_default(),
                    idx_tracer_payloads: idx_tracer_payloads__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.trace.AgentPayload", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for AttributeAnyValue {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.r#type != 0 {
            len += 1;
        }
        if !self.string_value.is_empty() {
            len += 1;
        }
        if self.bool_value {
            len += 1;
        }
        if self.int_value != 0 {
            len += 1;
        }
        if self.double_value != 0. {
            len += 1;
        }
        if self.array_value.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.trace.AttributeAnyValue", len)?;
        if self.r#type != 0 {
            let v = attribute_any_value::AttributeAnyValueType::try_from(self.r#type)
                .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", self.r#type)))?;
            struct_ser.serialize_field("type", &v)?;
        }
        if !self.string_value.is_empty() {
            struct_ser.serialize_field("stringValue", &self.string_value)?;
        }
        if self.bool_value {
            struct_ser.serialize_field("boolValue", &self.bool_value)?;
        }
        if self.int_value != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("intValue", ToString::to_string(&self.int_value).as_str())?;
        }
        if self.double_value != 0. {
            struct_ser.serialize_field("doubleValue", &self.double_value)?;
        }
        if let Some(v) = self.array_value.as_ref() {
            struct_ser.serialize_field("arrayValue", v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for AttributeAnyValue {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "type",
            "string_value",
            "stringValue",
            "bool_value",
            "boolValue",
            "int_value",
            "intValue",
            "double_value",
            "doubleValue",
            "array_value",
            "arrayValue",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Type,
            StringValue,
            BoolValue,
            IntValue,
            DoubleValue,
            ArrayValue,
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
                            "type" => Ok(GeneratedField::Type),
                            "stringValue" | "string_value" => Ok(GeneratedField::StringValue),
                            "boolValue" | "bool_value" => Ok(GeneratedField::BoolValue),
                            "intValue" | "int_value" => Ok(GeneratedField::IntValue),
                            "doubleValue" | "double_value" => Ok(GeneratedField::DoubleValue),
                            "arrayValue" | "array_value" => Ok(GeneratedField::ArrayValue),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = AttributeAnyValue;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.trace.AttributeAnyValue")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<AttributeAnyValue, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut r#type__ = None;
                let mut string_value__ = None;
                let mut bool_value__ = None;
                let mut int_value__ = None;
                let mut double_value__ = None;
                let mut array_value__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Type => {
                            if r#type__.is_some() {
                                return Err(serde::de::Error::duplicate_field("type"));
                            }
                            r#type__ = Some(map_.next_value::<attribute_any_value::AttributeAnyValueType>()? as i32);
                        }
                        GeneratedField::StringValue => {
                            if string_value__.is_some() {
                                return Err(serde::de::Error::duplicate_field("stringValue"));
                            }
                            string_value__ = Some(map_.next_value()?);
                        }
                        GeneratedField::BoolValue => {
                            if bool_value__.is_some() {
                                return Err(serde::de::Error::duplicate_field("boolValue"));
                            }
                            bool_value__ = Some(map_.next_value()?);
                        }
                        GeneratedField::IntValue => {
                            if int_value__.is_some() {
                                return Err(serde::de::Error::duplicate_field("intValue"));
                            }
                            int_value__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::DoubleValue => {
                            if double_value__.is_some() {
                                return Err(serde::de::Error::duplicate_field("doubleValue"));
                            }
                            double_value__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::ArrayValue => {
                            if array_value__.is_some() {
                                return Err(serde::de::Error::duplicate_field("arrayValue"));
                            }
                            array_value__ = map_.next_value()?;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(AttributeAnyValue {
                    r#type: r#type__.unwrap_or_default(),
                    string_value: string_value__.unwrap_or_default(),
                    bool_value: bool_value__.unwrap_or_default(),
                    int_value: int_value__.unwrap_or_default(),
                    double_value: double_value__.unwrap_or_default(),
                    array_value: array_value__,
                })
            }
        }
        deserializer.deserialize_struct("datadog.trace.AttributeAnyValue", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for attribute_any_value::AttributeAnyValueType {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let variant = match self {
            Self::StringValue => "STRING_VALUE",
            Self::BoolValue => "BOOL_VALUE",
            Self::IntValue => "INT_VALUE",
            Self::DoubleValue => "DOUBLE_VALUE",
            Self::ArrayValue => "ARRAY_VALUE",
        };
        serializer.serialize_str(variant)
    }
}
impl<'de> serde::Deserialize<'de> for attribute_any_value::AttributeAnyValueType {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "STRING_VALUE",
            "BOOL_VALUE",
            "INT_VALUE",
            "DOUBLE_VALUE",
            "ARRAY_VALUE",
        ];

        struct GeneratedVisitor;

        impl serde::de::Visitor<'_> for GeneratedVisitor {
            type Value = attribute_any_value::AttributeAnyValueType;

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
                    "STRING_VALUE" => Ok(attribute_any_value::AttributeAnyValueType::StringValue),
                    "BOOL_VALUE" => Ok(attribute_any_value::AttributeAnyValueType::BoolValue),
                    "INT_VALUE" => Ok(attribute_any_value::AttributeAnyValueType::IntValue),
                    "DOUBLE_VALUE" => Ok(attribute_any_value::AttributeAnyValueType::DoubleValue),
                    "ARRAY_VALUE" => Ok(attribute_any_value::AttributeAnyValueType::ArrayValue),
                    _ => Err(serde::de::Error::unknown_variant(value, FIELDS)),
                }
            }
        }
        deserializer.deserialize_any(GeneratedVisitor)
    }
}
impl serde::Serialize for AttributeArray {
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
        let mut struct_ser = serializer.serialize_struct("datadog.trace.AttributeArray", len)?;
        if !self.values.is_empty() {
            struct_ser.serialize_field("values", &self.values)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for AttributeArray {
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
            type Value = AttributeArray;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.trace.AttributeArray")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<AttributeArray, V::Error>
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
                Ok(AttributeArray {
                    values: values__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.trace.AttributeArray", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for AttributeArrayValue {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.r#type != 0 {
            len += 1;
        }
        if !self.string_value.is_empty() {
            len += 1;
        }
        if self.bool_value {
            len += 1;
        }
        if self.int_value != 0 {
            len += 1;
        }
        if self.double_value != 0. {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.trace.AttributeArrayValue", len)?;
        if self.r#type != 0 {
            let v = attribute_array_value::AttributeArrayValueType::try_from(self.r#type)
                .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", self.r#type)))?;
            struct_ser.serialize_field("type", &v)?;
        }
        if !self.string_value.is_empty() {
            struct_ser.serialize_field("stringValue", &self.string_value)?;
        }
        if self.bool_value {
            struct_ser.serialize_field("boolValue", &self.bool_value)?;
        }
        if self.int_value != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("intValue", ToString::to_string(&self.int_value).as_str())?;
        }
        if self.double_value != 0. {
            struct_ser.serialize_field("doubleValue", &self.double_value)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for AttributeArrayValue {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "type",
            "string_value",
            "stringValue",
            "bool_value",
            "boolValue",
            "int_value",
            "intValue",
            "double_value",
            "doubleValue",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Type,
            StringValue,
            BoolValue,
            IntValue,
            DoubleValue,
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
                            "type" => Ok(GeneratedField::Type),
                            "stringValue" | "string_value" => Ok(GeneratedField::StringValue),
                            "boolValue" | "bool_value" => Ok(GeneratedField::BoolValue),
                            "intValue" | "int_value" => Ok(GeneratedField::IntValue),
                            "doubleValue" | "double_value" => Ok(GeneratedField::DoubleValue),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = AttributeArrayValue;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.trace.AttributeArrayValue")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<AttributeArrayValue, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut r#type__ = None;
                let mut string_value__ = None;
                let mut bool_value__ = None;
                let mut int_value__ = None;
                let mut double_value__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Type => {
                            if r#type__.is_some() {
                                return Err(serde::de::Error::duplicate_field("type"));
                            }
                            r#type__ = Some(map_.next_value::<attribute_array_value::AttributeArrayValueType>()? as i32);
                        }
                        GeneratedField::StringValue => {
                            if string_value__.is_some() {
                                return Err(serde::de::Error::duplicate_field("stringValue"));
                            }
                            string_value__ = Some(map_.next_value()?);
                        }
                        GeneratedField::BoolValue => {
                            if bool_value__.is_some() {
                                return Err(serde::de::Error::duplicate_field("boolValue"));
                            }
                            bool_value__ = Some(map_.next_value()?);
                        }
                        GeneratedField::IntValue => {
                            if int_value__.is_some() {
                                return Err(serde::de::Error::duplicate_field("intValue"));
                            }
                            int_value__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::DoubleValue => {
                            if double_value__.is_some() {
                                return Err(serde::de::Error::duplicate_field("doubleValue"));
                            }
                            double_value__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(AttributeArrayValue {
                    r#type: r#type__.unwrap_or_default(),
                    string_value: string_value__.unwrap_or_default(),
                    bool_value: bool_value__.unwrap_or_default(),
                    int_value: int_value__.unwrap_or_default(),
                    double_value: double_value__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.trace.AttributeArrayValue", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for attribute_array_value::AttributeArrayValueType {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let variant = match self {
            Self::StringValue => "STRING_VALUE",
            Self::BoolValue => "BOOL_VALUE",
            Self::IntValue => "INT_VALUE",
            Self::DoubleValue => "DOUBLE_VALUE",
        };
        serializer.serialize_str(variant)
    }
}
impl<'de> serde::Deserialize<'de> for attribute_array_value::AttributeArrayValueType {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "STRING_VALUE",
            "BOOL_VALUE",
            "INT_VALUE",
            "DOUBLE_VALUE",
        ];

        struct GeneratedVisitor;

        impl serde::de::Visitor<'_> for GeneratedVisitor {
            type Value = attribute_array_value::AttributeArrayValueType;

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
                    "STRING_VALUE" => Ok(attribute_array_value::AttributeArrayValueType::StringValue),
                    "BOOL_VALUE" => Ok(attribute_array_value::AttributeArrayValueType::BoolValue),
                    "INT_VALUE" => Ok(attribute_array_value::AttributeArrayValueType::IntValue),
                    "DOUBLE_VALUE" => Ok(attribute_array_value::AttributeArrayValueType::DoubleValue),
                    _ => Err(serde::de::Error::unknown_variant(value, FIELDS)),
                }
            }
        }
        deserializer.deserialize_any(GeneratedVisitor)
    }
}
impl serde::Serialize for ClientGroupedStats {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.service.is_empty() {
            len += 1;
        }
        if !self.name.is_empty() {
            len += 1;
        }
        if !self.resource.is_empty() {
            len += 1;
        }
        if self.http_status_code != 0 {
            len += 1;
        }
        if !self.r#type.is_empty() {
            len += 1;
        }
        if !self.db_type.is_empty() {
            len += 1;
        }
        if self.hits != 0 {
            len += 1;
        }
        if self.errors != 0 {
            len += 1;
        }
        if self.duration != 0 {
            len += 1;
        }
        if !self.ok_summary.is_empty() {
            len += 1;
        }
        if !self.error_summary.is_empty() {
            len += 1;
        }
        if self.synthetics {
            len += 1;
        }
        if self.top_level_hits != 0 {
            len += 1;
        }
        if !self.span_kind.is_empty() {
            len += 1;
        }
        if !self.peer_tags.is_empty() {
            len += 1;
        }
        if self.is_trace_root != 0 {
            len += 1;
        }
        if !self.grpc_status_code.is_empty() {
            len += 1;
        }
        if !self.http_method.is_empty() {
            len += 1;
        }
        if !self.http_endpoint.is_empty() {
            len += 1;
        }
        if !self.service_source.is_empty() {
            len += 1;
        }
        if !self.span_derived_primary_tags.is_empty() {
            len += 1;
        }
        if !self.additional_metric_tags.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.trace.ClientGroupedStats", len)?;
        if !self.service.is_empty() {
            struct_ser.serialize_field("service", &self.service)?;
        }
        if !self.name.is_empty() {
            struct_ser.serialize_field("name", &self.name)?;
        }
        if !self.resource.is_empty() {
            struct_ser.serialize_field("resource", &self.resource)?;
        }
        if self.http_status_code != 0 {
            struct_ser.serialize_field("HTTPStatusCode", &self.http_status_code)?;
        }
        if !self.r#type.is_empty() {
            struct_ser.serialize_field("type", &self.r#type)?;
        }
        if !self.db_type.is_empty() {
            struct_ser.serialize_field("DBType", &self.db_type)?;
        }
        if self.hits != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("hits", ToString::to_string(&self.hits).as_str())?;
        }
        if self.errors != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("errors", ToString::to_string(&self.errors).as_str())?;
        }
        if self.duration != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("duration", ToString::to_string(&self.duration).as_str())?;
        }
        if !self.ok_summary.is_empty() {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("okSummary", pbjson::private::base64::encode(&self.ok_summary).as_str())?;
        }
        if !self.error_summary.is_empty() {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("errorSummary", pbjson::private::base64::encode(&self.error_summary).as_str())?;
        }
        if self.synthetics {
            struct_ser.serialize_field("synthetics", &self.synthetics)?;
        }
        if self.top_level_hits != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("topLevelHits", ToString::to_string(&self.top_level_hits).as_str())?;
        }
        if !self.span_kind.is_empty() {
            struct_ser.serialize_field("spanKind", &self.span_kind)?;
        }
        if !self.peer_tags.is_empty() {
            struct_ser.serialize_field("peerTags", &self.peer_tags)?;
        }
        if self.is_trace_root != 0 {
            let v = Trilean::try_from(self.is_trace_root)
                .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", self.is_trace_root)))?;
            struct_ser.serialize_field("isTraceRoot", &v)?;
        }
        if !self.grpc_status_code.is_empty() {
            struct_ser.serialize_field("GRPCStatusCode", &self.grpc_status_code)?;
        }
        if !self.http_method.is_empty() {
            struct_ser.serialize_field("HTTPMethod", &self.http_method)?;
        }
        if !self.http_endpoint.is_empty() {
            struct_ser.serialize_field("HTTPEndpoint", &self.http_endpoint)?;
        }
        if !self.service_source.is_empty() {
            struct_ser.serialize_field("serviceSource", &self.service_source)?;
        }
        if !self.span_derived_primary_tags.is_empty() {
            struct_ser.serialize_field("spanDerivedPrimaryTags", &self.span_derived_primary_tags)?;
        }
        if !self.additional_metric_tags.is_empty() {
            struct_ser.serialize_field("additionalMetricTags", &self.additional_metric_tags)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for ClientGroupedStats {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "service",
            "name",
            "resource",
            "HTTP_status_code",
            "HTTPStatusCode",
            "type",
            "DB_type",
            "DBType",
            "hits",
            "errors",
            "duration",
            "okSummary",
            "errorSummary",
            "synthetics",
            "topLevelHits",
            "span_kind",
            "spanKind",
            "peer_tags",
            "peerTags",
            "is_trace_root",
            "isTraceRoot",
            "GRPC_status_code",
            "GRPCStatusCode",
            "HTTP_method",
            "HTTPMethod",
            "HTTP_endpoint",
            "HTTPEndpoint",
            "service_source",
            "serviceSource",
            "span_derived_primary_tags",
            "spanDerivedPrimaryTags",
            "additional_metric_tags",
            "additionalMetricTags",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Service,
            Name,
            Resource,
            HttpStatusCode,
            Type,
            DbType,
            Hits,
            Errors,
            Duration,
            OkSummary,
            ErrorSummary,
            Synthetics,
            TopLevelHits,
            SpanKind,
            PeerTags,
            IsTraceRoot,
            GrpcStatusCode,
            HttpMethod,
            HttpEndpoint,
            ServiceSource,
            SpanDerivedPrimaryTags,
            AdditionalMetricTags,
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
                            "service" => Ok(GeneratedField::Service),
                            "name" => Ok(GeneratedField::Name),
                            "resource" => Ok(GeneratedField::Resource),
                            "HTTPStatusCode" | "HTTP_status_code" => Ok(GeneratedField::HttpStatusCode),
                            "type" => Ok(GeneratedField::Type),
                            "DBType" | "DB_type" => Ok(GeneratedField::DbType),
                            "hits" => Ok(GeneratedField::Hits),
                            "errors" => Ok(GeneratedField::Errors),
                            "duration" => Ok(GeneratedField::Duration),
                            "okSummary" => Ok(GeneratedField::OkSummary),
                            "errorSummary" => Ok(GeneratedField::ErrorSummary),
                            "synthetics" => Ok(GeneratedField::Synthetics),
                            "topLevelHits" => Ok(GeneratedField::TopLevelHits),
                            "spanKind" | "span_kind" => Ok(GeneratedField::SpanKind),
                            "peerTags" | "peer_tags" => Ok(GeneratedField::PeerTags),
                            "isTraceRoot" | "is_trace_root" => Ok(GeneratedField::IsTraceRoot),
                            "GRPCStatusCode" | "GRPC_status_code" => Ok(GeneratedField::GrpcStatusCode),
                            "HTTPMethod" | "HTTP_method" => Ok(GeneratedField::HttpMethod),
                            "HTTPEndpoint" | "HTTP_endpoint" => Ok(GeneratedField::HttpEndpoint),
                            "serviceSource" | "service_source" => Ok(GeneratedField::ServiceSource),
                            "spanDerivedPrimaryTags" | "span_derived_primary_tags" => Ok(GeneratedField::SpanDerivedPrimaryTags),
                            "additionalMetricTags" | "additional_metric_tags" => Ok(GeneratedField::AdditionalMetricTags),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = ClientGroupedStats;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.trace.ClientGroupedStats")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<ClientGroupedStats, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut service__ = None;
                let mut name__ = None;
                let mut resource__ = None;
                let mut http_status_code__ = None;
                let mut r#type__ = None;
                let mut db_type__ = None;
                let mut hits__ = None;
                let mut errors__ = None;
                let mut duration__ = None;
                let mut ok_summary__ = None;
                let mut error_summary__ = None;
                let mut synthetics__ = None;
                let mut top_level_hits__ = None;
                let mut span_kind__ = None;
                let mut peer_tags__ = None;
                let mut is_trace_root__ = None;
                let mut grpc_status_code__ = None;
                let mut http_method__ = None;
                let mut http_endpoint__ = None;
                let mut service_source__ = None;
                let mut span_derived_primary_tags__ = None;
                let mut additional_metric_tags__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Service => {
                            if service__.is_some() {
                                return Err(serde::de::Error::duplicate_field("service"));
                            }
                            service__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Name => {
                            if name__.is_some() {
                                return Err(serde::de::Error::duplicate_field("name"));
                            }
                            name__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Resource => {
                            if resource__.is_some() {
                                return Err(serde::de::Error::duplicate_field("resource"));
                            }
                            resource__ = Some(map_.next_value()?);
                        }
                        GeneratedField::HttpStatusCode => {
                            if http_status_code__.is_some() {
                                return Err(serde::de::Error::duplicate_field("HTTPStatusCode"));
                            }
                            http_status_code__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Type => {
                            if r#type__.is_some() {
                                return Err(serde::de::Error::duplicate_field("type"));
                            }
                            r#type__ = Some(map_.next_value()?);
                        }
                        GeneratedField::DbType => {
                            if db_type__.is_some() {
                                return Err(serde::de::Error::duplicate_field("DBType"));
                            }
                            db_type__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Hits => {
                            if hits__.is_some() {
                                return Err(serde::de::Error::duplicate_field("hits"));
                            }
                            hits__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Errors => {
                            if errors__.is_some() {
                                return Err(serde::de::Error::duplicate_field("errors"));
                            }
                            errors__ = 
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
                        GeneratedField::OkSummary => {
                            if ok_summary__.is_some() {
                                return Err(serde::de::Error::duplicate_field("okSummary"));
                            }
                            ok_summary__ = 
                                Some(map_.next_value::<::pbjson::private::BytesDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::ErrorSummary => {
                            if error_summary__.is_some() {
                                return Err(serde::de::Error::duplicate_field("errorSummary"));
                            }
                            error_summary__ = 
                                Some(map_.next_value::<::pbjson::private::BytesDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Synthetics => {
                            if synthetics__.is_some() {
                                return Err(serde::de::Error::duplicate_field("synthetics"));
                            }
                            synthetics__ = Some(map_.next_value()?);
                        }
                        GeneratedField::TopLevelHits => {
                            if top_level_hits__.is_some() {
                                return Err(serde::de::Error::duplicate_field("topLevelHits"));
                            }
                            top_level_hits__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::SpanKind => {
                            if span_kind__.is_some() {
                                return Err(serde::de::Error::duplicate_field("spanKind"));
                            }
                            span_kind__ = Some(map_.next_value()?);
                        }
                        GeneratedField::PeerTags => {
                            if peer_tags__.is_some() {
                                return Err(serde::de::Error::duplicate_field("peerTags"));
                            }
                            peer_tags__ = Some(map_.next_value()?);
                        }
                        GeneratedField::IsTraceRoot => {
                            if is_trace_root__.is_some() {
                                return Err(serde::de::Error::duplicate_field("isTraceRoot"));
                            }
                            is_trace_root__ = Some(map_.next_value::<Trilean>()? as i32);
                        }
                        GeneratedField::GrpcStatusCode => {
                            if grpc_status_code__.is_some() {
                                return Err(serde::de::Error::duplicate_field("GRPCStatusCode"));
                            }
                            grpc_status_code__ = Some(map_.next_value()?);
                        }
                        GeneratedField::HttpMethod => {
                            if http_method__.is_some() {
                                return Err(serde::de::Error::duplicate_field("HTTPMethod"));
                            }
                            http_method__ = Some(map_.next_value()?);
                        }
                        GeneratedField::HttpEndpoint => {
                            if http_endpoint__.is_some() {
                                return Err(serde::de::Error::duplicate_field("HTTPEndpoint"));
                            }
                            http_endpoint__ = Some(map_.next_value()?);
                        }
                        GeneratedField::ServiceSource => {
                            if service_source__.is_some() {
                                return Err(serde::de::Error::duplicate_field("serviceSource"));
                            }
                            service_source__ = Some(map_.next_value()?);
                        }
                        GeneratedField::SpanDerivedPrimaryTags => {
                            if span_derived_primary_tags__.is_some() {
                                return Err(serde::de::Error::duplicate_field("spanDerivedPrimaryTags"));
                            }
                            span_derived_primary_tags__ = Some(map_.next_value()?);
                        }
                        GeneratedField::AdditionalMetricTags => {
                            if additional_metric_tags__.is_some() {
                                return Err(serde::de::Error::duplicate_field("additionalMetricTags"));
                            }
                            additional_metric_tags__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(ClientGroupedStats {
                    service: service__.unwrap_or_default(),
                    name: name__.unwrap_or_default(),
                    resource: resource__.unwrap_or_default(),
                    http_status_code: http_status_code__.unwrap_or_default(),
                    r#type: r#type__.unwrap_or_default(),
                    db_type: db_type__.unwrap_or_default(),
                    hits: hits__.unwrap_or_default(),
                    errors: errors__.unwrap_or_default(),
                    duration: duration__.unwrap_or_default(),
                    ok_summary: ok_summary__.unwrap_or_default(),
                    error_summary: error_summary__.unwrap_or_default(),
                    synthetics: synthetics__.unwrap_or_default(),
                    top_level_hits: top_level_hits__.unwrap_or_default(),
                    span_kind: span_kind__.unwrap_or_default(),
                    peer_tags: peer_tags__.unwrap_or_default(),
                    is_trace_root: is_trace_root__.unwrap_or_default(),
                    grpc_status_code: grpc_status_code__.unwrap_or_default(),
                    http_method: http_method__.unwrap_or_default(),
                    http_endpoint: http_endpoint__.unwrap_or_default(),
                    service_source: service_source__.unwrap_or_default(),
                    span_derived_primary_tags: span_derived_primary_tags__.unwrap_or_default(),
                    additional_metric_tags: additional_metric_tags__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.trace.ClientGroupedStats", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for ClientStatsBucket {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.start != 0 {
            len += 1;
        }
        if self.duration != 0 {
            len += 1;
        }
        if !self.stats.is_empty() {
            len += 1;
        }
        if self.agent_time_shift != 0 {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.trace.ClientStatsBucket", len)?;
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
        if !self.stats.is_empty() {
            struct_ser.serialize_field("stats", &self.stats)?;
        }
        if self.agent_time_shift != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("agentTimeShift", ToString::to_string(&self.agent_time_shift).as_str())?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for ClientStatsBucket {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "start",
            "duration",
            "stats",
            "agentTimeShift",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Start,
            Duration,
            Stats,
            AgentTimeShift,
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
                            "start" => Ok(GeneratedField::Start),
                            "duration" => Ok(GeneratedField::Duration),
                            "stats" => Ok(GeneratedField::Stats),
                            "agentTimeShift" => Ok(GeneratedField::AgentTimeShift),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = ClientStatsBucket;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.trace.ClientStatsBucket")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<ClientStatsBucket, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut start__ = None;
                let mut duration__ = None;
                let mut stats__ = None;
                let mut agent_time_shift__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
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
                        GeneratedField::Stats => {
                            if stats__.is_some() {
                                return Err(serde::de::Error::duplicate_field("stats"));
                            }
                            stats__ = Some(map_.next_value()?);
                        }
                        GeneratedField::AgentTimeShift => {
                            if agent_time_shift__.is_some() {
                                return Err(serde::de::Error::duplicate_field("agentTimeShift"));
                            }
                            agent_time_shift__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(ClientStatsBucket {
                    start: start__.unwrap_or_default(),
                    duration: duration__.unwrap_or_default(),
                    stats: stats__.unwrap_or_default(),
                    agent_time_shift: agent_time_shift__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.trace.ClientStatsBucket", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for ClientStatsPayload {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.hostname.is_empty() {
            len += 1;
        }
        if !self.env.is_empty() {
            len += 1;
        }
        if !self.version.is_empty() {
            len += 1;
        }
        if !self.stats.is_empty() {
            len += 1;
        }
        if !self.lang.is_empty() {
            len += 1;
        }
        if !self.tracer_version.is_empty() {
            len += 1;
        }
        if !self.runtime_id.is_empty() {
            len += 1;
        }
        if self.sequence != 0 {
            len += 1;
        }
        if !self.agent_aggregation.is_empty() {
            len += 1;
        }
        if !self.service.is_empty() {
            len += 1;
        }
        if !self.container_id.is_empty() {
            len += 1;
        }
        if !self.tags.is_empty() {
            len += 1;
        }
        if !self.git_commit_sha.is_empty() {
            len += 1;
        }
        if !self.image_tag.is_empty() {
            len += 1;
        }
        if self.process_tags_hash != 0 {
            len += 1;
        }
        if !self.process_tags.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.trace.ClientStatsPayload", len)?;
        if !self.hostname.is_empty() {
            struct_ser.serialize_field("hostname", &self.hostname)?;
        }
        if !self.env.is_empty() {
            struct_ser.serialize_field("env", &self.env)?;
        }
        if !self.version.is_empty() {
            struct_ser.serialize_field("version", &self.version)?;
        }
        if !self.stats.is_empty() {
            struct_ser.serialize_field("stats", &self.stats)?;
        }
        if !self.lang.is_empty() {
            struct_ser.serialize_field("lang", &self.lang)?;
        }
        if !self.tracer_version.is_empty() {
            struct_ser.serialize_field("tracerVersion", &self.tracer_version)?;
        }
        if !self.runtime_id.is_empty() {
            struct_ser.serialize_field("runtimeID", &self.runtime_id)?;
        }
        if self.sequence != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("sequence", ToString::to_string(&self.sequence).as_str())?;
        }
        if !self.agent_aggregation.is_empty() {
            struct_ser.serialize_field("agentAggregation", &self.agent_aggregation)?;
        }
        if !self.service.is_empty() {
            struct_ser.serialize_field("service", &self.service)?;
        }
        if !self.container_id.is_empty() {
            struct_ser.serialize_field("containerID", &self.container_id)?;
        }
        if !self.tags.is_empty() {
            struct_ser.serialize_field("tags", &self.tags)?;
        }
        if !self.git_commit_sha.is_empty() {
            struct_ser.serialize_field("gitCommitSha", &self.git_commit_sha)?;
        }
        if !self.image_tag.is_empty() {
            struct_ser.serialize_field("imageTag", &self.image_tag)?;
        }
        if self.process_tags_hash != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("processTagsHash", ToString::to_string(&self.process_tags_hash).as_str())?;
        }
        if !self.process_tags.is_empty() {
            struct_ser.serialize_field("processTags", &self.process_tags)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for ClientStatsPayload {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "hostname",
            "env",
            "version",
            "stats",
            "lang",
            "tracerVersion",
            "runtimeID",
            "sequence",
            "agentAggregation",
            "service",
            "containerID",
            "tags",
            "git_commit_sha",
            "gitCommitSha",
            "image_tag",
            "imageTag",
            "process_tags_hash",
            "processTagsHash",
            "process_tags",
            "processTags",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Hostname,
            Env,
            Version,
            Stats,
            Lang,
            TracerVersion,
            RuntimeId,
            Sequence,
            AgentAggregation,
            Service,
            ContainerId,
            Tags,
            GitCommitSha,
            ImageTag,
            ProcessTagsHash,
            ProcessTags,
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
                            "hostname" => Ok(GeneratedField::Hostname),
                            "env" => Ok(GeneratedField::Env),
                            "version" => Ok(GeneratedField::Version),
                            "stats" => Ok(GeneratedField::Stats),
                            "lang" => Ok(GeneratedField::Lang),
                            "tracerVersion" => Ok(GeneratedField::TracerVersion),
                            "runtimeID" => Ok(GeneratedField::RuntimeId),
                            "sequence" => Ok(GeneratedField::Sequence),
                            "agentAggregation" => Ok(GeneratedField::AgentAggregation),
                            "service" => Ok(GeneratedField::Service),
                            "containerID" => Ok(GeneratedField::ContainerId),
                            "tags" => Ok(GeneratedField::Tags),
                            "gitCommitSha" | "git_commit_sha" => Ok(GeneratedField::GitCommitSha),
                            "imageTag" | "image_tag" => Ok(GeneratedField::ImageTag),
                            "processTagsHash" | "process_tags_hash" => Ok(GeneratedField::ProcessTagsHash),
                            "processTags" | "process_tags" => Ok(GeneratedField::ProcessTags),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = ClientStatsPayload;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.trace.ClientStatsPayload")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<ClientStatsPayload, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut hostname__ = None;
                let mut env__ = None;
                let mut version__ = None;
                let mut stats__ = None;
                let mut lang__ = None;
                let mut tracer_version__ = None;
                let mut runtime_id__ = None;
                let mut sequence__ = None;
                let mut agent_aggregation__ = None;
                let mut service__ = None;
                let mut container_id__ = None;
                let mut tags__ = None;
                let mut git_commit_sha__ = None;
                let mut image_tag__ = None;
                let mut process_tags_hash__ = None;
                let mut process_tags__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Hostname => {
                            if hostname__.is_some() {
                                return Err(serde::de::Error::duplicate_field("hostname"));
                            }
                            hostname__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Env => {
                            if env__.is_some() {
                                return Err(serde::de::Error::duplicate_field("env"));
                            }
                            env__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Version => {
                            if version__.is_some() {
                                return Err(serde::de::Error::duplicate_field("version"));
                            }
                            version__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Stats => {
                            if stats__.is_some() {
                                return Err(serde::de::Error::duplicate_field("stats"));
                            }
                            stats__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Lang => {
                            if lang__.is_some() {
                                return Err(serde::de::Error::duplicate_field("lang"));
                            }
                            lang__ = Some(map_.next_value()?);
                        }
                        GeneratedField::TracerVersion => {
                            if tracer_version__.is_some() {
                                return Err(serde::de::Error::duplicate_field("tracerVersion"));
                            }
                            tracer_version__ = Some(map_.next_value()?);
                        }
                        GeneratedField::RuntimeId => {
                            if runtime_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("runtimeID"));
                            }
                            runtime_id__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Sequence => {
                            if sequence__.is_some() {
                                return Err(serde::de::Error::duplicate_field("sequence"));
                            }
                            sequence__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::AgentAggregation => {
                            if agent_aggregation__.is_some() {
                                return Err(serde::de::Error::duplicate_field("agentAggregation"));
                            }
                            agent_aggregation__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Service => {
                            if service__.is_some() {
                                return Err(serde::de::Error::duplicate_field("service"));
                            }
                            service__ = Some(map_.next_value()?);
                        }
                        GeneratedField::ContainerId => {
                            if container_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("containerID"));
                            }
                            container_id__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Tags => {
                            if tags__.is_some() {
                                return Err(serde::de::Error::duplicate_field("tags"));
                            }
                            tags__ = Some(map_.next_value()?);
                        }
                        GeneratedField::GitCommitSha => {
                            if git_commit_sha__.is_some() {
                                return Err(serde::de::Error::duplicate_field("gitCommitSha"));
                            }
                            git_commit_sha__ = Some(map_.next_value()?);
                        }
                        GeneratedField::ImageTag => {
                            if image_tag__.is_some() {
                                return Err(serde::de::Error::duplicate_field("imageTag"));
                            }
                            image_tag__ = Some(map_.next_value()?);
                        }
                        GeneratedField::ProcessTagsHash => {
                            if process_tags_hash__.is_some() {
                                return Err(serde::de::Error::duplicate_field("processTagsHash"));
                            }
                            process_tags_hash__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::ProcessTags => {
                            if process_tags__.is_some() {
                                return Err(serde::de::Error::duplicate_field("processTags"));
                            }
                            process_tags__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(ClientStatsPayload {
                    hostname: hostname__.unwrap_or_default(),
                    env: env__.unwrap_or_default(),
                    version: version__.unwrap_or_default(),
                    stats: stats__.unwrap_or_default(),
                    lang: lang__.unwrap_or_default(),
                    tracer_version: tracer_version__.unwrap_or_default(),
                    runtime_id: runtime_id__.unwrap_or_default(),
                    sequence: sequence__.unwrap_or_default(),
                    agent_aggregation: agent_aggregation__.unwrap_or_default(),
                    service: service__.unwrap_or_default(),
                    container_id: container_id__.unwrap_or_default(),
                    tags: tags__.unwrap_or_default(),
                    git_commit_sha: git_commit_sha__.unwrap_or_default(),
                    image_tag: image_tag__.unwrap_or_default(),
                    process_tags_hash: process_tags_hash__.unwrap_or_default(),
                    process_tags: process_tags__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.trace.ClientStatsPayload", FIELDS, GeneratedVisitor)
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
        if !self.error.is_empty() {
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
        if !self.buffer_eviction_reason.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.trace.ContainerDebug", len)?;
        if !self.error.is_empty() {
            struct_ser.serialize_field("error", &self.error)?;
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
        if !self.buffer_eviction_reason.is_empty() {
            struct_ser.serialize_field("bufferEvictionReason", &self.buffer_eviction_reason)?;
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
            "error",
            "latencyMs",
            "wasBuffered",
            "bufferMs",
            "bufferEvictionReason",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Error,
            LatencyMs,
            WasBuffered,
            BufferMs,
            BufferEvictionReason,
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
                            "error" => Ok(GeneratedField::Error),
                            "latencyMs" => Ok(GeneratedField::LatencyMs),
                            "wasBuffered" => Ok(GeneratedField::WasBuffered),
                            "bufferMs" => Ok(GeneratedField::BufferMs),
                            "bufferEvictionReason" => Ok(GeneratedField::BufferEvictionReason),
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
                formatter.write_str("struct datadog.trace.ContainerDebug")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<ContainerDebug, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut error__ = None;
                let mut latency_ms__ = None;
                let mut was_buffered__ = None;
                let mut buffer_ms__ = None;
                let mut buffer_eviction_reason__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Error => {
                            if error__.is_some() {
                                return Err(serde::de::Error::duplicate_field("error"));
                            }
                            error__ = Some(map_.next_value()?);
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
                        GeneratedField::BufferEvictionReason => {
                            if buffer_eviction_reason__.is_some() {
                                return Err(serde::de::Error::duplicate_field("bufferEvictionReason"));
                            }
                            buffer_eviction_reason__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(ContainerDebug {
                    error: error__.unwrap_or_default(),
                    latency_ms: latency_ms__.unwrap_or_default(),
                    was_buffered: was_buffered__.unwrap_or_default(),
                    buffer_ms: buffer_ms__.unwrap_or_default(),
                    buffer_eviction_reason: buffer_eviction_reason__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.trace.ContainerDebug", FIELDS, GeneratedVisitor)
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
        if !self.service.is_empty() {
            len += 1;
        }
        if !self.name.is_empty() {
            len += 1;
        }
        if !self.resource.is_empty() {
            len += 1;
        }
        if self.trace_id != 0 {
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
        if self.error != 0 {
            len += 1;
        }
        if !self.meta.is_empty() {
            len += 1;
        }
        if !self.metrics.is_empty() {
            len += 1;
        }
        if !self.r#type.is_empty() {
            len += 1;
        }
        if !self.meta_struct.is_empty() {
            len += 1;
        }
        if !self.span_links.is_empty() {
            len += 1;
        }
        if !self.span_events.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.trace.Span", len)?;
        if !self.service.is_empty() {
            struct_ser.serialize_field("service", &self.service)?;
        }
        if !self.name.is_empty() {
            struct_ser.serialize_field("name", &self.name)?;
        }
        if !self.resource.is_empty() {
            struct_ser.serialize_field("resource", &self.resource)?;
        }
        if self.trace_id != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("traceID", ToString::to_string(&self.trace_id).as_str())?;
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
        if self.error != 0 {
            struct_ser.serialize_field("error", &self.error)?;
        }
        if !self.meta.is_empty() {
            struct_ser.serialize_field("meta", &self.meta)?;
        }
        if !self.metrics.is_empty() {
            struct_ser.serialize_field("metrics", &self.metrics)?;
        }
        if !self.r#type.is_empty() {
            struct_ser.serialize_field("type", &self.r#type)?;
        }
        if !self.meta_struct.is_empty() {
            let v: std::collections::HashMap<_, _> = self.meta_struct.iter()
                .map(|(k, v)| (k, pbjson::private::base64::encode(v))).collect();
            struct_ser.serialize_field("metaStruct", &v)?;
        }
        if !self.span_links.is_empty() {
            struct_ser.serialize_field("spanLinks", &self.span_links)?;
        }
        if !self.span_events.is_empty() {
            struct_ser.serialize_field("spanEvents", &self.span_events)?;
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
            "service",
            "name",
            "resource",
            "traceID",
            "spanID",
            "parentID",
            "start",
            "duration",
            "error",
            "meta",
            "metrics",
            "type",
            "meta_struct",
            "metaStruct",
            "spanLinks",
            "spanEvents",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Service,
            Name,
            Resource,
            TraceId,
            SpanId,
            ParentId,
            Start,
            Duration,
            Error,
            Meta,
            Metrics,
            Type,
            MetaStruct,
            SpanLinks,
            SpanEvents,
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
                            "service" => Ok(GeneratedField::Service),
                            "name" => Ok(GeneratedField::Name),
                            "resource" => Ok(GeneratedField::Resource),
                            "traceID" => Ok(GeneratedField::TraceId),
                            "spanID" => Ok(GeneratedField::SpanId),
                            "parentID" => Ok(GeneratedField::ParentId),
                            "start" => Ok(GeneratedField::Start),
                            "duration" => Ok(GeneratedField::Duration),
                            "error" => Ok(GeneratedField::Error),
                            "meta" => Ok(GeneratedField::Meta),
                            "metrics" => Ok(GeneratedField::Metrics),
                            "type" => Ok(GeneratedField::Type),
                            "metaStruct" | "meta_struct" => Ok(GeneratedField::MetaStruct),
                            "spanLinks" => Ok(GeneratedField::SpanLinks),
                            "spanEvents" => Ok(GeneratedField::SpanEvents),
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
                formatter.write_str("struct datadog.trace.Span")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<Span, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut service__ = None;
                let mut name__ = None;
                let mut resource__ = None;
                let mut trace_id__ = None;
                let mut span_id__ = None;
                let mut parent_id__ = None;
                let mut start__ = None;
                let mut duration__ = None;
                let mut error__ = None;
                let mut meta__ = None;
                let mut metrics__ = None;
                let mut r#type__ = None;
                let mut meta_struct__ = None;
                let mut span_links__ = None;
                let mut span_events__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Service => {
                            if service__.is_some() {
                                return Err(serde::de::Error::duplicate_field("service"));
                            }
                            service__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Name => {
                            if name__.is_some() {
                                return Err(serde::de::Error::duplicate_field("name"));
                            }
                            name__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Resource => {
                            if resource__.is_some() {
                                return Err(serde::de::Error::duplicate_field("resource"));
                            }
                            resource__ = Some(map_.next_value()?);
                        }
                        GeneratedField::TraceId => {
                            if trace_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("traceID"));
                            }
                            trace_id__ = 
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
                            error__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Meta => {
                            if meta__.is_some() {
                                return Err(serde::de::Error::duplicate_field("meta"));
                            }
                            meta__ = Some(
                                map_.next_value::<std::collections::HashMap<_, _>>()?
                            );
                        }
                        GeneratedField::Metrics => {
                            if metrics__.is_some() {
                                return Err(serde::de::Error::duplicate_field("metrics"));
                            }
                            metrics__ = Some(
                                map_.next_value::<std::collections::HashMap<_, ::pbjson::private::NumberDeserialize<f64>>>()?
                                    .into_iter().map(|(k,v)| (k, v.0)).collect()
                            );
                        }
                        GeneratedField::Type => {
                            if r#type__.is_some() {
                                return Err(serde::de::Error::duplicate_field("type"));
                            }
                            r#type__ = Some(map_.next_value()?);
                        }
                        GeneratedField::MetaStruct => {
                            if meta_struct__.is_some() {
                                return Err(serde::de::Error::duplicate_field("metaStruct"));
                            }
                            meta_struct__ = Some(
                                map_.next_value::<std::collections::HashMap<_, ::pbjson::private::BytesDeserialize<_>>>()?
                                    .into_iter().map(|(k,v)| (k, v.0)).collect()
                            );
                        }
                        GeneratedField::SpanLinks => {
                            if span_links__.is_some() {
                                return Err(serde::de::Error::duplicate_field("spanLinks"));
                            }
                            span_links__ = Some(map_.next_value()?);
                        }
                        GeneratedField::SpanEvents => {
                            if span_events__.is_some() {
                                return Err(serde::de::Error::duplicate_field("spanEvents"));
                            }
                            span_events__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(Span {
                    service: service__.unwrap_or_default(),
                    name: name__.unwrap_or_default(),
                    resource: resource__.unwrap_or_default(),
                    trace_id: trace_id__.unwrap_or_default(),
                    span_id: span_id__.unwrap_or_default(),
                    parent_id: parent_id__.unwrap_or_default(),
                    start: start__.unwrap_or_default(),
                    duration: duration__.unwrap_or_default(),
                    error: error__.unwrap_or_default(),
                    meta: meta__.unwrap_or_default(),
                    metrics: metrics__.unwrap_or_default(),
                    r#type: r#type__.unwrap_or_default(),
                    meta_struct: meta_struct__.unwrap_or_default(),
                    span_links: span_links__.unwrap_or_default(),
                    span_events: span_events__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.trace.Span", FIELDS, GeneratedVisitor)
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
        if self.time_unix_nano != 0 {
            len += 1;
        }
        if !self.name.is_empty() {
            len += 1;
        }
        if !self.attributes.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.trace.SpanEvent", len)?;
        if self.time_unix_nano != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("timeUnixNano", ToString::to_string(&self.time_unix_nano).as_str())?;
        }
        if !self.name.is_empty() {
            struct_ser.serialize_field("name", &self.name)?;
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
            "time_unix_nano",
            "timeUnixNano",
            "name",
            "attributes",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            TimeUnixNano,
            Name,
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
                            "timeUnixNano" | "time_unix_nano" => Ok(GeneratedField::TimeUnixNano),
                            "name" => Ok(GeneratedField::Name),
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
                formatter.write_str("struct datadog.trace.SpanEvent")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<SpanEvent, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut time_unix_nano__ = None;
                let mut name__ = None;
                let mut attributes__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::TimeUnixNano => {
                            if time_unix_nano__.is_some() {
                                return Err(serde::de::Error::duplicate_field("timeUnixNano"));
                            }
                            time_unix_nano__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Name => {
                            if name__.is_some() {
                                return Err(serde::de::Error::duplicate_field("name"));
                            }
                            name__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Attributes => {
                            if attributes__.is_some() {
                                return Err(serde::de::Error::duplicate_field("attributes"));
                            }
                            attributes__ = Some(
                                map_.next_value::<std::collections::HashMap<_, _>>()?
                            );
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(SpanEvent {
                    time_unix_nano: time_unix_nano__.unwrap_or_default(),
                    name: name__.unwrap_or_default(),
                    attributes: attributes__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.trace.SpanEvent", FIELDS, GeneratedVisitor)
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
        if self.trace_id != 0 {
            len += 1;
        }
        if self.trace_id_high != 0 {
            len += 1;
        }
        if self.span_id != 0 {
            len += 1;
        }
        if !self.attributes.is_empty() {
            len += 1;
        }
        if !self.tracestate.is_empty() {
            len += 1;
        }
        if self.flags != 0 {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.trace.SpanLink", len)?;
        if self.trace_id != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("traceID", ToString::to_string(&self.trace_id).as_str())?;
        }
        if self.trace_id_high != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("traceIDHigh", ToString::to_string(&self.trace_id_high).as_str())?;
        }
        if self.span_id != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("spanID", ToString::to_string(&self.span_id).as_str())?;
        }
        if !self.attributes.is_empty() {
            struct_ser.serialize_field("attributes", &self.attributes)?;
        }
        if !self.tracestate.is_empty() {
            struct_ser.serialize_field("tracestate", &self.tracestate)?;
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
            "traceID_high",
            "traceIDHigh",
            "spanID",
            "attributes",
            "tracestate",
            "flags",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            TraceId,
            TraceIdHigh,
            SpanId,
            Attributes,
            Tracestate,
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
                            "traceIDHigh" | "traceID_high" => Ok(GeneratedField::TraceIdHigh),
                            "spanID" => Ok(GeneratedField::SpanId),
                            "attributes" => Ok(GeneratedField::Attributes),
                            "tracestate" => Ok(GeneratedField::Tracestate),
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
                formatter.write_str("struct datadog.trace.SpanLink")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<SpanLink, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut trace_id__ = None;
                let mut trace_id_high__ = None;
                let mut span_id__ = None;
                let mut attributes__ = None;
                let mut tracestate__ = None;
                let mut flags__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::TraceId => {
                            if trace_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("traceID"));
                            }
                            trace_id__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::TraceIdHigh => {
                            if trace_id_high__.is_some() {
                                return Err(serde::de::Error::duplicate_field("traceIDHigh"));
                            }
                            trace_id_high__ = 
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
                        GeneratedField::Attributes => {
                            if attributes__.is_some() {
                                return Err(serde::de::Error::duplicate_field("attributes"));
                            }
                            attributes__ = Some(
                                map_.next_value::<std::collections::HashMap<_, _>>()?
                            );
                        }
                        GeneratedField::Tracestate => {
                            if tracestate__.is_some() {
                                return Err(serde::de::Error::duplicate_field("tracestate"));
                            }
                            tracestate__ = Some(map_.next_value()?);
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
                    trace_id_high: trace_id_high__.unwrap_or_default(),
                    span_id: span_id__.unwrap_or_default(),
                    attributes: attributes__.unwrap_or_default(),
                    tracestate: tracestate__.unwrap_or_default(),
                    flags: flags__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.trace.SpanLink", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for StatsPayload {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.agent_hostname.is_empty() {
            len += 1;
        }
        if !self.agent_env.is_empty() {
            len += 1;
        }
        if !self.stats.is_empty() {
            len += 1;
        }
        if !self.agent_version.is_empty() {
            len += 1;
        }
        if self.client_computed {
            len += 1;
        }
        if self.split_payload {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.trace.StatsPayload", len)?;
        if !self.agent_hostname.is_empty() {
            struct_ser.serialize_field("agentHostname", &self.agent_hostname)?;
        }
        if !self.agent_env.is_empty() {
            struct_ser.serialize_field("agentEnv", &self.agent_env)?;
        }
        if !self.stats.is_empty() {
            struct_ser.serialize_field("stats", &self.stats)?;
        }
        if !self.agent_version.is_empty() {
            struct_ser.serialize_field("agentVersion", &self.agent_version)?;
        }
        if self.client_computed {
            struct_ser.serialize_field("clientComputed", &self.client_computed)?;
        }
        if self.split_payload {
            struct_ser.serialize_field("splitPayload", &self.split_payload)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for StatsPayload {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "agentHostname",
            "agentEnv",
            "stats",
            "agentVersion",
            "clientComputed",
            "splitPayload",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            AgentHostname,
            AgentEnv,
            Stats,
            AgentVersion,
            ClientComputed,
            SplitPayload,
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
                            "agentHostname" => Ok(GeneratedField::AgentHostname),
                            "agentEnv" => Ok(GeneratedField::AgentEnv),
                            "stats" => Ok(GeneratedField::Stats),
                            "agentVersion" => Ok(GeneratedField::AgentVersion),
                            "clientComputed" => Ok(GeneratedField::ClientComputed),
                            "splitPayload" => Ok(GeneratedField::SplitPayload),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = StatsPayload;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.trace.StatsPayload")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<StatsPayload, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut agent_hostname__ = None;
                let mut agent_env__ = None;
                let mut stats__ = None;
                let mut agent_version__ = None;
                let mut client_computed__ = None;
                let mut split_payload__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::AgentHostname => {
                            if agent_hostname__.is_some() {
                                return Err(serde::de::Error::duplicate_field("agentHostname"));
                            }
                            agent_hostname__ = Some(map_.next_value()?);
                        }
                        GeneratedField::AgentEnv => {
                            if agent_env__.is_some() {
                                return Err(serde::de::Error::duplicate_field("agentEnv"));
                            }
                            agent_env__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Stats => {
                            if stats__.is_some() {
                                return Err(serde::de::Error::duplicate_field("stats"));
                            }
                            stats__ = Some(map_.next_value()?);
                        }
                        GeneratedField::AgentVersion => {
                            if agent_version__.is_some() {
                                return Err(serde::de::Error::duplicate_field("agentVersion"));
                            }
                            agent_version__ = Some(map_.next_value()?);
                        }
                        GeneratedField::ClientComputed => {
                            if client_computed__.is_some() {
                                return Err(serde::de::Error::duplicate_field("clientComputed"));
                            }
                            client_computed__ = Some(map_.next_value()?);
                        }
                        GeneratedField::SplitPayload => {
                            if split_payload__.is_some() {
                                return Err(serde::de::Error::duplicate_field("splitPayload"));
                            }
                            split_payload__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(StatsPayload {
                    agent_hostname: agent_hostname__.unwrap_or_default(),
                    agent_env: agent_env__.unwrap_or_default(),
                    stats: stats__.unwrap_or_default(),
                    agent_version: agent_version__.unwrap_or_default(),
                    client_computed: client_computed__.unwrap_or_default(),
                    split_payload: split_payload__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.trace.StatsPayload", FIELDS, GeneratedVisitor)
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
        if !self.origin.is_empty() {
            len += 1;
        }
        if !self.spans.is_empty() {
            len += 1;
        }
        if !self.tags.is_empty() {
            len += 1;
        }
        if self.dropped_trace {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.trace.TraceChunk", len)?;
        if self.priority != 0 {
            struct_ser.serialize_field("priority", &self.priority)?;
        }
        if !self.origin.is_empty() {
            struct_ser.serialize_field("origin", &self.origin)?;
        }
        if !self.spans.is_empty() {
            struct_ser.serialize_field("spans", &self.spans)?;
        }
        if !self.tags.is_empty() {
            struct_ser.serialize_field("tags", &self.tags)?;
        }
        if self.dropped_trace {
            struct_ser.serialize_field("droppedTrace", &self.dropped_trace)?;
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
            "origin",
            "spans",
            "tags",
            "droppedTrace",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Priority,
            Origin,
            Spans,
            Tags,
            DroppedTrace,
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
                            "origin" => Ok(GeneratedField::Origin),
                            "spans" => Ok(GeneratedField::Spans),
                            "tags" => Ok(GeneratedField::Tags),
                            "droppedTrace" => Ok(GeneratedField::DroppedTrace),
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
                formatter.write_str("struct datadog.trace.TraceChunk")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<TraceChunk, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut priority__ = None;
                let mut origin__ = None;
                let mut spans__ = None;
                let mut tags__ = None;
                let mut dropped_trace__ = None;
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
                        GeneratedField::Origin => {
                            if origin__.is_some() {
                                return Err(serde::de::Error::duplicate_field("origin"));
                            }
                            origin__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Spans => {
                            if spans__.is_some() {
                                return Err(serde::de::Error::duplicate_field("spans"));
                            }
                            spans__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Tags => {
                            if tags__.is_some() {
                                return Err(serde::de::Error::duplicate_field("tags"));
                            }
                            tags__ = Some(
                                map_.next_value::<std::collections::HashMap<_, _>>()?
                            );
                        }
                        GeneratedField::DroppedTrace => {
                            if dropped_trace__.is_some() {
                                return Err(serde::de::Error::duplicate_field("droppedTrace"));
                            }
                            dropped_trace__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(TraceChunk {
                    priority: priority__.unwrap_or_default(),
                    origin: origin__.unwrap_or_default(),
                    spans: spans__.unwrap_or_default(),
                    tags: tags__.unwrap_or_default(),
                    dropped_trace: dropped_trace__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.trace.TraceChunk", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for TraceRootFlag {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let variant = match self {
            Self::DeprecatedNotSet => "DEPRECATED_NOT_SET",
            Self::DeprecatedTrue => "DEPRECATED_TRUE",
            Self::DeprecatedFalse => "DEPRECATED_FALSE",
        };
        serializer.serialize_str(variant)
    }
}
impl<'de> serde::Deserialize<'de> for TraceRootFlag {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "DEPRECATED_NOT_SET",
            "DEPRECATED_TRUE",
            "DEPRECATED_FALSE",
        ];

        struct GeneratedVisitor;

        impl serde::de::Visitor<'_> for GeneratedVisitor {
            type Value = TraceRootFlag;

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
                    "DEPRECATED_NOT_SET" => Ok(TraceRootFlag::DeprecatedNotSet),
                    "DEPRECATED_TRUE" => Ok(TraceRootFlag::DeprecatedTrue),
                    "DEPRECATED_FALSE" => Ok(TraceRootFlag::DeprecatedFalse),
                    _ => Err(serde::de::Error::unknown_variant(value, FIELDS)),
                }
            }
        }
        deserializer.deserialize_any(GeneratedVisitor)
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
        if !self.container_id.is_empty() {
            len += 1;
        }
        if !self.language_name.is_empty() {
            len += 1;
        }
        if !self.language_version.is_empty() {
            len += 1;
        }
        if !self.tracer_version.is_empty() {
            len += 1;
        }
        if !self.runtime_id.is_empty() {
            len += 1;
        }
        if !self.chunks.is_empty() {
            len += 1;
        }
        if !self.tags.is_empty() {
            len += 1;
        }
        if !self.env.is_empty() {
            len += 1;
        }
        if !self.hostname.is_empty() {
            len += 1;
        }
        if !self.app_version.is_empty() {
            len += 1;
        }
        if self.container_debug.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.trace.TracerPayload", len)?;
        if !self.container_id.is_empty() {
            struct_ser.serialize_field("containerID", &self.container_id)?;
        }
        if !self.language_name.is_empty() {
            struct_ser.serialize_field("languageName", &self.language_name)?;
        }
        if !self.language_version.is_empty() {
            struct_ser.serialize_field("languageVersion", &self.language_version)?;
        }
        if !self.tracer_version.is_empty() {
            struct_ser.serialize_field("tracerVersion", &self.tracer_version)?;
        }
        if !self.runtime_id.is_empty() {
            struct_ser.serialize_field("runtimeID", &self.runtime_id)?;
        }
        if !self.chunks.is_empty() {
            struct_ser.serialize_field("chunks", &self.chunks)?;
        }
        if !self.tags.is_empty() {
            struct_ser.serialize_field("tags", &self.tags)?;
        }
        if !self.env.is_empty() {
            struct_ser.serialize_field("env", &self.env)?;
        }
        if !self.hostname.is_empty() {
            struct_ser.serialize_field("hostname", &self.hostname)?;
        }
        if !self.app_version.is_empty() {
            struct_ser.serialize_field("appVersion", &self.app_version)?;
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
            "containerID",
            "languageName",
            "languageVersion",
            "tracerVersion",
            "runtimeID",
            "chunks",
            "tags",
            "env",
            "hostname",
            "appVersion",
            "containerDebug",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            ContainerId,
            LanguageName,
            LanguageVersion,
            TracerVersion,
            RuntimeId,
            Chunks,
            Tags,
            Env,
            Hostname,
            AppVersion,
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
                            "containerID" => Ok(GeneratedField::ContainerId),
                            "languageName" => Ok(GeneratedField::LanguageName),
                            "languageVersion" => Ok(GeneratedField::LanguageVersion),
                            "tracerVersion" => Ok(GeneratedField::TracerVersion),
                            "runtimeID" => Ok(GeneratedField::RuntimeId),
                            "chunks" => Ok(GeneratedField::Chunks),
                            "tags" => Ok(GeneratedField::Tags),
                            "env" => Ok(GeneratedField::Env),
                            "hostname" => Ok(GeneratedField::Hostname),
                            "appVersion" => Ok(GeneratedField::AppVersion),
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
                formatter.write_str("struct datadog.trace.TracerPayload")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<TracerPayload, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut container_id__ = None;
                let mut language_name__ = None;
                let mut language_version__ = None;
                let mut tracer_version__ = None;
                let mut runtime_id__ = None;
                let mut chunks__ = None;
                let mut tags__ = None;
                let mut env__ = None;
                let mut hostname__ = None;
                let mut app_version__ = None;
                let mut container_debug__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::ContainerId => {
                            if container_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("containerID"));
                            }
                            container_id__ = Some(map_.next_value()?);
                        }
                        GeneratedField::LanguageName => {
                            if language_name__.is_some() {
                                return Err(serde::de::Error::duplicate_field("languageName"));
                            }
                            language_name__ = Some(map_.next_value()?);
                        }
                        GeneratedField::LanguageVersion => {
                            if language_version__.is_some() {
                                return Err(serde::de::Error::duplicate_field("languageVersion"));
                            }
                            language_version__ = Some(map_.next_value()?);
                        }
                        GeneratedField::TracerVersion => {
                            if tracer_version__.is_some() {
                                return Err(serde::de::Error::duplicate_field("tracerVersion"));
                            }
                            tracer_version__ = Some(map_.next_value()?);
                        }
                        GeneratedField::RuntimeId => {
                            if runtime_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("runtimeID"));
                            }
                            runtime_id__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Chunks => {
                            if chunks__.is_some() {
                                return Err(serde::de::Error::duplicate_field("chunks"));
                            }
                            chunks__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Tags => {
                            if tags__.is_some() {
                                return Err(serde::de::Error::duplicate_field("tags"));
                            }
                            tags__ = Some(
                                map_.next_value::<std::collections::HashMap<_, _>>()?
                            );
                        }
                        GeneratedField::Env => {
                            if env__.is_some() {
                                return Err(serde::de::Error::duplicate_field("env"));
                            }
                            env__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Hostname => {
                            if hostname__.is_some() {
                                return Err(serde::de::Error::duplicate_field("hostname"));
                            }
                            hostname__ = Some(map_.next_value()?);
                        }
                        GeneratedField::AppVersion => {
                            if app_version__.is_some() {
                                return Err(serde::de::Error::duplicate_field("appVersion"));
                            }
                            app_version__ = Some(map_.next_value()?);
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
                    container_id: container_id__.unwrap_or_default(),
                    language_name: language_name__.unwrap_or_default(),
                    language_version: language_version__.unwrap_or_default(),
                    tracer_version: tracer_version__.unwrap_or_default(),
                    runtime_id: runtime_id__.unwrap_or_default(),
                    chunks: chunks__.unwrap_or_default(),
                    tags: tags__.unwrap_or_default(),
                    env: env__.unwrap_or_default(),
                    hostname: hostname__.unwrap_or_default(),
                    app_version: app_version__.unwrap_or_default(),
                    container_debug: container_debug__,
                })
            }
        }
        deserializer.deserialize_struct("datadog.trace.TracerPayload", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for Trilean {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let variant = match self {
            Self::NotSet => "NOT_SET",
            Self::True => "TRUE",
            Self::False => "FALSE",
        };
        serializer.serialize_str(variant)
    }
}
impl<'de> serde::Deserialize<'de> for Trilean {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "NOT_SET",
            "TRUE",
            "FALSE",
        ];

        struct GeneratedVisitor;

        impl serde::de::Visitor<'_> for GeneratedVisitor {
            type Value = Trilean;

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
                    "NOT_SET" => Ok(Trilean::NotSet),
                    "TRUE" => Ok(Trilean::True),
                    "FALSE" => Ok(Trilean::False),
                    _ => Err(serde::de::Error::unknown_variant(value, FIELDS)),
                }
            }
        }
        deserializer.deserialize_any(GeneratedVisitor)
    }
}
