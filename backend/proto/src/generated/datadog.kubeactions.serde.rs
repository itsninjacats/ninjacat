impl serde::Serialize for DeletePodParams {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.grace_period_seconds.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.kubeactions.DeletePodParams", len)?;
        if let Some(v) = self.grace_period_seconds.as_ref() {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("gracePeriodSeconds", ToString::to_string(&v).as_str())?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for DeletePodParams {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "grace_period_seconds",
            "gracePeriodSeconds",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            GracePeriodSeconds,
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
                            "gracePeriodSeconds" | "grace_period_seconds" => Ok(GeneratedField::GracePeriodSeconds),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = DeletePodParams;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.kubeactions.DeletePodParams")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<DeletePodParams, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut grace_period_seconds__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::GracePeriodSeconds => {
                            if grace_period_seconds__.is_some() {
                                return Err(serde::de::Error::duplicate_field("gracePeriodSeconds"));
                            }
                            grace_period_seconds__ = 
                                map_.next_value::<::std::option::Option<::pbjson::private::NumberDeserialize<_>>>()?.map(|x| x.0)
                            ;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(DeletePodParams {
                    grace_period_seconds: grace_period_seconds__,
                })
            }
        }
        deserializer.deserialize_struct("datadog.kubeactions.DeletePodParams", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for GetResourceParams {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let len = 0;
        let struct_ser = serializer.serialize_struct("datadog.kubeactions.GetResourceParams", len)?;
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for GetResourceParams {
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
            type Value = GetResourceParams;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.kubeactions.GetResourceParams")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<GetResourceParams, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                while map_.next_key::<GeneratedField>()?.is_some() {
                    let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                }
                Ok(GetResourceParams {
                })
            }
        }
        deserializer.deserialize_struct("datadog.kubeactions.GetResourceParams", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for KubeAction {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.resource.is_some() {
            len += 1;
        }
        if self.timestamp.is_some() {
            len += 1;
        }
        if !self.action_id.is_empty() {
            len += 1;
        }
        if !self.requested_by.is_empty() {
            len += 1;
        }
        if self.action.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.kubeactions.KubeAction", len)?;
        if let Some(v) = self.resource.as_ref() {
            struct_ser.serialize_field("resource", v)?;
        }
        if let Some(v) = self.timestamp.as_ref() {
            struct_ser.serialize_field("timestamp", v)?;
        }
        if !self.action_id.is_empty() {
            struct_ser.serialize_field("actionId", &self.action_id)?;
        }
        if !self.requested_by.is_empty() {
            struct_ser.serialize_field("requestedBy", &self.requested_by)?;
        }
        if let Some(v) = self.action.as_ref() {
            match v {
                kube_action::Action::DeletePod(v) => {
                    struct_ser.serialize_field("deletePod", v)?;
                }
                kube_action::Action::RestartDeployment(v) => {
                    struct_ser.serialize_field("restartDeployment", v)?;
                }
                kube_action::Action::PatchDeployment(v) => {
                    struct_ser.serialize_field("patchDeployment", v)?;
                }
                kube_action::Action::GetResource(v) => {
                    struct_ser.serialize_field("getResource", v)?;
                }
                kube_action::Action::RollbackDeployment(v) => {
                    struct_ser.serialize_field("rollbackDeployment", v)?;
                }
                kube_action::Action::PatchDaemonset(v) => {
                    struct_ser.serialize_field("patchDaemonset", v)?;
                }
                kube_action::Action::PatchStatefulset(v) => {
                    struct_ser.serialize_field("patchStatefulset", v)?;
                }
            }
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for KubeAction {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "resource",
            "timestamp",
            "action_id",
            "actionId",
            "requested_by",
            "requestedBy",
            "delete_pod",
            "deletePod",
            "restart_deployment",
            "restartDeployment",
            "patch_deployment",
            "patchDeployment",
            "get_resource",
            "getResource",
            "rollback_deployment",
            "rollbackDeployment",
            "patch_daemonset",
            "patchDaemonset",
            "patch_statefulset",
            "patchStatefulset",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Resource,
            Timestamp,
            ActionId,
            RequestedBy,
            DeletePod,
            RestartDeployment,
            PatchDeployment,
            GetResource,
            RollbackDeployment,
            PatchDaemonset,
            PatchStatefulset,
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
                            "resource" => Ok(GeneratedField::Resource),
                            "timestamp" => Ok(GeneratedField::Timestamp),
                            "actionId" | "action_id" => Ok(GeneratedField::ActionId),
                            "requestedBy" | "requested_by" => Ok(GeneratedField::RequestedBy),
                            "deletePod" | "delete_pod" => Ok(GeneratedField::DeletePod),
                            "restartDeployment" | "restart_deployment" => Ok(GeneratedField::RestartDeployment),
                            "patchDeployment" | "patch_deployment" => Ok(GeneratedField::PatchDeployment),
                            "getResource" | "get_resource" => Ok(GeneratedField::GetResource),
                            "rollbackDeployment" | "rollback_deployment" => Ok(GeneratedField::RollbackDeployment),
                            "patchDaemonset" | "patch_daemonset" => Ok(GeneratedField::PatchDaemonset),
                            "patchStatefulset" | "patch_statefulset" => Ok(GeneratedField::PatchStatefulset),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = KubeAction;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.kubeactions.KubeAction")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<KubeAction, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut resource__ = None;
                let mut timestamp__ = None;
                let mut action_id__ = None;
                let mut requested_by__ = None;
                let mut action__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Resource => {
                            if resource__.is_some() {
                                return Err(serde::de::Error::duplicate_field("resource"));
                            }
                            resource__ = map_.next_value()?;
                        }
                        GeneratedField::Timestamp => {
                            if timestamp__.is_some() {
                                return Err(serde::de::Error::duplicate_field("timestamp"));
                            }
                            timestamp__ = map_.next_value()?;
                        }
                        GeneratedField::ActionId => {
                            if action_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("actionId"));
                            }
                            action_id__ = Some(map_.next_value()?);
                        }
                        GeneratedField::RequestedBy => {
                            if requested_by__.is_some() {
                                return Err(serde::de::Error::duplicate_field("requestedBy"));
                            }
                            requested_by__ = Some(map_.next_value()?);
                        }
                        GeneratedField::DeletePod => {
                            if action__.is_some() {
                                return Err(serde::de::Error::duplicate_field("deletePod"));
                            }
                            action__ = map_.next_value::<::std::option::Option<_>>()?.map(kube_action::Action::DeletePod)
;
                        }
                        GeneratedField::RestartDeployment => {
                            if action__.is_some() {
                                return Err(serde::de::Error::duplicate_field("restartDeployment"));
                            }
                            action__ = map_.next_value::<::std::option::Option<_>>()?.map(kube_action::Action::RestartDeployment)
;
                        }
                        GeneratedField::PatchDeployment => {
                            if action__.is_some() {
                                return Err(serde::de::Error::duplicate_field("patchDeployment"));
                            }
                            action__ = map_.next_value::<::std::option::Option<_>>()?.map(kube_action::Action::PatchDeployment)
;
                        }
                        GeneratedField::GetResource => {
                            if action__.is_some() {
                                return Err(serde::de::Error::duplicate_field("getResource"));
                            }
                            action__ = map_.next_value::<::std::option::Option<_>>()?.map(kube_action::Action::GetResource)
;
                        }
                        GeneratedField::RollbackDeployment => {
                            if action__.is_some() {
                                return Err(serde::de::Error::duplicate_field("rollbackDeployment"));
                            }
                            action__ = map_.next_value::<::std::option::Option<_>>()?.map(kube_action::Action::RollbackDeployment)
;
                        }
                        GeneratedField::PatchDaemonset => {
                            if action__.is_some() {
                                return Err(serde::de::Error::duplicate_field("patchDaemonset"));
                            }
                            action__ = map_.next_value::<::std::option::Option<_>>()?.map(kube_action::Action::PatchDaemonset)
;
                        }
                        GeneratedField::PatchStatefulset => {
                            if action__.is_some() {
                                return Err(serde::de::Error::duplicate_field("patchStatefulset"));
                            }
                            action__ = map_.next_value::<::std::option::Option<_>>()?.map(kube_action::Action::PatchStatefulset)
;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(KubeAction {
                    resource: resource__,
                    timestamp: timestamp__,
                    action_id: action_id__.unwrap_or_default(),
                    requested_by: requested_by__.unwrap_or_default(),
                    action: action__,
                })
            }
        }
        deserializer.deserialize_struct("datadog.kubeactions.KubeAction", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for KubeActionResult {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.action_id.is_empty() {
            len += 1;
        }
        if self.version != 0 {
            len += 1;
        }
        if !self.action_type.is_empty() {
            len += 1;
        }
        if !self.status.is_empty() {
            len += 1;
        }
        if !self.message.is_empty() {
            len += 1;
        }
        if self.executed_at.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.kubeactions.KubeActionResult", len)?;
        if !self.action_id.is_empty() {
            struct_ser.serialize_field("actionId", &self.action_id)?;
        }
        if self.version != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("version", ToString::to_string(&self.version).as_str())?;
        }
        if !self.action_type.is_empty() {
            struct_ser.serialize_field("actionType", &self.action_type)?;
        }
        if !self.status.is_empty() {
            struct_ser.serialize_field("status", &self.status)?;
        }
        if !self.message.is_empty() {
            struct_ser.serialize_field("message", &self.message)?;
        }
        if let Some(v) = self.executed_at.as_ref() {
            struct_ser.serialize_field("executedAt", v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for KubeActionResult {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "action_id",
            "actionId",
            "version",
            "action_type",
            "actionType",
            "status",
            "message",
            "executed_at",
            "executedAt",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            ActionId,
            Version,
            ActionType,
            Status,
            Message,
            ExecutedAt,
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
                            "actionId" | "action_id" => Ok(GeneratedField::ActionId),
                            "version" => Ok(GeneratedField::Version),
                            "actionType" | "action_type" => Ok(GeneratedField::ActionType),
                            "status" => Ok(GeneratedField::Status),
                            "message" => Ok(GeneratedField::Message),
                            "executedAt" | "executed_at" => Ok(GeneratedField::ExecutedAt),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = KubeActionResult;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.kubeactions.KubeActionResult")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<KubeActionResult, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut action_id__ = None;
                let mut version__ = None;
                let mut action_type__ = None;
                let mut status__ = None;
                let mut message__ = None;
                let mut executed_at__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::ActionId => {
                            if action_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("actionId"));
                            }
                            action_id__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Version => {
                            if version__.is_some() {
                                return Err(serde::de::Error::duplicate_field("version"));
                            }
                            version__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::ActionType => {
                            if action_type__.is_some() {
                                return Err(serde::de::Error::duplicate_field("actionType"));
                            }
                            action_type__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Status => {
                            if status__.is_some() {
                                return Err(serde::de::Error::duplicate_field("status"));
                            }
                            status__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Message => {
                            if message__.is_some() {
                                return Err(serde::de::Error::duplicate_field("message"));
                            }
                            message__ = Some(map_.next_value()?);
                        }
                        GeneratedField::ExecutedAt => {
                            if executed_at__.is_some() {
                                return Err(serde::de::Error::duplicate_field("executedAt"));
                            }
                            executed_at__ = map_.next_value()?;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(KubeActionResult {
                    action_id: action_id__.unwrap_or_default(),
                    version: version__.unwrap_or_default(),
                    action_type: action_type__.unwrap_or_default(),
                    status: status__.unwrap_or_default(),
                    message: message__.unwrap_or_default(),
                    executed_at: executed_at__,
                })
            }
        }
        deserializer.deserialize_struct("datadog.kubeactions.KubeActionResult", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for KubeActionsList {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.actions.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.kubeactions.KubeActionsList", len)?;
        if !self.actions.is_empty() {
            struct_ser.serialize_field("actions", &self.actions)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for KubeActionsList {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "actions",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Actions,
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
                            "actions" => Ok(GeneratedField::Actions),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = KubeActionsList;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.kubeactions.KubeActionsList")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<KubeActionsList, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut actions__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Actions => {
                            if actions__.is_some() {
                                return Err(serde::de::Error::duplicate_field("actions"));
                            }
                            actions__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(KubeActionsList {
                    actions: actions__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.kubeactions.KubeActionsList", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for KubeResource {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.api_version.is_empty() {
            len += 1;
        }
        if !self.kind.is_empty() {
            len += 1;
        }
        if !self.namespace.is_empty() {
            len += 1;
        }
        if !self.name.is_empty() {
            len += 1;
        }
        if !self.resource_id.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.kubeactions.KubeResource", len)?;
        if !self.api_version.is_empty() {
            struct_ser.serialize_field("apiVersion", &self.api_version)?;
        }
        if !self.kind.is_empty() {
            struct_ser.serialize_field("kind", &self.kind)?;
        }
        if !self.namespace.is_empty() {
            struct_ser.serialize_field("namespace", &self.namespace)?;
        }
        if !self.name.is_empty() {
            struct_ser.serialize_field("name", &self.name)?;
        }
        if !self.resource_id.is_empty() {
            struct_ser.serialize_field("resourceId", &self.resource_id)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for KubeResource {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "api_version",
            "apiVersion",
            "kind",
            "namespace",
            "name",
            "resource_id",
            "resourceId",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            ApiVersion,
            Kind,
            Namespace,
            Name,
            ResourceId,
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
                            "apiVersion" | "api_version" => Ok(GeneratedField::ApiVersion),
                            "kind" => Ok(GeneratedField::Kind),
                            "namespace" => Ok(GeneratedField::Namespace),
                            "name" => Ok(GeneratedField::Name),
                            "resourceId" | "resource_id" => Ok(GeneratedField::ResourceId),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = KubeResource;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.kubeactions.KubeResource")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<KubeResource, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut api_version__ = None;
                let mut kind__ = None;
                let mut namespace__ = None;
                let mut name__ = None;
                let mut resource_id__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::ApiVersion => {
                            if api_version__.is_some() {
                                return Err(serde::de::Error::duplicate_field("apiVersion"));
                            }
                            api_version__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Kind => {
                            if kind__.is_some() {
                                return Err(serde::de::Error::duplicate_field("kind"));
                            }
                            kind__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Namespace => {
                            if namespace__.is_some() {
                                return Err(serde::de::Error::duplicate_field("namespace"));
                            }
                            namespace__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Name => {
                            if name__.is_some() {
                                return Err(serde::de::Error::duplicate_field("name"));
                            }
                            name__ = Some(map_.next_value()?);
                        }
                        GeneratedField::ResourceId => {
                            if resource_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("resourceId"));
                            }
                            resource_id__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(KubeResource {
                    api_version: api_version__.unwrap_or_default(),
                    kind: kind__.unwrap_or_default(),
                    namespace: namespace__.unwrap_or_default(),
                    name: name__.unwrap_or_default(),
                    resource_id: resource_id__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.kubeactions.KubeResource", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for PatchDaemonSetParams {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.patch.is_some() {
            len += 1;
        }
        if !self.patch_strategy.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.kubeactions.PatchDaemonSetParams", len)?;
        if let Some(v) = self.patch.as_ref() {
            struct_ser.serialize_field("patch", v)?;
        }
        if !self.patch_strategy.is_empty() {
            struct_ser.serialize_field("patchStrategy", &self.patch_strategy)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for PatchDaemonSetParams {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "patch",
            "patch_strategy",
            "patchStrategy",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Patch,
            PatchStrategy,
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
                            "patch" => Ok(GeneratedField::Patch),
                            "patchStrategy" | "patch_strategy" => Ok(GeneratedField::PatchStrategy),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = PatchDaemonSetParams;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.kubeactions.PatchDaemonSetParams")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<PatchDaemonSetParams, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut patch__ = None;
                let mut patch_strategy__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Patch => {
                            if patch__.is_some() {
                                return Err(serde::de::Error::duplicate_field("patch"));
                            }
                            patch__ = map_.next_value()?;
                        }
                        GeneratedField::PatchStrategy => {
                            if patch_strategy__.is_some() {
                                return Err(serde::de::Error::duplicate_field("patchStrategy"));
                            }
                            patch_strategy__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(PatchDaemonSetParams {
                    patch: patch__,
                    patch_strategy: patch_strategy__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.kubeactions.PatchDaemonSetParams", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for PatchDeploymentParams {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.patch.is_some() {
            len += 1;
        }
        if !self.patch_strategy.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.kubeactions.PatchDeploymentParams", len)?;
        if let Some(v) = self.patch.as_ref() {
            struct_ser.serialize_field("patch", v)?;
        }
        if !self.patch_strategy.is_empty() {
            struct_ser.serialize_field("patchStrategy", &self.patch_strategy)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for PatchDeploymentParams {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "patch",
            "patch_strategy",
            "patchStrategy",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Patch,
            PatchStrategy,
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
                            "patch" => Ok(GeneratedField::Patch),
                            "patchStrategy" | "patch_strategy" => Ok(GeneratedField::PatchStrategy),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = PatchDeploymentParams;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.kubeactions.PatchDeploymentParams")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<PatchDeploymentParams, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut patch__ = None;
                let mut patch_strategy__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Patch => {
                            if patch__.is_some() {
                                return Err(serde::de::Error::duplicate_field("patch"));
                            }
                            patch__ = map_.next_value()?;
                        }
                        GeneratedField::PatchStrategy => {
                            if patch_strategy__.is_some() {
                                return Err(serde::de::Error::duplicate_field("patchStrategy"));
                            }
                            patch_strategy__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(PatchDeploymentParams {
                    patch: patch__,
                    patch_strategy: patch_strategy__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.kubeactions.PatchDeploymentParams", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for PatchStatefulSetParams {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.patch.is_some() {
            len += 1;
        }
        if !self.patch_strategy.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.kubeactions.PatchStatefulSetParams", len)?;
        if let Some(v) = self.patch.as_ref() {
            struct_ser.serialize_field("patch", v)?;
        }
        if !self.patch_strategy.is_empty() {
            struct_ser.serialize_field("patchStrategy", &self.patch_strategy)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for PatchStatefulSetParams {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "patch",
            "patch_strategy",
            "patchStrategy",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Patch,
            PatchStrategy,
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
                            "patch" => Ok(GeneratedField::Patch),
                            "patchStrategy" | "patch_strategy" => Ok(GeneratedField::PatchStrategy),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = PatchStatefulSetParams;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.kubeactions.PatchStatefulSetParams")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<PatchStatefulSetParams, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut patch__ = None;
                let mut patch_strategy__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Patch => {
                            if patch__.is_some() {
                                return Err(serde::de::Error::duplicate_field("patch"));
                            }
                            patch__ = map_.next_value()?;
                        }
                        GeneratedField::PatchStrategy => {
                            if patch_strategy__.is_some() {
                                return Err(serde::de::Error::duplicate_field("patchStrategy"));
                            }
                            patch_strategy__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(PatchStatefulSetParams {
                    patch: patch__,
                    patch_strategy: patch_strategy__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.kubeactions.PatchStatefulSetParams", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for RestartDeploymentParams {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let len = 0;
        let struct_ser = serializer.serialize_struct("datadog.kubeactions.RestartDeploymentParams", len)?;
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for RestartDeploymentParams {
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
            type Value = RestartDeploymentParams;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.kubeactions.RestartDeploymentParams")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<RestartDeploymentParams, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                while map_.next_key::<GeneratedField>()?.is_some() {
                    let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                }
                Ok(RestartDeploymentParams {
                })
            }
        }
        deserializer.deserialize_struct("datadog.kubeactions.RestartDeploymentParams", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for RollbackDeploymentParams {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.target_revision.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.kubeactions.RollbackDeploymentParams", len)?;
        if let Some(v) = self.target_revision.as_ref() {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("targetRevision", ToString::to_string(&v).as_str())?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for RollbackDeploymentParams {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "target_revision",
            "targetRevision",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            TargetRevision,
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
                            "targetRevision" | "target_revision" => Ok(GeneratedField::TargetRevision),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = RollbackDeploymentParams;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.kubeactions.RollbackDeploymentParams")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<RollbackDeploymentParams, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut target_revision__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::TargetRevision => {
                            if target_revision__.is_some() {
                                return Err(serde::de::Error::duplicate_field("targetRevision"));
                            }
                            target_revision__ = 
                                map_.next_value::<::std::option::Option<::pbjson::private::NumberDeserialize<_>>>()?.map(|x| x.0)
                            ;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(RollbackDeploymentParams {
                    target_revision: target_revision__,
                })
            }
        }
        deserializer.deserialize_struct("datadog.kubeactions.RollbackDeploymentParams", FIELDS, GeneratedVisitor)
    }
}
