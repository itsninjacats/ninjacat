impl serde::Serialize for ContainerImage {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.id.is_empty() {
            len += 1;
        }
        if !self.dd_tags.is_empty() {
            len += 1;
        }
        if !self.name.is_empty() {
            len += 1;
        }
        if !self.registry.is_empty() {
            len += 1;
        }
        if !self.short_name.is_empty() {
            len += 1;
        }
        if !self.repo_tags.is_empty() {
            len += 1;
        }
        if !self.digest.is_empty() {
            len += 1;
        }
        if self.size != 0 {
            len += 1;
        }
        if !self.repo_digests.is_empty() {
            len += 1;
        }
        if self.os.is_some() {
            len += 1;
        }
        if !self.layers.is_empty() {
            len += 1;
        }
        if self.built_at.is_some() {
            len += 1;
        }
        if self.published_at.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.contimage.ContainerImage", len)?;
        if !self.id.is_empty() {
            struct_ser.serialize_field("id", &self.id)?;
        }
        if !self.dd_tags.is_empty() {
            struct_ser.serialize_field("ddTags", &self.dd_tags)?;
        }
        if !self.name.is_empty() {
            struct_ser.serialize_field("name", &self.name)?;
        }
        if !self.registry.is_empty() {
            struct_ser.serialize_field("registry", &self.registry)?;
        }
        if !self.short_name.is_empty() {
            struct_ser.serialize_field("shortName", &self.short_name)?;
        }
        if !self.repo_tags.is_empty() {
            struct_ser.serialize_field("repoTags", &self.repo_tags)?;
        }
        if !self.digest.is_empty() {
            struct_ser.serialize_field("digest", &self.digest)?;
        }
        if self.size != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("size", ToString::to_string(&self.size).as_str())?;
        }
        if !self.repo_digests.is_empty() {
            struct_ser.serialize_field("repoDigests", &self.repo_digests)?;
        }
        if let Some(v) = self.os.as_ref() {
            struct_ser.serialize_field("os", v)?;
        }
        if !self.layers.is_empty() {
            struct_ser.serialize_field("layers", &self.layers)?;
        }
        if let Some(v) = self.built_at.as_ref() {
            struct_ser.serialize_field("builtAt", v)?;
        }
        if let Some(v) = self.published_at.as_ref() {
            struct_ser.serialize_field("publishedAt", v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for ContainerImage {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "id",
            "dd_tags",
            "ddTags",
            "name",
            "registry",
            "short_name",
            "shortName",
            "repo_tags",
            "repoTags",
            "digest",
            "size",
            "repo_digests",
            "repoDigests",
            "os",
            "layers",
            "builtAt",
            "publishedAt",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Id,
            DdTags,
            Name,
            Registry,
            ShortName,
            RepoTags,
            Digest,
            Size,
            RepoDigests,
            Os,
            Layers,
            BuiltAt,
            PublishedAt,
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
                            "ddTags" | "dd_tags" => Ok(GeneratedField::DdTags),
                            "name" => Ok(GeneratedField::Name),
                            "registry" => Ok(GeneratedField::Registry),
                            "shortName" | "short_name" => Ok(GeneratedField::ShortName),
                            "repoTags" | "repo_tags" => Ok(GeneratedField::RepoTags),
                            "digest" => Ok(GeneratedField::Digest),
                            "size" => Ok(GeneratedField::Size),
                            "repoDigests" | "repo_digests" => Ok(GeneratedField::RepoDigests),
                            "os" => Ok(GeneratedField::Os),
                            "layers" => Ok(GeneratedField::Layers),
                            "builtAt" => Ok(GeneratedField::BuiltAt),
                            "publishedAt" => Ok(GeneratedField::PublishedAt),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = ContainerImage;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.contimage.ContainerImage")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<ContainerImage, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut id__ = None;
                let mut dd_tags__ = None;
                let mut name__ = None;
                let mut registry__ = None;
                let mut short_name__ = None;
                let mut repo_tags__ = None;
                let mut digest__ = None;
                let mut size__ = None;
                let mut repo_digests__ = None;
                let mut os__ = None;
                let mut layers__ = None;
                let mut built_at__ = None;
                let mut published_at__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Id => {
                            if id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("id"));
                            }
                            id__ = Some(map_.next_value()?);
                        }
                        GeneratedField::DdTags => {
                            if dd_tags__.is_some() {
                                return Err(serde::de::Error::duplicate_field("ddTags"));
                            }
                            dd_tags__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Name => {
                            if name__.is_some() {
                                return Err(serde::de::Error::duplicate_field("name"));
                            }
                            name__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Registry => {
                            if registry__.is_some() {
                                return Err(serde::de::Error::duplicate_field("registry"));
                            }
                            registry__ = Some(map_.next_value()?);
                        }
                        GeneratedField::ShortName => {
                            if short_name__.is_some() {
                                return Err(serde::de::Error::duplicate_field("shortName"));
                            }
                            short_name__ = Some(map_.next_value()?);
                        }
                        GeneratedField::RepoTags => {
                            if repo_tags__.is_some() {
                                return Err(serde::de::Error::duplicate_field("repoTags"));
                            }
                            repo_tags__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Digest => {
                            if digest__.is_some() {
                                return Err(serde::de::Error::duplicate_field("digest"));
                            }
                            digest__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Size => {
                            if size__.is_some() {
                                return Err(serde::de::Error::duplicate_field("size"));
                            }
                            size__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::RepoDigests => {
                            if repo_digests__.is_some() {
                                return Err(serde::de::Error::duplicate_field("repoDigests"));
                            }
                            repo_digests__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Os => {
                            if os__.is_some() {
                                return Err(serde::de::Error::duplicate_field("os"));
                            }
                            os__ = map_.next_value()?;
                        }
                        GeneratedField::Layers => {
                            if layers__.is_some() {
                                return Err(serde::de::Error::duplicate_field("layers"));
                            }
                            layers__ = Some(map_.next_value()?);
                        }
                        GeneratedField::BuiltAt => {
                            if built_at__.is_some() {
                                return Err(serde::de::Error::duplicate_field("builtAt"));
                            }
                            built_at__ = map_.next_value()?;
                        }
                        GeneratedField::PublishedAt => {
                            if published_at__.is_some() {
                                return Err(serde::de::Error::duplicate_field("publishedAt"));
                            }
                            published_at__ = map_.next_value()?;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(ContainerImage {
                    id: id__.unwrap_or_default(),
                    dd_tags: dd_tags__.unwrap_or_default(),
                    name: name__.unwrap_or_default(),
                    registry: registry__.unwrap_or_default(),
                    short_name: short_name__.unwrap_or_default(),
                    repo_tags: repo_tags__.unwrap_or_default(),
                    digest: digest__.unwrap_or_default(),
                    size: size__.unwrap_or_default(),
                    repo_digests: repo_digests__.unwrap_or_default(),
                    os: os__,
                    layers: layers__.unwrap_or_default(),
                    built_at: built_at__,
                    published_at: published_at__,
                })
            }
        }
        deserializer.deserialize_struct("datadog.contimage.ContainerImage", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for container_image::ContainerImageLayer {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.urls.is_empty() {
            len += 1;
        }
        if !self.media_type.is_empty() {
            len += 1;
        }
        if !self.digest.is_empty() {
            len += 1;
        }
        if self.size != 0 {
            len += 1;
        }
        if self.history.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.contimage.ContainerImage.ContainerImageLayer", len)?;
        if !self.urls.is_empty() {
            struct_ser.serialize_field("urls", &self.urls)?;
        }
        if !self.media_type.is_empty() {
            struct_ser.serialize_field("mediaType", &self.media_type)?;
        }
        if !self.digest.is_empty() {
            struct_ser.serialize_field("digest", &self.digest)?;
        }
        if self.size != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("size", ToString::to_string(&self.size).as_str())?;
        }
        if let Some(v) = self.history.as_ref() {
            struct_ser.serialize_field("history", v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for container_image::ContainerImageLayer {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "urls",
            "mediaType",
            "digest",
            "size",
            "history",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Urls,
            MediaType,
            Digest,
            Size,
            History,
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
                            "urls" => Ok(GeneratedField::Urls),
                            "mediaType" => Ok(GeneratedField::MediaType),
                            "digest" => Ok(GeneratedField::Digest),
                            "size" => Ok(GeneratedField::Size),
                            "history" => Ok(GeneratedField::History),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = container_image::ContainerImageLayer;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.contimage.ContainerImage.ContainerImageLayer")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<container_image::ContainerImageLayer, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut urls__ = None;
                let mut media_type__ = None;
                let mut digest__ = None;
                let mut size__ = None;
                let mut history__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Urls => {
                            if urls__.is_some() {
                                return Err(serde::de::Error::duplicate_field("urls"));
                            }
                            urls__ = Some(map_.next_value()?);
                        }
                        GeneratedField::MediaType => {
                            if media_type__.is_some() {
                                return Err(serde::de::Error::duplicate_field("mediaType"));
                            }
                            media_type__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Digest => {
                            if digest__.is_some() {
                                return Err(serde::de::Error::duplicate_field("digest"));
                            }
                            digest__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Size => {
                            if size__.is_some() {
                                return Err(serde::de::Error::duplicate_field("size"));
                            }
                            size__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::History => {
                            if history__.is_some() {
                                return Err(serde::de::Error::duplicate_field("history"));
                            }
                            history__ = map_.next_value()?;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(container_image::ContainerImageLayer {
                    urls: urls__.unwrap_or_default(),
                    media_type: media_type__.unwrap_or_default(),
                    digest: digest__.unwrap_or_default(),
                    size: size__.unwrap_or_default(),
                    history: history__,
                })
            }
        }
        deserializer.deserialize_struct("datadog.contimage.ContainerImage.ContainerImageLayer", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for container_image::container_image_layer::History {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.created.is_some() {
            len += 1;
        }
        if !self.created_by.is_empty() {
            len += 1;
        }
        if !self.author.is_empty() {
            len += 1;
        }
        if !self.comment.is_empty() {
            len += 1;
        }
        if self.empty_layer {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.contimage.ContainerImage.ContainerImageLayer.History", len)?;
        if let Some(v) = self.created.as_ref() {
            struct_ser.serialize_field("created", v)?;
        }
        if !self.created_by.is_empty() {
            struct_ser.serialize_field("createdBy", &self.created_by)?;
        }
        if !self.author.is_empty() {
            struct_ser.serialize_field("author", &self.author)?;
        }
        if !self.comment.is_empty() {
            struct_ser.serialize_field("comment", &self.comment)?;
        }
        if self.empty_layer {
            struct_ser.serialize_field("emptyLayer", &self.empty_layer)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for container_image::container_image_layer::History {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "created",
            "createdBy",
            "author",
            "comment",
            "emptyLayer",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Created,
            CreatedBy,
            Author,
            Comment,
            EmptyLayer,
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
                            "created" => Ok(GeneratedField::Created),
                            "createdBy" => Ok(GeneratedField::CreatedBy),
                            "author" => Ok(GeneratedField::Author),
                            "comment" => Ok(GeneratedField::Comment),
                            "emptyLayer" => Ok(GeneratedField::EmptyLayer),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = container_image::container_image_layer::History;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.contimage.ContainerImage.ContainerImageLayer.History")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<container_image::container_image_layer::History, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut created__ = None;
                let mut created_by__ = None;
                let mut author__ = None;
                let mut comment__ = None;
                let mut empty_layer__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Created => {
                            if created__.is_some() {
                                return Err(serde::de::Error::duplicate_field("created"));
                            }
                            created__ = map_.next_value()?;
                        }
                        GeneratedField::CreatedBy => {
                            if created_by__.is_some() {
                                return Err(serde::de::Error::duplicate_field("createdBy"));
                            }
                            created_by__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Author => {
                            if author__.is_some() {
                                return Err(serde::de::Error::duplicate_field("author"));
                            }
                            author__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Comment => {
                            if comment__.is_some() {
                                return Err(serde::de::Error::duplicate_field("comment"));
                            }
                            comment__ = Some(map_.next_value()?);
                        }
                        GeneratedField::EmptyLayer => {
                            if empty_layer__.is_some() {
                                return Err(serde::de::Error::duplicate_field("emptyLayer"));
                            }
                            empty_layer__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(container_image::container_image_layer::History {
                    created: created__,
                    created_by: created_by__.unwrap_or_default(),
                    author: author__.unwrap_or_default(),
                    comment: comment__.unwrap_or_default(),
                    empty_layer: empty_layer__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.contimage.ContainerImage.ContainerImageLayer.History", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for container_image::OperatingSystem {
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
        if !self.version.is_empty() {
            len += 1;
        }
        if !self.architecture.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.contimage.ContainerImage.OperatingSystem", len)?;
        if !self.name.is_empty() {
            struct_ser.serialize_field("name", &self.name)?;
        }
        if !self.version.is_empty() {
            struct_ser.serialize_field("version", &self.version)?;
        }
        if !self.architecture.is_empty() {
            struct_ser.serialize_field("architecture", &self.architecture)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for container_image::OperatingSystem {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "name",
            "version",
            "architecture",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Name,
            Version,
            Architecture,
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
                            "version" => Ok(GeneratedField::Version),
                            "architecture" => Ok(GeneratedField::Architecture),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = container_image::OperatingSystem;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.contimage.ContainerImage.OperatingSystem")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<container_image::OperatingSystem, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut name__ = None;
                let mut version__ = None;
                let mut architecture__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
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
                        GeneratedField::Architecture => {
                            if architecture__.is_some() {
                                return Err(serde::de::Error::duplicate_field("architecture"));
                            }
                            architecture__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(container_image::OperatingSystem {
                    name: name__.unwrap_or_default(),
                    version: version__.unwrap_or_default(),
                    architecture: architecture__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.contimage.ContainerImage.OperatingSystem", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for ContainerImagePayload {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.version.is_empty() {
            len += 1;
        }
        if !self.host.is_empty() {
            len += 1;
        }
        if self.source.is_some() {
            len += 1;
        }
        if !self.images.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.contimage.ContainerImagePayload", len)?;
        if !self.version.is_empty() {
            struct_ser.serialize_field("version", &self.version)?;
        }
        if !self.host.is_empty() {
            struct_ser.serialize_field("host", &self.host)?;
        }
        if let Some(v) = self.source.as_ref() {
            struct_ser.serialize_field("source", v)?;
        }
        if !self.images.is_empty() {
            struct_ser.serialize_field("images", &self.images)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for ContainerImagePayload {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "version",
            "host",
            "source",
            "images",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Version,
            Host,
            Source,
            Images,
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
                            "images" => Ok(GeneratedField::Images),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = ContainerImagePayload;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.contimage.ContainerImagePayload")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<ContainerImagePayload, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut version__ = None;
                let mut host__ = None;
                let mut source__ = None;
                let mut images__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Version => {
                            if version__.is_some() {
                                return Err(serde::de::Error::duplicate_field("version"));
                            }
                            version__ = Some(map_.next_value()?);
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
                        GeneratedField::Images => {
                            if images__.is_some() {
                                return Err(serde::de::Error::duplicate_field("images"));
                            }
                            images__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(ContainerImagePayload {
                    version: version__.unwrap_or_default(),
                    host: host__.unwrap_or_default(),
                    source: source__,
                    images: images__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.contimage.ContainerImagePayload", FIELDS, GeneratedVisitor)
    }
}
