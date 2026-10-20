impl serde::Serialize for EnumOptions {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.enums_as_constants {
            len += 1;
        }
        if self.enums_as_strings_only {
            len += 1;
        }
        if self.enums_trim_prefix {
            len += 1;
        }
        if self.ignore {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("protoc.gen.jsonschema.EnumOptions", len)?;
        if self.enums_as_constants {
            struct_ser.serialize_field("enumsAsConstants", &self.enums_as_constants)?;
        }
        if self.enums_as_strings_only {
            struct_ser.serialize_field("enumsAsStringsOnly", &self.enums_as_strings_only)?;
        }
        if self.enums_trim_prefix {
            struct_ser.serialize_field("enumsTrimPrefix", &self.enums_trim_prefix)?;
        }
        if self.ignore {
            struct_ser.serialize_field("ignore", &self.ignore)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for EnumOptions {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "enums_as_constants",
            "enumsAsConstants",
            "enums_as_strings_only",
            "enumsAsStringsOnly",
            "enums_trim_prefix",
            "enumsTrimPrefix",
            "ignore",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            EnumsAsConstants,
            EnumsAsStringsOnly,
            EnumsTrimPrefix,
            Ignore,
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
                            "enumsAsConstants" | "enums_as_constants" => Ok(GeneratedField::EnumsAsConstants),
                            "enumsAsStringsOnly" | "enums_as_strings_only" => Ok(GeneratedField::EnumsAsStringsOnly),
                            "enumsTrimPrefix" | "enums_trim_prefix" => Ok(GeneratedField::EnumsTrimPrefix),
                            "ignore" => Ok(GeneratedField::Ignore),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = EnumOptions;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct protoc.gen.jsonschema.EnumOptions")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<EnumOptions, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut enums_as_constants__ = None;
                let mut enums_as_strings_only__ = None;
                let mut enums_trim_prefix__ = None;
                let mut ignore__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::EnumsAsConstants => {
                            if enums_as_constants__.is_some() {
                                return Err(serde::de::Error::duplicate_field("enumsAsConstants"));
                            }
                            enums_as_constants__ = Some(map_.next_value()?);
                        }
                        GeneratedField::EnumsAsStringsOnly => {
                            if enums_as_strings_only__.is_some() {
                                return Err(serde::de::Error::duplicate_field("enumsAsStringsOnly"));
                            }
                            enums_as_strings_only__ = Some(map_.next_value()?);
                        }
                        GeneratedField::EnumsTrimPrefix => {
                            if enums_trim_prefix__.is_some() {
                                return Err(serde::de::Error::duplicate_field("enumsTrimPrefix"));
                            }
                            enums_trim_prefix__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Ignore => {
                            if ignore__.is_some() {
                                return Err(serde::de::Error::duplicate_field("ignore"));
                            }
                            ignore__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(EnumOptions {
                    enums_as_constants: enums_as_constants__.unwrap_or_default(),
                    enums_as_strings_only: enums_as_strings_only__.unwrap_or_default(),
                    enums_trim_prefix: enums_trim_prefix__.unwrap_or_default(),
                    ignore: ignore__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("protoc.gen.jsonschema.EnumOptions", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for FieldOptions {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.ignore {
            len += 1;
        }
        if self.required {
            len += 1;
        }
        if self.min_length != 0 {
            len += 1;
        }
        if self.max_length != 0 {
            len += 1;
        }
        if !self.pattern.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("protoc.gen.jsonschema.FieldOptions", len)?;
        if self.ignore {
            struct_ser.serialize_field("ignore", &self.ignore)?;
        }
        if self.required {
            struct_ser.serialize_field("required", &self.required)?;
        }
        if self.min_length != 0 {
            struct_ser.serialize_field("minLength", &self.min_length)?;
        }
        if self.max_length != 0 {
            struct_ser.serialize_field("maxLength", &self.max_length)?;
        }
        if !self.pattern.is_empty() {
            struct_ser.serialize_field("pattern", &self.pattern)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for FieldOptions {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "ignore",
            "required",
            "min_length",
            "minLength",
            "max_length",
            "maxLength",
            "pattern",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Ignore,
            Required,
            MinLength,
            MaxLength,
            Pattern,
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
                            "ignore" => Ok(GeneratedField::Ignore),
                            "required" => Ok(GeneratedField::Required),
                            "minLength" | "min_length" => Ok(GeneratedField::MinLength),
                            "maxLength" | "max_length" => Ok(GeneratedField::MaxLength),
                            "pattern" => Ok(GeneratedField::Pattern),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = FieldOptions;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct protoc.gen.jsonschema.FieldOptions")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<FieldOptions, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut ignore__ = None;
                let mut required__ = None;
                let mut min_length__ = None;
                let mut max_length__ = None;
                let mut pattern__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Ignore => {
                            if ignore__.is_some() {
                                return Err(serde::de::Error::duplicate_field("ignore"));
                            }
                            ignore__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Required => {
                            if required__.is_some() {
                                return Err(serde::de::Error::duplicate_field("required"));
                            }
                            required__ = Some(map_.next_value()?);
                        }
                        GeneratedField::MinLength => {
                            if min_length__.is_some() {
                                return Err(serde::de::Error::duplicate_field("minLength"));
                            }
                            min_length__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::MaxLength => {
                            if max_length__.is_some() {
                                return Err(serde::de::Error::duplicate_field("maxLength"));
                            }
                            max_length__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Pattern => {
                            if pattern__.is_some() {
                                return Err(serde::de::Error::duplicate_field("pattern"));
                            }
                            pattern__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(FieldOptions {
                    ignore: ignore__.unwrap_or_default(),
                    required: required__.unwrap_or_default(),
                    min_length: min_length__.unwrap_or_default(),
                    max_length: max_length__.unwrap_or_default(),
                    pattern: pattern__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("protoc.gen.jsonschema.FieldOptions", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for FileOptions {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.ignore {
            len += 1;
        }
        if !self.extension.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("protoc.gen.jsonschema.FileOptions", len)?;
        if self.ignore {
            struct_ser.serialize_field("ignore", &self.ignore)?;
        }
        if !self.extension.is_empty() {
            struct_ser.serialize_field("extension", &self.extension)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for FileOptions {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "ignore",
            "extension",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Ignore,
            Extension,
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
                            "ignore" => Ok(GeneratedField::Ignore),
                            "extension" => Ok(GeneratedField::Extension),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = FileOptions;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct protoc.gen.jsonschema.FileOptions")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<FileOptions, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut ignore__ = None;
                let mut extension__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Ignore => {
                            if ignore__.is_some() {
                                return Err(serde::de::Error::duplicate_field("ignore"));
                            }
                            ignore__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Extension => {
                            if extension__.is_some() {
                                return Err(serde::de::Error::duplicate_field("extension"));
                            }
                            extension__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(FileOptions {
                    ignore: ignore__.unwrap_or_default(),
                    extension: extension__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("protoc.gen.jsonschema.FileOptions", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for MessageOptions {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.ignore {
            len += 1;
        }
        if self.all_fields_required {
            len += 1;
        }
        if self.allow_null_values {
            len += 1;
        }
        if self.disallow_additional_properties {
            len += 1;
        }
        if self.enums_as_constants {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("protoc.gen.jsonschema.MessageOptions", len)?;
        if self.ignore {
            struct_ser.serialize_field("ignore", &self.ignore)?;
        }
        if self.all_fields_required {
            struct_ser.serialize_field("allFieldsRequired", &self.all_fields_required)?;
        }
        if self.allow_null_values {
            struct_ser.serialize_field("allowNullValues", &self.allow_null_values)?;
        }
        if self.disallow_additional_properties {
            struct_ser.serialize_field("disallowAdditionalProperties", &self.disallow_additional_properties)?;
        }
        if self.enums_as_constants {
            struct_ser.serialize_field("enumsAsConstants", &self.enums_as_constants)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for MessageOptions {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "ignore",
            "all_fields_required",
            "allFieldsRequired",
            "allow_null_values",
            "allowNullValues",
            "disallow_additional_properties",
            "disallowAdditionalProperties",
            "enums_as_constants",
            "enumsAsConstants",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Ignore,
            AllFieldsRequired,
            AllowNullValues,
            DisallowAdditionalProperties,
            EnumsAsConstants,
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
                            "ignore" => Ok(GeneratedField::Ignore),
                            "allFieldsRequired" | "all_fields_required" => Ok(GeneratedField::AllFieldsRequired),
                            "allowNullValues" | "allow_null_values" => Ok(GeneratedField::AllowNullValues),
                            "disallowAdditionalProperties" | "disallow_additional_properties" => Ok(GeneratedField::DisallowAdditionalProperties),
                            "enumsAsConstants" | "enums_as_constants" => Ok(GeneratedField::EnumsAsConstants),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = MessageOptions;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct protoc.gen.jsonschema.MessageOptions")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<MessageOptions, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut ignore__ = None;
                let mut all_fields_required__ = None;
                let mut allow_null_values__ = None;
                let mut disallow_additional_properties__ = None;
                let mut enums_as_constants__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Ignore => {
                            if ignore__.is_some() {
                                return Err(serde::de::Error::duplicate_field("ignore"));
                            }
                            ignore__ = Some(map_.next_value()?);
                        }
                        GeneratedField::AllFieldsRequired => {
                            if all_fields_required__.is_some() {
                                return Err(serde::de::Error::duplicate_field("allFieldsRequired"));
                            }
                            all_fields_required__ = Some(map_.next_value()?);
                        }
                        GeneratedField::AllowNullValues => {
                            if allow_null_values__.is_some() {
                                return Err(serde::de::Error::duplicate_field("allowNullValues"));
                            }
                            allow_null_values__ = Some(map_.next_value()?);
                        }
                        GeneratedField::DisallowAdditionalProperties => {
                            if disallow_additional_properties__.is_some() {
                                return Err(serde::de::Error::duplicate_field("disallowAdditionalProperties"));
                            }
                            disallow_additional_properties__ = Some(map_.next_value()?);
                        }
                        GeneratedField::EnumsAsConstants => {
                            if enums_as_constants__.is_some() {
                                return Err(serde::de::Error::duplicate_field("enumsAsConstants"));
                            }
                            enums_as_constants__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(MessageOptions {
                    ignore: ignore__.unwrap_or_default(),
                    all_fields_required: all_fields_required__.unwrap_or_default(),
                    allow_null_values: allow_null_values__.unwrap_or_default(),
                    disallow_additional_properties: disallow_additional_properties__.unwrap_or_default(),
                    enums_as_constants: enums_as_constants__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("protoc.gen.jsonschema.MessageOptions", FIELDS, GeneratedVisitor)
    }
}
