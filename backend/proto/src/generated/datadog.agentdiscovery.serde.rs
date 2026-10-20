impl serde::Serialize for AgentDiscoveryConfigFile {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.path.is_empty() {
            len += 1;
        }
        if !self.content.is_empty() {
            len += 1;
        }
        if self.truncated {
            len += 1;
        }
        if self.payload_format != 0 {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.agentdiscovery.AgentDiscoveryConfigFile", len)?;
        if !self.path.is_empty() {
            struct_ser.serialize_field("path", &self.path)?;
        }
        if !self.content.is_empty() {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("content", pbjson::private::base64::encode(&self.content).as_str())?;
        }
        if self.truncated {
            struct_ser.serialize_field("truncated", &self.truncated)?;
        }
        if self.payload_format != 0 {
            let v = AgentDiscoveryConfigFilePayloadFormat::try_from(self.payload_format)
                .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", self.payload_format)))?;
            struct_ser.serialize_field("payloadFormat", &v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for AgentDiscoveryConfigFile {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "path",
            "content",
            "truncated",
            "payload_format",
            "payloadFormat",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Path,
            Content,
            Truncated,
            PayloadFormat,
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
                            "path" => Ok(GeneratedField::Path),
                            "content" => Ok(GeneratedField::Content),
                            "truncated" => Ok(GeneratedField::Truncated),
                            "payloadFormat" | "payload_format" => Ok(GeneratedField::PayloadFormat),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = AgentDiscoveryConfigFile;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.agentdiscovery.AgentDiscoveryConfigFile")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<AgentDiscoveryConfigFile, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut path__ = None;
                let mut content__ = None;
                let mut truncated__ = None;
                let mut payload_format__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Path => {
                            if path__.is_some() {
                                return Err(serde::de::Error::duplicate_field("path"));
                            }
                            path__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Content => {
                            if content__.is_some() {
                                return Err(serde::de::Error::duplicate_field("content"));
                            }
                            content__ = 
                                Some(map_.next_value::<::pbjson::private::BytesDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Truncated => {
                            if truncated__.is_some() {
                                return Err(serde::de::Error::duplicate_field("truncated"));
                            }
                            truncated__ = Some(map_.next_value()?);
                        }
                        GeneratedField::PayloadFormat => {
                            if payload_format__.is_some() {
                                return Err(serde::de::Error::duplicate_field("payloadFormat"));
                            }
                            payload_format__ = Some(map_.next_value::<AgentDiscoveryConfigFilePayloadFormat>()? as i32);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(AgentDiscoveryConfigFile {
                    path: path__.unwrap_or_default(),
                    content: content__.unwrap_or_default(),
                    truncated: truncated__.unwrap_or_default(),
                    payload_format: payload_format__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.agentdiscovery.AgentDiscoveryConfigFile", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for AgentDiscoveryConfigFilePayloadFormat {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let variant = match self {
            Self::PayloadFormatUnknown => "PAYLOAD_FORMAT_UNKNOWN",
            Self::PayloadFormatJson => "PAYLOAD_FORMAT_JSON",
            Self::PayloadFormatYaml => "PAYLOAD_FORMAT_YAML",
            Self::PayloadFormatToml => "PAYLOAD_FORMAT_TOML",
            Self::PayloadFormatIni => "PAYLOAD_FORMAT_INI",
            Self::PayloadFormatXml => "PAYLOAD_FORMAT_XML",
            Self::PayloadFormatProperties => "PAYLOAD_FORMAT_PROPERTIES",
            Self::PayloadFormatHcl => "PAYLOAD_FORMAT_HCL",
            Self::PayloadFormatRedisConf => "PAYLOAD_FORMAT_REDIS_CONF",
        };
        serializer.serialize_str(variant)
    }
}
impl<'de> serde::Deserialize<'de> for AgentDiscoveryConfigFilePayloadFormat {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "PAYLOAD_FORMAT_UNKNOWN",
            "PAYLOAD_FORMAT_JSON",
            "PAYLOAD_FORMAT_YAML",
            "PAYLOAD_FORMAT_TOML",
            "PAYLOAD_FORMAT_INI",
            "PAYLOAD_FORMAT_XML",
            "PAYLOAD_FORMAT_PROPERTIES",
            "PAYLOAD_FORMAT_HCL",
            "PAYLOAD_FORMAT_REDIS_CONF",
        ];

        struct GeneratedVisitor;

        impl serde::de::Visitor<'_> for GeneratedVisitor {
            type Value = AgentDiscoveryConfigFilePayloadFormat;

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
                    "PAYLOAD_FORMAT_UNKNOWN" => Ok(AgentDiscoveryConfigFilePayloadFormat::PayloadFormatUnknown),
                    "PAYLOAD_FORMAT_JSON" => Ok(AgentDiscoveryConfigFilePayloadFormat::PayloadFormatJson),
                    "PAYLOAD_FORMAT_YAML" => Ok(AgentDiscoveryConfigFilePayloadFormat::PayloadFormatYaml),
                    "PAYLOAD_FORMAT_TOML" => Ok(AgentDiscoveryConfigFilePayloadFormat::PayloadFormatToml),
                    "PAYLOAD_FORMAT_INI" => Ok(AgentDiscoveryConfigFilePayloadFormat::PayloadFormatIni),
                    "PAYLOAD_FORMAT_XML" => Ok(AgentDiscoveryConfigFilePayloadFormat::PayloadFormatXml),
                    "PAYLOAD_FORMAT_PROPERTIES" => Ok(AgentDiscoveryConfigFilePayloadFormat::PayloadFormatProperties),
                    "PAYLOAD_FORMAT_HCL" => Ok(AgentDiscoveryConfigFilePayloadFormat::PayloadFormatHcl),
                    "PAYLOAD_FORMAT_REDIS_CONF" => Ok(AgentDiscoveryConfigFilePayloadFormat::PayloadFormatRedisConf),
                    _ => Err(serde::de::Error::unknown_variant(value, FIELDS)),
                }
            }
        }
        deserializer.deserialize_any(GeneratedVisitor)
    }
}
impl serde::Serialize for AgentDiscoveryEnvVar {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.name.is_empty() {
            len += 1;
        }
        if !self.value.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.agentdiscovery.AgentDiscoveryEnvVar", len)?;
        if !self.name.is_empty() {
            struct_ser.serialize_field("name", &self.name)?;
        }
        if !self.value.is_empty() {
            struct_ser.serialize_field("value", &self.value)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for AgentDiscoveryEnvVar {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "name",
            "value",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Name,
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
                            "name" => Ok(GeneratedField::Name),
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
            type Value = AgentDiscoveryEnvVar;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.agentdiscovery.AgentDiscoveryEnvVar")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<AgentDiscoveryEnvVar, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut name__ = None;
                let mut value__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Name => {
                            if name__.is_some() {
                                return Err(serde::de::Error::duplicate_field("name"));
                            }
                            name__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Value => {
                            if value__.is_some() {
                                return Err(serde::de::Error::duplicate_field("value"));
                            }
                            value__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(AgentDiscoveryEnvVar {
                    name: name__.unwrap_or_default(),
                    value: value__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.agentdiscovery.AgentDiscoveryEnvVar", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for AgentDiscoveryPayload {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.integration.is_empty() {
            len += 1;
        }
        if !self.runtime.is_empty() {
            len += 1;
        }
        if !self.config_files.is_empty() {
            len += 1;
        }
        if !self.env_vars.is_empty() {
            len += 1;
        }
        if !self.runtime_id.is_empty() {
            len += 1;
        }
        if self.ingestion_timestamp.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.agentdiscovery.AgentDiscoveryPayload", len)?;
        if !self.integration.is_empty() {
            struct_ser.serialize_field("integration", &self.integration)?;
        }
        if !self.runtime.is_empty() {
            struct_ser.serialize_field("runtime", &self.runtime)?;
        }
        if !self.config_files.is_empty() {
            struct_ser.serialize_field("configFiles", &self.config_files)?;
        }
        if !self.env_vars.is_empty() {
            struct_ser.serialize_field("envVars", &self.env_vars)?;
        }
        if !self.runtime_id.is_empty() {
            struct_ser.serialize_field("runtimeId", &self.runtime_id)?;
        }
        if let Some(v) = self.ingestion_timestamp.as_ref() {
            struct_ser.serialize_field("ingestionTimestamp", v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for AgentDiscoveryPayload {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "integration",
            "runtime",
            "config_files",
            "configFiles",
            "env_vars",
            "envVars",
            "runtime_id",
            "runtimeId",
            "ingestion_timestamp",
            "ingestionTimestamp",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Integration,
            Runtime,
            ConfigFiles,
            EnvVars,
            RuntimeId,
            IngestionTimestamp,
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
                            "integration" => Ok(GeneratedField::Integration),
                            "runtime" => Ok(GeneratedField::Runtime),
                            "configFiles" | "config_files" => Ok(GeneratedField::ConfigFiles),
                            "envVars" | "env_vars" => Ok(GeneratedField::EnvVars),
                            "runtimeId" | "runtime_id" => Ok(GeneratedField::RuntimeId),
                            "ingestionTimestamp" | "ingestion_timestamp" => Ok(GeneratedField::IngestionTimestamp),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = AgentDiscoveryPayload;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.agentdiscovery.AgentDiscoveryPayload")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<AgentDiscoveryPayload, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut integration__ = None;
                let mut runtime__ = None;
                let mut config_files__ = None;
                let mut env_vars__ = None;
                let mut runtime_id__ = None;
                let mut ingestion_timestamp__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Integration => {
                            if integration__.is_some() {
                                return Err(serde::de::Error::duplicate_field("integration"));
                            }
                            integration__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Runtime => {
                            if runtime__.is_some() {
                                return Err(serde::de::Error::duplicate_field("runtime"));
                            }
                            runtime__ = Some(map_.next_value()?);
                        }
                        GeneratedField::ConfigFiles => {
                            if config_files__.is_some() {
                                return Err(serde::de::Error::duplicate_field("configFiles"));
                            }
                            config_files__ = Some(map_.next_value()?);
                        }
                        GeneratedField::EnvVars => {
                            if env_vars__.is_some() {
                                return Err(serde::de::Error::duplicate_field("envVars"));
                            }
                            env_vars__ = Some(map_.next_value()?);
                        }
                        GeneratedField::RuntimeId => {
                            if runtime_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("runtimeId"));
                            }
                            runtime_id__ = Some(map_.next_value()?);
                        }
                        GeneratedField::IngestionTimestamp => {
                            if ingestion_timestamp__.is_some() {
                                return Err(serde::de::Error::duplicate_field("ingestionTimestamp"));
                            }
                            ingestion_timestamp__ = map_.next_value()?;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(AgentDiscoveryPayload {
                    integration: integration__.unwrap_or_default(),
                    runtime: runtime__.unwrap_or_default(),
                    config_files: config_files__.unwrap_or_default(),
                    env_vars: env_vars__.unwrap_or_default(),
                    runtime_id: runtime_id__.unwrap_or_default(),
                    ingestion_timestamp: ingestion_timestamp__,
                })
            }
        }
        deserializer.deserialize_struct("datadog.agentdiscovery.AgentDiscoveryPayload", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for AgentDiscoveryPayloadBatch {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.payloads.is_empty() {
            len += 1;
        }
        if !self.host_id.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.agentdiscovery.AgentDiscoveryPayloadBatch", len)?;
        if !self.payloads.is_empty() {
            struct_ser.serialize_field("payloads", &self.payloads)?;
        }
        if !self.host_id.is_empty() {
            struct_ser.serialize_field("hostId", &self.host_id)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for AgentDiscoveryPayloadBatch {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "payloads",
            "host_id",
            "hostId",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Payloads,
            HostId,
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
                            "payloads" => Ok(GeneratedField::Payloads),
                            "hostId" | "host_id" => Ok(GeneratedField::HostId),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = AgentDiscoveryPayloadBatch;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.agentdiscovery.AgentDiscoveryPayloadBatch")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<AgentDiscoveryPayloadBatch, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut payloads__ = None;
                let mut host_id__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Payloads => {
                            if payloads__.is_some() {
                                return Err(serde::de::Error::duplicate_field("payloads"));
                            }
                            payloads__ = Some(map_.next_value()?);
                        }
                        GeneratedField::HostId => {
                            if host_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("hostId"));
                            }
                            host_id__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(AgentDiscoveryPayloadBatch {
                    payloads: payloads__.unwrap_or_default(),
                    host_id: host_id__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.agentdiscovery.AgentDiscoveryPayloadBatch", FIELDS, GeneratedVisitor)
    }
}
