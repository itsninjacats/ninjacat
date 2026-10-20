impl serde::Serialize for ConditionValue {
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
        if !self.status.is_empty() {
            len += 1;
        }
        if self.reason.is_some() {
            len += 1;
        }
        if self.message.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.contlcycle.ConditionValue", len)?;
        if !self.r#type.is_empty() {
            struct_ser.serialize_field("type", &self.r#type)?;
        }
        if !self.status.is_empty() {
            struct_ser.serialize_field("status", &self.status)?;
        }
        if let Some(v) = self.reason.as_ref() {
            struct_ser.serialize_field("reason", v)?;
        }
        if let Some(v) = self.message.as_ref() {
            struct_ser.serialize_field("message", v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for ConditionValue {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "type",
            "status",
            "reason",
            "message",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Type,
            Status,
            Reason,
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
                            "type" => Ok(GeneratedField::Type),
                            "status" => Ok(GeneratedField::Status),
                            "reason" => Ok(GeneratedField::Reason),
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
            type Value = ConditionValue;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.contlcycle.ConditionValue")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<ConditionValue, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut r#type__ = None;
                let mut status__ = None;
                let mut reason__ = None;
                let mut message__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Type => {
                            if r#type__.is_some() {
                                return Err(serde::de::Error::duplicate_field("type"));
                            }
                            r#type__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Status => {
                            if status__.is_some() {
                                return Err(serde::de::Error::duplicate_field("status"));
                            }
                            status__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Reason => {
                            if reason__.is_some() {
                                return Err(serde::de::Error::duplicate_field("reason"));
                            }
                            reason__ = map_.next_value()?;
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
                Ok(ConditionValue {
                    r#type: r#type__.unwrap_or_default(),
                    status: status__.unwrap_or_default(),
                    reason: reason__,
                    message: message__,
                })
            }
        }
        deserializer.deserialize_struct("datadog.contlcycle.ConditionValue", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for ContainerEvent {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.container_id.is_empty() {
            len += 1;
        }
        if !self.source.is_empty() {
            len += 1;
        }
        if self.owner.is_some() {
            len += 1;
        }
        if self.container_name.is_some() {
            len += 1;
        }
        if self.transition.is_some() {
            len += 1;
        }
        if self.optional_exit_code.is_some() {
            len += 1;
        }
        if self.optional_exit_timestamp.is_some() {
            len += 1;
        }
        if self.optional_creation_timestamp.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.contlcycle.ContainerEvent", len)?;
        if !self.container_id.is_empty() {
            struct_ser.serialize_field("containerID", &self.container_id)?;
        }
        if !self.source.is_empty() {
            struct_ser.serialize_field("source", &self.source)?;
        }
        if let Some(v) = self.owner.as_ref() {
            struct_ser.serialize_field("owner", v)?;
        }
        if let Some(v) = self.container_name.as_ref() {
            struct_ser.serialize_field("containerName", v)?;
        }
        if let Some(v) = self.transition.as_ref() {
            struct_ser.serialize_field("transition", v)?;
        }
        if let Some(v) = self.optional_exit_code.as_ref() {
            match v {
                container_event::OptionalExitCode::ExitCode(v) => {
                    struct_ser.serialize_field("exitCode", v)?;
                }
            }
        }
        if let Some(v) = self.optional_exit_timestamp.as_ref() {
            match v {
                container_event::OptionalExitTimestamp::ExitTimestamp(v) => {
                    #[allow(clippy::needless_borrow)]
                    #[allow(clippy::needless_borrows_for_generic_args)]
                    struct_ser.serialize_field("exitTimestamp", ToString::to_string(&v).as_str())?;
                }
            }
        }
        if let Some(v) = self.optional_creation_timestamp.as_ref() {
            match v {
                container_event::OptionalCreationTimestamp::CreationTimestamp(v) => {
                    #[allow(clippy::needless_borrow)]
                    #[allow(clippy::needless_borrows_for_generic_args)]
                    struct_ser.serialize_field("creationTimestamp", ToString::to_string(&v).as_str())?;
                }
            }
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for ContainerEvent {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "containerID",
            "source",
            "owner",
            "containerName",
            "transition",
            "exitCode",
            "exitTimestamp",
            "creationTimestamp",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            ContainerId,
            Source,
            Owner,
            ContainerName,
            Transition,
            ExitCode,
            ExitTimestamp,
            CreationTimestamp,
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
                            "containerID" => Ok(GeneratedField::ContainerId),
                            "source" => Ok(GeneratedField::Source),
                            "owner" => Ok(GeneratedField::Owner),
                            "containerName" => Ok(GeneratedField::ContainerName),
                            "transition" => Ok(GeneratedField::Transition),
                            "exitCode" => Ok(GeneratedField::ExitCode),
                            "exitTimestamp" => Ok(GeneratedField::ExitTimestamp),
                            "creationTimestamp" => Ok(GeneratedField::CreationTimestamp),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = ContainerEvent;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.contlcycle.ContainerEvent")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<ContainerEvent, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut container_id__ = None;
                let mut source__ = None;
                let mut owner__ = None;
                let mut container_name__ = None;
                let mut transition__ = None;
                let mut optional_exit_code__ = None;
                let mut optional_exit_timestamp__ = None;
                let mut optional_creation_timestamp__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::ContainerId => {
                            if container_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("containerID"));
                            }
                            container_id__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Source => {
                            if source__.is_some() {
                                return Err(serde::de::Error::duplicate_field("source"));
                            }
                            source__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Owner => {
                            if owner__.is_some() {
                                return Err(serde::de::Error::duplicate_field("owner"));
                            }
                            owner__ = map_.next_value()?;
                        }
                        GeneratedField::ContainerName => {
                            if container_name__.is_some() {
                                return Err(serde::de::Error::duplicate_field("containerName"));
                            }
                            container_name__ = map_.next_value()?;
                        }
                        GeneratedField::Transition => {
                            if transition__.is_some() {
                                return Err(serde::de::Error::duplicate_field("transition"));
                            }
                            transition__ = map_.next_value()?;
                        }
                        GeneratedField::ExitCode => {
                            if optional_exit_code__.is_some() {
                                return Err(serde::de::Error::duplicate_field("exitCode"));
                            }
                            optional_exit_code__ = map_.next_value::<::std::option::Option<::pbjson::private::NumberDeserialize<_>>>()?.map(|x| container_event::OptionalExitCode::ExitCode(x.0));
                        }
                        GeneratedField::ExitTimestamp => {
                            if optional_exit_timestamp__.is_some() {
                                return Err(serde::de::Error::duplicate_field("exitTimestamp"));
                            }
                            optional_exit_timestamp__ = map_.next_value::<::std::option::Option<::pbjson::private::NumberDeserialize<_>>>()?.map(|x| container_event::OptionalExitTimestamp::ExitTimestamp(x.0));
                        }
                        GeneratedField::CreationTimestamp => {
                            if optional_creation_timestamp__.is_some() {
                                return Err(serde::de::Error::duplicate_field("creationTimestamp"));
                            }
                            optional_creation_timestamp__ = map_.next_value::<::std::option::Option<::pbjson::private::NumberDeserialize<_>>>()?.map(|x| container_event::OptionalCreationTimestamp::CreationTimestamp(x.0));
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(ContainerEvent {
                    container_id: container_id__.unwrap_or_default(),
                    source: source__.unwrap_or_default(),
                    owner: owner__,
                    container_name: container_name__,
                    transition: transition__,
                    optional_exit_code: optional_exit_code__,
                    optional_exit_timestamp: optional_exit_timestamp__,
                    optional_creation_timestamp: optional_creation_timestamp__,
                })
            }
        }
        deserializer.deserialize_struct("datadog.contlcycle.ContainerEvent", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for container_event::Owner {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.owner_type != 0 {
            len += 1;
        }
        if !self.owner_uid.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.contlcycle.ContainerEvent.Owner", len)?;
        if self.owner_type != 0 {
            let v = ObjectKind::try_from(self.owner_type)
                .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", self.owner_type)))?;
            struct_ser.serialize_field("ownerType", &v)?;
        }
        if !self.owner_uid.is_empty() {
            struct_ser.serialize_field("ownerUID", &self.owner_uid)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for container_event::Owner {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "ownerType",
            "ownerUID",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            OwnerType,
            OwnerUid,
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
                            "ownerType" => Ok(GeneratedField::OwnerType),
                            "ownerUID" => Ok(GeneratedField::OwnerUid),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = container_event::Owner;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.contlcycle.ContainerEvent.Owner")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<container_event::Owner, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut owner_type__ = None;
                let mut owner_uid__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::OwnerType => {
                            if owner_type__.is_some() {
                                return Err(serde::de::Error::duplicate_field("ownerType"));
                            }
                            owner_type__ = Some(map_.next_value::<ObjectKind>()? as i32);
                        }
                        GeneratedField::OwnerUid => {
                            if owner_uid__.is_some() {
                                return Err(serde::de::Error::duplicate_field("ownerUID"));
                            }
                            owner_uid__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(container_event::Owner {
                    owner_type: owner_type__.unwrap_or_default(),
                    owner_uid: owner_uid__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.contlcycle.ContainerEvent.Owner", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for ContainerKind {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let variant = match self {
            Self::Unspecified => "CONTAINER_KIND_UNSPECIFIED",
            Self::Regular => "CONTAINER_KIND_REGULAR",
            Self::Init => "CONTAINER_KIND_INIT",
            Self::Ephemeral => "CONTAINER_KIND_EPHEMERAL",
        };
        serializer.serialize_str(variant)
    }
}
impl<'de> serde::Deserialize<'de> for ContainerKind {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "CONTAINER_KIND_UNSPECIFIED",
            "CONTAINER_KIND_REGULAR",
            "CONTAINER_KIND_INIT",
            "CONTAINER_KIND_EPHEMERAL",
        ];

        struct GeneratedVisitor;

        impl serde::de::Visitor<'_> for GeneratedVisitor {
            type Value = ContainerKind;

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
                    "CONTAINER_KIND_UNSPECIFIED" => Ok(ContainerKind::Unspecified),
                    "CONTAINER_KIND_REGULAR" => Ok(ContainerKind::Regular),
                    "CONTAINER_KIND_INIT" => Ok(ContainerKind::Init),
                    "CONTAINER_KIND_EPHEMERAL" => Ok(ContainerKind::Ephemeral),
                    _ => Err(serde::de::Error::unknown_variant(value, FIELDS)),
                }
            }
        }
        deserializer.deserialize_any(GeneratedVisitor)
    }
}
impl serde::Serialize for ContainerStateKind {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let variant = match self {
            Self::Unspecified => "CONTAINER_STATE_KIND_UNSPECIFIED",
            Self::Waiting => "CONTAINER_STATE_KIND_WAITING",
            Self::Running => "CONTAINER_STATE_KIND_RUNNING",
            Self::Terminated => "CONTAINER_STATE_KIND_TERMINATED",
        };
        serializer.serialize_str(variant)
    }
}
impl<'de> serde::Deserialize<'de> for ContainerStateKind {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "CONTAINER_STATE_KIND_UNSPECIFIED",
            "CONTAINER_STATE_KIND_WAITING",
            "CONTAINER_STATE_KIND_RUNNING",
            "CONTAINER_STATE_KIND_TERMINATED",
        ];

        struct GeneratedVisitor;

        impl serde::de::Visitor<'_> for GeneratedVisitor {
            type Value = ContainerStateKind;

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
                    "CONTAINER_STATE_KIND_UNSPECIFIED" => Ok(ContainerStateKind::Unspecified),
                    "CONTAINER_STATE_KIND_WAITING" => Ok(ContainerStateKind::Waiting),
                    "CONTAINER_STATE_KIND_RUNNING" => Ok(ContainerStateKind::Running),
                    "CONTAINER_STATE_KIND_TERMINATED" => Ok(ContainerStateKind::Terminated),
                    _ => Err(serde::de::Error::unknown_variant(value, FIELDS)),
                }
            }
        }
        deserializer.deserialize_any(GeneratedVisitor)
    }
}
impl serde::Serialize for ContainerStateTransition {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.container_kind != 0 {
            len += 1;
        }
        if self.last_observed_state.is_some() {
            len += 1;
        }
        if self.new_state.is_some() {
            len += 1;
        }
        if self.transition_timestamp != 0 {
            len += 1;
        }
        if self.precision != 0 {
            len += 1;
        }
        if self.missed_intermediate != 0 {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.contlcycle.ContainerStateTransition", len)?;
        if self.container_kind != 0 {
            let v = ContainerKind::try_from(self.container_kind)
                .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", self.container_kind)))?;
            struct_ser.serialize_field("containerKind", &v)?;
        }
        if let Some(v) = self.last_observed_state.as_ref() {
            struct_ser.serialize_field("lastObservedState", v)?;
        }
        if let Some(v) = self.new_state.as_ref() {
            struct_ser.serialize_field("newState", v)?;
        }
        if self.transition_timestamp != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("transitionTimestamp", ToString::to_string(&self.transition_timestamp).as_str())?;
        }
        if self.precision != 0 {
            let v = Precision::try_from(self.precision)
                .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", self.precision)))?;
            struct_ser.serialize_field("precision", &v)?;
        }
        if self.missed_intermediate != 0 {
            let v = MissedIntermediate::try_from(self.missed_intermediate)
                .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", self.missed_intermediate)))?;
            struct_ser.serialize_field("missedIntermediate", &v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for ContainerStateTransition {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "containerKind",
            "lastObservedState",
            "newState",
            "transitionTimestamp",
            "precision",
            "missedIntermediate",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            ContainerKind,
            LastObservedState,
            NewState,
            TransitionTimestamp,
            Precision,
            MissedIntermediate,
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
                            "containerKind" => Ok(GeneratedField::ContainerKind),
                            "lastObservedState" => Ok(GeneratedField::LastObservedState),
                            "newState" => Ok(GeneratedField::NewState),
                            "transitionTimestamp" => Ok(GeneratedField::TransitionTimestamp),
                            "precision" => Ok(GeneratedField::Precision),
                            "missedIntermediate" => Ok(GeneratedField::MissedIntermediate),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = ContainerStateTransition;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.contlcycle.ContainerStateTransition")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<ContainerStateTransition, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut container_kind__ = None;
                let mut last_observed_state__ = None;
                let mut new_state__ = None;
                let mut transition_timestamp__ = None;
                let mut precision__ = None;
                let mut missed_intermediate__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::ContainerKind => {
                            if container_kind__.is_some() {
                                return Err(serde::de::Error::duplicate_field("containerKind"));
                            }
                            container_kind__ = Some(map_.next_value::<ContainerKind>()? as i32);
                        }
                        GeneratedField::LastObservedState => {
                            if last_observed_state__.is_some() {
                                return Err(serde::de::Error::duplicate_field("lastObservedState"));
                            }
                            last_observed_state__ = map_.next_value()?;
                        }
                        GeneratedField::NewState => {
                            if new_state__.is_some() {
                                return Err(serde::de::Error::duplicate_field("newState"));
                            }
                            new_state__ = map_.next_value()?;
                        }
                        GeneratedField::TransitionTimestamp => {
                            if transition_timestamp__.is_some() {
                                return Err(serde::de::Error::duplicate_field("transitionTimestamp"));
                            }
                            transition_timestamp__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Precision => {
                            if precision__.is_some() {
                                return Err(serde::de::Error::duplicate_field("precision"));
                            }
                            precision__ = Some(map_.next_value::<Precision>()? as i32);
                        }
                        GeneratedField::MissedIntermediate => {
                            if missed_intermediate__.is_some() {
                                return Err(serde::de::Error::duplicate_field("missedIntermediate"));
                            }
                            missed_intermediate__ = Some(map_.next_value::<MissedIntermediate>()? as i32);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(ContainerStateTransition {
                    container_kind: container_kind__.unwrap_or_default(),
                    last_observed_state: last_observed_state__,
                    new_state: new_state__,
                    transition_timestamp: transition_timestamp__.unwrap_or_default(),
                    precision: precision__.unwrap_or_default(),
                    missed_intermediate: missed_intermediate__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.contlcycle.ContainerStateTransition", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for ContainerStateValue {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.kind != 0 {
            len += 1;
        }
        if self.reason.is_some() {
            len += 1;
        }
        if self.exit_code.is_some() {
            len += 1;
        }
        if self.signal.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.contlcycle.ContainerStateValue", len)?;
        if self.kind != 0 {
            let v = ContainerStateKind::try_from(self.kind)
                .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", self.kind)))?;
            struct_ser.serialize_field("kind", &v)?;
        }
        if let Some(v) = self.reason.as_ref() {
            struct_ser.serialize_field("reason", v)?;
        }
        if let Some(v) = self.exit_code.as_ref() {
            struct_ser.serialize_field("exitCode", v)?;
        }
        if let Some(v) = self.signal.as_ref() {
            struct_ser.serialize_field("signal", v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for ContainerStateValue {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "kind",
            "reason",
            "exitCode",
            "signal",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Kind,
            Reason,
            ExitCode,
            Signal,
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
                            "kind" => Ok(GeneratedField::Kind),
                            "reason" => Ok(GeneratedField::Reason),
                            "exitCode" => Ok(GeneratedField::ExitCode),
                            "signal" => Ok(GeneratedField::Signal),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = ContainerStateValue;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.contlcycle.ContainerStateValue")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<ContainerStateValue, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut kind__ = None;
                let mut reason__ = None;
                let mut exit_code__ = None;
                let mut signal__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Kind => {
                            if kind__.is_some() {
                                return Err(serde::de::Error::duplicate_field("kind"));
                            }
                            kind__ = Some(map_.next_value::<ContainerStateKind>()? as i32);
                        }
                        GeneratedField::Reason => {
                            if reason__.is_some() {
                                return Err(serde::de::Error::duplicate_field("reason"));
                            }
                            reason__ = map_.next_value()?;
                        }
                        GeneratedField::ExitCode => {
                            if exit_code__.is_some() {
                                return Err(serde::de::Error::duplicate_field("exitCode"));
                            }
                            exit_code__ = 
                                map_.next_value::<::std::option::Option<::pbjson::private::NumberDeserialize<_>>>()?.map(|x| x.0)
                            ;
                        }
                        GeneratedField::Signal => {
                            if signal__.is_some() {
                                return Err(serde::de::Error::duplicate_field("signal"));
                            }
                            signal__ = 
                                map_.next_value::<::std::option::Option<::pbjson::private::NumberDeserialize<_>>>()?.map(|x| x.0)
                            ;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(ContainerStateValue {
                    kind: kind__.unwrap_or_default(),
                    reason: reason__,
                    exit_code: exit_code__,
                    signal: signal__,
                })
            }
        }
        deserializer.deserialize_struct("datadog.contlcycle.ContainerStateValue", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for Event {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.event_type != 0 {
            len += 1;
        }
        if self.typed_event.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.contlcycle.Event", len)?;
        if self.event_type != 0 {
            let v = event::EventType::try_from(self.event_type)
                .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", self.event_type)))?;
            struct_ser.serialize_field("eventType", &v)?;
        }
        if let Some(v) = self.typed_event.as_ref() {
            match v {
                event::TypedEvent::Container(v) => {
                    struct_ser.serialize_field("container", v)?;
                }
                event::TypedEvent::Pod(v) => {
                    struct_ser.serialize_field("pod", v)?;
                }
                event::TypedEvent::Task(v) => {
                    struct_ser.serialize_field("task", v)?;
                }
            }
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for Event {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "eventType",
            "container",
            "pod",
            "task",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            EventType,
            Container,
            Pod,
            Task,
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
                            "eventType" => Ok(GeneratedField::EventType),
                            "container" => Ok(GeneratedField::Container),
                            "pod" => Ok(GeneratedField::Pod),
                            "task" => Ok(GeneratedField::Task),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = Event;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.contlcycle.Event")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<Event, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut event_type__ = None;
                let mut typed_event__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::EventType => {
                            if event_type__.is_some() {
                                return Err(serde::de::Error::duplicate_field("eventType"));
                            }
                            event_type__ = Some(map_.next_value::<event::EventType>()? as i32);
                        }
                        GeneratedField::Container => {
                            if typed_event__.is_some() {
                                return Err(serde::de::Error::duplicate_field("container"));
                            }
                            typed_event__ = map_.next_value::<::std::option::Option<_>>()?.map(event::TypedEvent::Container)
;
                        }
                        GeneratedField::Pod => {
                            if typed_event__.is_some() {
                                return Err(serde::de::Error::duplicate_field("pod"));
                            }
                            typed_event__ = map_.next_value::<::std::option::Option<_>>()?.map(event::TypedEvent::Pod)
;
                        }
                        GeneratedField::Task => {
                            if typed_event__.is_some() {
                                return Err(serde::de::Error::duplicate_field("task"));
                            }
                            typed_event__ = map_.next_value::<::std::option::Option<_>>()?.map(event::TypedEvent::Task)
;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(Event {
                    event_type: event_type__.unwrap_or_default(),
                    typed_event: typed_event__,
                })
            }
        }
        deserializer.deserialize_struct("datadog.contlcycle.Event", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for event::EventType {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let variant = match self {
            Self::Delete => "Delete",
            Self::Transition => "Transition",
            Self::Create => "Create",
        };
        serializer.serialize_str(variant)
    }
}
impl<'de> serde::Deserialize<'de> for event::EventType {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "Delete",
            "Transition",
            "Create",
        ];

        struct GeneratedVisitor;

        impl serde::de::Visitor<'_> for GeneratedVisitor {
            type Value = event::EventType;

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
                    "Delete" => Ok(event::EventType::Delete),
                    "Transition" => Ok(event::EventType::Transition),
                    "Create" => Ok(event::EventType::Create),
                    _ => Err(serde::de::Error::unknown_variant(value, FIELDS)),
                }
            }
        }
        deserializer.deserialize_any(GeneratedVisitor)
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
        if !self.version.is_empty() {
            len += 1;
        }
        if !self.host.is_empty() {
            len += 1;
        }
        if self.object_kind != 0 {
            len += 1;
        }
        if !self.events.is_empty() {
            len += 1;
        }
        if !self.cluster_id.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.contlcycle.EventsPayload", len)?;
        if !self.version.is_empty() {
            struct_ser.serialize_field("version", &self.version)?;
        }
        if !self.host.is_empty() {
            struct_ser.serialize_field("host", &self.host)?;
        }
        if self.object_kind != 0 {
            let v = ObjectKind::try_from(self.object_kind)
                .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", self.object_kind)))?;
            struct_ser.serialize_field("objectKind", &v)?;
        }
        if !self.events.is_empty() {
            struct_ser.serialize_field("events", &self.events)?;
        }
        if !self.cluster_id.is_empty() {
            struct_ser.serialize_field("clusterId", &self.cluster_id)?;
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
            "version",
            "host",
            "objectKind",
            "events",
            "clusterId",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Version,
            Host,
            ObjectKind,
            Events,
            ClusterId,
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
                            "objectKind" => Ok(GeneratedField::ObjectKind),
                            "events" => Ok(GeneratedField::Events),
                            "clusterId" => Ok(GeneratedField::ClusterId),
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
                formatter.write_str("struct datadog.contlcycle.EventsPayload")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<EventsPayload, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut version__ = None;
                let mut host__ = None;
                let mut object_kind__ = None;
                let mut events__ = None;
                let mut cluster_id__ = None;
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
                        GeneratedField::ObjectKind => {
                            if object_kind__.is_some() {
                                return Err(serde::de::Error::duplicate_field("objectKind"));
                            }
                            object_kind__ = Some(map_.next_value::<ObjectKind>()? as i32);
                        }
                        GeneratedField::Events => {
                            if events__.is_some() {
                                return Err(serde::de::Error::duplicate_field("events"));
                            }
                            events__ = Some(map_.next_value()?);
                        }
                        GeneratedField::ClusterId => {
                            if cluster_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("clusterId"));
                            }
                            cluster_id__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(EventsPayload {
                    version: version__.unwrap_or_default(),
                    host: host__.unwrap_or_default(),
                    object_kind: object_kind__.unwrap_or_default(),
                    events: events__.unwrap_or_default(),
                    cluster_id: cluster_id__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.contlcycle.EventsPayload", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for MissedIntermediate {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let variant = match self {
            Self::Unknowable => "MISSED_INTERMEDIATE_UNKNOWABLE",
            Self::Proven => "MISSED_INTERMEDIATE_PROVEN",
        };
        serializer.serialize_str(variant)
    }
}
impl<'de> serde::Deserialize<'de> for MissedIntermediate {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "MISSED_INTERMEDIATE_UNKNOWABLE",
            "MISSED_INTERMEDIATE_PROVEN",
        ];

        struct GeneratedVisitor;

        impl serde::de::Visitor<'_> for GeneratedVisitor {
            type Value = MissedIntermediate;

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
                    "MISSED_INTERMEDIATE_UNKNOWABLE" => Ok(MissedIntermediate::Unknowable),
                    "MISSED_INTERMEDIATE_PROVEN" => Ok(MissedIntermediate::Proven),
                    _ => Err(serde::de::Error::unknown_variant(value, FIELDS)),
                }
            }
        }
        deserializer.deserialize_any(GeneratedVisitor)
    }
}
impl serde::Serialize for ObjectKind {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let variant = match self {
            Self::Container => "Container",
            Self::Pod => "Pod",
            Self::Task => "Task",
        };
        serializer.serialize_str(variant)
    }
}
impl<'de> serde::Deserialize<'de> for ObjectKind {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "Container",
            "Pod",
            "Task",
        ];

        struct GeneratedVisitor;

        impl serde::de::Visitor<'_> for GeneratedVisitor {
            type Value = ObjectKind;

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
                    "Container" => Ok(ObjectKind::Container),
                    "Pod" => Ok(ObjectKind::Pod),
                    "Task" => Ok(ObjectKind::Task),
                    _ => Err(serde::de::Error::unknown_variant(value, FIELDS)),
                }
            }
        }
        deserializer.deserialize_any(GeneratedVisitor)
    }
}
impl serde::Serialize for PodEvent {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.pod_uid.is_empty() {
            len += 1;
        }
        if !self.source.is_empty() {
            len += 1;
        }
        if self.exit_timestamp.is_some() {
            len += 1;
        }
        if self.creation_timestamp.is_some() {
            len += 1;
        }
        if self.transition.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.contlcycle.PodEvent", len)?;
        if !self.pod_uid.is_empty() {
            struct_ser.serialize_field("podUID", &self.pod_uid)?;
        }
        if !self.source.is_empty() {
            struct_ser.serialize_field("source", &self.source)?;
        }
        if let Some(v) = self.exit_timestamp.as_ref() {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("exitTimestamp", ToString::to_string(&v).as_str())?;
        }
        if let Some(v) = self.creation_timestamp.as_ref() {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("creationTimestamp", ToString::to_string(&v).as_str())?;
        }
        if let Some(v) = self.transition.as_ref() {
            struct_ser.serialize_field("transition", v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for PodEvent {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "podUID",
            "source",
            "exitTimestamp",
            "creationTimestamp",
            "transition",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            PodUid,
            Source,
            ExitTimestamp,
            CreationTimestamp,
            Transition,
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
                            "podUID" => Ok(GeneratedField::PodUid),
                            "source" => Ok(GeneratedField::Source),
                            "exitTimestamp" => Ok(GeneratedField::ExitTimestamp),
                            "creationTimestamp" => Ok(GeneratedField::CreationTimestamp),
                            "transition" => Ok(GeneratedField::Transition),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = PodEvent;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.contlcycle.PodEvent")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<PodEvent, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut pod_uid__ = None;
                let mut source__ = None;
                let mut exit_timestamp__ = None;
                let mut creation_timestamp__ = None;
                let mut transition__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::PodUid => {
                            if pod_uid__.is_some() {
                                return Err(serde::de::Error::duplicate_field("podUID"));
                            }
                            pod_uid__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Source => {
                            if source__.is_some() {
                                return Err(serde::de::Error::duplicate_field("source"));
                            }
                            source__ = Some(map_.next_value()?);
                        }
                        GeneratedField::ExitTimestamp => {
                            if exit_timestamp__.is_some() {
                                return Err(serde::de::Error::duplicate_field("exitTimestamp"));
                            }
                            exit_timestamp__ = 
                                map_.next_value::<::std::option::Option<::pbjson::private::NumberDeserialize<_>>>()?.map(|x| x.0)
                            ;
                        }
                        GeneratedField::CreationTimestamp => {
                            if creation_timestamp__.is_some() {
                                return Err(serde::de::Error::duplicate_field("creationTimestamp"));
                            }
                            creation_timestamp__ = 
                                map_.next_value::<::std::option::Option<::pbjson::private::NumberDeserialize<_>>>()?.map(|x| x.0)
                            ;
                        }
                        GeneratedField::Transition => {
                            if transition__.is_some() {
                                return Err(serde::de::Error::duplicate_field("transition"));
                            }
                            transition__ = map_.next_value()?;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(PodEvent {
                    pod_uid: pod_uid__.unwrap_or_default(),
                    source: source__.unwrap_or_default(),
                    exit_timestamp: exit_timestamp__,
                    creation_timestamp: creation_timestamp__,
                    transition: transition__,
                })
            }
        }
        deserializer.deserialize_struct("datadog.contlcycle.PodEvent", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for PodStateTransition {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.field != 0 {
            len += 1;
        }
        if self.last_observed_state.is_some() {
            len += 1;
        }
        if self.new_state.is_some() {
            len += 1;
        }
        if self.transition_timestamp != 0 {
            len += 1;
        }
        if self.precision != 0 {
            len += 1;
        }
        if self.missed_intermediate != 0 {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.contlcycle.PodStateTransition", len)?;
        if self.field != 0 {
            let v = PodStatusField::try_from(self.field)
                .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", self.field)))?;
            struct_ser.serialize_field("field", &v)?;
        }
        if let Some(v) = self.last_observed_state.as_ref() {
            struct_ser.serialize_field("lastObservedState", v)?;
        }
        if let Some(v) = self.new_state.as_ref() {
            struct_ser.serialize_field("newState", v)?;
        }
        if self.transition_timestamp != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("transitionTimestamp", ToString::to_string(&self.transition_timestamp).as_str())?;
        }
        if self.precision != 0 {
            let v = Precision::try_from(self.precision)
                .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", self.precision)))?;
            struct_ser.serialize_field("precision", &v)?;
        }
        if self.missed_intermediate != 0 {
            let v = MissedIntermediate::try_from(self.missed_intermediate)
                .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", self.missed_intermediate)))?;
            struct_ser.serialize_field("missedIntermediate", &v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for PodStateTransition {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "field",
            "lastObservedState",
            "newState",
            "transitionTimestamp",
            "precision",
            "missedIntermediate",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Field,
            LastObservedState,
            NewState,
            TransitionTimestamp,
            Precision,
            MissedIntermediate,
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
                            "field" => Ok(GeneratedField::Field),
                            "lastObservedState" => Ok(GeneratedField::LastObservedState),
                            "newState" => Ok(GeneratedField::NewState),
                            "transitionTimestamp" => Ok(GeneratedField::TransitionTimestamp),
                            "precision" => Ok(GeneratedField::Precision),
                            "missedIntermediate" => Ok(GeneratedField::MissedIntermediate),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = PodStateTransition;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.contlcycle.PodStateTransition")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<PodStateTransition, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut field__ = None;
                let mut last_observed_state__ = None;
                let mut new_state__ = None;
                let mut transition_timestamp__ = None;
                let mut precision__ = None;
                let mut missed_intermediate__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Field => {
                            if field__.is_some() {
                                return Err(serde::de::Error::duplicate_field("field"));
                            }
                            field__ = Some(map_.next_value::<PodStatusField>()? as i32);
                        }
                        GeneratedField::LastObservedState => {
                            if last_observed_state__.is_some() {
                                return Err(serde::de::Error::duplicate_field("lastObservedState"));
                            }
                            last_observed_state__ = map_.next_value()?;
                        }
                        GeneratedField::NewState => {
                            if new_state__.is_some() {
                                return Err(serde::de::Error::duplicate_field("newState"));
                            }
                            new_state__ = map_.next_value()?;
                        }
                        GeneratedField::TransitionTimestamp => {
                            if transition_timestamp__.is_some() {
                                return Err(serde::de::Error::duplicate_field("transitionTimestamp"));
                            }
                            transition_timestamp__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Precision => {
                            if precision__.is_some() {
                                return Err(serde::de::Error::duplicate_field("precision"));
                            }
                            precision__ = Some(map_.next_value::<Precision>()? as i32);
                        }
                        GeneratedField::MissedIntermediate => {
                            if missed_intermediate__.is_some() {
                                return Err(serde::de::Error::duplicate_field("missedIntermediate"));
                            }
                            missed_intermediate__ = Some(map_.next_value::<MissedIntermediate>()? as i32);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(PodStateTransition {
                    field: field__.unwrap_or_default(),
                    last_observed_state: last_observed_state__,
                    new_state: new_state__,
                    transition_timestamp: transition_timestamp__.unwrap_or_default(),
                    precision: precision__.unwrap_or_default(),
                    missed_intermediate: missed_intermediate__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadog.contlcycle.PodStateTransition", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for PodStatusField {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let variant = match self {
            Self::Unspecified => "POD_STATUS_FIELD_UNSPECIFIED",
            Self::Phase => "POD_STATUS_FIELD_PHASE",
            Self::Condition => "POD_STATUS_FIELD_CONDITION",
        };
        serializer.serialize_str(variant)
    }
}
impl<'de> serde::Deserialize<'de> for PodStatusField {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "POD_STATUS_FIELD_UNSPECIFIED",
            "POD_STATUS_FIELD_PHASE",
            "POD_STATUS_FIELD_CONDITION",
        ];

        struct GeneratedVisitor;

        impl serde::de::Visitor<'_> for GeneratedVisitor {
            type Value = PodStatusField;

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
                    "POD_STATUS_FIELD_UNSPECIFIED" => Ok(PodStatusField::Unspecified),
                    "POD_STATUS_FIELD_PHASE" => Ok(PodStatusField::Phase),
                    "POD_STATUS_FIELD_CONDITION" => Ok(PodStatusField::Condition),
                    _ => Err(serde::de::Error::unknown_variant(value, FIELDS)),
                }
            }
        }
        deserializer.deserialize_any(GeneratedVisitor)
    }
}
impl serde::Serialize for PodStatusValue {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.value.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.contlcycle.PodStatusValue", len)?;
        if let Some(v) = self.value.as_ref() {
            match v {
                pod_status_value::Value::Phase(v) => {
                    struct_ser.serialize_field("phase", v)?;
                }
                pod_status_value::Value::Condition(v) => {
                    struct_ser.serialize_field("condition", v)?;
                }
            }
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for PodStatusValue {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "phase",
            "condition",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Phase,
            Condition,
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
                            "phase" => Ok(GeneratedField::Phase),
                            "condition" => Ok(GeneratedField::Condition),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = PodStatusValue;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.contlcycle.PodStatusValue")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<PodStatusValue, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut value__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Phase => {
                            if value__.is_some() {
                                return Err(serde::de::Error::duplicate_field("phase"));
                            }
                            value__ = map_.next_value::<::std::option::Option<_>>()?.map(pod_status_value::Value::Phase);
                        }
                        GeneratedField::Condition => {
                            if value__.is_some() {
                                return Err(serde::de::Error::duplicate_field("condition"));
                            }
                            value__ = map_.next_value::<::std::option::Option<_>>()?.map(pod_status_value::Value::Condition)
;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(PodStatusValue {
                    value: value__,
                })
            }
        }
        deserializer.deserialize_struct("datadog.contlcycle.PodStatusValue", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for Precision {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let variant = match self {
            Self::Approximate => "PRECISION_APPROXIMATE",
            Self::Exact => "PRECISION_EXACT",
        };
        serializer.serialize_str(variant)
    }
}
impl<'de> serde::Deserialize<'de> for Precision {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "PRECISION_APPROXIMATE",
            "PRECISION_EXACT",
        ];

        struct GeneratedVisitor;

        impl serde::de::Visitor<'_> for GeneratedVisitor {
            type Value = Precision;

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
                    "PRECISION_APPROXIMATE" => Ok(Precision::Approximate),
                    "PRECISION_EXACT" => Ok(Precision::Exact),
                    _ => Err(serde::de::Error::unknown_variant(value, FIELDS)),
                }
            }
        }
        deserializer.deserialize_any(GeneratedVisitor)
    }
}
impl serde::Serialize for TaskEvent {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.task_arn.is_empty() {
            len += 1;
        }
        if !self.source.is_empty() {
            len += 1;
        }
        if self.exit_timestamp.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadog.contlcycle.TaskEvent", len)?;
        if !self.task_arn.is_empty() {
            struct_ser.serialize_field("taskARN", &self.task_arn)?;
        }
        if !self.source.is_empty() {
            struct_ser.serialize_field("source", &self.source)?;
        }
        if let Some(v) = self.exit_timestamp.as_ref() {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("exitTimestamp", ToString::to_string(&v).as_str())?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for TaskEvent {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "taskARN",
            "source",
            "exitTimestamp",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            TaskArn,
            Source,
            ExitTimestamp,
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
                            "taskARN" => Ok(GeneratedField::TaskArn),
                            "source" => Ok(GeneratedField::Source),
                            "exitTimestamp" => Ok(GeneratedField::ExitTimestamp),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = TaskEvent;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadog.contlcycle.TaskEvent")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<TaskEvent, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut task_arn__ = None;
                let mut source__ = None;
                let mut exit_timestamp__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::TaskArn => {
                            if task_arn__.is_some() {
                                return Err(serde::de::Error::duplicate_field("taskARN"));
                            }
                            task_arn__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Source => {
                            if source__.is_some() {
                                return Err(serde::de::Error::duplicate_field("source"));
                            }
                            source__ = Some(map_.next_value()?);
                        }
                        GeneratedField::ExitTimestamp => {
                            if exit_timestamp__.is_some() {
                                return Err(serde::de::Error::duplicate_field("exitTimestamp"));
                            }
                            exit_timestamp__ = 
                                map_.next_value::<::std::option::Option<::pbjson::private::NumberDeserialize<_>>>()?.map(|x| x.0)
                            ;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(TaskEvent {
                    task_arn: task_arn__.unwrap_or_default(),
                    source: source__.unwrap_or_default(),
                    exit_timestamp: exit_timestamp__,
                })
            }
        }
        deserializer.deserialize_struct("datadog.contlcycle.TaskEvent", FIELDS, GeneratedVisitor)
    }
}
