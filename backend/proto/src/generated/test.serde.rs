impl serde::Serialize for DdSketch {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.mapping.is_some() {
            len += 1;
        }
        if self.positive_values.is_some() {
            len += 1;
        }
        if self.negative_values.is_some() {
            len += 1;
        }
        if self.zero_count != 0. {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("test.DDSketch", len)?;
        if let Some(v) = self.mapping.as_ref() {
            struct_ser.serialize_field("mapping", v)?;
        }
        if let Some(v) = self.positive_values.as_ref() {
            struct_ser.serialize_field("positiveValues", v)?;
        }
        if let Some(v) = self.negative_values.as_ref() {
            struct_ser.serialize_field("negativeValues", v)?;
        }
        if self.zero_count != 0. {
            struct_ser.serialize_field("zeroCount", &self.zero_count)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for DdSketch {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "mapping",
            "positiveValues",
            "negativeValues",
            "zeroCount",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Mapping,
            PositiveValues,
            NegativeValues,
            ZeroCount,
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
                            "mapping" => Ok(GeneratedField::Mapping),
                            "positiveValues" => Ok(GeneratedField::PositiveValues),
                            "negativeValues" => Ok(GeneratedField::NegativeValues),
                            "zeroCount" => Ok(GeneratedField::ZeroCount),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = DdSketch;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct test.DDSketch")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<DdSketch, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut mapping__ = None;
                let mut positive_values__ = None;
                let mut negative_values__ = None;
                let mut zero_count__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Mapping => {
                            if mapping__.is_some() {
                                return Err(serde::de::Error::duplicate_field("mapping"));
                            }
                            mapping__ = map_.next_value()?;
                        }
                        GeneratedField::PositiveValues => {
                            if positive_values__.is_some() {
                                return Err(serde::de::Error::duplicate_field("positiveValues"));
                            }
                            positive_values__ = map_.next_value()?;
                        }
                        GeneratedField::NegativeValues => {
                            if negative_values__.is_some() {
                                return Err(serde::de::Error::duplicate_field("negativeValues"));
                            }
                            negative_values__ = map_.next_value()?;
                        }
                        GeneratedField::ZeroCount => {
                            if zero_count__.is_some() {
                                return Err(serde::de::Error::duplicate_field("zeroCount"));
                            }
                            zero_count__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(DdSketch {
                    mapping: mapping__,
                    positive_values: positive_values__,
                    negative_values: negative_values__,
                    zero_count: zero_count__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("test.DDSketch", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for IndexMapping {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.gamma != 0. {
            len += 1;
        }
        if self.index_offset != 0. {
            len += 1;
        }
        if self.interpolation != 0 {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("test.IndexMapping", len)?;
        if self.gamma != 0. {
            struct_ser.serialize_field("gamma", &self.gamma)?;
        }
        if self.index_offset != 0. {
            struct_ser.serialize_field("indexOffset", &self.index_offset)?;
        }
        if self.interpolation != 0 {
            let v = index_mapping::Interpolation::try_from(self.interpolation)
                .map_err(|_| serde::ser::Error::custom(format!("Invalid variant {}", self.interpolation)))?;
            struct_ser.serialize_field("interpolation", &v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for IndexMapping {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "gamma",
            "indexOffset",
            "interpolation",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Gamma,
            IndexOffset,
            Interpolation,
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
                            "gamma" => Ok(GeneratedField::Gamma),
                            "indexOffset" => Ok(GeneratedField::IndexOffset),
                            "interpolation" => Ok(GeneratedField::Interpolation),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = IndexMapping;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct test.IndexMapping")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<IndexMapping, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut gamma__ = None;
                let mut index_offset__ = None;
                let mut interpolation__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Gamma => {
                            if gamma__.is_some() {
                                return Err(serde::de::Error::duplicate_field("gamma"));
                            }
                            gamma__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::IndexOffset => {
                            if index_offset__.is_some() {
                                return Err(serde::de::Error::duplicate_field("indexOffset"));
                            }
                            index_offset__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Interpolation => {
                            if interpolation__.is_some() {
                                return Err(serde::de::Error::duplicate_field("interpolation"));
                            }
                            interpolation__ = Some(map_.next_value::<index_mapping::Interpolation>()? as i32);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(IndexMapping {
                    gamma: gamma__.unwrap_or_default(),
                    index_offset: index_offset__.unwrap_or_default(),
                    interpolation: interpolation__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("test.IndexMapping", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for index_mapping::Interpolation {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let variant = match self {
            Self::None => "NONE",
            Self::Linear => "LINEAR",
            Self::Quadratic => "QUADRATIC",
            Self::Cubic => "CUBIC",
        };
        serializer.serialize_str(variant)
    }
}
impl<'de> serde::Deserialize<'de> for index_mapping::Interpolation {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "NONE",
            "LINEAR",
            "QUADRATIC",
            "CUBIC",
        ];

        struct GeneratedVisitor;

        impl serde::de::Visitor<'_> for GeneratedVisitor {
            type Value = index_mapping::Interpolation;

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
                    "NONE" => Ok(index_mapping::Interpolation::None),
                    "LINEAR" => Ok(index_mapping::Interpolation::Linear),
                    "QUADRATIC" => Ok(index_mapping::Interpolation::Quadratic),
                    "CUBIC" => Ok(index_mapping::Interpolation::Cubic),
                    _ => Err(serde::de::Error::unknown_variant(value, FIELDS)),
                }
            }
        }
        deserializer.deserialize_any(GeneratedVisitor)
    }
}
impl serde::Serialize for Store {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.bin_counts.is_empty() {
            len += 1;
        }
        if !self.contiguous_bin_counts.is_empty() {
            len += 1;
        }
        if self.contiguous_bin_index_offset != 0 {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("test.Store", len)?;
        if !self.bin_counts.is_empty() {
            struct_ser.serialize_field("binCounts", &self.bin_counts)?;
        }
        if !self.contiguous_bin_counts.is_empty() {
            struct_ser.serialize_field("contiguousBinCounts", &self.contiguous_bin_counts)?;
        }
        if self.contiguous_bin_index_offset != 0 {
            struct_ser.serialize_field("contiguousBinIndexOffset", &self.contiguous_bin_index_offset)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for Store {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "binCounts",
            "contiguousBinCounts",
            "contiguousBinIndexOffset",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            BinCounts,
            ContiguousBinCounts,
            ContiguousBinIndexOffset,
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
                            "binCounts" => Ok(GeneratedField::BinCounts),
                            "contiguousBinCounts" => Ok(GeneratedField::ContiguousBinCounts),
                            "contiguousBinIndexOffset" => Ok(GeneratedField::ContiguousBinIndexOffset),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = Store;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct test.Store")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<Store, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut bin_counts__ = None;
                let mut contiguous_bin_counts__ = None;
                let mut contiguous_bin_index_offset__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::BinCounts => {
                            if bin_counts__.is_some() {
                                return Err(serde::de::Error::duplicate_field("binCounts"));
                            }
                            bin_counts__ = Some(
                                map_.next_value::<std::collections::HashMap<::pbjson::private::NumberDeserialize<i32>, ::pbjson::private::NumberDeserialize<f64>>>()?
                                    .into_iter().map(|(k,v)| (k.0, v.0)).collect()
                            );
                        }
                        GeneratedField::ContiguousBinCounts => {
                            if contiguous_bin_counts__.is_some() {
                                return Err(serde::de::Error::duplicate_field("contiguousBinCounts"));
                            }
                            contiguous_bin_counts__ = 
                                Some(map_.next_value::<Vec<::pbjson::private::NumberDeserialize<_>>>()?
                                    .into_iter().map(|x| x.0).collect())
                            ;
                        }
                        GeneratedField::ContiguousBinIndexOffset => {
                            if contiguous_bin_index_offset__.is_some() {
                                return Err(serde::de::Error::duplicate_field("contiguousBinIndexOffset"));
                            }
                            contiguous_bin_index_offset__ = 
                                Some(map_.next_value::<::pbjson::private::NumberDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(Store {
                    bin_counts: bin_counts__.unwrap_or_default(),
                    contiguous_bin_counts: contiguous_bin_counts__.unwrap_or_default(),
                    contiguous_bin_index_offset: contiguous_bin_index_offset__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("test.Store", FIELDS, GeneratedVisitor)
    }
}
