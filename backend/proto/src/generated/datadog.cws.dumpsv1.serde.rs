impl serde::Serialize for AwsimdsEvent {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.is_imds_v2 {
            len += 1;
        }
        if self.security_credentials.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.cws.dumpsv1.AWSIMDSEvent", len)?;
        if self.is_imds_v2 {
            struct_ser.serialize_field("isImdsV2", &self.is_imds_v2)?;
        }
        if let Some(v) = self.security_credentials.as_ref() {
            struct_ser.serialize_field("securityCredentials", v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for AwsimdsEvent {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "is_imds_v2",
            "isImdsV2",
            "security_credentials",
            "securityCredentials",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            IsImdsV2,
            SecurityCredentials,
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
                            "isImdsV2" | "is_imds_v2" => Ok(GeneratedField::IsImdsV2),
                            "securityCredentials" | "security_credentials" => Ok(GeneratedField::SecurityCredentials),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = AwsimdsEvent;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.cws.dumpsv1.AWSIMDSEvent")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<AwsimdsEvent, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut is_imds_v2__ = None;
                let mut security_credentials__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::IsImdsV2 => {
                            if is_imds_v2__.is_some() {
                                return Err(serde::de::Error::duplicate_field("isImdsV2"));
                            }
                            is_imds_v2__ = Some(map_.next_value()?);
                        }
                        GeneratedField::SecurityCredentials => {
                            if security_credentials__.is_some() {
                                return Err(serde::de::Error::duplicate_field("securityCredentials"));
                            }
                            security_credentials__ = map_.next_value()?;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(AwsimdsEvent {
                    is_imds_v2: is_imds_v2__.unwrap_or_default(),
                    security_credentials: security_credentials__,
                })
            }
        }
        deserializer.deserialize_struct("datadog.cws.dumpsv1.AWSIMDSEvent", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for AwsSecurityCredentials {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.code.is_empty() {
            len += 1;
        }
        if !self.r#type.is_empty() {
            len += 1;
        }
        if !self.access_key_id.is_empty() {
            len += 1;
        }
        if !self.last_updated.is_empty() {
            len += 1;
        }
        if !self.expiration_raw.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.cws.dumpsv1.AWSSecurityCredentials", len)?;
        if !self.code.is_empty() {
            struct_ser.serialize_field("code", &self.code)?;
        }
        if !self.r#type.is_empty() {
            struct_ser.serialize_field("type", &self.r#type)?;
        }
        if !self.access_key_id.is_empty() {
            struct_ser.serialize_field("accessKeyId", &self.access_key_id)?;
        }
        if !self.last_updated.is_empty() {
            struct_ser.serialize_field("lastUpdated", &self.last_updated)?;
        }
        if !self.expiration_raw.is_empty() {
            struct_ser.serialize_field("expirationRaw", &self.expiration_raw)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for AwsSecurityCredentials {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "code",
            "type",
            "access_key_id",
            "accessKeyId",
            "last_updated",
            "lastUpdated",
            "expiration_raw",
            "expirationRaw",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Code,
            Type,
            AccessKeyId,
            LastUpdated,
            ExpirationRaw,
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
                            "code" => Ok(GeneratedField::Code),
                            "type" => Ok(GeneratedField::Type),
                            "accessKeyId" | "access_key_id" => Ok(GeneratedField::AccessKeyId),
                            "lastUpdated" | "last_updated" => Ok(GeneratedField::LastUpdated),
                            "expirationRaw" | "expiration_raw" => Ok(GeneratedField::ExpirationRaw),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = AwsSecurityCredentials;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.cws.dumpsv1.AWSSecurityCredentials")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<AwsSecurityCredentials, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut code__ = None;
                let mut r#type__ = None;
                let mut access_key_id__ = None;
                let mut last_updated__ = None;
                let mut expiration_raw__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Code => {
                            if code__.is_some() {
                                return Err(serde::de::Error::duplicate_field("code"));
                            }
                            code__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Type => {
                            if r#type__.is_some() {
                                return Err(serde::de::Error::duplicate_field("type"));
                            }
                            r#type__ = Some(map_.next_value()?);
                        }
                        GeneratedField::AccessKeyId => {
                            if access_key_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("accessKeyId"));
                            }
                            access_key_id__ = Some(map_.next_value()?);
                        }
                        GeneratedField::LastUpdated => {
                            if last_updated__.is_some() {
                                return Err(serde::de::Error::duplicate_field("lastUpdated"));
                            }
                            last_updated__ = Some(map_.next_value()?);
                        }
                        GeneratedField::ExpirationRaw => {
                            if expiration_raw__.is_some() {
                                return Err(serde::de::Error::duplicate_field("expirationRaw"));
                            }
                            expiration_raw__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(AwsSecurityCredentials {
                    code: code__.unwrap_or_default(),
                    r#type: r#type__.unwrap_or_default(),
                    access_key_id: access_key_id__.unwrap_or_default(),
                    last_updated: last_updated__.unwrap_or_default(),
                    expiration_raw: expiration_raw__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.cws.dumpsv1.AWSSecurityCredentials", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for BindNode {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.matched_rules.is_empty() {
            len += 1;
        }
        if !self.image_tags.is_empty() {
            len += 1;
        }
        if self.node_base.is_some() {
            len += 1;
        }
        if self.port != 0 {
            len += 1;
        }
        if !self.ip.is_empty() {
            len += 1;
        }
        if self.protocol != 0 {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.cws.dumpsv1.BindNode", len)?;
        if !self.matched_rules.is_empty() {
            struct_ser.serialize_field("matchedRules", &self.matched_rules)?;
        }
        if !self.image_tags.is_empty() {
            struct_ser.serialize_field("imageTags", &self.image_tags)?;
        }
        if let Some(v) = self.node_base.as_ref() {
            struct_ser.serialize_field("nodeBase", v)?;
        }
        if self.port != 0 {
            struct_ser.serialize_field("port", &self.port)?;
        }
        if !self.ip.is_empty() {
            struct_ser.serialize_field("ip", &self.ip)?;
        }
        if self.protocol != 0 {
            struct_ser.serialize_field("protocol", &self.protocol)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for BindNode {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "matched_rules",
            "matchedRules",
            "image_tags",
            "imageTags",
            "node_base",
            "nodeBase",
            "port",
            "ip",
            "protocol",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            MatchedRules,
            ImageTags,
            NodeBase,
            Port,
            Ip,
            Protocol,
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
                            "matchedRules" | "matched_rules" => Ok(GeneratedField::MatchedRules),
                            "imageTags" | "image_tags" => Ok(GeneratedField::ImageTags),
                            "nodeBase" | "node_base" => Ok(GeneratedField::NodeBase),
                            "port" => Ok(GeneratedField::Port),
                            "ip" => Ok(GeneratedField::Ip),
                            "protocol" => Ok(GeneratedField::Protocol),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = BindNode;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.cws.dumpsv1.BindNode")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<BindNode, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut matched_rules__ = None;
                let mut image_tags__ = None;
                let mut node_base__ = None;
                let mut port__ = None;
                let mut ip__ = None;
                let mut protocol__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::MatchedRules => {
                            if matched_rules__.is_some() {
                                return Err(serde::de::Error::duplicate_field("matchedRules"));
                            }
                            matched_rules__ = Some(map_.next_value()?);
                        }
                        GeneratedField::ImageTags => {
                            if image_tags__.is_some() {
                                return Err(serde::de::Error::duplicate_field("imageTags"));
                            }
                            image_tags__ = Some(map_.next_value()?);
                        }
                        GeneratedField::NodeBase => {
                            if node_base__.is_some() {
                                return Err(serde::de::Error::duplicate_field("nodeBase"));
                            }
                            node_base__ = map_.next_value()?;
                        }
                        GeneratedField::Port => {
                            if port__.is_some() {
                                return Err(serde::de::Error::duplicate_field("port"));
                            }
                            port__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Ip => {
                            if ip__.is_some() {
                                return Err(serde::de::Error::duplicate_field("ip"));
                            }
                            ip__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Protocol => {
                            if protocol__.is_some() {
                                return Err(serde::de::Error::duplicate_field("protocol"));
                            }
                            protocol__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(BindNode {
                    matched_rules: matched_rules__.unwrap_or_default(),
                    image_tags: image_tags__.unwrap_or_default(),
                    node_base: node_base__,
                    port: port__.unwrap_or_default(),
                    ip: ip__.unwrap_or_default(),
                    protocol: protocol__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.cws.dumpsv1.BindNode", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for CapabilityNode {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.node_base.is_some() {
            len += 1;
        }
        if self.capability != 0 {
            len += 1;
        }
        if self.is_capable {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.cws.dumpsv1.CapabilityNode", len)?;
        if let Some(v) = self.node_base.as_ref() {
            struct_ser.serialize_field("nodeBase", v)?;
        }
        if self.capability != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("capability", ToString::to_string(&self.capability).as_str())?;
        }
        if self.is_capable {
            struct_ser.serialize_field("isCapable", &self.is_capable)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for CapabilityNode {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "node_base",
            "nodeBase",
            "capability",
            "is_capable",
            "isCapable",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            NodeBase,
            Capability,
            IsCapable,
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
                            "nodeBase" | "node_base" => Ok(GeneratedField::NodeBase),
                            "capability" => Ok(GeneratedField::Capability),
                            "isCapable" | "is_capable" => Ok(GeneratedField::IsCapable),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = CapabilityNode;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.cws.dumpsv1.CapabilityNode")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<CapabilityNode, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut node_base__ = None;
                let mut capability__ = None;
                let mut is_capable__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::NodeBase => {
                            if node_base__.is_some() {
                                return Err(serde::de::Error::duplicate_field("nodeBase"));
                            }
                            node_base__ = map_.next_value()?;
                        }
                        GeneratedField::Capability => {
                            if capability__.is_some() {
                                return Err(serde::de::Error::duplicate_field("capability"));
                            }
                            capability__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::IsCapable => {
                            if is_capable__.is_some() {
                                return Err(serde::de::Error::duplicate_field("isCapable"));
                            }
                            is_capable__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(CapabilityNode {
                    node_base: node_base__,
                    capability: capability__.unwrap_or_default(),
                    is_capable: is_capable__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.cws.dumpsv1.CapabilityNode", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for Credentials {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.uid != 0 {
            len += 1;
        }
        if self.gid != 0 {
            len += 1;
        }
        if !self.user.is_empty() {
            len += 1;
        }
        if !self.group.is_empty() {
            len += 1;
        }
        if self.effective_uid != 0 {
            len += 1;
        }
        if self.effective_gid != 0 {
            len += 1;
        }
        if !self.effective_user.is_empty() {
            len += 1;
        }
        if !self.effective_group.is_empty() {
            len += 1;
        }
        if self.fs_uid != 0 {
            len += 1;
        }
        if self.fs_gid != 0 {
            len += 1;
        }
        if !self.fs_user.is_empty() {
            len += 1;
        }
        if !self.fs_group.is_empty() {
            len += 1;
        }
        if self.cap_effective != 0 {
            len += 1;
        }
        if self.cap_permitted != 0 {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.cws.dumpsv1.Credentials", len)?;
        if self.uid != 0 {
            struct_ser.serialize_field("uid", &self.uid)?;
        }
        if self.gid != 0 {
            struct_ser.serialize_field("gid", &self.gid)?;
        }
        if !self.user.is_empty() {
            struct_ser.serialize_field("user", &self.user)?;
        }
        if !self.group.is_empty() {
            struct_ser.serialize_field("group", &self.group)?;
        }
        if self.effective_uid != 0 {
            struct_ser.serialize_field("effectiveUid", &self.effective_uid)?;
        }
        if self.effective_gid != 0 {
            struct_ser.serialize_field("effectiveGid", &self.effective_gid)?;
        }
        if !self.effective_user.is_empty() {
            struct_ser.serialize_field("effectiveUser", &self.effective_user)?;
        }
        if !self.effective_group.is_empty() {
            struct_ser.serialize_field("effectiveGroup", &self.effective_group)?;
        }
        if self.fs_uid != 0 {
            struct_ser.serialize_field("fsUid", &self.fs_uid)?;
        }
        if self.fs_gid != 0 {
            struct_ser.serialize_field("fsGid", &self.fs_gid)?;
        }
        if !self.fs_user.is_empty() {
            struct_ser.serialize_field("fsUser", &self.fs_user)?;
        }
        if !self.fs_group.is_empty() {
            struct_ser.serialize_field("fsGroup", &self.fs_group)?;
        }
        if self.cap_effective != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("capEffective", ToString::to_string(&self.cap_effective).as_str())?;
        }
        if self.cap_permitted != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("capPermitted", ToString::to_string(&self.cap_permitted).as_str())?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for Credentials {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "uid",
            "gid",
            "user",
            "group",
            "effective_uid",
            "effectiveUid",
            "effective_gid",
            "effectiveGid",
            "effective_user",
            "effectiveUser",
            "effective_group",
            "effectiveGroup",
            "fs_uid",
            "fsUid",
            "fs_gid",
            "fsGid",
            "fs_user",
            "fsUser",
            "fs_group",
            "fsGroup",
            "cap_effective",
            "capEffective",
            "cap_permitted",
            "capPermitted",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Uid,
            Gid,
            User,
            Group,
            EffectiveUid,
            EffectiveGid,
            EffectiveUser,
            EffectiveGroup,
            FsUid,
            FsGid,
            FsUser,
            FsGroup,
            CapEffective,
            CapPermitted,
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
                            "gid" => Ok(GeneratedField::Gid),
                            "user" => Ok(GeneratedField::User),
                            "group" => Ok(GeneratedField::Group),
                            "effectiveUid" | "effective_uid" => Ok(GeneratedField::EffectiveUid),
                            "effectiveGid" | "effective_gid" => Ok(GeneratedField::EffectiveGid),
                            "effectiveUser" | "effective_user" => Ok(GeneratedField::EffectiveUser),
                            "effectiveGroup" | "effective_group" => Ok(GeneratedField::EffectiveGroup),
                            "fsUid" | "fs_uid" => Ok(GeneratedField::FsUid),
                            "fsGid" | "fs_gid" => Ok(GeneratedField::FsGid),
                            "fsUser" | "fs_user" => Ok(GeneratedField::FsUser),
                            "fsGroup" | "fs_group" => Ok(GeneratedField::FsGroup),
                            "capEffective" | "cap_effective" => Ok(GeneratedField::CapEffective),
                            "capPermitted" | "cap_permitted" => Ok(GeneratedField::CapPermitted),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = Credentials;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.cws.dumpsv1.Credentials")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<Credentials, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut uid__ = None;
                let mut gid__ = None;
                let mut user__ = None;
                let mut group__ = None;
                let mut effective_uid__ = None;
                let mut effective_gid__ = None;
                let mut effective_user__ = None;
                let mut effective_group__ = None;
                let mut fs_uid__ = None;
                let mut fs_gid__ = None;
                let mut fs_user__ = None;
                let mut fs_group__ = None;
                let mut cap_effective__ = None;
                let mut cap_permitted__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Uid => {
                            if uid__.is_some() {
                                return Err(serde::de::Error::duplicate_field("uid"));
                            }
                            uid__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Gid => {
                            if gid__.is_some() {
                                return Err(serde::de::Error::duplicate_field("gid"));
                            }
                            gid__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::User => {
                            if user__.is_some() {
                                return Err(serde::de::Error::duplicate_field("user"));
                            }
                            user__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Group => {
                            if group__.is_some() {
                                return Err(serde::de::Error::duplicate_field("group"));
                            }
                            group__ = Some(map_.next_value()?);
                        }
                        GeneratedField::EffectiveUid => {
                            if effective_uid__.is_some() {
                                return Err(serde::de::Error::duplicate_field("effectiveUid"));
                            }
                            effective_uid__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::EffectiveGid => {
                            if effective_gid__.is_some() {
                                return Err(serde::de::Error::duplicate_field("effectiveGid"));
                            }
                            effective_gid__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::EffectiveUser => {
                            if effective_user__.is_some() {
                                return Err(serde::de::Error::duplicate_field("effectiveUser"));
                            }
                            effective_user__ = Some(map_.next_value()?);
                        }
                        GeneratedField::EffectiveGroup => {
                            if effective_group__.is_some() {
                                return Err(serde::de::Error::duplicate_field("effectiveGroup"));
                            }
                            effective_group__ = Some(map_.next_value()?);
                        }
                        GeneratedField::FsUid => {
                            if fs_uid__.is_some() {
                                return Err(serde::de::Error::duplicate_field("fsUid"));
                            }
                            fs_uid__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::FsGid => {
                            if fs_gid__.is_some() {
                                return Err(serde::de::Error::duplicate_field("fsGid"));
                            }
                            fs_gid__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::FsUser => {
                            if fs_user__.is_some() {
                                return Err(serde::de::Error::duplicate_field("fsUser"));
                            }
                            fs_user__ = Some(map_.next_value()?);
                        }
                        GeneratedField::FsGroup => {
                            if fs_group__.is_some() {
                                return Err(serde::de::Error::duplicate_field("fsGroup"));
                            }
                            fs_group__ = Some(map_.next_value()?);
                        }
                        GeneratedField::CapEffective => {
                            if cap_effective__.is_some() {
                                return Err(serde::de::Error::duplicate_field("capEffective"));
                            }
                            cap_effective__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::CapPermitted => {
                            if cap_permitted__.is_some() {
                                return Err(serde::de::Error::duplicate_field("capPermitted"));
                            }
                            cap_permitted__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(Credentials {
                    uid: uid__.unwrap_or_default(),
                    gid: gid__.unwrap_or_default(),
                    user: user__.unwrap_or_default(),
                    group: group__.unwrap_or_default(),
                    effective_uid: effective_uid__.unwrap_or_default(),
                    effective_gid: effective_gid__.unwrap_or_default(),
                    effective_user: effective_user__.unwrap_or_default(),
                    effective_group: effective_group__.unwrap_or_default(),
                    fs_uid: fs_uid__.unwrap_or_default(),
                    fs_gid: fs_gid__.unwrap_or_default(),
                    fs_user: fs_user__.unwrap_or_default(),
                    fs_group: fs_group__.unwrap_or_default(),
                    cap_effective: cap_effective__.unwrap_or_default(),
                    cap_permitted: cap_permitted__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.cws.dumpsv1.Credentials", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for DnsInfo {
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
        if self.r#type != 0 {
            len += 1;
        }
        if self.class != 0 {
            len += 1;
        }
        if self.size != 0 {
            len += 1;
        }
        if self.count != 0 {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.cws.dumpsv1.DNSInfo", len)?;
        if !self.name.is_empty() {
            struct_ser.serialize_field("name", &self.name)?;
        }
        if self.r#type != 0 {
            struct_ser.serialize_field("type", &self.r#type)?;
        }
        if self.class != 0 {
            struct_ser.serialize_field("class", &self.class)?;
        }
        if self.size != 0 {
            struct_ser.serialize_field("size", &self.size)?;
        }
        if self.count != 0 {
            struct_ser.serialize_field("count", &self.count)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for DnsInfo {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "name",
            "type",
            "class",
            "size",
            "count",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Name,
            Type,
            Class,
            Size,
            Count,
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
                            "type" => Ok(GeneratedField::Type),
                            "class" => Ok(GeneratedField::Class),
                            "size" => Ok(GeneratedField::Size),
                            "count" => Ok(GeneratedField::Count),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = DnsInfo;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.cws.dumpsv1.DNSInfo")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<DnsInfo, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut name__ = None;
                let mut r#type__ = None;
                let mut class__ = None;
                let mut size__ = None;
                let mut count__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Name => {
                            if name__.is_some() {
                                return Err(serde::de::Error::duplicate_field("name"));
                            }
                            name__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Type => {
                            if r#type__.is_some() {
                                return Err(serde::de::Error::duplicate_field("type"));
                            }
                            r#type__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Class => {
                            if class__.is_some() {
                                return Err(serde::de::Error::duplicate_field("class"));
                            }
                            class__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Size => {
                            if size__.is_some() {
                                return Err(serde::de::Error::duplicate_field("size"));
                            }
                            size__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Count => {
                            if count__.is_some() {
                                return Err(serde::de::Error::duplicate_field("count"));
                            }
                            count__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(DnsInfo {
                    name: name__.unwrap_or_default(),
                    r#type: r#type__.unwrap_or_default(),
                    class: class__.unwrap_or_default(),
                    size: size__.unwrap_or_default(),
                    count: count__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.cws.dumpsv1.DNSInfo", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for DnsNode {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.matched_rules.is_empty() {
            len += 1;
        }
        if !self.image_tags.is_empty() {
            len += 1;
        }
        if self.node_base.is_some() {
            len += 1;
        }
        if !self.requests.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.cws.dumpsv1.DNSNode", len)?;
        if !self.matched_rules.is_empty() {
            struct_ser.serialize_field("matchedRules", &self.matched_rules)?;
        }
        if !self.image_tags.is_empty() {
            struct_ser.serialize_field("imageTags", &self.image_tags)?;
        }
        if let Some(v) = self.node_base.as_ref() {
            struct_ser.serialize_field("nodeBase", v)?;
        }
        if !self.requests.is_empty() {
            struct_ser.serialize_field("requests", &self.requests)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for DnsNode {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "matched_rules",
            "matchedRules",
            "image_tags",
            "imageTags",
            "node_base",
            "nodeBase",
            "requests",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            MatchedRules,
            ImageTags,
            NodeBase,
            Requests,
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
                            "matchedRules" | "matched_rules" => Ok(GeneratedField::MatchedRules),
                            "imageTags" | "image_tags" => Ok(GeneratedField::ImageTags),
                            "nodeBase" | "node_base" => Ok(GeneratedField::NodeBase),
                            "requests" => Ok(GeneratedField::Requests),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = DnsNode;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.cws.dumpsv1.DNSNode")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<DnsNode, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut matched_rules__ = None;
                let mut image_tags__ = None;
                let mut node_base__ = None;
                let mut requests__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::MatchedRules => {
                            if matched_rules__.is_some() {
                                return Err(serde::de::Error::duplicate_field("matchedRules"));
                            }
                            matched_rules__ = Some(map_.next_value()?);
                        }
                        GeneratedField::ImageTags => {
                            if image_tags__.is_some() {
                                return Err(serde::de::Error::duplicate_field("imageTags"));
                            }
                            image_tags__ = Some(map_.next_value()?);
                        }
                        GeneratedField::NodeBase => {
                            if node_base__.is_some() {
                                return Err(serde::de::Error::duplicate_field("nodeBase"));
                            }
                            node_base__ = map_.next_value()?;
                        }
                        GeneratedField::Requests => {
                            if requests__.is_some() {
                                return Err(serde::de::Error::duplicate_field("requests"));
                            }
                            requests__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(DnsNode {
                    matched_rules: matched_rules__.unwrap_or_default(),
                    image_tags: image_tags__.unwrap_or_default(),
                    node_base: node_base__,
                    requests: requests__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.cws.dumpsv1.DNSNode", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for FileActivityNode {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.matched_rules.is_empty() {
            len += 1;
        }
        if !self.image_tags.is_empty() {
            len += 1;
        }
        if self.node_base.is_some() {
            len += 1;
        }
        if !self.name.is_empty() {
            len += 1;
        }
        if self.is_pattern {
            len += 1;
        }
        if self.file.is_some() {
            len += 1;
        }
        if self.generation_type != 0 {
            len += 1;
        }
        if self.first_seen != 0 {
            len += 1;
        }
        if self.open.is_some() {
            len += 1;
        }
        if !self.children.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.cws.dumpsv1.FileActivityNode", len)?;
        if !self.matched_rules.is_empty() {
            struct_ser.serialize_field("matchedRules", &self.matched_rules)?;
        }
        if !self.image_tags.is_empty() {
            struct_ser.serialize_field("imageTags", &self.image_tags)?;
        }
        if let Some(v) = self.node_base.as_ref() {
            struct_ser.serialize_field("nodeBase", v)?;
        }
        if !self.name.is_empty() {
            struct_ser.serialize_field("name", &self.name)?;
        }
        if self.is_pattern {
            struct_ser.serialize_field("isPattern", &self.is_pattern)?;
        }
        if let Some(v) = self.file.as_ref() {
            struct_ser.serialize_field("file", v)?;
        }
        if self.generation_type != 0 {
            let v = GenerationType::try_from(self.generation_type)
                .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", self.generation_type)))?;
            struct_ser.serialize_field("generationType", &v)?;
        }
        if self.first_seen != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("firstSeen", ToString::to_string(&self.first_seen).as_str())?;
        }
        if let Some(v) = self.open.as_ref() {
            struct_ser.serialize_field("open", v)?;
        }
        if !self.children.is_empty() {
            struct_ser.serialize_field("children", &self.children)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for FileActivityNode {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "matched_rules",
            "matchedRules",
            "image_tags",
            "imageTags",
            "node_base",
            "nodeBase",
            "name",
            "is_pattern",
            "isPattern",
            "file",
            "generation_type",
            "generationType",
            "first_seen",
            "firstSeen",
            "open",
            "children",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            MatchedRules,
            ImageTags,
            NodeBase,
            Name,
            IsPattern,
            File,
            GenerationType,
            FirstSeen,
            Open,
            Children,
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
                            "matchedRules" | "matched_rules" => Ok(GeneratedField::MatchedRules),
                            "imageTags" | "image_tags" => Ok(GeneratedField::ImageTags),
                            "nodeBase" | "node_base" => Ok(GeneratedField::NodeBase),
                            "name" => Ok(GeneratedField::Name),
                            "isPattern" | "is_pattern" => Ok(GeneratedField::IsPattern),
                            "file" => Ok(GeneratedField::File),
                            "generationType" | "generation_type" => Ok(GeneratedField::GenerationType),
                            "firstSeen" | "first_seen" => Ok(GeneratedField::FirstSeen),
                            "open" => Ok(GeneratedField::Open),
                            "children" => Ok(GeneratedField::Children),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = FileActivityNode;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.cws.dumpsv1.FileActivityNode")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<FileActivityNode, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut matched_rules__ = None;
                let mut image_tags__ = None;
                let mut node_base__ = None;
                let mut name__ = None;
                let mut is_pattern__ = None;
                let mut file__ = None;
                let mut generation_type__ = None;
                let mut first_seen__ = None;
                let mut open__ = None;
                let mut children__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::MatchedRules => {
                            if matched_rules__.is_some() {
                                return Err(serde::de::Error::duplicate_field("matchedRules"));
                            }
                            matched_rules__ = Some(map_.next_value()?);
                        }
                        GeneratedField::ImageTags => {
                            if image_tags__.is_some() {
                                return Err(serde::de::Error::duplicate_field("imageTags"));
                            }
                            image_tags__ = Some(map_.next_value()?);
                        }
                        GeneratedField::NodeBase => {
                            if node_base__.is_some() {
                                return Err(serde::de::Error::duplicate_field("nodeBase"));
                            }
                            node_base__ = map_.next_value()?;
                        }
                        GeneratedField::Name => {
                            if name__.is_some() {
                                return Err(serde::de::Error::duplicate_field("name"));
                            }
                            name__ = Some(map_.next_value()?);
                        }
                        GeneratedField::IsPattern => {
                            if is_pattern__.is_some() {
                                return Err(serde::de::Error::duplicate_field("isPattern"));
                            }
                            is_pattern__ = Some(map_.next_value()?);
                        }
                        GeneratedField::File => {
                            if file__.is_some() {
                                return Err(serde::de::Error::duplicate_field("file"));
                            }
                            file__ = map_.next_value()?;
                        }
                        GeneratedField::GenerationType => {
                            if generation_type__.is_some() {
                                return Err(serde::de::Error::duplicate_field("generationType"));
                            }
                            generation_type__ = Some(map_.next_value::<GenerationType>()? as i32);
                        }
                        GeneratedField::FirstSeen => {
                            if first_seen__.is_some() {
                                return Err(serde::de::Error::duplicate_field("firstSeen"));
                            }
                            first_seen__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Open => {
                            if open__.is_some() {
                                return Err(serde::de::Error::duplicate_field("open"));
                            }
                            open__ = map_.next_value()?;
                        }
                        GeneratedField::Children => {
                            if children__.is_some() {
                                return Err(serde::de::Error::duplicate_field("children"));
                            }
                            children__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(FileActivityNode {
                    matched_rules: matched_rules__.unwrap_or_default(),
                    image_tags: image_tags__.unwrap_or_default(),
                    node_base: node_base__,
                    name: name__.unwrap_or_default(),
                    is_pattern: is_pattern__.unwrap_or_default(),
                    file: file__,
                    generation_type: generation_type__.unwrap_or_default(),
                    first_seen: first_seen__.unwrap_or_default(),
                    open: open__,
                    children: children__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.cws.dumpsv1.FileActivityNode", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for FileInfo {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.uid != 0 {
            len += 1;
        }
        if !self.user.is_empty() {
            len += 1;
        }
        if self.gid != 0 {
            len += 1;
        }
        if !self.group.is_empty() {
            len += 1;
        }
        if self.mode != 0 {
            len += 1;
        }
        if self.ctime != 0 {
            len += 1;
        }
        if self.mtime != 0 {
            len += 1;
        }
        if self.mount_id != 0 {
            len += 1;
        }
        if self.inode != 0 {
            len += 1;
        }
        if self.in_upper_layer {
            len += 1;
        }
        if !self.path.is_empty() {
            len += 1;
        }
        if !self.basename.is_empty() {
            len += 1;
        }
        if !self.filesystem.is_empty() {
            len += 1;
        }
        if !self.package_name.is_empty() {
            len += 1;
        }
        if !self.package_version.is_empty() {
            len += 1;
        }
        if self.package_epoch.is_some() {
            len += 1;
        }
        if self.package_release.is_some() {
            len += 1;
        }
        if !self.package_src_version.is_empty() {
            len += 1;
        }
        if self.package_src_epoch.is_some() {
            len += 1;
        }
        if self.package_src_release.is_some() {
            len += 1;
        }
        if !self.hashes.is_empty() {
            len += 1;
        }
        if self.hash_state != 0 {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.cws.dumpsv1.FileInfo", len)?;
        if self.uid != 0 {
            struct_ser.serialize_field("uid", &self.uid)?;
        }
        if !self.user.is_empty() {
            struct_ser.serialize_field("user", &self.user)?;
        }
        if self.gid != 0 {
            struct_ser.serialize_field("gid", &self.gid)?;
        }
        if !self.group.is_empty() {
            struct_ser.serialize_field("group", &self.group)?;
        }
        if self.mode != 0 {
            struct_ser.serialize_field("mode", &self.mode)?;
        }
        if self.ctime != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("ctime", ToString::to_string(&self.ctime).as_str())?;
        }
        if self.mtime != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("mtime", ToString::to_string(&self.mtime).as_str())?;
        }
        if self.mount_id != 0 {
            struct_ser.serialize_field("mountId", &self.mount_id)?;
        }
        if self.inode != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("inode", ToString::to_string(&self.inode).as_str())?;
        }
        if self.in_upper_layer {
            struct_ser.serialize_field("inUpperLayer", &self.in_upper_layer)?;
        }
        if !self.path.is_empty() {
            struct_ser.serialize_field("path", &self.path)?;
        }
        if !self.basename.is_empty() {
            struct_ser.serialize_field("basename", &self.basename)?;
        }
        if !self.filesystem.is_empty() {
            struct_ser.serialize_field("filesystem", &self.filesystem)?;
        }
        if !self.package_name.is_empty() {
            struct_ser.serialize_field("packageName", &self.package_name)?;
        }
        if !self.package_version.is_empty() {
            struct_ser.serialize_field("packageVersion", &self.package_version)?;
        }
        if let Some(v) = self.package_epoch.as_ref() {
            struct_ser.serialize_field("packageEpoch", v)?;
        }
        if let Some(v) = self.package_release.as_ref() {
            struct_ser.serialize_field("packageRelease", v)?;
        }
        if !self.package_src_version.is_empty() {
            struct_ser.serialize_field("packageSrcVersion", &self.package_src_version)?;
        }
        if let Some(v) = self.package_src_epoch.as_ref() {
            struct_ser.serialize_field("packageSrcEpoch", v)?;
        }
        if let Some(v) = self.package_src_release.as_ref() {
            struct_ser.serialize_field("packageSrcRelease", v)?;
        }
        if !self.hashes.is_empty() {
            struct_ser.serialize_field("hashes", &self.hashes)?;
        }
        if self.hash_state != 0 {
            let v = HashState::try_from(self.hash_state)
                .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", self.hash_state)))?;
            struct_ser.serialize_field("hashState", &v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for FileInfo {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "uid",
            "user",
            "gid",
            "group",
            "mode",
            "ctime",
            "mtime",
            "mount_id",
            "mountId",
            "inode",
            "in_upper_layer",
            "inUpperLayer",
            "path",
            "basename",
            "filesystem",
            "package_name",
            "packageName",
            "package_version",
            "packageVersion",
            "package_epoch",
            "packageEpoch",
            "package_release",
            "packageRelease",
            "package_src_version",
            "packageSrcVersion",
            "package_src_epoch",
            "packageSrcEpoch",
            "package_src_release",
            "packageSrcRelease",
            "hashes",
            "hash_state",
            "hashState",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Uid,
            User,
            Gid,
            Group,
            Mode,
            Ctime,
            Mtime,
            MountId,
            Inode,
            InUpperLayer,
            Path,
            Basename,
            Filesystem,
            PackageName,
            PackageVersion,
            PackageEpoch,
            PackageRelease,
            PackageSrcVersion,
            PackageSrcEpoch,
            PackageSrcRelease,
            Hashes,
            HashState,
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
                            "user" => Ok(GeneratedField::User),
                            "gid" => Ok(GeneratedField::Gid),
                            "group" => Ok(GeneratedField::Group),
                            "mode" => Ok(GeneratedField::Mode),
                            "ctime" => Ok(GeneratedField::Ctime),
                            "mtime" => Ok(GeneratedField::Mtime),
                            "mountId" | "mount_id" => Ok(GeneratedField::MountId),
                            "inode" => Ok(GeneratedField::Inode),
                            "inUpperLayer" | "in_upper_layer" => Ok(GeneratedField::InUpperLayer),
                            "path" => Ok(GeneratedField::Path),
                            "basename" => Ok(GeneratedField::Basename),
                            "filesystem" => Ok(GeneratedField::Filesystem),
                            "packageName" | "package_name" => Ok(GeneratedField::PackageName),
                            "packageVersion" | "package_version" => Ok(GeneratedField::PackageVersion),
                            "packageEpoch" | "package_epoch" => Ok(GeneratedField::PackageEpoch),
                            "packageRelease" | "package_release" => Ok(GeneratedField::PackageRelease),
                            "packageSrcVersion" | "package_src_version" => Ok(GeneratedField::PackageSrcVersion),
                            "packageSrcEpoch" | "package_src_epoch" => Ok(GeneratedField::PackageSrcEpoch),
                            "packageSrcRelease" | "package_src_release" => Ok(GeneratedField::PackageSrcRelease),
                            "hashes" => Ok(GeneratedField::Hashes),
                            "hashState" | "hash_state" => Ok(GeneratedField::HashState),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = FileInfo;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.cws.dumpsv1.FileInfo")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<FileInfo, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut uid__ = None;
                let mut user__ = None;
                let mut gid__ = None;
                let mut group__ = None;
                let mut mode__ = None;
                let mut ctime__ = None;
                let mut mtime__ = None;
                let mut mount_id__ = None;
                let mut inode__ = None;
                let mut in_upper_layer__ = None;
                let mut path__ = None;
                let mut basename__ = None;
                let mut filesystem__ = None;
                let mut package_name__ = None;
                let mut package_version__ = None;
                let mut package_epoch__ = None;
                let mut package_release__ = None;
                let mut package_src_version__ = None;
                let mut package_src_epoch__ = None;
                let mut package_src_release__ = None;
                let mut hashes__ = None;
                let mut hash_state__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Uid => {
                            if uid__.is_some() {
                                return Err(serde::de::Error::duplicate_field("uid"));
                            }
                            uid__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::User => {
                            if user__.is_some() {
                                return Err(serde::de::Error::duplicate_field("user"));
                            }
                            user__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Gid => {
                            if gid__.is_some() {
                                return Err(serde::de::Error::duplicate_field("gid"));
                            }
                            gid__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Group => {
                            if group__.is_some() {
                                return Err(serde::de::Error::duplicate_field("group"));
                            }
                            group__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Mode => {
                            if mode__.is_some() {
                                return Err(serde::de::Error::duplicate_field("mode"));
                            }
                            mode__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Ctime => {
                            if ctime__.is_some() {
                                return Err(serde::de::Error::duplicate_field("ctime"));
                            }
                            ctime__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Mtime => {
                            if mtime__.is_some() {
                                return Err(serde::de::Error::duplicate_field("mtime"));
                            }
                            mtime__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::MountId => {
                            if mount_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("mountId"));
                            }
                            mount_id__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Inode => {
                            if inode__.is_some() {
                                return Err(serde::de::Error::duplicate_field("inode"));
                            }
                            inode__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::InUpperLayer => {
                            if in_upper_layer__.is_some() {
                                return Err(serde::de::Error::duplicate_field("inUpperLayer"));
                            }
                            in_upper_layer__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Path => {
                            if path__.is_some() {
                                return Err(serde::de::Error::duplicate_field("path"));
                            }
                            path__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Basename => {
                            if basename__.is_some() {
                                return Err(serde::de::Error::duplicate_field("basename"));
                            }
                            basename__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Filesystem => {
                            if filesystem__.is_some() {
                                return Err(serde::de::Error::duplicate_field("filesystem"));
                            }
                            filesystem__ = Some(map_.next_value()?);
                        }
                        GeneratedField::PackageName => {
                            if package_name__.is_some() {
                                return Err(serde::de::Error::duplicate_field("packageName"));
                            }
                            package_name__ = Some(map_.next_value()?);
                        }
                        GeneratedField::PackageVersion => {
                            if package_version__.is_some() {
                                return Err(serde::de::Error::duplicate_field("packageVersion"));
                            }
                            package_version__ = Some(map_.next_value()?);
                        }
                        GeneratedField::PackageEpoch => {
                            if package_epoch__.is_some() {
                                return Err(serde::de::Error::duplicate_field("packageEpoch"));
                            }
                            package_epoch__ = 
                                map_.next_value::<::std::option::Option<::pbjson::private::NumberDeserialize<_>>>()?.map(|x| x.0)
                            ;
                        }
                        GeneratedField::PackageRelease => {
                            if package_release__.is_some() {
                                return Err(serde::de::Error::duplicate_field("packageRelease"));
                            }
                            package_release__ = map_.next_value()?;
                        }
                        GeneratedField::PackageSrcVersion => {
                            if package_src_version__.is_some() {
                                return Err(serde::de::Error::duplicate_field("packageSrcVersion"));
                            }
                            package_src_version__ = Some(map_.next_value()?);
                        }
                        GeneratedField::PackageSrcEpoch => {
                            if package_src_epoch__.is_some() {
                                return Err(serde::de::Error::duplicate_field("packageSrcEpoch"));
                            }
                            package_src_epoch__ = 
                                map_.next_value::<::std::option::Option<::pbjson::private::NumberDeserialize<_>>>()?.map(|x| x.0)
                            ;
                        }
                        GeneratedField::PackageSrcRelease => {
                            if package_src_release__.is_some() {
                                return Err(serde::de::Error::duplicate_field("packageSrcRelease"));
                            }
                            package_src_release__ = map_.next_value()?;
                        }
                        GeneratedField::Hashes => {
                            if hashes__.is_some() {
                                return Err(serde::de::Error::duplicate_field("hashes"));
                            }
                            hashes__ = Some(map_.next_value()?);
                        }
                        GeneratedField::HashState => {
                            if hash_state__.is_some() {
                                return Err(serde::de::Error::duplicate_field("hashState"));
                            }
                            hash_state__ = Some(map_.next_value::<HashState>()? as i32);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(FileInfo {
                    uid: uid__.unwrap_or_default(),
                    user: user__.unwrap_or_default(),
                    gid: gid__.unwrap_or_default(),
                    group: group__.unwrap_or_default(),
                    mode: mode__.unwrap_or_default(),
                    ctime: ctime__.unwrap_or_default(),
                    mtime: mtime__.unwrap_or_default(),
                    mount_id: mount_id__.unwrap_or_default(),
                    inode: inode__.unwrap_or_default(),
                    in_upper_layer: in_upper_layer__.unwrap_or_default(),
                    path: path__.unwrap_or_default(),
                    basename: basename__.unwrap_or_default(),
                    filesystem: filesystem__.unwrap_or_default(),
                    package_name: package_name__.unwrap_or_default(),
                    package_version: package_version__.unwrap_or_default(),
                    package_epoch: package_epoch__,
                    package_release: package_release__,
                    package_src_version: package_src_version__.unwrap_or_default(),
                    package_src_epoch: package_src_epoch__,
                    package_src_release: package_src_release__,
                    hashes: hashes__.unwrap_or_default(),
                    hash_state: hash_state__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.cws.dumpsv1.FileInfo", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for FlowNode {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.image_tags.is_empty() {
            len += 1;
        }
        if self.node_base.is_some() {
            len += 1;
        }
        if self.source.is_some() {
            len += 1;
        }
        if self.destination.is_some() {
            len += 1;
        }
        if self.l3_protocol != 0 {
            len += 1;
        }
        if self.l4_protocol != 0 {
            len += 1;
        }
        if self.ingress.is_some() {
            len += 1;
        }
        if self.egress.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.cws.dumpsv1.FlowNode", len)?;
        if !self.image_tags.is_empty() {
            struct_ser.serialize_field("imageTags", &self.image_tags)?;
        }
        if let Some(v) = self.node_base.as_ref() {
            struct_ser.serialize_field("nodeBase", v)?;
        }
        if let Some(v) = self.source.as_ref() {
            struct_ser.serialize_field("source", v)?;
        }
        if let Some(v) = self.destination.as_ref() {
            struct_ser.serialize_field("destination", v)?;
        }
        if self.l3_protocol != 0 {
            struct_ser.serialize_field("l3Protocol", &self.l3_protocol)?;
        }
        if self.l4_protocol != 0 {
            struct_ser.serialize_field("l4Protocol", &self.l4_protocol)?;
        }
        if let Some(v) = self.ingress.as_ref() {
            struct_ser.serialize_field("ingress", v)?;
        }
        if let Some(v) = self.egress.as_ref() {
            struct_ser.serialize_field("egress", v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for FlowNode {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "image_tags",
            "imageTags",
            "node_base",
            "nodeBase",
            "source",
            "destination",
            "l3_protocol",
            "l3Protocol",
            "l4_protocol",
            "l4Protocol",
            "ingress",
            "egress",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            ImageTags,
            NodeBase,
            Source,
            Destination,
            L3Protocol,
            L4Protocol,
            Ingress,
            Egress,
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
                            "imageTags" | "image_tags" => Ok(GeneratedField::ImageTags),
                            "nodeBase" | "node_base" => Ok(GeneratedField::NodeBase),
                            "source" => Ok(GeneratedField::Source),
                            "destination" => Ok(GeneratedField::Destination),
                            "l3Protocol" | "l3_protocol" => Ok(GeneratedField::L3Protocol),
                            "l4Protocol" | "l4_protocol" => Ok(GeneratedField::L4Protocol),
                            "ingress" => Ok(GeneratedField::Ingress),
                            "egress" => Ok(GeneratedField::Egress),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = FlowNode;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.cws.dumpsv1.FlowNode")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<FlowNode, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut image_tags__ = None;
                let mut node_base__ = None;
                let mut source__ = None;
                let mut destination__ = None;
                let mut l3_protocol__ = None;
                let mut l4_protocol__ = None;
                let mut ingress__ = None;
                let mut egress__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::ImageTags => {
                            if image_tags__.is_some() {
                                return Err(serde::de::Error::duplicate_field("imageTags"));
                            }
                            image_tags__ = Some(map_.next_value()?);
                        }
                        GeneratedField::NodeBase => {
                            if node_base__.is_some() {
                                return Err(serde::de::Error::duplicate_field("nodeBase"));
                            }
                            node_base__ = map_.next_value()?;
                        }
                        GeneratedField::Source => {
                            if source__.is_some() {
                                return Err(serde::de::Error::duplicate_field("source"));
                            }
                            source__ = map_.next_value()?;
                        }
                        GeneratedField::Destination => {
                            if destination__.is_some() {
                                return Err(serde::de::Error::duplicate_field("destination"));
                            }
                            destination__ = map_.next_value()?;
                        }
                        GeneratedField::L3Protocol => {
                            if l3_protocol__.is_some() {
                                return Err(serde::de::Error::duplicate_field("l3Protocol"));
                            }
                            l3_protocol__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::L4Protocol => {
                            if l4_protocol__.is_some() {
                                return Err(serde::de::Error::duplicate_field("l4Protocol"));
                            }
                            l4_protocol__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Ingress => {
                            if ingress__.is_some() {
                                return Err(serde::de::Error::duplicate_field("ingress"));
                            }
                            ingress__ = map_.next_value()?;
                        }
                        GeneratedField::Egress => {
                            if egress__.is_some() {
                                return Err(serde::de::Error::duplicate_field("egress"));
                            }
                            egress__ = map_.next_value()?;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(FlowNode {
                    image_tags: image_tags__.unwrap_or_default(),
                    node_base: node_base__,
                    source: source__,
                    destination: destination__,
                    l3_protocol: l3_protocol__.unwrap_or_default(),
                    l4_protocol: l4_protocol__.unwrap_or_default(),
                    ingress: ingress__,
                    egress: egress__,
                })
            }
        }
        deserializer.deserialize_struct("datadog.cws.dumpsv1.FlowNode", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for GenerationType {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let variant = match self {
            Self::Unknown => "UNKNOWN",
            Self::Runtime => "RUNTIME",
            Self::Snapshot => "SNAPSHOT",
        };
        serializer.serialize_str(variant)
    }
}
impl<'de> serde::Deserialize<'de> for GenerationType {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "UNKNOWN",
            "RUNTIME",
            "SNAPSHOT",
        ];

        struct GeneratedVisitor;

        impl serde::de::Visitor<'_> for GeneratedVisitor {
            type Value = GenerationType;

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
                    "UNKNOWN" => Ok(GenerationType::Unknown),
                    "RUNTIME" => Ok(GenerationType::Runtime),
                    "SNAPSHOT" => Ok(GenerationType::Snapshot),
                    _ => Err(serde::de::Error::unknown_variant(value, FIELDS)),
                }
            }
        }
        deserializer.deserialize_any(GeneratedVisitor)
    }
}
impl serde::Serialize for HashState {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let variant = match self {
            Self::NoHash => "NO_HASH",
            Self::Done => "DONE",
            Self::FileNotFound => "FILE_NOT_FOUND",
            Self::PathnameResolutionError => "PATHNAME_RESOLUTION_ERROR",
            Self::FileTooBig => "FILE_TOO_BIG",
            Self::EventTypeNotConfigured => "EVENT_TYPE_NOT_CONFIGURED",
            Self::HashWasRateLimited => "HASH_WAS_RATE_LIMITED",
            Self::UnknownHashError => "UNKNOWN_HASH_ERROR",
        };
        serializer.serialize_str(variant)
    }
}
impl<'de> serde::Deserialize<'de> for HashState {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "NO_HASH",
            "DONE",
            "FILE_NOT_FOUND",
            "PATHNAME_RESOLUTION_ERROR",
            "FILE_TOO_BIG",
            "EVENT_TYPE_NOT_CONFIGURED",
            "HASH_WAS_RATE_LIMITED",
            "UNKNOWN_HASH_ERROR",
        ];

        struct GeneratedVisitor;

        impl serde::de::Visitor<'_> for GeneratedVisitor {
            type Value = HashState;

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
                    "NO_HASH" => Ok(HashState::NoHash),
                    "DONE" => Ok(HashState::Done),
                    "FILE_NOT_FOUND" => Ok(HashState::FileNotFound),
                    "PATHNAME_RESOLUTION_ERROR" => Ok(HashState::PathnameResolutionError),
                    "FILE_TOO_BIG" => Ok(HashState::FileTooBig),
                    "EVENT_TYPE_NOT_CONFIGURED" => Ok(HashState::EventTypeNotConfigured),
                    "HASH_WAS_RATE_LIMITED" => Ok(HashState::HashWasRateLimited),
                    "UNKNOWN_HASH_ERROR" => Ok(HashState::UnknownHashError),
                    _ => Err(serde::de::Error::unknown_variant(value, FIELDS)),
                }
            }
        }
        deserializer.deserialize_any(GeneratedVisitor)
    }
}
impl serde::Serialize for ImdsEvent {
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
        if !self.cloud_provider.is_empty() {
            len += 1;
        }
        if !self.url.is_empty() {
            len += 1;
        }
        if !self.host.is_empty() {
            len += 1;
        }
        if !self.user_agent.is_empty() {
            len += 1;
        }
        if !self.server.is_empty() {
            len += 1;
        }
        if self.aws.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.cws.dumpsv1.IMDSEvent", len)?;
        if !self.r#type.is_empty() {
            struct_ser.serialize_field("type", &self.r#type)?;
        }
        if !self.cloud_provider.is_empty() {
            struct_ser.serialize_field("cloudProvider", &self.cloud_provider)?;
        }
        if !self.url.is_empty() {
            struct_ser.serialize_field("url", &self.url)?;
        }
        if !self.host.is_empty() {
            struct_ser.serialize_field("host", &self.host)?;
        }
        if !self.user_agent.is_empty() {
            struct_ser.serialize_field("userAgent", &self.user_agent)?;
        }
        if !self.server.is_empty() {
            struct_ser.serialize_field("server", &self.server)?;
        }
        if let Some(v) = self.aws.as_ref() {
            struct_ser.serialize_field("aws", v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for ImdsEvent {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "type",
            "cloud_provider",
            "cloudProvider",
            "url",
            "host",
            "user_agent",
            "userAgent",
            "server",
            "aws",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Type,
            CloudProvider,
            Url,
            Host,
            UserAgent,
            Server,
            Aws,
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
                            "cloudProvider" | "cloud_provider" => Ok(GeneratedField::CloudProvider),
                            "url" => Ok(GeneratedField::Url),
                            "host" => Ok(GeneratedField::Host),
                            "userAgent" | "user_agent" => Ok(GeneratedField::UserAgent),
                            "server" => Ok(GeneratedField::Server),
                            "aws" => Ok(GeneratedField::Aws),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = ImdsEvent;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.cws.dumpsv1.IMDSEvent")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<ImdsEvent, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut r#type__ = None;
                let mut cloud_provider__ = None;
                let mut url__ = None;
                let mut host__ = None;
                let mut user_agent__ = None;
                let mut server__ = None;
                let mut aws__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Type => {
                            if r#type__.is_some() {
                                return Err(serde::de::Error::duplicate_field("type"));
                            }
                            r#type__ = Some(map_.next_value()?);
                        }
                        GeneratedField::CloudProvider => {
                            if cloud_provider__.is_some() {
                                return Err(serde::de::Error::duplicate_field("cloudProvider"));
                            }
                            cloud_provider__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Url => {
                            if url__.is_some() {
                                return Err(serde::de::Error::duplicate_field("url"));
                            }
                            url__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Host => {
                            if host__.is_some() {
                                return Err(serde::de::Error::duplicate_field("host"));
                            }
                            host__ = Some(map_.next_value()?);
                        }
                        GeneratedField::UserAgent => {
                            if user_agent__.is_some() {
                                return Err(serde::de::Error::duplicate_field("userAgent"));
                            }
                            user_agent__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Server => {
                            if server__.is_some() {
                                return Err(serde::de::Error::duplicate_field("server"));
                            }
                            server__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Aws => {
                            if aws__.is_some() {
                                return Err(serde::de::Error::duplicate_field("aws"));
                            }
                            aws__ = map_.next_value()?;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(ImdsEvent {
                    r#type: r#type__.unwrap_or_default(),
                    cloud_provider: cloud_provider__.unwrap_or_default(),
                    url: url__.unwrap_or_default(),
                    host: host__.unwrap_or_default(),
                    user_agent: user_agent__.unwrap_or_default(),
                    server: server__.unwrap_or_default(),
                    aws: aws__,
                })
            }
        }
        deserializer.deserialize_struct("datadog.cws.dumpsv1.IMDSEvent", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for ImdsNode {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.matched_rules.is_empty() {
            len += 1;
        }
        if !self.image_tags.is_empty() {
            len += 1;
        }
        if self.node_base.is_some() {
            len += 1;
        }
        if self.event.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.cws.dumpsv1.IMDSNode", len)?;
        if !self.matched_rules.is_empty() {
            struct_ser.serialize_field("matchedRules", &self.matched_rules)?;
        }
        if !self.image_tags.is_empty() {
            struct_ser.serialize_field("imageTags", &self.image_tags)?;
        }
        if let Some(v) = self.node_base.as_ref() {
            struct_ser.serialize_field("nodeBase", v)?;
        }
        if let Some(v) = self.event.as_ref() {
            struct_ser.serialize_field("event", v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for ImdsNode {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "matched_rules",
            "matchedRules",
            "image_tags",
            "imageTags",
            "node_base",
            "nodeBase",
            "event",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            MatchedRules,
            ImageTags,
            NodeBase,
            Event,
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
                            "matchedRules" | "matched_rules" => Ok(GeneratedField::MatchedRules),
                            "imageTags" | "image_tags" => Ok(GeneratedField::ImageTags),
                            "nodeBase" | "node_base" => Ok(GeneratedField::NodeBase),
                            "event" => Ok(GeneratedField::Event),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = ImdsNode;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.cws.dumpsv1.IMDSNode")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<ImdsNode, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut matched_rules__ = None;
                let mut image_tags__ = None;
                let mut node_base__ = None;
                let mut event__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::MatchedRules => {
                            if matched_rules__.is_some() {
                                return Err(serde::de::Error::duplicate_field("matchedRules"));
                            }
                            matched_rules__ = Some(map_.next_value()?);
                        }
                        GeneratedField::ImageTags => {
                            if image_tags__.is_some() {
                                return Err(serde::de::Error::duplicate_field("imageTags"));
                            }
                            image_tags__ = Some(map_.next_value()?);
                        }
                        GeneratedField::NodeBase => {
                            if node_base__.is_some() {
                                return Err(serde::de::Error::duplicate_field("nodeBase"));
                            }
                            node_base__ = map_.next_value()?;
                        }
                        GeneratedField::Event => {
                            if event__.is_some() {
                                return Err(serde::de::Error::duplicate_field("event"));
                            }
                            event__ = map_.next_value()?;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(ImdsNode {
                    matched_rules: matched_rules__.unwrap_or_default(),
                    image_tags: image_tags__.unwrap_or_default(),
                    node_base: node_base__,
                    event: event__,
                })
            }
        }
        deserializer.deserialize_struct("datadog.cws.dumpsv1.IMDSNode", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for IpPortContext {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.ip.is_empty() {
            len += 1;
        }
        if self.port != 0 {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.cws.dumpsv1.IPPortContext", len)?;
        if !self.ip.is_empty() {
            struct_ser.serialize_field("ip", &self.ip)?;
        }
        if self.port != 0 {
            struct_ser.serialize_field("port", &self.port)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for IpPortContext {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "ip",
            "port",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Ip,
            Port,
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
                            "ip" => Ok(GeneratedField::Ip),
                            "port" => Ok(GeneratedField::Port),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = IpPortContext;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.cws.dumpsv1.IPPortContext")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<IpPortContext, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut ip__ = None;
                let mut port__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Ip => {
                            if ip__.is_some() {
                                return Err(serde::de::Error::duplicate_field("ip"));
                            }
                            ip__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Port => {
                            if port__.is_some() {
                                return Err(serde::de::Error::duplicate_field("port"));
                            }
                            port__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(IpPortContext {
                    ip: ip__.unwrap_or_default(),
                    port: port__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.cws.dumpsv1.IPPortContext", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for ImageTagTimes {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.first_seen != 0 {
            len += 1;
        }
        if self.last_seen != 0 {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.cws.dumpsv1.ImageTagTimes", len)?;
        if self.first_seen != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("firstSeen", ToString::to_string(&self.first_seen).as_str())?;
        }
        if self.last_seen != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("lastSeen", ToString::to_string(&self.last_seen).as_str())?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for ImageTagTimes {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "first_seen",
            "firstSeen",
            "last_seen",
            "lastSeen",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            FirstSeen,
            LastSeen,
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
                            "firstSeen" | "first_seen" => Ok(GeneratedField::FirstSeen),
                            "lastSeen" | "last_seen" => Ok(GeneratedField::LastSeen),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = ImageTagTimes;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.cws.dumpsv1.ImageTagTimes")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<ImageTagTimes, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut first_seen__ = None;
                let mut last_seen__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::FirstSeen => {
                            if first_seen__.is_some() {
                                return Err(serde::de::Error::duplicate_field("firstSeen"));
                            }
                            first_seen__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::LastSeen => {
                            if last_seen__.is_some() {
                                return Err(serde::de::Error::duplicate_field("lastSeen"));
                            }
                            last_seen__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(ImageTagTimes {
                    first_seen: first_seen__.unwrap_or_default(),
                    last_seen: last_seen__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.cws.dumpsv1.ImageTagTimes", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for MatchedRule {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.rule_id.is_empty() {
            len += 1;
        }
        if !self.rule_version.is_empty() {
            len += 1;
        }
        if !self.policy_name.is_empty() {
            len += 1;
        }
        if !self.policy_version.is_empty() {
            len += 1;
        }
        if !self.rule_tags.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.cws.dumpsv1.MatchedRule", len)?;
        if !self.rule_id.is_empty() {
            struct_ser.serialize_field("ruleId", &self.rule_id)?;
        }
        if !self.rule_version.is_empty() {
            struct_ser.serialize_field("ruleVersion", &self.rule_version)?;
        }
        if !self.policy_name.is_empty() {
            struct_ser.serialize_field("policyName", &self.policy_name)?;
        }
        if !self.policy_version.is_empty() {
            struct_ser.serialize_field("policyVersion", &self.policy_version)?;
        }
        if !self.rule_tags.is_empty() {
            struct_ser.serialize_field("ruleTags", &self.rule_tags)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for MatchedRule {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "rule_id",
            "ruleId",
            "rule_version",
            "ruleVersion",
            "policy_name",
            "policyName",
            "policy_version",
            "policyVersion",
            "rule_tags",
            "ruleTags",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            RuleId,
            RuleVersion,
            PolicyName,
            PolicyVersion,
            RuleTags,
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
                            "ruleId" | "rule_id" => Ok(GeneratedField::RuleId),
                            "ruleVersion" | "rule_version" => Ok(GeneratedField::RuleVersion),
                            "policyName" | "policy_name" => Ok(GeneratedField::PolicyName),
                            "policyVersion" | "policy_version" => Ok(GeneratedField::PolicyVersion),
                            "ruleTags" | "rule_tags" => Ok(GeneratedField::RuleTags),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = MatchedRule;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.cws.dumpsv1.MatchedRule")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<MatchedRule, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut rule_id__ = None;
                let mut rule_version__ = None;
                let mut policy_name__ = None;
                let mut policy_version__ = None;
                let mut rule_tags__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::RuleId => {
                            if rule_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("ruleId"));
                            }
                            rule_id__ = Some(map_.next_value()?);
                        }
                        GeneratedField::RuleVersion => {
                            if rule_version__.is_some() {
                                return Err(serde::de::Error::duplicate_field("ruleVersion"));
                            }
                            rule_version__ = Some(map_.next_value()?);
                        }
                        GeneratedField::PolicyName => {
                            if policy_name__.is_some() {
                                return Err(serde::de::Error::duplicate_field("policyName"));
                            }
                            policy_name__ = Some(map_.next_value()?);
                        }
                        GeneratedField::PolicyVersion => {
                            if policy_version__.is_some() {
                                return Err(serde::de::Error::duplicate_field("policyVersion"));
                            }
                            policy_version__ = Some(map_.next_value()?);
                        }
                        GeneratedField::RuleTags => {
                            if rule_tags__.is_some() {
                                return Err(serde::de::Error::duplicate_field("ruleTags"));
                            }
                            rule_tags__ = Some(
                                map_.next_value::<std::collections::HashMap<_, _>>()?
                            );
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(MatchedRule {
                    rule_id: rule_id__.unwrap_or_default(),
                    rule_version: rule_version__.unwrap_or_default(),
                    policy_name: policy_name__.unwrap_or_default(),
                    policy_version: policy_version__.unwrap_or_default(),
                    rule_tags: rule_tags__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.cws.dumpsv1.MatchedRule", FIELDS, GeneratedVisitor)
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
        if !self.agent_version.is_empty() {
            len += 1;
        }
        if !self.agent_commit.is_empty() {
            len += 1;
        }
        if !self.kernel_version.is_empty() {
            len += 1;
        }
        if !self.linux_distribution.is_empty() {
            len += 1;
        }
        if !self.arch.is_empty() {
            len += 1;
        }
        if !self.name.is_empty() {
            len += 1;
        }
        if !self.protobuf_version.is_empty() {
            len += 1;
        }
        if self.differentiate_args {
            len += 1;
        }
        if !self.comm.is_empty() {
            len += 1;
        }
        if !self.container_id.is_empty() {
            len += 1;
        }
        if self.start != 0 {
            len += 1;
        }
        if self.end != 0 {
            len += 1;
        }
        if self.size != 0 {
            len += 1;
        }
        if !self.serialization.is_empty() {
            len += 1;
        }
        if !self.cgroup_id.is_empty() {
            len += 1;
        }
        if !self.cgroup_manager.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.cws.dumpsv1.Metadata", len)?;
        if !self.agent_version.is_empty() {
            struct_ser.serialize_field("agentVersion", &self.agent_version)?;
        }
        if !self.agent_commit.is_empty() {
            struct_ser.serialize_field("agentCommit", &self.agent_commit)?;
        }
        if !self.kernel_version.is_empty() {
            struct_ser.serialize_field("kernelVersion", &self.kernel_version)?;
        }
        if !self.linux_distribution.is_empty() {
            struct_ser.serialize_field("linuxDistribution", &self.linux_distribution)?;
        }
        if !self.arch.is_empty() {
            struct_ser.serialize_field("arch", &self.arch)?;
        }
        if !self.name.is_empty() {
            struct_ser.serialize_field("name", &self.name)?;
        }
        if !self.protobuf_version.is_empty() {
            struct_ser.serialize_field("protobufVersion", &self.protobuf_version)?;
        }
        if self.differentiate_args {
            struct_ser.serialize_field("differentiateArgs", &self.differentiate_args)?;
        }
        if !self.comm.is_empty() {
            struct_ser.serialize_field("comm", &self.comm)?;
        }
        if !self.container_id.is_empty() {
            struct_ser.serialize_field("containerId", &self.container_id)?;
        }
        if self.start != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("start", ToString::to_string(&self.start).as_str())?;
        }
        if self.end != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("end", ToString::to_string(&self.end).as_str())?;
        }
        if self.size != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("size", ToString::to_string(&self.size).as_str())?;
        }
        if !self.serialization.is_empty() {
            struct_ser.serialize_field("serialization", &self.serialization)?;
        }
        if !self.cgroup_id.is_empty() {
            struct_ser.serialize_field("cgroupId", &self.cgroup_id)?;
        }
        if !self.cgroup_manager.is_empty() {
            struct_ser.serialize_field("cgroupManager", &self.cgroup_manager)?;
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
            "agent_version",
            "agentVersion",
            "agent_commit",
            "agentCommit",
            "kernel_version",
            "kernelVersion",
            "linux_distribution",
            "linuxDistribution",
            "arch",
            "name",
            "protobuf_version",
            "protobufVersion",
            "differentiate_args",
            "differentiateArgs",
            "comm",
            "container_id",
            "containerId",
            "start",
            "end",
            "size",
            "serialization",
            "cgroup_id",
            "cgroupId",
            "cgroup_manager",
            "cgroupManager",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            AgentVersion,
            AgentCommit,
            KernelVersion,
            LinuxDistribution,
            Arch,
            Name,
            ProtobufVersion,
            DifferentiateArgs,
            Comm,
            ContainerId,
            Start,
            End,
            Size,
            Serialization,
            CgroupId,
            CgroupManager,
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
                            "agentVersion" | "agent_version" => Ok(GeneratedField::AgentVersion),
                            "agentCommit" | "agent_commit" => Ok(GeneratedField::AgentCommit),
                            "kernelVersion" | "kernel_version" => Ok(GeneratedField::KernelVersion),
                            "linuxDistribution" | "linux_distribution" => Ok(GeneratedField::LinuxDistribution),
                            "arch" => Ok(GeneratedField::Arch),
                            "name" => Ok(GeneratedField::Name),
                            "protobufVersion" | "protobuf_version" => Ok(GeneratedField::ProtobufVersion),
                            "differentiateArgs" | "differentiate_args" => Ok(GeneratedField::DifferentiateArgs),
                            "comm" => Ok(GeneratedField::Comm),
                            "containerId" | "container_id" => Ok(GeneratedField::ContainerId),
                            "start" => Ok(GeneratedField::Start),
                            "end" => Ok(GeneratedField::End),
                            "size" => Ok(GeneratedField::Size),
                            "serialization" => Ok(GeneratedField::Serialization),
                            "cgroupId" | "cgroup_id" => Ok(GeneratedField::CgroupId),
                            "cgroupManager" | "cgroup_manager" => Ok(GeneratedField::CgroupManager),
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
                formatter.write_str("struct datadog.cws.dumpsv1.Metadata")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<Metadata, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut agent_version__ = None;
                let mut agent_commit__ = None;
                let mut kernel_version__ = None;
                let mut linux_distribution__ = None;
                let mut arch__ = None;
                let mut name__ = None;
                let mut protobuf_version__ = None;
                let mut differentiate_args__ = None;
                let mut comm__ = None;
                let mut container_id__ = None;
                let mut start__ = None;
                let mut end__ = None;
                let mut size__ = None;
                let mut serialization__ = None;
                let mut cgroup_id__ = None;
                let mut cgroup_manager__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::AgentVersion => {
                            if agent_version__.is_some() {
                                return Err(serde::de::Error::duplicate_field("agentVersion"));
                            }
                            agent_version__ = Some(map_.next_value()?);
                        }
                        GeneratedField::AgentCommit => {
                            if agent_commit__.is_some() {
                                return Err(serde::de::Error::duplicate_field("agentCommit"));
                            }
                            agent_commit__ = Some(map_.next_value()?);
                        }
                        GeneratedField::KernelVersion => {
                            if kernel_version__.is_some() {
                                return Err(serde::de::Error::duplicate_field("kernelVersion"));
                            }
                            kernel_version__ = Some(map_.next_value()?);
                        }
                        GeneratedField::LinuxDistribution => {
                            if linux_distribution__.is_some() {
                                return Err(serde::de::Error::duplicate_field("linuxDistribution"));
                            }
                            linux_distribution__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Arch => {
                            if arch__.is_some() {
                                return Err(serde::de::Error::duplicate_field("arch"));
                            }
                            arch__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Name => {
                            if name__.is_some() {
                                return Err(serde::de::Error::duplicate_field("name"));
                            }
                            name__ = Some(map_.next_value()?);
                        }
                        GeneratedField::ProtobufVersion => {
                            if protobuf_version__.is_some() {
                                return Err(serde::de::Error::duplicate_field("protobufVersion"));
                            }
                            protobuf_version__ = Some(map_.next_value()?);
                        }
                        GeneratedField::DifferentiateArgs => {
                            if differentiate_args__.is_some() {
                                return Err(serde::de::Error::duplicate_field("differentiateArgs"));
                            }
                            differentiate_args__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Comm => {
                            if comm__.is_some() {
                                return Err(serde::de::Error::duplicate_field("comm"));
                            }
                            comm__ = Some(map_.next_value()?);
                        }
                        GeneratedField::ContainerId => {
                            if container_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("containerId"));
                            }
                            container_id__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Start => {
                            if start__.is_some() {
                                return Err(serde::de::Error::duplicate_field("start"));
                            }
                            start__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::End => {
                            if end__.is_some() {
                                return Err(serde::de::Error::duplicate_field("end"));
                            }
                            end__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Size => {
                            if size__.is_some() {
                                return Err(serde::de::Error::duplicate_field("size"));
                            }
                            size__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Serialization => {
                            if serialization__.is_some() {
                                return Err(serde::de::Error::duplicate_field("serialization"));
                            }
                            serialization__ = Some(map_.next_value()?);
                        }
                        GeneratedField::CgroupId => {
                            if cgroup_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("cgroupId"));
                            }
                            cgroup_id__ = Some(map_.next_value()?);
                        }
                        GeneratedField::CgroupManager => {
                            if cgroup_manager__.is_some() {
                                return Err(serde::de::Error::duplicate_field("cgroupManager"));
                            }
                            cgroup_manager__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(Metadata {
                    agent_version: agent_version__.unwrap_or_default(),
                    agent_commit: agent_commit__.unwrap_or_default(),
                    kernel_version: kernel_version__.unwrap_or_default(),
                    linux_distribution: linux_distribution__.unwrap_or_default(),
                    arch: arch__.unwrap_or_default(),
                    name: name__.unwrap_or_default(),
                    protobuf_version: protobuf_version__.unwrap_or_default(),
                    differentiate_args: differentiate_args__.unwrap_or_default(),
                    comm: comm__.unwrap_or_default(),
                    container_id: container_id__.unwrap_or_default(),
                    start: start__.unwrap_or_default(),
                    end: end__.unwrap_or_default(),
                    size: size__.unwrap_or_default(),
                    serialization: serialization__.unwrap_or_default(),
                    cgroup_id: cgroup_id__.unwrap_or_default(),
                    cgroup_manager: cgroup_manager__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.cws.dumpsv1.Metadata", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for NetworkDeviceNode {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.matched_rules.is_empty() {
            len += 1;
        }
        if self.netns != 0 {
            len += 1;
        }
        if self.ifindex != 0 {
            len += 1;
        }
        if !self.ifname.is_empty() {
            len += 1;
        }
        if !self.flow_nodes.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.cws.dumpsv1.NetworkDeviceNode", len)?;
        if !self.matched_rules.is_empty() {
            struct_ser.serialize_field("matchedRules", &self.matched_rules)?;
        }
        if self.netns != 0 {
            struct_ser.serialize_field("netns", &self.netns)?;
        }
        if self.ifindex != 0 {
            struct_ser.serialize_field("ifindex", &self.ifindex)?;
        }
        if !self.ifname.is_empty() {
            struct_ser.serialize_field("ifname", &self.ifname)?;
        }
        if !self.flow_nodes.is_empty() {
            struct_ser.serialize_field("flowNodes", &self.flow_nodes)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for NetworkDeviceNode {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "matched_rules",
            "matchedRules",
            "netns",
            "ifindex",
            "ifname",
            "flow_nodes",
            "flowNodes",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            MatchedRules,
            Netns,
            Ifindex,
            Ifname,
            FlowNodes,
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
                            "matchedRules" | "matched_rules" => Ok(GeneratedField::MatchedRules),
                            "netns" => Ok(GeneratedField::Netns),
                            "ifindex" => Ok(GeneratedField::Ifindex),
                            "ifname" => Ok(GeneratedField::Ifname),
                            "flowNodes" | "flow_nodes" => Ok(GeneratedField::FlowNodes),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = NetworkDeviceNode;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.cws.dumpsv1.NetworkDeviceNode")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<NetworkDeviceNode, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut matched_rules__ = None;
                let mut netns__ = None;
                let mut ifindex__ = None;
                let mut ifname__ = None;
                let mut flow_nodes__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::MatchedRules => {
                            if matched_rules__.is_some() {
                                return Err(serde::de::Error::duplicate_field("matchedRules"));
                            }
                            matched_rules__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Netns => {
                            if netns__.is_some() {
                                return Err(serde::de::Error::duplicate_field("netns"));
                            }
                            netns__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Ifindex => {
                            if ifindex__.is_some() {
                                return Err(serde::de::Error::duplicate_field("ifindex"));
                            }
                            ifindex__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Ifname => {
                            if ifname__.is_some() {
                                return Err(serde::de::Error::duplicate_field("ifname"));
                            }
                            ifname__ = Some(map_.next_value()?);
                        }
                        GeneratedField::FlowNodes => {
                            if flow_nodes__.is_some() {
                                return Err(serde::de::Error::duplicate_field("flowNodes"));
                            }
                            flow_nodes__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(NetworkDeviceNode {
                    matched_rules: matched_rules__.unwrap_or_default(),
                    netns: netns__.unwrap_or_default(),
                    ifindex: ifindex__.unwrap_or_default(),
                    ifname: ifname__.unwrap_or_default(),
                    flow_nodes: flow_nodes__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.cws.dumpsv1.NetworkDeviceNode", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for NetworkStats {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.data_size != 0 {
            len += 1;
        }
        if self.packet_count != 0 {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.cws.dumpsv1.NetworkStats", len)?;
        if self.data_size != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("dataSize", ToString::to_string(&self.data_size).as_str())?;
        }
        if self.packet_count != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("packetCount", ToString::to_string(&self.packet_count).as_str())?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for NetworkStats {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "data_size",
            "dataSize",
            "packet_count",
            "packetCount",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            DataSize,
            PacketCount,
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
                            "dataSize" | "data_size" => Ok(GeneratedField::DataSize),
                            "packetCount" | "packet_count" => Ok(GeneratedField::PacketCount),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = NetworkStats;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.cws.dumpsv1.NetworkStats")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<NetworkStats, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut data_size__ = None;
                let mut packet_count__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::DataSize => {
                            if data_size__.is_some() {
                                return Err(serde::de::Error::duplicate_field("dataSize"));
                            }
                            data_size__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::PacketCount => {
                            if packet_count__.is_some() {
                                return Err(serde::de::Error::duplicate_field("packetCount"));
                            }
                            packet_count__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(NetworkStats {
                    data_size: data_size__.unwrap_or_default(),
                    packet_count: packet_count__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.cws.dumpsv1.NetworkStats", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for NodeBase {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.seen.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.cws.dumpsv1.NodeBase", len)?;
        if !self.seen.is_empty() {
            struct_ser.serialize_field("seen", &self.seen)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for NodeBase {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "seen",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Seen,
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
                            "seen" => Ok(GeneratedField::Seen),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = NodeBase;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.cws.dumpsv1.NodeBase")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<NodeBase, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut seen__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Seen => {
                            if seen__.is_some() {
                                return Err(serde::de::Error::duplicate_field("seen"));
                            }
                            seen__ = Some(
                                map_.next_value::<std::collections::HashMap<_, _>>()?
                            );
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(NodeBase {
                    seen: seen__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.cws.dumpsv1.NodeBase", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for OpenNode {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.retval != 0 {
            len += 1;
        }
        if self.flags != 0 {
            len += 1;
        }
        if self.mode != 0 {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.cws.dumpsv1.OpenNode", len)?;
        if self.retval != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("retval", ToString::to_string(&self.retval).as_str())?;
        }
        if self.flags != 0 {
            struct_ser.serialize_field("flags", &self.flags)?;
        }
        if self.mode != 0 {
            struct_ser.serialize_field("mode", &self.mode)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for OpenNode {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "retval",
            "flags",
            "mode",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Retval,
            Flags,
            Mode,
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
                            "retval" => Ok(GeneratedField::Retval),
                            "flags" => Ok(GeneratedField::Flags),
                            "mode" => Ok(GeneratedField::Mode),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = OpenNode;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.cws.dumpsv1.OpenNode")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<OpenNode, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut retval__ = None;
                let mut flags__ = None;
                let mut mode__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Retval => {
                            if retval__.is_some() {
                                return Err(serde::de::Error::duplicate_field("retval"));
                            }
                            retval__ = 
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
                        GeneratedField::Mode => {
                            if mode__.is_some() {
                                return Err(serde::de::Error::duplicate_field("mode"));
                            }
                            mode__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(OpenNode {
                    retval: retval__.unwrap_or_default(),
                    flags: flags__.unwrap_or_default(),
                    mode: mode__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.cws.dumpsv1.OpenNode", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for ProcessActivityNode {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.process.is_some() {
            len += 1;
        }
        if self.generation_type != 0 {
            len += 1;
        }
        if !self.matched_rules.is_empty() {
            len += 1;
        }
        if !self.children.is_empty() {
            len += 1;
        }
        if !self.files.is_empty() {
            len += 1;
        }
        if !self.dns_names.is_empty() {
            len += 1;
        }
        if !self.sockets.is_empty() {
            len += 1;
        }
        if !self.syscalls.is_empty() {
            len += 1;
        }
        if !self.image_tags.is_empty() {
            len += 1;
        }
        if self.node_base.is_some() {
            len += 1;
        }
        if !self.imds_events.is_empty() {
            len += 1;
        }
        if !self.syscall_nodes.is_empty() {
            len += 1;
        }
        if !self.network_devices.is_empty() {
            len += 1;
        }
        if !self.capability_nodes.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.cws.dumpsv1.ProcessActivityNode", len)?;
        if let Some(v) = self.process.as_ref() {
            struct_ser.serialize_field("process", v)?;
        }
        if self.generation_type != 0 {
            let v = GenerationType::try_from(self.generation_type)
                .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", self.generation_type)))?;
            struct_ser.serialize_field("generationType", &v)?;
        }
        if !self.matched_rules.is_empty() {
            struct_ser.serialize_field("matchedRules", &self.matched_rules)?;
        }
        if !self.children.is_empty() {
            struct_ser.serialize_field("children", &self.children)?;
        }
        if !self.files.is_empty() {
            struct_ser.serialize_field("files", &self.files)?;
        }
        if !self.dns_names.is_empty() {
            struct_ser.serialize_field("dnsNames", &self.dns_names)?;
        }
        if !self.sockets.is_empty() {
            struct_ser.serialize_field("sockets", &self.sockets)?;
        }
        if !self.syscalls.is_empty() {
            struct_ser.serialize_field("syscalls", &self.syscalls)?;
        }
        if !self.image_tags.is_empty() {
            struct_ser.serialize_field("imageTags", &self.image_tags)?;
        }
        if let Some(v) = self.node_base.as_ref() {
            struct_ser.serialize_field("nodeBase", v)?;
        }
        if !self.imds_events.is_empty() {
            struct_ser.serialize_field("imdsEvents", &self.imds_events)?;
        }
        if !self.syscall_nodes.is_empty() {
            struct_ser.serialize_field("syscallNodes", &self.syscall_nodes)?;
        }
        if !self.network_devices.is_empty() {
            struct_ser.serialize_field("networkDevices", &self.network_devices)?;
        }
        if !self.capability_nodes.is_empty() {
            struct_ser.serialize_field("capabilityNodes", &self.capability_nodes)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for ProcessActivityNode {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "process",
            "generation_type",
            "generationType",
            "matched_rules",
            "matchedRules",
            "children",
            "files",
            "dns_names",
            "dnsNames",
            "sockets",
            "syscalls",
            "image_tags",
            "imageTags",
            "node_base",
            "nodeBase",
            "imds_events",
            "imdsEvents",
            "syscall_nodes",
            "syscallNodes",
            "network_devices",
            "networkDevices",
            "capability_nodes",
            "capabilityNodes",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Process,
            GenerationType,
            MatchedRules,
            Children,
            Files,
            DnsNames,
            Sockets,
            Syscalls,
            ImageTags,
            NodeBase,
            ImdsEvents,
            SyscallNodes,
            NetworkDevices,
            CapabilityNodes,
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
                            "process" => Ok(GeneratedField::Process),
                            "generationType" | "generation_type" => Ok(GeneratedField::GenerationType),
                            "matchedRules" | "matched_rules" => Ok(GeneratedField::MatchedRules),
                            "children" => Ok(GeneratedField::Children),
                            "files" => Ok(GeneratedField::Files),
                            "dnsNames" | "dns_names" => Ok(GeneratedField::DnsNames),
                            "sockets" => Ok(GeneratedField::Sockets),
                            "syscalls" => Ok(GeneratedField::Syscalls),
                            "imageTags" | "image_tags" => Ok(GeneratedField::ImageTags),
                            "nodeBase" | "node_base" => Ok(GeneratedField::NodeBase),
                            "imdsEvents" | "imds_events" => Ok(GeneratedField::ImdsEvents),
                            "syscallNodes" | "syscall_nodes" => Ok(GeneratedField::SyscallNodes),
                            "networkDevices" | "network_devices" => Ok(GeneratedField::NetworkDevices),
                            "capabilityNodes" | "capability_nodes" => Ok(GeneratedField::CapabilityNodes),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = ProcessActivityNode;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.cws.dumpsv1.ProcessActivityNode")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<ProcessActivityNode, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut process__ = None;
                let mut generation_type__ = None;
                let mut matched_rules__ = None;
                let mut children__ = None;
                let mut files__ = None;
                let mut dns_names__ = None;
                let mut sockets__ = None;
                let mut syscalls__ = None;
                let mut image_tags__ = None;
                let mut node_base__ = None;
                let mut imds_events__ = None;
                let mut syscall_nodes__ = None;
                let mut network_devices__ = None;
                let mut capability_nodes__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Process => {
                            if process__.is_some() {
                                return Err(serde::de::Error::duplicate_field("process"));
                            }
                            process__ = map_.next_value()?;
                        }
                        GeneratedField::GenerationType => {
                            if generation_type__.is_some() {
                                return Err(serde::de::Error::duplicate_field("generationType"));
                            }
                            generation_type__ = Some(map_.next_value::<GenerationType>()? as i32);
                        }
                        GeneratedField::MatchedRules => {
                            if matched_rules__.is_some() {
                                return Err(serde::de::Error::duplicate_field("matchedRules"));
                            }
                            matched_rules__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Children => {
                            if children__.is_some() {
                                return Err(serde::de::Error::duplicate_field("children"));
                            }
                            children__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Files => {
                            if files__.is_some() {
                                return Err(serde::de::Error::duplicate_field("files"));
                            }
                            files__ = Some(map_.next_value()?);
                        }
                        GeneratedField::DnsNames => {
                            if dns_names__.is_some() {
                                return Err(serde::de::Error::duplicate_field("dnsNames"));
                            }
                            dns_names__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Sockets => {
                            if sockets__.is_some() {
                                return Err(serde::de::Error::duplicate_field("sockets"));
                            }
                            sockets__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Syscalls => {
                            if syscalls__.is_some() {
                                return Err(serde::de::Error::duplicate_field("syscalls"));
                            }
                            syscalls__ = 
                                Some(map_.next_value::<Vec<::pbjson::private::NumberDeserialize<_>>>()?
                                    .into_iter().map(|x| x.0).collect())
                            ;
                        }
                        GeneratedField::ImageTags => {
                            if image_tags__.is_some() {
                                return Err(serde::de::Error::duplicate_field("imageTags"));
                            }
                            image_tags__ = Some(map_.next_value()?);
                        }
                        GeneratedField::NodeBase => {
                            if node_base__.is_some() {
                                return Err(serde::de::Error::duplicate_field("nodeBase"));
                            }
                            node_base__ = map_.next_value()?;
                        }
                        GeneratedField::ImdsEvents => {
                            if imds_events__.is_some() {
                                return Err(serde::de::Error::duplicate_field("imdsEvents"));
                            }
                            imds_events__ = Some(map_.next_value()?);
                        }
                        GeneratedField::SyscallNodes => {
                            if syscall_nodes__.is_some() {
                                return Err(serde::de::Error::duplicate_field("syscallNodes"));
                            }
                            syscall_nodes__ = Some(map_.next_value()?);
                        }
                        GeneratedField::NetworkDevices => {
                            if network_devices__.is_some() {
                                return Err(serde::de::Error::duplicate_field("networkDevices"));
                            }
                            network_devices__ = Some(map_.next_value()?);
                        }
                        GeneratedField::CapabilityNodes => {
                            if capability_nodes__.is_some() {
                                return Err(serde::de::Error::duplicate_field("capabilityNodes"));
                            }
                            capability_nodes__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(ProcessActivityNode {
                    process: process__,
                    generation_type: generation_type__.unwrap_or_default(),
                    matched_rules: matched_rules__.unwrap_or_default(),
                    children: children__.unwrap_or_default(),
                    files: files__.unwrap_or_default(),
                    dns_names: dns_names__.unwrap_or_default(),
                    sockets: sockets__.unwrap_or_default(),
                    syscalls: syscalls__.unwrap_or_default(),
                    image_tags: image_tags__.unwrap_or_default(),
                    node_base: node_base__,
                    imds_events: imds_events__.unwrap_or_default(),
                    syscall_nodes: syscall_nodes__.unwrap_or_default(),
                    network_devices: network_devices__.unwrap_or_default(),
                    capability_nodes: capability_nodes__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.cws.dumpsv1.ProcessActivityNode", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for ProcessInfo {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.pid != 0 {
            len += 1;
        }
        if self.tid != 0 {
            len += 1;
        }
        if self.ppid != 0 {
            len += 1;
        }
        if self.cookie != 0 {
            len += 1;
        }
        if self.is_thread {
            len += 1;
        }
        if self.file.is_some() {
            len += 1;
        }
        if !self.container_id.is_empty() {
            len += 1;
        }
        if self.span_id != 0 {
            len += 1;
        }
        if self.trace_id != 0 {
            len += 1;
        }
        if !self.tty.is_empty() {
            len += 1;
        }
        if !self.comm.is_empty() {
            len += 1;
        }
        if self.fork_time != 0 {
            len += 1;
        }
        if self.exit_time != 0 {
            len += 1;
        }
        if self.exec_time != 0 {
            len += 1;
        }
        if self.credentials.is_some() {
            len += 1;
        }
        if !self.args.is_empty() {
            len += 1;
        }
        if !self.argv0.is_empty() {
            len += 1;
        }
        if self.args_truncated {
            len += 1;
        }
        if !self.envs.is_empty() {
            len += 1;
        }
        if self.envs_truncated {
            len += 1;
        }
        if self.is_exec_child {
            len += 1;
        }
        if self.cookie64 != 0 {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.cws.dumpsv1.ProcessInfo", len)?;
        if self.pid != 0 {
            struct_ser.serialize_field("pid", &self.pid)?;
        }
        if self.tid != 0 {
            struct_ser.serialize_field("tid", &self.tid)?;
        }
        if self.ppid != 0 {
            struct_ser.serialize_field("ppid", &self.ppid)?;
        }
        if self.cookie != 0 {
            struct_ser.serialize_field("cookie", &self.cookie)?;
        }
        if self.is_thread {
            struct_ser.serialize_field("isThread", &self.is_thread)?;
        }
        if let Some(v) = self.file.as_ref() {
            struct_ser.serialize_field("file", v)?;
        }
        if !self.container_id.is_empty() {
            struct_ser.serialize_field("containerId", &self.container_id)?;
        }
        if self.span_id != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("spanId", ToString::to_string(&self.span_id).as_str())?;
        }
        if self.trace_id != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("traceId", ToString::to_string(&self.trace_id).as_str())?;
        }
        if !self.tty.is_empty() {
            struct_ser.serialize_field("tty", &self.tty)?;
        }
        if !self.comm.is_empty() {
            struct_ser.serialize_field("comm", &self.comm)?;
        }
        if self.fork_time != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("forkTime", ToString::to_string(&self.fork_time).as_str())?;
        }
        if self.exit_time != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("exitTime", ToString::to_string(&self.exit_time).as_str())?;
        }
        if self.exec_time != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("execTime", ToString::to_string(&self.exec_time).as_str())?;
        }
        if let Some(v) = self.credentials.as_ref() {
            struct_ser.serialize_field("credentials", v)?;
        }
        if !self.args.is_empty() {
            struct_ser.serialize_field("args", &self.args)?;
        }
        if !self.argv0.is_empty() {
            struct_ser.serialize_field("argv0", &self.argv0)?;
        }
        if self.args_truncated {
            struct_ser.serialize_field("argsTruncated", &self.args_truncated)?;
        }
        if !self.envs.is_empty() {
            struct_ser.serialize_field("envs", &self.envs)?;
        }
        if self.envs_truncated {
            struct_ser.serialize_field("envsTruncated", &self.envs_truncated)?;
        }
        if self.is_exec_child {
            struct_ser.serialize_field("isExecChild", &self.is_exec_child)?;
        }
        if self.cookie64 != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("cookie64", ToString::to_string(&self.cookie64).as_str())?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for ProcessInfo {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "pid",
            "tid",
            "ppid",
            "cookie",
            "is_thread",
            "isThread",
            "file",
            "container_id",
            "containerId",
            "span_id",
            "spanId",
            "trace_id",
            "traceId",
            "tty",
            "comm",
            "fork_time",
            "forkTime",
            "exit_time",
            "exitTime",
            "exec_time",
            "execTime",
            "credentials",
            "args",
            "argv0",
            "args_truncated",
            "argsTruncated",
            "envs",
            "envs_truncated",
            "envsTruncated",
            "is_exec_child",
            "isExecChild",
            "cookie64",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Pid,
            Tid,
            Ppid,
            Cookie,
            IsThread,
            File,
            ContainerId,
            SpanId,
            TraceId,
            Tty,
            Comm,
            ForkTime,
            ExitTime,
            ExecTime,
            Credentials,
            Args,
            Argv0,
            ArgsTruncated,
            Envs,
            EnvsTruncated,
            IsExecChild,
            Cookie64,
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
                            "pid" => Ok(GeneratedField::Pid),
                            "tid" => Ok(GeneratedField::Tid),
                            "ppid" => Ok(GeneratedField::Ppid),
                            "cookie" => Ok(GeneratedField::Cookie),
                            "isThread" | "is_thread" => Ok(GeneratedField::IsThread),
                            "file" => Ok(GeneratedField::File),
                            "containerId" | "container_id" => Ok(GeneratedField::ContainerId),
                            "spanId" | "span_id" => Ok(GeneratedField::SpanId),
                            "traceId" | "trace_id" => Ok(GeneratedField::TraceId),
                            "tty" => Ok(GeneratedField::Tty),
                            "comm" => Ok(GeneratedField::Comm),
                            "forkTime" | "fork_time" => Ok(GeneratedField::ForkTime),
                            "exitTime" | "exit_time" => Ok(GeneratedField::ExitTime),
                            "execTime" | "exec_time" => Ok(GeneratedField::ExecTime),
                            "credentials" => Ok(GeneratedField::Credentials),
                            "args" => Ok(GeneratedField::Args),
                            "argv0" => Ok(GeneratedField::Argv0),
                            "argsTruncated" | "args_truncated" => Ok(GeneratedField::ArgsTruncated),
                            "envs" => Ok(GeneratedField::Envs),
                            "envsTruncated" | "envs_truncated" => Ok(GeneratedField::EnvsTruncated),
                            "isExecChild" | "is_exec_child" => Ok(GeneratedField::IsExecChild),
                            "cookie64" => Ok(GeneratedField::Cookie64),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = ProcessInfo;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.cws.dumpsv1.ProcessInfo")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<ProcessInfo, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut pid__ = None;
                let mut tid__ = None;
                let mut ppid__ = None;
                let mut cookie__ = None;
                let mut is_thread__ = None;
                let mut file__ = None;
                let mut container_id__ = None;
                let mut span_id__ = None;
                let mut trace_id__ = None;
                let mut tty__ = None;
                let mut comm__ = None;
                let mut fork_time__ = None;
                let mut exit_time__ = None;
                let mut exec_time__ = None;
                let mut credentials__ = None;
                let mut args__ = None;
                let mut argv0__ = None;
                let mut args_truncated__ = None;
                let mut envs__ = None;
                let mut envs_truncated__ = None;
                let mut is_exec_child__ = None;
                let mut cookie64__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Pid => {
                            if pid__.is_some() {
                                return Err(serde::de::Error::duplicate_field("pid"));
                            }
                            pid__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Tid => {
                            if tid__.is_some() {
                                return Err(serde::de::Error::duplicate_field("tid"));
                            }
                            tid__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Ppid => {
                            if ppid__.is_some() {
                                return Err(serde::de::Error::duplicate_field("ppid"));
                            }
                            ppid__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Cookie => {
                            if cookie__.is_some() {
                                return Err(serde::de::Error::duplicate_field("cookie"));
                            }
                            cookie__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::IsThread => {
                            if is_thread__.is_some() {
                                return Err(serde::de::Error::duplicate_field("isThread"));
                            }
                            is_thread__ = Some(map_.next_value()?);
                        }
                        GeneratedField::File => {
                            if file__.is_some() {
                                return Err(serde::de::Error::duplicate_field("file"));
                            }
                            file__ = map_.next_value()?;
                        }
                        GeneratedField::ContainerId => {
                            if container_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("containerId"));
                            }
                            container_id__ = Some(map_.next_value()?);
                        }
                        GeneratedField::SpanId => {
                            if span_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("spanId"));
                            }
                            span_id__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::TraceId => {
                            if trace_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("traceId"));
                            }
                            trace_id__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Tty => {
                            if tty__.is_some() {
                                return Err(serde::de::Error::duplicate_field("tty"));
                            }
                            tty__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Comm => {
                            if comm__.is_some() {
                                return Err(serde::de::Error::duplicate_field("comm"));
                            }
                            comm__ = Some(map_.next_value()?);
                        }
                        GeneratedField::ForkTime => {
                            if fork_time__.is_some() {
                                return Err(serde::de::Error::duplicate_field("forkTime"));
                            }
                            fork_time__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::ExitTime => {
                            if exit_time__.is_some() {
                                return Err(serde::de::Error::duplicate_field("exitTime"));
                            }
                            exit_time__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::ExecTime => {
                            if exec_time__.is_some() {
                                return Err(serde::de::Error::duplicate_field("execTime"));
                            }
                            exec_time__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Credentials => {
                            if credentials__.is_some() {
                                return Err(serde::de::Error::duplicate_field("credentials"));
                            }
                            credentials__ = map_.next_value()?;
                        }
                        GeneratedField::Args => {
                            if args__.is_some() {
                                return Err(serde::de::Error::duplicate_field("args"));
                            }
                            args__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Argv0 => {
                            if argv0__.is_some() {
                                return Err(serde::de::Error::duplicate_field("argv0"));
                            }
                            argv0__ = Some(map_.next_value()?);
                        }
                        GeneratedField::ArgsTruncated => {
                            if args_truncated__.is_some() {
                                return Err(serde::de::Error::duplicate_field("argsTruncated"));
                            }
                            args_truncated__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Envs => {
                            if envs__.is_some() {
                                return Err(serde::de::Error::duplicate_field("envs"));
                            }
                            envs__ = Some(map_.next_value()?);
                        }
                        GeneratedField::EnvsTruncated => {
                            if envs_truncated__.is_some() {
                                return Err(serde::de::Error::duplicate_field("envsTruncated"));
                            }
                            envs_truncated__ = Some(map_.next_value()?);
                        }
                        GeneratedField::IsExecChild => {
                            if is_exec_child__.is_some() {
                                return Err(serde::de::Error::duplicate_field("isExecChild"));
                            }
                            is_exec_child__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Cookie64 => {
                            if cookie64__.is_some() {
                                return Err(serde::de::Error::duplicate_field("cookie64"));
                            }
                            cookie64__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(ProcessInfo {
                    pid: pid__.unwrap_or_default(),
                    tid: tid__.unwrap_or_default(),
                    ppid: ppid__.unwrap_or_default(),
                    cookie: cookie__.unwrap_or_default(),
                    is_thread: is_thread__.unwrap_or_default(),
                    file: file__,
                    container_id: container_id__.unwrap_or_default(),
                    span_id: span_id__.unwrap_or_default(),
                    trace_id: trace_id__.unwrap_or_default(),
                    tty: tty__.unwrap_or_default(),
                    comm: comm__.unwrap_or_default(),
                    fork_time: fork_time__.unwrap_or_default(),
                    exit_time: exit_time__.unwrap_or_default(),
                    exec_time: exec_time__.unwrap_or_default(),
                    credentials: credentials__,
                    args: args__.unwrap_or_default(),
                    argv0: argv0__.unwrap_or_default(),
                    args_truncated: args_truncated__.unwrap_or_default(),
                    envs: envs__.unwrap_or_default(),
                    envs_truncated: envs_truncated__.unwrap_or_default(),
                    is_exec_child: is_exec_child__.unwrap_or_default(),
                    cookie64: cookie64__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.cws.dumpsv1.ProcessInfo", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for ProfileContext {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.first_seen != 0 {
            len += 1;
        }
        if self.last_seen != 0 {
            len += 1;
        }
        if !self.event_type_state.is_empty() {
            len += 1;
        }
        if !self.syscalls.is_empty() {
            len += 1;
        }
        if !self.tags.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.cws.dumpsv1.ProfileContext", len)?;
        if self.first_seen != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("firstSeen", ToString::to_string(&self.first_seen).as_str())?;
        }
        if self.last_seen != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("lastSeen", ToString::to_string(&self.last_seen).as_str())?;
        }
        if !self.event_type_state.is_empty() {
            struct_ser.serialize_field("eventTypeState", &self.event_type_state)?;
        }
        if !self.syscalls.is_empty() {
            struct_ser.serialize_field("syscalls", &self.syscalls)?;
        }
        if !self.tags.is_empty() {
            struct_ser.serialize_field("tags", &self.tags)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for ProfileContext {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "first_seen",
            "firstSeen",
            "last_seen",
            "lastSeen",
            "event_type_state",
            "eventTypeState",
            "syscalls",
            "tags",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            FirstSeen,
            LastSeen,
            EventTypeState,
            Syscalls,
            Tags,
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
                            "firstSeen" | "first_seen" => Ok(GeneratedField::FirstSeen),
                            "lastSeen" | "last_seen" => Ok(GeneratedField::LastSeen),
                            "eventTypeState" | "event_type_state" => Ok(GeneratedField::EventTypeState),
                            "syscalls" => Ok(GeneratedField::Syscalls),
                            "tags" => Ok(GeneratedField::Tags),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = ProfileContext;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.cws.dumpsv1.ProfileContext")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<ProfileContext, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut first_seen__ = None;
                let mut last_seen__ = None;
                let mut event_type_state__ = None;
                let mut syscalls__ = None;
                let mut tags__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::FirstSeen => {
                            if first_seen__.is_some() {
                                return Err(serde::de::Error::duplicate_field("firstSeen"));
                            }
                            first_seen__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::LastSeen => {
                            if last_seen__.is_some() {
                                return Err(serde::de::Error::duplicate_field("lastSeen"));
                            }
                            last_seen__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::EventTypeState => {
                            if event_type_state__.is_some() {
                                return Err(serde::de::Error::duplicate_field("eventTypeState"));
                            }
                            event_type_state__ = Some(
                                map_.next_value::<std::collections::HashMap<::pbjson::private::NumberDeserialize<u32>, _>>()?
                                    .into_iter().map(|(k,v)| (k.0, v)).collect()
                            );
                        }
                        GeneratedField::Syscalls => {
                            if syscalls__.is_some() {
                                return Err(serde::de::Error::duplicate_field("syscalls"));
                            }
                            syscalls__ = 
                                Some(map_.next_value::<Vec<::pbjson::private::NumberDeserialize<_>>>()?
                                    .into_iter().map(|x| x.0).collect())
                            ;
                        }
                        GeneratedField::Tags => {
                            if tags__.is_some() {
                                return Err(serde::de::Error::duplicate_field("tags"));
                            }
                            tags__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(ProfileContext {
                    first_seen: first_seen__.unwrap_or_default(),
                    last_seen: last_seen__.unwrap_or_default(),
                    event_type_state: event_type_state__.unwrap_or_default(),
                    syscalls: syscalls__.unwrap_or_default(),
                    tags: tags__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.cws.dumpsv1.ProfileContext", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for ProfileSelector {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.image_name.is_empty() {
            len += 1;
        }
        if !self.image_tag.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.cws.dumpsv1.ProfileSelector", len)?;
        if !self.image_name.is_empty() {
            struct_ser.serialize_field("imageName", &self.image_name)?;
        }
        if !self.image_tag.is_empty() {
            struct_ser.serialize_field("imageTag", &self.image_tag)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for ProfileSelector {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "image_name",
            "imageName",
            "image_tag",
            "imageTag",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            ImageName,
            ImageTag,
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
                            "imageName" | "image_name" => Ok(GeneratedField::ImageName),
                            "imageTag" | "image_tag" => Ok(GeneratedField::ImageTag),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = ProfileSelector;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.cws.dumpsv1.ProfileSelector")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<ProfileSelector, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut image_name__ = None;
                let mut image_tag__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::ImageName => {
                            if image_name__.is_some() {
                                return Err(serde::de::Error::duplicate_field("imageName"));
                            }
                            image_name__ = Some(map_.next_value()?);
                        }
                        GeneratedField::ImageTag => {
                            if image_tag__.is_some() {
                                return Err(serde::de::Error::duplicate_field("imageTag"));
                            }
                            image_tag__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(ProfileSelector {
                    image_name: image_name__.unwrap_or_default(),
                    image_tag: image_tag__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.cws.dumpsv1.ProfileSelector", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for SecDump {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.host.is_empty() {
            len += 1;
        }
        if !self.service.is_empty() {
            len += 1;
        }
        if !self.source.is_empty() {
            len += 1;
        }
        if self.metadata.is_some() {
            len += 1;
        }
        if !self.tags.is_empty() {
            len += 1;
        }
        if !self.tree.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.cws.dumpsv1.SecDump", len)?;
        if !self.host.is_empty() {
            struct_ser.serialize_field("host", &self.host)?;
        }
        if !self.service.is_empty() {
            struct_ser.serialize_field("service", &self.service)?;
        }
        if !self.source.is_empty() {
            struct_ser.serialize_field("source", &self.source)?;
        }
        if let Some(v) = self.metadata.as_ref() {
            struct_ser.serialize_field("metadata", v)?;
        }
        if !self.tags.is_empty() {
            struct_ser.serialize_field("tags", &self.tags)?;
        }
        if !self.tree.is_empty() {
            struct_ser.serialize_field("tree", &self.tree)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for SecDump {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "host",
            "service",
            "source",
            "metadata",
            "tags",
            "tree",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Host,
            Service,
            Source,
            Metadata,
            Tags,
            Tree,
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
                            "host" => Ok(GeneratedField::Host),
                            "service" => Ok(GeneratedField::Service),
                            "source" => Ok(GeneratedField::Source),
                            "metadata" => Ok(GeneratedField::Metadata),
                            "tags" => Ok(GeneratedField::Tags),
                            "tree" => Ok(GeneratedField::Tree),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = SecDump;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.cws.dumpsv1.SecDump")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<SecDump, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut host__ = None;
                let mut service__ = None;
                let mut source__ = None;
                let mut metadata__ = None;
                let mut tags__ = None;
                let mut tree__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Host => {
                            if host__.is_some() {
                                return Err(serde::de::Error::duplicate_field("host"));
                            }
                            host__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Service => {
                            if service__.is_some() {
                                return Err(serde::de::Error::duplicate_field("service"));
                            }
                            service__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Source => {
                            if source__.is_some() {
                                return Err(serde::de::Error::duplicate_field("source"));
                            }
                            source__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Metadata => {
                            if metadata__.is_some() {
                                return Err(serde::de::Error::duplicate_field("metadata"));
                            }
                            metadata__ = map_.next_value()?;
                        }
                        GeneratedField::Tags => {
                            if tags__.is_some() {
                                return Err(serde::de::Error::duplicate_field("tags"));
                            }
                            tags__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Tree => {
                            if tree__.is_some() {
                                return Err(serde::de::Error::duplicate_field("tree"));
                            }
                            tree__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(SecDump {
                    host: host__.unwrap_or_default(),
                    service: service__.unwrap_or_default(),
                    source: source__.unwrap_or_default(),
                    metadata: metadata__,
                    tags: tags__.unwrap_or_default(),
                    tree: tree__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.cws.dumpsv1.SecDump", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for SecurityProfile {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.status != 0 {
            len += 1;
        }
        if !self.version.is_empty() {
            len += 1;
        }
        if self.metadata.is_some() {
            len += 1;
        }
        if !self.tags.is_empty() {
            len += 1;
        }
        if !self.syscalls.is_empty() {
            len += 1;
        }
        if !self.tree.is_empty() {
            len += 1;
        }
        if !self.profile_contexts.is_empty() {
            len += 1;
        }
        if self.selector.is_some() {
            len += 1;
        }
        if self.disabled {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.cws.dumpsv1.SecurityProfile", len)?;
        if self.status != 0 {
            struct_ser.serialize_field("status", &self.status)?;
        }
        if !self.version.is_empty() {
            struct_ser.serialize_field("version", &self.version)?;
        }
        if let Some(v) = self.metadata.as_ref() {
            struct_ser.serialize_field("metadata", v)?;
        }
        if !self.tags.is_empty() {
            struct_ser.serialize_field("tags", &self.tags)?;
        }
        if !self.syscalls.is_empty() {
            struct_ser.serialize_field("syscalls", &self.syscalls)?;
        }
        if !self.tree.is_empty() {
            struct_ser.serialize_field("tree", &self.tree)?;
        }
        if !self.profile_contexts.is_empty() {
            struct_ser.serialize_field("profileContexts", &self.profile_contexts)?;
        }
        if let Some(v) = self.selector.as_ref() {
            struct_ser.serialize_field("selector", v)?;
        }
        if self.disabled {
            struct_ser.serialize_field("disabled", &self.disabled)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for SecurityProfile {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "status",
            "version",
            "metadata",
            "tags",
            "syscalls",
            "tree",
            "profile_contexts",
            "profileContexts",
            "selector",
            "disabled",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Status,
            Version,
            Metadata,
            Tags,
            Syscalls,
            Tree,
            ProfileContexts,
            Selector,
            Disabled,
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
                            "metadata" => Ok(GeneratedField::Metadata),
                            "tags" => Ok(GeneratedField::Tags),
                            "syscalls" => Ok(GeneratedField::Syscalls),
                            "tree" => Ok(GeneratedField::Tree),
                            "profileContexts" | "profile_contexts" => Ok(GeneratedField::ProfileContexts),
                            "selector" => Ok(GeneratedField::Selector),
                            "disabled" => Ok(GeneratedField::Disabled),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = SecurityProfile;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.cws.dumpsv1.SecurityProfile")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<SecurityProfile, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut status__ = None;
                let mut version__ = None;
                let mut metadata__ = None;
                let mut tags__ = None;
                let mut syscalls__ = None;
                let mut tree__ = None;
                let mut profile_contexts__ = None;
                let mut selector__ = None;
                let mut disabled__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Status => {
                            if status__.is_some() {
                                return Err(serde::de::Error::duplicate_field("status"));
                            }
                            status__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Version => {
                            if version__.is_some() {
                                return Err(serde::de::Error::duplicate_field("version"));
                            }
                            version__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Metadata => {
                            if metadata__.is_some() {
                                return Err(serde::de::Error::duplicate_field("metadata"));
                            }
                            metadata__ = map_.next_value()?;
                        }
                        GeneratedField::Tags => {
                            if tags__.is_some() {
                                return Err(serde::de::Error::duplicate_field("tags"));
                            }
                            tags__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Syscalls => {
                            if syscalls__.is_some() {
                                return Err(serde::de::Error::duplicate_field("syscalls"));
                            }
                            syscalls__ = 
                                Some(map_.next_value::<Vec<::pbjson::private::NumberDeserialize<_>>>()?
                                    .into_iter().map(|x| x.0).collect())
                            ;
                        }
                        GeneratedField::Tree => {
                            if tree__.is_some() {
                                return Err(serde::de::Error::duplicate_field("tree"));
                            }
                            tree__ = Some(map_.next_value()?);
                        }
                        GeneratedField::ProfileContexts => {
                            if profile_contexts__.is_some() {
                                return Err(serde::de::Error::duplicate_field("profileContexts"));
                            }
                            profile_contexts__ = Some(
                                map_.next_value::<std::collections::HashMap<_, _>>()?
                            );
                        }
                        GeneratedField::Selector => {
                            if selector__.is_some() {
                                return Err(serde::de::Error::duplicate_field("selector"));
                            }
                            selector__ = map_.next_value()?;
                        }
                        GeneratedField::Disabled => {
                            if disabled__.is_some() {
                                return Err(serde::de::Error::duplicate_field("disabled"));
                            }
                            disabled__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(SecurityProfile {
                    status: status__.unwrap_or_default(),
                    version: version__.unwrap_or_default(),
                    metadata: metadata__,
                    tags: tags__.unwrap_or_default(),
                    syscalls: syscalls__.unwrap_or_default(),
                    tree: tree__.unwrap_or_default(),
                    profile_contexts: profile_contexts__.unwrap_or_default(),
                    selector: selector__,
                    disabled: disabled__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.cws.dumpsv1.SecurityProfile", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for SocketNode {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.family.is_empty() {
            len += 1;
        }
        if !self.bind.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.cws.dumpsv1.SocketNode", len)?;
        if !self.family.is_empty() {
            struct_ser.serialize_field("family", &self.family)?;
        }
        if !self.bind.is_empty() {
            struct_ser.serialize_field("bind", &self.bind)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for SocketNode {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "family",
            "bind",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Family,
            Bind,
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
                            "family" => Ok(GeneratedField::Family),
                            "bind" => Ok(GeneratedField::Bind),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = SocketNode;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.cws.dumpsv1.SocketNode")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<SocketNode, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut family__ = None;
                let mut bind__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Family => {
                            if family__.is_some() {
                                return Err(serde::de::Error::duplicate_field("family"));
                            }
                            family__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Bind => {
                            if bind__.is_some() {
                                return Err(serde::de::Error::duplicate_field("bind"));
                            }
                            bind__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(SocketNode {
                    family: family__.unwrap_or_default(),
                    bind: bind__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.cws.dumpsv1.SocketNode", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for SyscallNode {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.image_tags.is_empty() {
            len += 1;
        }
        if self.node_base.is_some() {
            len += 1;
        }
        if self.syscall != 0 {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.cws.dumpsv1.SyscallNode", len)?;
        if !self.image_tags.is_empty() {
            struct_ser.serialize_field("imageTags", &self.image_tags)?;
        }
        if let Some(v) = self.node_base.as_ref() {
            struct_ser.serialize_field("nodeBase", v)?;
        }
        if self.syscall != 0 {
            struct_ser.serialize_field("syscall", &self.syscall)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for SyscallNode {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "image_tags",
            "imageTags",
            "node_base",
            "nodeBase",
            "syscall",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            ImageTags,
            NodeBase,
            Syscall,
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
                            "imageTags" | "image_tags" => Ok(GeneratedField::ImageTags),
                            "nodeBase" | "node_base" => Ok(GeneratedField::NodeBase),
                            "syscall" => Ok(GeneratedField::Syscall),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = SyscallNode;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.cws.dumpsv1.SyscallNode")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<SyscallNode, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut image_tags__ = None;
                let mut node_base__ = None;
                let mut syscall__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::ImageTags => {
                            if image_tags__.is_some() {
                                return Err(serde::de::Error::duplicate_field("imageTags"));
                            }
                            image_tags__ = Some(map_.next_value()?);
                        }
                        GeneratedField::NodeBase => {
                            if node_base__.is_some() {
                                return Err(serde::de::Error::duplicate_field("nodeBase"));
                            }
                            node_base__ = map_.next_value()?;
                        }
                        GeneratedField::Syscall => {
                            if syscall__.is_some() {
                                return Err(serde::de::Error::duplicate_field("syscall"));
                            }
                            syscall__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(SyscallNode {
                    image_tags: image_tags__.unwrap_or_default(),
                    node_base: node_base__,
                    syscall: syscall__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.cws.dumpsv1.SyscallNode", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for EventProfileState {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let variant = match self {
            Self::NoProfile => "NO_PROFILE",
            Self::ProfileAtMaxSize => "PROFILE_AT_MAX_SIZE",
            Self::UnstableProfile => "UNSTABLE_PROFILE",
            Self::StableProfile => "STABLE_PROFILE",
            Self::AutoLearning => "AUTO_LEARNING",
            Self::WorkloadWarmup => "WORKLOAD_WARMUP",
        };
        serializer.serialize_str(variant)
    }
}
impl<'de> serde::Deserialize<'de> for EventProfileState {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "NO_PROFILE",
            "PROFILE_AT_MAX_SIZE",
            "UNSTABLE_PROFILE",
            "STABLE_PROFILE",
            "AUTO_LEARNING",
            "WORKLOAD_WARMUP",
        ];

        struct GeneratedVisitor;

        impl serde::de::Visitor<'_> for GeneratedVisitor {
            type Value = EventProfileState;

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
                    "NO_PROFILE" => Ok(EventProfileState::NoProfile),
                    "PROFILE_AT_MAX_SIZE" => Ok(EventProfileState::ProfileAtMaxSize),
                    "UNSTABLE_PROFILE" => Ok(EventProfileState::UnstableProfile),
                    "STABLE_PROFILE" => Ok(EventProfileState::StableProfile),
                    "AUTO_LEARNING" => Ok(EventProfileState::AutoLearning),
                    "WORKLOAD_WARMUP" => Ok(EventProfileState::WorkloadWarmup),
                    _ => Err(serde::de::Error::unknown_variant(value, FIELDS)),
                }
            }
        }
        deserializer.deserialize_any(GeneratedVisitor)
    }
}
impl serde::Serialize for EventTypeState {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.last_anomaly_nano != 0 {
            len += 1;
        }
        if self.event_profile_state != 0 {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.cws.dumpsv1.event_type_state", len)?;
        if self.last_anomaly_nano != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("lastAnomalyNano", ToString::to_string(&self.last_anomaly_nano).as_str())?;
        }
        if self.event_profile_state != 0 {
            let v = EventProfileState::try_from(self.event_profile_state)
                .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", self.event_profile_state)))?;
            struct_ser.serialize_field("eventProfileState", &v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for EventTypeState {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "last_anomaly_nano",
            "lastAnomalyNano",
            "event_profile_state",
            "eventProfileState",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            LastAnomalyNano,
            EventProfileState,
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
                            "lastAnomalyNano" | "last_anomaly_nano" => Ok(GeneratedField::LastAnomalyNano),
                            "eventProfileState" | "event_profile_state" => Ok(GeneratedField::EventProfileState),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = EventTypeState;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.cws.dumpsv1.event_type_state")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<EventTypeState, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut last_anomaly_nano__ = None;
                let mut event_profile_state__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::LastAnomalyNano => {
                            if last_anomaly_nano__.is_some() {
                                return Err(serde::de::Error::duplicate_field("lastAnomalyNano"));
                            }
                            last_anomaly_nano__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::EventProfileState => {
                            if event_profile_state__.is_some() {
                                return Err(serde::de::Error::duplicate_field("eventProfileState"));
                            }
                            event_profile_state__ = Some(map_.next_value::<EventProfileState>()? as i32);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(EventTypeState {
                    last_anomaly_nano: last_anomaly_nano__.unwrap_or_default(),
                    event_profile_state: event_profile_state__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.cws.dumpsv1.event_type_state", FIELDS, GeneratedVisitor)
    }
}
