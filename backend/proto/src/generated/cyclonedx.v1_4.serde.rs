impl serde::Serialize for Advisory {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.title.is_some() {
            len += 1;
        }
        if !self.url.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("cyclonedx.v1_4.Advisory", len)?;
        if let Some(v) = self.title.as_ref() {
            struct_ser.serialize_field("title", v)?;
        }
        if !self.url.is_empty() {
            struct_ser.serialize_field("url", &self.url)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for Advisory {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "title",
            "url",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Title,
            Url,
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
                            "title" => Ok(GeneratedField::Title),
                            "url" => Ok(GeneratedField::Url),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = Advisory;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct cyclonedx.v1_4.Advisory")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<Advisory, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut title__ = None;
                let mut url__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Title => {
                            if title__.is_some() {
                                return Err(serde::de::Error::duplicate_field("title"));
                            }
                            title__ = map_.next_value()?;
                        }
                        GeneratedField::Url => {
                            if url__.is_some() {
                                return Err(serde::de::Error::duplicate_field("url"));
                            }
                            url__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(Advisory {
                    title: title__,
                    url: url__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("cyclonedx.v1_4.Advisory", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for Aggregate {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let variant = match self {
            Self::NotSpecified => "AGGREGATE_NOT_SPECIFIED",
            Self::Complete => "AGGREGATE_COMPLETE",
            Self::Incomplete => "AGGREGATE_INCOMPLETE",
            Self::IncompleteFirstPartyOnly => "AGGREGATE_INCOMPLETE_FIRST_PARTY_ONLY",
            Self::IncompleteThirdPartyOnly => "AGGREGATE_INCOMPLETE_THIRD_PARTY_ONLY",
            Self::Unknown => "AGGREGATE_UNKNOWN",
        };
        serializer.serialize_str(variant)
    }
}
impl<'de> serde::Deserialize<'de> for Aggregate {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "AGGREGATE_NOT_SPECIFIED",
            "AGGREGATE_COMPLETE",
            "AGGREGATE_INCOMPLETE",
            "AGGREGATE_INCOMPLETE_FIRST_PARTY_ONLY",
            "AGGREGATE_INCOMPLETE_THIRD_PARTY_ONLY",
            "AGGREGATE_UNKNOWN",
        ];

        struct GeneratedVisitor;

        impl serde::de::Visitor<'_> for GeneratedVisitor {
            type Value = Aggregate;

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
                    "AGGREGATE_NOT_SPECIFIED" => Ok(Aggregate::NotSpecified),
                    "AGGREGATE_COMPLETE" => Ok(Aggregate::Complete),
                    "AGGREGATE_INCOMPLETE" => Ok(Aggregate::Incomplete),
                    "AGGREGATE_INCOMPLETE_FIRST_PARTY_ONLY" => Ok(Aggregate::IncompleteFirstPartyOnly),
                    "AGGREGATE_INCOMPLETE_THIRD_PARTY_ONLY" => Ok(Aggregate::IncompleteThirdPartyOnly),
                    "AGGREGATE_UNKNOWN" => Ok(Aggregate::Unknown),
                    _ => Err(serde::de::Error::unknown_variant(value, FIELDS)),
                }
            }
        }
        deserializer.deserialize_any(GeneratedVisitor)
    }
}
impl serde::Serialize for AttachedText {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.content_type.is_some() {
            len += 1;
        }
        if self.encoding.is_some() {
            len += 1;
        }
        if !self.value.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("cyclonedx.v1_4.AttachedText", len)?;
        if let Some(v) = self.content_type.as_ref() {
            struct_ser.serialize_field("contentType", v)?;
        }
        if let Some(v) = self.encoding.as_ref() {
            struct_ser.serialize_field("encoding", v)?;
        }
        if !self.value.is_empty() {
            struct_ser.serialize_field("value", &self.value)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for AttachedText {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "content_type",
            "contentType",
            "encoding",
            "value",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            ContentType,
            Encoding,
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
                            "contentType" | "content_type" => Ok(GeneratedField::ContentType),
                            "encoding" => Ok(GeneratedField::Encoding),
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
            type Value = AttachedText;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct cyclonedx.v1_4.AttachedText")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<AttachedText, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut content_type__ = None;
                let mut encoding__ = None;
                let mut value__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::ContentType => {
                            if content_type__.is_some() {
                                return Err(serde::de::Error::duplicate_field("contentType"));
                            }
                            content_type__ = map_.next_value()?;
                        }
                        GeneratedField::Encoding => {
                            if encoding__.is_some() {
                                return Err(serde::de::Error::duplicate_field("encoding"));
                            }
                            encoding__ = map_.next_value()?;
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
                Ok(AttachedText {
                    content_type: content_type__,
                    encoding: encoding__,
                    value: value__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("cyclonedx.v1_4.AttachedText", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for Bom {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.spec_version.is_empty() {
            len += 1;
        }
        if self.version.is_some() {
            len += 1;
        }
        if self.serial_number.is_some() {
            len += 1;
        }
        if self.metadata.is_some() {
            len += 1;
        }
        if !self.components.is_empty() {
            len += 1;
        }
        if !self.services.is_empty() {
            len += 1;
        }
        if !self.external_references.is_empty() {
            len += 1;
        }
        if !self.dependencies.is_empty() {
            len += 1;
        }
        if !self.compositions.is_empty() {
            len += 1;
        }
        if !self.vulnerabilities.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("cyclonedx.v1_4.Bom", len)?;
        if !self.spec_version.is_empty() {
            struct_ser.serialize_field("specVersion", &self.spec_version)?;
        }
        if let Some(v) = self.version.as_ref() {
            struct_ser.serialize_field("version", v)?;
        }
        if let Some(v) = self.serial_number.as_ref() {
            struct_ser.serialize_field("serialNumber", v)?;
        }
        if let Some(v) = self.metadata.as_ref() {
            struct_ser.serialize_field("metadata", v)?;
        }
        if !self.components.is_empty() {
            struct_ser.serialize_field("components", &self.components)?;
        }
        if !self.services.is_empty() {
            struct_ser.serialize_field("services", &self.services)?;
        }
        if !self.external_references.is_empty() {
            struct_ser.serialize_field("externalReferences", &self.external_references)?;
        }
        if !self.dependencies.is_empty() {
            struct_ser.serialize_field("dependencies", &self.dependencies)?;
        }
        if !self.compositions.is_empty() {
            struct_ser.serialize_field("compositions", &self.compositions)?;
        }
        if !self.vulnerabilities.is_empty() {
            struct_ser.serialize_field("vulnerabilities", &self.vulnerabilities)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for Bom {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "spec_version",
            "specVersion",
            "version",
            "serial_number",
            "serialNumber",
            "metadata",
            "components",
            "services",
            "external_references",
            "externalReferences",
            "dependencies",
            "compositions",
            "vulnerabilities",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            SpecVersion,
            Version,
            SerialNumber,
            Metadata,
            Components,
            Services,
            ExternalReferences,
            Dependencies,
            Compositions,
            Vulnerabilities,
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
                            "specVersion" | "spec_version" => Ok(GeneratedField::SpecVersion),
                            "version" => Ok(GeneratedField::Version),
                            "serialNumber" | "serial_number" => Ok(GeneratedField::SerialNumber),
                            "metadata" => Ok(GeneratedField::Metadata),
                            "components" => Ok(GeneratedField::Components),
                            "services" => Ok(GeneratedField::Services),
                            "externalReferences" | "external_references" => Ok(GeneratedField::ExternalReferences),
                            "dependencies" => Ok(GeneratedField::Dependencies),
                            "compositions" => Ok(GeneratedField::Compositions),
                            "vulnerabilities" => Ok(GeneratedField::Vulnerabilities),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = Bom;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct cyclonedx.v1_4.Bom")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<Bom, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut spec_version__ = None;
                let mut version__ = None;
                let mut serial_number__ = None;
                let mut metadata__ = None;
                let mut components__ = None;
                let mut services__ = None;
                let mut external_references__ = None;
                let mut dependencies__ = None;
                let mut compositions__ = None;
                let mut vulnerabilities__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::SpecVersion => {
                            if spec_version__.is_some() {
                                return Err(serde::de::Error::duplicate_field("specVersion"));
                            }
                            spec_version__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Version => {
                            if version__.is_some() {
                                return Err(serde::de::Error::duplicate_field("version"));
                            }
                            version__ = 
                                map_.next_value::<::std::option::Option<::pbjson::private::NumberDeserialize<_>>>()?.map(|x| x.0)
                            ;
                        }
                        GeneratedField::SerialNumber => {
                            if serial_number__.is_some() {
                                return Err(serde::de::Error::duplicate_field("serialNumber"));
                            }
                            serial_number__ = map_.next_value()?;
                        }
                        GeneratedField::Metadata => {
                            if metadata__.is_some() {
                                return Err(serde::de::Error::duplicate_field("metadata"));
                            }
                            metadata__ = map_.next_value()?;
                        }
                        GeneratedField::Components => {
                            if components__.is_some() {
                                return Err(serde::de::Error::duplicate_field("components"));
                            }
                            components__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Services => {
                            if services__.is_some() {
                                return Err(serde::de::Error::duplicate_field("services"));
                            }
                            services__ = Some(map_.next_value()?);
                        }
                        GeneratedField::ExternalReferences => {
                            if external_references__.is_some() {
                                return Err(serde::de::Error::duplicate_field("externalReferences"));
                            }
                            external_references__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Dependencies => {
                            if dependencies__.is_some() {
                                return Err(serde::de::Error::duplicate_field("dependencies"));
                            }
                            dependencies__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Compositions => {
                            if compositions__.is_some() {
                                return Err(serde::de::Error::duplicate_field("compositions"));
                            }
                            compositions__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Vulnerabilities => {
                            if vulnerabilities__.is_some() {
                                return Err(serde::de::Error::duplicate_field("vulnerabilities"));
                            }
                            vulnerabilities__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(Bom {
                    spec_version: spec_version__.unwrap_or_default(),
                    version: version__,
                    serial_number: serial_number__,
                    metadata: metadata__,
                    components: components__.unwrap_or_default(),
                    services: services__.unwrap_or_default(),
                    external_references: external_references__.unwrap_or_default(),
                    dependencies: dependencies__.unwrap_or_default(),
                    compositions: compositions__.unwrap_or_default(),
                    vulnerabilities: vulnerabilities__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("cyclonedx.v1_4.Bom", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for Classification {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let variant = match self {
            Self::Null => "CLASSIFICATION_NULL",
            Self::Application => "CLASSIFICATION_APPLICATION",
            Self::Framework => "CLASSIFICATION_FRAMEWORK",
            Self::Library => "CLASSIFICATION_LIBRARY",
            Self::OperatingSystem => "CLASSIFICATION_OPERATING_SYSTEM",
            Self::Device => "CLASSIFICATION_DEVICE",
            Self::File => "CLASSIFICATION_FILE",
            Self::Container => "CLASSIFICATION_CONTAINER",
            Self::Firmware => "CLASSIFICATION_FIRMWARE",
        };
        serializer.serialize_str(variant)
    }
}
impl<'de> serde::Deserialize<'de> for Classification {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "CLASSIFICATION_NULL",
            "CLASSIFICATION_APPLICATION",
            "CLASSIFICATION_FRAMEWORK",
            "CLASSIFICATION_LIBRARY",
            "CLASSIFICATION_OPERATING_SYSTEM",
            "CLASSIFICATION_DEVICE",
            "CLASSIFICATION_FILE",
            "CLASSIFICATION_CONTAINER",
            "CLASSIFICATION_FIRMWARE",
        ];

        struct GeneratedVisitor;

        impl serde::de::Visitor<'_> for GeneratedVisitor {
            type Value = Classification;

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
                    "CLASSIFICATION_NULL" => Ok(Classification::Null),
                    "CLASSIFICATION_APPLICATION" => Ok(Classification::Application),
                    "CLASSIFICATION_FRAMEWORK" => Ok(Classification::Framework),
                    "CLASSIFICATION_LIBRARY" => Ok(Classification::Library),
                    "CLASSIFICATION_OPERATING_SYSTEM" => Ok(Classification::OperatingSystem),
                    "CLASSIFICATION_DEVICE" => Ok(Classification::Device),
                    "CLASSIFICATION_FILE" => Ok(Classification::File),
                    "CLASSIFICATION_CONTAINER" => Ok(Classification::Container),
                    "CLASSIFICATION_FIRMWARE" => Ok(Classification::Firmware),
                    _ => Err(serde::de::Error::unknown_variant(value, FIELDS)),
                }
            }
        }
        deserializer.deserialize_any(GeneratedVisitor)
    }
}
impl serde::Serialize for Commit {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.uid.is_some() {
            len += 1;
        }
        if self.url.is_some() {
            len += 1;
        }
        if self.author.is_some() {
            len += 1;
        }
        if self.committer.is_some() {
            len += 1;
        }
        if self.message.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("cyclonedx.v1_4.Commit", len)?;
        if let Some(v) = self.uid.as_ref() {
            struct_ser.serialize_field("uid", v)?;
        }
        if let Some(v) = self.url.as_ref() {
            struct_ser.serialize_field("url", v)?;
        }
        if let Some(v) = self.author.as_ref() {
            struct_ser.serialize_field("author", v)?;
        }
        if let Some(v) = self.committer.as_ref() {
            struct_ser.serialize_field("committer", v)?;
        }
        if let Some(v) = self.message.as_ref() {
            struct_ser.serialize_field("message", v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for Commit {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "uid",
            "url",
            "author",
            "committer",
            "message",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Uid,
            Url,
            Author,
            Committer,
            Message,
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
                            "uid" => Ok(GeneratedField::Uid),
                            "url" => Ok(GeneratedField::Url),
                            "author" => Ok(GeneratedField::Author),
                            "committer" => Ok(GeneratedField::Committer),
                            "message" => Ok(GeneratedField::Message),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = Commit;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct cyclonedx.v1_4.Commit")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<Commit, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut uid__ = None;
                let mut url__ = None;
                let mut author__ = None;
                let mut committer__ = None;
                let mut message__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Uid => {
                            if uid__.is_some() {
                                return Err(serde::de::Error::duplicate_field("uid"));
                            }
                            uid__ = map_.next_value()?;
                        }
                        GeneratedField::Url => {
                            if url__.is_some() {
                                return Err(serde::de::Error::duplicate_field("url"));
                            }
                            url__ = map_.next_value()?;
                        }
                        GeneratedField::Author => {
                            if author__.is_some() {
                                return Err(serde::de::Error::duplicate_field("author"));
                            }
                            author__ = map_.next_value()?;
                        }
                        GeneratedField::Committer => {
                            if committer__.is_some() {
                                return Err(serde::de::Error::duplicate_field("committer"));
                            }
                            committer__ = map_.next_value()?;
                        }
                        GeneratedField::Message => {
                            if message__.is_some() {
                                return Err(serde::de::Error::duplicate_field("message"));
                            }
                            message__ = map_.next_value()?;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(Commit {
                    uid: uid__,
                    url: url__,
                    author: author__,
                    committer: committer__,
                    message: message__,
                })
            }
        }
        deserializer.deserialize_struct("cyclonedx.v1_4.Commit", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for Component {
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
        if self.mime_type.is_some() {
            len += 1;
        }
        if self.bom_ref.is_some() {
            len += 1;
        }
        if self.supplier.is_some() {
            len += 1;
        }
        if self.author.is_some() {
            len += 1;
        }
        if self.publisher.is_some() {
            len += 1;
        }
        if self.group.is_some() {
            len += 1;
        }
        if !self.name.is_empty() {
            len += 1;
        }
        if !self.version.is_empty() {
            len += 1;
        }
        if self.description.is_some() {
            len += 1;
        }
        if self.scope.is_some() {
            len += 1;
        }
        if !self.hashes.is_empty() {
            len += 1;
        }
        if !self.licenses.is_empty() {
            len += 1;
        }
        if self.copyright.is_some() {
            len += 1;
        }
        if self.cpe.is_some() {
            len += 1;
        }
        if self.purl.is_some() {
            len += 1;
        }
        if self.swid.is_some() {
            len += 1;
        }
        if self.modified.is_some() {
            len += 1;
        }
        if self.pedigree.is_some() {
            len += 1;
        }
        if !self.external_references.is_empty() {
            len += 1;
        }
        if !self.components.is_empty() {
            len += 1;
        }
        if !self.properties.is_empty() {
            len += 1;
        }
        if !self.evidence.is_empty() {
            len += 1;
        }
        if self.release_notes.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("cyclonedx.v1_4.Component", len)?;
        if self.r#type != 0 {
            let v = Classification::try_from(self.r#type)
                .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", self.r#type)))?;
            struct_ser.serialize_field("type", &v)?;
        }
        if let Some(v) = self.mime_type.as_ref() {
            struct_ser.serialize_field("mimeType", v)?;
        }
        if let Some(v) = self.bom_ref.as_ref() {
            struct_ser.serialize_field("bomRef", v)?;
        }
        if let Some(v) = self.supplier.as_ref() {
            struct_ser.serialize_field("supplier", v)?;
        }
        if let Some(v) = self.author.as_ref() {
            struct_ser.serialize_field("author", v)?;
        }
        if let Some(v) = self.publisher.as_ref() {
            struct_ser.serialize_field("publisher", v)?;
        }
        if let Some(v) = self.group.as_ref() {
            struct_ser.serialize_field("group", v)?;
        }
        if !self.name.is_empty() {
            struct_ser.serialize_field("name", &self.name)?;
        }
        if !self.version.is_empty() {
            struct_ser.serialize_field("version", &self.version)?;
        }
        if let Some(v) = self.description.as_ref() {
            struct_ser.serialize_field("description", v)?;
        }
        if let Some(v) = self.scope.as_ref() {
            let v = Scope::try_from(*v)
                .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", *v)))?;
            struct_ser.serialize_field("scope", &v)?;
        }
        if !self.hashes.is_empty() {
            struct_ser.serialize_field("hashes", &self.hashes)?;
        }
        if !self.licenses.is_empty() {
            struct_ser.serialize_field("licenses", &self.licenses)?;
        }
        if let Some(v) = self.copyright.as_ref() {
            struct_ser.serialize_field("copyright", v)?;
        }
        if let Some(v) = self.cpe.as_ref() {
            struct_ser.serialize_field("cpe", v)?;
        }
        if let Some(v) = self.purl.as_ref() {
            struct_ser.serialize_field("purl", v)?;
        }
        if let Some(v) = self.swid.as_ref() {
            struct_ser.serialize_field("swid", v)?;
        }
        if let Some(v) = self.modified.as_ref() {
            struct_ser.serialize_field("modified", v)?;
        }
        if let Some(v) = self.pedigree.as_ref() {
            struct_ser.serialize_field("pedigree", v)?;
        }
        if !self.external_references.is_empty() {
            struct_ser.serialize_field("externalReferences", &self.external_references)?;
        }
        if !self.components.is_empty() {
            struct_ser.serialize_field("components", &self.components)?;
        }
        if !self.properties.is_empty() {
            struct_ser.serialize_field("properties", &self.properties)?;
        }
        if !self.evidence.is_empty() {
            struct_ser.serialize_field("evidence", &self.evidence)?;
        }
        if let Some(v) = self.release_notes.as_ref() {
            struct_ser.serialize_field("releaseNotes", v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for Component {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "type",
            "mime_type",
            "mimeType",
            "bom_ref",
            "bomRef",
            "supplier",
            "author",
            "publisher",
            "group",
            "name",
            "version",
            "description",
            "scope",
            "hashes",
            "licenses",
            "copyright",
            "cpe",
            "purl",
            "swid",
            "modified",
            "pedigree",
            "external_references",
            "externalReferences",
            "components",
            "properties",
            "evidence",
            "releaseNotes",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Type,
            MimeType,
            BomRef,
            Supplier,
            Author,
            Publisher,
            Group,
            Name,
            Version,
            Description,
            Scope,
            Hashes,
            Licenses,
            Copyright,
            Cpe,
            Purl,
            Swid,
            Modified,
            Pedigree,
            ExternalReferences,
            Components,
            Properties,
            Evidence,
            ReleaseNotes,
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
                            "mimeType" | "mime_type" => Ok(GeneratedField::MimeType),
                            "bomRef" | "bom_ref" => Ok(GeneratedField::BomRef),
                            "supplier" => Ok(GeneratedField::Supplier),
                            "author" => Ok(GeneratedField::Author),
                            "publisher" => Ok(GeneratedField::Publisher),
                            "group" => Ok(GeneratedField::Group),
                            "name" => Ok(GeneratedField::Name),
                            "version" => Ok(GeneratedField::Version),
                            "description" => Ok(GeneratedField::Description),
                            "scope" => Ok(GeneratedField::Scope),
                            "hashes" => Ok(GeneratedField::Hashes),
                            "licenses" => Ok(GeneratedField::Licenses),
                            "copyright" => Ok(GeneratedField::Copyright),
                            "cpe" => Ok(GeneratedField::Cpe),
                            "purl" => Ok(GeneratedField::Purl),
                            "swid" => Ok(GeneratedField::Swid),
                            "modified" => Ok(GeneratedField::Modified),
                            "pedigree" => Ok(GeneratedField::Pedigree),
                            "externalReferences" | "external_references" => Ok(GeneratedField::ExternalReferences),
                            "components" => Ok(GeneratedField::Components),
                            "properties" => Ok(GeneratedField::Properties),
                            "evidence" => Ok(GeneratedField::Evidence),
                            "releaseNotes" => Ok(GeneratedField::ReleaseNotes),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = Component;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct cyclonedx.v1_4.Component")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<Component, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut r#type__ = None;
                let mut mime_type__ = None;
                let mut bom_ref__ = None;
                let mut supplier__ = None;
                let mut author__ = None;
                let mut publisher__ = None;
                let mut group__ = None;
                let mut name__ = None;
                let mut version__ = None;
                let mut description__ = None;
                let mut scope__ = None;
                let mut hashes__ = None;
                let mut licenses__ = None;
                let mut copyright__ = None;
                let mut cpe__ = None;
                let mut purl__ = None;
                let mut swid__ = None;
                let mut modified__ = None;
                let mut pedigree__ = None;
                let mut external_references__ = None;
                let mut components__ = None;
                let mut properties__ = None;
                let mut evidence__ = None;
                let mut release_notes__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Type => {
                            if r#type__.is_some() {
                                return Err(serde::de::Error::duplicate_field("type"));
                            }
                            r#type__ = Some(map_.next_value::<Classification>()? as i32);
                        }
                        GeneratedField::MimeType => {
                            if mime_type__.is_some() {
                                return Err(serde::de::Error::duplicate_field("mimeType"));
                            }
                            mime_type__ = map_.next_value()?;
                        }
                        GeneratedField::BomRef => {
                            if bom_ref__.is_some() {
                                return Err(serde::de::Error::duplicate_field("bomRef"));
                            }
                            bom_ref__ = map_.next_value()?;
                        }
                        GeneratedField::Supplier => {
                            if supplier__.is_some() {
                                return Err(serde::de::Error::duplicate_field("supplier"));
                            }
                            supplier__ = map_.next_value()?;
                        }
                        GeneratedField::Author => {
                            if author__.is_some() {
                                return Err(serde::de::Error::duplicate_field("author"));
                            }
                            author__ = map_.next_value()?;
                        }
                        GeneratedField::Publisher => {
                            if publisher__.is_some() {
                                return Err(serde::de::Error::duplicate_field("publisher"));
                            }
                            publisher__ = map_.next_value()?;
                        }
                        GeneratedField::Group => {
                            if group__.is_some() {
                                return Err(serde::de::Error::duplicate_field("group"));
                            }
                            group__ = map_.next_value()?;
                        }
                        GeneratedField::Name => {
                            if name__.is_some() {
                                return Err(serde::de::Error::duplicate_field("name"));
                            }
                            name__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Version => {
                            if version__.is_some() {
                                return Err(serde::de::Error::duplicate_field("version"));
                            }
                            version__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Description => {
                            if description__.is_some() {
                                return Err(serde::de::Error::duplicate_field("description"));
                            }
                            description__ = map_.next_value()?;
                        }
                        GeneratedField::Scope => {
                            if scope__.is_some() {
                                return Err(serde::de::Error::duplicate_field("scope"));
                            }
                            scope__ = map_.next_value::<::std::option::Option<Scope>>()?.map(|x| x as i32);
                        }
                        GeneratedField::Hashes => {
                            if hashes__.is_some() {
                                return Err(serde::de::Error::duplicate_field("hashes"));
                            }
                            hashes__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Licenses => {
                            if licenses__.is_some() {
                                return Err(serde::de::Error::duplicate_field("licenses"));
                            }
                            licenses__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Copyright => {
                            if copyright__.is_some() {
                                return Err(serde::de::Error::duplicate_field("copyright"));
                            }
                            copyright__ = map_.next_value()?;
                        }
                        GeneratedField::Cpe => {
                            if cpe__.is_some() {
                                return Err(serde::de::Error::duplicate_field("cpe"));
                            }
                            cpe__ = map_.next_value()?;
                        }
                        GeneratedField::Purl => {
                            if purl__.is_some() {
                                return Err(serde::de::Error::duplicate_field("purl"));
                            }
                            purl__ = map_.next_value()?;
                        }
                        GeneratedField::Swid => {
                            if swid__.is_some() {
                                return Err(serde::de::Error::duplicate_field("swid"));
                            }
                            swid__ = map_.next_value()?;
                        }
                        GeneratedField::Modified => {
                            if modified__.is_some() {
                                return Err(serde::de::Error::duplicate_field("modified"));
                            }
                            modified__ = map_.next_value()?;
                        }
                        GeneratedField::Pedigree => {
                            if pedigree__.is_some() {
                                return Err(serde::de::Error::duplicate_field("pedigree"));
                            }
                            pedigree__ = map_.next_value()?;
                        }
                        GeneratedField::ExternalReferences => {
                            if external_references__.is_some() {
                                return Err(serde::de::Error::duplicate_field("externalReferences"));
                            }
                            external_references__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Components => {
                            if components__.is_some() {
                                return Err(serde::de::Error::duplicate_field("components"));
                            }
                            components__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Properties => {
                            if properties__.is_some() {
                                return Err(serde::de::Error::duplicate_field("properties"));
                            }
                            properties__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Evidence => {
                            if evidence__.is_some() {
                                return Err(serde::de::Error::duplicate_field("evidence"));
                            }
                            evidence__ = Some(map_.next_value()?);
                        }
                        GeneratedField::ReleaseNotes => {
                            if release_notes__.is_some() {
                                return Err(serde::de::Error::duplicate_field("releaseNotes"));
                            }
                            release_notes__ = map_.next_value()?;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(Component {
                    r#type: r#type__.unwrap_or_default(),
                    mime_type: mime_type__,
                    bom_ref: bom_ref__,
                    supplier: supplier__,
                    author: author__,
                    publisher: publisher__,
                    group: group__,
                    name: name__.unwrap_or_default(),
                    version: version__.unwrap_or_default(),
                    description: description__,
                    scope: scope__,
                    hashes: hashes__.unwrap_or_default(),
                    licenses: licenses__.unwrap_or_default(),
                    copyright: copyright__,
                    cpe: cpe__,
                    purl: purl__,
                    swid: swid__,
                    modified: modified__,
                    pedigree: pedigree__,
                    external_references: external_references__.unwrap_or_default(),
                    components: components__.unwrap_or_default(),
                    properties: properties__.unwrap_or_default(),
                    evidence: evidence__.unwrap_or_default(),
                    release_notes: release_notes__,
                })
            }
        }
        deserializer.deserialize_struct("cyclonedx.v1_4.Component", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for Composition {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.aggregate != 0 {
            len += 1;
        }
        if !self.assemblies.is_empty() {
            len += 1;
        }
        if !self.dependencies.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("cyclonedx.v1_4.Composition", len)?;
        if self.aggregate != 0 {
            let v = Aggregate::try_from(self.aggregate)
                .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", self.aggregate)))?;
            struct_ser.serialize_field("aggregate", &v)?;
        }
        if !self.assemblies.is_empty() {
            struct_ser.serialize_field("assemblies", &self.assemblies)?;
        }
        if !self.dependencies.is_empty() {
            struct_ser.serialize_field("dependencies", &self.dependencies)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for Composition {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "aggregate",
            "assemblies",
            "dependencies",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Aggregate,
            Assemblies,
            Dependencies,
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
                            "aggregate" => Ok(GeneratedField::Aggregate),
                            "assemblies" => Ok(GeneratedField::Assemblies),
                            "dependencies" => Ok(GeneratedField::Dependencies),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = Composition;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct cyclonedx.v1_4.Composition")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<Composition, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut aggregate__ = None;
                let mut assemblies__ = None;
                let mut dependencies__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Aggregate => {
                            if aggregate__.is_some() {
                                return Err(serde::de::Error::duplicate_field("aggregate"));
                            }
                            aggregate__ = Some(map_.next_value::<Aggregate>()? as i32);
                        }
                        GeneratedField::Assemblies => {
                            if assemblies__.is_some() {
                                return Err(serde::de::Error::duplicate_field("assemblies"));
                            }
                            assemblies__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Dependencies => {
                            if dependencies__.is_some() {
                                return Err(serde::de::Error::duplicate_field("dependencies"));
                            }
                            dependencies__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(Composition {
                    aggregate: aggregate__.unwrap_or_default(),
                    assemblies: assemblies__.unwrap_or_default(),
                    dependencies: dependencies__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("cyclonedx.v1_4.Composition", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for DataClassification {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.flow != 0 {
            len += 1;
        }
        if !self.value.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("cyclonedx.v1_4.DataClassification", len)?;
        if self.flow != 0 {
            let v = DataFlow::try_from(self.flow)
                .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", self.flow)))?;
            struct_ser.serialize_field("flow", &v)?;
        }
        if !self.value.is_empty() {
            struct_ser.serialize_field("value", &self.value)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for DataClassification {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "flow",
            "value",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Flow,
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
                            "flow" => Ok(GeneratedField::Flow),
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
            type Value = DataClassification;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct cyclonedx.v1_4.DataClassification")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<DataClassification, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut flow__ = None;
                let mut value__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Flow => {
                            if flow__.is_some() {
                                return Err(serde::de::Error::duplicate_field("flow"));
                            }
                            flow__ = Some(map_.next_value::<DataFlow>()? as i32);
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
                Ok(DataClassification {
                    flow: flow__.unwrap_or_default(),
                    value: value__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("cyclonedx.v1_4.DataClassification", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for DataFlow {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let variant = match self {
            Self::Null => "DATA_FLOW_NULL",
            Self::Inbound => "DATA_FLOW_INBOUND",
            Self::Outbound => "DATA_FLOW_OUTBOUND",
            Self::BiDirectional => "DATA_FLOW_BI_DIRECTIONAL",
            Self::Unknown => "DATA_FLOW_UNKNOWN",
        };
        serializer.serialize_str(variant)
    }
}
impl<'de> serde::Deserialize<'de> for DataFlow {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "DATA_FLOW_NULL",
            "DATA_FLOW_INBOUND",
            "DATA_FLOW_OUTBOUND",
            "DATA_FLOW_BI_DIRECTIONAL",
            "DATA_FLOW_UNKNOWN",
        ];

        struct GeneratedVisitor;

        impl serde::de::Visitor<'_> for GeneratedVisitor {
            type Value = DataFlow;

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
                    "DATA_FLOW_NULL" => Ok(DataFlow::Null),
                    "DATA_FLOW_INBOUND" => Ok(DataFlow::Inbound),
                    "DATA_FLOW_OUTBOUND" => Ok(DataFlow::Outbound),
                    "DATA_FLOW_BI_DIRECTIONAL" => Ok(DataFlow::BiDirectional),
                    "DATA_FLOW_UNKNOWN" => Ok(DataFlow::Unknown),
                    _ => Err(serde::de::Error::unknown_variant(value, FIELDS)),
                }
            }
        }
        deserializer.deserialize_any(GeneratedVisitor)
    }
}
impl serde::Serialize for Dependency {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.r#ref.is_empty() {
            len += 1;
        }
        if !self.dependencies.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("cyclonedx.v1_4.Dependency", len)?;
        if !self.r#ref.is_empty() {
            struct_ser.serialize_field("ref", &self.r#ref)?;
        }
        if !self.dependencies.is_empty() {
            struct_ser.serialize_field("dependencies", &self.dependencies)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for Dependency {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "ref",
            "dependencies",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Ref,
            Dependencies,
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
                            "ref" => Ok(GeneratedField::Ref),
                            "dependencies" => Ok(GeneratedField::Dependencies),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = Dependency;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct cyclonedx.v1_4.Dependency")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<Dependency, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut r#ref__ = None;
                let mut dependencies__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Ref => {
                            if r#ref__.is_some() {
                                return Err(serde::de::Error::duplicate_field("ref"));
                            }
                            r#ref__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Dependencies => {
                            if dependencies__.is_some() {
                                return Err(serde::de::Error::duplicate_field("dependencies"));
                            }
                            dependencies__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(Dependency {
                    r#ref: r#ref__.unwrap_or_default(),
                    dependencies: dependencies__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("cyclonedx.v1_4.Dependency", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for Diff {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.text.is_some() {
            len += 1;
        }
        if self.url.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("cyclonedx.v1_4.Diff", len)?;
        if let Some(v) = self.text.as_ref() {
            struct_ser.serialize_field("text", v)?;
        }
        if let Some(v) = self.url.as_ref() {
            struct_ser.serialize_field("url", v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for Diff {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "text",
            "url",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Text,
            Url,
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
                            "text" => Ok(GeneratedField::Text),
                            "url" => Ok(GeneratedField::Url),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = Diff;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct cyclonedx.v1_4.Diff")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<Diff, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut text__ = None;
                let mut url__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Text => {
                            if text__.is_some() {
                                return Err(serde::de::Error::duplicate_field("text"));
                            }
                            text__ = map_.next_value()?;
                        }
                        GeneratedField::Url => {
                            if url__.is_some() {
                                return Err(serde::de::Error::duplicate_field("url"));
                            }
                            url__ = map_.next_value()?;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(Diff {
                    text: text__,
                    url: url__,
                })
            }
        }
        deserializer.deserialize_struct("cyclonedx.v1_4.Diff", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for Evidence {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.licenses.is_empty() {
            len += 1;
        }
        if !self.copyright.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("cyclonedx.v1_4.Evidence", len)?;
        if !self.licenses.is_empty() {
            struct_ser.serialize_field("licenses", &self.licenses)?;
        }
        if !self.copyright.is_empty() {
            struct_ser.serialize_field("copyright", &self.copyright)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for Evidence {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "licenses",
            "copyright",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Licenses,
            Copyright,
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
                            "licenses" => Ok(GeneratedField::Licenses),
                            "copyright" => Ok(GeneratedField::Copyright),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = Evidence;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct cyclonedx.v1_4.Evidence")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<Evidence, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut licenses__ = None;
                let mut copyright__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Licenses => {
                            if licenses__.is_some() {
                                return Err(serde::de::Error::duplicate_field("licenses"));
                            }
                            licenses__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Copyright => {
                            if copyright__.is_some() {
                                return Err(serde::de::Error::duplicate_field("copyright"));
                            }
                            copyright__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(Evidence {
                    licenses: licenses__.unwrap_or_default(),
                    copyright: copyright__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("cyclonedx.v1_4.Evidence", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for EvidenceCopyright {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.text.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("cyclonedx.v1_4.EvidenceCopyright", len)?;
        if !self.text.is_empty() {
            struct_ser.serialize_field("text", &self.text)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for EvidenceCopyright {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "text",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Text,
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
                            "text" => Ok(GeneratedField::Text),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = EvidenceCopyright;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct cyclonedx.v1_4.EvidenceCopyright")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<EvidenceCopyright, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut text__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Text => {
                            if text__.is_some() {
                                return Err(serde::de::Error::duplicate_field("text"));
                            }
                            text__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(EvidenceCopyright {
                    text: text__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("cyclonedx.v1_4.EvidenceCopyright", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for ExternalReference {
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
        if !self.url.is_empty() {
            len += 1;
        }
        if self.comment.is_some() {
            len += 1;
        }
        if !self.hashes.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("cyclonedx.v1_4.ExternalReference", len)?;
        if self.r#type != 0 {
            let v = ExternalReferenceType::try_from(self.r#type)
                .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", self.r#type)))?;
            struct_ser.serialize_field("type", &v)?;
        }
        if !self.url.is_empty() {
            struct_ser.serialize_field("url", &self.url)?;
        }
        if let Some(v) = self.comment.as_ref() {
            struct_ser.serialize_field("comment", v)?;
        }
        if !self.hashes.is_empty() {
            struct_ser.serialize_field("hashes", &self.hashes)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for ExternalReference {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "type",
            "url",
            "comment",
            "hashes",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Type,
            Url,
            Comment,
            Hashes,
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
                            "url" => Ok(GeneratedField::Url),
                            "comment" => Ok(GeneratedField::Comment),
                            "hashes" => Ok(GeneratedField::Hashes),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = ExternalReference;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct cyclonedx.v1_4.ExternalReference")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<ExternalReference, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut r#type__ = None;
                let mut url__ = None;
                let mut comment__ = None;
                let mut hashes__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Type => {
                            if r#type__.is_some() {
                                return Err(serde::de::Error::duplicate_field("type"));
                            }
                            r#type__ = Some(map_.next_value::<ExternalReferenceType>()? as i32);
                        }
                        GeneratedField::Url => {
                            if url__.is_some() {
                                return Err(serde::de::Error::duplicate_field("url"));
                            }
                            url__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Comment => {
                            if comment__.is_some() {
                                return Err(serde::de::Error::duplicate_field("comment"));
                            }
                            comment__ = map_.next_value()?;
                        }
                        GeneratedField::Hashes => {
                            if hashes__.is_some() {
                                return Err(serde::de::Error::duplicate_field("hashes"));
                            }
                            hashes__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(ExternalReference {
                    r#type: r#type__.unwrap_or_default(),
                    url: url__.unwrap_or_default(),
                    comment: comment__,
                    hashes: hashes__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("cyclonedx.v1_4.ExternalReference", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for ExternalReferenceType {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let variant = match self {
            Self::Other => "EXTERNAL_REFERENCE_TYPE_OTHER",
            Self::Vcs => "EXTERNAL_REFERENCE_TYPE_VCS",
            Self::IssueTracker => "EXTERNAL_REFERENCE_TYPE_ISSUE_TRACKER",
            Self::Website => "EXTERNAL_REFERENCE_TYPE_WEBSITE",
            Self::Advisories => "EXTERNAL_REFERENCE_TYPE_ADVISORIES",
            Self::Bom => "EXTERNAL_REFERENCE_TYPE_BOM",
            Self::MailingList => "EXTERNAL_REFERENCE_TYPE_MAILING_LIST",
            Self::Social => "EXTERNAL_REFERENCE_TYPE_SOCIAL",
            Self::Chat => "EXTERNAL_REFERENCE_TYPE_CHAT",
            Self::Documentation => "EXTERNAL_REFERENCE_TYPE_DOCUMENTATION",
            Self::Support => "EXTERNAL_REFERENCE_TYPE_SUPPORT",
            Self::Distribution => "EXTERNAL_REFERENCE_TYPE_DISTRIBUTION",
            Self::License => "EXTERNAL_REFERENCE_TYPE_LICENSE",
            Self::BuildMeta => "EXTERNAL_REFERENCE_TYPE_BUILD_META",
            Self::BuildSystem => "EXTERNAL_REFERENCE_TYPE_BUILD_SYSTEM",
        };
        serializer.serialize_str(variant)
    }
}
impl<'de> serde::Deserialize<'de> for ExternalReferenceType {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "EXTERNAL_REFERENCE_TYPE_OTHER",
            "EXTERNAL_REFERENCE_TYPE_VCS",
            "EXTERNAL_REFERENCE_TYPE_ISSUE_TRACKER",
            "EXTERNAL_REFERENCE_TYPE_WEBSITE",
            "EXTERNAL_REFERENCE_TYPE_ADVISORIES",
            "EXTERNAL_REFERENCE_TYPE_BOM",
            "EXTERNAL_REFERENCE_TYPE_MAILING_LIST",
            "EXTERNAL_REFERENCE_TYPE_SOCIAL",
            "EXTERNAL_REFERENCE_TYPE_CHAT",
            "EXTERNAL_REFERENCE_TYPE_DOCUMENTATION",
            "EXTERNAL_REFERENCE_TYPE_SUPPORT",
            "EXTERNAL_REFERENCE_TYPE_DISTRIBUTION",
            "EXTERNAL_REFERENCE_TYPE_LICENSE",
            "EXTERNAL_REFERENCE_TYPE_BUILD_META",
            "EXTERNAL_REFERENCE_TYPE_BUILD_SYSTEM",
        ];

        struct GeneratedVisitor;

        impl serde::de::Visitor<'_> for GeneratedVisitor {
            type Value = ExternalReferenceType;

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
                    "EXTERNAL_REFERENCE_TYPE_OTHER" => Ok(ExternalReferenceType::Other),
                    "EXTERNAL_REFERENCE_TYPE_VCS" => Ok(ExternalReferenceType::Vcs),
                    "EXTERNAL_REFERENCE_TYPE_ISSUE_TRACKER" => Ok(ExternalReferenceType::IssueTracker),
                    "EXTERNAL_REFERENCE_TYPE_WEBSITE" => Ok(ExternalReferenceType::Website),
                    "EXTERNAL_REFERENCE_TYPE_ADVISORIES" => Ok(ExternalReferenceType::Advisories),
                    "EXTERNAL_REFERENCE_TYPE_BOM" => Ok(ExternalReferenceType::Bom),
                    "EXTERNAL_REFERENCE_TYPE_MAILING_LIST" => Ok(ExternalReferenceType::MailingList),
                    "EXTERNAL_REFERENCE_TYPE_SOCIAL" => Ok(ExternalReferenceType::Social),
                    "EXTERNAL_REFERENCE_TYPE_CHAT" => Ok(ExternalReferenceType::Chat),
                    "EXTERNAL_REFERENCE_TYPE_DOCUMENTATION" => Ok(ExternalReferenceType::Documentation),
                    "EXTERNAL_REFERENCE_TYPE_SUPPORT" => Ok(ExternalReferenceType::Support),
                    "EXTERNAL_REFERENCE_TYPE_DISTRIBUTION" => Ok(ExternalReferenceType::Distribution),
                    "EXTERNAL_REFERENCE_TYPE_LICENSE" => Ok(ExternalReferenceType::License),
                    "EXTERNAL_REFERENCE_TYPE_BUILD_META" => Ok(ExternalReferenceType::BuildMeta),
                    "EXTERNAL_REFERENCE_TYPE_BUILD_SYSTEM" => Ok(ExternalReferenceType::BuildSystem),
                    _ => Err(serde::de::Error::unknown_variant(value, FIELDS)),
                }
            }
        }
        deserializer.deserialize_any(GeneratedVisitor)
    }
}
impl serde::Serialize for Hash {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.alg != 0 {
            len += 1;
        }
        if !self.value.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("cyclonedx.v1_4.Hash", len)?;
        if self.alg != 0 {
            let v = HashAlg::try_from(self.alg)
                .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", self.alg)))?;
            struct_ser.serialize_field("alg", &v)?;
        }
        if !self.value.is_empty() {
            struct_ser.serialize_field("value", &self.value)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for Hash {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "alg",
            "value",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Alg,
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
                            "alg" => Ok(GeneratedField::Alg),
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
            type Value = Hash;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct cyclonedx.v1_4.Hash")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<Hash, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut alg__ = None;
                let mut value__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Alg => {
                            if alg__.is_some() {
                                return Err(serde::de::Error::duplicate_field("alg"));
                            }
                            alg__ = Some(map_.next_value::<HashAlg>()? as i32);
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
                Ok(Hash {
                    alg: alg__.unwrap_or_default(),
                    value: value__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("cyclonedx.v1_4.Hash", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for HashAlg {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let variant = match self {
            Self::Null => "HASH_ALG_NULL",
            Self::Md5 => "HASH_ALG_MD_5",
            Self::Sha1 => "HASH_ALG_SHA_1",
            Self::Sha256 => "HASH_ALG_SHA_256",
            Self::Sha384 => "HASH_ALG_SHA_384",
            Self::Sha512 => "HASH_ALG_SHA_512",
            Self::Sha3256 => "HASH_ALG_SHA_3_256",
            Self::Sha3384 => "HASH_ALG_SHA_3_384",
            Self::Sha3512 => "HASH_ALG_SHA_3_512",
            Self::Blake2B256 => "HASH_ALG_BLAKE_2_B_256",
            Self::Blake2B384 => "HASH_ALG_BLAKE_2_B_384",
            Self::Blake2B512 => "HASH_ALG_BLAKE_2_B_512",
            Self::Blake3 => "HASH_ALG_BLAKE_3",
        };
        serializer.serialize_str(variant)
    }
}
impl<'de> serde::Deserialize<'de> for HashAlg {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "HASH_ALG_NULL",
            "HASH_ALG_MD_5",
            "HASH_ALG_SHA_1",
            "HASH_ALG_SHA_256",
            "HASH_ALG_SHA_384",
            "HASH_ALG_SHA_512",
            "HASH_ALG_SHA_3_256",
            "HASH_ALG_SHA_3_384",
            "HASH_ALG_SHA_3_512",
            "HASH_ALG_BLAKE_2_B_256",
            "HASH_ALG_BLAKE_2_B_384",
            "HASH_ALG_BLAKE_2_B_512",
            "HASH_ALG_BLAKE_3",
        ];

        struct GeneratedVisitor;

        impl serde::de::Visitor<'_> for GeneratedVisitor {
            type Value = HashAlg;

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
                    "HASH_ALG_NULL" => Ok(HashAlg::Null),
                    "HASH_ALG_MD_5" => Ok(HashAlg::Md5),
                    "HASH_ALG_SHA_1" => Ok(HashAlg::Sha1),
                    "HASH_ALG_SHA_256" => Ok(HashAlg::Sha256),
                    "HASH_ALG_SHA_384" => Ok(HashAlg::Sha384),
                    "HASH_ALG_SHA_512" => Ok(HashAlg::Sha512),
                    "HASH_ALG_SHA_3_256" => Ok(HashAlg::Sha3256),
                    "HASH_ALG_SHA_3_384" => Ok(HashAlg::Sha3384),
                    "HASH_ALG_SHA_3_512" => Ok(HashAlg::Sha3512),
                    "HASH_ALG_BLAKE_2_B_256" => Ok(HashAlg::Blake2B256),
                    "HASH_ALG_BLAKE_2_B_384" => Ok(HashAlg::Blake2B384),
                    "HASH_ALG_BLAKE_2_B_512" => Ok(HashAlg::Blake2B512),
                    "HASH_ALG_BLAKE_3" => Ok(HashAlg::Blake3),
                    _ => Err(serde::de::Error::unknown_variant(value, FIELDS)),
                }
            }
        }
        deserializer.deserialize_any(GeneratedVisitor)
    }
}
impl serde::Serialize for IdentifiableAction {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.timestamp.is_some() {
            len += 1;
        }
        if self.name.is_some() {
            len += 1;
        }
        if self.email.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("cyclonedx.v1_4.IdentifiableAction", len)?;
        if let Some(v) = self.timestamp.as_ref() {
            struct_ser.serialize_field("timestamp", v)?;
        }
        if let Some(v) = self.name.as_ref() {
            struct_ser.serialize_field("name", v)?;
        }
        if let Some(v) = self.email.as_ref() {
            struct_ser.serialize_field("email", v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for IdentifiableAction {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "timestamp",
            "name",
            "email",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Timestamp,
            Name,
            Email,
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
                            "timestamp" => Ok(GeneratedField::Timestamp),
                            "name" => Ok(GeneratedField::Name),
                            "email" => Ok(GeneratedField::Email),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = IdentifiableAction;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct cyclonedx.v1_4.IdentifiableAction")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<IdentifiableAction, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut timestamp__ = None;
                let mut name__ = None;
                let mut email__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Timestamp => {
                            if timestamp__.is_some() {
                                return Err(serde::de::Error::duplicate_field("timestamp"));
                            }
                            timestamp__ = map_.next_value()?;
                        }
                        GeneratedField::Name => {
                            if name__.is_some() {
                                return Err(serde::de::Error::duplicate_field("name"));
                            }
                            name__ = map_.next_value()?;
                        }
                        GeneratedField::Email => {
                            if email__.is_some() {
                                return Err(serde::de::Error::duplicate_field("email"));
                            }
                            email__ = map_.next_value()?;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(IdentifiableAction {
                    timestamp: timestamp__,
                    name: name__,
                    email: email__,
                })
            }
        }
        deserializer.deserialize_struct("cyclonedx.v1_4.IdentifiableAction", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for ImpactAnalysisJustification {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let variant = match self {
            Self::Null => "IMPACT_ANALYSIS_JUSTIFICATION_NULL",
            Self::CodeNotPresent => "IMPACT_ANALYSIS_JUSTIFICATION_CODE_NOT_PRESENT",
            Self::CodeNotReachable => "IMPACT_ANALYSIS_JUSTIFICATION_CODE_NOT_REACHABLE",
            Self::RequiresConfiguration => "IMPACT_ANALYSIS_JUSTIFICATION_REQUIRES_CONFIGURATION",
            Self::RequiresDependency => "IMPACT_ANALYSIS_JUSTIFICATION_REQUIRES_DEPENDENCY",
            Self::RequiresEnvironment => "IMPACT_ANALYSIS_JUSTIFICATION_REQUIRES_ENVIRONMENT",
            Self::ProtectedByCompiler => "IMPACT_ANALYSIS_JUSTIFICATION_PROTECTED_BY_COMPILER",
            Self::ProtectedAtRuntime => "IMPACT_ANALYSIS_JUSTIFICATION_PROTECTED_AT_RUNTIME",
            Self::ProtectedAtPerimeter => "IMPACT_ANALYSIS_JUSTIFICATION_PROTECTED_AT_PERIMETER",
            Self::ProtectedByMitigatingControl => "IMPACT_ANALYSIS_JUSTIFICATION_PROTECTED_BY_MITIGATING_CONTROL",
        };
        serializer.serialize_str(variant)
    }
}
impl<'de> serde::Deserialize<'de> for ImpactAnalysisJustification {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "IMPACT_ANALYSIS_JUSTIFICATION_NULL",
            "IMPACT_ANALYSIS_JUSTIFICATION_CODE_NOT_PRESENT",
            "IMPACT_ANALYSIS_JUSTIFICATION_CODE_NOT_REACHABLE",
            "IMPACT_ANALYSIS_JUSTIFICATION_REQUIRES_CONFIGURATION",
            "IMPACT_ANALYSIS_JUSTIFICATION_REQUIRES_DEPENDENCY",
            "IMPACT_ANALYSIS_JUSTIFICATION_REQUIRES_ENVIRONMENT",
            "IMPACT_ANALYSIS_JUSTIFICATION_PROTECTED_BY_COMPILER",
            "IMPACT_ANALYSIS_JUSTIFICATION_PROTECTED_AT_RUNTIME",
            "IMPACT_ANALYSIS_JUSTIFICATION_PROTECTED_AT_PERIMETER",
            "IMPACT_ANALYSIS_JUSTIFICATION_PROTECTED_BY_MITIGATING_CONTROL",
        ];

        struct GeneratedVisitor;

        impl serde::de::Visitor<'_> for GeneratedVisitor {
            type Value = ImpactAnalysisJustification;

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
                    "IMPACT_ANALYSIS_JUSTIFICATION_NULL" => Ok(ImpactAnalysisJustification::Null),
                    "IMPACT_ANALYSIS_JUSTIFICATION_CODE_NOT_PRESENT" => Ok(ImpactAnalysisJustification::CodeNotPresent),
                    "IMPACT_ANALYSIS_JUSTIFICATION_CODE_NOT_REACHABLE" => Ok(ImpactAnalysisJustification::CodeNotReachable),
                    "IMPACT_ANALYSIS_JUSTIFICATION_REQUIRES_CONFIGURATION" => Ok(ImpactAnalysisJustification::RequiresConfiguration),
                    "IMPACT_ANALYSIS_JUSTIFICATION_REQUIRES_DEPENDENCY" => Ok(ImpactAnalysisJustification::RequiresDependency),
                    "IMPACT_ANALYSIS_JUSTIFICATION_REQUIRES_ENVIRONMENT" => Ok(ImpactAnalysisJustification::RequiresEnvironment),
                    "IMPACT_ANALYSIS_JUSTIFICATION_PROTECTED_BY_COMPILER" => Ok(ImpactAnalysisJustification::ProtectedByCompiler),
                    "IMPACT_ANALYSIS_JUSTIFICATION_PROTECTED_AT_RUNTIME" => Ok(ImpactAnalysisJustification::ProtectedAtRuntime),
                    "IMPACT_ANALYSIS_JUSTIFICATION_PROTECTED_AT_PERIMETER" => Ok(ImpactAnalysisJustification::ProtectedAtPerimeter),
                    "IMPACT_ANALYSIS_JUSTIFICATION_PROTECTED_BY_MITIGATING_CONTROL" => Ok(ImpactAnalysisJustification::ProtectedByMitigatingControl),
                    _ => Err(serde::de::Error::unknown_variant(value, FIELDS)),
                }
            }
        }
        deserializer.deserialize_any(GeneratedVisitor)
    }
}
impl serde::Serialize for ImpactAnalysisState {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let variant = match self {
            Self::Null => "IMPACT_ANALYSIS_STATE_NULL",
            Self::Resolved => "IMPACT_ANALYSIS_STATE_RESOLVED",
            Self::ResolvedWithPedigree => "IMPACT_ANALYSIS_STATE_RESOLVED_WITH_PEDIGREE",
            Self::Exploitable => "IMPACT_ANALYSIS_STATE_EXPLOITABLE",
            Self::InTriage => "IMPACT_ANALYSIS_STATE_IN_TRIAGE",
            Self::FalsePositive => "IMPACT_ANALYSIS_STATE_FALSE_POSITIVE",
            Self::NotAffected => "IMPACT_ANALYSIS_STATE_NOT_AFFECTED",
        };
        serializer.serialize_str(variant)
    }
}
impl<'de> serde::Deserialize<'de> for ImpactAnalysisState {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "IMPACT_ANALYSIS_STATE_NULL",
            "IMPACT_ANALYSIS_STATE_RESOLVED",
            "IMPACT_ANALYSIS_STATE_RESOLVED_WITH_PEDIGREE",
            "IMPACT_ANALYSIS_STATE_EXPLOITABLE",
            "IMPACT_ANALYSIS_STATE_IN_TRIAGE",
            "IMPACT_ANALYSIS_STATE_FALSE_POSITIVE",
            "IMPACT_ANALYSIS_STATE_NOT_AFFECTED",
        ];

        struct GeneratedVisitor;

        impl serde::de::Visitor<'_> for GeneratedVisitor {
            type Value = ImpactAnalysisState;

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
                    "IMPACT_ANALYSIS_STATE_NULL" => Ok(ImpactAnalysisState::Null),
                    "IMPACT_ANALYSIS_STATE_RESOLVED" => Ok(ImpactAnalysisState::Resolved),
                    "IMPACT_ANALYSIS_STATE_RESOLVED_WITH_PEDIGREE" => Ok(ImpactAnalysisState::ResolvedWithPedigree),
                    "IMPACT_ANALYSIS_STATE_EXPLOITABLE" => Ok(ImpactAnalysisState::Exploitable),
                    "IMPACT_ANALYSIS_STATE_IN_TRIAGE" => Ok(ImpactAnalysisState::InTriage),
                    "IMPACT_ANALYSIS_STATE_FALSE_POSITIVE" => Ok(ImpactAnalysisState::FalsePositive),
                    "IMPACT_ANALYSIS_STATE_NOT_AFFECTED" => Ok(ImpactAnalysisState::NotAffected),
                    _ => Err(serde::de::Error::unknown_variant(value, FIELDS)),
                }
            }
        }
        deserializer.deserialize_any(GeneratedVisitor)
    }
}
impl serde::Serialize for Issue {
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
        if self.id.is_some() {
            len += 1;
        }
        if self.name.is_some() {
            len += 1;
        }
        if self.description.is_some() {
            len += 1;
        }
        if self.source.is_some() {
            len += 1;
        }
        if !self.references.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("cyclonedx.v1_4.Issue", len)?;
        if self.r#type != 0 {
            let v = IssueClassification::try_from(self.r#type)
                .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", self.r#type)))?;
            struct_ser.serialize_field("type", &v)?;
        }
        if let Some(v) = self.id.as_ref() {
            struct_ser.serialize_field("id", v)?;
        }
        if let Some(v) = self.name.as_ref() {
            struct_ser.serialize_field("name", v)?;
        }
        if let Some(v) = self.description.as_ref() {
            struct_ser.serialize_field("description", v)?;
        }
        if let Some(v) = self.source.as_ref() {
            struct_ser.serialize_field("source", v)?;
        }
        if !self.references.is_empty() {
            struct_ser.serialize_field("references", &self.references)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for Issue {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "type",
            "id",
            "name",
            "description",
            "source",
            "references",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Type,
            Id,
            Name,
            Description,
            Source,
            References,
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
                            "name" => Ok(GeneratedField::Name),
                            "description" => Ok(GeneratedField::Description),
                            "source" => Ok(GeneratedField::Source),
                            "references" => Ok(GeneratedField::References),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = Issue;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct cyclonedx.v1_4.Issue")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<Issue, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut r#type__ = None;
                let mut id__ = None;
                let mut name__ = None;
                let mut description__ = None;
                let mut source__ = None;
                let mut references__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Type => {
                            if r#type__.is_some() {
                                return Err(serde::de::Error::duplicate_field("type"));
                            }
                            r#type__ = Some(map_.next_value::<IssueClassification>()? as i32);
                        }
                        GeneratedField::Id => {
                            if id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("id"));
                            }
                            id__ = map_.next_value()?;
                        }
                        GeneratedField::Name => {
                            if name__.is_some() {
                                return Err(serde::de::Error::duplicate_field("name"));
                            }
                            name__ = map_.next_value()?;
                        }
                        GeneratedField::Description => {
                            if description__.is_some() {
                                return Err(serde::de::Error::duplicate_field("description"));
                            }
                            description__ = map_.next_value()?;
                        }
                        GeneratedField::Source => {
                            if source__.is_some() {
                                return Err(serde::de::Error::duplicate_field("source"));
                            }
                            source__ = map_.next_value()?;
                        }
                        GeneratedField::References => {
                            if references__.is_some() {
                                return Err(serde::de::Error::duplicate_field("references"));
                            }
                            references__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(Issue {
                    r#type: r#type__.unwrap_or_default(),
                    id: id__,
                    name: name__,
                    description: description__,
                    source: source__,
                    references: references__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("cyclonedx.v1_4.Issue", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for IssueClassification {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let variant = match self {
            Self::Null => "ISSUE_CLASSIFICATION_NULL",
            Self::Defect => "ISSUE_CLASSIFICATION_DEFECT",
            Self::Enhancement => "ISSUE_CLASSIFICATION_ENHANCEMENT",
            Self::Security => "ISSUE_CLASSIFICATION_SECURITY",
        };
        serializer.serialize_str(variant)
    }
}
impl<'de> serde::Deserialize<'de> for IssueClassification {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "ISSUE_CLASSIFICATION_NULL",
            "ISSUE_CLASSIFICATION_DEFECT",
            "ISSUE_CLASSIFICATION_ENHANCEMENT",
            "ISSUE_CLASSIFICATION_SECURITY",
        ];

        struct GeneratedVisitor;

        impl serde::de::Visitor<'_> for GeneratedVisitor {
            type Value = IssueClassification;

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
                    "ISSUE_CLASSIFICATION_NULL" => Ok(IssueClassification::Null),
                    "ISSUE_CLASSIFICATION_DEFECT" => Ok(IssueClassification::Defect),
                    "ISSUE_CLASSIFICATION_ENHANCEMENT" => Ok(IssueClassification::Enhancement),
                    "ISSUE_CLASSIFICATION_SECURITY" => Ok(IssueClassification::Security),
                    _ => Err(serde::de::Error::unknown_variant(value, FIELDS)),
                }
            }
        }
        deserializer.deserialize_any(GeneratedVisitor)
    }
}
impl serde::Serialize for License {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.text.is_some() {
            len += 1;
        }
        if self.url.is_some() {
            len += 1;
        }
        if self.license.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("cyclonedx.v1_4.License", len)?;
        if let Some(v) = self.text.as_ref() {
            struct_ser.serialize_field("text", v)?;
        }
        if let Some(v) = self.url.as_ref() {
            struct_ser.serialize_field("url", v)?;
        }
        if let Some(v) = self.license.as_ref() {
            match v {
                license::License::Id(v) => {
                    struct_ser.serialize_field("id", v)?;
                }
                license::License::Name(v) => {
                    struct_ser.serialize_field("name", v)?;
                }
            }
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for License {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "text",
            "url",
            "id",
            "name",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Text,
            Url,
            Id,
            Name,
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
                            "text" => Ok(GeneratedField::Text),
                            "url" => Ok(GeneratedField::Url),
                            "id" => Ok(GeneratedField::Id),
                            "name" => Ok(GeneratedField::Name),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = License;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct cyclonedx.v1_4.License")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<License, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut text__ = None;
                let mut url__ = None;
                let mut license__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Text => {
                            if text__.is_some() {
                                return Err(serde::de::Error::duplicate_field("text"));
                            }
                            text__ = map_.next_value()?;
                        }
                        GeneratedField::Url => {
                            if url__.is_some() {
                                return Err(serde::de::Error::duplicate_field("url"));
                            }
                            url__ = map_.next_value()?;
                        }
                        GeneratedField::Id => {
                            if license__.is_some() {
                                return Err(serde::de::Error::duplicate_field("id"));
                            }
                            license__ = map_.next_value::<::std::option::Option<_>>()?.map(license::License::Id);
                        }
                        GeneratedField::Name => {
                            if license__.is_some() {
                                return Err(serde::de::Error::duplicate_field("name"));
                            }
                            license__ = map_.next_value::<::std::option::Option<_>>()?.map(license::License::Name);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(License {
                    text: text__,
                    url: url__,
                    license: license__,
                })
            }
        }
        deserializer.deserialize_struct("cyclonedx.v1_4.License", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for LicenseChoice {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.choice.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("cyclonedx.v1_4.LicenseChoice", len)?;
        if let Some(v) = self.choice.as_ref() {
            match v {
                license_choice::Choice::License(v) => {
                    struct_ser.serialize_field("license", v)?;
                }
                license_choice::Choice::Expression(v) => {
                    struct_ser.serialize_field("expression", v)?;
                }
            }
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for LicenseChoice {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "license",
            "expression",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            License,
            Expression,
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
                            "license" => Ok(GeneratedField::License),
                            "expression" => Ok(GeneratedField::Expression),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = LicenseChoice;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct cyclonedx.v1_4.LicenseChoice")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<LicenseChoice, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut choice__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::License => {
                            if choice__.is_some() {
                                return Err(serde::de::Error::duplicate_field("license"));
                            }
                            choice__ = map_.next_value::<::std::option::Option<_>>()?.map(license_choice::Choice::License)
;
                        }
                        GeneratedField::Expression => {
                            if choice__.is_some() {
                                return Err(serde::de::Error::duplicate_field("expression"));
                            }
                            choice__ = map_.next_value::<::std::option::Option<_>>()?.map(license_choice::Choice::Expression);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(LicenseChoice {
                    choice: choice__,
                })
            }
        }
        deserializer.deserialize_struct("cyclonedx.v1_4.LicenseChoice", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for Metadata {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.timestamp.is_some() {
            len += 1;
        }
        if !self.tools.is_empty() {
            len += 1;
        }
        if !self.authors.is_empty() {
            len += 1;
        }
        if self.component.is_some() {
            len += 1;
        }
        if self.manufacture.is_some() {
            len += 1;
        }
        if self.supplier.is_some() {
            len += 1;
        }
        if self.licenses.is_some() {
            len += 1;
        }
        if !self.properties.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("cyclonedx.v1_4.Metadata", len)?;
        if let Some(v) = self.timestamp.as_ref() {
            struct_ser.serialize_field("timestamp", v)?;
        }
        if !self.tools.is_empty() {
            struct_ser.serialize_field("tools", &self.tools)?;
        }
        if !self.authors.is_empty() {
            struct_ser.serialize_field("authors", &self.authors)?;
        }
        if let Some(v) = self.component.as_ref() {
            struct_ser.serialize_field("component", v)?;
        }
        if let Some(v) = self.manufacture.as_ref() {
            struct_ser.serialize_field("manufacture", v)?;
        }
        if let Some(v) = self.supplier.as_ref() {
            struct_ser.serialize_field("supplier", v)?;
        }
        if let Some(v) = self.licenses.as_ref() {
            struct_ser.serialize_field("licenses", v)?;
        }
        if !self.properties.is_empty() {
            struct_ser.serialize_field("properties", &self.properties)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for Metadata {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "timestamp",
            "tools",
            "authors",
            "component",
            "manufacture",
            "supplier",
            "licenses",
            "properties",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Timestamp,
            Tools,
            Authors,
            Component,
            Manufacture,
            Supplier,
            Licenses,
            Properties,
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
                            "timestamp" => Ok(GeneratedField::Timestamp),
                            "tools" => Ok(GeneratedField::Tools),
                            "authors" => Ok(GeneratedField::Authors),
                            "component" => Ok(GeneratedField::Component),
                            "manufacture" => Ok(GeneratedField::Manufacture),
                            "supplier" => Ok(GeneratedField::Supplier),
                            "licenses" => Ok(GeneratedField::Licenses),
                            "properties" => Ok(GeneratedField::Properties),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = Metadata;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct cyclonedx.v1_4.Metadata")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<Metadata, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut timestamp__ = None;
                let mut tools__ = None;
                let mut authors__ = None;
                let mut component__ = None;
                let mut manufacture__ = None;
                let mut supplier__ = None;
                let mut licenses__ = None;
                let mut properties__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Timestamp => {
                            if timestamp__.is_some() {
                                return Err(serde::de::Error::duplicate_field("timestamp"));
                            }
                            timestamp__ = map_.next_value()?;
                        }
                        GeneratedField::Tools => {
                            if tools__.is_some() {
                                return Err(serde::de::Error::duplicate_field("tools"));
                            }
                            tools__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Authors => {
                            if authors__.is_some() {
                                return Err(serde::de::Error::duplicate_field("authors"));
                            }
                            authors__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Component => {
                            if component__.is_some() {
                                return Err(serde::de::Error::duplicate_field("component"));
                            }
                            component__ = map_.next_value()?;
                        }
                        GeneratedField::Manufacture => {
                            if manufacture__.is_some() {
                                return Err(serde::de::Error::duplicate_field("manufacture"));
                            }
                            manufacture__ = map_.next_value()?;
                        }
                        GeneratedField::Supplier => {
                            if supplier__.is_some() {
                                return Err(serde::de::Error::duplicate_field("supplier"));
                            }
                            supplier__ = map_.next_value()?;
                        }
                        GeneratedField::Licenses => {
                            if licenses__.is_some() {
                                return Err(serde::de::Error::duplicate_field("licenses"));
                            }
                            licenses__ = map_.next_value()?;
                        }
                        GeneratedField::Properties => {
                            if properties__.is_some() {
                                return Err(serde::de::Error::duplicate_field("properties"));
                            }
                            properties__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(Metadata {
                    timestamp: timestamp__,
                    tools: tools__.unwrap_or_default(),
                    authors: authors__.unwrap_or_default(),
                    component: component__,
                    manufacture: manufacture__,
                    supplier: supplier__,
                    licenses: licenses__,
                    properties: properties__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("cyclonedx.v1_4.Metadata", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for Note {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.locale.is_some() {
            len += 1;
        }
        if self.text.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("cyclonedx.v1_4.Note", len)?;
        if let Some(v) = self.locale.as_ref() {
            struct_ser.serialize_field("locale", v)?;
        }
        if let Some(v) = self.text.as_ref() {
            struct_ser.serialize_field("text", v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for Note {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "locale",
            "text",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Locale,
            Text,
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
                            "locale" => Ok(GeneratedField::Locale),
                            "text" => Ok(GeneratedField::Text),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = Note;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct cyclonedx.v1_4.Note")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<Note, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut locale__ = None;
                let mut text__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Locale => {
                            if locale__.is_some() {
                                return Err(serde::de::Error::duplicate_field("locale"));
                            }
                            locale__ = map_.next_value()?;
                        }
                        GeneratedField::Text => {
                            if text__.is_some() {
                                return Err(serde::de::Error::duplicate_field("text"));
                            }
                            text__ = map_.next_value()?;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(Note {
                    locale: locale__,
                    text: text__,
                })
            }
        }
        deserializer.deserialize_struct("cyclonedx.v1_4.Note", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for OrganizationalContact {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.name.is_some() {
            len += 1;
        }
        if self.email.is_some() {
            len += 1;
        }
        if self.phone.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("cyclonedx.v1_4.OrganizationalContact", len)?;
        if let Some(v) = self.name.as_ref() {
            struct_ser.serialize_field("name", v)?;
        }
        if let Some(v) = self.email.as_ref() {
            struct_ser.serialize_field("email", v)?;
        }
        if let Some(v) = self.phone.as_ref() {
            struct_ser.serialize_field("phone", v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for OrganizationalContact {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "name",
            "email",
            "phone",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Name,
            Email,
            Phone,
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
                            "email" => Ok(GeneratedField::Email),
                            "phone" => Ok(GeneratedField::Phone),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = OrganizationalContact;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct cyclonedx.v1_4.OrganizationalContact")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<OrganizationalContact, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut name__ = None;
                let mut email__ = None;
                let mut phone__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Name => {
                            if name__.is_some() {
                                return Err(serde::de::Error::duplicate_field("name"));
                            }
                            name__ = map_.next_value()?;
                        }
                        GeneratedField::Email => {
                            if email__.is_some() {
                                return Err(serde::de::Error::duplicate_field("email"));
                            }
                            email__ = map_.next_value()?;
                        }
                        GeneratedField::Phone => {
                            if phone__.is_some() {
                                return Err(serde::de::Error::duplicate_field("phone"));
                            }
                            phone__ = map_.next_value()?;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(OrganizationalContact {
                    name: name__,
                    email: email__,
                    phone: phone__,
                })
            }
        }
        deserializer.deserialize_struct("cyclonedx.v1_4.OrganizationalContact", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for OrganizationalEntity {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.name.is_some() {
            len += 1;
        }
        if !self.url.is_empty() {
            len += 1;
        }
        if !self.contact.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("cyclonedx.v1_4.OrganizationalEntity", len)?;
        if let Some(v) = self.name.as_ref() {
            struct_ser.serialize_field("name", v)?;
        }
        if !self.url.is_empty() {
            struct_ser.serialize_field("url", &self.url)?;
        }
        if !self.contact.is_empty() {
            struct_ser.serialize_field("contact", &self.contact)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for OrganizationalEntity {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "name",
            "url",
            "contact",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Name,
            Url,
            Contact,
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
                            "url" => Ok(GeneratedField::Url),
                            "contact" => Ok(GeneratedField::Contact),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = OrganizationalEntity;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct cyclonedx.v1_4.OrganizationalEntity")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<OrganizationalEntity, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut name__ = None;
                let mut url__ = None;
                let mut contact__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Name => {
                            if name__.is_some() {
                                return Err(serde::de::Error::duplicate_field("name"));
                            }
                            name__ = map_.next_value()?;
                        }
                        GeneratedField::Url => {
                            if url__.is_some() {
                                return Err(serde::de::Error::duplicate_field("url"));
                            }
                            url__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Contact => {
                            if contact__.is_some() {
                                return Err(serde::de::Error::duplicate_field("contact"));
                            }
                            contact__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(OrganizationalEntity {
                    name: name__,
                    url: url__.unwrap_or_default(),
                    contact: contact__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("cyclonedx.v1_4.OrganizationalEntity", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for Patch {
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
        if self.diff.is_some() {
            len += 1;
        }
        if !self.resolves.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("cyclonedx.v1_4.Patch", len)?;
        if self.r#type != 0 {
            let v = PatchClassification::try_from(self.r#type)
                .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", self.r#type)))?;
            struct_ser.serialize_field("type", &v)?;
        }
        if let Some(v) = self.diff.as_ref() {
            struct_ser.serialize_field("diff", v)?;
        }
        if !self.resolves.is_empty() {
            struct_ser.serialize_field("resolves", &self.resolves)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for Patch {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "type",
            "diff",
            "resolves",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Type,
            Diff,
            Resolves,
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
                            "diff" => Ok(GeneratedField::Diff),
                            "resolves" => Ok(GeneratedField::Resolves),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = Patch;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct cyclonedx.v1_4.Patch")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<Patch, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut r#type__ = None;
                let mut diff__ = None;
                let mut resolves__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Type => {
                            if r#type__.is_some() {
                                return Err(serde::de::Error::duplicate_field("type"));
                            }
                            r#type__ = Some(map_.next_value::<PatchClassification>()? as i32);
                        }
                        GeneratedField::Diff => {
                            if diff__.is_some() {
                                return Err(serde::de::Error::duplicate_field("diff"));
                            }
                            diff__ = map_.next_value()?;
                        }
                        GeneratedField::Resolves => {
                            if resolves__.is_some() {
                                return Err(serde::de::Error::duplicate_field("resolves"));
                            }
                            resolves__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(Patch {
                    r#type: r#type__.unwrap_or_default(),
                    diff: diff__,
                    resolves: resolves__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("cyclonedx.v1_4.Patch", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for PatchClassification {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let variant = match self {
            Self::Null => "PATCH_CLASSIFICATION_NULL",
            Self::Unofficial => "PATCH_CLASSIFICATION_UNOFFICIAL",
            Self::Monkey => "PATCH_CLASSIFICATION_MONKEY",
            Self::Backport => "PATCH_CLASSIFICATION_BACKPORT",
            Self::CherryPick => "PATCH_CLASSIFICATION_CHERRY_PICK",
        };
        serializer.serialize_str(variant)
    }
}
impl<'de> serde::Deserialize<'de> for PatchClassification {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "PATCH_CLASSIFICATION_NULL",
            "PATCH_CLASSIFICATION_UNOFFICIAL",
            "PATCH_CLASSIFICATION_MONKEY",
            "PATCH_CLASSIFICATION_BACKPORT",
            "PATCH_CLASSIFICATION_CHERRY_PICK",
        ];

        struct GeneratedVisitor;

        impl serde::de::Visitor<'_> for GeneratedVisitor {
            type Value = PatchClassification;

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
                    "PATCH_CLASSIFICATION_NULL" => Ok(PatchClassification::Null),
                    "PATCH_CLASSIFICATION_UNOFFICIAL" => Ok(PatchClassification::Unofficial),
                    "PATCH_CLASSIFICATION_MONKEY" => Ok(PatchClassification::Monkey),
                    "PATCH_CLASSIFICATION_BACKPORT" => Ok(PatchClassification::Backport),
                    "PATCH_CLASSIFICATION_CHERRY_PICK" => Ok(PatchClassification::CherryPick),
                    _ => Err(serde::de::Error::unknown_variant(value, FIELDS)),
                }
            }
        }
        deserializer.deserialize_any(GeneratedVisitor)
    }
}
impl serde::Serialize for Pedigree {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.ancestors.is_empty() {
            len += 1;
        }
        if !self.descendants.is_empty() {
            len += 1;
        }
        if !self.variants.is_empty() {
            len += 1;
        }
        if !self.commits.is_empty() {
            len += 1;
        }
        if !self.patches.is_empty() {
            len += 1;
        }
        if self.notes.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("cyclonedx.v1_4.Pedigree", len)?;
        if !self.ancestors.is_empty() {
            struct_ser.serialize_field("ancestors", &self.ancestors)?;
        }
        if !self.descendants.is_empty() {
            struct_ser.serialize_field("descendants", &self.descendants)?;
        }
        if !self.variants.is_empty() {
            struct_ser.serialize_field("variants", &self.variants)?;
        }
        if !self.commits.is_empty() {
            struct_ser.serialize_field("commits", &self.commits)?;
        }
        if !self.patches.is_empty() {
            struct_ser.serialize_field("patches", &self.patches)?;
        }
        if let Some(v) = self.notes.as_ref() {
            struct_ser.serialize_field("notes", v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for Pedigree {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "ancestors",
            "descendants",
            "variants",
            "commits",
            "patches",
            "notes",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Ancestors,
            Descendants,
            Variants,
            Commits,
            Patches,
            Notes,
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
                            "ancestors" => Ok(GeneratedField::Ancestors),
                            "descendants" => Ok(GeneratedField::Descendants),
                            "variants" => Ok(GeneratedField::Variants),
                            "commits" => Ok(GeneratedField::Commits),
                            "patches" => Ok(GeneratedField::Patches),
                            "notes" => Ok(GeneratedField::Notes),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = Pedigree;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct cyclonedx.v1_4.Pedigree")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<Pedigree, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut ancestors__ = None;
                let mut descendants__ = None;
                let mut variants__ = None;
                let mut commits__ = None;
                let mut patches__ = None;
                let mut notes__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Ancestors => {
                            if ancestors__.is_some() {
                                return Err(serde::de::Error::duplicate_field("ancestors"));
                            }
                            ancestors__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Descendants => {
                            if descendants__.is_some() {
                                return Err(serde::de::Error::duplicate_field("descendants"));
                            }
                            descendants__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Variants => {
                            if variants__.is_some() {
                                return Err(serde::de::Error::duplicate_field("variants"));
                            }
                            variants__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Commits => {
                            if commits__.is_some() {
                                return Err(serde::de::Error::duplicate_field("commits"));
                            }
                            commits__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Patches => {
                            if patches__.is_some() {
                                return Err(serde::de::Error::duplicate_field("patches"));
                            }
                            patches__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Notes => {
                            if notes__.is_some() {
                                return Err(serde::de::Error::duplicate_field("notes"));
                            }
                            notes__ = map_.next_value()?;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(Pedigree {
                    ancestors: ancestors__.unwrap_or_default(),
                    descendants: descendants__.unwrap_or_default(),
                    variants: variants__.unwrap_or_default(),
                    commits: commits__.unwrap_or_default(),
                    patches: patches__.unwrap_or_default(),
                    notes: notes__,
                })
            }
        }
        deserializer.deserialize_struct("cyclonedx.v1_4.Pedigree", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for Property {
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
        if self.value.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("cyclonedx.v1_4.Property", len)?;
        if !self.name.is_empty() {
            struct_ser.serialize_field("name", &self.name)?;
        }
        if let Some(v) = self.value.as_ref() {
            struct_ser.serialize_field("value", v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for Property {
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
            type Value = Property;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct cyclonedx.v1_4.Property")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<Property, V::Error>
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
                            value__ = map_.next_value()?;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(Property {
                    name: name__.unwrap_or_default(),
                    value: value__,
                })
            }
        }
        deserializer.deserialize_struct("cyclonedx.v1_4.Property", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for ReleaseNotes {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.r#type.is_empty() {
            len += 1;
        }
        if self.title.is_some() {
            len += 1;
        }
        if self.featured_image.is_some() {
            len += 1;
        }
        if self.social_image.is_some() {
            len += 1;
        }
        if self.description.is_some() {
            len += 1;
        }
        if self.timestamp.is_some() {
            len += 1;
        }
        if !self.aliases.is_empty() {
            len += 1;
        }
        if !self.tags.is_empty() {
            len += 1;
        }
        if !self.resolves.is_empty() {
            len += 1;
        }
        if !self.notes.is_empty() {
            len += 1;
        }
        if !self.properties.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("cyclonedx.v1_4.ReleaseNotes", len)?;
        if !self.r#type.is_empty() {
            struct_ser.serialize_field("type", &self.r#type)?;
        }
        if let Some(v) = self.title.as_ref() {
            struct_ser.serialize_field("title", v)?;
        }
        if let Some(v) = self.featured_image.as_ref() {
            struct_ser.serialize_field("featuredImage", v)?;
        }
        if let Some(v) = self.social_image.as_ref() {
            struct_ser.serialize_field("socialImage", v)?;
        }
        if let Some(v) = self.description.as_ref() {
            struct_ser.serialize_field("description", v)?;
        }
        if let Some(v) = self.timestamp.as_ref() {
            struct_ser.serialize_field("timestamp", v)?;
        }
        if !self.aliases.is_empty() {
            struct_ser.serialize_field("aliases", &self.aliases)?;
        }
        if !self.tags.is_empty() {
            struct_ser.serialize_field("tags", &self.tags)?;
        }
        if !self.resolves.is_empty() {
            struct_ser.serialize_field("resolves", &self.resolves)?;
        }
        if !self.notes.is_empty() {
            struct_ser.serialize_field("notes", &self.notes)?;
        }
        if !self.properties.is_empty() {
            struct_ser.serialize_field("properties", &self.properties)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for ReleaseNotes {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "type",
            "title",
            "featuredImage",
            "socialImage",
            "description",
            "timestamp",
            "aliases",
            "tags",
            "resolves",
            "notes",
            "properties",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Type,
            Title,
            FeaturedImage,
            SocialImage,
            Description,
            Timestamp,
            Aliases,
            Tags,
            Resolves,
            Notes,
            Properties,
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
                            "title" => Ok(GeneratedField::Title),
                            "featuredImage" => Ok(GeneratedField::FeaturedImage),
                            "socialImage" => Ok(GeneratedField::SocialImage),
                            "description" => Ok(GeneratedField::Description),
                            "timestamp" => Ok(GeneratedField::Timestamp),
                            "aliases" => Ok(GeneratedField::Aliases),
                            "tags" => Ok(GeneratedField::Tags),
                            "resolves" => Ok(GeneratedField::Resolves),
                            "notes" => Ok(GeneratedField::Notes),
                            "properties" => Ok(GeneratedField::Properties),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = ReleaseNotes;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct cyclonedx.v1_4.ReleaseNotes")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<ReleaseNotes, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut r#type__ = None;
                let mut title__ = None;
                let mut featured_image__ = None;
                let mut social_image__ = None;
                let mut description__ = None;
                let mut timestamp__ = None;
                let mut aliases__ = None;
                let mut tags__ = None;
                let mut resolves__ = None;
                let mut notes__ = None;
                let mut properties__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Type => {
                            if r#type__.is_some() {
                                return Err(serde::de::Error::duplicate_field("type"));
                            }
                            r#type__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Title => {
                            if title__.is_some() {
                                return Err(serde::de::Error::duplicate_field("title"));
                            }
                            title__ = map_.next_value()?;
                        }
                        GeneratedField::FeaturedImage => {
                            if featured_image__.is_some() {
                                return Err(serde::de::Error::duplicate_field("featuredImage"));
                            }
                            featured_image__ = map_.next_value()?;
                        }
                        GeneratedField::SocialImage => {
                            if social_image__.is_some() {
                                return Err(serde::de::Error::duplicate_field("socialImage"));
                            }
                            social_image__ = map_.next_value()?;
                        }
                        GeneratedField::Description => {
                            if description__.is_some() {
                                return Err(serde::de::Error::duplicate_field("description"));
                            }
                            description__ = map_.next_value()?;
                        }
                        GeneratedField::Timestamp => {
                            if timestamp__.is_some() {
                                return Err(serde::de::Error::duplicate_field("timestamp"));
                            }
                            timestamp__ = map_.next_value()?;
                        }
                        GeneratedField::Aliases => {
                            if aliases__.is_some() {
                                return Err(serde::de::Error::duplicate_field("aliases"));
                            }
                            aliases__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Tags => {
                            if tags__.is_some() {
                                return Err(serde::de::Error::duplicate_field("tags"));
                            }
                            tags__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Resolves => {
                            if resolves__.is_some() {
                                return Err(serde::de::Error::duplicate_field("resolves"));
                            }
                            resolves__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Notes => {
                            if notes__.is_some() {
                                return Err(serde::de::Error::duplicate_field("notes"));
                            }
                            notes__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Properties => {
                            if properties__.is_some() {
                                return Err(serde::de::Error::duplicate_field("properties"));
                            }
                            properties__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(ReleaseNotes {
                    r#type: r#type__.unwrap_or_default(),
                    title: title__,
                    featured_image: featured_image__,
                    social_image: social_image__,
                    description: description__,
                    timestamp: timestamp__,
                    aliases: aliases__.unwrap_or_default(),
                    tags: tags__.unwrap_or_default(),
                    resolves: resolves__.unwrap_or_default(),
                    notes: notes__.unwrap_or_default(),
                    properties: properties__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("cyclonedx.v1_4.ReleaseNotes", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for Scope {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let variant = match self {
            Self::Unspecified => "SCOPE_UNSPECIFIED",
            Self::Required => "SCOPE_REQUIRED",
            Self::Optional => "SCOPE_OPTIONAL",
            Self::Excluded => "SCOPE_EXCLUDED",
        };
        serializer.serialize_str(variant)
    }
}
impl<'de> serde::Deserialize<'de> for Scope {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "SCOPE_UNSPECIFIED",
            "SCOPE_REQUIRED",
            "SCOPE_OPTIONAL",
            "SCOPE_EXCLUDED",
        ];

        struct GeneratedVisitor;

        impl serde::de::Visitor<'_> for GeneratedVisitor {
            type Value = Scope;

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
                    "SCOPE_UNSPECIFIED" => Ok(Scope::Unspecified),
                    "SCOPE_REQUIRED" => Ok(Scope::Required),
                    "SCOPE_OPTIONAL" => Ok(Scope::Optional),
                    "SCOPE_EXCLUDED" => Ok(Scope::Excluded),
                    _ => Err(serde::de::Error::unknown_variant(value, FIELDS)),
                }
            }
        }
        deserializer.deserialize_any(GeneratedVisitor)
    }
}
impl serde::Serialize for ScoreMethod {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let variant = match self {
            Self::Null => "SCORE_METHOD_NULL",
            Self::Cvssv2 => "SCORE_METHOD_CVSSV2",
            Self::Cvssv3 => "SCORE_METHOD_CVSSV3",
            Self::Cvssv31 => "SCORE_METHOD_CVSSV31",
            Self::Owasp => "SCORE_METHOD_OWASP",
            Self::Other => "SCORE_METHOD_OTHER",
        };
        serializer.serialize_str(variant)
    }
}
impl<'de> serde::Deserialize<'de> for ScoreMethod {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "SCORE_METHOD_NULL",
            "SCORE_METHOD_CVSSV2",
            "SCORE_METHOD_CVSSV3",
            "SCORE_METHOD_CVSSV31",
            "SCORE_METHOD_OWASP",
            "SCORE_METHOD_OTHER",
        ];

        struct GeneratedVisitor;

        impl serde::de::Visitor<'_> for GeneratedVisitor {
            type Value = ScoreMethod;

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
                    "SCORE_METHOD_NULL" => Ok(ScoreMethod::Null),
                    "SCORE_METHOD_CVSSV2" => Ok(ScoreMethod::Cvssv2),
                    "SCORE_METHOD_CVSSV3" => Ok(ScoreMethod::Cvssv3),
                    "SCORE_METHOD_CVSSV31" => Ok(ScoreMethod::Cvssv31),
                    "SCORE_METHOD_OWASP" => Ok(ScoreMethod::Owasp),
                    "SCORE_METHOD_OTHER" => Ok(ScoreMethod::Other),
                    _ => Err(serde::de::Error::unknown_variant(value, FIELDS)),
                }
            }
        }
        deserializer.deserialize_any(GeneratedVisitor)
    }
}
impl serde::Serialize for Service {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.bom_ref.is_some() {
            len += 1;
        }
        if self.provider.is_some() {
            len += 1;
        }
        if self.group.is_some() {
            len += 1;
        }
        if !self.name.is_empty() {
            len += 1;
        }
        if self.version.is_some() {
            len += 1;
        }
        if self.description.is_some() {
            len += 1;
        }
        if !self.endpoints.is_empty() {
            len += 1;
        }
        if self.authenticated.is_some() {
            len += 1;
        }
        if self.x_trust_boundary.is_some() {
            len += 1;
        }
        if !self.data.is_empty() {
            len += 1;
        }
        if !self.licenses.is_empty() {
            len += 1;
        }
        if !self.external_references.is_empty() {
            len += 1;
        }
        if !self.services.is_empty() {
            len += 1;
        }
        if !self.properties.is_empty() {
            len += 1;
        }
        if self.release_notes.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("cyclonedx.v1_4.Service", len)?;
        if let Some(v) = self.bom_ref.as_ref() {
            struct_ser.serialize_field("bomRef", v)?;
        }
        if let Some(v) = self.provider.as_ref() {
            struct_ser.serialize_field("provider", v)?;
        }
        if let Some(v) = self.group.as_ref() {
            struct_ser.serialize_field("group", v)?;
        }
        if !self.name.is_empty() {
            struct_ser.serialize_field("name", &self.name)?;
        }
        if let Some(v) = self.version.as_ref() {
            struct_ser.serialize_field("version", v)?;
        }
        if let Some(v) = self.description.as_ref() {
            struct_ser.serialize_field("description", v)?;
        }
        if !self.endpoints.is_empty() {
            struct_ser.serialize_field("endpoints", &self.endpoints)?;
        }
        if let Some(v) = self.authenticated.as_ref() {
            struct_ser.serialize_field("authenticated", v)?;
        }
        if let Some(v) = self.x_trust_boundary.as_ref() {
            struct_ser.serialize_field("xTrustBoundary", v)?;
        }
        if !self.data.is_empty() {
            struct_ser.serialize_field("data", &self.data)?;
        }
        if !self.licenses.is_empty() {
            struct_ser.serialize_field("licenses", &self.licenses)?;
        }
        if !self.external_references.is_empty() {
            struct_ser.serialize_field("externalReferences", &self.external_references)?;
        }
        if !self.services.is_empty() {
            struct_ser.serialize_field("services", &self.services)?;
        }
        if !self.properties.is_empty() {
            struct_ser.serialize_field("properties", &self.properties)?;
        }
        if let Some(v) = self.release_notes.as_ref() {
            struct_ser.serialize_field("releaseNotes", v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for Service {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "bom_ref",
            "bomRef",
            "provider",
            "group",
            "name",
            "version",
            "description",
            "endpoints",
            "authenticated",
            "x_trust_boundary",
            "xTrustBoundary",
            "data",
            "licenses",
            "external_references",
            "externalReferences",
            "services",
            "properties",
            "releaseNotes",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            BomRef,
            Provider,
            Group,
            Name,
            Version,
            Description,
            Endpoints,
            Authenticated,
            XTrustBoundary,
            Data,
            Licenses,
            ExternalReferences,
            Services,
            Properties,
            ReleaseNotes,
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
                            "bomRef" | "bom_ref" => Ok(GeneratedField::BomRef),
                            "provider" => Ok(GeneratedField::Provider),
                            "group" => Ok(GeneratedField::Group),
                            "name" => Ok(GeneratedField::Name),
                            "version" => Ok(GeneratedField::Version),
                            "description" => Ok(GeneratedField::Description),
                            "endpoints" => Ok(GeneratedField::Endpoints),
                            "authenticated" => Ok(GeneratedField::Authenticated),
                            "xTrustBoundary" | "x_trust_boundary" => Ok(GeneratedField::XTrustBoundary),
                            "data" => Ok(GeneratedField::Data),
                            "licenses" => Ok(GeneratedField::Licenses),
                            "externalReferences" | "external_references" => Ok(GeneratedField::ExternalReferences),
                            "services" => Ok(GeneratedField::Services),
                            "properties" => Ok(GeneratedField::Properties),
                            "releaseNotes" => Ok(GeneratedField::ReleaseNotes),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = Service;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct cyclonedx.v1_4.Service")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<Service, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut bom_ref__ = None;
                let mut provider__ = None;
                let mut group__ = None;
                let mut name__ = None;
                let mut version__ = None;
                let mut description__ = None;
                let mut endpoints__ = None;
                let mut authenticated__ = None;
                let mut x_trust_boundary__ = None;
                let mut data__ = None;
                let mut licenses__ = None;
                let mut external_references__ = None;
                let mut services__ = None;
                let mut properties__ = None;
                let mut release_notes__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::BomRef => {
                            if bom_ref__.is_some() {
                                return Err(serde::de::Error::duplicate_field("bomRef"));
                            }
                            bom_ref__ = map_.next_value()?;
                        }
                        GeneratedField::Provider => {
                            if provider__.is_some() {
                                return Err(serde::de::Error::duplicate_field("provider"));
                            }
                            provider__ = map_.next_value()?;
                        }
                        GeneratedField::Group => {
                            if group__.is_some() {
                                return Err(serde::de::Error::duplicate_field("group"));
                            }
                            group__ = map_.next_value()?;
                        }
                        GeneratedField::Name => {
                            if name__.is_some() {
                                return Err(serde::de::Error::duplicate_field("name"));
                            }
                            name__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Version => {
                            if version__.is_some() {
                                return Err(serde::de::Error::duplicate_field("version"));
                            }
                            version__ = map_.next_value()?;
                        }
                        GeneratedField::Description => {
                            if description__.is_some() {
                                return Err(serde::de::Error::duplicate_field("description"));
                            }
                            description__ = map_.next_value()?;
                        }
                        GeneratedField::Endpoints => {
                            if endpoints__.is_some() {
                                return Err(serde::de::Error::duplicate_field("endpoints"));
                            }
                            endpoints__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Authenticated => {
                            if authenticated__.is_some() {
                                return Err(serde::de::Error::duplicate_field("authenticated"));
                            }
                            authenticated__ = map_.next_value()?;
                        }
                        GeneratedField::XTrustBoundary => {
                            if x_trust_boundary__.is_some() {
                                return Err(serde::de::Error::duplicate_field("xTrustBoundary"));
                            }
                            x_trust_boundary__ = map_.next_value()?;
                        }
                        GeneratedField::Data => {
                            if data__.is_some() {
                                return Err(serde::de::Error::duplicate_field("data"));
                            }
                            data__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Licenses => {
                            if licenses__.is_some() {
                                return Err(serde::de::Error::duplicate_field("licenses"));
                            }
                            licenses__ = Some(map_.next_value()?);
                        }
                        GeneratedField::ExternalReferences => {
                            if external_references__.is_some() {
                                return Err(serde::de::Error::duplicate_field("externalReferences"));
                            }
                            external_references__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Services => {
                            if services__.is_some() {
                                return Err(serde::de::Error::duplicate_field("services"));
                            }
                            services__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Properties => {
                            if properties__.is_some() {
                                return Err(serde::de::Error::duplicate_field("properties"));
                            }
                            properties__ = Some(map_.next_value()?);
                        }
                        GeneratedField::ReleaseNotes => {
                            if release_notes__.is_some() {
                                return Err(serde::de::Error::duplicate_field("releaseNotes"));
                            }
                            release_notes__ = map_.next_value()?;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(Service {
                    bom_ref: bom_ref__,
                    provider: provider__,
                    group: group__,
                    name: name__.unwrap_or_default(),
                    version: version__,
                    description: description__,
                    endpoints: endpoints__.unwrap_or_default(),
                    authenticated: authenticated__,
                    x_trust_boundary: x_trust_boundary__,
                    data: data__.unwrap_or_default(),
                    licenses: licenses__.unwrap_or_default(),
                    external_references: external_references__.unwrap_or_default(),
                    services: services__.unwrap_or_default(),
                    properties: properties__.unwrap_or_default(),
                    release_notes: release_notes__,
                })
            }
        }
        deserializer.deserialize_struct("cyclonedx.v1_4.Service", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for Severity {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let variant = match self {
            Self::Unknown => "SEVERITY_UNKNOWN",
            Self::Critical => "SEVERITY_CRITICAL",
            Self::High => "SEVERITY_HIGH",
            Self::Medium => "SEVERITY_MEDIUM",
            Self::Low => "SEVERITY_LOW",
            Self::Info => "SEVERITY_INFO",
            Self::None => "SEVERITY_NONE",
        };
        serializer.serialize_str(variant)
    }
}
impl<'de> serde::Deserialize<'de> for Severity {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "SEVERITY_UNKNOWN",
            "SEVERITY_CRITICAL",
            "SEVERITY_HIGH",
            "SEVERITY_MEDIUM",
            "SEVERITY_LOW",
            "SEVERITY_INFO",
            "SEVERITY_NONE",
        ];

        struct GeneratedVisitor;

        impl serde::de::Visitor<'_> for GeneratedVisitor {
            type Value = Severity;

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
                    "SEVERITY_UNKNOWN" => Ok(Severity::Unknown),
                    "SEVERITY_CRITICAL" => Ok(Severity::Critical),
                    "SEVERITY_HIGH" => Ok(Severity::High),
                    "SEVERITY_MEDIUM" => Ok(Severity::Medium),
                    "SEVERITY_LOW" => Ok(Severity::Low),
                    "SEVERITY_INFO" => Ok(Severity::Info),
                    "SEVERITY_NONE" => Ok(Severity::None),
                    _ => Err(serde::de::Error::unknown_variant(value, FIELDS)),
                }
            }
        }
        deserializer.deserialize_any(GeneratedVisitor)
    }
}
impl serde::Serialize for Source {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.name.is_some() {
            len += 1;
        }
        if self.url.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("cyclonedx.v1_4.Source", len)?;
        if let Some(v) = self.name.as_ref() {
            struct_ser.serialize_field("name", v)?;
        }
        if let Some(v) = self.url.as_ref() {
            struct_ser.serialize_field("url", v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for Source {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "name",
            "url",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Name,
            Url,
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
                            "url" => Ok(GeneratedField::Url),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = Source;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct cyclonedx.v1_4.Source")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<Source, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut name__ = None;
                let mut url__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Name => {
                            if name__.is_some() {
                                return Err(serde::de::Error::duplicate_field("name"));
                            }
                            name__ = map_.next_value()?;
                        }
                        GeneratedField::Url => {
                            if url__.is_some() {
                                return Err(serde::de::Error::duplicate_field("url"));
                            }
                            url__ = map_.next_value()?;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(Source {
                    name: name__,
                    url: url__,
                })
            }
        }
        deserializer.deserialize_struct("cyclonedx.v1_4.Source", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for Swid {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.tag_id.is_empty() {
            len += 1;
        }
        if !self.name.is_empty() {
            len += 1;
        }
        if self.version.is_some() {
            len += 1;
        }
        if self.tag_version.is_some() {
            len += 1;
        }
        if self.patch.is_some() {
            len += 1;
        }
        if self.text.is_some() {
            len += 1;
        }
        if self.url.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("cyclonedx.v1_4.Swid", len)?;
        if !self.tag_id.is_empty() {
            struct_ser.serialize_field("tagId", &self.tag_id)?;
        }
        if !self.name.is_empty() {
            struct_ser.serialize_field("name", &self.name)?;
        }
        if let Some(v) = self.version.as_ref() {
            struct_ser.serialize_field("version", v)?;
        }
        if let Some(v) = self.tag_version.as_ref() {
            struct_ser.serialize_field("tagVersion", v)?;
        }
        if let Some(v) = self.patch.as_ref() {
            struct_ser.serialize_field("patch", v)?;
        }
        if let Some(v) = self.text.as_ref() {
            struct_ser.serialize_field("text", v)?;
        }
        if let Some(v) = self.url.as_ref() {
            struct_ser.serialize_field("url", v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for Swid {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "tag_id",
            "tagId",
            "name",
            "version",
            "tag_version",
            "tagVersion",
            "patch",
            "text",
            "url",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            TagId,
            Name,
            Version,
            TagVersion,
            Patch,
            Text,
            Url,
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
                            "tagId" | "tag_id" => Ok(GeneratedField::TagId),
                            "name" => Ok(GeneratedField::Name),
                            "version" => Ok(GeneratedField::Version),
                            "tagVersion" | "tag_version" => Ok(GeneratedField::TagVersion),
                            "patch" => Ok(GeneratedField::Patch),
                            "text" => Ok(GeneratedField::Text),
                            "url" => Ok(GeneratedField::Url),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = Swid;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct cyclonedx.v1_4.Swid")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<Swid, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut tag_id__ = None;
                let mut name__ = None;
                let mut version__ = None;
                let mut tag_version__ = None;
                let mut patch__ = None;
                let mut text__ = None;
                let mut url__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::TagId => {
                            if tag_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("tagId"));
                            }
                            tag_id__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Name => {
                            if name__.is_some() {
                                return Err(serde::de::Error::duplicate_field("name"));
                            }
                            name__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Version => {
                            if version__.is_some() {
                                return Err(serde::de::Error::duplicate_field("version"));
                            }
                            version__ = map_.next_value()?;
                        }
                        GeneratedField::TagVersion => {
                            if tag_version__.is_some() {
                                return Err(serde::de::Error::duplicate_field("tagVersion"));
                            }
                            tag_version__ = 
                                map_.next_value::<::std::option::Option<::pbjson::private::NumberDeserialize<_>>>()?.map(|x| x.0)
                            ;
                        }
                        GeneratedField::Patch => {
                            if patch__.is_some() {
                                return Err(serde::de::Error::duplicate_field("patch"));
                            }
                            patch__ = map_.next_value()?;
                        }
                        GeneratedField::Text => {
                            if text__.is_some() {
                                return Err(serde::de::Error::duplicate_field("text"));
                            }
                            text__ = map_.next_value()?;
                        }
                        GeneratedField::Url => {
                            if url__.is_some() {
                                return Err(serde::de::Error::duplicate_field("url"));
                            }
                            url__ = map_.next_value()?;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(Swid {
                    tag_id: tag_id__.unwrap_or_default(),
                    name: name__.unwrap_or_default(),
                    version: version__,
                    tag_version: tag_version__,
                    patch: patch__,
                    text: text__,
                    url: url__,
                })
            }
        }
        deserializer.deserialize_struct("cyclonedx.v1_4.Swid", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for Tool {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.vendor.is_some() {
            len += 1;
        }
        if self.name.is_some() {
            len += 1;
        }
        if self.version.is_some() {
            len += 1;
        }
        if !self.hashes.is_empty() {
            len += 1;
        }
        if !self.external_references.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("cyclonedx.v1_4.Tool", len)?;
        if let Some(v) = self.vendor.as_ref() {
            struct_ser.serialize_field("vendor", v)?;
        }
        if let Some(v) = self.name.as_ref() {
            struct_ser.serialize_field("name", v)?;
        }
        if let Some(v) = self.version.as_ref() {
            struct_ser.serialize_field("version", v)?;
        }
        if !self.hashes.is_empty() {
            struct_ser.serialize_field("hashes", &self.hashes)?;
        }
        if !self.external_references.is_empty() {
            struct_ser.serialize_field("externalReferences", &self.external_references)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for Tool {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "vendor",
            "name",
            "version",
            "hashes",
            "external_references",
            "externalReferences",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Vendor,
            Name,
            Version,
            Hashes,
            ExternalReferences,
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
                            "vendor" => Ok(GeneratedField::Vendor),
                            "name" => Ok(GeneratedField::Name),
                            "version" => Ok(GeneratedField::Version),
                            "hashes" => Ok(GeneratedField::Hashes),
                            "externalReferences" | "external_references" => Ok(GeneratedField::ExternalReferences),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = Tool;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct cyclonedx.v1_4.Tool")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<Tool, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut vendor__ = None;
                let mut name__ = None;
                let mut version__ = None;
                let mut hashes__ = None;
                let mut external_references__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Vendor => {
                            if vendor__.is_some() {
                                return Err(serde::de::Error::duplicate_field("vendor"));
                            }
                            vendor__ = map_.next_value()?;
                        }
                        GeneratedField::Name => {
                            if name__.is_some() {
                                return Err(serde::de::Error::duplicate_field("name"));
                            }
                            name__ = map_.next_value()?;
                        }
                        GeneratedField::Version => {
                            if version__.is_some() {
                                return Err(serde::de::Error::duplicate_field("version"));
                            }
                            version__ = map_.next_value()?;
                        }
                        GeneratedField::Hashes => {
                            if hashes__.is_some() {
                                return Err(serde::de::Error::duplicate_field("hashes"));
                            }
                            hashes__ = Some(map_.next_value()?);
                        }
                        GeneratedField::ExternalReferences => {
                            if external_references__.is_some() {
                                return Err(serde::de::Error::duplicate_field("externalReferences"));
                            }
                            external_references__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(Tool {
                    vendor: vendor__,
                    name: name__,
                    version: version__,
                    hashes: hashes__.unwrap_or_default(),
                    external_references: external_references__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("cyclonedx.v1_4.Tool", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for Vulnerability {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.bom_ref.is_some() {
            len += 1;
        }
        if self.id.is_some() {
            len += 1;
        }
        if self.source.is_some() {
            len += 1;
        }
        if !self.references.is_empty() {
            len += 1;
        }
        if !self.ratings.is_empty() {
            len += 1;
        }
        if !self.cwes.is_empty() {
            len += 1;
        }
        if self.description.is_some() {
            len += 1;
        }
        if self.detail.is_some() {
            len += 1;
        }
        if self.recommendation.is_some() {
            len += 1;
        }
        if !self.advisories.is_empty() {
            len += 1;
        }
        if self.created.is_some() {
            len += 1;
        }
        if self.published.is_some() {
            len += 1;
        }
        if self.updated.is_some() {
            len += 1;
        }
        if self.credits.is_some() {
            len += 1;
        }
        if !self.tools.is_empty() {
            len += 1;
        }
        if self.analysis.is_some() {
            len += 1;
        }
        if !self.affects.is_empty() {
            len += 1;
        }
        if !self.properties.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("cyclonedx.v1_4.Vulnerability", len)?;
        if let Some(v) = self.bom_ref.as_ref() {
            struct_ser.serialize_field("bomRef", v)?;
        }
        if let Some(v) = self.id.as_ref() {
            struct_ser.serialize_field("id", v)?;
        }
        if let Some(v) = self.source.as_ref() {
            struct_ser.serialize_field("source", v)?;
        }
        if !self.references.is_empty() {
            struct_ser.serialize_field("references", &self.references)?;
        }
        if !self.ratings.is_empty() {
            struct_ser.serialize_field("ratings", &self.ratings)?;
        }
        if !self.cwes.is_empty() {
            struct_ser.serialize_field("cwes", &self.cwes)?;
        }
        if let Some(v) = self.description.as_ref() {
            struct_ser.serialize_field("description", v)?;
        }
        if let Some(v) = self.detail.as_ref() {
            struct_ser.serialize_field("detail", v)?;
        }
        if let Some(v) = self.recommendation.as_ref() {
            struct_ser.serialize_field("recommendation", v)?;
        }
        if !self.advisories.is_empty() {
            struct_ser.serialize_field("advisories", &self.advisories)?;
        }
        if let Some(v) = self.created.as_ref() {
            struct_ser.serialize_field("created", v)?;
        }
        if let Some(v) = self.published.as_ref() {
            struct_ser.serialize_field("published", v)?;
        }
        if let Some(v) = self.updated.as_ref() {
            struct_ser.serialize_field("updated", v)?;
        }
        if let Some(v) = self.credits.as_ref() {
            struct_ser.serialize_field("credits", v)?;
        }
        if !self.tools.is_empty() {
            struct_ser.serialize_field("tools", &self.tools)?;
        }
        if let Some(v) = self.analysis.as_ref() {
            struct_ser.serialize_field("analysis", v)?;
        }
        if !self.affects.is_empty() {
            struct_ser.serialize_field("affects", &self.affects)?;
        }
        if !self.properties.is_empty() {
            struct_ser.serialize_field("properties", &self.properties)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for Vulnerability {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "bom_ref",
            "bomRef",
            "id",
            "source",
            "references",
            "ratings",
            "cwes",
            "description",
            "detail",
            "recommendation",
            "advisories",
            "created",
            "published",
            "updated",
            "credits",
            "tools",
            "analysis",
            "affects",
            "properties",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            BomRef,
            Id,
            Source,
            References,
            Ratings,
            Cwes,
            Description,
            Detail,
            Recommendation,
            Advisories,
            Created,
            Published,
            Updated,
            Credits,
            Tools,
            Analysis,
            Affects,
            Properties,
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
                            "bomRef" | "bom_ref" => Ok(GeneratedField::BomRef),
                            "id" => Ok(GeneratedField::Id),
                            "source" => Ok(GeneratedField::Source),
                            "references" => Ok(GeneratedField::References),
                            "ratings" => Ok(GeneratedField::Ratings),
                            "cwes" => Ok(GeneratedField::Cwes),
                            "description" => Ok(GeneratedField::Description),
                            "detail" => Ok(GeneratedField::Detail),
                            "recommendation" => Ok(GeneratedField::Recommendation),
                            "advisories" => Ok(GeneratedField::Advisories),
                            "created" => Ok(GeneratedField::Created),
                            "published" => Ok(GeneratedField::Published),
                            "updated" => Ok(GeneratedField::Updated),
                            "credits" => Ok(GeneratedField::Credits),
                            "tools" => Ok(GeneratedField::Tools),
                            "analysis" => Ok(GeneratedField::Analysis),
                            "affects" => Ok(GeneratedField::Affects),
                            "properties" => Ok(GeneratedField::Properties),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = Vulnerability;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct cyclonedx.v1_4.Vulnerability")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<Vulnerability, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut bom_ref__ = None;
                let mut id__ = None;
                let mut source__ = None;
                let mut references__ = None;
                let mut ratings__ = None;
                let mut cwes__ = None;
                let mut description__ = None;
                let mut detail__ = None;
                let mut recommendation__ = None;
                let mut advisories__ = None;
                let mut created__ = None;
                let mut published__ = None;
                let mut updated__ = None;
                let mut credits__ = None;
                let mut tools__ = None;
                let mut analysis__ = None;
                let mut affects__ = None;
                let mut properties__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::BomRef => {
                            if bom_ref__.is_some() {
                                return Err(serde::de::Error::duplicate_field("bomRef"));
                            }
                            bom_ref__ = map_.next_value()?;
                        }
                        GeneratedField::Id => {
                            if id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("id"));
                            }
                            id__ = map_.next_value()?;
                        }
                        GeneratedField::Source => {
                            if source__.is_some() {
                                return Err(serde::de::Error::duplicate_field("source"));
                            }
                            source__ = map_.next_value()?;
                        }
                        GeneratedField::References => {
                            if references__.is_some() {
                                return Err(serde::de::Error::duplicate_field("references"));
                            }
                            references__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Ratings => {
                            if ratings__.is_some() {
                                return Err(serde::de::Error::duplicate_field("ratings"));
                            }
                            ratings__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Cwes => {
                            if cwes__.is_some() {
                                return Err(serde::de::Error::duplicate_field("cwes"));
                            }
                            cwes__ = 
                                Some(map_.next_value::<Vec<::pbjson::private::NumberDeserialize<_>>>()?
                                    .into_iter().map(|x| x.0).collect())
                            ;
                        }
                        GeneratedField::Description => {
                            if description__.is_some() {
                                return Err(serde::de::Error::duplicate_field("description"));
                            }
                            description__ = map_.next_value()?;
                        }
                        GeneratedField::Detail => {
                            if detail__.is_some() {
                                return Err(serde::de::Error::duplicate_field("detail"));
                            }
                            detail__ = map_.next_value()?;
                        }
                        GeneratedField::Recommendation => {
                            if recommendation__.is_some() {
                                return Err(serde::de::Error::duplicate_field("recommendation"));
                            }
                            recommendation__ = map_.next_value()?;
                        }
                        GeneratedField::Advisories => {
                            if advisories__.is_some() {
                                return Err(serde::de::Error::duplicate_field("advisories"));
                            }
                            advisories__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Created => {
                            if created__.is_some() {
                                return Err(serde::de::Error::duplicate_field("created"));
                            }
                            created__ = map_.next_value()?;
                        }
                        GeneratedField::Published => {
                            if published__.is_some() {
                                return Err(serde::de::Error::duplicate_field("published"));
                            }
                            published__ = map_.next_value()?;
                        }
                        GeneratedField::Updated => {
                            if updated__.is_some() {
                                return Err(serde::de::Error::duplicate_field("updated"));
                            }
                            updated__ = map_.next_value()?;
                        }
                        GeneratedField::Credits => {
                            if credits__.is_some() {
                                return Err(serde::de::Error::duplicate_field("credits"));
                            }
                            credits__ = map_.next_value()?;
                        }
                        GeneratedField::Tools => {
                            if tools__.is_some() {
                                return Err(serde::de::Error::duplicate_field("tools"));
                            }
                            tools__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Analysis => {
                            if analysis__.is_some() {
                                return Err(serde::de::Error::duplicate_field("analysis"));
                            }
                            analysis__ = map_.next_value()?;
                        }
                        GeneratedField::Affects => {
                            if affects__.is_some() {
                                return Err(serde::de::Error::duplicate_field("affects"));
                            }
                            affects__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Properties => {
                            if properties__.is_some() {
                                return Err(serde::de::Error::duplicate_field("properties"));
                            }
                            properties__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(Vulnerability {
                    bom_ref: bom_ref__,
                    id: id__,
                    source: source__,
                    references: references__.unwrap_or_default(),
                    ratings: ratings__.unwrap_or_default(),
                    cwes: cwes__.unwrap_or_default(),
                    description: description__,
                    detail: detail__,
                    recommendation: recommendation__,
                    advisories: advisories__.unwrap_or_default(),
                    created: created__,
                    published: published__,
                    updated: updated__,
                    credits: credits__,
                    tools: tools__.unwrap_or_default(),
                    analysis: analysis__,
                    affects: affects__.unwrap_or_default(),
                    properties: properties__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("cyclonedx.v1_4.Vulnerability", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for VulnerabilityAffectedStatus {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let variant = match self {
            Self::Unknown => "VULNERABILITY_AFFECTED_STATUS_UNKNOWN",
            Self::Affected => "VULNERABILITY_AFFECTED_STATUS_AFFECTED",
            Self::NotAffected => "VULNERABILITY_AFFECTED_STATUS_NOT_AFFECTED",
        };
        serializer.serialize_str(variant)
    }
}
impl<'de> serde::Deserialize<'de> for VulnerabilityAffectedStatus {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "VULNERABILITY_AFFECTED_STATUS_UNKNOWN",
            "VULNERABILITY_AFFECTED_STATUS_AFFECTED",
            "VULNERABILITY_AFFECTED_STATUS_NOT_AFFECTED",
        ];

        struct GeneratedVisitor;

        impl serde::de::Visitor<'_> for GeneratedVisitor {
            type Value = VulnerabilityAffectedStatus;

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
                    "VULNERABILITY_AFFECTED_STATUS_UNKNOWN" => Ok(VulnerabilityAffectedStatus::Unknown),
                    "VULNERABILITY_AFFECTED_STATUS_AFFECTED" => Ok(VulnerabilityAffectedStatus::Affected),
                    "VULNERABILITY_AFFECTED_STATUS_NOT_AFFECTED" => Ok(VulnerabilityAffectedStatus::NotAffected),
                    _ => Err(serde::de::Error::unknown_variant(value, FIELDS)),
                }
            }
        }
        deserializer.deserialize_any(GeneratedVisitor)
    }
}
impl serde::Serialize for VulnerabilityAffectedVersions {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.status.is_some() {
            len += 1;
        }
        if self.choice.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("cyclonedx.v1_4.VulnerabilityAffectedVersions", len)?;
        if let Some(v) = self.status.as_ref() {
            let v = VulnerabilityAffectedStatus::try_from(*v)
                .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", *v)))?;
            struct_ser.serialize_field("status", &v)?;
        }
        if let Some(v) = self.choice.as_ref() {
            match v {
                vulnerability_affected_versions::Choice::Version(v) => {
                    struct_ser.serialize_field("version", v)?;
                }
                vulnerability_affected_versions::Choice::Range(v) => {
                    struct_ser.serialize_field("range", v)?;
                }
            }
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for VulnerabilityAffectedVersions {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "status",
            "version",
            "range",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Status,
            Version,
            Range,
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
                            "status" => Ok(GeneratedField::Status),
                            "version" => Ok(GeneratedField::Version),
                            "range" => Ok(GeneratedField::Range),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = VulnerabilityAffectedVersions;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct cyclonedx.v1_4.VulnerabilityAffectedVersions")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<VulnerabilityAffectedVersions, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut status__ = None;
                let mut choice__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Status => {
                            if status__.is_some() {
                                return Err(serde::de::Error::duplicate_field("status"));
                            }
                            status__ = map_.next_value::<::std::option::Option<VulnerabilityAffectedStatus>>()?.map(|x| x as i32);
                        }
                        GeneratedField::Version => {
                            if choice__.is_some() {
                                return Err(serde::de::Error::duplicate_field("version"));
                            }
                            choice__ = map_.next_value::<::std::option::Option<_>>()?.map(vulnerability_affected_versions::Choice::Version);
                        }
                        GeneratedField::Range => {
                            if choice__.is_some() {
                                return Err(serde::de::Error::duplicate_field("range"));
                            }
                            choice__ = map_.next_value::<::std::option::Option<_>>()?.map(vulnerability_affected_versions::Choice::Range);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(VulnerabilityAffectedVersions {
                    status: status__,
                    choice: choice__,
                })
            }
        }
        deserializer.deserialize_struct("cyclonedx.v1_4.VulnerabilityAffectedVersions", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for VulnerabilityAffects {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.r#ref.is_empty() {
            len += 1;
        }
        if !self.versions.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("cyclonedx.v1_4.VulnerabilityAffects", len)?;
        if !self.r#ref.is_empty() {
            struct_ser.serialize_field("ref", &self.r#ref)?;
        }
        if !self.versions.is_empty() {
            struct_ser.serialize_field("versions", &self.versions)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for VulnerabilityAffects {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "ref",
            "versions",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Ref,
            Versions,
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
                            "ref" => Ok(GeneratedField::Ref),
                            "versions" => Ok(GeneratedField::Versions),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = VulnerabilityAffects;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct cyclonedx.v1_4.VulnerabilityAffects")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<VulnerabilityAffects, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut r#ref__ = None;
                let mut versions__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Ref => {
                            if r#ref__.is_some() {
                                return Err(serde::de::Error::duplicate_field("ref"));
                            }
                            r#ref__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Versions => {
                            if versions__.is_some() {
                                return Err(serde::de::Error::duplicate_field("versions"));
                            }
                            versions__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(VulnerabilityAffects {
                    r#ref: r#ref__.unwrap_or_default(),
                    versions: versions__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("cyclonedx.v1_4.VulnerabilityAffects", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for VulnerabilityAnalysis {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.state.is_some() {
            len += 1;
        }
        if self.justification.is_some() {
            len += 1;
        }
        if !self.response.is_empty() {
            len += 1;
        }
        if self.detail.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("cyclonedx.v1_4.VulnerabilityAnalysis", len)?;
        if let Some(v) = self.state.as_ref() {
            let v = ImpactAnalysisState::try_from(*v)
                .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", *v)))?;
            struct_ser.serialize_field("state", &v)?;
        }
        if let Some(v) = self.justification.as_ref() {
            let v = ImpactAnalysisJustification::try_from(*v)
                .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", *v)))?;
            struct_ser.serialize_field("justification", &v)?;
        }
        if !self.response.is_empty() {
            let v = self.response.iter().cloned().map(|v| {
                VulnerabilityResponse::try_from(v)
                    .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", v)))
                }).collect::<std::result::Result<Vec<_>, _>>()?;
            struct_ser.serialize_field("response", &v)?;
        }
        if let Some(v) = self.detail.as_ref() {
            struct_ser.serialize_field("detail", v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for VulnerabilityAnalysis {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "state",
            "justification",
            "response",
            "detail",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            State,
            Justification,
            Response,
            Detail,
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
                            "state" => Ok(GeneratedField::State),
                            "justification" => Ok(GeneratedField::Justification),
                            "response" => Ok(GeneratedField::Response),
                            "detail" => Ok(GeneratedField::Detail),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = VulnerabilityAnalysis;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct cyclonedx.v1_4.VulnerabilityAnalysis")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<VulnerabilityAnalysis, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut state__ = None;
                let mut justification__ = None;
                let mut response__ = None;
                let mut detail__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::State => {
                            if state__.is_some() {
                                return Err(serde::de::Error::duplicate_field("state"));
                            }
                            state__ = map_.next_value::<::std::option::Option<ImpactAnalysisState>>()?.map(|x| x as i32);
                        }
                        GeneratedField::Justification => {
                            if justification__.is_some() {
                                return Err(serde::de::Error::duplicate_field("justification"));
                            }
                            justification__ = map_.next_value::<::std::option::Option<ImpactAnalysisJustification>>()?.map(|x| x as i32);
                        }
                        GeneratedField::Response => {
                            if response__.is_some() {
                                return Err(serde::de::Error::duplicate_field("response"));
                            }
                            response__ = Some(map_.next_value::<Vec<VulnerabilityResponse>>()?.into_iter().map(|x| x as i32).collect());
                        }
                        GeneratedField::Detail => {
                            if detail__.is_some() {
                                return Err(serde::de::Error::duplicate_field("detail"));
                            }
                            detail__ = map_.next_value()?;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(VulnerabilityAnalysis {
                    state: state__,
                    justification: justification__,
                    response: response__.unwrap_or_default(),
                    detail: detail__,
                })
            }
        }
        deserializer.deserialize_struct("cyclonedx.v1_4.VulnerabilityAnalysis", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for VulnerabilityCredits {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.organizations.is_empty() {
            len += 1;
        }
        if !self.individuals.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("cyclonedx.v1_4.VulnerabilityCredits", len)?;
        if !self.organizations.is_empty() {
            struct_ser.serialize_field("organizations", &self.organizations)?;
        }
        if !self.individuals.is_empty() {
            struct_ser.serialize_field("individuals", &self.individuals)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for VulnerabilityCredits {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "organizations",
            "individuals",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Organizations,
            Individuals,
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
                            "organizations" => Ok(GeneratedField::Organizations),
                            "individuals" => Ok(GeneratedField::Individuals),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = VulnerabilityCredits;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct cyclonedx.v1_4.VulnerabilityCredits")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<VulnerabilityCredits, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut organizations__ = None;
                let mut individuals__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Organizations => {
                            if organizations__.is_some() {
                                return Err(serde::de::Error::duplicate_field("organizations"));
                            }
                            organizations__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Individuals => {
                            if individuals__.is_some() {
                                return Err(serde::de::Error::duplicate_field("individuals"));
                            }
                            individuals__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(VulnerabilityCredits {
                    organizations: organizations__.unwrap_or_default(),
                    individuals: individuals__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("cyclonedx.v1_4.VulnerabilityCredits", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for VulnerabilityRating {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.source.is_some() {
            len += 1;
        }
        if self.score.is_some() {
            len += 1;
        }
        if self.severity.is_some() {
            len += 1;
        }
        if self.method.is_some() {
            len += 1;
        }
        if self.vector.is_some() {
            len += 1;
        }
        if self.justification.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("cyclonedx.v1_4.VulnerabilityRating", len)?;
        if let Some(v) = self.source.as_ref() {
            struct_ser.serialize_field("source", v)?;
        }
        if let Some(v) = self.score.as_ref() {
            struct_ser.serialize_field("score", v)?;
        }
        if let Some(v) = self.severity.as_ref() {
            let v = Severity::try_from(*v)
                .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", *v)))?;
            struct_ser.serialize_field("severity", &v)?;
        }
        if let Some(v) = self.method.as_ref() {
            let v = ScoreMethod::try_from(*v)
                .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", *v)))?;
            struct_ser.serialize_field("method", &v)?;
        }
        if let Some(v) = self.vector.as_ref() {
            struct_ser.serialize_field("vector", v)?;
        }
        if let Some(v) = self.justification.as_ref() {
            struct_ser.serialize_field("justification", v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for VulnerabilityRating {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "source",
            "score",
            "severity",
            "method",
            "vector",
            "justification",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Source,
            Score,
            Severity,
            Method,
            Vector,
            Justification,
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
                            "source" => Ok(GeneratedField::Source),
                            "score" => Ok(GeneratedField::Score),
                            "severity" => Ok(GeneratedField::Severity),
                            "method" => Ok(GeneratedField::Method),
                            "vector" => Ok(GeneratedField::Vector),
                            "justification" => Ok(GeneratedField::Justification),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = VulnerabilityRating;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct cyclonedx.v1_4.VulnerabilityRating")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<VulnerabilityRating, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut source__ = None;
                let mut score__ = None;
                let mut severity__ = None;
                let mut method__ = None;
                let mut vector__ = None;
                let mut justification__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Source => {
                            if source__.is_some() {
                                return Err(serde::de::Error::duplicate_field("source"));
                            }
                            source__ = map_.next_value()?;
                        }
                        GeneratedField::Score => {
                            if score__.is_some() {
                                return Err(serde::de::Error::duplicate_field("score"));
                            }
                            score__ = 
                                map_.next_value::<::std::option::Option<::pbjson::private::NumberDeserialize<_>>>()?.map(|x| x.0)
                            ;
                        }
                        GeneratedField::Severity => {
                            if severity__.is_some() {
                                return Err(serde::de::Error::duplicate_field("severity"));
                            }
                            severity__ = map_.next_value::<::std::option::Option<Severity>>()?.map(|x| x as i32);
                        }
                        GeneratedField::Method => {
                            if method__.is_some() {
                                return Err(serde::de::Error::duplicate_field("method"));
                            }
                            method__ = map_.next_value::<::std::option::Option<ScoreMethod>>()?.map(|x| x as i32);
                        }
                        GeneratedField::Vector => {
                            if vector__.is_some() {
                                return Err(serde::de::Error::duplicate_field("vector"));
                            }
                            vector__ = map_.next_value()?;
                        }
                        GeneratedField::Justification => {
                            if justification__.is_some() {
                                return Err(serde::de::Error::duplicate_field("justification"));
                            }
                            justification__ = map_.next_value()?;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(VulnerabilityRating {
                    source: source__,
                    score: score__,
                    severity: severity__,
                    method: method__,
                    vector: vector__,
                    justification: justification__,
                })
            }
        }
        deserializer.deserialize_struct("cyclonedx.v1_4.VulnerabilityRating", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for VulnerabilityReference {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.id.is_some() {
            len += 1;
        }
        if self.source.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("cyclonedx.v1_4.VulnerabilityReference", len)?;
        if let Some(v) = self.id.as_ref() {
            struct_ser.serialize_field("id", v)?;
        }
        if let Some(v) = self.source.as_ref() {
            struct_ser.serialize_field("source", v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for VulnerabilityReference {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "id",
            "source",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Id,
            Source,
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
                            "id" => Ok(GeneratedField::Id),
                            "source" => Ok(GeneratedField::Source),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = VulnerabilityReference;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct cyclonedx.v1_4.VulnerabilityReference")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<VulnerabilityReference, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut id__ = None;
                let mut source__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Id => {
                            if id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("id"));
                            }
                            id__ = map_.next_value()?;
                        }
                        GeneratedField::Source => {
                            if source__.is_some() {
                                return Err(serde::de::Error::duplicate_field("source"));
                            }
                            source__ = map_.next_value()?;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(VulnerabilityReference {
                    id: id__,
                    source: source__,
                })
            }
        }
        deserializer.deserialize_struct("cyclonedx.v1_4.VulnerabilityReference", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for VulnerabilityResponse {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let variant = match self {
            Self::Null => "VULNERABILITY_RESPONSE_NULL",
            Self::CanNotFix => "VULNERABILITY_RESPONSE_CAN_NOT_FIX",
            Self::WillNotFix => "VULNERABILITY_RESPONSE_WILL_NOT_FIX",
            Self::Update => "VULNERABILITY_RESPONSE_UPDATE",
            Self::Rollback => "VULNERABILITY_RESPONSE_ROLLBACK",
            Self::WorkaroundAvailable => "VULNERABILITY_RESPONSE_WORKAROUND_AVAILABLE",
        };
        serializer.serialize_str(variant)
    }
}
impl<'de> serde::Deserialize<'de> for VulnerabilityResponse {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "VULNERABILITY_RESPONSE_NULL",
            "VULNERABILITY_RESPONSE_CAN_NOT_FIX",
            "VULNERABILITY_RESPONSE_WILL_NOT_FIX",
            "VULNERABILITY_RESPONSE_UPDATE",
            "VULNERABILITY_RESPONSE_ROLLBACK",
            "VULNERABILITY_RESPONSE_WORKAROUND_AVAILABLE",
        ];

        struct GeneratedVisitor;

        impl serde::de::Visitor<'_> for GeneratedVisitor {
            type Value = VulnerabilityResponse;

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
                    "VULNERABILITY_RESPONSE_NULL" => Ok(VulnerabilityResponse::Null),
                    "VULNERABILITY_RESPONSE_CAN_NOT_FIX" => Ok(VulnerabilityResponse::CanNotFix),
                    "VULNERABILITY_RESPONSE_WILL_NOT_FIX" => Ok(VulnerabilityResponse::WillNotFix),
                    "VULNERABILITY_RESPONSE_UPDATE" => Ok(VulnerabilityResponse::Update),
                    "VULNERABILITY_RESPONSE_ROLLBACK" => Ok(VulnerabilityResponse::Rollback),
                    "VULNERABILITY_RESPONSE_WORKAROUND_AVAILABLE" => Ok(VulnerabilityResponse::WorkaroundAvailable),
                    _ => Err(serde::de::Error::unknown_variant(value, FIELDS)),
                }
            }
        }
        deserializer.deserialize_any(GeneratedVisitor)
    }
}
