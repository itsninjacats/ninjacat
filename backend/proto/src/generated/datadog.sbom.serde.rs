impl serde::Serialize for SbomEntity {
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
        if !self.id.is_empty() {
            len += 1;
        }
        if self.generated_at.is_some() {
            len += 1;
        }
        if !self.repo_tags.is_empty() {
            len += 1;
        }
        if self.in_use {
            len += 1;
        }
        if self.generation_duration.is_some() {
            len += 1;
        }
        if !self.dd_tags.is_empty() {
            len += 1;
        }
        if self.heartbeat {
            len += 1;
        }
        if !self.hash.is_empty() {
            len += 1;
        }
        if self.status != 0 {
            len += 1;
        }
        if !self.repo_digests.is_empty() {
            len += 1;
        }
        if !self.kernel_version.is_empty() {
            len += 1;
        }
        if !self.cpu_architecture.is_empty() {
            len += 1;
        }
        if self.sbom.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.sbom.SBOMEntity", len)?;
        if self.r#type != 0 {
            let v = SbomSourceType::try_from(self.r#type)
                .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", self.r#type)))?;
            struct_ser.serialize_field("type", &v)?;
        }
        if !self.id.is_empty() {
            struct_ser.serialize_field("id", &self.id)?;
        }
        if let Some(v) = self.generated_at.as_ref() {
            struct_ser.serialize_field("generatedAt", v)?;
        }
        if !self.repo_tags.is_empty() {
            struct_ser.serialize_field("repoTags", &self.repo_tags)?;
        }
        if self.in_use {
            struct_ser.serialize_field("inUse", &self.in_use)?;
        }
        if let Some(v) = self.generation_duration.as_ref() {
            struct_ser.serialize_field("generationDuration", v)?;
        }
        if !self.dd_tags.is_empty() {
            struct_ser.serialize_field("ddTags", &self.dd_tags)?;
        }
        if self.heartbeat {
            struct_ser.serialize_field("heartbeat", &self.heartbeat)?;
        }
        if !self.hash.is_empty() {
            struct_ser.serialize_field("hash", &self.hash)?;
        }
        if self.status != 0 {
            let v = SbomStatus::try_from(self.status)
                .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", self.status)))?;
            struct_ser.serialize_field("status", &v)?;
        }
        if !self.repo_digests.is_empty() {
            struct_ser.serialize_field("repoDigests", &self.repo_digests)?;
        }
        if !self.kernel_version.is_empty() {
            struct_ser.serialize_field("kernelVersion", &self.kernel_version)?;
        }
        if !self.cpu_architecture.is_empty() {
            struct_ser.serialize_field("cpuArchitecture", &self.cpu_architecture)?;
        }
        if let Some(v) = self.sbom.as_ref() {
            match v {
                sbom_entity::Sbom::Cyclonedx(v) => {
                    struct_ser.serialize_field("cyclonedx", v)?;
                }
                sbom_entity::Sbom::Error(v) => {
                    struct_ser.serialize_field("error", v)?;
                }
            }
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for SbomEntity {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "type",
            "id",
            "generatedAt",
            "repo_tags",
            "repoTags",
            "inUse",
            "generationDuration",
            "dd_tags",
            "ddTags",
            "heartbeat",
            "hash",
            "status",
            "repo_digests",
            "repoDigests",
            "kernel_version",
            "kernelVersion",
            "cpu_architecture",
            "cpuArchitecture",
            "cyclonedx",
            "error",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Type,
            Id,
            GeneratedAt,
            RepoTags,
            InUse,
            GenerationDuration,
            DdTags,
            Heartbeat,
            Hash,
            Status,
            RepoDigests,
            KernelVersion,
            CpuArchitecture,
            Cyclonedx,
            Error,
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
                            "id" => Ok(GeneratedField::Id),
                            "generatedAt" => Ok(GeneratedField::GeneratedAt),
                            "repoTags" | "repo_tags" => Ok(GeneratedField::RepoTags),
                            "inUse" => Ok(GeneratedField::InUse),
                            "generationDuration" => Ok(GeneratedField::GenerationDuration),
                            "ddTags" | "dd_tags" => Ok(GeneratedField::DdTags),
                            "heartbeat" => Ok(GeneratedField::Heartbeat),
                            "hash" => Ok(GeneratedField::Hash),
                            "status" => Ok(GeneratedField::Status),
                            "repoDigests" | "repo_digests" => Ok(GeneratedField::RepoDigests),
                            "kernelVersion" | "kernel_version" => Ok(GeneratedField::KernelVersion),
                            "cpuArchitecture" | "cpu_architecture" => Ok(GeneratedField::CpuArchitecture),
                            "cyclonedx" => Ok(GeneratedField::Cyclonedx),
                            "error" => Ok(GeneratedField::Error),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = SbomEntity;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.sbom.SBOMEntity")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<SbomEntity, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut r#type__ = None;
                let mut id__ = None;
                let mut generated_at__ = None;
                let mut repo_tags__ = None;
                let mut in_use__ = None;
                let mut generation_duration__ = None;
                let mut dd_tags__ = None;
                let mut heartbeat__ = None;
                let mut hash__ = None;
                let mut status__ = None;
                let mut repo_digests__ = None;
                let mut kernel_version__ = None;
                let mut cpu_architecture__ = None;
                let mut sbom__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Type => {
                            if r#type__.is_some() {
                                return Err(serde::de::Error::duplicate_field("type"));
                            }
                            r#type__ = Some(map_.next_value::<SbomSourceType>()? as i32);
                        }
                        GeneratedField::Id => {
                            if id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("id"));
                            }
                            id__ = Some(map_.next_value()?);
                        }
                        GeneratedField::GeneratedAt => {
                            if generated_at__.is_some() {
                                return Err(serde::de::Error::duplicate_field("generatedAt"));
                            }
                            generated_at__ = map_.next_value()?;
                        }
                        GeneratedField::RepoTags => {
                            if repo_tags__.is_some() {
                                return Err(serde::de::Error::duplicate_field("repoTags"));
                            }
                            repo_tags__ = Some(map_.next_value()?);
                        }
                        GeneratedField::InUse => {
                            if in_use__.is_some() {
                                return Err(serde::de::Error::duplicate_field("inUse"));
                            }
                            in_use__ = Some(map_.next_value()?);
                        }
                        GeneratedField::GenerationDuration => {
                            if generation_duration__.is_some() {
                                return Err(serde::de::Error::duplicate_field("generationDuration"));
                            }
                            generation_duration__ = map_.next_value()?;
                        }
                        GeneratedField::DdTags => {
                            if dd_tags__.is_some() {
                                return Err(serde::de::Error::duplicate_field("ddTags"));
                            }
                            dd_tags__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Heartbeat => {
                            if heartbeat__.is_some() {
                                return Err(serde::de::Error::duplicate_field("heartbeat"));
                            }
                            heartbeat__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Hash => {
                            if hash__.is_some() {
                                return Err(serde::de::Error::duplicate_field("hash"));
                            }
                            hash__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Status => {
                            if status__.is_some() {
                                return Err(serde::de::Error::duplicate_field("status"));
                            }
                            status__ = Some(map_.next_value::<SbomStatus>()? as i32);
                        }
                        GeneratedField::RepoDigests => {
                            if repo_digests__.is_some() {
                                return Err(serde::de::Error::duplicate_field("repoDigests"));
                            }
                            repo_digests__ = Some(map_.next_value()?);
                        }
                        GeneratedField::KernelVersion => {
                            if kernel_version__.is_some() {
                                return Err(serde::de::Error::duplicate_field("kernelVersion"));
                            }
                            kernel_version__ = Some(map_.next_value()?);
                        }
                        GeneratedField::CpuArchitecture => {
                            if cpu_architecture__.is_some() {
                                return Err(serde::de::Error::duplicate_field("cpuArchitecture"));
                            }
                            cpu_architecture__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Cyclonedx => {
                            if sbom__.is_some() {
                                return Err(serde::de::Error::duplicate_field("cyclonedx"));
                            }
                            sbom__ = map_.next_value::<::std::option::Option<_>>()?.map(sbom_entity::Sbom::Cyclonedx)
;
                        }
                        GeneratedField::Error => {
                            if sbom__.is_some() {
                                return Err(serde::de::Error::duplicate_field("error"));
                            }
                            sbom__ = map_.next_value::<::std::option::Option<_>>()?.map(sbom_entity::Sbom::Error);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(SbomEntity {
                    r#type: r#type__.unwrap_or_default(),
                    id: id__.unwrap_or_default(),
                    generated_at: generated_at__,
                    repo_tags: repo_tags__.unwrap_or_default(),
                    in_use: in_use__.unwrap_or_default(),
                    generation_duration: generation_duration__,
                    dd_tags: dd_tags__.unwrap_or_default(),
                    heartbeat: heartbeat__.unwrap_or_default(),
                    hash: hash__.unwrap_or_default(),
                    status: status__.unwrap_or_default(),
                    repo_digests: repo_digests__.unwrap_or_default(),
                    kernel_version: kernel_version__.unwrap_or_default(),
                    cpu_architecture: cpu_architecture__.unwrap_or_default(),
                    sbom: sbom__,
                })
            }
        }
        deserializer.deserialize_struct("datadog.sbom.SBOMEntity", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for SbomPayload {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.version != 0 {
            len += 1;
        }
        if !self.host.is_empty() {
            len += 1;
        }
        if self.source.is_some() {
            len += 1;
        }
        if !self.entities.is_empty() {
            len += 1;
        }
        if self.dd_env.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.sbom.SBOMPayload", len)?;
        if self.version != 0 {
            struct_ser.serialize_field("version", &self.version)?;
        }
        if !self.host.is_empty() {
            struct_ser.serialize_field("host", &self.host)?;
        }
        if let Some(v) = self.source.as_ref() {
            struct_ser.serialize_field("source", v)?;
        }
        if !self.entities.is_empty() {
            struct_ser.serialize_field("entities", &self.entities)?;
        }
        if let Some(v) = self.dd_env.as_ref() {
            struct_ser.serialize_field("ddEnv", v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for SbomPayload {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "version",
            "host",
            "source",
            "entities",
            "dd_env",
            "ddEnv",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Version,
            Host,
            Source,
            Entities,
            DdEnv,
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
                            "version" => Ok(GeneratedField::Version),
                            "host" => Ok(GeneratedField::Host),
                            "source" => Ok(GeneratedField::Source),
                            "entities" => Ok(GeneratedField::Entities),
                            "ddEnv" | "dd_env" => Ok(GeneratedField::DdEnv),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = SbomPayload;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.sbom.SBOMPayload")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<SbomPayload, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut version__ = None;
                let mut host__ = None;
                let mut source__ = None;
                let mut entities__ = None;
                let mut dd_env__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Version => {
                            if version__.is_some() {
                                return Err(serde::de::Error::duplicate_field("version"));
                            }
                            version__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Host => {
                            if host__.is_some() {
                                return Err(serde::de::Error::duplicate_field("host"));
                            }
                            host__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Source => {
                            if source__.is_some() {
                                return Err(serde::de::Error::duplicate_field("source"));
                            }
                            source__ = map_.next_value()?;
                        }
                        GeneratedField::Entities => {
                            if entities__.is_some() {
                                return Err(serde::de::Error::duplicate_field("entities"));
                            }
                            entities__ = Some(map_.next_value()?);
                        }
                        GeneratedField::DdEnv => {
                            if dd_env__.is_some() {
                                return Err(serde::de::Error::duplicate_field("ddEnv"));
                            }
                            dd_env__ = map_.next_value()?;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(SbomPayload {
                    version: version__.unwrap_or_default(),
                    host: host__.unwrap_or_default(),
                    source: source__,
                    entities: entities__.unwrap_or_default(),
                    dd_env: dd_env__,
                })
            }
        }
        deserializer.deserialize_struct("datadog.sbom.SBOMPayload", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for SbomSourceType {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let variant = match self {
            Self::Unspecified => "UNSPECIFIED",
            Self::ContainerImageLayers => "CONTAINER_IMAGE_LAYERS",
            Self::ContainerFileSystem => "CONTAINER_FILE_SYSTEM",
            Self::HostFileSystem => "HOST_FILE_SYSTEM",
            Self::CiPipeline => "CI_PIPELINE",
            Self::HostImage => "HOST_IMAGE",
            Self::ServerlessFunction => "SERVERLESS_FUNCTION",
        };
        serializer.serialize_str(variant)
    }
}
impl<'de> serde::Deserialize<'de> for SbomSourceType {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "UNSPECIFIED",
            "CONTAINER_IMAGE_LAYERS",
            "CONTAINER_FILE_SYSTEM",
            "HOST_FILE_SYSTEM",
            "CI_PIPELINE",
            "HOST_IMAGE",
            "SERVERLESS_FUNCTION",
        ];

        struct GeneratedVisitor;

        impl serde::de::Visitor<'_> for GeneratedVisitor {
            type Value = SbomSourceType;

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
                    "UNSPECIFIED" => Ok(SbomSourceType::Unspecified),
                    "CONTAINER_IMAGE_LAYERS" => Ok(SbomSourceType::ContainerImageLayers),
                    "CONTAINER_FILE_SYSTEM" => Ok(SbomSourceType::ContainerFileSystem),
                    "HOST_FILE_SYSTEM" => Ok(SbomSourceType::HostFileSystem),
                    "CI_PIPELINE" => Ok(SbomSourceType::CiPipeline),
                    "HOST_IMAGE" => Ok(SbomSourceType::HostImage),
                    "SERVERLESS_FUNCTION" => Ok(SbomSourceType::ServerlessFunction),
                    _ => Err(serde::de::Error::unknown_variant(value, FIELDS)),
                }
            }
        }
        deserializer.deserialize_any(GeneratedVisitor)
    }
}
impl serde::Serialize for SbomStatus {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let variant = match self {
            Self::Success => "SUCCESS",
            Self::Failed => "FAILED",
            Self::Pending => "PENDING",
        };
        serializer.serialize_str(variant)
    }
}
impl<'de> serde::Deserialize<'de> for SbomStatus {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "SUCCESS",
            "FAILED",
            "PENDING",
        ];

        struct GeneratedVisitor;

        impl serde::de::Visitor<'_> for GeneratedVisitor {
            type Value = SbomStatus;

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
                    "SUCCESS" => Ok(SbomStatus::Success),
                    "FAILED" => Ok(SbomStatus::Failed),
                    "PENDING" => Ok(SbomStatus::Pending),
                    _ => Err(serde::de::Error::unknown_variant(value, FIELDS)),
                }
            }
        }
        deserializer.deserialize_any(GeneratedVisitor)
    }
}
