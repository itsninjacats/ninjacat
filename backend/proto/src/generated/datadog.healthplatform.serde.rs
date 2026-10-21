impl serde::Serialize for HealthReport {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.schema_version.is_empty() {
            len += 1;
        }
        if !self.event_type.is_empty() {
            len += 1;
        }
        if !self.emitted_at.is_empty() {
            len += 1;
        }
        if self.host.is_some() {
            len += 1;
        }
        if !self.issues.is_empty() {
            len += 1;
        }
        if !self.service.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.healthplatform.HealthReport", len)?;
        if !self.schema_version.is_empty() {
            struct_ser.serialize_field("schemaVersion", &self.schema_version)?;
        }
        if !self.event_type.is_empty() {
            struct_ser.serialize_field("eventType", &self.event_type)?;
        }
        if !self.emitted_at.is_empty() {
            struct_ser.serialize_field("emittedAt", &self.emitted_at)?;
        }
        if let Some(v) = self.host.as_ref() {
            struct_ser.serialize_field("host", v)?;
        }
        if !self.issues.is_empty() {
            struct_ser.serialize_field("issues", &self.issues)?;
        }
        if !self.service.is_empty() {
            struct_ser.serialize_field("service", &self.service)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for HealthReport {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "schema_version",
            "schemaVersion",
            "event_type",
            "eventType",
            "emitted_at",
            "emittedAt",
            "host",
            "issues",
            "service",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            SchemaVersion,
            EventType,
            EmittedAt,
            Host,
            Issues,
            Service,
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
                            "schemaVersion" | "schema_version" => Ok(GeneratedField::SchemaVersion),
                            "eventType" | "event_type" => Ok(GeneratedField::EventType),
                            "emittedAt" | "emitted_at" => Ok(GeneratedField::EmittedAt),
                            "host" => Ok(GeneratedField::Host),
                            "issues" => Ok(GeneratedField::Issues),
                            "service" => Ok(GeneratedField::Service),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = HealthReport;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.healthplatform.HealthReport")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<HealthReport, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut schema_version__ = None;
                let mut event_type__ = None;
                let mut emitted_at__ = None;
                let mut host__ = None;
                let mut issues__ = None;
                let mut service__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::SchemaVersion => {
                            if schema_version__.is_some() {
                                return Err(serde::de::Error::duplicate_field("schemaVersion"));
                            }
                            schema_version__ = Some(map_.next_value()?);
                        }
                        GeneratedField::EventType => {
                            if event_type__.is_some() {
                                return Err(serde::de::Error::duplicate_field("eventType"));
                            }
                            event_type__ = Some(map_.next_value()?);
                        }
                        GeneratedField::EmittedAt => {
                            if emitted_at__.is_some() {
                                return Err(serde::de::Error::duplicate_field("emittedAt"));
                            }
                            emitted_at__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Host => {
                            if host__.is_some() {
                                return Err(serde::de::Error::duplicate_field("host"));
                            }
                            host__ = map_.next_value()?;
                        }
                        GeneratedField::Issues => {
                            if issues__.is_some() {
                                return Err(serde::de::Error::duplicate_field("issues"));
                            }
                            issues__ = Some(
                                map_.next_value::<std::collections::HashMap<_, _>>()?
                            );
                        }
                        GeneratedField::Service => {
                            if service__.is_some() {
                                return Err(serde::de::Error::duplicate_field("service"));
                            }
                            service__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(HealthReport {
                    schema_version: schema_version__.unwrap_or_default(),
                    event_type: event_type__.unwrap_or_default(),
                    emitted_at: emitted_at__.unwrap_or_default(),
                    host: host__,
                    issues: issues__.unwrap_or_default(),
                    service: service__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.healthplatform.HealthReport", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for HostInfo {
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
        if self.agent_version.is_some() {
            len += 1;
        }
        if !self.par_ids.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.healthplatform.HostInfo", len)?;
        if !self.hostname.is_empty() {
            struct_ser.serialize_field("hostname", &self.hostname)?;
        }
        if let Some(v) = self.agent_version.as_ref() {
            struct_ser.serialize_field("agentVersion", v)?;
        }
        if !self.par_ids.is_empty() {
            struct_ser.serialize_field("parIds", &self.par_ids)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for HostInfo {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "hostname",
            "agent_version",
            "agentVersion",
            "par_ids",
            "parIds",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Hostname,
            AgentVersion,
            ParIds,
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
                            "agentVersion" | "agent_version" => Ok(GeneratedField::AgentVersion),
                            "parIds" | "par_ids" => Ok(GeneratedField::ParIds),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = HostInfo;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.healthplatform.HostInfo")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<HostInfo, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut hostname__ = None;
                let mut agent_version__ = None;
                let mut par_ids__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Hostname => {
                            if hostname__.is_some() {
                                return Err(serde::de::Error::duplicate_field("hostname"));
                            }
                            hostname__ = Some(map_.next_value()?);
                        }
                        GeneratedField::AgentVersion => {
                            if agent_version__.is_some() {
                                return Err(serde::de::Error::duplicate_field("agentVersion"));
                            }
                            agent_version__ = map_.next_value()?;
                        }
                        GeneratedField::ParIds => {
                            if par_ids__.is_some() {
                                return Err(serde::de::Error::duplicate_field("parIds"));
                            }
                            par_ids__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(HostInfo {
                    hostname: hostname__.unwrap_or_default(),
                    agent_version: agent_version__,
                    par_ids: par_ids__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.healthplatform.HostInfo", FIELDS, GeneratedVisitor)
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
        if !self.id.is_empty() {
            len += 1;
        }
        if !self.issue_name.is_empty() {
            len += 1;
        }
        if !self.title.is_empty() {
            len += 1;
        }
        if !self.description.is_empty() {
            len += 1;
        }
        if !self.category.is_empty() {
            len += 1;
        }
        if !self.location.is_empty() {
            len += 1;
        }
        if self.severity != 0 {
            len += 1;
        }
        if !self.detected_at.is_empty() {
            len += 1;
        }
        if !self.source.is_empty() {
            len += 1;
        }
        if self.extra.is_some() {
            len += 1;
        }
        if self.remediation.is_some() {
            len += 1;
        }
        if !self.tags.is_empty() {
            len += 1;
        }
        if self.persisted_issue.is_some() {
            len += 1;
        }
        if !self.issue_type.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.healthplatform.Issue", len)?;
        if !self.id.is_empty() {
            struct_ser.serialize_field("id", &self.id)?;
        }
        if !self.issue_name.is_empty() {
            struct_ser.serialize_field("issueName", &self.issue_name)?;
        }
        if !self.title.is_empty() {
            struct_ser.serialize_field("title", &self.title)?;
        }
        if !self.description.is_empty() {
            struct_ser.serialize_field("description", &self.description)?;
        }
        if !self.category.is_empty() {
            struct_ser.serialize_field("category", &self.category)?;
        }
        if !self.location.is_empty() {
            struct_ser.serialize_field("location", &self.location)?;
        }
        if self.severity != 0 {
            let v = IssueSeverity::try_from(self.severity)
                .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", self.severity)))?;
            struct_ser.serialize_field("severity", &v)?;
        }
        if !self.detected_at.is_empty() {
            struct_ser.serialize_field("detectedAt", &self.detected_at)?;
        }
        if !self.source.is_empty() {
            struct_ser.serialize_field("source", &self.source)?;
        }
        if let Some(v) = self.extra.as_ref() {
            struct_ser.serialize_field("extra", v)?;
        }
        if let Some(v) = self.remediation.as_ref() {
            struct_ser.serialize_field("remediation", v)?;
        }
        if !self.tags.is_empty() {
            struct_ser.serialize_field("tags", &self.tags)?;
        }
        if let Some(v) = self.persisted_issue.as_ref() {
            struct_ser.serialize_field("persistedIssue", v)?;
        }
        if !self.issue_type.is_empty() {
            struct_ser.serialize_field("issueType", &self.issue_type)?;
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
            "id",
            "issue_name",
            "issueName",
            "title",
            "description",
            "category",
            "location",
            "severity",
            "detected_at",
            "detectedAt",
            "source",
            "extra",
            "remediation",
            "tags",
            "persisted_issue",
            "persistedIssue",
            "issue_type",
            "issueType",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Id,
            IssueName,
            Title,
            Description,
            Category,
            Location,
            Severity,
            DetectedAt,
            Source,
            Extra,
            Remediation,
            Tags,
            PersistedIssue,
            IssueType,
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
                            "issueName" | "issue_name" => Ok(GeneratedField::IssueName),
                            "title" => Ok(GeneratedField::Title),
                            "description" => Ok(GeneratedField::Description),
                            "category" => Ok(GeneratedField::Category),
                            "location" => Ok(GeneratedField::Location),
                            "severity" => Ok(GeneratedField::Severity),
                            "detectedAt" | "detected_at" => Ok(GeneratedField::DetectedAt),
                            "source" => Ok(GeneratedField::Source),
                            "extra" => Ok(GeneratedField::Extra),
                            "remediation" => Ok(GeneratedField::Remediation),
                            "tags" => Ok(GeneratedField::Tags),
                            "persistedIssue" | "persisted_issue" => Ok(GeneratedField::PersistedIssue),
                            "issueType" | "issue_type" => Ok(GeneratedField::IssueType),
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
                formatter.write_str("struct datadog.healthplatform.Issue")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<Issue, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut id__ = None;
                let mut issue_name__ = None;
                let mut title__ = None;
                let mut description__ = None;
                let mut category__ = None;
                let mut location__ = None;
                let mut severity__ = None;
                let mut detected_at__ = None;
                let mut source__ = None;
                let mut extra__ = None;
                let mut remediation__ = None;
                let mut tags__ = None;
                let mut persisted_issue__ = None;
                let mut issue_type__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Id => {
                            if id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("id"));
                            }
                            id__ = Some(map_.next_value()?);
                        }
                        GeneratedField::IssueName => {
                            if issue_name__.is_some() {
                                return Err(serde::de::Error::duplicate_field("issueName"));
                            }
                            issue_name__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Title => {
                            if title__.is_some() {
                                return Err(serde::de::Error::duplicate_field("title"));
                            }
                            title__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Description => {
                            if description__.is_some() {
                                return Err(serde::de::Error::duplicate_field("description"));
                            }
                            description__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Category => {
                            if category__.is_some() {
                                return Err(serde::de::Error::duplicate_field("category"));
                            }
                            category__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Location => {
                            if location__.is_some() {
                                return Err(serde::de::Error::duplicate_field("location"));
                            }
                            location__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Severity => {
                            if severity__.is_some() {
                                return Err(serde::de::Error::duplicate_field("severity"));
                            }
                            severity__ = Some(map_.next_value::<IssueSeverity>()? as i32);
                        }
                        GeneratedField::DetectedAt => {
                            if detected_at__.is_some() {
                                return Err(serde::de::Error::duplicate_field("detectedAt"));
                            }
                            detected_at__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Source => {
                            if source__.is_some() {
                                return Err(serde::de::Error::duplicate_field("source"));
                            }
                            source__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Extra => {
                            if extra__.is_some() {
                                return Err(serde::de::Error::duplicate_field("extra"));
                            }
                            extra__ = map_.next_value()?;
                        }
                        GeneratedField::Remediation => {
                            if remediation__.is_some() {
                                return Err(serde::de::Error::duplicate_field("remediation"));
                            }
                            remediation__ = map_.next_value()?;
                        }
                        GeneratedField::Tags => {
                            if tags__.is_some() {
                                return Err(serde::de::Error::duplicate_field("tags"));
                            }
                            tags__ = Some(map_.next_value()?);
                        }
                        GeneratedField::PersistedIssue => {
                            if persisted_issue__.is_some() {
                                return Err(serde::de::Error::duplicate_field("persistedIssue"));
                            }
                            persisted_issue__ = map_.next_value()?;
                        }
                        GeneratedField::IssueType => {
                            if issue_type__.is_some() {
                                return Err(serde::de::Error::duplicate_field("issueType"));
                            }
                            issue_type__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(Issue {
                    id: id__.unwrap_or_default(),
                    issue_name: issue_name__.unwrap_or_default(),
                    title: title__.unwrap_or_default(),
                    description: description__.unwrap_or_default(),
                    category: category__.unwrap_or_default(),
                    location: location__.unwrap_or_default(),
                    severity: severity__.unwrap_or_default(),
                    detected_at: detected_at__.unwrap_or_default(),
                    source: source__.unwrap_or_default(),
                    extra: extra__,
                    remediation: remediation__,
                    tags: tags__.unwrap_or_default(),
                    persisted_issue: persisted_issue__,
                    issue_type: issue_type__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.healthplatform.Issue", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for IssueReport {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.issue_id.is_empty() {
            len += 1;
        }
        if !self.context.is_empty() {
            len += 1;
        }
        if !self.tags.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.healthplatform.IssueReport", len)?;
        if !self.issue_id.is_empty() {
            struct_ser.serialize_field("issueId", &self.issue_id)?;
        }
        if !self.context.is_empty() {
            struct_ser.serialize_field("context", &self.context)?;
        }
        if !self.tags.is_empty() {
            struct_ser.serialize_field("tags", &self.tags)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for IssueReport {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "issue_id",
            "issueId",
            "context",
            "tags",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            IssueId,
            Context,
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
                            "issueId" | "issue_id" => Ok(GeneratedField::IssueId),
                            "context" => Ok(GeneratedField::Context),
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
            type Value = IssueReport;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.healthplatform.IssueReport")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<IssueReport, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut issue_id__ = None;
                let mut context__ = None;
                let mut tags__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::IssueId => {
                            if issue_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("issueId"));
                            }
                            issue_id__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Context => {
                            if context__.is_some() {
                                return Err(serde::de::Error::duplicate_field("context"));
                            }
                            context__ = Some(
                                map_.next_value::<std::collections::HashMap<_, _>>()?
                            );
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
                Ok(IssueReport {
                    issue_id: issue_id__.unwrap_or_default(),
                    context: context__.unwrap_or_default(),
                    tags: tags__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.healthplatform.IssueReport", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for IssueSeverity {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let variant = match self {
            Self::Unspecified => "ISSUE_SEVERITY_UNSPECIFIED",
            Self::Low => "ISSUE_SEVERITY_LOW",
            Self::Medium => "ISSUE_SEVERITY_MEDIUM",
            Self::High => "ISSUE_SEVERITY_HIGH",
        };
        serializer.serialize_str(variant)
    }
}
impl<'de> serde::Deserialize<'de> for IssueSeverity {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "ISSUE_SEVERITY_UNSPECIFIED",
            "ISSUE_SEVERITY_LOW",
            "ISSUE_SEVERITY_MEDIUM",
            "ISSUE_SEVERITY_HIGH",
        ];

        struct GeneratedVisitor;

        impl serde::de::Visitor<'_> for GeneratedVisitor {
            type Value = IssueSeverity;

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
                    "ISSUE_SEVERITY_UNSPECIFIED" => Ok(IssueSeverity::Unspecified),
                    "ISSUE_SEVERITY_LOW" => Ok(IssueSeverity::Low),
                    "ISSUE_SEVERITY_MEDIUM" => Ok(IssueSeverity::Medium),
                    "ISSUE_SEVERITY_HIGH" => Ok(IssueSeverity::High),
                    _ => Err(serde::de::Error::unknown_variant(value, FIELDS)),
                }
            }
        }
        deserializer.deserialize_any(GeneratedVisitor)
    }
}
impl serde::Serialize for IssueState {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let variant = match self {
            Self::Unspecified => "ISSUE_STATE_UNSPECIFIED",
            Self::New => "ISSUE_STATE_NEW",
            Self::Ongoing => "ISSUE_STATE_ONGOING",
            Self::Resolved => "ISSUE_STATE_RESOLVED",
            Self::Active => "ISSUE_STATE_ACTIVE",
        };
        serializer.serialize_str(variant)
    }
}
impl<'de> serde::Deserialize<'de> for IssueState {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "ISSUE_STATE_UNSPECIFIED",
            "ISSUE_STATE_NEW",
            "ISSUE_STATE_ONGOING",
            "ISSUE_STATE_RESOLVED",
            "ISSUE_STATE_ACTIVE",
        ];

        struct GeneratedVisitor;

        impl serde::de::Visitor<'_> for GeneratedVisitor {
            type Value = IssueState;

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
                    "ISSUE_STATE_UNSPECIFIED" => Ok(IssueState::Unspecified),
                    "ISSUE_STATE_NEW" => Ok(IssueState::New),
                    "ISSUE_STATE_ONGOING" => Ok(IssueState::Ongoing),
                    "ISSUE_STATE_RESOLVED" => Ok(IssueState::Resolved),
                    "ISSUE_STATE_ACTIVE" => Ok(IssueState::Active),
                    _ => Err(serde::de::Error::unknown_variant(value, FIELDS)),
                }
            }
        }
        deserializer.deserialize_any(GeneratedVisitor)
    }
}
impl serde::Serialize for JsonapiMeta {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.schema_version.is_empty() {
            len += 1;
        }
        if !self.event_type.is_empty() {
            len += 1;
        }
        if !self.emitted_at.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.healthplatform.JSONAPIMeta", len)?;
        if !self.schema_version.is_empty() {
            struct_ser.serialize_field("schemaVersion", &self.schema_version)?;
        }
        if !self.event_type.is_empty() {
            struct_ser.serialize_field("eventType", &self.event_type)?;
        }
        if !self.emitted_at.is_empty() {
            struct_ser.serialize_field("emittedAt", &self.emitted_at)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for JsonapiMeta {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "schema_version",
            "schemaVersion",
            "event_type",
            "eventType",
            "emitted_at",
            "emittedAt",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            SchemaVersion,
            EventType,
            EmittedAt,
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
                            "schemaVersion" | "schema_version" => Ok(GeneratedField::SchemaVersion),
                            "eventType" | "event_type" => Ok(GeneratedField::EventType),
                            "emittedAt" | "emitted_at" => Ok(GeneratedField::EmittedAt),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = JsonapiMeta;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.healthplatform.JSONAPIMeta")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<JsonapiMeta, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut schema_version__ = None;
                let mut event_type__ = None;
                let mut emitted_at__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::SchemaVersion => {
                            if schema_version__.is_some() {
                                return Err(serde::de::Error::duplicate_field("schemaVersion"));
                            }
                            schema_version__ = Some(map_.next_value()?);
                        }
                        GeneratedField::EventType => {
                            if event_type__.is_some() {
                                return Err(serde::de::Error::duplicate_field("eventType"));
                            }
                            event_type__ = Some(map_.next_value()?);
                        }
                        GeneratedField::EmittedAt => {
                            if emitted_at__.is_some() {
                                return Err(serde::de::Error::duplicate_field("emittedAt"));
                            }
                            emitted_at__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(JsonapiMeta {
                    schema_version: schema_version__.unwrap_or_default(),
                    event_type: event_type__.unwrap_or_default(),
                    emitted_at: emitted_at__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.healthplatform.JSONAPIMeta", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for JsonapiResponse {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.data.is_some() {
            len += 1;
        }
        if self.meta.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.healthplatform.JSONAPIResponse", len)?;
        if let Some(v) = self.data.as_ref() {
            struct_ser.serialize_field("data", v)?;
        }
        if let Some(v) = self.meta.as_ref() {
            struct_ser.serialize_field("meta", v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for JsonapiResponse {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "data",
            "meta",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Data,
            Meta,
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
                            "data" => Ok(GeneratedField::Data),
                            "meta" => Ok(GeneratedField::Meta),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = JsonapiResponse;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.healthplatform.JSONAPIResponse")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<JsonapiResponse, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut data__ = None;
                let mut meta__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Data => {
                            if data__.is_some() {
                                return Err(serde::de::Error::duplicate_field("data"));
                            }
                            data__ = map_.next_value()?;
                        }
                        GeneratedField::Meta => {
                            if meta__.is_some() {
                                return Err(serde::de::Error::duplicate_field("meta"));
                            }
                            meta__ = map_.next_value()?;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(JsonapiResponse {
                    data: data__,
                    meta: meta__,
                })
            }
        }
        deserializer.deserialize_struct("datadog.healthplatform.JSONAPIResponse", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for PersistedIssue {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.state != 0 {
            len += 1;
        }
        if !self.first_seen.is_empty() {
            len += 1;
        }
        if !self.last_seen.is_empty() {
            len += 1;
        }
        if self.resolved_at.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.healthplatform.PersistedIssue", len)?;
        if self.state != 0 {
            let v = IssueState::try_from(self.state)
                .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", self.state)))?;
            struct_ser.serialize_field("state", &v)?;
        }
        if !self.first_seen.is_empty() {
            struct_ser.serialize_field("firstSeen", &self.first_seen)?;
        }
        if !self.last_seen.is_empty() {
            struct_ser.serialize_field("lastSeen", &self.last_seen)?;
        }
        if let Some(v) = self.resolved_at.as_ref() {
            struct_ser.serialize_field("resolvedAt", v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for PersistedIssue {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "state",
            "first_seen",
            "firstSeen",
            "last_seen",
            "lastSeen",
            "resolved_at",
            "resolvedAt",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            State,
            FirstSeen,
            LastSeen,
            ResolvedAt,
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
                            "firstSeen" | "first_seen" => Ok(GeneratedField::FirstSeen),
                            "lastSeen" | "last_seen" => Ok(GeneratedField::LastSeen),
                            "resolvedAt" | "resolved_at" => Ok(GeneratedField::ResolvedAt),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = PersistedIssue;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.healthplatform.PersistedIssue")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<PersistedIssue, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut state__ = None;
                let mut first_seen__ = None;
                let mut last_seen__ = None;
                let mut resolved_at__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::State => {
                            if state__.is_some() {
                                return Err(serde::de::Error::duplicate_field("state"));
                            }
                            state__ = Some(map_.next_value::<IssueState>()? as i32);
                        }
                        GeneratedField::FirstSeen => {
                            if first_seen__.is_some() {
                                return Err(serde::de::Error::duplicate_field("firstSeen"));
                            }
                            first_seen__ = Some(map_.next_value()?);
                        }
                        GeneratedField::LastSeen => {
                            if last_seen__.is_some() {
                                return Err(serde::de::Error::duplicate_field("lastSeen"));
                            }
                            last_seen__ = Some(map_.next_value()?);
                        }
                        GeneratedField::ResolvedAt => {
                            if resolved_at__.is_some() {
                                return Err(serde::de::Error::duplicate_field("resolvedAt"));
                            }
                            resolved_at__ = map_.next_value()?;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(PersistedIssue {
                    state: state__.unwrap_or_default(),
                    first_seen: first_seen__.unwrap_or_default(),
                    last_seen: last_seen__.unwrap_or_default(),
                    resolved_at: resolved_at__,
                })
            }
        }
        deserializer.deserialize_struct("datadog.healthplatform.PersistedIssue", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for Remediation {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.summary.is_empty() {
            len += 1;
        }
        if !self.steps.is_empty() {
            len += 1;
        }
        if self.script.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.healthplatform.Remediation", len)?;
        if !self.summary.is_empty() {
            struct_ser.serialize_field("summary", &self.summary)?;
        }
        if !self.steps.is_empty() {
            struct_ser.serialize_field("steps", &self.steps)?;
        }
        if let Some(v) = self.script.as_ref() {
            struct_ser.serialize_field("script", v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for Remediation {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "summary",
            "steps",
            "script",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Summary,
            Steps,
            Script,
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
                            "summary" => Ok(GeneratedField::Summary),
                            "steps" => Ok(GeneratedField::Steps),
                            "script" => Ok(GeneratedField::Script),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = Remediation;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.healthplatform.Remediation")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<Remediation, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut summary__ = None;
                let mut steps__ = None;
                let mut script__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Summary => {
                            if summary__.is_some() {
                                return Err(serde::de::Error::duplicate_field("summary"));
                            }
                            summary__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Steps => {
                            if steps__.is_some() {
                                return Err(serde::de::Error::duplicate_field("steps"));
                            }
                            steps__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Script => {
                            if script__.is_some() {
                                return Err(serde::de::Error::duplicate_field("script"));
                            }
                            script__ = map_.next_value()?;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(Remediation {
                    summary: summary__.unwrap_or_default(),
                    steps: steps__.unwrap_or_default(),
                    script: script__,
                })
            }
        }
        deserializer.deserialize_struct("datadog.healthplatform.Remediation", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for RemediationStep {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.order != 0 {
            len += 1;
        }
        if !self.text.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.healthplatform.RemediationStep", len)?;
        if self.order != 0 {
            struct_ser.serialize_field("order", &self.order)?;
        }
        if !self.text.is_empty() {
            struct_ser.serialize_field("text", &self.text)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for RemediationStep {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "order",
            "text",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Order,
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
                            "order" => Ok(GeneratedField::Order),
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
            type Value = RemediationStep;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.healthplatform.RemediationStep")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<RemediationStep, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut order__ = None;
                let mut text__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Order => {
                            if order__.is_some() {
                                return Err(serde::de::Error::duplicate_field("order"));
                            }
                            order__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
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
                Ok(RemediationStep {
                    order: order__.unwrap_or_default(),
                    text: text__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.healthplatform.RemediationStep", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for Script {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.language.is_empty() {
            len += 1;
        }
        if !self.language_version.is_empty() {
            len += 1;
        }
        if !self.filename.is_empty() {
            len += 1;
        }
        if self.requires_root {
            len += 1;
        }
        if !self.content.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.healthplatform.Script", len)?;
        if !self.language.is_empty() {
            struct_ser.serialize_field("language", &self.language)?;
        }
        if !self.language_version.is_empty() {
            struct_ser.serialize_field("languageVersion", &self.language_version)?;
        }
        if !self.filename.is_empty() {
            struct_ser.serialize_field("filename", &self.filename)?;
        }
        if self.requires_root {
            struct_ser.serialize_field("requiresRoot", &self.requires_root)?;
        }
        if !self.content.is_empty() {
            struct_ser.serialize_field("content", &self.content)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for Script {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "language",
            "language_version",
            "languageVersion",
            "filename",
            "requires_root",
            "requiresRoot",
            "content",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Language,
            LanguageVersion,
            Filename,
            RequiresRoot,
            Content,
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
                            "language" => Ok(GeneratedField::Language),
                            "languageVersion" | "language_version" => Ok(GeneratedField::LanguageVersion),
                            "filename" => Ok(GeneratedField::Filename),
                            "requiresRoot" | "requires_root" => Ok(GeneratedField::RequiresRoot),
                            "content" => Ok(GeneratedField::Content),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = Script;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.healthplatform.Script")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<Script, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut language__ = None;
                let mut language_version__ = None;
                let mut filename__ = None;
                let mut requires_root__ = None;
                let mut content__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Language => {
                            if language__.is_some() {
                                return Err(serde::de::Error::duplicate_field("language"));
                            }
                            language__ = Some(map_.next_value()?);
                        }
                        GeneratedField::LanguageVersion => {
                            if language_version__.is_some() {
                                return Err(serde::de::Error::duplicate_field("languageVersion"));
                            }
                            language_version__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Filename => {
                            if filename__.is_some() {
                                return Err(serde::de::Error::duplicate_field("filename"));
                            }
                            filename__ = Some(map_.next_value()?);
                        }
                        GeneratedField::RequiresRoot => {
                            if requires_root__.is_some() {
                                return Err(serde::de::Error::duplicate_field("requiresRoot"));
                            }
                            requires_root__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Content => {
                            if content__.is_some() {
                                return Err(serde::de::Error::duplicate_field("content"));
                            }
                            content__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(Script {
                    language: language__.unwrap_or_default(),
                    language_version: language_version__.unwrap_or_default(),
                    filename: filename__.unwrap_or_default(),
                    requires_root: requires_root__.unwrap_or_default(),
                    content: content__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.healthplatform.Script", FIELDS, GeneratedVisitor)
    }
}
