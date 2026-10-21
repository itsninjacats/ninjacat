impl serde::Serialize for Function {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.id != 0 {
            len += 1;
        }
        if self.name != 0 {
            len += 1;
        }
        if self.system_name != 0 {
            len += 1;
        }
        if self.filename != 0 {
            len += 1;
        }
        if self.start_line != 0 {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("perftools.profiles.Function", len)?;
        if self.id != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("id", ToString::to_string(&self.id).as_str())?;
        }
        if self.name != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("name", ToString::to_string(&self.name).as_str())?;
        }
        if self.system_name != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("systemName", ToString::to_string(&self.system_name).as_str())?;
        }
        if self.filename != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("filename", ToString::to_string(&self.filename).as_str())?;
        }
        if self.start_line != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("startLine", ToString::to_string(&self.start_line).as_str())?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for Function {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "id",
            "name",
            "system_name",
            "systemName",
            "filename",
            "start_line",
            "startLine",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Id,
            Name,
            SystemName,
            Filename,
            StartLine,
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
                            "systemName" | "system_name" => Ok(GeneratedField::SystemName),
                            "filename" => Ok(GeneratedField::Filename),
                            "startLine" | "start_line" => Ok(GeneratedField::StartLine),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = Function;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct perftools.profiles.Function")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<Function, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut id__ = None;
                let mut name__ = None;
                let mut system_name__ = None;
                let mut filename__ = None;
                let mut start_line__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Id => {
                            if id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("id"));
                            }
                            id__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Name => {
                            if name__.is_some() {
                                return Err(serde::de::Error::duplicate_field("name"));
                            }
                            name__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::SystemName => {
                            if system_name__.is_some() {
                                return Err(serde::de::Error::duplicate_field("systemName"));
                            }
                            system_name__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Filename => {
                            if filename__.is_some() {
                                return Err(serde::de::Error::duplicate_field("filename"));
                            }
                            filename__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::StartLine => {
                            if start_line__.is_some() {
                                return Err(serde::de::Error::duplicate_field("startLine"));
                            }
                            start_line__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(Function {
                    id: id__.unwrap_or_default(),
                    name: name__.unwrap_or_default(),
                    system_name: system_name__.unwrap_or_default(),
                    filename: filename__.unwrap_or_default(),
                    start_line: start_line__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("perftools.profiles.Function", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for Label {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.key != 0 {
            len += 1;
        }
        if self.str != 0 {
            len += 1;
        }
        if self.num != 0 {
            len += 1;
        }
        if self.num_unit != 0 {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("perftools.profiles.Label", len)?;
        if self.key != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("key", ToString::to_string(&self.key).as_str())?;
        }
        if self.str != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("str", ToString::to_string(&self.str).as_str())?;
        }
        if self.num != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("num", ToString::to_string(&self.num).as_str())?;
        }
        if self.num_unit != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("numUnit", ToString::to_string(&self.num_unit).as_str())?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for Label {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "key",
            "str",
            "num",
            "num_unit",
            "numUnit",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Key,
            Str,
            Num,
            NumUnit,
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
                            "key" => Ok(GeneratedField::Key),
                            "str" => Ok(GeneratedField::Str),
                            "num" => Ok(GeneratedField::Num),
                            "numUnit" | "num_unit" => Ok(GeneratedField::NumUnit),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = Label;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct perftools.profiles.Label")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<Label, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut key__ = None;
                let mut str__ = None;
                let mut num__ = None;
                let mut num_unit__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Key => {
                            if key__.is_some() {
                                return Err(serde::de::Error::duplicate_field("key"));
                            }
                            key__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Str => {
                            if str__.is_some() {
                                return Err(serde::de::Error::duplicate_field("str"));
                            }
                            str__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Num => {
                            if num__.is_some() {
                                return Err(serde::de::Error::duplicate_field("num"));
                            }
                            num__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::NumUnit => {
                            if num_unit__.is_some() {
                                return Err(serde::de::Error::duplicate_field("numUnit"));
                            }
                            num_unit__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(Label {
                    key: key__.unwrap_or_default(),
                    str: str__.unwrap_or_default(),
                    num: num__.unwrap_or_default(),
                    num_unit: num_unit__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("perftools.profiles.Label", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for Line {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.function_id != 0 {
            len += 1;
        }
        if self.line != 0 {
            len += 1;
        }
        if self.column != 0 {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("perftools.profiles.Line", len)?;
        if self.function_id != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("functionId", ToString::to_string(&self.function_id).as_str())?;
        }
        if self.line != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("line", ToString::to_string(&self.line).as_str())?;
        }
        if self.column != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("column", ToString::to_string(&self.column).as_str())?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for Line {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "function_id",
            "functionId",
            "line",
            "column",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            FunctionId,
            Line,
            Column,
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
                            "functionId" | "function_id" => Ok(GeneratedField::FunctionId),
                            "line" => Ok(GeneratedField::Line),
                            "column" => Ok(GeneratedField::Column),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = Line;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct perftools.profiles.Line")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<Line, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut function_id__ = None;
                let mut line__ = None;
                let mut column__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::FunctionId => {
                            if function_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("functionId"));
                            }
                            function_id__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Line => {
                            if line__.is_some() {
                                return Err(serde::de::Error::duplicate_field("line"));
                            }
                            line__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Column => {
                            if column__.is_some() {
                                return Err(serde::de::Error::duplicate_field("column"));
                            }
                            column__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(Line {
                    function_id: function_id__.unwrap_or_default(),
                    line: line__.unwrap_or_default(),
                    column: column__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("perftools.profiles.Line", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for Location {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.id != 0 {
            len += 1;
        }
        if self.mapping_id != 0 {
            len += 1;
        }
        if self.address != 0 {
            len += 1;
        }
        if !self.line.is_empty() {
            len += 1;
        }
        if self.is_folded {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("perftools.profiles.Location", len)?;
        if self.id != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("id", ToString::to_string(&self.id).as_str())?;
        }
        if self.mapping_id != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("mappingId", ToString::to_string(&self.mapping_id).as_str())?;
        }
        if self.address != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("address", ToString::to_string(&self.address).as_str())?;
        }
        if !self.line.is_empty() {
            struct_ser.serialize_field("line", &self.line)?;
        }
        if self.is_folded {
            struct_ser.serialize_field("isFolded", &self.is_folded)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for Location {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "id",
            "mapping_id",
            "mappingId",
            "address",
            "line",
            "is_folded",
            "isFolded",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Id,
            MappingId,
            Address,
            Line,
            IsFolded,
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
                            "mappingId" | "mapping_id" => Ok(GeneratedField::MappingId),
                            "address" => Ok(GeneratedField::Address),
                            "line" => Ok(GeneratedField::Line),
                            "isFolded" | "is_folded" => Ok(GeneratedField::IsFolded),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = Location;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct perftools.profiles.Location")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<Location, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut id__ = None;
                let mut mapping_id__ = None;
                let mut address__ = None;
                let mut line__ = None;
                let mut is_folded__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Id => {
                            if id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("id"));
                            }
                            id__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::MappingId => {
                            if mapping_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("mappingId"));
                            }
                            mapping_id__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Address => {
                            if address__.is_some() {
                                return Err(serde::de::Error::duplicate_field("address"));
                            }
                            address__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Line => {
                            if line__.is_some() {
                                return Err(serde::de::Error::duplicate_field("line"));
                            }
                            line__ = Some(map_.next_value()?);
                        }
                        GeneratedField::IsFolded => {
                            if is_folded__.is_some() {
                                return Err(serde::de::Error::duplicate_field("isFolded"));
                            }
                            is_folded__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(Location {
                    id: id__.unwrap_or_default(),
                    mapping_id: mapping_id__.unwrap_or_default(),
                    address: address__.unwrap_or_default(),
                    line: line__.unwrap_or_default(),
                    is_folded: is_folded__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("perftools.profiles.Location", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for Mapping {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.id != 0 {
            len += 1;
        }
        if self.memory_start != 0 {
            len += 1;
        }
        if self.memory_limit != 0 {
            len += 1;
        }
        if self.file_offset != 0 {
            len += 1;
        }
        if self.filename != 0 {
            len += 1;
        }
        if self.build_id != 0 {
            len += 1;
        }
        if self.has_functions {
            len += 1;
        }
        if self.has_filenames {
            len += 1;
        }
        if self.has_line_numbers {
            len += 1;
        }
        if self.has_inline_frames {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("perftools.profiles.Mapping", len)?;
        if self.id != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("id", ToString::to_string(&self.id).as_str())?;
        }
        if self.memory_start != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("memoryStart", ToString::to_string(&self.memory_start).as_str())?;
        }
        if self.memory_limit != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("memoryLimit", ToString::to_string(&self.memory_limit).as_str())?;
        }
        if self.file_offset != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("fileOffset", ToString::to_string(&self.file_offset).as_str())?;
        }
        if self.filename != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("filename", ToString::to_string(&self.filename).as_str())?;
        }
        if self.build_id != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("buildId", ToString::to_string(&self.build_id).as_str())?;
        }
        if self.has_functions {
            struct_ser.serialize_field("hasFunctions", &self.has_functions)?;
        }
        if self.has_filenames {
            struct_ser.serialize_field("hasFilenames", &self.has_filenames)?;
        }
        if self.has_line_numbers {
            struct_ser.serialize_field("hasLineNumbers", &self.has_line_numbers)?;
        }
        if self.has_inline_frames {
            struct_ser.serialize_field("hasInlineFrames", &self.has_inline_frames)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for Mapping {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "id",
            "memory_start",
            "memoryStart",
            "memory_limit",
            "memoryLimit",
            "file_offset",
            "fileOffset",
            "filename",
            "build_id",
            "buildId",
            "has_functions",
            "hasFunctions",
            "has_filenames",
            "hasFilenames",
            "has_line_numbers",
            "hasLineNumbers",
            "has_inline_frames",
            "hasInlineFrames",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Id,
            MemoryStart,
            MemoryLimit,
            FileOffset,
            Filename,
            BuildId,
            HasFunctions,
            HasFilenames,
            HasLineNumbers,
            HasInlineFrames,
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
                            "memoryStart" | "memory_start" => Ok(GeneratedField::MemoryStart),
                            "memoryLimit" | "memory_limit" => Ok(GeneratedField::MemoryLimit),
                            "fileOffset" | "file_offset" => Ok(GeneratedField::FileOffset),
                            "filename" => Ok(GeneratedField::Filename),
                            "buildId" | "build_id" => Ok(GeneratedField::BuildId),
                            "hasFunctions" | "has_functions" => Ok(GeneratedField::HasFunctions),
                            "hasFilenames" | "has_filenames" => Ok(GeneratedField::HasFilenames),
                            "hasLineNumbers" | "has_line_numbers" => Ok(GeneratedField::HasLineNumbers),
                            "hasInlineFrames" | "has_inline_frames" => Ok(GeneratedField::HasInlineFrames),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = Mapping;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct perftools.profiles.Mapping")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<Mapping, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut id__ = None;
                let mut memory_start__ = None;
                let mut memory_limit__ = None;
                let mut file_offset__ = None;
                let mut filename__ = None;
                let mut build_id__ = None;
                let mut has_functions__ = None;
                let mut has_filenames__ = None;
                let mut has_line_numbers__ = None;
                let mut has_inline_frames__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Id => {
                            if id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("id"));
                            }
                            id__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::MemoryStart => {
                            if memory_start__.is_some() {
                                return Err(serde::de::Error::duplicate_field("memoryStart"));
                            }
                            memory_start__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::MemoryLimit => {
                            if memory_limit__.is_some() {
                                return Err(serde::de::Error::duplicate_field("memoryLimit"));
                            }
                            memory_limit__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::FileOffset => {
                            if file_offset__.is_some() {
                                return Err(serde::de::Error::duplicate_field("fileOffset"));
                            }
                            file_offset__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Filename => {
                            if filename__.is_some() {
                                return Err(serde::de::Error::duplicate_field("filename"));
                            }
                            filename__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::BuildId => {
                            if build_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("buildId"));
                            }
                            build_id__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::HasFunctions => {
                            if has_functions__.is_some() {
                                return Err(serde::de::Error::duplicate_field("hasFunctions"));
                            }
                            has_functions__ = Some(map_.next_value()?);
                        }
                        GeneratedField::HasFilenames => {
                            if has_filenames__.is_some() {
                                return Err(serde::de::Error::duplicate_field("hasFilenames"));
                            }
                            has_filenames__ = Some(map_.next_value()?);
                        }
                        GeneratedField::HasLineNumbers => {
                            if has_line_numbers__.is_some() {
                                return Err(serde::de::Error::duplicate_field("hasLineNumbers"));
                            }
                            has_line_numbers__ = Some(map_.next_value()?);
                        }
                        GeneratedField::HasInlineFrames => {
                            if has_inline_frames__.is_some() {
                                return Err(serde::de::Error::duplicate_field("hasInlineFrames"));
                            }
                            has_inline_frames__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(Mapping {
                    id: id__.unwrap_or_default(),
                    memory_start: memory_start__.unwrap_or_default(),
                    memory_limit: memory_limit__.unwrap_or_default(),
                    file_offset: file_offset__.unwrap_or_default(),
                    filename: filename__.unwrap_or_default(),
                    build_id: build_id__.unwrap_or_default(),
                    has_functions: has_functions__.unwrap_or_default(),
                    has_filenames: has_filenames__.unwrap_or_default(),
                    has_line_numbers: has_line_numbers__.unwrap_or_default(),
                    has_inline_frames: has_inline_frames__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("perftools.profiles.Mapping", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for Profile {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.sample_type.is_empty() {
            len += 1;
        }
        if !self.sample.is_empty() {
            len += 1;
        }
        if !self.mapping.is_empty() {
            len += 1;
        }
        if !self.location.is_empty() {
            len += 1;
        }
        if !self.function.is_empty() {
            len += 1;
        }
        if !self.string_table.is_empty() {
            len += 1;
        }
        if self.drop_frames != 0 {
            len += 1;
        }
        if self.keep_frames != 0 {
            len += 1;
        }
        if self.time_nanos != 0 {
            len += 1;
        }
        if self.duration_nanos != 0 {
            len += 1;
        }
        if self.period_type.is_some() {
            len += 1;
        }
        if self.period != 0 {
            len += 1;
        }
        if !self.comment.is_empty() {
            len += 1;
        }
        if self.default_sample_type != 0 {
            len += 1;
        }
        if self.doc_url != 0 {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("perftools.profiles.Profile", len)?;
        if !self.sample_type.is_empty() {
            struct_ser.serialize_field("sampleType", &self.sample_type)?;
        }
        if !self.sample.is_empty() {
            struct_ser.serialize_field("sample", &self.sample)?;
        }
        if !self.mapping.is_empty() {
            struct_ser.serialize_field("mapping", &self.mapping)?;
        }
        if !self.location.is_empty() {
            struct_ser.serialize_field("location", &self.location)?;
        }
        if !self.function.is_empty() {
            struct_ser.serialize_field("function", &self.function)?;
        }
        if !self.string_table.is_empty() {
            struct_ser.serialize_field("stringTable", &self.string_table)?;
        }
        if self.drop_frames != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("dropFrames", ToString::to_string(&self.drop_frames).as_str())?;
        }
        if self.keep_frames != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("keepFrames", ToString::to_string(&self.keep_frames).as_str())?;
        }
        if self.time_nanos != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("timeNanos", ToString::to_string(&self.time_nanos).as_str())?;
        }
        if self.duration_nanos != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("durationNanos", ToString::to_string(&self.duration_nanos).as_str())?;
        }
        if let Some(v) = self.period_type.as_ref() {
            struct_ser.serialize_field("periodType", v)?;
        }
        if self.period != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("period", ToString::to_string(&self.period).as_str())?;
        }
        if !self.comment.is_empty() {
            struct_ser.serialize_field("comment", &self.comment.iter().map(ToString::to_string).collect::<Vec<_>>())?;
        }
        if self.default_sample_type != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("defaultSampleType", ToString::to_string(&self.default_sample_type).as_str())?;
        }
        if self.doc_url != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("docUrl", ToString::to_string(&self.doc_url).as_str())?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for Profile {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "sample_type",
            "sampleType",
            "sample",
            "mapping",
            "location",
            "function",
            "string_table",
            "stringTable",
            "drop_frames",
            "dropFrames",
            "keep_frames",
            "keepFrames",
            "time_nanos",
            "timeNanos",
            "duration_nanos",
            "durationNanos",
            "period_type",
            "periodType",
            "period",
            "comment",
            "default_sample_type",
            "defaultSampleType",
            "doc_url",
            "docUrl",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            SampleType,
            Sample,
            Mapping,
            Location,
            Function,
            StringTable,
            DropFrames,
            KeepFrames,
            TimeNanos,
            DurationNanos,
            PeriodType,
            Period,
            Comment,
            DefaultSampleType,
            DocUrl,
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
                            "sampleType" | "sample_type" => Ok(GeneratedField::SampleType),
                            "sample" => Ok(GeneratedField::Sample),
                            "mapping" => Ok(GeneratedField::Mapping),
                            "location" => Ok(GeneratedField::Location),
                            "function" => Ok(GeneratedField::Function),
                            "stringTable" | "string_table" => Ok(GeneratedField::StringTable),
                            "dropFrames" | "drop_frames" => Ok(GeneratedField::DropFrames),
                            "keepFrames" | "keep_frames" => Ok(GeneratedField::KeepFrames),
                            "timeNanos" | "time_nanos" => Ok(GeneratedField::TimeNanos),
                            "durationNanos" | "duration_nanos" => Ok(GeneratedField::DurationNanos),
                            "periodType" | "period_type" => Ok(GeneratedField::PeriodType),
                            "period" => Ok(GeneratedField::Period),
                            "comment" => Ok(GeneratedField::Comment),
                            "defaultSampleType" | "default_sample_type" => Ok(GeneratedField::DefaultSampleType),
                            "docUrl" | "doc_url" => Ok(GeneratedField::DocUrl),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = Profile;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct perftools.profiles.Profile")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<Profile, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut sample_type__ = None;
                let mut sample__ = None;
                let mut mapping__ = None;
                let mut location__ = None;
                let mut function__ = None;
                let mut string_table__ = None;
                let mut drop_frames__ = None;
                let mut keep_frames__ = None;
                let mut time_nanos__ = None;
                let mut duration_nanos__ = None;
                let mut period_type__ = None;
                let mut period__ = None;
                let mut comment__ = None;
                let mut default_sample_type__ = None;
                let mut doc_url__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::SampleType => {
                            if sample_type__.is_some() {
                                return Err(serde::de::Error::duplicate_field("sampleType"));
                            }
                            sample_type__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Sample => {
                            if sample__.is_some() {
                                return Err(serde::de::Error::duplicate_field("sample"));
                            }
                            sample__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Mapping => {
                            if mapping__.is_some() {
                                return Err(serde::de::Error::duplicate_field("mapping"));
                            }
                            mapping__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Location => {
                            if location__.is_some() {
                                return Err(serde::de::Error::duplicate_field("location"));
                            }
                            location__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Function => {
                            if function__.is_some() {
                                return Err(serde::de::Error::duplicate_field("function"));
                            }
                            function__ = Some(map_.next_value()?);
                        }
                        GeneratedField::StringTable => {
                            if string_table__.is_some() {
                                return Err(serde::de::Error::duplicate_field("stringTable"));
                            }
                            string_table__ = Some(map_.next_value()?);
                        }
                        GeneratedField::DropFrames => {
                            if drop_frames__.is_some() {
                                return Err(serde::de::Error::duplicate_field("dropFrames"));
                            }
                            drop_frames__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::KeepFrames => {
                            if keep_frames__.is_some() {
                                return Err(serde::de::Error::duplicate_field("keepFrames"));
                            }
                            keep_frames__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::TimeNanos => {
                            if time_nanos__.is_some() {
                                return Err(serde::de::Error::duplicate_field("timeNanos"));
                            }
                            time_nanos__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::DurationNanos => {
                            if duration_nanos__.is_some() {
                                return Err(serde::de::Error::duplicate_field("durationNanos"));
                            }
                            duration_nanos__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::PeriodType => {
                            if period_type__.is_some() {
                                return Err(serde::de::Error::duplicate_field("periodType"));
                            }
                            period_type__ = map_.next_value()?;
                        }
                        GeneratedField::Period => {
                            if period__.is_some() {
                                return Err(serde::de::Error::duplicate_field("period"));
                            }
                            period__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Comment => {
                            if comment__.is_some() {
                                return Err(serde::de::Error::duplicate_field("comment"));
                            }
                            comment__ = 
                                Some(map_.next_value::<Vec<::pbjson::private::NumberDeserialize<_>>>()?
                                    .into_iter().map(|x| x.0).collect())
                            ;
                        }
                        GeneratedField::DefaultSampleType => {
                            if default_sample_type__.is_some() {
                                return Err(serde::de::Error::duplicate_field("defaultSampleType"));
                            }
                            default_sample_type__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::DocUrl => {
                            if doc_url__.is_some() {
                                return Err(serde::de::Error::duplicate_field("docUrl"));
                            }
                            doc_url__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(Profile {
                    sample_type: sample_type__.unwrap_or_default(),
                    sample: sample__.unwrap_or_default(),
                    mapping: mapping__.unwrap_or_default(),
                    location: location__.unwrap_or_default(),
                    function: function__.unwrap_or_default(),
                    string_table: string_table__.unwrap_or_default(),
                    drop_frames: drop_frames__.unwrap_or_default(),
                    keep_frames: keep_frames__.unwrap_or_default(),
                    time_nanos: time_nanos__.unwrap_or_default(),
                    duration_nanos: duration_nanos__.unwrap_or_default(),
                    period_type: period_type__,
                    period: period__.unwrap_or_default(),
                    comment: comment__.unwrap_or_default(),
                    default_sample_type: default_sample_type__.unwrap_or_default(),
                    doc_url: doc_url__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("perftools.profiles.Profile", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for Sample {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.location_id.is_empty() {
            len += 1;
        }
        if !self.value.is_empty() {
            len += 1;
        }
        if !self.label.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("perftools.profiles.Sample", len)?;
        if !self.location_id.is_empty() {
            struct_ser.serialize_field("locationId", &self.location_id.iter().map(ToString::to_string).collect::<Vec<_>>())?;
        }
        if !self.value.is_empty() {
            struct_ser.serialize_field("value", &self.value.iter().map(ToString::to_string).collect::<Vec<_>>())?;
        }
        if !self.label.is_empty() {
            struct_ser.serialize_field("label", &self.label)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for Sample {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "location_id",
            "locationId",
            "value",
            "label",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            LocationId,
            Value,
            Label,
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
                            "locationId" | "location_id" => Ok(GeneratedField::LocationId),
                            "value" => Ok(GeneratedField::Value),
                            "label" => Ok(GeneratedField::Label),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = Sample;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct perftools.profiles.Sample")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<Sample, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut location_id__ = None;
                let mut value__ = None;
                let mut label__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::LocationId => {
                            if location_id__.is_some() {
                                return Err(serde::de::Error::duplicate_field("locationId"));
                            }
                            location_id__ = 
                                Some(map_.next_value::<Vec<::pbjson::private::NumberDeserialize<_>>>()?
                                    .into_iter().map(|x| x.0).collect())
                            ;
                        }
                        GeneratedField::Value => {
                            if value__.is_some() {
                                return Err(serde::de::Error::duplicate_field("value"));
                            }
                            value__ = 
                                Some(map_.next_value::<Vec<::pbjson::private::NumberDeserialize<_>>>()?
                                    .into_iter().map(|x| x.0).collect())
                            ;
                        }
                        GeneratedField::Label => {
                            if label__.is_some() {
                                return Err(serde::de::Error::duplicate_field("label"));
                            }
                            label__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(Sample {
                    location_id: location_id__.unwrap_or_default(),
                    value: value__.unwrap_or_default(),
                    label: label__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("perftools.profiles.Sample", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for ValueType {
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
        if self.unit != 0 {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("perftools.profiles.ValueType", len)?;
        if self.r#type != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("type", ToString::to_string(&self.r#type).as_str())?;
        }
        if self.unit != 0 {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("unit", ToString::to_string(&self.unit).as_str())?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for ValueType {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "type",
            "unit",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Type,
            Unit,
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
                            "unit" => Ok(GeneratedField::Unit),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = ValueType;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct perftools.profiles.ValueType")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<ValueType, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut r#type__ = None;
                let mut unit__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Type => {
                            if r#type__.is_some() {
                                return Err(serde::de::Error::duplicate_field("type"));
                            }
                            r#type__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Unit => {
                            if unit__.is_some() {
                                return Err(serde::de::Error::duplicate_field("unit"));
                            }
                            unit__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(ValueType {
                    r#type: r#type__.unwrap_or_default(),
                    unit: unit__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("perftools.profiles.ValueType", FIELDS, GeneratedVisitor)
    }
}
