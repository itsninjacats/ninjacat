impl serde::Serialize for Client {
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
        if !self.id.is_empty() {
            len += 1;
        }
        if !self.products.is_empty() {
            len += 1;
        }
        if self.is_tracer {
            len += 1;
        }
        if self.client_tracer.is_some() {
            len += 1;
        }
        if self.is_agent {
            len += 1;
        }
        if self.client_agent.is_some() {
            len += 1;
        }
        if self.last_seen != 0 {
            len += 1;
        }
        if !self.capabilities.is_empty() {
            len += 1;
        }
        if self.is_updater {
            len += 1;
        }
        if self.client_updater.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.config.Client", len)?;
        if let Some(v) = self.state.as_ref() {
            struct_ser.serialize_field("state", v)?;
        }
        if !self.id.is_empty() {
            struct_ser.serialize_field("id", &self.id)?;
        }
        if !self.products.is_empty() {
            struct_ser.serialize_field("products", &self.products)?;
        }
        if self.is_tracer {
            struct_ser.serialize_field("isTracer", &self.is_tracer)?;
        }
        if let Some(v) = self.client_tracer.as_ref() {
            struct_ser.serialize_field("clientTracer", v)?;
        }
        if self.is_agent {
            struct_ser.serialize_field("isAgent", &self.is_agent)?;
        }
        if let Some(v) = self.client_agent.as_ref() {
            struct_ser.serialize_field("clientAgent", v)?;
        }
        if self.last_seen != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("lastSeen", ToString::to_string(&self.last_seen).as_str())?;
        }
        if !self.capabilities.is_empty() {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("capabilities", pbjson::private::base64::encode(&self.capabilities).as_str())?;
        }
        if self.is_updater {
            struct_ser.serialize_field("isUpdater", &self.is_updater)?;
        }
        if let Some(v) = self.client_updater.as_ref() {
            struct_ser.serialize_field("clientUpdater", v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for Client {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "state",
            "id",
            "products",
            "is_tracer",
            "isTracer",
            "client_tracer",
            "clientTracer",
            "is_agent",
            "isAgent",
            "client_agent",
            "clientAgent",
            "last_seen",
            "lastSeen",
            "capabilities",
            "is_updater",
            "isUpdater",
            "client_updater",
            "clientUpdater",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            State,
            Id,
            Products,
            IsTracer,
            ClientTracer,
            IsAgent,
            ClientAgent,
            LastSeen,
            Capabilities,
            IsUpdater,
            ClientUpdater,
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
                            "id" => Ok(GeneratedField::Id),
                            "products" => Ok(GeneratedField::Products),
                            "isTracer" | "is_tracer" => Ok(GeneratedField::IsTracer),
                            "clientTracer" | "client_tracer" => Ok(GeneratedField::ClientTracer),
                            "isAgent" | "is_agent" => Ok(GeneratedField::IsAgent),
                            "clientAgent" | "client_agent" => Ok(GeneratedField::ClientAgent),
                            "lastSeen" | "last_seen" => Ok(GeneratedField::LastSeen),
                            "capabilities" => Ok(GeneratedField::Capabilities),
                            "isUpdater" | "is_updater" => Ok(GeneratedField::IsUpdater),
                            "clientUpdater" | "client_updater" => Ok(GeneratedField::ClientUpdater),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = Client;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.config.Client")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<Client, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut state__ = None;
                let mut id__ = None;
                let mut products__ = None;
                let mut is_tracer__ = None;
                let mut client_tracer__ = None;
                let mut is_agent__ = None;
                let mut client_agent__ = None;
                let mut last_seen__ = None;
                let mut capabilities__ = None;
                let mut is_updater__ = None;
                let mut client_updater__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::State => {
                            if state__.is_some() {
                                return Err(serde::de::Error::duplicate_field("state"));
                            }
                            state__ = map_.next_value()?;
                        }
                        GeneratedField::Id => {
                            if id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("id"));
                            }
                            id__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Products => {
                            if products__.is_some() {
                                return Err(serde::de::Error::duplicate_field("products"));
                            }
                            products__ = Some(map_.next_value()?);
                        }
                        GeneratedField::IsTracer => {
                            if is_tracer__.is_some() {
                                return Err(serde::de::Error::duplicate_field("isTracer"));
                            }
                            is_tracer__ = Some(map_.next_value()?);
                        }
                        GeneratedField::ClientTracer => {
                            if client_tracer__.is_some() {
                                return Err(serde::de::Error::duplicate_field("clientTracer"));
                            }
                            client_tracer__ = map_.next_value()?;
                        }
                        GeneratedField::IsAgent => {
                            if is_agent__.is_some() {
                                return Err(serde::de::Error::duplicate_field("isAgent"));
                            }
                            is_agent__ = Some(map_.next_value()?);
                        }
                        GeneratedField::ClientAgent => {
                            if client_agent__.is_some() {
                                return Err(serde::de::Error::duplicate_field("clientAgent"));
                            }
                            client_agent__ = map_.next_value()?;
                        }
                        GeneratedField::LastSeen => {
                            if last_seen__.is_some() {
                                return Err(serde::de::Error::duplicate_field("lastSeen"));
                            }
                            last_seen__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Capabilities => {
                            if capabilities__.is_some() {
                                return Err(serde::de::Error::duplicate_field("capabilities"));
                            }
                            capabilities__ = 
                                Some(map_.next_value::<::pbjson::private::BytesDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::IsUpdater => {
                            if is_updater__.is_some() {
                                return Err(serde::de::Error::duplicate_field("isUpdater"));
                            }
                            is_updater__ = Some(map_.next_value()?);
                        }
                        GeneratedField::ClientUpdater => {
                            if client_updater__.is_some() {
                                return Err(serde::de::Error::duplicate_field("clientUpdater"));
                            }
                            client_updater__ = map_.next_value()?;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(Client {
                    state: state__,
                    id: id__.unwrap_or_default(),
                    products: products__.unwrap_or_default(),
                    is_tracer: is_tracer__.unwrap_or_default(),
                    client_tracer: client_tracer__,
                    is_agent: is_agent__.unwrap_or_default(),
                    client_agent: client_agent__,
                    last_seen: last_seen__.unwrap_or_default(),
                    capabilities: capabilities__.unwrap_or_default(),
                    is_updater: is_updater__.unwrap_or_default(),
                    client_updater: client_updater__,
                })
            }
        }
        deserializer.deserialize_struct("datadog.config.Client", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for ClientAgent {
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
        if !self.cluster_name.is_empty() {
            len += 1;
        }
        if !self.cluster_id.is_empty() {
            len += 1;
        }
        if !self.cws_workloads.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.config.ClientAgent", len)?;
        if !self.name.is_empty() {
            struct_ser.serialize_field("name", &self.name)?;
        }
        if !self.version.is_empty() {
            struct_ser.serialize_field("version", &self.version)?;
        }
        if !self.cluster_name.is_empty() {
            struct_ser.serialize_field("clusterName", &self.cluster_name)?;
        }
        if !self.cluster_id.is_empty() {
            struct_ser.serialize_field("clusterId", &self.cluster_id)?;
        }
        if !self.cws_workloads.is_empty() {
            struct_ser.serialize_field("cwsWorkloads", &self.cws_workloads)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for ClientAgent {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "name",
            "version",
            "cluster_name",
            "clusterName",
            "cluster_id",
            "clusterId",
            "cws_workloads",
            "cwsWorkloads",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Name,
            Version,
            ClusterName,
            ClusterId,
            CwsWorkloads,
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
                            "clusterName" | "cluster_name" => Ok(GeneratedField::ClusterName),
                            "clusterId" | "cluster_id" => Ok(GeneratedField::ClusterId),
                            "cwsWorkloads" | "cws_workloads" => Ok(GeneratedField::CwsWorkloads),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = ClientAgent;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.config.ClientAgent")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<ClientAgent, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut name__ = None;
                let mut version__ = None;
                let mut cluster_name__ = None;
                let mut cluster_id__ = None;
                let mut cws_workloads__ = None;
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
                        GeneratedField::ClusterName => {
                            if cluster_name__.is_some() {
                                return Err(serde::de::Error::duplicate_field("clusterName"));
                            }
                            cluster_name__ = Some(map_.next_value()?);
                        }
                        GeneratedField::ClusterId => {
                            if cluster_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("clusterId"));
                            }
                            cluster_id__ = Some(map_.next_value()?);
                        }
                        GeneratedField::CwsWorkloads => {
                            if cws_workloads__.is_some() {
                                return Err(serde::de::Error::duplicate_field("cwsWorkloads"));
                            }
                            cws_workloads__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(ClientAgent {
                    name: name__.unwrap_or_default(),
                    version: version__.unwrap_or_default(),
                    cluster_name: cluster_name__.unwrap_or_default(),
                    cluster_id: cluster_id__.unwrap_or_default(),
                    cws_workloads: cws_workloads__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.config.ClientAgent", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for ClientGetConfigsRequest {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.client.is_some() {
            len += 1;
        }
        if !self.cached_target_files.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.config.ClientGetConfigsRequest", len)?;
        if let Some(v) = self.client.as_ref() {
            struct_ser.serialize_field("client", v)?;
        }
        if !self.cached_target_files.is_empty() {
            struct_ser.serialize_field("cachedTargetFiles", &self.cached_target_files)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for ClientGetConfigsRequest {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "client",
            "cached_target_files",
            "cachedTargetFiles",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Client,
            CachedTargetFiles,
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
                            "client" => Ok(GeneratedField::Client),
                            "cachedTargetFiles" | "cached_target_files" => Ok(GeneratedField::CachedTargetFiles),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = ClientGetConfigsRequest;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.config.ClientGetConfigsRequest")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<ClientGetConfigsRequest, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut client__ = None;
                let mut cached_target_files__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Client => {
                            if client__.is_some() {
                                return Err(serde::de::Error::duplicate_field("client"));
                            }
                            client__ = map_.next_value()?;
                        }
                        GeneratedField::CachedTargetFiles => {
                            if cached_target_files__.is_some() {
                                return Err(serde::de::Error::duplicate_field("cachedTargetFiles"));
                            }
                            cached_target_files__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(ClientGetConfigsRequest {
                    client: client__,
                    cached_target_files: cached_target_files__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.config.ClientGetConfigsRequest", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for ClientGetConfigsResponse {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.roots.is_empty() {
            len += 1;
        }
        if !self.targets.is_empty() {
            len += 1;
        }
        if !self.target_files.is_empty() {
            len += 1;
        }
        if !self.client_configs.is_empty() {
            len += 1;
        }
        if self.config_status != 0 {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.config.ClientGetConfigsResponse", len)?;
        if !self.roots.is_empty() {
            struct_ser.serialize_field("roots", &self.roots.iter().map(pbjson::private::base64::encode).collect::<Vec<_>>())?;
        }
        if !self.targets.is_empty() {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("targets", pbjson::private::base64::encode(&self.targets).as_str())?;
        }
        if !self.target_files.is_empty() {
            struct_ser.serialize_field("targetFiles", &self.target_files)?;
        }
        if !self.client_configs.is_empty() {
            struct_ser.serialize_field("clientConfigs", &self.client_configs)?;
        }
        if self.config_status != 0 {
            let v = ConfigStatus::try_from(self.config_status)
                .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", self.config_status)))?;
            struct_ser.serialize_field("configStatus", &v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for ClientGetConfigsResponse {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "roots",
            "targets",
            "target_files",
            "targetFiles",
            "client_configs",
            "clientConfigs",
            "config_status",
            "configStatus",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Roots,
            Targets,
            TargetFiles,
            ClientConfigs,
            ConfigStatus,
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
                            "roots" => Ok(GeneratedField::Roots),
                            "targets" => Ok(GeneratedField::Targets),
                            "targetFiles" | "target_files" => Ok(GeneratedField::TargetFiles),
                            "clientConfigs" | "client_configs" => Ok(GeneratedField::ClientConfigs),
                            "configStatus" | "config_status" => Ok(GeneratedField::ConfigStatus),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = ClientGetConfigsResponse;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.config.ClientGetConfigsResponse")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<ClientGetConfigsResponse, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut roots__ = None;
                let mut targets__ = None;
                let mut target_files__ = None;
                let mut client_configs__ = None;
                let mut config_status__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Roots => {
                            if roots__.is_some() {
                                return Err(serde::de::Error::duplicate_field("roots"));
                            }
                            roots__ = 
                                Some(map_.next_value::<Vec<::pbjson::private::BytesDeserialize<_>>>()?
                                    .into_iter().map(|x| x.0).collect())
                            ;
                        }
                        GeneratedField::Targets => {
                            if targets__.is_some() {
                                return Err(serde::de::Error::duplicate_field("targets"));
                            }
                            targets__ = 
                                Some(map_.next_value::<::pbjson::private::BytesDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::TargetFiles => {
                            if target_files__.is_some() {
                                return Err(serde::de::Error::duplicate_field("targetFiles"));
                            }
                            target_files__ = Some(map_.next_value()?);
                        }
                        GeneratedField::ClientConfigs => {
                            if client_configs__.is_some() {
                                return Err(serde::de::Error::duplicate_field("clientConfigs"));
                            }
                            client_configs__ = Some(map_.next_value()?);
                        }
                        GeneratedField::ConfigStatus => {
                            if config_status__.is_some() {
                                return Err(serde::de::Error::duplicate_field("configStatus"));
                            }
                            config_status__ = Some(map_.next_value::<ConfigStatus>()? as i32);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(ClientGetConfigsResponse {
                    roots: roots__.unwrap_or_default(),
                    targets: targets__.unwrap_or_default(),
                    target_files: target_files__.unwrap_or_default(),
                    client_configs: client_configs__.unwrap_or_default(),
                    config_status: config_status__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.config.ClientGetConfigsResponse", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for ClientState {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.root_version != 0 {
            len += 1;
        }
        if self.targets_version != 0 {
            len += 1;
        }
        if !self.config_states.is_empty() {
            len += 1;
        }
        if self.has_error {
            len += 1;
        }
        if !self.error.is_empty() {
            len += 1;
        }
        if !self.backend_client_state.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.config.ClientState", len)?;
        if self.root_version != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("rootVersion", ToString::to_string(&self.root_version).as_str())?;
        }
        if self.targets_version != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("targetsVersion", ToString::to_string(&self.targets_version).as_str())?;
        }
        if !self.config_states.is_empty() {
            struct_ser.serialize_field("configStates", &self.config_states)?;
        }
        if self.has_error {
            struct_ser.serialize_field("hasError", &self.has_error)?;
        }
        if !self.error.is_empty() {
            struct_ser.serialize_field("error", &self.error)?;
        }
        if !self.backend_client_state.is_empty() {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("backendClientState", pbjson::private::base64::encode(&self.backend_client_state).as_str())?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for ClientState {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "root_version",
            "rootVersion",
            "targets_version",
            "targetsVersion",
            "config_states",
            "configStates",
            "has_error",
            "hasError",
            "error",
            "backend_client_state",
            "backendClientState",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            RootVersion,
            TargetsVersion,
            ConfigStates,
            HasError,
            Error,
            BackendClientState,
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
                            "rootVersion" | "root_version" => Ok(GeneratedField::RootVersion),
                            "targetsVersion" | "targets_version" => Ok(GeneratedField::TargetsVersion),
                            "configStates" | "config_states" => Ok(GeneratedField::ConfigStates),
                            "hasError" | "has_error" => Ok(GeneratedField::HasError),
                            "error" => Ok(GeneratedField::Error),
                            "backendClientState" | "backend_client_state" => Ok(GeneratedField::BackendClientState),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = ClientState;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.config.ClientState")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<ClientState, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut root_version__ = None;
                let mut targets_version__ = None;
                let mut config_states__ = None;
                let mut has_error__ = None;
                let mut error__ = None;
                let mut backend_client_state__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::RootVersion => {
                            if root_version__.is_some() {
                                return Err(serde::de::Error::duplicate_field("rootVersion"));
                            }
                            root_version__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::TargetsVersion => {
                            if targets_version__.is_some() {
                                return Err(serde::de::Error::duplicate_field("targetsVersion"));
                            }
                            targets_version__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::ConfigStates => {
                            if config_states__.is_some() {
                                return Err(serde::de::Error::duplicate_field("configStates"));
                            }
                            config_states__ = Some(map_.next_value()?);
                        }
                        GeneratedField::HasError => {
                            if has_error__.is_some() {
                                return Err(serde::de::Error::duplicate_field("hasError"));
                            }
                            has_error__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Error => {
                            if error__.is_some() {
                                return Err(serde::de::Error::duplicate_field("error"));
                            }
                            error__ = Some(map_.next_value()?);
                        }
                        GeneratedField::BackendClientState => {
                            if backend_client_state__.is_some() {
                                return Err(serde::de::Error::duplicate_field("backendClientState"));
                            }
                            backend_client_state__ = 
                                Some(map_.next_value::<::pbjson::private::BytesDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(ClientState {
                    root_version: root_version__.unwrap_or_default(),
                    targets_version: targets_version__.unwrap_or_default(),
                    config_states: config_states__.unwrap_or_default(),
                    has_error: has_error__.unwrap_or_default(),
                    error: error__.unwrap_or_default(),
                    backend_client_state: backend_client_state__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.config.ClientState", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for ClientTracer {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.runtime_id.is_empty() {
            len += 1;
        }
        if !self.language.is_empty() {
            len += 1;
        }
        if !self.tracer_version.is_empty() {
            len += 1;
        }
        if !self.service.is_empty() {
            len += 1;
        }
        if !self.extra_services.is_empty() {
            len += 1;
        }
        if !self.env.is_empty() {
            len += 1;
        }
        if !self.app_version.is_empty() {
            len += 1;
        }
        if !self.tags.is_empty() {
            len += 1;
        }
        if !self.process_tags.is_empty() {
            len += 1;
        }
        if !self.container_tags.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.config.ClientTracer", len)?;
        if !self.runtime_id.is_empty() {
            struct_ser.serialize_field("runtimeId", &self.runtime_id)?;
        }
        if !self.language.is_empty() {
            struct_ser.serialize_field("language", &self.language)?;
        }
        if !self.tracer_version.is_empty() {
            struct_ser.serialize_field("tracerVersion", &self.tracer_version)?;
        }
        if !self.service.is_empty() {
            struct_ser.serialize_field("service", &self.service)?;
        }
        if !self.extra_services.is_empty() {
            struct_ser.serialize_field("extraServices", &self.extra_services)?;
        }
        if !self.env.is_empty() {
            struct_ser.serialize_field("env", &self.env)?;
        }
        if !self.app_version.is_empty() {
            struct_ser.serialize_field("appVersion", &self.app_version)?;
        }
        if !self.tags.is_empty() {
            struct_ser.serialize_field("tags", &self.tags)?;
        }
        if !self.process_tags.is_empty() {
            struct_ser.serialize_field("processTags", &self.process_tags)?;
        }
        if !self.container_tags.is_empty() {
            struct_ser.serialize_field("containerTags", &self.container_tags)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for ClientTracer {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "runtime_id",
            "runtimeId",
            "language",
            "tracer_version",
            "tracerVersion",
            "service",
            "extra_services",
            "extraServices",
            "env",
            "app_version",
            "appVersion",
            "tags",
            "process_tags",
            "processTags",
            "container_tags",
            "containerTags",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            RuntimeId,
            Language,
            TracerVersion,
            Service,
            ExtraServices,
            Env,
            AppVersion,
            Tags,
            ProcessTags,
            ContainerTags,
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
                            "runtimeId" | "runtime_id" => Ok(GeneratedField::RuntimeId),
                            "language" => Ok(GeneratedField::Language),
                            "tracerVersion" | "tracer_version" => Ok(GeneratedField::TracerVersion),
                            "service" => Ok(GeneratedField::Service),
                            "extraServices" | "extra_services" => Ok(GeneratedField::ExtraServices),
                            "env" => Ok(GeneratedField::Env),
                            "appVersion" | "app_version" => Ok(GeneratedField::AppVersion),
                            "tags" => Ok(GeneratedField::Tags),
                            "processTags" | "process_tags" => Ok(GeneratedField::ProcessTags),
                            "containerTags" | "container_tags" => Ok(GeneratedField::ContainerTags),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = ClientTracer;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.config.ClientTracer")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<ClientTracer, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut runtime_id__ = None;
                let mut language__ = None;
                let mut tracer_version__ = None;
                let mut service__ = None;
                let mut extra_services__ = None;
                let mut env__ = None;
                let mut app_version__ = None;
                let mut tags__ = None;
                let mut process_tags__ = None;
                let mut container_tags__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::RuntimeId => {
                            if runtime_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("runtimeId"));
                            }
                            runtime_id__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Language => {
                            if language__.is_some() {
                                return Err(serde::de::Error::duplicate_field("language"));
                            }
                            language__ = Some(map_.next_value()?);
                        }
                        GeneratedField::TracerVersion => {
                            if tracer_version__.is_some() {
                                return Err(serde::de::Error::duplicate_field("tracerVersion"));
                            }
                            tracer_version__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Service => {
                            if service__.is_some() {
                                return Err(serde::de::Error::duplicate_field("service"));
                            }
                            service__ = Some(map_.next_value()?);
                        }
                        GeneratedField::ExtraServices => {
                            if extra_services__.is_some() {
                                return Err(serde::de::Error::duplicate_field("extraServices"));
                            }
                            extra_services__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Env => {
                            if env__.is_some() {
                                return Err(serde::de::Error::duplicate_field("env"));
                            }
                            env__ = Some(map_.next_value()?);
                        }
                        GeneratedField::AppVersion => {
                            if app_version__.is_some() {
                                return Err(serde::de::Error::duplicate_field("appVersion"));
                            }
                            app_version__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Tags => {
                            if tags__.is_some() {
                                return Err(serde::de::Error::duplicate_field("tags"));
                            }
                            tags__ = Some(map_.next_value()?);
                        }
                        GeneratedField::ProcessTags => {
                            if process_tags__.is_some() {
                                return Err(serde::de::Error::duplicate_field("processTags"));
                            }
                            process_tags__ = Some(map_.next_value()?);
                        }
                        GeneratedField::ContainerTags => {
                            if container_tags__.is_some() {
                                return Err(serde::de::Error::duplicate_field("containerTags"));
                            }
                            container_tags__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(ClientTracer {
                    runtime_id: runtime_id__.unwrap_or_default(),
                    language: language__.unwrap_or_default(),
                    tracer_version: tracer_version__.unwrap_or_default(),
                    service: service__.unwrap_or_default(),
                    extra_services: extra_services__.unwrap_or_default(),
                    env: env__.unwrap_or_default(),
                    app_version: app_version__.unwrap_or_default(),
                    tags: tags__.unwrap_or_default(),
                    process_tags: process_tags__.unwrap_or_default(),
                    container_tags: container_tags__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.config.ClientTracer", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for ClientUpdater {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.tags.is_empty() {
            len += 1;
        }
        if !self.packages.is_empty() {
            len += 1;
        }
        if self.available_disk_space != 0 {
            len += 1;
        }
        if !self.secrets_pub_key.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.config.ClientUpdater", len)?;
        if !self.tags.is_empty() {
            struct_ser.serialize_field("tags", &self.tags)?;
        }
        if !self.packages.is_empty() {
            struct_ser.serialize_field("packages", &self.packages)?;
        }
        if self.available_disk_space != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("availableDiskSpace", ToString::to_string(&self.available_disk_space).as_str())?;
        }
        if !self.secrets_pub_key.is_empty() {
            struct_ser.serialize_field("secretsPubKey", &self.secrets_pub_key)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for ClientUpdater {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "tags",
            "packages",
            "available_disk_space",
            "availableDiskSpace",
            "secrets_pub_key",
            "secretsPubKey",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Tags,
            Packages,
            AvailableDiskSpace,
            SecretsPubKey,
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
                            "tags" => Ok(GeneratedField::Tags),
                            "packages" => Ok(GeneratedField::Packages),
                            "availableDiskSpace" | "available_disk_space" => Ok(GeneratedField::AvailableDiskSpace),
                            "secretsPubKey" | "secrets_pub_key" => Ok(GeneratedField::SecretsPubKey),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = ClientUpdater;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.config.ClientUpdater")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<ClientUpdater, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut tags__ = None;
                let mut packages__ = None;
                let mut available_disk_space__ = None;
                let mut secrets_pub_key__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Tags => {
                            if tags__.is_some() {
                                return Err(serde::de::Error::duplicate_field("tags"));
                            }
                            tags__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Packages => {
                            if packages__.is_some() {
                                return Err(serde::de::Error::duplicate_field("packages"));
                            }
                            packages__ = Some(map_.next_value()?);
                        }
                        GeneratedField::AvailableDiskSpace => {
                            if available_disk_space__.is_some() {
                                return Err(serde::de::Error::duplicate_field("availableDiskSpace"));
                            }
                            available_disk_space__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::SecretsPubKey => {
                            if secrets_pub_key__.is_some() {
                                return Err(serde::de::Error::duplicate_field("secretsPubKey"));
                            }
                            secrets_pub_key__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(ClientUpdater {
                    tags: tags__.unwrap_or_default(),
                    packages: packages__.unwrap_or_default(),
                    available_disk_space: available_disk_space__.unwrap_or_default(),
                    secrets_pub_key: secrets_pub_key__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.config.ClientUpdater", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for ConfigMetas {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.roots.is_empty() {
            len += 1;
        }
        if self.timestamp.is_some() {
            len += 1;
        }
        if self.snapshot.is_some() {
            len += 1;
        }
        if self.top_targets.is_some() {
            len += 1;
        }
        if !self.delegated_targets.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.config.ConfigMetas", len)?;
        if !self.roots.is_empty() {
            struct_ser.serialize_field("roots", &self.roots)?;
        }
        if let Some(v) = self.timestamp.as_ref() {
            struct_ser.serialize_field("timestamp", v)?;
        }
        if let Some(v) = self.snapshot.as_ref() {
            struct_ser.serialize_field("snapshot", v)?;
        }
        if let Some(v) = self.top_targets.as_ref() {
            struct_ser.serialize_field("topTargets", v)?;
        }
        if !self.delegated_targets.is_empty() {
            struct_ser.serialize_field("delegatedTargets", &self.delegated_targets)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for ConfigMetas {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "roots",
            "timestamp",
            "snapshot",
            "topTargets",
            "delegatedTargets",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Roots,
            Timestamp,
            Snapshot,
            TopTargets,
            DelegatedTargets,
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
                            "roots" => Ok(GeneratedField::Roots),
                            "timestamp" => Ok(GeneratedField::Timestamp),
                            "snapshot" => Ok(GeneratedField::Snapshot),
                            "topTargets" => Ok(GeneratedField::TopTargets),
                            "delegatedTargets" => Ok(GeneratedField::DelegatedTargets),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = ConfigMetas;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.config.ConfigMetas")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<ConfigMetas, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut roots__ = None;
                let mut timestamp__ = None;
                let mut snapshot__ = None;
                let mut top_targets__ = None;
                let mut delegated_targets__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Roots => {
                            if roots__.is_some() {
                                return Err(serde::de::Error::duplicate_field("roots"));
                            }
                            roots__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Timestamp => {
                            if timestamp__.is_some() {
                                return Err(serde::de::Error::duplicate_field("timestamp"));
                            }
                            timestamp__ = map_.next_value()?;
                        }
                        GeneratedField::Snapshot => {
                            if snapshot__.is_some() {
                                return Err(serde::de::Error::duplicate_field("snapshot"));
                            }
                            snapshot__ = map_.next_value()?;
                        }
                        GeneratedField::TopTargets => {
                            if top_targets__.is_some() {
                                return Err(serde::de::Error::duplicate_field("topTargets"));
                            }
                            top_targets__ = map_.next_value()?;
                        }
                        GeneratedField::DelegatedTargets => {
                            if delegated_targets__.is_some() {
                                return Err(serde::de::Error::duplicate_field("delegatedTargets"));
                            }
                            delegated_targets__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(ConfigMetas {
                    roots: roots__.unwrap_or_default(),
                    timestamp: timestamp__,
                    snapshot: snapshot__,
                    top_targets: top_targets__,
                    delegated_targets: delegated_targets__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.config.ConfigMetas", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for ConfigState {
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
        if self.version != 0 {
            len += 1;
        }
        if !self.product.is_empty() {
            len += 1;
        }
        if self.apply_state != 0 {
            len += 1;
        }
        if !self.apply_error.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.config.ConfigState", len)?;
        if !self.id.is_empty() {
            struct_ser.serialize_field("id", &self.id)?;
        }
        if self.version != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("version", ToString::to_string(&self.version).as_str())?;
        }
        if !self.product.is_empty() {
            struct_ser.serialize_field("product", &self.product)?;
        }
        if self.apply_state != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("applyState", ToString::to_string(&self.apply_state).as_str())?;
        }
        if !self.apply_error.is_empty() {
            struct_ser.serialize_field("applyError", &self.apply_error)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for ConfigState {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "id",
            "version",
            "product",
            "apply_state",
            "applyState",
            "apply_error",
            "applyError",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Id,
            Version,
            Product,
            ApplyState,
            ApplyError,
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
                            "version" => Ok(GeneratedField::Version),
                            "product" => Ok(GeneratedField::Product),
                            "applyState" | "apply_state" => Ok(GeneratedField::ApplyState),
                            "applyError" | "apply_error" => Ok(GeneratedField::ApplyError),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = ConfigState;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.config.ConfigState")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<ConfigState, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut id__ = None;
                let mut version__ = None;
                let mut product__ = None;
                let mut apply_state__ = None;
                let mut apply_error__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Id => {
                            if id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("id"));
                            }
                            id__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Version => {
                            if version__.is_some() {
                                return Err(serde::de::Error::duplicate_field("version"));
                            }
                            version__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Product => {
                            if product__.is_some() {
                                return Err(serde::de::Error::duplicate_field("product"));
                            }
                            product__ = Some(map_.next_value()?);
                        }
                        GeneratedField::ApplyState => {
                            if apply_state__.is_some() {
                                return Err(serde::de::Error::duplicate_field("applyState"));
                            }
                            apply_state__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::ApplyError => {
                            if apply_error__.is_some() {
                                return Err(serde::de::Error::duplicate_field("applyError"));
                            }
                            apply_error__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(ConfigState {
                    id: id__.unwrap_or_default(),
                    version: version__.unwrap_or_default(),
                    product: product__.unwrap_or_default(),
                    apply_state: apply_state__.unwrap_or_default(),
                    apply_error: apply_error__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.config.ConfigState", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for ConfigStatus {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let variant = match self {
            Self::Ok => "CONFIG_STATUS_OK",
            Self::Expired => "CONFIG_STATUS_EXPIRED",
        };
        serializer.serialize_str(variant)
    }
}
impl<'de> serde::Deserialize<'de> for ConfigStatus {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "CONFIG_STATUS_OK",
            "CONFIG_STATUS_EXPIRED",
        ];

        struct GeneratedVisitor;

        impl serde::de::Visitor<'_> for GeneratedVisitor {
            type Value = ConfigStatus;

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
                    "CONFIG_STATUS_OK" => Ok(ConfigStatus::Ok),
                    "CONFIG_STATUS_EXPIRED" => Ok(ConfigStatus::Expired),
                    _ => Err(serde::de::Error::unknown_variant(value, FIELDS)),
                }
            }
        }
        deserializer.deserialize_any(GeneratedVisitor)
    }
}
impl serde::Serialize for ConfigSubscriptionProducts {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let variant = match self {
            Self::Invalid => "INVALID",
            Self::LiveDebugging => "LIVE_DEBUGGING",
        };
        serializer.serialize_str(variant)
    }
}
impl<'de> serde::Deserialize<'de> for ConfigSubscriptionProducts {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "INVALID",
            "LIVE_DEBUGGING",
        ];

        struct GeneratedVisitor;

        impl serde::de::Visitor<'_> for GeneratedVisitor {
            type Value = ConfigSubscriptionProducts;

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
                    "INVALID" => Ok(ConfigSubscriptionProducts::Invalid),
                    "LIVE_DEBUGGING" => Ok(ConfigSubscriptionProducts::LiveDebugging),
                    _ => Err(serde::de::Error::unknown_variant(value, FIELDS)),
                }
            }
        }
        deserializer.deserialize_any(GeneratedVisitor)
    }
}
impl serde::Serialize for ConfigSubscriptionRequest {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.runtime_id.is_empty() {
            len += 1;
        }
        if self.action != 0 {
            len += 1;
        }
        if self.products != 0 {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.config.ConfigSubscriptionRequest", len)?;
        if !self.runtime_id.is_empty() {
            struct_ser.serialize_field("runtimeId", &self.runtime_id)?;
        }
        if self.action != 0 {
            let v = config_subscription_request::Action::try_from(self.action)
                .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", self.action)))?;
            struct_ser.serialize_field("action", &v)?;
        }
        if self.products != 0 {
            let v = ConfigSubscriptionProducts::try_from(self.products)
                .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", self.products)))?;
            struct_ser.serialize_field("products", &v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for ConfigSubscriptionRequest {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "runtime_id",
            "runtimeId",
            "action",
            "products",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            RuntimeId,
            Action,
            Products,
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
                            "runtimeId" | "runtime_id" => Ok(GeneratedField::RuntimeId),
                            "action" => Ok(GeneratedField::Action),
                            "products" => Ok(GeneratedField::Products),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = ConfigSubscriptionRequest;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.config.ConfigSubscriptionRequest")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<ConfigSubscriptionRequest, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut runtime_id__ = None;
                let mut action__ = None;
                let mut products__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::RuntimeId => {
                            if runtime_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("runtimeId"));
                            }
                            runtime_id__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Action => {
                            if action__.is_some() {
                                return Err(serde::de::Error::duplicate_field("action"));
                            }
                            action__ = Some(map_.next_value::<config_subscription_request::Action>()? as i32);
                        }
                        GeneratedField::Products => {
                            if products__.is_some() {
                                return Err(serde::de::Error::duplicate_field("products"));
                            }
                            products__ = Some(map_.next_value::<ConfigSubscriptionProducts>()? as i32);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(ConfigSubscriptionRequest {
                    runtime_id: runtime_id__.unwrap_or_default(),
                    action: action__.unwrap_or_default(),
                    products: products__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.config.ConfigSubscriptionRequest", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for config_subscription_request::Action {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let variant = match self {
            Self::Invalid => "INVALID",
            Self::Track => "TRACK",
            Self::Untrack => "UNTRACK",
        };
        serializer.serialize_str(variant)
    }
}
impl<'de> serde::Deserialize<'de> for config_subscription_request::Action {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "INVALID",
            "TRACK",
            "UNTRACK",
        ];

        struct GeneratedVisitor;

        impl serde::de::Visitor<'_> for GeneratedVisitor {
            type Value = config_subscription_request::Action;

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
                    "INVALID" => Ok(config_subscription_request::Action::Invalid),
                    "TRACK" => Ok(config_subscription_request::Action::Track),
                    "UNTRACK" => Ok(config_subscription_request::Action::Untrack),
                    _ => Err(serde::de::Error::unknown_variant(value, FIELDS)),
                }
            }
        }
        deserializer.deserialize_any(GeneratedVisitor)
    }
}
impl serde::Serialize for ConfigSubscriptionResponse {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.client.is_some() {
            len += 1;
        }
        if !self.matched_configs.is_empty() {
            len += 1;
        }
        if !self.target_files.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.config.ConfigSubscriptionResponse", len)?;
        if let Some(v) = self.client.as_ref() {
            struct_ser.serialize_field("client", v)?;
        }
        if !self.matched_configs.is_empty() {
            struct_ser.serialize_field("matchedConfigs", &self.matched_configs)?;
        }
        if !self.target_files.is_empty() {
            struct_ser.serialize_field("targetFiles", &self.target_files)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for ConfigSubscriptionResponse {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "client",
            "matched_configs",
            "matchedConfigs",
            "target_files",
            "targetFiles",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Client,
            MatchedConfigs,
            TargetFiles,
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
                            "client" => Ok(GeneratedField::Client),
                            "matchedConfigs" | "matched_configs" => Ok(GeneratedField::MatchedConfigs),
                            "targetFiles" | "target_files" => Ok(GeneratedField::TargetFiles),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = ConfigSubscriptionResponse;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.config.ConfigSubscriptionResponse")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<ConfigSubscriptionResponse, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut client__ = None;
                let mut matched_configs__ = None;
                let mut target_files__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Client => {
                            if client__.is_some() {
                                return Err(serde::de::Error::duplicate_field("client"));
                            }
                            client__ = map_.next_value()?;
                        }
                        GeneratedField::MatchedConfigs => {
                            if matched_configs__.is_some() {
                                return Err(serde::de::Error::duplicate_field("matchedConfigs"));
                            }
                            matched_configs__ = Some(map_.next_value()?);
                        }
                        GeneratedField::TargetFiles => {
                            if target_files__.is_some() {
                                return Err(serde::de::Error::duplicate_field("targetFiles"));
                            }
                            target_files__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(ConfigSubscriptionResponse {
                    client: client__,
                    matched_configs: matched_configs__.unwrap_or_default(),
                    target_files: target_files__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.config.ConfigSubscriptionResponse", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for ConfigSubscriptionState {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.subscription_id != 0 {
            len += 1;
        }
        if !self.tracked_clients.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.config.ConfigSubscriptionState", len)?;
        if self.subscription_id != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("subscriptionId", ToString::to_string(&self.subscription_id).as_str())?;
        }
        if !self.tracked_clients.is_empty() {
            struct_ser.serialize_field("trackedClients", &self.tracked_clients)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for ConfigSubscriptionState {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "subscription_id",
            "subscriptionId",
            "tracked_clients",
            "trackedClients",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            SubscriptionId,
            TrackedClients,
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
                            "subscriptionId" | "subscription_id" => Ok(GeneratedField::SubscriptionId),
                            "trackedClients" | "tracked_clients" => Ok(GeneratedField::TrackedClients),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = ConfigSubscriptionState;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.config.ConfigSubscriptionState")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<ConfigSubscriptionState, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut subscription_id__ = None;
                let mut tracked_clients__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::SubscriptionId => {
                            if subscription_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("subscriptionId"));
                            }
                            subscription_id__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::TrackedClients => {
                            if tracked_clients__.is_some() {
                                return Err(serde::de::Error::duplicate_field("trackedClients"));
                            }
                            tracked_clients__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(ConfigSubscriptionState {
                    subscription_id: subscription_id__.unwrap_or_default(),
                    tracked_clients: tracked_clients__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.config.ConfigSubscriptionState", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for config_subscription_state::TrackedClient {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.runtime_id.is_empty() {
            len += 1;
        }
        if self.seen_any {
            len += 1;
        }
        if self.products != 0 {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.config.ConfigSubscriptionState.TrackedClient", len)?;
        if !self.runtime_id.is_empty() {
            struct_ser.serialize_field("runtimeId", &self.runtime_id)?;
        }
        if self.seen_any {
            struct_ser.serialize_field("seenAny", &self.seen_any)?;
        }
        if self.products != 0 {
            let v = ConfigSubscriptionProducts::try_from(self.products)
                .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", self.products)))?;
            struct_ser.serialize_field("products", &v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for config_subscription_state::TrackedClient {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "runtime_id",
            "runtimeId",
            "seen_any",
            "seenAny",
            "products",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            RuntimeId,
            SeenAny,
            Products,
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
                            "runtimeId" | "runtime_id" => Ok(GeneratedField::RuntimeId),
                            "seenAny" | "seen_any" => Ok(GeneratedField::SeenAny),
                            "products" => Ok(GeneratedField::Products),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = config_subscription_state::TrackedClient;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.config.ConfigSubscriptionState.TrackedClient")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<config_subscription_state::TrackedClient, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut runtime_id__ = None;
                let mut seen_any__ = None;
                let mut products__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::RuntimeId => {
                            if runtime_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("runtimeId"));
                            }
                            runtime_id__ = Some(map_.next_value()?);
                        }
                        GeneratedField::SeenAny => {
                            if seen_any__.is_some() {
                                return Err(serde::de::Error::duplicate_field("seenAny"));
                            }
                            seen_any__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Products => {
                            if products__.is_some() {
                                return Err(serde::de::Error::duplicate_field("products"));
                            }
                            products__ = Some(map_.next_value::<ConfigSubscriptionProducts>()? as i32);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(config_subscription_state::TrackedClient {
                    runtime_id: runtime_id__.unwrap_or_default(),
                    seen_any: seen_any__.unwrap_or_default(),
                    products: products__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.config.ConfigSubscriptionState.TrackedClient", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for DelegatedMeta {
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
        if !self.role.is_empty() {
            len += 1;
        }
        if !self.raw.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.config.DelegatedMeta", len)?;
        if self.version != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("version", ToString::to_string(&self.version).as_str())?;
        }
        if !self.role.is_empty() {
            struct_ser.serialize_field("role", &self.role)?;
        }
        if !self.raw.is_empty() {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("raw", pbjson::private::base64::encode(&self.raw).as_str())?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for DelegatedMeta {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "version",
            "role",
            "raw",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Version,
            Role,
            Raw,
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
                            "role" => Ok(GeneratedField::Role),
                            "raw" => Ok(GeneratedField::Raw),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = DelegatedMeta;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.config.DelegatedMeta")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<DelegatedMeta, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut version__ = None;
                let mut role__ = None;
                let mut raw__ = None;
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
                        GeneratedField::Role => {
                            if role__.is_some() {
                                return Err(serde::de::Error::duplicate_field("role"));
                            }
                            role__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Raw => {
                            if raw__.is_some() {
                                return Err(serde::de::Error::duplicate_field("raw"));
                            }
                            raw__ = 
                                Some(map_.next_value::<::pbjson::private::BytesDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(DelegatedMeta {
                    version: version__.unwrap_or_default(),
                    role: role__.unwrap_or_default(),
                    raw: raw__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.config.DelegatedMeta", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for DirectorMetas {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.roots.is_empty() {
            len += 1;
        }
        if self.timestamp.is_some() {
            len += 1;
        }
        if self.snapshot.is_some() {
            len += 1;
        }
        if self.targets.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.config.DirectorMetas", len)?;
        if !self.roots.is_empty() {
            struct_ser.serialize_field("roots", &self.roots)?;
        }
        if let Some(v) = self.timestamp.as_ref() {
            struct_ser.serialize_field("timestamp", v)?;
        }
        if let Some(v) = self.snapshot.as_ref() {
            struct_ser.serialize_field("snapshot", v)?;
        }
        if let Some(v) = self.targets.as_ref() {
            struct_ser.serialize_field("targets", v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for DirectorMetas {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "roots",
            "timestamp",
            "snapshot",
            "targets",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Roots,
            Timestamp,
            Snapshot,
            Targets,
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
                            "roots" => Ok(GeneratedField::Roots),
                            "timestamp" => Ok(GeneratedField::Timestamp),
                            "snapshot" => Ok(GeneratedField::Snapshot),
                            "targets" => Ok(GeneratedField::Targets),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = DirectorMetas;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.config.DirectorMetas")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<DirectorMetas, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut roots__ = None;
                let mut timestamp__ = None;
                let mut snapshot__ = None;
                let mut targets__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Roots => {
                            if roots__.is_some() {
                                return Err(serde::de::Error::duplicate_field("roots"));
                            }
                            roots__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Timestamp => {
                            if timestamp__.is_some() {
                                return Err(serde::de::Error::duplicate_field("timestamp"));
                            }
                            timestamp__ = map_.next_value()?;
                        }
                        GeneratedField::Snapshot => {
                            if snapshot__.is_some() {
                                return Err(serde::de::Error::duplicate_field("snapshot"));
                            }
                            snapshot__ = map_.next_value()?;
                        }
                        GeneratedField::Targets => {
                            if targets__.is_some() {
                                return Err(serde::de::Error::duplicate_field("targets"));
                            }
                            targets__ = map_.next_value()?;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(DirectorMetas {
                    roots: roots__.unwrap_or_default(),
                    timestamp: timestamp__,
                    snapshot: snapshot__,
                    targets: targets__,
                })
            }
        }
        deserializer.deserialize_struct("datadog.config.DirectorMetas", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for File {
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
        if !self.raw.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.config.File", len)?;
        if !self.path.is_empty() {
            struct_ser.serialize_field("path", &self.path)?;
        }
        if !self.raw.is_empty() {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("raw", pbjson::private::base64::encode(&self.raw).as_str())?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for File {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "path",
            "raw",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Path,
            Raw,
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
                            "raw" => Ok(GeneratedField::Raw),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = File;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.config.File")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<File, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut path__ = None;
                let mut raw__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Path => {
                            if path__.is_some() {
                                return Err(serde::de::Error::duplicate_field("path"));
                            }
                            path__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Raw => {
                            if raw__.is_some() {
                                return Err(serde::de::Error::duplicate_field("raw"));
                            }
                            raw__ = 
                                Some(map_.next_value::<::pbjson::private::BytesDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(File {
                    path: path__.unwrap_or_default(),
                    raw: raw__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.config.File", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for FileMetaState {
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
        if !self.hash.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.config.FileMetaState", len)?;
        if self.version != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("version", ToString::to_string(&self.version).as_str())?;
        }
        if !self.hash.is_empty() {
            struct_ser.serialize_field("hash", &self.hash)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for FileMetaState {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "version",
            "hash",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Version,
            Hash,
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
                            "hash" => Ok(GeneratedField::Hash),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = FileMetaState;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.config.FileMetaState")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<FileMetaState, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut version__ = None;
                let mut hash__ = None;
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
                        GeneratedField::Hash => {
                            if hash__.is_some() {
                                return Err(serde::de::Error::duplicate_field("hash"));
                            }
                            hash__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(FileMetaState {
                    version: version__.unwrap_or_default(),
                    hash: hash__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.config.FileMetaState", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for GetStateConfigResponse {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.config_state.is_empty() {
            len += 1;
        }
        if !self.director_state.is_empty() {
            len += 1;
        }
        if !self.target_filenames.is_empty() {
            len += 1;
        }
        if !self.active_clients.is_empty() {
            len += 1;
        }
        if !self.config_subscription_states.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.config.GetStateConfigResponse", len)?;
        if !self.config_state.is_empty() {
            struct_ser.serialize_field("configState", &self.config_state)?;
        }
        if !self.director_state.is_empty() {
            struct_ser.serialize_field("directorState", &self.director_state)?;
        }
        if !self.target_filenames.is_empty() {
            struct_ser.serialize_field("targetFilenames", &self.target_filenames)?;
        }
        if !self.active_clients.is_empty() {
            struct_ser.serialize_field("activeClients", &self.active_clients)?;
        }
        if !self.config_subscription_states.is_empty() {
            struct_ser.serialize_field("configSubscriptionStates", &self.config_subscription_states)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for GetStateConfigResponse {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "config_state",
            "configState",
            "director_state",
            "directorState",
            "target_filenames",
            "targetFilenames",
            "active_clients",
            "activeClients",
            "config_subscription_states",
            "configSubscriptionStates",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            ConfigState,
            DirectorState,
            TargetFilenames,
            ActiveClients,
            ConfigSubscriptionStates,
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
                            "configState" | "config_state" => Ok(GeneratedField::ConfigState),
                            "directorState" | "director_state" => Ok(GeneratedField::DirectorState),
                            "targetFilenames" | "target_filenames" => Ok(GeneratedField::TargetFilenames),
                            "activeClients" | "active_clients" => Ok(GeneratedField::ActiveClients),
                            "configSubscriptionStates" | "config_subscription_states" => Ok(GeneratedField::ConfigSubscriptionStates),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = GetStateConfigResponse;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.config.GetStateConfigResponse")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<GetStateConfigResponse, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut config_state__ = None;
                let mut director_state__ = None;
                let mut target_filenames__ = None;
                let mut active_clients__ = None;
                let mut config_subscription_states__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::ConfigState => {
                            if config_state__.is_some() {
                                return Err(serde::de::Error::duplicate_field("configState"));
                            }
                            config_state__ = Some(
                                map_.next_value::<std::collections::HashMap<_, _>>()?
                            );
                        }
                        GeneratedField::DirectorState => {
                            if director_state__.is_some() {
                                return Err(serde::de::Error::duplicate_field("directorState"));
                            }
                            director_state__ = Some(
                                map_.next_value::<std::collections::HashMap<_, _>>()?
                            );
                        }
                        GeneratedField::TargetFilenames => {
                            if target_filenames__.is_some() {
                                return Err(serde::de::Error::duplicate_field("targetFilenames"));
                            }
                            target_filenames__ = Some(
                                map_.next_value::<std::collections::HashMap<_, _>>()?
                            );
                        }
                        GeneratedField::ActiveClients => {
                            if active_clients__.is_some() {
                                return Err(serde::de::Error::duplicate_field("activeClients"));
                            }
                            active_clients__ = Some(map_.next_value()?);
                        }
                        GeneratedField::ConfigSubscriptionStates => {
                            if config_subscription_states__.is_some() {
                                return Err(serde::de::Error::duplicate_field("configSubscriptionStates"));
                            }
                            config_subscription_states__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(GetStateConfigResponse {
                    config_state: config_state__.unwrap_or_default(),
                    director_state: director_state__.unwrap_or_default(),
                    target_filenames: target_filenames__.unwrap_or_default(),
                    active_clients: active_clients__.unwrap_or_default(),
                    config_subscription_states: config_subscription_states__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.config.GetStateConfigResponse", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for LatestConfigsRequest {
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
        if !self.agent_version.is_empty() {
            len += 1;
        }
        if self.current_config_snapshot_version != 0 {
            len += 1;
        }
        if self.current_config_root_version != 0 {
            len += 1;
        }
        if self.current_director_root_version != 0 {
            len += 1;
        }
        if !self.products.is_empty() {
            len += 1;
        }
        if !self.new_products.is_empty() {
            len += 1;
        }
        if !self.active_clients.is_empty() {
            len += 1;
        }
        if !self.backend_client_state.is_empty() {
            len += 1;
        }
        if self.has_error {
            len += 1;
        }
        if !self.error.is_empty() {
            len += 1;
        }
        if !self.trace_agent_env.is_empty() {
            len += 1;
        }
        if !self.org_uuid.is_empty() {
            len += 1;
        }
        if !self.tags.is_empty() {
            len += 1;
        }
        if !self.agent_uuid.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.config.LatestConfigsRequest", len)?;
        if !self.hostname.is_empty() {
            struct_ser.serialize_field("hostname", &self.hostname)?;
        }
        if !self.agent_version.is_empty() {
            struct_ser.serialize_field("agentVersion", &self.agent_version)?;
        }
        if self.current_config_snapshot_version != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("currentConfigSnapshotVersion", ToString::to_string(&self.current_config_snapshot_version).as_str())?;
        }
        if self.current_config_root_version != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("currentConfigRootVersion", ToString::to_string(&self.current_config_root_version).as_str())?;
        }
        if self.current_director_root_version != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("currentDirectorRootVersion", ToString::to_string(&self.current_director_root_version).as_str())?;
        }
        if !self.products.is_empty() {
            struct_ser.serialize_field("products", &self.products)?;
        }
        if !self.new_products.is_empty() {
            struct_ser.serialize_field("newProducts", &self.new_products)?;
        }
        if !self.active_clients.is_empty() {
            struct_ser.serialize_field("activeClients", &self.active_clients)?;
        }
        if !self.backend_client_state.is_empty() {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("backendClientState", pbjson::private::base64::encode(&self.backend_client_state).as_str())?;
        }
        if self.has_error {
            struct_ser.serialize_field("hasError", &self.has_error)?;
        }
        if !self.error.is_empty() {
            struct_ser.serialize_field("error", &self.error)?;
        }
        if !self.trace_agent_env.is_empty() {
            struct_ser.serialize_field("traceAgentEnv", &self.trace_agent_env)?;
        }
        if !self.org_uuid.is_empty() {
            struct_ser.serialize_field("orgUuid", &self.org_uuid)?;
        }
        if !self.tags.is_empty() {
            struct_ser.serialize_field("tags", &self.tags)?;
        }
        if !self.agent_uuid.is_empty() {
            struct_ser.serialize_field("agentUuid", &self.agent_uuid)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for LatestConfigsRequest {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "hostname",
            "agentVersion",
            "current_config_snapshot_version",
            "currentConfigSnapshotVersion",
            "current_config_root_version",
            "currentConfigRootVersion",
            "current_director_root_version",
            "currentDirectorRootVersion",
            "products",
            "new_products",
            "newProducts",
            "active_clients",
            "activeClients",
            "backend_client_state",
            "backendClientState",
            "has_error",
            "hasError",
            "error",
            "trace_agent_env",
            "traceAgentEnv",
            "org_uuid",
            "orgUuid",
            "tags",
            "agent_uuid",
            "agentUuid",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Hostname,
            AgentVersion,
            CurrentConfigSnapshotVersion,
            CurrentConfigRootVersion,
            CurrentDirectorRootVersion,
            Products,
            NewProducts,
            ActiveClients,
            BackendClientState,
            HasError,
            Error,
            TraceAgentEnv,
            OrgUuid,
            Tags,
            AgentUuid,
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
                            "agentVersion" => Ok(GeneratedField::AgentVersion),
                            "currentConfigSnapshotVersion" | "current_config_snapshot_version" => Ok(GeneratedField::CurrentConfigSnapshotVersion),
                            "currentConfigRootVersion" | "current_config_root_version" => Ok(GeneratedField::CurrentConfigRootVersion),
                            "currentDirectorRootVersion" | "current_director_root_version" => Ok(GeneratedField::CurrentDirectorRootVersion),
                            "products" => Ok(GeneratedField::Products),
                            "newProducts" | "new_products" => Ok(GeneratedField::NewProducts),
                            "activeClients" | "active_clients" => Ok(GeneratedField::ActiveClients),
                            "backendClientState" | "backend_client_state" => Ok(GeneratedField::BackendClientState),
                            "hasError" | "has_error" => Ok(GeneratedField::HasError),
                            "error" => Ok(GeneratedField::Error),
                            "traceAgentEnv" | "trace_agent_env" => Ok(GeneratedField::TraceAgentEnv),
                            "orgUuid" | "org_uuid" => Ok(GeneratedField::OrgUuid),
                            "tags" => Ok(GeneratedField::Tags),
                            "agentUuid" | "agent_uuid" => Ok(GeneratedField::AgentUuid),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = LatestConfigsRequest;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.config.LatestConfigsRequest")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<LatestConfigsRequest, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut hostname__ = None;
                let mut agent_version__ = None;
                let mut current_config_snapshot_version__ = None;
                let mut current_config_root_version__ = None;
                let mut current_director_root_version__ = None;
                let mut products__ = None;
                let mut new_products__ = None;
                let mut active_clients__ = None;
                let mut backend_client_state__ = None;
                let mut has_error__ = None;
                let mut error__ = None;
                let mut trace_agent_env__ = None;
                let mut org_uuid__ = None;
                let mut tags__ = None;
                let mut agent_uuid__ = None;
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
                            agent_version__ = Some(map_.next_value()?);
                        }
                        GeneratedField::CurrentConfigSnapshotVersion => {
                            if current_config_snapshot_version__.is_some() {
                                return Err(serde::de::Error::duplicate_field("currentConfigSnapshotVersion"));
                            }
                            current_config_snapshot_version__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::CurrentConfigRootVersion => {
                            if current_config_root_version__.is_some() {
                                return Err(serde::de::Error::duplicate_field("currentConfigRootVersion"));
                            }
                            current_config_root_version__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::CurrentDirectorRootVersion => {
                            if current_director_root_version__.is_some() {
                                return Err(serde::de::Error::duplicate_field("currentDirectorRootVersion"));
                            }
                            current_director_root_version__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Products => {
                            if products__.is_some() {
                                return Err(serde::de::Error::duplicate_field("products"));
                            }
                            products__ = Some(map_.next_value()?);
                        }
                        GeneratedField::NewProducts => {
                            if new_products__.is_some() {
                                return Err(serde::de::Error::duplicate_field("newProducts"));
                            }
                            new_products__ = Some(map_.next_value()?);
                        }
                        GeneratedField::ActiveClients => {
                            if active_clients__.is_some() {
                                return Err(serde::de::Error::duplicate_field("activeClients"));
                            }
                            active_clients__ = Some(map_.next_value()?);
                        }
                        GeneratedField::BackendClientState => {
                            if backend_client_state__.is_some() {
                                return Err(serde::de::Error::duplicate_field("backendClientState"));
                            }
                            backend_client_state__ = 
                                Some(map_.next_value::<::pbjson::private::BytesDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::HasError => {
                            if has_error__.is_some() {
                                return Err(serde::de::Error::duplicate_field("hasError"));
                            }
                            has_error__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Error => {
                            if error__.is_some() {
                                return Err(serde::de::Error::duplicate_field("error"));
                            }
                            error__ = Some(map_.next_value()?);
                        }
                        GeneratedField::TraceAgentEnv => {
                            if trace_agent_env__.is_some() {
                                return Err(serde::de::Error::duplicate_field("traceAgentEnv"));
                            }
                            trace_agent_env__ = Some(map_.next_value()?);
                        }
                        GeneratedField::OrgUuid => {
                            if org_uuid__.is_some() {
                                return Err(serde::de::Error::duplicate_field("orgUuid"));
                            }
                            org_uuid__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Tags => {
                            if tags__.is_some() {
                                return Err(serde::de::Error::duplicate_field("tags"));
                            }
                            tags__ = Some(map_.next_value()?);
                        }
                        GeneratedField::AgentUuid => {
                            if agent_uuid__.is_some() {
                                return Err(serde::de::Error::duplicate_field("agentUuid"));
                            }
                            agent_uuid__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(LatestConfigsRequest {
                    hostname: hostname__.unwrap_or_default(),
                    agent_version: agent_version__.unwrap_or_default(),
                    current_config_snapshot_version: current_config_snapshot_version__.unwrap_or_default(),
                    current_config_root_version: current_config_root_version__.unwrap_or_default(),
                    current_director_root_version: current_director_root_version__.unwrap_or_default(),
                    products: products__.unwrap_or_default(),
                    new_products: new_products__.unwrap_or_default(),
                    active_clients: active_clients__.unwrap_or_default(),
                    backend_client_state: backend_client_state__.unwrap_or_default(),
                    has_error: has_error__.unwrap_or_default(),
                    error: error__.unwrap_or_default(),
                    trace_agent_env: trace_agent_env__.unwrap_or_default(),
                    org_uuid: org_uuid__.unwrap_or_default(),
                    tags: tags__.unwrap_or_default(),
                    agent_uuid: agent_uuid__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.config.LatestConfigsRequest", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for LatestConfigsResponse {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.config_metas.is_some() {
            len += 1;
        }
        if self.director_metas.is_some() {
            len += 1;
        }
        if !self.target_files.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.config.LatestConfigsResponse", len)?;
        if let Some(v) = self.config_metas.as_ref() {
            struct_ser.serialize_field("configMetas", v)?;
        }
        if let Some(v) = self.director_metas.as_ref() {
            struct_ser.serialize_field("directorMetas", v)?;
        }
        if !self.target_files.is_empty() {
            struct_ser.serialize_field("targetFiles", &self.target_files)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for LatestConfigsResponse {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "config_metas",
            "configMetas",
            "director_metas",
            "directorMetas",
            "target_files",
            "targetFiles",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            ConfigMetas,
            DirectorMetas,
            TargetFiles,
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
                            "configMetas" | "config_metas" => Ok(GeneratedField::ConfigMetas),
                            "directorMetas" | "director_metas" => Ok(GeneratedField::DirectorMetas),
                            "targetFiles" | "target_files" => Ok(GeneratedField::TargetFiles),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = LatestConfigsResponse;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.config.LatestConfigsResponse")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<LatestConfigsResponse, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut config_metas__ = None;
                let mut director_metas__ = None;
                let mut target_files__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::ConfigMetas => {
                            if config_metas__.is_some() {
                                return Err(serde::de::Error::duplicate_field("configMetas"));
                            }
                            config_metas__ = map_.next_value()?;
                        }
                        GeneratedField::DirectorMetas => {
                            if director_metas__.is_some() {
                                return Err(serde::de::Error::duplicate_field("directorMetas"));
                            }
                            director_metas__ = map_.next_value()?;
                        }
                        GeneratedField::TargetFiles => {
                            if target_files__.is_some() {
                                return Err(serde::de::Error::duplicate_field("targetFiles"));
                            }
                            target_files__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(LatestConfigsResponse {
                    config_metas: config_metas__,
                    director_metas: director_metas__,
                    target_files: target_files__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.config.LatestConfigsResponse", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for OrgDataResponse {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.uuid.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.config.OrgDataResponse", len)?;
        if !self.uuid.is_empty() {
            struct_ser.serialize_field("uuid", &self.uuid)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for OrgDataResponse {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "uuid",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Uuid,
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
                            "uuid" => Ok(GeneratedField::Uuid),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = OrgDataResponse;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.config.OrgDataResponse")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<OrgDataResponse, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut uuid__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Uuid => {
                            if uuid__.is_some() {
                                return Err(serde::de::Error::duplicate_field("uuid"));
                            }
                            uuid__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(OrgDataResponse {
                    uuid: uuid__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.config.OrgDataResponse", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for OrgStatusResponse {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.enabled {
            len += 1;
        }
        if self.authorized {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.config.OrgStatusResponse", len)?;
        if self.enabled {
            struct_ser.serialize_field("enabled", &self.enabled)?;
        }
        if self.authorized {
            struct_ser.serialize_field("authorized", &self.authorized)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for OrgStatusResponse {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "enabled",
            "authorized",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Enabled,
            Authorized,
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
                            "enabled" => Ok(GeneratedField::Enabled),
                            "authorized" => Ok(GeneratedField::Authorized),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = OrgStatusResponse;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.config.OrgStatusResponse")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<OrgStatusResponse, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut enabled__ = None;
                let mut authorized__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Enabled => {
                            if enabled__.is_some() {
                                return Err(serde::de::Error::duplicate_field("enabled"));
                            }
                            enabled__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Authorized => {
                            if authorized__.is_some() {
                                return Err(serde::de::Error::duplicate_field("authorized"));
                            }
                            authorized__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(OrgStatusResponse {
                    enabled: enabled__.unwrap_or_default(),
                    authorized: authorized__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.config.OrgStatusResponse", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for PackageState {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.package.is_empty() {
            len += 1;
        }
        if !self.stable_version.is_empty() {
            len += 1;
        }
        if !self.experiment_version.is_empty() {
            len += 1;
        }
        if self.task.is_some() {
            len += 1;
        }
        if !self.stable_config_version.is_empty() {
            len += 1;
        }
        if !self.experiment_config_version.is_empty() {
            len += 1;
        }
        if !self.running_version.is_empty() {
            len += 1;
        }
        if !self.running_config_version.is_empty() {
            len += 1;
        }
        if self.heartbeat_timestamp != 0 {
            len += 1;
        }
        if self.completion != 0. {
            len += 1;
        }
        if !self.process_states.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.config.PackageState", len)?;
        if !self.package.is_empty() {
            struct_ser.serialize_field("package", &self.package)?;
        }
        if !self.stable_version.is_empty() {
            struct_ser.serialize_field("stableVersion", &self.stable_version)?;
        }
        if !self.experiment_version.is_empty() {
            struct_ser.serialize_field("experimentVersion", &self.experiment_version)?;
        }
        if let Some(v) = self.task.as_ref() {
            struct_ser.serialize_field("task", v)?;
        }
        if !self.stable_config_version.is_empty() {
            struct_ser.serialize_field("stableConfigVersion", &self.stable_config_version)?;
        }
        if !self.experiment_config_version.is_empty() {
            struct_ser.serialize_field("experimentConfigVersion", &self.experiment_config_version)?;
        }
        if !self.running_version.is_empty() {
            struct_ser.serialize_field("runningVersion", &self.running_version)?;
        }
        if !self.running_config_version.is_empty() {
            struct_ser.serialize_field("runningConfigVersion", &self.running_config_version)?;
        }
        if self.heartbeat_timestamp != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("heartbeatTimestamp", ToString::to_string(&self.heartbeat_timestamp).as_str())?;
        }
        if self.completion != 0. {
            struct_ser.serialize_field("completion", &self.completion)?;
        }
        if !self.process_states.is_empty() {
            struct_ser.serialize_field("processStates", &self.process_states)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for PackageState {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "package",
            "stable_version",
            "stableVersion",
            "experiment_version",
            "experimentVersion",
            "task",
            "stable_config_version",
            "stableConfigVersion",
            "experiment_config_version",
            "experimentConfigVersion",
            "running_version",
            "runningVersion",
            "running_config_version",
            "runningConfigVersion",
            "heartbeat_timestamp",
            "heartbeatTimestamp",
            "completion",
            "process_states",
            "processStates",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Package,
            StableVersion,
            ExperimentVersion,
            Task,
            StableConfigVersion,
            ExperimentConfigVersion,
            RunningVersion,
            RunningConfigVersion,
            HeartbeatTimestamp,
            Completion,
            ProcessStates,
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
                            "package" => Ok(GeneratedField::Package),
                            "stableVersion" | "stable_version" => Ok(GeneratedField::StableVersion),
                            "experimentVersion" | "experiment_version" => Ok(GeneratedField::ExperimentVersion),
                            "task" => Ok(GeneratedField::Task),
                            "stableConfigVersion" | "stable_config_version" => Ok(GeneratedField::StableConfigVersion),
                            "experimentConfigVersion" | "experiment_config_version" => Ok(GeneratedField::ExperimentConfigVersion),
                            "runningVersion" | "running_version" => Ok(GeneratedField::RunningVersion),
                            "runningConfigVersion" | "running_config_version" => Ok(GeneratedField::RunningConfigVersion),
                            "heartbeatTimestamp" | "heartbeat_timestamp" => Ok(GeneratedField::HeartbeatTimestamp),
                            "completion" => Ok(GeneratedField::Completion),
                            "processStates" | "process_states" => Ok(GeneratedField::ProcessStates),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = PackageState;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.config.PackageState")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<PackageState, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut package__ = None;
                let mut stable_version__ = None;
                let mut experiment_version__ = None;
                let mut task__ = None;
                let mut stable_config_version__ = None;
                let mut experiment_config_version__ = None;
                let mut running_version__ = None;
                let mut running_config_version__ = None;
                let mut heartbeat_timestamp__ = None;
                let mut completion__ = None;
                let mut process_states__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Package => {
                            if package__.is_some() {
                                return Err(serde::de::Error::duplicate_field("package"));
                            }
                            package__ = Some(map_.next_value()?);
                        }
                        GeneratedField::StableVersion => {
                            if stable_version__.is_some() {
                                return Err(serde::de::Error::duplicate_field("stableVersion"));
                            }
                            stable_version__ = Some(map_.next_value()?);
                        }
                        GeneratedField::ExperimentVersion => {
                            if experiment_version__.is_some() {
                                return Err(serde::de::Error::duplicate_field("experimentVersion"));
                            }
                            experiment_version__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Task => {
                            if task__.is_some() {
                                return Err(serde::de::Error::duplicate_field("task"));
                            }
                            task__ = map_.next_value()?;
                        }
                        GeneratedField::StableConfigVersion => {
                            if stable_config_version__.is_some() {
                                return Err(serde::de::Error::duplicate_field("stableConfigVersion"));
                            }
                            stable_config_version__ = Some(map_.next_value()?);
                        }
                        GeneratedField::ExperimentConfigVersion => {
                            if experiment_config_version__.is_some() {
                                return Err(serde::de::Error::duplicate_field("experimentConfigVersion"));
                            }
                            experiment_config_version__ = Some(map_.next_value()?);
                        }
                        GeneratedField::RunningVersion => {
                            if running_version__.is_some() {
                                return Err(serde::de::Error::duplicate_field("runningVersion"));
                            }
                            running_version__ = Some(map_.next_value()?);
                        }
                        GeneratedField::RunningConfigVersion => {
                            if running_config_version__.is_some() {
                                return Err(serde::de::Error::duplicate_field("runningConfigVersion"));
                            }
                            running_config_version__ = Some(map_.next_value()?);
                        }
                        GeneratedField::HeartbeatTimestamp => {
                            if heartbeat_timestamp__.is_some() {
                                return Err(serde::de::Error::duplicate_field("heartbeatTimestamp"));
                            }
                            heartbeat_timestamp__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Completion => {
                            if completion__.is_some() {
                                return Err(serde::de::Error::duplicate_field("completion"));
                            }
                            completion__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::ProcessStates => {
                            if process_states__.is_some() {
                                return Err(serde::de::Error::duplicate_field("processStates"));
                            }
                            process_states__ = Some(
                                map_.next_value::<std::collections::HashMap<_, _>>()?
                            );
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(PackageState {
                    package: package__.unwrap_or_default(),
                    stable_version: stable_version__.unwrap_or_default(),
                    experiment_version: experiment_version__.unwrap_or_default(),
                    task: task__,
                    stable_config_version: stable_config_version__.unwrap_or_default(),
                    experiment_config_version: experiment_config_version__.unwrap_or_default(),
                    running_version: running_version__.unwrap_or_default(),
                    running_config_version: running_config_version__.unwrap_or_default(),
                    heartbeat_timestamp: heartbeat_timestamp__.unwrap_or_default(),
                    completion: completion__.unwrap_or_default(),
                    process_states: process_states__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.config.PackageState", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for PackageStateTask {
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
        if self.state != 0 {
            len += 1;
        }
        if self.error.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.config.PackageStateTask", len)?;
        if !self.id.is_empty() {
            struct_ser.serialize_field("id", &self.id)?;
        }
        if self.state != 0 {
            let v = TaskState::try_from(self.state)
                .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", self.state)))?;
            struct_ser.serialize_field("state", &v)?;
        }
        if let Some(v) = self.error.as_ref() {
            struct_ser.serialize_field("error", v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for PackageStateTask {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "id",
            "state",
            "error",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Id,
            State,
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
                            "id" => Ok(GeneratedField::Id),
                            "state" => Ok(GeneratedField::State),
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
            type Value = PackageStateTask;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.config.PackageStateTask")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<PackageStateTask, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut id__ = None;
                let mut state__ = None;
                let mut error__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Id => {
                            if id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("id"));
                            }
                            id__ = Some(map_.next_value()?);
                        }
                        GeneratedField::State => {
                            if state__.is_some() {
                                return Err(serde::de::Error::duplicate_field("state"));
                            }
                            state__ = Some(map_.next_value::<TaskState>()? as i32);
                        }
                        GeneratedField::Error => {
                            if error__.is_some() {
                                return Err(serde::de::Error::duplicate_field("error"));
                            }
                            error__ = map_.next_value()?;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(PackageStateTask {
                    id: id__.unwrap_or_default(),
                    state: state__.unwrap_or_default(),
                    error: error__,
                })
            }
        }
        deserializer.deserialize_struct("datadog.config.PackageStateTask", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for ResetStateConfigResponse {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let len = 0;
        let struct_ser = serializer.serialize_struct("datadog.config.ResetStateConfigResponse", len)?;
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for ResetStateConfigResponse {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
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
                            Ok(GeneratedField::__SkipField__)
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = ResetStateConfigResponse;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.config.ResetStateConfigResponse")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<ResetStateConfigResponse, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                while map_.next_key::<GeneratedField>()?.is_some() {
                    let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                }
                Ok(ResetStateConfigResponse {
                })
            }
        }
        deserializer.deserialize_struct("datadog.config.ResetStateConfigResponse", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for TargetFileHash {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.algorithm.is_empty() {
            len += 1;
        }
        if !self.hash.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.config.TargetFileHash", len)?;
        if !self.algorithm.is_empty() {
            struct_ser.serialize_field("algorithm", &self.algorithm)?;
        }
        if !self.hash.is_empty() {
            struct_ser.serialize_field("hash", &self.hash)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for TargetFileHash {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "algorithm",
            "hash",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Algorithm,
            Hash,
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
                            "algorithm" => Ok(GeneratedField::Algorithm),
                            "hash" => Ok(GeneratedField::Hash),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = TargetFileHash;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.config.TargetFileHash")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<TargetFileHash, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut algorithm__ = None;
                let mut hash__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Algorithm => {
                            if algorithm__.is_some() {
                                return Err(serde::de::Error::duplicate_field("algorithm"));
                            }
                            algorithm__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Hash => {
                            if hash__.is_some() {
                                return Err(serde::de::Error::duplicate_field("hash"));
                            }
                            hash__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(TargetFileHash {
                    algorithm: algorithm__.unwrap_or_default(),
                    hash: hash__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.config.TargetFileHash", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for TargetFileMeta {
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
        if self.length != 0 {
            len += 1;
        }
        if !self.hashes.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.config.TargetFileMeta", len)?;
        if !self.path.is_empty() {
            struct_ser.serialize_field("path", &self.path)?;
        }
        if self.length != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("length", ToString::to_string(&self.length).as_str())?;
        }
        if !self.hashes.is_empty() {
            struct_ser.serialize_field("hashes", &self.hashes)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for TargetFileMeta {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "path",
            "length",
            "hashes",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Path,
            Length,
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
                            "path" => Ok(GeneratedField::Path),
                            "length" => Ok(GeneratedField::Length),
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
            type Value = TargetFileMeta;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.config.TargetFileMeta")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<TargetFileMeta, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut path__ = None;
                let mut length__ = None;
                let mut hashes__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Path => {
                            if path__.is_some() {
                                return Err(serde::de::Error::duplicate_field("path"));
                            }
                            path__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Length => {
                            if length__.is_some() {
                                return Err(serde::de::Error::duplicate_field("length"));
                            }
                            length__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
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
                Ok(TargetFileMeta {
                    path: path__.unwrap_or_default(),
                    length: length__.unwrap_or_default(),
                    hashes: hashes__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.config.TargetFileMeta", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for TaskError {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.code != 0 {
            len += 1;
        }
        if !self.message.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.config.TaskError", len)?;
        if self.code != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("code", ToString::to_string(&self.code).as_str())?;
        }
        if !self.message.is_empty() {
            struct_ser.serialize_field("message", &self.message)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for TaskError {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "code",
            "message",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Code,
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
                            "code" => Ok(GeneratedField::Code),
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
            type Value = TaskError;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.config.TaskError")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<TaskError, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut code__ = None;
                let mut message__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Code => {
                            if code__.is_some() {
                                return Err(serde::de::Error::duplicate_field("code"));
                            }
                            code__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Message => {
                            if message__.is_some() {
                                return Err(serde::de::Error::duplicate_field("message"));
                            }
                            message__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(TaskError {
                    code: code__.unwrap_or_default(),
                    message: message__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.config.TaskError", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for TaskState {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let variant = match self {
            Self::Idle => "IDLE",
            Self::Running => "RUNNING",
            Self::Done => "DONE",
            Self::InvalidState => "INVALID_STATE",
            Self::Error => "ERROR",
        };
        serializer.serialize_str(variant)
    }
}
impl<'de> serde::Deserialize<'de> for TaskState {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "IDLE",
            "RUNNING",
            "DONE",
            "INVALID_STATE",
            "ERROR",
        ];

        struct GeneratedVisitor;

        impl serde::de::Visitor<'_> for GeneratedVisitor {
            type Value = TaskState;

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
                    "IDLE" => Ok(TaskState::Idle),
                    "RUNNING" => Ok(TaskState::Running),
                    "DONE" => Ok(TaskState::Done),
                    "INVALID_STATE" => Ok(TaskState::InvalidState),
                    "ERROR" => Ok(TaskState::Error),
                    _ => Err(serde::de::Error::unknown_variant(value, FIELDS)),
                }
            }
        }
        deserializer.deserialize_any(GeneratedVisitor)
    }
}
impl serde::Serialize for TopMeta {
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
        if !self.raw.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.config.TopMeta", len)?;
        if self.version != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("version", ToString::to_string(&self.version).as_str())?;
        }
        if !self.raw.is_empty() {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("raw", pbjson::private::base64::encode(&self.raw).as_str())?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for TopMeta {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "version",
            "raw",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Version,
            Raw,
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
                            "raw" => Ok(GeneratedField::Raw),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = TopMeta;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.config.TopMeta")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<TopMeta, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut version__ = None;
                let mut raw__ = None;
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
                        GeneratedField::Raw => {
                            if raw__.is_some() {
                                return Err(serde::de::Error::duplicate_field("raw"));
                            }
                            raw__ = 
                                Some(map_.next_value::<::pbjson::private::BytesDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(TopMeta {
                    version: version__.unwrap_or_default(),
                    raw: raw__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.config.TopMeta", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for TracerPredicateV1 {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.client_id.is_empty() {
            len += 1;
        }
        if !self.service.is_empty() {
            len += 1;
        }
        if !self.environment.is_empty() {
            len += 1;
        }
        if !self.app_version.is_empty() {
            len += 1;
        }
        if !self.tracer_version.is_empty() {
            len += 1;
        }
        if !self.language.is_empty() {
            len += 1;
        }
        if !self.runtime_id.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.config.TracerPredicateV1", len)?;
        if !self.client_id.is_empty() {
            struct_ser.serialize_field("clientID", &self.client_id)?;
        }
        if !self.service.is_empty() {
            struct_ser.serialize_field("service", &self.service)?;
        }
        if !self.environment.is_empty() {
            struct_ser.serialize_field("environment", &self.environment)?;
        }
        if !self.app_version.is_empty() {
            struct_ser.serialize_field("appVersion", &self.app_version)?;
        }
        if !self.tracer_version.is_empty() {
            struct_ser.serialize_field("tracerVersion", &self.tracer_version)?;
        }
        if !self.language.is_empty() {
            struct_ser.serialize_field("language", &self.language)?;
        }
        if !self.runtime_id.is_empty() {
            struct_ser.serialize_field("runtimeID", &self.runtime_id)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for TracerPredicateV1 {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "clientID",
            "service",
            "environment",
            "appVersion",
            "tracerVersion",
            "language",
            "runtimeID",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            ClientId,
            Service,
            Environment,
            AppVersion,
            TracerVersion,
            Language,
            RuntimeId,
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
                            "clientID" => Ok(GeneratedField::ClientId),
                            "service" => Ok(GeneratedField::Service),
                            "environment" => Ok(GeneratedField::Environment),
                            "appVersion" => Ok(GeneratedField::AppVersion),
                            "tracerVersion" => Ok(GeneratedField::TracerVersion),
                            "language" => Ok(GeneratedField::Language),
                            "runtimeID" => Ok(GeneratedField::RuntimeId),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = TracerPredicateV1;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.config.TracerPredicateV1")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<TracerPredicateV1, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut client_id__ = None;
                let mut service__ = None;
                let mut environment__ = None;
                let mut app_version__ = None;
                let mut tracer_version__ = None;
                let mut language__ = None;
                let mut runtime_id__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::ClientId => {
                            if client_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("clientID"));
                            }
                            client_id__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Service => {
                            if service__.is_some() {
                                return Err(serde::de::Error::duplicate_field("service"));
                            }
                            service__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Environment => {
                            if environment__.is_some() {
                                return Err(serde::de::Error::duplicate_field("environment"));
                            }
                            environment__ = Some(map_.next_value()?);
                        }
                        GeneratedField::AppVersion => {
                            if app_version__.is_some() {
                                return Err(serde::de::Error::duplicate_field("appVersion"));
                            }
                            app_version__ = Some(map_.next_value()?);
                        }
                        GeneratedField::TracerVersion => {
                            if tracer_version__.is_some() {
                                return Err(serde::de::Error::duplicate_field("tracerVersion"));
                            }
                            tracer_version__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Language => {
                            if language__.is_some() {
                                return Err(serde::de::Error::duplicate_field("language"));
                            }
                            language__ = Some(map_.next_value()?);
                        }
                        GeneratedField::RuntimeId => {
                            if runtime_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("runtimeID"));
                            }
                            runtime_id__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(TracerPredicateV1 {
                    client_id: client_id__.unwrap_or_default(),
                    service: service__.unwrap_or_default(),
                    environment: environment__.unwrap_or_default(),
                    app_version: app_version__.unwrap_or_default(),
                    tracer_version: tracer_version__.unwrap_or_default(),
                    language: language__.unwrap_or_default(),
                    runtime_id: runtime_id__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.config.TracerPredicateV1", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for TracerPredicates {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.tracer_predicates_v1.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.config.TracerPredicates", len)?;
        if !self.tracer_predicates_v1.is_empty() {
            struct_ser.serialize_field("tracerPredicatesV1", &self.tracer_predicates_v1)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for TracerPredicates {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "tracer_predicates_v1",
            "tracerPredicatesV1",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            TracerPredicatesV1,
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
                            "tracerPredicatesV1" | "tracer_predicates_v1" => Ok(GeneratedField::TracerPredicatesV1),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = TracerPredicates;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.config.TracerPredicates")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<TracerPredicates, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut tracer_predicates_v1__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::TracerPredicatesV1 => {
                            if tracer_predicates_v1__.is_some() {
                                return Err(serde::de::Error::duplicate_field("tracerPredicatesV1"));
                            }
                            tracer_predicates_v1__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(TracerPredicates {
                    tracer_predicates_v1: tracer_predicates_v1__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.config.TracerPredicates", FIELDS, GeneratedVisitor)
    }
}
