impl serde::Serialize for CommonMetadata {
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
        if !self.timezone.is_empty() {
            len += 1;
        }
        if self.current_epoch != 0. {
            len += 1;
        }
        if !self.internal_ip.is_empty() {
            len += 1;
        }
        if !self.public_ip.is_empty() {
            len += 1;
        }
        if !self.api_key.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.agentpayload.CommonMetadata", len)?;
        if !self.agent_version.is_empty() {
            struct_ser.serialize_field("agentVersion", &self.agent_version)?;
        }
        if !self.timezone.is_empty() {
            struct_ser.serialize_field("timezone", &self.timezone)?;
        }
        if self.current_epoch != 0. {
            struct_ser.serialize_field("currentEpoch", &self.current_epoch)?;
        }
        if !self.internal_ip.is_empty() {
            struct_ser.serialize_field("internalIp", &self.internal_ip)?;
        }
        if !self.public_ip.is_empty() {
            struct_ser.serialize_field("publicIp", &self.public_ip)?;
        }
        if !self.api_key.is_empty() {
            struct_ser.serialize_field("apiKey", &self.api_key)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for CommonMetadata {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "agent_version",
            "agentVersion",
            "timezone",
            "current_epoch",
            "currentEpoch",
            "internal_ip",
            "internalIp",
            "public_ip",
            "publicIp",
            "api_key",
            "apiKey",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            AgentVersion,
            Timezone,
            CurrentEpoch,
            InternalIp,
            PublicIp,
            ApiKey,
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
                            "timezone" => Ok(GeneratedField::Timezone),
                            "currentEpoch" | "current_epoch" => Ok(GeneratedField::CurrentEpoch),
                            "internalIp" | "internal_ip" => Ok(GeneratedField::InternalIp),
                            "publicIp" | "public_ip" => Ok(GeneratedField::PublicIp),
                            "apiKey" | "api_key" => Ok(GeneratedField::ApiKey),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = CommonMetadata;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.agentpayload.CommonMetadata")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<CommonMetadata, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut agent_version__ = None;
                let mut timezone__ = None;
                let mut current_epoch__ = None;
                let mut internal_ip__ = None;
                let mut public_ip__ = None;
                let mut api_key__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::AgentVersion => {
                            if agent_version__.is_some() {
                                return Err(serde::de::Error::duplicate_field("agentVersion"));
                            }
                            agent_version__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Timezone => {
                            if timezone__.is_some() {
                                return Err(serde::de::Error::duplicate_field("timezone"));
                            }
                            timezone__ = Some(map_.next_value()?);
                        }
                        GeneratedField::CurrentEpoch => {
                            if current_epoch__.is_some() {
                                return Err(serde::de::Error::duplicate_field("currentEpoch"));
                            }
                            current_epoch__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::InternalIp => {
                            if internal_ip__.is_some() {
                                return Err(serde::de::Error::duplicate_field("internalIp"));
                            }
                            internal_ip__ = Some(map_.next_value()?);
                        }
                        GeneratedField::PublicIp => {
                            if public_ip__.is_some() {
                                return Err(serde::de::Error::duplicate_field("publicIp"));
                            }
                            public_ip__ = Some(map_.next_value()?);
                        }
                        GeneratedField::ApiKey => {
                            if api_key__.is_some() {
                                return Err(serde::de::Error::duplicate_field("apiKey"));
                            }
                            api_key__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(CommonMetadata {
                    agent_version: agent_version__.unwrap_or_default(),
                    timezone: timezone__.unwrap_or_default(),
                    current_epoch: current_epoch__.unwrap_or_default(),
                    internal_ip: internal_ip__.unwrap_or_default(),
                    public_ip: public_ip__.unwrap_or_default(),
                    api_key: api_key__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.agentpayload.CommonMetadata", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for EventsPayload {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.events.is_empty() {
            len += 1;
        }
        if self.metadata.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.agentpayload.EventsPayload", len)?;
        if !self.events.is_empty() {
            struct_ser.serialize_field("events", &self.events)?;
        }
        if let Some(v) = self.metadata.as_ref() {
            struct_ser.serialize_field("metadata", v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for EventsPayload {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "events",
            "metadata",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Events,
            Metadata,
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
                            "events" => Ok(GeneratedField::Events),
                            "metadata" => Ok(GeneratedField::Metadata),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = EventsPayload;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.agentpayload.EventsPayload")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<EventsPayload, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut events__ = None;
                let mut metadata__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Events => {
                            if events__.is_some() {
                                return Err(serde::de::Error::duplicate_field("events"));
                            }
                            events__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Metadata => {
                            if metadata__.is_some() {
                                return Err(serde::de::Error::duplicate_field("metadata"));
                            }
                            metadata__ = map_.next_value()?;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(EventsPayload {
                    events: events__.unwrap_or_default(),
                    metadata: metadata__,
                })
            }
        }
        deserializer.deserialize_struct("datadog.agentpayload.EventsPayload", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for events_payload::Event {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.title.is_empty() {
            len += 1;
        }
        if !self.text.is_empty() {
            len += 1;
        }
        if self.ts != 0 {
            len += 1;
        }
        if !self.priority.is_empty() {
            len += 1;
        }
        if !self.host.is_empty() {
            len += 1;
        }
        if !self.tags.is_empty() {
            len += 1;
        }
        if !self.alert_type.is_empty() {
            len += 1;
        }
        if !self.aggregation_key.is_empty() {
            len += 1;
        }
        if !self.source_type_name.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.agentpayload.EventsPayload.Event", len)?;
        if !self.title.is_empty() {
            struct_ser.serialize_field("title", &self.title)?;
        }
        if !self.text.is_empty() {
            struct_ser.serialize_field("text", &self.text)?;
        }
        if self.ts != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("ts", ToString::to_string(&self.ts).as_str())?;
        }
        if !self.priority.is_empty() {
            struct_ser.serialize_field("priority", &self.priority)?;
        }
        if !self.host.is_empty() {
            struct_ser.serialize_field("host", &self.host)?;
        }
        if !self.tags.is_empty() {
            struct_ser.serialize_field("tags", &self.tags)?;
        }
        if !self.alert_type.is_empty() {
            struct_ser.serialize_field("alertType", &self.alert_type)?;
        }
        if !self.aggregation_key.is_empty() {
            struct_ser.serialize_field("aggregationKey", &self.aggregation_key)?;
        }
        if !self.source_type_name.is_empty() {
            struct_ser.serialize_field("sourceTypeName", &self.source_type_name)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for events_payload::Event {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "title",
            "text",
            "ts",
            "priority",
            "host",
            "tags",
            "alert_type",
            "alertType",
            "aggregation_key",
            "aggregationKey",
            "source_type_name",
            "sourceTypeName",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Title,
            Text,
            Ts,
            Priority,
            Host,
            Tags,
            AlertType,
            AggregationKey,
            SourceTypeName,
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
                            "text" => Ok(GeneratedField::Text),
                            "ts" => Ok(GeneratedField::Ts),
                            "priority" => Ok(GeneratedField::Priority),
                            "host" => Ok(GeneratedField::Host),
                            "tags" => Ok(GeneratedField::Tags),
                            "alertType" | "alert_type" => Ok(GeneratedField::AlertType),
                            "aggregationKey" | "aggregation_key" => Ok(GeneratedField::AggregationKey),
                            "sourceTypeName" | "source_type_name" => Ok(GeneratedField::SourceTypeName),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = events_payload::Event;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.agentpayload.EventsPayload.Event")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<events_payload::Event, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut title__ = None;
                let mut text__ = None;
                let mut ts__ = None;
                let mut priority__ = None;
                let mut host__ = None;
                let mut tags__ = None;
                let mut alert_type__ = None;
                let mut aggregation_key__ = None;
                let mut source_type_name__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Title => {
                            if title__.is_some() {
                                return Err(serde::de::Error::duplicate_field("title"));
                            }
                            title__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Text => {
                            if text__.is_some() {
                                return Err(serde::de::Error::duplicate_field("text"));
                            }
                            text__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Ts => {
                            if ts__.is_some() {
                                return Err(serde::de::Error::duplicate_field("ts"));
                            }
                            ts__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Priority => {
                            if priority__.is_some() {
                                return Err(serde::de::Error::duplicate_field("priority"));
                            }
                            priority__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Host => {
                            if host__.is_some() {
                                return Err(serde::de::Error::duplicate_field("host"));
                            }
                            host__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Tags => {
                            if tags__.is_some() {
                                return Err(serde::de::Error::duplicate_field("tags"));
                            }
                            tags__ = Some(map_.next_value()?);
                        }
                        GeneratedField::AlertType => {
                            if alert_type__.is_some() {
                                return Err(serde::de::Error::duplicate_field("alertType"));
                            }
                            alert_type__ = Some(map_.next_value()?);
                        }
                        GeneratedField::AggregationKey => {
                            if aggregation_key__.is_some() {
                                return Err(serde::de::Error::duplicate_field("aggregationKey"));
                            }
                            aggregation_key__ = Some(map_.next_value()?);
                        }
                        GeneratedField::SourceTypeName => {
                            if source_type_name__.is_some() {
                                return Err(serde::de::Error::duplicate_field("sourceTypeName"));
                            }
                            source_type_name__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(events_payload::Event {
                    title: title__.unwrap_or_default(),
                    text: text__.unwrap_or_default(),
                    ts: ts__.unwrap_or_default(),
                    priority: priority__.unwrap_or_default(),
                    host: host__.unwrap_or_default(),
                    tags: tags__.unwrap_or_default(),
                    alert_type: alert_type__.unwrap_or_default(),
                    aggregation_key: aggregation_key__.unwrap_or_default(),
                    source_type_name: source_type_name__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.agentpayload.EventsPayload.Event", FIELDS, GeneratedVisitor)
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
        if self.origin.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.agentpayload.Metadata", len)?;
        if let Some(v) = self.origin.as_ref() {
            struct_ser.serialize_field("origin", v)?;
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
            "origin",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Origin,
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
                            "origin" => Ok(GeneratedField::Origin),
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
                formatter.write_str("struct datadog.agentpayload.Metadata")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<Metadata, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut origin__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Origin => {
                            if origin__.is_some() {
                                return Err(serde::de::Error::duplicate_field("origin"));
                            }
                            origin__ = map_.next_value()?;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(Metadata {
                    origin: origin__,
                })
            }
        }
        deserializer.deserialize_struct("datadog.agentpayload.Metadata", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for MetricPayload {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.series.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.agentpayload.MetricPayload", len)?;
        if !self.series.is_empty() {
            struct_ser.serialize_field("series", &self.series)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for MetricPayload {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "series",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Series,
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
                            "series" => Ok(GeneratedField::Series),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = MetricPayload;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.agentpayload.MetricPayload")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<MetricPayload, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut series__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Series => {
                            if series__.is_some() {
                                return Err(serde::de::Error::duplicate_field("series"));
                            }
                            series__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(MetricPayload {
                    series: series__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.agentpayload.MetricPayload", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for metric_payload::MetricPoint {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.value != 0. {
            len += 1;
        }
        if self.timestamp != 0 {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.agentpayload.MetricPayload.MetricPoint", len)?;
        if self.value != 0. {
            struct_ser.serialize_field("value", &self.value)?;
        }
        if self.timestamp != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("timestamp", ToString::to_string(&self.timestamp).as_str())?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for metric_payload::MetricPoint {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "value",
            "timestamp",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Value,
            Timestamp,
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
                            "value" => Ok(GeneratedField::Value),
                            "timestamp" => Ok(GeneratedField::Timestamp),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = metric_payload::MetricPoint;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.agentpayload.MetricPayload.MetricPoint")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<metric_payload::MetricPoint, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut value__ = None;
                let mut timestamp__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Value => {
                            if value__.is_some() {
                                return Err(serde::de::Error::duplicate_field("value"));
                            }
                            value__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Timestamp => {
                            if timestamp__.is_some() {
                                return Err(serde::de::Error::duplicate_field("timestamp"));
                            }
                            timestamp__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(metric_payload::MetricPoint {
                    value: value__.unwrap_or_default(),
                    timestamp: timestamp__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.agentpayload.MetricPayload.MetricPoint", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for metric_payload::MetricSeries {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.resources.is_empty() {
            len += 1;
        }
        if !self.metric.is_empty() {
            len += 1;
        }
        if !self.tags.is_empty() {
            len += 1;
        }
        if !self.points.is_empty() {
            len += 1;
        }
        if self.r#type != 0 {
            len += 1;
        }
        if !self.unit.is_empty() {
            len += 1;
        }
        if !self.source_type_name.is_empty() {
            len += 1;
        }
        if self.interval != 0 {
            len += 1;
        }
        if self.metadata.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.agentpayload.MetricPayload.MetricSeries", len)?;
        if !self.resources.is_empty() {
            struct_ser.serialize_field("resources", &self.resources)?;
        }
        if !self.metric.is_empty() {
            struct_ser.serialize_field("metric", &self.metric)?;
        }
        if !self.tags.is_empty() {
            struct_ser.serialize_field("tags", &self.tags)?;
        }
        if !self.points.is_empty() {
            struct_ser.serialize_field("points", &self.points)?;
        }
        if self.r#type != 0 {
            let v = metric_payload::MetricType::try_from(self.r#type)
                .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", self.r#type)))?;
            struct_ser.serialize_field("type", &v)?;
        }
        if !self.unit.is_empty() {
            struct_ser.serialize_field("unit", &self.unit)?;
        }
        if !self.source_type_name.is_empty() {
            struct_ser.serialize_field("sourceTypeName", &self.source_type_name)?;
        }
        if self.interval != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("interval", ToString::to_string(&self.interval).as_str())?;
        }
        if let Some(v) = self.metadata.as_ref() {
            struct_ser.serialize_field("metadata", v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for metric_payload::MetricSeries {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "resources",
            "metric",
            "tags",
            "points",
            "type",
            "unit",
            "source_type_name",
            "sourceTypeName",
            "interval",
            "metadata",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Resources,
            Metric,
            Tags,
            Points,
            Type,
            Unit,
            SourceTypeName,
            Interval,
            Metadata,
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
                            "resources" => Ok(GeneratedField::Resources),
                            "metric" => Ok(GeneratedField::Metric),
                            "tags" => Ok(GeneratedField::Tags),
                            "points" => Ok(GeneratedField::Points),
                            "type" => Ok(GeneratedField::Type),
                            "unit" => Ok(GeneratedField::Unit),
                            "sourceTypeName" | "source_type_name" => Ok(GeneratedField::SourceTypeName),
                            "interval" => Ok(GeneratedField::Interval),
                            "metadata" => Ok(GeneratedField::Metadata),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = metric_payload::MetricSeries;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.agentpayload.MetricPayload.MetricSeries")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<metric_payload::MetricSeries, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut resources__ = None;
                let mut metric__ = None;
                let mut tags__ = None;
                let mut points__ = None;
                let mut r#type__ = None;
                let mut unit__ = None;
                let mut source_type_name__ = None;
                let mut interval__ = None;
                let mut metadata__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Resources => {
                            if resources__.is_some() {
                                return Err(serde::de::Error::duplicate_field("resources"));
                            }
                            resources__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Metric => {
                            if metric__.is_some() {
                                return Err(serde::de::Error::duplicate_field("metric"));
                            }
                            metric__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Tags => {
                            if tags__.is_some() {
                                return Err(serde::de::Error::duplicate_field("tags"));
                            }
                            tags__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Points => {
                            if points__.is_some() {
                                return Err(serde::de::Error::duplicate_field("points"));
                            }
                            points__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Type => {
                            if r#type__.is_some() {
                                return Err(serde::de::Error::duplicate_field("type"));
                            }
                            r#type__ = Some(map_.next_value::<metric_payload::MetricType>()? as i32);
                        }
                        GeneratedField::Unit => {
                            if unit__.is_some() {
                                return Err(serde::de::Error::duplicate_field("unit"));
                            }
                            unit__ = Some(map_.next_value()?);
                        }
                        GeneratedField::SourceTypeName => {
                            if source_type_name__.is_some() {
                                return Err(serde::de::Error::duplicate_field("sourceTypeName"));
                            }
                            source_type_name__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Interval => {
                            if interval__.is_some() {
                                return Err(serde::de::Error::duplicate_field("interval"));
                            }
                            interval__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Metadata => {
                            if metadata__.is_some() {
                                return Err(serde::de::Error::duplicate_field("metadata"));
                            }
                            metadata__ = map_.next_value()?;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(metric_payload::MetricSeries {
                    resources: resources__.unwrap_or_default(),
                    metric: metric__.unwrap_or_default(),
                    tags: tags__.unwrap_or_default(),
                    points: points__.unwrap_or_default(),
                    r#type: r#type__.unwrap_or_default(),
                    unit: unit__.unwrap_or_default(),
                    source_type_name: source_type_name__.unwrap_or_default(),
                    interval: interval__.unwrap_or_default(),
                    metadata: metadata__,
                })
            }
        }
        deserializer.deserialize_struct("datadog.agentpayload.MetricPayload.MetricSeries", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for metric_payload::MetricType {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let variant = match self {
            Self::Unspecified => "UNSPECIFIED",
            Self::Count => "COUNT",
            Self::Rate => "RATE",
            Self::Gauge => "GAUGE",
        };
        serializer.serialize_str(variant)
    }
}
impl<'de> serde::Deserialize<'de> for metric_payload::MetricType {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "UNSPECIFIED",
            "COUNT",
            "RATE",
            "GAUGE",
        ];

        struct GeneratedVisitor;

        impl serde::de::Visitor<'_> for GeneratedVisitor {
            type Value = metric_payload::MetricType;

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
                    "UNSPECIFIED" => Ok(metric_payload::MetricType::Unspecified),
                    "COUNT" => Ok(metric_payload::MetricType::Count),
                    "RATE" => Ok(metric_payload::MetricType::Rate),
                    "GAUGE" => Ok(metric_payload::MetricType::Gauge),
                    _ => Err(serde::de::Error::unknown_variant(value, FIELDS)),
                }
            }
        }
        deserializer.deserialize_any(GeneratedVisitor)
    }
}
impl serde::Serialize for metric_payload::Resource {
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
        if !self.name.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.agentpayload.MetricPayload.Resource", len)?;
        if !self.r#type.is_empty() {
            struct_ser.serialize_field("type", &self.r#type)?;
        }
        if !self.name.is_empty() {
            struct_ser.serialize_field("name", &self.name)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for metric_payload::Resource {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "type",
            "name",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Type,
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
                            "type" => Ok(GeneratedField::Type),
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
            type Value = metric_payload::Resource;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.agentpayload.MetricPayload.Resource")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<metric_payload::Resource, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut r#type__ = None;
                let mut name__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Type => {
                            if r#type__.is_some() {
                                return Err(serde::de::Error::duplicate_field("type"));
                            }
                            r#type__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Name => {
                            if name__.is_some() {
                                return Err(serde::de::Error::duplicate_field("name"));
                            }
                            name__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(metric_payload::Resource {
                    r#type: r#type__.unwrap_or_default(),
                    name: name__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.agentpayload.MetricPayload.Resource", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for Origin {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.origin_product != 0 {
            len += 1;
        }
        if self.origin_category != 0 {
            len += 1;
        }
        if self.origin_service != 0 {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.agentpayload.Origin", len)?;
        if self.origin_product != 0 {
            struct_ser.serialize_field("originProduct", &self.origin_product)?;
        }
        if self.origin_category != 0 {
            struct_ser.serialize_field("originCategory", &self.origin_category)?;
        }
        if self.origin_service != 0 {
            struct_ser.serialize_field("originService", &self.origin_service)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for Origin {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "origin_product",
            "originProduct",
            "origin_category",
            "originCategory",
            "origin_service",
            "originService",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            OriginProduct,
            OriginCategory,
            OriginService,
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
                            "originProduct" | "origin_product" => Ok(GeneratedField::OriginProduct),
                            "originCategory" | "origin_category" => Ok(GeneratedField::OriginCategory),
                            "originService" | "origin_service" => Ok(GeneratedField::OriginService),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = Origin;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.agentpayload.Origin")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<Origin, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut origin_product__ = None;
                let mut origin_category__ = None;
                let mut origin_service__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::OriginProduct => {
                            if origin_product__.is_some() {
                                return Err(serde::de::Error::duplicate_field("originProduct"));
                            }
                            origin_product__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::OriginCategory => {
                            if origin_category__.is_some() {
                                return Err(serde::de::Error::duplicate_field("originCategory"));
                            }
                            origin_category__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::OriginService => {
                            if origin_service__.is_some() {
                                return Err(serde::de::Error::duplicate_field("originService"));
                            }
                            origin_service__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(Origin {
                    origin_product: origin_product__.unwrap_or_default(),
                    origin_category: origin_category__.unwrap_or_default(),
                    origin_service: origin_service__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.agentpayload.Origin", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for SketchPayload {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.sketches.is_empty() {
            len += 1;
        }
        if self.metadata.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.agentpayload.SketchPayload", len)?;
        if !self.sketches.is_empty() {
            struct_ser.serialize_field("sketches", &self.sketches)?;
        }
        if let Some(v) = self.metadata.as_ref() {
            struct_ser.serialize_field("metadata", v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for SketchPayload {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "sketches",
            "metadata",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Sketches,
            Metadata,
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
                            "sketches" => Ok(GeneratedField::Sketches),
                            "metadata" => Ok(GeneratedField::Metadata),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = SketchPayload;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.agentpayload.SketchPayload")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<SketchPayload, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut sketches__ = None;
                let mut metadata__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Sketches => {
                            if sketches__.is_some() {
                                return Err(serde::de::Error::duplicate_field("sketches"));
                            }
                            sketches__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Metadata => {
                            if metadata__.is_some() {
                                return Err(serde::de::Error::duplicate_field("metadata"));
                            }
                            metadata__ = map_.next_value()?;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(SketchPayload {
                    sketches: sketches__.unwrap_or_default(),
                    metadata: metadata__,
                })
            }
        }
        deserializer.deserialize_struct("datadog.agentpayload.SketchPayload", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for sketch_payload::Sketch {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.metric.is_empty() {
            len += 1;
        }
        if !self.host.is_empty() {
            len += 1;
        }
        if !self.distributions.is_empty() {
            len += 1;
        }
        if !self.tags.is_empty() {
            len += 1;
        }
        if !self.dogsketches.is_empty() {
            len += 1;
        }
        if self.metadata.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.agentpayload.SketchPayload.Sketch", len)?;
        if !self.metric.is_empty() {
            struct_ser.serialize_field("metric", &self.metric)?;
        }
        if !self.host.is_empty() {
            struct_ser.serialize_field("host", &self.host)?;
        }
        if !self.distributions.is_empty() {
            struct_ser.serialize_field("distributions", &self.distributions)?;
        }
        if !self.tags.is_empty() {
            struct_ser.serialize_field("tags", &self.tags)?;
        }
        if !self.dogsketches.is_empty() {
            struct_ser.serialize_field("dogsketches", &self.dogsketches)?;
        }
        if let Some(v) = self.metadata.as_ref() {
            struct_ser.serialize_field("metadata", v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for sketch_payload::Sketch {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "metric",
            "host",
            "distributions",
            "tags",
            "dogsketches",
            "metadata",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Metric,
            Host,
            Distributions,
            Tags,
            Dogsketches,
            Metadata,
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
                            "metric" => Ok(GeneratedField::Metric),
                            "host" => Ok(GeneratedField::Host),
                            "distributions" => Ok(GeneratedField::Distributions),
                            "tags" => Ok(GeneratedField::Tags),
                            "dogsketches" => Ok(GeneratedField::Dogsketches),
                            "metadata" => Ok(GeneratedField::Metadata),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = sketch_payload::Sketch;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.agentpayload.SketchPayload.Sketch")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<sketch_payload::Sketch, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut metric__ = None;
                let mut host__ = None;
                let mut distributions__ = None;
                let mut tags__ = None;
                let mut dogsketches__ = None;
                let mut metadata__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Metric => {
                            if metric__.is_some() {
                                return Err(serde::de::Error::duplicate_field("metric"));
                            }
                            metric__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Host => {
                            if host__.is_some() {
                                return Err(serde::de::Error::duplicate_field("host"));
                            }
                            host__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Distributions => {
                            if distributions__.is_some() {
                                return Err(serde::de::Error::duplicate_field("distributions"));
                            }
                            distributions__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Tags => {
                            if tags__.is_some() {
                                return Err(serde::de::Error::duplicate_field("tags"));
                            }
                            tags__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Dogsketches => {
                            if dogsketches__.is_some() {
                                return Err(serde::de::Error::duplicate_field("dogsketches"));
                            }
                            dogsketches__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Metadata => {
                            if metadata__.is_some() {
                                return Err(serde::de::Error::duplicate_field("metadata"));
                            }
                            metadata__ = map_.next_value()?;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(sketch_payload::Sketch {
                    metric: metric__.unwrap_or_default(),
                    host: host__.unwrap_or_default(),
                    distributions: distributions__.unwrap_or_default(),
                    tags: tags__.unwrap_or_default(),
                    dogsketches: dogsketches__.unwrap_or_default(),
                    metadata: metadata__,
                })
            }
        }
        deserializer.deserialize_struct("datadog.agentpayload.SketchPayload.Sketch", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for sketch_payload::sketch::Distribution {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.ts != 0 {
            len += 1;
        }
        if self.cnt != 0 {
            len += 1;
        }
        if self.min != 0. {
            len += 1;
        }
        if self.max != 0. {
            len += 1;
        }
        if self.avg != 0. {
            len += 1;
        }
        if self.sum != 0. {
            len += 1;
        }
        if !self.v.is_empty() {
            len += 1;
        }
        if !self.g.is_empty() {
            len += 1;
        }
        if !self.delta.is_empty() {
            len += 1;
        }
        if !self.buf.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.agentpayload.SketchPayload.Sketch.Distribution", len)?;
        if self.ts != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("ts", ToString::to_string(&self.ts).as_str())?;
        }
        if self.cnt != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("cnt", ToString::to_string(&self.cnt).as_str())?;
        }
        if self.min != 0. {
            struct_ser.serialize_field("min", &self.min)?;
        }
        if self.max != 0. {
            struct_ser.serialize_field("max", &self.max)?;
        }
        if self.avg != 0. {
            struct_ser.serialize_field("avg", &self.avg)?;
        }
        if self.sum != 0. {
            struct_ser.serialize_field("sum", &self.sum)?;
        }
        if !self.v.is_empty() {
            struct_ser.serialize_field("v", &self.v)?;
        }
        if !self.g.is_empty() {
            struct_ser.serialize_field("g", &self.g)?;
        }
        if !self.delta.is_empty() {
            struct_ser.serialize_field("delta", &self.delta)?;
        }
        if !self.buf.is_empty() {
            struct_ser.serialize_field("buf", &self.buf)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for sketch_payload::sketch::Distribution {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "ts",
            "cnt",
            "min",
            "max",
            "avg",
            "sum",
            "v",
            "g",
            "delta",
            "buf",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Ts,
            Cnt,
            Min,
            Max,
            Avg,
            Sum,
            V,
            G,
            Delta,
            Buf,
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
                            "ts" => Ok(GeneratedField::Ts),
                            "cnt" => Ok(GeneratedField::Cnt),
                            "min" => Ok(GeneratedField::Min),
                            "max" => Ok(GeneratedField::Max),
                            "avg" => Ok(GeneratedField::Avg),
                            "sum" => Ok(GeneratedField::Sum),
                            "v" => Ok(GeneratedField::V),
                            "g" => Ok(GeneratedField::G),
                            "delta" => Ok(GeneratedField::Delta),
                            "buf" => Ok(GeneratedField::Buf),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = sketch_payload::sketch::Distribution;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.agentpayload.SketchPayload.Sketch.Distribution")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<sketch_payload::sketch::Distribution, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut ts__ = None;
                let mut cnt__ = None;
                let mut min__ = None;
                let mut max__ = None;
                let mut avg__ = None;
                let mut sum__ = None;
                let mut v__ = None;
                let mut g__ = None;
                let mut delta__ = None;
                let mut buf__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Ts => {
                            if ts__.is_some() {
                                return Err(serde::de::Error::duplicate_field("ts"));
                            }
                            ts__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Cnt => {
                            if cnt__.is_some() {
                                return Err(serde::de::Error::duplicate_field("cnt"));
                            }
                            cnt__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Min => {
                            if min__.is_some() {
                                return Err(serde::de::Error::duplicate_field("min"));
                            }
                            min__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Max => {
                            if max__.is_some() {
                                return Err(serde::de::Error::duplicate_field("max"));
                            }
                            max__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Avg => {
                            if avg__.is_some() {
                                return Err(serde::de::Error::duplicate_field("avg"));
                            }
                            avg__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Sum => {
                            if sum__.is_some() {
                                return Err(serde::de::Error::duplicate_field("sum"));
                            }
                            sum__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::V => {
                            if v__.is_some() {
                                return Err(serde::de::Error::duplicate_field("v"));
                            }
                            v__ = 
                                Some(map_.next_value::<Vec<::pbjson::private::NumberDeserialize<_>>>()?
                                    .into_iter().map(|x| x.0).collect())
                            ;
                        }
                        GeneratedField::G => {
                            if g__.is_some() {
                                return Err(serde::de::Error::duplicate_field("g"));
                            }
                            g__ = 
                                Some(map_.next_value::<Vec<::pbjson::private::NumberDeserialize<_>>>()?
                                    .into_iter().map(|x| x.0).collect())
                            ;
                        }
                        GeneratedField::Delta => {
                            if delta__.is_some() {
                                return Err(serde::de::Error::duplicate_field("delta"));
                            }
                            delta__ = 
                                Some(map_.next_value::<Vec<::pbjson::private::NumberDeserialize<_>>>()?
                                    .into_iter().map(|x| x.0).collect())
                            ;
                        }
                        GeneratedField::Buf => {
                            if buf__.is_some() {
                                return Err(serde::de::Error::duplicate_field("buf"));
                            }
                            buf__ = 
                                Some(map_.next_value::<Vec<::pbjson::private::NumberDeserialize<_>>>()?
                                    .into_iter().map(|x| x.0).collect())
                            ;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(sketch_payload::sketch::Distribution {
                    ts: ts__.unwrap_or_default(),
                    cnt: cnt__.unwrap_or_default(),
                    min: min__.unwrap_or_default(),
                    max: max__.unwrap_or_default(),
                    avg: avg__.unwrap_or_default(),
                    sum: sum__.unwrap_or_default(),
                    v: v__.unwrap_or_default(),
                    g: g__.unwrap_or_default(),
                    delta: delta__.unwrap_or_default(),
                    buf: buf__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.agentpayload.SketchPayload.Sketch.Distribution", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for sketch_payload::sketch::Dogsketch {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.ts != 0 {
            len += 1;
        }
        if self.cnt != 0 {
            len += 1;
        }
        if self.min != 0. {
            len += 1;
        }
        if self.max != 0. {
            len += 1;
        }
        if self.avg != 0. {
            len += 1;
        }
        if self.sum != 0. {
            len += 1;
        }
        if !self.k.is_empty() {
            len += 1;
        }
        if !self.n.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.agentpayload.SketchPayload.Sketch.Dogsketch", len)?;
        if self.ts != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("ts", ToString::to_string(&self.ts).as_str())?;
        }
        if self.cnt != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("cnt", ToString::to_string(&self.cnt).as_str())?;
        }
        if self.min != 0. {
            struct_ser.serialize_field("min", &self.min)?;
        }
        if self.max != 0. {
            struct_ser.serialize_field("max", &self.max)?;
        }
        if self.avg != 0. {
            struct_ser.serialize_field("avg", &self.avg)?;
        }
        if self.sum != 0. {
            struct_ser.serialize_field("sum", &self.sum)?;
        }
        if !self.k.is_empty() {
            struct_ser.serialize_field("k", &self.k)?;
        }
        if !self.n.is_empty() {
            struct_ser.serialize_field("n", &self.n)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for sketch_payload::sketch::Dogsketch {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "ts",
            "cnt",
            "min",
            "max",
            "avg",
            "sum",
            "k",
            "n",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Ts,
            Cnt,
            Min,
            Max,
            Avg,
            Sum,
            K,
            N,
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
                            "ts" => Ok(GeneratedField::Ts),
                            "cnt" => Ok(GeneratedField::Cnt),
                            "min" => Ok(GeneratedField::Min),
                            "max" => Ok(GeneratedField::Max),
                            "avg" => Ok(GeneratedField::Avg),
                            "sum" => Ok(GeneratedField::Sum),
                            "k" => Ok(GeneratedField::K),
                            "n" => Ok(GeneratedField::N),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = sketch_payload::sketch::Dogsketch;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.agentpayload.SketchPayload.Sketch.Dogsketch")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<sketch_payload::sketch::Dogsketch, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut ts__ = None;
                let mut cnt__ = None;
                let mut min__ = None;
                let mut max__ = None;
                let mut avg__ = None;
                let mut sum__ = None;
                let mut k__ = None;
                let mut n__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Ts => {
                            if ts__.is_some() {
                                return Err(serde::de::Error::duplicate_field("ts"));
                            }
                            ts__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Cnt => {
                            if cnt__.is_some() {
                                return Err(serde::de::Error::duplicate_field("cnt"));
                            }
                            cnt__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Min => {
                            if min__.is_some() {
                                return Err(serde::de::Error::duplicate_field("min"));
                            }
                            min__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Max => {
                            if max__.is_some() {
                                return Err(serde::de::Error::duplicate_field("max"));
                            }
                            max__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Avg => {
                            if avg__.is_some() {
                                return Err(serde::de::Error::duplicate_field("avg"));
                            }
                            avg__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Sum => {
                            if sum__.is_some() {
                                return Err(serde::de::Error::duplicate_field("sum"));
                            }
                            sum__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::K => {
                            if k__.is_some() {
                                return Err(serde::de::Error::duplicate_field("k"));
                            }
                            k__ = 
                                Some(map_.next_value::<Vec<::pbjson::private::NumberDeserialize<_>>>()?
                                    .into_iter().map(|x| x.0).collect())
                            ;
                        }
                        GeneratedField::N => {
                            if n__.is_some() {
                                return Err(serde::de::Error::duplicate_field("n"));
                            }
                            n__ = 
                                Some(map_.next_value::<Vec<::pbjson::private::NumberDeserialize<_>>>()?
                                    .into_iter().map(|x| x.0).collect())
                            ;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(sketch_payload::sketch::Dogsketch {
                    ts: ts__.unwrap_or_default(),
                    cnt: cnt__.unwrap_or_default(),
                    min: min__.unwrap_or_default(),
                    max: max__.unwrap_or_default(),
                    avg: avg__.unwrap_or_default(),
                    sum: sum__.unwrap_or_default(),
                    k: k__.unwrap_or_default(),
                    n: n__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.agentpayload.SketchPayload.Sketch.Dogsketch", FIELDS, GeneratedVisitor)
    }
}
