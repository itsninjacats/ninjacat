impl serde::Serialize for ScanStats {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.scan_duration_ms != 0 {
            len += 1;
        }
        if self.total_files_found != 0 {
            len += 1;
        }
        if self.files_scanned != 0 {
            len += 1;
        }
        if self.files_skipped_unsupported_type != 0 {
            len += 1;
        }
        if self.files_partially_scanned_size_limit != 0 {
            len += 1;
        }
        if self.total_data_scanned_bytes != 0 {
            len += 1;
        }
        if !self.skipped_files_by_type.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.sds.ScanStats", len)?;
        if self.scan_duration_ms != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("scanDurationMs", ToString::to_string(&self.scan_duration_ms).as_str())?;
        }
        if self.total_files_found != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("totalFilesFound", ToString::to_string(&self.total_files_found).as_str())?;
        }
        if self.files_scanned != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("filesScanned", ToString::to_string(&self.files_scanned).as_str())?;
        }
        if self.files_skipped_unsupported_type != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("filesSkippedUnsupportedType", ToString::to_string(&self.files_skipped_unsupported_type).as_str())?;
        }
        if self.files_partially_scanned_size_limit != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("filesPartiallyScannedSizeLimit", ToString::to_string(&self.files_partially_scanned_size_limit).as_str())?;
        }
        if self.total_data_scanned_bytes != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("totalDataScannedBytes", ToString::to_string(&self.total_data_scanned_bytes).as_str())?;
        }
        if !self.skipped_files_by_type.is_empty() {
            let v: std::collections::HashMap<_, _> = self.skipped_files_by_type.iter()
                .map(|(k, v)| (k, v.to_string())).collect();
            struct_ser.serialize_field("skippedFilesByType", &v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for ScanStats {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "scan_duration_ms",
            "scanDurationMs",
            "total_files_found",
            "totalFilesFound",
            "files_scanned",
            "filesScanned",
            "files_skipped_unsupported_type",
            "filesSkippedUnsupportedType",
            "files_partially_scanned_size_limit",
            "filesPartiallyScannedSizeLimit",
            "total_data_scanned_bytes",
            "totalDataScannedBytes",
            "skipped_files_by_type",
            "skippedFilesByType",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            ScanDurationMs,
            TotalFilesFound,
            FilesScanned,
            FilesSkippedUnsupportedType,
            FilesPartiallyScannedSizeLimit,
            TotalDataScannedBytes,
            SkippedFilesByType,
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
                            "scanDurationMs" | "scan_duration_ms" => Ok(GeneratedField::ScanDurationMs),
                            "totalFilesFound" | "total_files_found" => Ok(GeneratedField::TotalFilesFound),
                            "filesScanned" | "files_scanned" => Ok(GeneratedField::FilesScanned),
                            "filesSkippedUnsupportedType" | "files_skipped_unsupported_type" => Ok(GeneratedField::FilesSkippedUnsupportedType),
                            "filesPartiallyScannedSizeLimit" | "files_partially_scanned_size_limit" => Ok(GeneratedField::FilesPartiallyScannedSizeLimit),
                            "totalDataScannedBytes" | "total_data_scanned_bytes" => Ok(GeneratedField::TotalDataScannedBytes),
                            "skippedFilesByType" | "skipped_files_by_type" => Ok(GeneratedField::SkippedFilesByType),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = ScanStats;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.sds.ScanStats")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<ScanStats, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut scan_duration_ms__ = None;
                let mut total_files_found__ = None;
                let mut files_scanned__ = None;
                let mut files_skipped_unsupported_type__ = None;
                let mut files_partially_scanned_size_limit__ = None;
                let mut total_data_scanned_bytes__ = None;
                let mut skipped_files_by_type__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::ScanDurationMs => {
                            if scan_duration_ms__.is_some() {
                                return Err(serde::de::Error::duplicate_field("scanDurationMs"));
                            }
                            scan_duration_ms__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::TotalFilesFound => {
                            if total_files_found__.is_some() {
                                return Err(serde::de::Error::duplicate_field("totalFilesFound"));
                            }
                            total_files_found__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::FilesScanned => {
                            if files_scanned__.is_some() {
                                return Err(serde::de::Error::duplicate_field("filesScanned"));
                            }
                            files_scanned__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::FilesSkippedUnsupportedType => {
                            if files_skipped_unsupported_type__.is_some() {
                                return Err(serde::de::Error::duplicate_field("filesSkippedUnsupportedType"));
                            }
                            files_skipped_unsupported_type__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::FilesPartiallyScannedSizeLimit => {
                            if files_partially_scanned_size_limit__.is_some() {
                                return Err(serde::de::Error::duplicate_field("filesPartiallyScannedSizeLimit"));
                            }
                            files_partially_scanned_size_limit__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::TotalDataScannedBytes => {
                            if total_data_scanned_bytes__.is_some() {
                                return Err(serde::de::Error::duplicate_field("totalDataScannedBytes"));
                            }
                            total_data_scanned_bytes__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::SkippedFilesByType => {
                            if skipped_files_by_type__.is_some() {
                                return Err(serde::de::Error::duplicate_field("skippedFilesByType"));
                            }
                            skipped_files_by_type__ = Some(
                                map_.next_value::<std::collections::HashMap<_, ::pbjson::private::NumberDeserialize<i64>>>()?
                                    .into_iter().map(|(k,v)| (k, v.0)).collect()
                            );
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(ScanStats {
                    scan_duration_ms: scan_duration_ms__.unwrap_or_default(),
                    total_files_found: total_files_found__.unwrap_or_default(),
                    files_scanned: files_scanned__.unwrap_or_default(),
                    files_skipped_unsupported_type: files_skipped_unsupported_type__.unwrap_or_default(),
                    files_partially_scanned_size_limit: files_partially_scanned_size_limit__.unwrap_or_default(),
                    total_data_scanned_bytes: total_data_scanned_bytes__.unwrap_or_default(),
                    skipped_files_by_type: skipped_files_by_type__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.sds.ScanStats", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for ScannerMetadata {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.version.is_some() {
            len += 1;
        }
        if self.region.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.sds.ScannerMetadata", len)?;
        if let Some(v) = self.version.as_ref() {
            struct_ser.serialize_field("version", v)?;
        }
        if let Some(v) = self.region.as_ref() {
            struct_ser.serialize_field("region", v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for ScannerMetadata {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "version",
            "region",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Version,
            Region,
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
                            "region" => Ok(GeneratedField::Region),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = ScannerMetadata;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.sds.ScannerMetadata")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<ScannerMetadata, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut version__ = None;
                let mut region__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Version => {
                            if version__.is_some() {
                                return Err(serde::de::Error::duplicate_field("version"));
                            }
                            version__ = map_.next_value()?;
                        }
                        GeneratedField::Region => {
                            if region__.is_some() {
                                return Err(serde::de::Error::duplicate_field("region"));
                            }
                            region__ = map_.next_value()?;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(ScannerMetadata {
                    version: version__,
                    region: region__,
                })
            }
        }
        deserializer.deserialize_struct("datadog.sds.ScannerMetadata", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for ScanningSource {
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
        let mut struct_ser = serializer.serialize_struct("datadog.sds.ScanningSource", len)?;
        if let Some(v) = self.source.as_ref() {
            match v {
                scanning_source::Source::Agentless(v) => {
                    struct_ser.serialize_field("agentless", v)?;
                }
                scanning_source::Source::DatadogCrawler(v) => {
                    struct_ser.serialize_field("datadogCrawler", v)?;
                }
                scanning_source::Source::Agent(v) => {
                    struct_ser.serialize_field("agent", v)?;
                }
            }
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for ScanningSource {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "agentless",
            "datadog_crawler",
            "datadogCrawler",
            "agent",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Agentless,
            DatadogCrawler,
            Agent,
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
                            "agentless" => Ok(GeneratedField::Agentless),
                            "datadogCrawler" | "datadog_crawler" => Ok(GeneratedField::DatadogCrawler),
                            "agent" => Ok(GeneratedField::Agent),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = ScanningSource;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.sds.ScanningSource")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<ScanningSource, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut source__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Agentless => {
                            if source__.is_some() {
                                return Err(serde::de::Error::duplicate_field("agentless"));
                            }
                            source__ = map_.next_value::<::std::option::Option<_>>()?.map(scanning_source::Source::Agentless)
;
                        }
                        GeneratedField::DatadogCrawler => {
                            if source__.is_some() {
                                return Err(serde::de::Error::duplicate_field("datadogCrawler"));
                            }
                            source__ = map_.next_value::<::std::option::Option<_>>()?.map(scanning_source::Source::DatadogCrawler)
;
                        }
                        GeneratedField::Agent => {
                            if source__.is_some() {
                                return Err(serde::de::Error::duplicate_field("agent"));
                            }
                            source__ = map_.next_value::<::std::option::Option<_>>()?.map(scanning_source::Source::Agent)
;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(ScanningSource {
                    source: source__,
                })
            }
        }
        deserializer.deserialize_struct("datadog.sds.ScanningSource", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for scanning_source::Agent {
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
        if !self.version.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.sds.ScanningSource.Agent", len)?;
        if !self.hostname.is_empty() {
            struct_ser.serialize_field("hostname", &self.hostname)?;
        }
        if !self.version.is_empty() {
            struct_ser.serialize_field("version", &self.version)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for scanning_source::Agent {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "hostname",
            "version",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Hostname,
            Version,
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
                            "version" => Ok(GeneratedField::Version),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = scanning_source::Agent;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.sds.ScanningSource.Agent")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<scanning_source::Agent, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut hostname__ = None;
                let mut version__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Hostname => {
                            if hostname__.is_some() {
                                return Err(serde::de::Error::duplicate_field("hostname"));
                            }
                            hostname__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Version => {
                            if version__.is_some() {
                                return Err(serde::de::Error::duplicate_field("version"));
                            }
                            version__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(scanning_source::Agent {
                    hostname: hostname__.unwrap_or_default(),
                    version: version__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.sds.ScanningSource.Agent", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for scanning_source::Agentless {
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
        if !self.region.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.sds.ScanningSource.Agentless", len)?;
        if !self.version.is_empty() {
            struct_ser.serialize_field("version", &self.version)?;
        }
        if !self.region.is_empty() {
            struct_ser.serialize_field("region", &self.region)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for scanning_source::Agentless {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "version",
            "region",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Version,
            Region,
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
                            "region" => Ok(GeneratedField::Region),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = scanning_source::Agentless;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.sds.ScanningSource.Agentless")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<scanning_source::Agentless, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut version__ = None;
                let mut region__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Version => {
                            if version__.is_some() {
                                return Err(serde::de::Error::duplicate_field("version"));
                            }
                            version__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Region => {
                            if region__.is_some() {
                                return Err(serde::de::Error::duplicate_field("region"));
                            }
                            region__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(scanning_source::Agentless {
                    version: version__.unwrap_or_default(),
                    region: region__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.sds.ScanningSource.Agentless", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for scanning_source::DatadogCrawler {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.service_name.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.sds.ScanningSource.DatadogCrawler", len)?;
        if !self.service_name.is_empty() {
            struct_ser.serialize_field("serviceName", &self.service_name)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for scanning_source::DatadogCrawler {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "service_name",
            "serviceName",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            ServiceName,
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
                            "serviceName" | "service_name" => Ok(GeneratedField::ServiceName),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = scanning_source::DatadogCrawler;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.sds.ScanningSource.DatadogCrawler")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<scanning_source::DatadogCrawler, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut service_name__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::ServiceName => {
                            if service_name__.is_some() {
                                return Err(serde::de::Error::duplicate_field("serviceName"));
                            }
                            service_name__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(scanning_source::DatadogCrawler {
                    service_name: service_name__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.sds.ScanningSource.DatadogCrawler", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for SdsResultPayload {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.scan_source != 0 {
            len += 1;
        }
        if self.timestamp != 0 {
            len += 1;
        }
        if self.resource.is_some() {
            len += 1;
        }
        if !self.scan_results.is_empty() {
            len += 1;
        }
        if self.scan_stats.is_some() {
            len += 1;
        }
        if self.scanner_metadata.is_some() {
            len += 1;
        }
        if !self.rules.is_empty() {
            len += 1;
        }
        if self.scanning_source.is_some() {
            len += 1;
        }
        if !self.rule_ids.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.sds.SdsResultPayload", len)?;
        if self.scan_source != 0 {
            let v = sds_result_payload::ScanSource::try_from(self.scan_source)
                .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", self.scan_source)))?;
            struct_ser.serialize_field("scanSource", &v)?;
        }
        if self.timestamp != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("timestamp", ToString::to_string(&self.timestamp).as_str())?;
        }
        if let Some(v) = self.resource.as_ref() {
            struct_ser.serialize_field("resource", v)?;
        }
        if !self.scan_results.is_empty() {
            struct_ser.serialize_field("scanResults", &self.scan_results)?;
        }
        if let Some(v) = self.scan_stats.as_ref() {
            struct_ser.serialize_field("scanStats", v)?;
        }
        if let Some(v) = self.scanner_metadata.as_ref() {
            struct_ser.serialize_field("scannerMetadata", v)?;
        }
        if !self.rules.is_empty() {
            struct_ser.serialize_field("rules", &self.rules)?;
        }
        if let Some(v) = self.scanning_source.as_ref() {
            struct_ser.serialize_field("scanningSource", v)?;
        }
        if !self.rule_ids.is_empty() {
            struct_ser.serialize_field("ruleIds", &self.rule_ids)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for SdsResultPayload {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "scan_source",
            "scanSource",
            "timestamp",
            "resource",
            "scan_results",
            "scanResults",
            "scan_stats",
            "scanStats",
            "scanner_metadata",
            "scannerMetadata",
            "rules",
            "scanning_source",
            "scanningSource",
            "rule_ids",
            "ruleIds",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            ScanSource,
            Timestamp,
            Resource,
            ScanResults,
            ScanStats,
            ScannerMetadata,
            Rules,
            ScanningSource,
            RuleIds,
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
                            "scanSource" | "scan_source" => Ok(GeneratedField::ScanSource),
                            "timestamp" => Ok(GeneratedField::Timestamp),
                            "resource" => Ok(GeneratedField::Resource),
                            "scanResults" | "scan_results" => Ok(GeneratedField::ScanResults),
                            "scanStats" | "scan_stats" => Ok(GeneratedField::ScanStats),
                            "scannerMetadata" | "scanner_metadata" => Ok(GeneratedField::ScannerMetadata),
                            "rules" => Ok(GeneratedField::Rules),
                            "scanningSource" | "scanning_source" => Ok(GeneratedField::ScanningSource),
                            "ruleIds" | "rule_ids" => Ok(GeneratedField::RuleIds),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = SdsResultPayload;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.sds.SdsResultPayload")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<SdsResultPayload, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut scan_source__ = None;
                let mut timestamp__ = None;
                let mut resource__ = None;
                let mut scan_results__ = None;
                let mut scan_stats__ = None;
                let mut scanner_metadata__ = None;
                let mut rules__ = None;
                let mut scanning_source__ = None;
                let mut rule_ids__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::ScanSource => {
                            if scan_source__.is_some() {
                                return Err(serde::de::Error::duplicate_field("scanSource"));
                            }
                            scan_source__ = Some(map_.next_value::<sds_result_payload::ScanSource>()? as i32);
                        }
                        GeneratedField::Timestamp => {
                            if timestamp__.is_some() {
                                return Err(serde::de::Error::duplicate_field("timestamp"));
                            }
                            timestamp__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Resource => {
                            if resource__.is_some() {
                                return Err(serde::de::Error::duplicate_field("resource"));
                            }
                            resource__ = map_.next_value()?;
                        }
                        GeneratedField::ScanResults => {
                            if scan_results__.is_some() {
                                return Err(serde::de::Error::duplicate_field("scanResults"));
                            }
                            scan_results__ = Some(map_.next_value()?);
                        }
                        GeneratedField::ScanStats => {
                            if scan_stats__.is_some() {
                                return Err(serde::de::Error::duplicate_field("scanStats"));
                            }
                            scan_stats__ = map_.next_value()?;
                        }
                        GeneratedField::ScannerMetadata => {
                            if scanner_metadata__.is_some() {
                                return Err(serde::de::Error::duplicate_field("scannerMetadata"));
                            }
                            scanner_metadata__ = map_.next_value()?;
                        }
                        GeneratedField::Rules => {
                            if rules__.is_some() {
                                return Err(serde::de::Error::duplicate_field("rules"));
                            }
                            rules__ = Some(
                                map_.next_value::<std::collections::HashMap<_, _>>()?
                            );
                        }
                        GeneratedField::ScanningSource => {
                            if scanning_source__.is_some() {
                                return Err(serde::de::Error::duplicate_field("scanningSource"));
                            }
                            scanning_source__ = map_.next_value()?;
                        }
                        GeneratedField::RuleIds => {
                            if rule_ids__.is_some() {
                                return Err(serde::de::Error::duplicate_field("ruleIds"));
                            }
                            rule_ids__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(SdsResultPayload {
                    scan_source: scan_source__.unwrap_or_default(),
                    timestamp: timestamp__.unwrap_or_default(),
                    resource: resource__,
                    scan_results: scan_results__.unwrap_or_default(),
                    scan_stats: scan_stats__,
                    scanner_metadata: scanner_metadata__,
                    rules: rules__.unwrap_or_default(),
                    scanning_source: scanning_source__,
                    rule_ids: rule_ids__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.sds.SdsResultPayload", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for sds_result_payload::PostgresTable {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.database_cluster_name.is_empty() {
            len += 1;
        }
        if !self.database_instance_name.is_empty() {
            len += 1;
        }
        if !self.database_host_name.is_empty() {
            len += 1;
        }
        if !self.database_name.is_empty() {
            len += 1;
        }
        if !self.schema_name.is_empty() {
            len += 1;
        }
        if !self.table_name.is_empty() {
            len += 1;
        }
        if self.table_row_count != 0 {
            len += 1;
        }
        if self.scanned_row_count != 0 {
            len += 1;
        }
        if !self.scanned_columns.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.sds.SdsResultPayload.PostgresTable", len)?;
        if !self.database_cluster_name.is_empty() {
            struct_ser.serialize_field("databaseClusterName", &self.database_cluster_name)?;
        }
        if !self.database_instance_name.is_empty() {
            struct_ser.serialize_field("databaseInstanceName", &self.database_instance_name)?;
        }
        if !self.database_host_name.is_empty() {
            struct_ser.serialize_field("databaseHostName", &self.database_host_name)?;
        }
        if !self.database_name.is_empty() {
            struct_ser.serialize_field("databaseName", &self.database_name)?;
        }
        if !self.schema_name.is_empty() {
            struct_ser.serialize_field("schemaName", &self.schema_name)?;
        }
        if !self.table_name.is_empty() {
            struct_ser.serialize_field("tableName", &self.table_name)?;
        }
        if self.table_row_count != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("tableRowCount", ToString::to_string(&self.table_row_count).as_str())?;
        }
        if self.scanned_row_count != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("scannedRowCount", ToString::to_string(&self.scanned_row_count).as_str())?;
        }
        if !self.scanned_columns.is_empty() {
            struct_ser.serialize_field("scannedColumns", &self.scanned_columns)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for sds_result_payload::PostgresTable {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "database_cluster_name",
            "databaseClusterName",
            "database_instance_name",
            "databaseInstanceName",
            "database_host_name",
            "databaseHostName",
            "database_name",
            "databaseName",
            "schema_name",
            "schemaName",
            "table_name",
            "tableName",
            "table_row_count",
            "tableRowCount",
            "scanned_row_count",
            "scannedRowCount",
            "scanned_columns",
            "scannedColumns",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            DatabaseClusterName,
            DatabaseInstanceName,
            DatabaseHostName,
            DatabaseName,
            SchemaName,
            TableName,
            TableRowCount,
            ScannedRowCount,
            ScannedColumns,
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
                            "databaseClusterName" | "database_cluster_name" => Ok(GeneratedField::DatabaseClusterName),
                            "databaseInstanceName" | "database_instance_name" => Ok(GeneratedField::DatabaseInstanceName),
                            "databaseHostName" | "database_host_name" => Ok(GeneratedField::DatabaseHostName),
                            "databaseName" | "database_name" => Ok(GeneratedField::DatabaseName),
                            "schemaName" | "schema_name" => Ok(GeneratedField::SchemaName),
                            "tableName" | "table_name" => Ok(GeneratedField::TableName),
                            "tableRowCount" | "table_row_count" => Ok(GeneratedField::TableRowCount),
                            "scannedRowCount" | "scanned_row_count" => Ok(GeneratedField::ScannedRowCount),
                            "scannedColumns" | "scanned_columns" => Ok(GeneratedField::ScannedColumns),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = sds_result_payload::PostgresTable;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.sds.SdsResultPayload.PostgresTable")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<sds_result_payload::PostgresTable, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut database_cluster_name__ = None;
                let mut database_instance_name__ = None;
                let mut database_host_name__ = None;
                let mut database_name__ = None;
                let mut schema_name__ = None;
                let mut table_name__ = None;
                let mut table_row_count__ = None;
                let mut scanned_row_count__ = None;
                let mut scanned_columns__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::DatabaseClusterName => {
                            if database_cluster_name__.is_some() {
                                return Err(serde::de::Error::duplicate_field("databaseClusterName"));
                            }
                            database_cluster_name__ = Some(map_.next_value()?);
                        }
                        GeneratedField::DatabaseInstanceName => {
                            if database_instance_name__.is_some() {
                                return Err(serde::de::Error::duplicate_field("databaseInstanceName"));
                            }
                            database_instance_name__ = Some(map_.next_value()?);
                        }
                        GeneratedField::DatabaseHostName => {
                            if database_host_name__.is_some() {
                                return Err(serde::de::Error::duplicate_field("databaseHostName"));
                            }
                            database_host_name__ = Some(map_.next_value()?);
                        }
                        GeneratedField::DatabaseName => {
                            if database_name__.is_some() {
                                return Err(serde::de::Error::duplicate_field("databaseName"));
                            }
                            database_name__ = Some(map_.next_value()?);
                        }
                        GeneratedField::SchemaName => {
                            if schema_name__.is_some() {
                                return Err(serde::de::Error::duplicate_field("schemaName"));
                            }
                            schema_name__ = Some(map_.next_value()?);
                        }
                        GeneratedField::TableName => {
                            if table_name__.is_some() {
                                return Err(serde::de::Error::duplicate_field("tableName"));
                            }
                            table_name__ = Some(map_.next_value()?);
                        }
                        GeneratedField::TableRowCount => {
                            if table_row_count__.is_some() {
                                return Err(serde::de::Error::duplicate_field("tableRowCount"));
                            }
                            table_row_count__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::ScannedRowCount => {
                            if scanned_row_count__.is_some() {
                                return Err(serde::de::Error::duplicate_field("scannedRowCount"));
                            }
                            scanned_row_count__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::ScannedColumns => {
                            if scanned_columns__.is_some() {
                                return Err(serde::de::Error::duplicate_field("scannedColumns"));
                            }
                            scanned_columns__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(sds_result_payload::PostgresTable {
                    database_cluster_name: database_cluster_name__.unwrap_or_default(),
                    database_instance_name: database_instance_name__.unwrap_or_default(),
                    database_host_name: database_host_name__.unwrap_or_default(),
                    database_name: database_name__.unwrap_or_default(),
                    schema_name: schema_name__.unwrap_or_default(),
                    table_name: table_name__.unwrap_or_default(),
                    table_row_count: table_row_count__.unwrap_or_default(),
                    scanned_row_count: scanned_row_count__.unwrap_or_default(),
                    scanned_columns: scanned_columns__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.sds.SdsResultPayload.PostgresTable", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for sds_result_payload::postgres_table::ScannedColumn {
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
        if !self.data_type.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.sds.SdsResultPayload.PostgresTable.ScannedColumn", len)?;
        if !self.name.is_empty() {
            struct_ser.serialize_field("name", &self.name)?;
        }
        if !self.data_type.is_empty() {
            struct_ser.serialize_field("dataType", &self.data_type)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for sds_result_payload::postgres_table::ScannedColumn {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "name",
            "data_type",
            "dataType",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Name,
            DataType,
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
                            "dataType" | "data_type" => Ok(GeneratedField::DataType),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = sds_result_payload::postgres_table::ScannedColumn;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.sds.SdsResultPayload.PostgresTable.ScannedColumn")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<sds_result_payload::postgres_table::ScannedColumn, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut name__ = None;
                let mut data_type__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Name => {
                            if name__.is_some() {
                                return Err(serde::de::Error::duplicate_field("name"));
                            }
                            name__ = Some(map_.next_value()?);
                        }
                        GeneratedField::DataType => {
                            if data_type__.is_some() {
                                return Err(serde::de::Error::duplicate_field("dataType"));
                            }
                            data_type__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(sds_result_payload::postgres_table::ScannedColumn {
                    name: name__.unwrap_or_default(),
                    data_type: data_type__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.sds.SdsResultPayload.PostgresTable.ScannedColumn", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for sds_result_payload::RdsTable {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.instance_arn.is_empty() {
            len += 1;
        }
        if !self.snapshot_arn.is_empty() {
            len += 1;
        }
        if self.snapshot_timestamp != 0 {
            len += 1;
        }
        if !self.database_name.is_empty() {
            len += 1;
        }
        if !self.table_name.is_empty() {
            len += 1;
        }
        if self.table_row_count.is_some() {
            len += 1;
        }
        if self.scanned_row_count.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.sds.SdsResultPayload.RdsTable", len)?;
        if !self.instance_arn.is_empty() {
            struct_ser.serialize_field("instanceArn", &self.instance_arn)?;
        }
        if !self.snapshot_arn.is_empty() {
            struct_ser.serialize_field("snapshotArn", &self.snapshot_arn)?;
        }
        if self.snapshot_timestamp != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("snapshotTimestamp", ToString::to_string(&self.snapshot_timestamp).as_str())?;
        }
        if !self.database_name.is_empty() {
            struct_ser.serialize_field("databaseName", &self.database_name)?;
        }
        if !self.table_name.is_empty() {
            struct_ser.serialize_field("tableName", &self.table_name)?;
        }
        if let Some(v) = self.table_row_count.as_ref() {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("tableRowCount", ToString::to_string(&v).as_str())?;
        }
        if let Some(v) = self.scanned_row_count.as_ref() {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("scannedRowCount", ToString::to_string(&v).as_str())?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for sds_result_payload::RdsTable {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "instance_arn",
            "instanceArn",
            "snapshot_arn",
            "snapshotArn",
            "snapshot_timestamp",
            "snapshotTimestamp",
            "database_name",
            "databaseName",
            "table_name",
            "tableName",
            "table_row_count",
            "tableRowCount",
            "scanned_row_count",
            "scannedRowCount",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            InstanceArn,
            SnapshotArn,
            SnapshotTimestamp,
            DatabaseName,
            TableName,
            TableRowCount,
            ScannedRowCount,
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
                            "instanceArn" | "instance_arn" => Ok(GeneratedField::InstanceArn),
                            "snapshotArn" | "snapshot_arn" => Ok(GeneratedField::SnapshotArn),
                            "snapshotTimestamp" | "snapshot_timestamp" => Ok(GeneratedField::SnapshotTimestamp),
                            "databaseName" | "database_name" => Ok(GeneratedField::DatabaseName),
                            "tableName" | "table_name" => Ok(GeneratedField::TableName),
                            "tableRowCount" | "table_row_count" => Ok(GeneratedField::TableRowCount),
                            "scannedRowCount" | "scanned_row_count" => Ok(GeneratedField::ScannedRowCount),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = sds_result_payload::RdsTable;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.sds.SdsResultPayload.RdsTable")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<sds_result_payload::RdsTable, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut instance_arn__ = None;
                let mut snapshot_arn__ = None;
                let mut snapshot_timestamp__ = None;
                let mut database_name__ = None;
                let mut table_name__ = None;
                let mut table_row_count__ = None;
                let mut scanned_row_count__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::InstanceArn => {
                            if instance_arn__.is_some() {
                                return Err(serde::de::Error::duplicate_field("instanceArn"));
                            }
                            instance_arn__ = Some(map_.next_value()?);
                        }
                        GeneratedField::SnapshotArn => {
                            if snapshot_arn__.is_some() {
                                return Err(serde::de::Error::duplicate_field("snapshotArn"));
                            }
                            snapshot_arn__ = Some(map_.next_value()?);
                        }
                        GeneratedField::SnapshotTimestamp => {
                            if snapshot_timestamp__.is_some() {
                                return Err(serde::de::Error::duplicate_field("snapshotTimestamp"));
                            }
                            snapshot_timestamp__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::DatabaseName => {
                            if database_name__.is_some() {
                                return Err(serde::de::Error::duplicate_field("databaseName"));
                            }
                            database_name__ = Some(map_.next_value()?);
                        }
                        GeneratedField::TableName => {
                            if table_name__.is_some() {
                                return Err(serde::de::Error::duplicate_field("tableName"));
                            }
                            table_name__ = Some(map_.next_value()?);
                        }
                        GeneratedField::TableRowCount => {
                            if table_row_count__.is_some() {
                                return Err(serde::de::Error::duplicate_field("tableRowCount"));
                            }
                            table_row_count__ = 
                                map_.next_value::<::std::option::Option<::pbjson::private::NumberDeserialize<_>>>()?.map(|x| x.0)
                            ;
                        }
                        GeneratedField::ScannedRowCount => {
                            if scanned_row_count__.is_some() {
                                return Err(serde::de::Error::duplicate_field("scannedRowCount"));
                            }
                            scanned_row_count__ = 
                                map_.next_value::<::std::option::Option<::pbjson::private::NumberDeserialize<_>>>()?.map(|x| x.0)
                            ;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(sds_result_payload::RdsTable {
                    instance_arn: instance_arn__.unwrap_or_default(),
                    snapshot_arn: snapshot_arn__.unwrap_or_default(),
                    snapshot_timestamp: snapshot_timestamp__.unwrap_or_default(),
                    database_name: database_name__.unwrap_or_default(),
                    table_name: table_name__.unwrap_or_default(),
                    table_row_count: table_row_count__,
                    scanned_row_count: scanned_row_count__,
                })
            }
        }
        deserializer.deserialize_struct("datadog.sds.SdsResultPayload.RdsTable", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for sds_result_payload::Resource {
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
        let mut struct_ser = serializer.serialize_struct("datadog.sds.SdsResultPayload.Resource", len)?;
        if !self.r#type.is_empty() {
            struct_ser.serialize_field("type", &self.r#type)?;
        }
        if !self.name.is_empty() {
            struct_ser.serialize_field("name", &self.name)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for sds_result_payload::Resource {
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
            type Value = sds_result_payload::Resource;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.sds.SdsResultPayload.Resource")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<sds_result_payload::Resource, V::Error>
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
                Ok(sds_result_payload::Resource {
                    r#type: r#type__.unwrap_or_default(),
                    name: name__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.sds.SdsResultPayload.Resource", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for sds_result_payload::RuleInfo {
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
        if !self.name.is_empty() {
            len += 1;
        }
        if !self.priority.is_empty() {
            len += 1;
        }
        if !self.tags.is_empty() {
            len += 1;
        }
        if !self.labels.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.sds.SdsResultPayload.RuleInfo", len)?;
        if !self.id.is_empty() {
            struct_ser.serialize_field("id", &self.id)?;
        }
        if !self.name.is_empty() {
            struct_ser.serialize_field("name", &self.name)?;
        }
        if !self.priority.is_empty() {
            struct_ser.serialize_field("priority", &self.priority)?;
        }
        if !self.tags.is_empty() {
            struct_ser.serialize_field("tags", &self.tags)?;
        }
        if !self.labels.is_empty() {
            struct_ser.serialize_field("labels", &self.labels)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for sds_result_payload::RuleInfo {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "id",
            "name",
            "priority",
            "tags",
            "labels",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Id,
            Name,
            Priority,
            Tags,
            Labels,
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
                            "name" => Ok(GeneratedField::Name),
                            "priority" => Ok(GeneratedField::Priority),
                            "tags" => Ok(GeneratedField::Tags),
                            "labels" => Ok(GeneratedField::Labels),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = sds_result_payload::RuleInfo;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.sds.SdsResultPayload.RuleInfo")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<sds_result_payload::RuleInfo, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut id__ = None;
                let mut name__ = None;
                let mut priority__ = None;
                let mut tags__ = None;
                let mut labels__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Id => {
                            if id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("id"));
                            }
                            id__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Name => {
                            if name__.is_some() {
                                return Err(serde::de::Error::duplicate_field("name"));
                            }
                            name__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Priority => {
                            if priority__.is_some() {
                                return Err(serde::de::Error::duplicate_field("priority"));
                            }
                            priority__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Tags => {
                            if tags__.is_some() {
                                return Err(serde::de::Error::duplicate_field("tags"));
                            }
                            tags__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Labels => {
                            if labels__.is_some() {
                                return Err(serde::de::Error::duplicate_field("labels"));
                            }
                            labels__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(sds_result_payload::RuleInfo {
                    id: id__.unwrap_or_default(),
                    name: name__.unwrap_or_default(),
                    priority: priority__.unwrap_or_default(),
                    tags: tags__.unwrap_or_default(),
                    labels: labels__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.sds.SdsResultPayload.RuleInfo", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for sds_result_payload::S3File {
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
        if self.last_modified_timestamp != 0 {
            len += 1;
        }
        if self.compressed_size_in_bytes != 0 {
            len += 1;
        }
        if self.uncompressed_size_in_bytes != 0 {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.sds.SdsResultPayload.S3File", len)?;
        if !self.path.is_empty() {
            struct_ser.serialize_field("path", &self.path)?;
        }
        if self.last_modified_timestamp != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("lastModifiedTimestamp", ToString::to_string(&self.last_modified_timestamp).as_str())?;
        }
        if self.compressed_size_in_bytes != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("compressedSizeInBytes", ToString::to_string(&self.compressed_size_in_bytes).as_str())?;
        }
        if self.uncompressed_size_in_bytes != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("uncompressedSizeInBytes", ToString::to_string(&self.uncompressed_size_in_bytes).as_str())?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for sds_result_payload::S3File {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "path",
            "last_modified_timestamp",
            "lastModifiedTimestamp",
            "compressed_size_in_bytes",
            "compressedSizeInBytes",
            "uncompressed_size_in_bytes",
            "uncompressedSizeInBytes",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Path,
            LastModifiedTimestamp,
            CompressedSizeInBytes,
            UncompressedSizeInBytes,
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
                            "lastModifiedTimestamp" | "last_modified_timestamp" => Ok(GeneratedField::LastModifiedTimestamp),
                            "compressedSizeInBytes" | "compressed_size_in_bytes" => Ok(GeneratedField::CompressedSizeInBytes),
                            "uncompressedSizeInBytes" | "uncompressed_size_in_bytes" => Ok(GeneratedField::UncompressedSizeInBytes),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = sds_result_payload::S3File;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.sds.SdsResultPayload.S3File")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<sds_result_payload::S3File, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut path__ = None;
                let mut last_modified_timestamp__ = None;
                let mut compressed_size_in_bytes__ = None;
                let mut uncompressed_size_in_bytes__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Path => {
                            if path__.is_some() {
                                return Err(serde::de::Error::duplicate_field("path"));
                            }
                            path__ = Some(map_.next_value()?);
                        }
                        GeneratedField::LastModifiedTimestamp => {
                            if last_modified_timestamp__.is_some() {
                                return Err(serde::de::Error::duplicate_field("lastModifiedTimestamp"));
                            }
                            last_modified_timestamp__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::CompressedSizeInBytes => {
                            if compressed_size_in_bytes__.is_some() {
                                return Err(serde::de::Error::duplicate_field("compressedSizeInBytes"));
                            }
                            compressed_size_in_bytes__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::UncompressedSizeInBytes => {
                            if uncompressed_size_in_bytes__.is_some() {
                                return Err(serde::de::Error::duplicate_field("uncompressedSizeInBytes"));
                            }
                            uncompressed_size_in_bytes__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(sds_result_payload::S3File {
                    path: path__.unwrap_or_default(),
                    last_modified_timestamp: last_modified_timestamp__.unwrap_or_default(),
                    compressed_size_in_bytes: compressed_size_in_bytes__.unwrap_or_default(),
                    uncompressed_size_in_bytes: uncompressed_size_in_bytes__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.sds.SdsResultPayload.S3File", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for sds_result_payload::ScanLocation {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.size_in_bytes != 0 {
            len += 1;
        }
        if self.last_modified_timestamp != 0 {
            len += 1;
        }
        if !self.table.is_empty() {
            len += 1;
        }
        if self.scan_location_type.is_some() {
            len += 1;
        }
        if self.scan_location.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.sds.SdsResultPayload.ScanLocation", len)?;
        if self.size_in_bytes != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("sizeInBytes", ToString::to_string(&self.size_in_bytes).as_str())?;
        }
        if self.last_modified_timestamp != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("lastModifiedTimestamp", ToString::to_string(&self.last_modified_timestamp).as_str())?;
        }
        if !self.table.is_empty() {
            struct_ser.serialize_field("table", &self.table)?;
        }
        if let Some(v) = self.scan_location_type.as_ref() {
            match v {
                sds_result_payload::scan_location::ScanLocationType::Path(v) => {
                    struct_ser.serialize_field("path", v)?;
                }
                sds_result_payload::scan_location::ScanLocationType::Database(v) => {
                    struct_ser.serialize_field("database", v)?;
                }
            }
        }
        if let Some(v) = self.scan_location.as_ref() {
            match v {
                sds_result_payload::scan_location::ScanLocation::S3File(v) => {
                    struct_ser.serialize_field("s3File", v)?;
                }
                sds_result_payload::scan_location::ScanLocation::RdsTable(v) => {
                    struct_ser.serialize_field("rdsTable", v)?;
                }
                sds_result_payload::scan_location::ScanLocation::SnowflakeTable(v) => {
                    struct_ser.serialize_field("snowflakeTable", v)?;
                }
                sds_result_payload::scan_location::ScanLocation::PostgresTable(v) => {
                    struct_ser.serialize_field("postgresTable", v)?;
                }
            }
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for sds_result_payload::ScanLocation {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "size_in_bytes",
            "sizeInBytes",
            "last_modified_timestamp",
            "lastModifiedTimestamp",
            "table",
            "path",
            "database",
            "s3File",
            "rdsTable",
            "snowflakeTable",
            "postgresTable",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            SizeInBytes,
            LastModifiedTimestamp,
            Table,
            Path,
            Database,
            S3File,
            RdsTable,
            SnowflakeTable,
            PostgresTable,
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
                            "sizeInBytes" | "size_in_bytes" => Ok(GeneratedField::SizeInBytes),
                            "lastModifiedTimestamp" | "last_modified_timestamp" => Ok(GeneratedField::LastModifiedTimestamp),
                            "table" => Ok(GeneratedField::Table),
                            "path" => Ok(GeneratedField::Path),
                            "database" => Ok(GeneratedField::Database),
                            "s3File" => Ok(GeneratedField::S3File),
                            "rdsTable" => Ok(GeneratedField::RdsTable),
                            "snowflakeTable" => Ok(GeneratedField::SnowflakeTable),
                            "postgresTable" => Ok(GeneratedField::PostgresTable),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = sds_result_payload::ScanLocation;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.sds.SdsResultPayload.ScanLocation")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<sds_result_payload::ScanLocation, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut size_in_bytes__ = None;
                let mut last_modified_timestamp__ = None;
                let mut table__ = None;
                let mut scan_location_type__ = None;
                let mut scan_location__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::SizeInBytes => {
                            if size_in_bytes__.is_some() {
                                return Err(serde::de::Error::duplicate_field("sizeInBytes"));
                            }
                            size_in_bytes__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::LastModifiedTimestamp => {
                            if last_modified_timestamp__.is_some() {
                                return Err(serde::de::Error::duplicate_field("lastModifiedTimestamp"));
                            }
                            last_modified_timestamp__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Table => {
                            if table__.is_some() {
                                return Err(serde::de::Error::duplicate_field("table"));
                            }
                            table__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Path => {
                            if scan_location_type__.is_some() {
                                return Err(serde::de::Error::duplicate_field("path"));
                            }
                            scan_location_type__ = map_.next_value::<::std::option::Option<_>>()?.map(sds_result_payload::scan_location::ScanLocationType::Path);
                        }
                        GeneratedField::Database => {
                            if scan_location_type__.is_some() {
                                return Err(serde::de::Error::duplicate_field("database"));
                            }
                            scan_location_type__ = map_.next_value::<::std::option::Option<_>>()?.map(sds_result_payload::scan_location::ScanLocationType::Database);
                        }
                        GeneratedField::S3File => {
                            if scan_location__.is_some() {
                                return Err(serde::de::Error::duplicate_field("s3File"));
                            }
                            scan_location__ = map_.next_value::<::std::option::Option<_>>()?.map(sds_result_payload::scan_location::ScanLocation::S3File)
;
                        }
                        GeneratedField::RdsTable => {
                            if scan_location__.is_some() {
                                return Err(serde::de::Error::duplicate_field("rdsTable"));
                            }
                            scan_location__ = map_.next_value::<::std::option::Option<_>>()?.map(sds_result_payload::scan_location::ScanLocation::RdsTable)
;
                        }
                        GeneratedField::SnowflakeTable => {
                            if scan_location__.is_some() {
                                return Err(serde::de::Error::duplicate_field("snowflakeTable"));
                            }
                            scan_location__ = map_.next_value::<::std::option::Option<_>>()?.map(sds_result_payload::scan_location::ScanLocation::SnowflakeTable)
;
                        }
                        GeneratedField::PostgresTable => {
                            if scan_location__.is_some() {
                                return Err(serde::de::Error::duplicate_field("postgresTable"));
                            }
                            scan_location__ = map_.next_value::<::std::option::Option<_>>()?.map(sds_result_payload::scan_location::ScanLocation::PostgresTable)
;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(sds_result_payload::ScanLocation {
                    size_in_bytes: size_in_bytes__.unwrap_or_default(),
                    last_modified_timestamp: last_modified_timestamp__.unwrap_or_default(),
                    table: table__.unwrap_or_default(),
                    scan_location_type: scan_location_type__,
                    scan_location: scan_location__,
                })
            }
        }
        deserializer.deserialize_struct("datadog.sds.SdsResultPayload.ScanLocation", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for sds_result_payload::ScanMatch {
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
        if self.start_index != 0 {
            len += 1;
        }
        if self.end_index != 0 {
            len += 1;
        }
        if !self.sample.is_empty() {
            len += 1;
        }
        if self.path.is_some() {
            len += 1;
        }
        if self.line.is_some() {
            len += 1;
        }
        if self.column.is_some() {
            len += 1;
        }
        if self.start_line.is_some() {
            len += 1;
        }
        if self.end_line.is_some() {
            len += 1;
        }
        if self.row.is_some() {
            len += 1;
        }
        if self.start_index_in_line.is_some() {
            len += 1;
        }
        if self.end_index_in_line.is_some() {
            len += 1;
        }
        if self.match_status.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.sds.SdsResultPayload.ScanMatch", len)?;
        if !self.rule_id.is_empty() {
            struct_ser.serialize_field("ruleId", &self.rule_id)?;
        }
        if self.start_index != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("startIndex", ToString::to_string(&self.start_index).as_str())?;
        }
        if self.end_index != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("endIndex", ToString::to_string(&self.end_index).as_str())?;
        }
        if !self.sample.is_empty() {
            struct_ser.serialize_field("sample", &self.sample)?;
        }
        if let Some(v) = self.path.as_ref() {
            struct_ser.serialize_field("path", v)?;
        }
        if let Some(v) = self.line.as_ref() {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("line", ToString::to_string(&v).as_str())?;
        }
        if let Some(v) = self.column.as_ref() {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("column", ToString::to_string(&v).as_str())?;
        }
        if let Some(v) = self.start_line.as_ref() {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("startLine", ToString::to_string(&v).as_str())?;
        }
        if let Some(v) = self.end_line.as_ref() {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("endLine", ToString::to_string(&v).as_str())?;
        }
        if let Some(v) = self.row.as_ref() {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("row", ToString::to_string(&v).as_str())?;
        }
        if let Some(v) = self.start_index_in_line.as_ref() {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("startIndexInLine", ToString::to_string(&v).as_str())?;
        }
        if let Some(v) = self.end_index_in_line.as_ref() {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("endIndexInLine", ToString::to_string(&v).as_str())?;
        }
        if let Some(v) = self.match_status.as_ref() {
            struct_ser.serialize_field("matchStatus", v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for sds_result_payload::ScanMatch {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "rule_id",
            "ruleId",
            "start_index",
            "startIndex",
            "end_index",
            "endIndex",
            "sample",
            "path",
            "line",
            "column",
            "start_line",
            "startLine",
            "end_line",
            "endLine",
            "row",
            "start_index_in_line",
            "startIndexInLine",
            "end_index_in_line",
            "endIndexInLine",
            "match_status",
            "matchStatus",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            RuleId,
            StartIndex,
            EndIndex,
            Sample,
            Path,
            Line,
            Column,
            StartLine,
            EndLine,
            Row,
            StartIndexInLine,
            EndIndexInLine,
            MatchStatus,
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
                            "startIndex" | "start_index" => Ok(GeneratedField::StartIndex),
                            "endIndex" | "end_index" => Ok(GeneratedField::EndIndex),
                            "sample" => Ok(GeneratedField::Sample),
                            "path" => Ok(GeneratedField::Path),
                            "line" => Ok(GeneratedField::Line),
                            "column" => Ok(GeneratedField::Column),
                            "startLine" | "start_line" => Ok(GeneratedField::StartLine),
                            "endLine" | "end_line" => Ok(GeneratedField::EndLine),
                            "row" => Ok(GeneratedField::Row),
                            "startIndexInLine" | "start_index_in_line" => Ok(GeneratedField::StartIndexInLine),
                            "endIndexInLine" | "end_index_in_line" => Ok(GeneratedField::EndIndexInLine),
                            "matchStatus" | "match_status" => Ok(GeneratedField::MatchStatus),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = sds_result_payload::ScanMatch;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.sds.SdsResultPayload.ScanMatch")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<sds_result_payload::ScanMatch, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut rule_id__ = None;
                let mut start_index__ = None;
                let mut end_index__ = None;
                let mut sample__ = None;
                let mut path__ = None;
                let mut line__ = None;
                let mut column__ = None;
                let mut start_line__ = None;
                let mut end_line__ = None;
                let mut row__ = None;
                let mut start_index_in_line__ = None;
                let mut end_index_in_line__ = None;
                let mut match_status__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::RuleId => {
                            if rule_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("ruleId"));
                            }
                            rule_id__ = Some(map_.next_value()?);
                        }
                        GeneratedField::StartIndex => {
                            if start_index__.is_some() {
                                return Err(serde::de::Error::duplicate_field("startIndex"));
                            }
                            start_index__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::EndIndex => {
                            if end_index__.is_some() {
                                return Err(serde::de::Error::duplicate_field("endIndex"));
                            }
                            end_index__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Sample => {
                            if sample__.is_some() {
                                return Err(serde::de::Error::duplicate_field("sample"));
                            }
                            sample__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Path => {
                            if path__.is_some() {
                                return Err(serde::de::Error::duplicate_field("path"));
                            }
                            path__ = map_.next_value()?;
                        }
                        GeneratedField::Line => {
                            if line__.is_some() {
                                return Err(serde::de::Error::duplicate_field("line"));
                            }
                            line__ = 
                                map_.next_value::<::std::option::Option<::pbjson::private::NumberDeserialize<_>>>()?.map(|x| x.0)
                            ;
                        }
                        GeneratedField::Column => {
                            if column__.is_some() {
                                return Err(serde::de::Error::duplicate_field("column"));
                            }
                            column__ = 
                                map_.next_value::<::std::option::Option<::pbjson::private::NumberDeserialize<_>>>()?.map(|x| x.0)
                            ;
                        }
                        GeneratedField::StartLine => {
                            if start_line__.is_some() {
                                return Err(serde::de::Error::duplicate_field("startLine"));
                            }
                            start_line__ = 
                                map_.next_value::<::std::option::Option<::pbjson::private::NumberDeserialize<_>>>()?.map(|x| x.0)
                            ;
                        }
                        GeneratedField::EndLine => {
                            if end_line__.is_some() {
                                return Err(serde::de::Error::duplicate_field("endLine"));
                            }
                            end_line__ = 
                                map_.next_value::<::std::option::Option<::pbjson::private::NumberDeserialize<_>>>()?.map(|x| x.0)
                            ;
                        }
                        GeneratedField::Row => {
                            if row__.is_some() {
                                return Err(serde::de::Error::duplicate_field("row"));
                            }
                            row__ = 
                                map_.next_value::<::std::option::Option<::pbjson::private::NumberDeserialize<_>>>()?.map(|x| x.0)
                            ;
                        }
                        GeneratedField::StartIndexInLine => {
                            if start_index_in_line__.is_some() {
                                return Err(serde::de::Error::duplicate_field("startIndexInLine"));
                            }
                            start_index_in_line__ = 
                                map_.next_value::<::std::option::Option<::pbjson::private::NumberDeserialize<_>>>()?.map(|x| x.0)
                            ;
                        }
                        GeneratedField::EndIndexInLine => {
                            if end_index_in_line__.is_some() {
                                return Err(serde::de::Error::duplicate_field("endIndexInLine"));
                            }
                            end_index_in_line__ = 
                                map_.next_value::<::std::option::Option<::pbjson::private::NumberDeserialize<_>>>()?.map(|x| x.0)
                            ;
                        }
                        GeneratedField::MatchStatus => {
                            if match_status__.is_some() {
                                return Err(serde::de::Error::duplicate_field("matchStatus"));
                            }
                            match_status__ = map_.next_value()?;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(sds_result_payload::ScanMatch {
                    rule_id: rule_id__.unwrap_or_default(),
                    start_index: start_index__.unwrap_or_default(),
                    end_index: end_index__.unwrap_or_default(),
                    sample: sample__.unwrap_or_default(),
                    path: path__,
                    line: line__,
                    column: column__,
                    start_line: start_line__,
                    end_line: end_line__,
                    row: row__,
                    start_index_in_line: start_index_in_line__,
                    end_index_in_line: end_index_in_line__,
                    match_status: match_status__,
                })
            }
        }
        deserializer.deserialize_struct("datadog.sds.SdsResultPayload.ScanMatch", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for sds_result_payload::ScanMetadata {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.scan_task_metadata.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.sds.SdsResultPayload.ScanMetadata", len)?;
        if let Some(v) = self.scan_task_metadata.as_ref() {
            struct_ser.serialize_field("scanTaskMetadata", v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for sds_result_payload::ScanMetadata {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "scan_task_metadata",
            "scanTaskMetadata",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            ScanTaskMetadata,
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
                            "scanTaskMetadata" | "scan_task_metadata" => Ok(GeneratedField::ScanTaskMetadata),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = sds_result_payload::ScanMetadata;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.sds.SdsResultPayload.ScanMetadata")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<sds_result_payload::ScanMetadata, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut scan_task_metadata__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::ScanTaskMetadata => {
                            if scan_task_metadata__.is_some() {
                                return Err(serde::de::Error::duplicate_field("scanTaskMetadata"));
                            }
                            scan_task_metadata__ = map_.next_value()?;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(sds_result_payload::ScanMetadata {
                    scan_task_metadata: scan_task_metadata__,
                })
            }
        }
        deserializer.deserialize_struct("datadog.sds.SdsResultPayload.ScanMetadata", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for sds_result_payload::scan_metadata::ScanTaskMetadata {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.task_id.is_empty() {
            len += 1;
        }
        if !self.sub_task_id.is_empty() {
            len += 1;
        }
        if self.started_at.is_some() {
            len += 1;
        }
        if self.ended_at.is_some() {
            len += 1;
        }
        if self.status != 0 {
            len += 1;
        }
        if self.failure_reason.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.sds.SdsResultPayload.ScanMetadata.ScanTaskMetadata", len)?;
        if !self.task_id.is_empty() {
            struct_ser.serialize_field("taskId", &self.task_id)?;
        }
        if !self.sub_task_id.is_empty() {
            struct_ser.serialize_field("subTaskId", &self.sub_task_id)?;
        }
        if let Some(v) = self.started_at.as_ref() {
            struct_ser.serialize_field("startedAt", v)?;
        }
        if let Some(v) = self.ended_at.as_ref() {
            struct_ser.serialize_field("endedAt", v)?;
        }
        if self.status != 0 {
            let v = sds_result_payload::scan_metadata::scan_task_metadata::Status::try_from(self.status)
                .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", self.status)))?;
            struct_ser.serialize_field("status", &v)?;
        }
        if let Some(v) = self.failure_reason.as_ref() {
            struct_ser.serialize_field("failureReason", v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for sds_result_payload::scan_metadata::ScanTaskMetadata {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "task_id",
            "taskId",
            "sub_task_id",
            "subTaskId",
            "started_at",
            "startedAt",
            "ended_at",
            "endedAt",
            "status",
            "failure_reason",
            "failureReason",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            TaskId,
            SubTaskId,
            StartedAt,
            EndedAt,
            Status,
            FailureReason,
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
                            "taskId" | "task_id" => Ok(GeneratedField::TaskId),
                            "subTaskId" | "sub_task_id" => Ok(GeneratedField::SubTaskId),
                            "startedAt" | "started_at" => Ok(GeneratedField::StartedAt),
                            "endedAt" | "ended_at" => Ok(GeneratedField::EndedAt),
                            "status" => Ok(GeneratedField::Status),
                            "failureReason" | "failure_reason" => Ok(GeneratedField::FailureReason),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = sds_result_payload::scan_metadata::ScanTaskMetadata;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.sds.SdsResultPayload.ScanMetadata.ScanTaskMetadata")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<sds_result_payload::scan_metadata::ScanTaskMetadata, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut task_id__ = None;
                let mut sub_task_id__ = None;
                let mut started_at__ = None;
                let mut ended_at__ = None;
                let mut status__ = None;
                let mut failure_reason__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::TaskId => {
                            if task_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("taskId"));
                            }
                            task_id__ = Some(map_.next_value()?);
                        }
                        GeneratedField::SubTaskId => {
                            if sub_task_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("subTaskId"));
                            }
                            sub_task_id__ = Some(map_.next_value()?);
                        }
                        GeneratedField::StartedAt => {
                            if started_at__.is_some() {
                                return Err(serde::de::Error::duplicate_field("startedAt"));
                            }
                            started_at__ = map_.next_value()?;
                        }
                        GeneratedField::EndedAt => {
                            if ended_at__.is_some() {
                                return Err(serde::de::Error::duplicate_field("endedAt"));
                            }
                            ended_at__ = map_.next_value()?;
                        }
                        GeneratedField::Status => {
                            if status__.is_some() {
                                return Err(serde::de::Error::duplicate_field("status"));
                            }
                            status__ = Some(map_.next_value::<sds_result_payload::scan_metadata::scan_task_metadata::Status>()? as i32);
                        }
                        GeneratedField::FailureReason => {
                            if failure_reason__.is_some() {
                                return Err(serde::de::Error::duplicate_field("failureReason"));
                            }
                            failure_reason__ = map_.next_value()?;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(sds_result_payload::scan_metadata::ScanTaskMetadata {
                    task_id: task_id__.unwrap_or_default(),
                    sub_task_id: sub_task_id__.unwrap_or_default(),
                    started_at: started_at__,
                    ended_at: ended_at__,
                    status: status__.unwrap_or_default(),
                    failure_reason: failure_reason__,
                })
            }
        }
        deserializer.deserialize_struct("datadog.sds.SdsResultPayload.ScanMetadata.ScanTaskMetadata", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for sds_result_payload::scan_metadata::scan_task_metadata::Status {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let variant = match self {
            Self::Unknown => "STATUS_UNKNOWN",
            Self::Success => "SUCCESS",
            Self::Error => "ERROR",
        };
        serializer.serialize_str(variant)
    }
}
impl<'de> serde::Deserialize<'de> for sds_result_payload::scan_metadata::scan_task_metadata::Status {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "STATUS_UNKNOWN",
            "SUCCESS",
            "ERROR",
        ];

        struct GeneratedVisitor;

        impl serde::de::Visitor<'_> for GeneratedVisitor {
            type Value = sds_result_payload::scan_metadata::scan_task_metadata::Status;

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
                    "STATUS_UNKNOWN" => Ok(sds_result_payload::scan_metadata::scan_task_metadata::Status::Unknown),
                    "SUCCESS" => Ok(sds_result_payload::scan_metadata::scan_task_metadata::Status::Success),
                    "ERROR" => Ok(sds_result_payload::scan_metadata::scan_task_metadata::Status::Error),
                    _ => Err(serde::de::Error::unknown_variant(value, FIELDS)),
                }
            }
        }
        deserializer.deserialize_any(GeneratedVisitor)
    }
}
impl serde::Serialize for sds_result_payload::ScanResult {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.matches.is_empty() {
            len += 1;
        }
        if self.location.is_some() {
            len += 1;
        }
        if self.duration != 0 {
            len += 1;
        }
        if !self.table_matches.is_empty() {
            len += 1;
        }
        if self.scan_metadata.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.sds.SdsResultPayload.ScanResult", len)?;
        if !self.matches.is_empty() {
            struct_ser.serialize_field("matches", &self.matches)?;
        }
        if let Some(v) = self.location.as_ref() {
            struct_ser.serialize_field("location", v)?;
        }
        if self.duration != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("duration", ToString::to_string(&self.duration).as_str())?;
        }
        if !self.table_matches.is_empty() {
            struct_ser.serialize_field("tableMatches", &self.table_matches)?;
        }
        if let Some(v) = self.scan_metadata.as_ref() {
            struct_ser.serialize_field("scanMetadata", v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for sds_result_payload::ScanResult {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "matches",
            "location",
            "duration",
            "table_matches",
            "tableMatches",
            "scan_metadata",
            "scanMetadata",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Matches,
            Location,
            Duration,
            TableMatches,
            ScanMetadata,
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
                            "matches" => Ok(GeneratedField::Matches),
                            "location" => Ok(GeneratedField::Location),
                            "duration" => Ok(GeneratedField::Duration),
                            "tableMatches" | "table_matches" => Ok(GeneratedField::TableMatches),
                            "scanMetadata" | "scan_metadata" => Ok(GeneratedField::ScanMetadata),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = sds_result_payload::ScanResult;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.sds.SdsResultPayload.ScanResult")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<sds_result_payload::ScanResult, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut matches__ = None;
                let mut location__ = None;
                let mut duration__ = None;
                let mut table_matches__ = None;
                let mut scan_metadata__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Matches => {
                            if matches__.is_some() {
                                return Err(serde::de::Error::duplicate_field("matches"));
                            }
                            matches__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Location => {
                            if location__.is_some() {
                                return Err(serde::de::Error::duplicate_field("location"));
                            }
                            location__ = map_.next_value()?;
                        }
                        GeneratedField::Duration => {
                            if duration__.is_some() {
                                return Err(serde::de::Error::duplicate_field("duration"));
                            }
                            duration__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::TableMatches => {
                            if table_matches__.is_some() {
                                return Err(serde::de::Error::duplicate_field("tableMatches"));
                            }
                            table_matches__ = Some(map_.next_value()?);
                        }
                        GeneratedField::ScanMetadata => {
                            if scan_metadata__.is_some() {
                                return Err(serde::de::Error::duplicate_field("scanMetadata"));
                            }
                            scan_metadata__ = map_.next_value()?;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(sds_result_payload::ScanResult {
                    matches: matches__.unwrap_or_default(),
                    location: location__,
                    duration: duration__.unwrap_or_default(),
                    table_matches: table_matches__.unwrap_or_default(),
                    scan_metadata: scan_metadata__,
                })
            }
        }
        deserializer.deserialize_struct("datadog.sds.SdsResultPayload.ScanResult", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for sds_result_payload::ScanSource {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let variant = match self {
            Self::Unknown => "UNKNOWN",
            Self::Agentless => "AGENTLESS",
        };
        serializer.serialize_str(variant)
    }
}
impl<'de> serde::Deserialize<'de> for sds_result_payload::ScanSource {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "UNKNOWN",
            "AGENTLESS",
        ];

        struct GeneratedVisitor;

        impl serde::de::Visitor<'_> for GeneratedVisitor {
            type Value = sds_result_payload::ScanSource;

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
                    "UNKNOWN" => Ok(sds_result_payload::ScanSource::Unknown),
                    "AGENTLESS" => Ok(sds_result_payload::ScanSource::Agentless),
                    _ => Err(serde::de::Error::unknown_variant(value, FIELDS)),
                }
            }
        }
        deserializer.deserialize_any(GeneratedVisitor)
    }
}
impl serde::Serialize for sds_result_payload::SnowflakeTable {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.account_name.is_empty() {
            len += 1;
        }
        if !self.database_name.is_empty() {
            len += 1;
        }
        if !self.schema_name.is_empty() {
            len += 1;
        }
        if !self.table_name.is_empty() {
            len += 1;
        }
        if self.last_modified_timestamp != 0 {
            len += 1;
        }
        if self.table_row_count != 0 {
            len += 1;
        }
        if self.scanned_row_count != 0 {
            len += 1;
        }
        if !self.scanned_columns.is_empty() {
            len += 1;
        }
        if !self.entity_id.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.sds.SdsResultPayload.SnowflakeTable", len)?;
        if !self.account_name.is_empty() {
            struct_ser.serialize_field("accountName", &self.account_name)?;
        }
        if !self.database_name.is_empty() {
            struct_ser.serialize_field("databaseName", &self.database_name)?;
        }
        if !self.schema_name.is_empty() {
            struct_ser.serialize_field("schemaName", &self.schema_name)?;
        }
        if !self.table_name.is_empty() {
            struct_ser.serialize_field("tableName", &self.table_name)?;
        }
        if self.last_modified_timestamp != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("lastModifiedTimestamp", ToString::to_string(&self.last_modified_timestamp).as_str())?;
        }
        if self.table_row_count != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("tableRowCount", ToString::to_string(&self.table_row_count).as_str())?;
        }
        if self.scanned_row_count != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("scannedRowCount", ToString::to_string(&self.scanned_row_count).as_str())?;
        }
        if !self.scanned_columns.is_empty() {
            struct_ser.serialize_field("scannedColumns", &self.scanned_columns)?;
        }
        if !self.entity_id.is_empty() {
            struct_ser.serialize_field("entityId", &self.entity_id)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for sds_result_payload::SnowflakeTable {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "account_name",
            "accountName",
            "database_name",
            "databaseName",
            "schema_name",
            "schemaName",
            "table_name",
            "tableName",
            "last_modified_timestamp",
            "lastModifiedTimestamp",
            "table_row_count",
            "tableRowCount",
            "scanned_row_count",
            "scannedRowCount",
            "scanned_columns",
            "scannedColumns",
            "entity_id",
            "entityId",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            AccountName,
            DatabaseName,
            SchemaName,
            TableName,
            LastModifiedTimestamp,
            TableRowCount,
            ScannedRowCount,
            ScannedColumns,
            EntityId,
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
                            "accountName" | "account_name" => Ok(GeneratedField::AccountName),
                            "databaseName" | "database_name" => Ok(GeneratedField::DatabaseName),
                            "schemaName" | "schema_name" => Ok(GeneratedField::SchemaName),
                            "tableName" | "table_name" => Ok(GeneratedField::TableName),
                            "lastModifiedTimestamp" | "last_modified_timestamp" => Ok(GeneratedField::LastModifiedTimestamp),
                            "tableRowCount" | "table_row_count" => Ok(GeneratedField::TableRowCount),
                            "scannedRowCount" | "scanned_row_count" => Ok(GeneratedField::ScannedRowCount),
                            "scannedColumns" | "scanned_columns" => Ok(GeneratedField::ScannedColumns),
                            "entityId" | "entity_id" => Ok(GeneratedField::EntityId),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = sds_result_payload::SnowflakeTable;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.sds.SdsResultPayload.SnowflakeTable")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<sds_result_payload::SnowflakeTable, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut account_name__ = None;
                let mut database_name__ = None;
                let mut schema_name__ = None;
                let mut table_name__ = None;
                let mut last_modified_timestamp__ = None;
                let mut table_row_count__ = None;
                let mut scanned_row_count__ = None;
                let mut scanned_columns__ = None;
                let mut entity_id__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::AccountName => {
                            if account_name__.is_some() {
                                return Err(serde::de::Error::duplicate_field("accountName"));
                            }
                            account_name__ = Some(map_.next_value()?);
                        }
                        GeneratedField::DatabaseName => {
                            if database_name__.is_some() {
                                return Err(serde::de::Error::duplicate_field("databaseName"));
                            }
                            database_name__ = Some(map_.next_value()?);
                        }
                        GeneratedField::SchemaName => {
                            if schema_name__.is_some() {
                                return Err(serde::de::Error::duplicate_field("schemaName"));
                            }
                            schema_name__ = Some(map_.next_value()?);
                        }
                        GeneratedField::TableName => {
                            if table_name__.is_some() {
                                return Err(serde::de::Error::duplicate_field("tableName"));
                            }
                            table_name__ = Some(map_.next_value()?);
                        }
                        GeneratedField::LastModifiedTimestamp => {
                            if last_modified_timestamp__.is_some() {
                                return Err(serde::de::Error::duplicate_field("lastModifiedTimestamp"));
                            }
                            last_modified_timestamp__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::TableRowCount => {
                            if table_row_count__.is_some() {
                                return Err(serde::de::Error::duplicate_field("tableRowCount"));
                            }
                            table_row_count__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::ScannedRowCount => {
                            if scanned_row_count__.is_some() {
                                return Err(serde::de::Error::duplicate_field("scannedRowCount"));
                            }
                            scanned_row_count__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::ScannedColumns => {
                            if scanned_columns__.is_some() {
                                return Err(serde::de::Error::duplicate_field("scannedColumns"));
                            }
                            scanned_columns__ = Some(map_.next_value()?);
                        }
                        GeneratedField::EntityId => {
                            if entity_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("entityId"));
                            }
                            entity_id__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(sds_result_payload::SnowflakeTable {
                    account_name: account_name__.unwrap_or_default(),
                    database_name: database_name__.unwrap_or_default(),
                    schema_name: schema_name__.unwrap_or_default(),
                    table_name: table_name__.unwrap_or_default(),
                    last_modified_timestamp: last_modified_timestamp__.unwrap_or_default(),
                    table_row_count: table_row_count__.unwrap_or_default(),
                    scanned_row_count: scanned_row_count__.unwrap_or_default(),
                    scanned_columns: scanned_columns__.unwrap_or_default(),
                    entity_id: entity_id__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.sds.SdsResultPayload.SnowflakeTable", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for sds_result_payload::snowflake_table::ScannedColumn {
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
        if !self.data_type.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.sds.SdsResultPayload.SnowflakeTable.ScannedColumn", len)?;
        if !self.name.is_empty() {
            struct_ser.serialize_field("name", &self.name)?;
        }
        if !self.data_type.is_empty() {
            struct_ser.serialize_field("dataType", &self.data_type)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for sds_result_payload::snowflake_table::ScannedColumn {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "name",
            "data_type",
            "dataType",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Name,
            DataType,
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
                            "dataType" | "data_type" => Ok(GeneratedField::DataType),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = sds_result_payload::snowflake_table::ScannedColumn;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.sds.SdsResultPayload.SnowflakeTable.ScannedColumn")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<sds_result_payload::snowflake_table::ScannedColumn, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut name__ = None;
                let mut data_type__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Name => {
                            if name__.is_some() {
                                return Err(serde::de::Error::duplicate_field("name"));
                            }
                            name__ = Some(map_.next_value()?);
                        }
                        GeneratedField::DataType => {
                            if data_type__.is_some() {
                                return Err(serde::de::Error::duplicate_field("dataType"));
                            }
                            data_type__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(sds_result_payload::snowflake_table::ScannedColumn {
                    name: name__.unwrap_or_default(),
                    data_type: data_type__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.sds.SdsResultPayload.SnowflakeTable.ScannedColumn", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for sds_result_payload::TableMatch {
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
        if !self.column_name.is_empty() {
            len += 1;
        }
        if self.count_matched_rows != 0 {
            len += 1;
        }
        if self.count_total_rows != 0 {
            len += 1;
        }
        if self.count_matches != 0 {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.sds.SdsResultPayload.TableMatch", len)?;
        if !self.rule_id.is_empty() {
            struct_ser.serialize_field("ruleId", &self.rule_id)?;
        }
        if !self.column_name.is_empty() {
            struct_ser.serialize_field("columnName", &self.column_name)?;
        }
        if self.count_matched_rows != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("countMatchedRows", ToString::to_string(&self.count_matched_rows).as_str())?;
        }
        if self.count_total_rows != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("countTotalRows", ToString::to_string(&self.count_total_rows).as_str())?;
        }
        if self.count_matches != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("countMatches", ToString::to_string(&self.count_matches).as_str())?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for sds_result_payload::TableMatch {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "rule_id",
            "ruleId",
            "column_name",
            "columnName",
            "count_matched_rows",
            "countMatchedRows",
            "count_total_rows",
            "countTotalRows",
            "count_matches",
            "countMatches",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            RuleId,
            ColumnName,
            CountMatchedRows,
            CountTotalRows,
            CountMatches,
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
                            "columnName" | "column_name" => Ok(GeneratedField::ColumnName),
                            "countMatchedRows" | "count_matched_rows" => Ok(GeneratedField::CountMatchedRows),
                            "countTotalRows" | "count_total_rows" => Ok(GeneratedField::CountTotalRows),
                            "countMatches" | "count_matches" => Ok(GeneratedField::CountMatches),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = sds_result_payload::TableMatch;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.sds.SdsResultPayload.TableMatch")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<sds_result_payload::TableMatch, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut rule_id__ = None;
                let mut column_name__ = None;
                let mut count_matched_rows__ = None;
                let mut count_total_rows__ = None;
                let mut count_matches__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::RuleId => {
                            if rule_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("ruleId"));
                            }
                            rule_id__ = Some(map_.next_value()?);
                        }
                        GeneratedField::ColumnName => {
                            if column_name__.is_some() {
                                return Err(serde::de::Error::duplicate_field("columnName"));
                            }
                            column_name__ = Some(map_.next_value()?);
                        }
                        GeneratedField::CountMatchedRows => {
                            if count_matched_rows__.is_some() {
                                return Err(serde::de::Error::duplicate_field("countMatchedRows"));
                            }
                            count_matched_rows__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::CountTotalRows => {
                            if count_total_rows__.is_some() {
                                return Err(serde::de::Error::duplicate_field("countTotalRows"));
                            }
                            count_total_rows__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::CountMatches => {
                            if count_matches__.is_some() {
                                return Err(serde::de::Error::duplicate_field("countMatches"));
                            }
                            count_matches__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(sds_result_payload::TableMatch {
                    rule_id: rule_id__.unwrap_or_default(),
                    column_name: column_name__.unwrap_or_default(),
                    count_matched_rows: count_matched_rows__.unwrap_or_default(),
                    count_total_rows: count_total_rows__.unwrap_or_default(),
                    count_matches: count_matches__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.sds.SdsResultPayload.TableMatch", FIELDS, GeneratedVisitor)
    }
}
