impl serde::Serialize for Metadata {
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
        if !self.resources.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadoghq.api.metrics.v3.Metadata", len)?;
        if !self.tags.is_empty() {
            struct_ser.serialize_field("tags", &self.tags)?;
        }
        if !self.resources.is_empty() {
            struct_ser.serialize_field("resources", &self.resources)?;
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
            "tags",
            "resources",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Tags,
            Resources,
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
                            "resources" => Ok(GeneratedField::Resources),
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
                formatter.write_str("struct datadoghq.api.metrics.v3.Metadata")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<Metadata, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut tags__ = None;
                let mut resources__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Tags => {
                            if tags__.is_some() {
                                return Err(serde::de::Error::duplicate_field("tags"));
                            }
                            tags__ = Some(map_.next_value()?);
                        }
                        GeneratedField::Resources => {
                            if resources__.is_some() {
                                return Err(serde::de::Error::duplicate_field("resources"));
                            }
                            resources__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(Metadata {
                    tags: tags__.unwrap_or_default(),
                    resources: resources__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadoghq.api.metrics.v3.Metadata", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for MetricData {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.dict_name_str.is_empty() {
            len += 1;
        }
        if !self.dict_tag_str.is_empty() {
            len += 1;
        }
        if !self.dict_tagsets.is_empty() {
            len += 1;
        }
        if !self.dict_resource_str.is_empty() {
            len += 1;
        }
        if !self.dict_resource_len.is_empty() {
            len += 1;
        }
        if !self.dict_resource_type.is_empty() {
            len += 1;
        }
        if !self.dict_resource_name.is_empty() {
            len += 1;
        }
        if !self.dict_source_type_name.is_empty() {
            len += 1;
        }
        if !self.dict_origin_info.is_empty() {
            len += 1;
        }
        if !self.dict_unit_str.is_empty() {
            len += 1;
        }
        if !self.types.is_empty() {
            len += 1;
        }
        if !self.name_refs.is_empty() {
            len += 1;
        }
        if !self.tagset_refs.is_empty() {
            len += 1;
        }
        if !self.resources_refs.is_empty() {
            len += 1;
        }
        if !self.intervals.is_empty() {
            len += 1;
        }
        if !self.num_points.is_empty() {
            len += 1;
        }
        if !self.source_type_name_refs.is_empty() {
            len += 1;
        }
        if !self.origin_info_refs.is_empty() {
            len += 1;
        }
        if !self.unit_refs.is_empty() {
            len += 1;
        }
        if !self.timestamps.is_empty() {
            len += 1;
        }
        if !self.vals_sint64.is_empty() {
            len += 1;
        }
        if !self.vals_float32.is_empty() {
            len += 1;
        }
        if !self.vals_float64.is_empty() {
            len += 1;
        }
        if !self.sketch_num_bins.is_empty() {
            len += 1;
        }
        if !self.sketch_bin_keys.is_empty() {
            len += 1;
        }
        if !self.sketch_bin_cnts.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadoghq.api.metrics.v3.MetricData", len)?;
        if !self.dict_name_str.is_empty() {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("dictNameStr", pbjson::private::base64::encode(&self.dict_name_str).as_str())?;
        }
        if !self.dict_tag_str.is_empty() {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("dictTagStr", pbjson::private::base64::encode(&self.dict_tag_str).as_str())?;
        }
        if !self.dict_tagsets.is_empty() {
            struct_ser.serialize_field("dictTagsets", &self.dict_tagsets.iter().map(ToString::to_string).collect::<Vec<_>>())?;
        }
        if !self.dict_resource_str.is_empty() {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("dictResourceStr", pbjson::private::base64::encode(&self.dict_resource_str).as_str())?;
        }
        if !self.dict_resource_len.is_empty() {
            struct_ser.serialize_field("dictResourceLen", &self.dict_resource_len.iter().map(ToString::to_string).collect::<Vec<_>>())?;
        }
        if !self.dict_resource_type.is_empty() {
            struct_ser.serialize_field("dictResourceType", &self.dict_resource_type.iter().map(ToString::to_string).collect::<Vec<_>>())?;
        }
        if !self.dict_resource_name.is_empty() {
            struct_ser.serialize_field("dictResourceName", &self.dict_resource_name.iter().map(ToString::to_string).collect::<Vec<_>>())?;
        }
        if !self.dict_source_type_name.is_empty() {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("dictSourceTypeName", pbjson::private::base64::encode(&self.dict_source_type_name).as_str())?;
        }
        if !self.dict_origin_info.is_empty() {
            struct_ser.serialize_field("dictOriginInfo", &self.dict_origin_info)?;
        }
        if !self.dict_unit_str.is_empty() {
            #[allow(clippy::needless_borrow)]
            #[allow(clippy::needless_borrows_for_generic_args)]
            struct_ser.serialize_field("dictUnitStr", pbjson::private::base64::encode(&self.dict_unit_str).as_str())?;
        }
        if !self.types.is_empty() {
            struct_ser.serialize_field("types", &self.types.iter().map(ToString::to_string).collect::<Vec<_>>())?;
        }
        if !self.name_refs.is_empty() {
            struct_ser.serialize_field("nameRefs", &self.name_refs.iter().map(ToString::to_string).collect::<Vec<_>>())?;
        }
        if !self.tagset_refs.is_empty() {
            struct_ser.serialize_field("tagsetRefs", &self.tagset_refs.iter().map(ToString::to_string).collect::<Vec<_>>())?;
        }
        if !self.resources_refs.is_empty() {
            struct_ser.serialize_field("resourcesRefs", &self.resources_refs.iter().map(ToString::to_string).collect::<Vec<_>>())?;
        }
        if !self.intervals.is_empty() {
            struct_ser.serialize_field("intervals", &self.intervals.iter().map(ToString::to_string).collect::<Vec<_>>())?;
        }
        if !self.num_points.is_empty() {
            struct_ser.serialize_field("numPoints", &self.num_points.iter().map(ToString::to_string).collect::<Vec<_>>())?;
        }
        if !self.source_type_name_refs.is_empty() {
            struct_ser.serialize_field("sourceTypeNameRefs", &self.source_type_name_refs.iter().map(ToString::to_string).collect::<Vec<_>>())?;
        }
        if !self.origin_info_refs.is_empty() {
            struct_ser.serialize_field("originInfoRefs", &self.origin_info_refs.iter().map(ToString::to_string).collect::<Vec<_>>())?;
        }
        if !self.unit_refs.is_empty() {
            struct_ser.serialize_field("unitRefs", &self.unit_refs.iter().map(ToString::to_string).collect::<Vec<_>>())?;
        }
        if !self.timestamps.is_empty() {
            struct_ser.serialize_field("timestamps", &self.timestamps.iter().map(ToString::to_string).collect::<Vec<_>>())?;
        }
        if !self.vals_sint64.is_empty() {
            struct_ser.serialize_field("valsSint64", &self.vals_sint64.iter().map(ToString::to_string).collect::<Vec<_>>())?;
        }
        if !self.vals_float32.is_empty() {
            struct_ser.serialize_field("valsFloat32", &self.vals_float32)?;
        }
        if !self.vals_float64.is_empty() {
            struct_ser.serialize_field("valsFloat64", &self.vals_float64)?;
        }
        if !self.sketch_num_bins.is_empty() {
            struct_ser.serialize_field("sketchNumBins", &self.sketch_num_bins.iter().map(ToString::to_string).collect::<Vec<_>>())?;
        }
        if !self.sketch_bin_keys.is_empty() {
            struct_ser.serialize_field("sketchBinKeys", &self.sketch_bin_keys)?;
        }
        if !self.sketch_bin_cnts.is_empty() {
            struct_ser.serialize_field("sketchBinCnts", &self.sketch_bin_cnts)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for MetricData {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "dictNameStr",
            "dictTagStr",
            "dictTagsets",
            "dictResourceStr",
            "dictResourceLen",
            "dictResourceType",
            "dictResourceName",
            "dictSourceTypeName",
            "dictOriginInfo",
            "dictUnitStr",
            "types",
            "nameRefs",
            "tagsetRefs",
            "resourcesRefs",
            "intervals",
            "numPoints",
            "sourceTypeNameRefs",
            "originInfoRefs",
            "unitRefs",
            "timestamps",
            "valsSint64",
            "valsFloat32",
            "valsFloat64",
            "sketchNumBins",
            "sketchBinKeys",
            "sketchBinCnts",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            DictNameStr,
            DictTagStr,
            DictTagsets,
            DictResourceStr,
            DictResourceLen,
            DictResourceType,
            DictResourceName,
            DictSourceTypeName,
            DictOriginInfo,
            DictUnitStr,
            Types,
            NameRefs,
            TagsetRefs,
            ResourcesRefs,
            Intervals,
            NumPoints,
            SourceTypeNameRefs,
            OriginInfoRefs,
            UnitRefs,
            Timestamps,
            ValsSint64,
            ValsFloat32,
            ValsFloat64,
            SketchNumBins,
            SketchBinKeys,
            SketchBinCnts,
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
                            "dictNameStr" => Ok(GeneratedField::DictNameStr),
                            "dictTagStr" => Ok(GeneratedField::DictTagStr),
                            "dictTagsets" => Ok(GeneratedField::DictTagsets),
                            "dictResourceStr" => Ok(GeneratedField::DictResourceStr),
                            "dictResourceLen" => Ok(GeneratedField::DictResourceLen),
                            "dictResourceType" => Ok(GeneratedField::DictResourceType),
                            "dictResourceName" => Ok(GeneratedField::DictResourceName),
                            "dictSourceTypeName" => Ok(GeneratedField::DictSourceTypeName),
                            "dictOriginInfo" => Ok(GeneratedField::DictOriginInfo),
                            "dictUnitStr" => Ok(GeneratedField::DictUnitStr),
                            "types" => Ok(GeneratedField::Types),
                            "nameRefs" => Ok(GeneratedField::NameRefs),
                            "tagsetRefs" => Ok(GeneratedField::TagsetRefs),
                            "resourcesRefs" => Ok(GeneratedField::ResourcesRefs),
                            "intervals" => Ok(GeneratedField::Intervals),
                            "numPoints" => Ok(GeneratedField::NumPoints),
                            "sourceTypeNameRefs" => Ok(GeneratedField::SourceTypeNameRefs),
                            "originInfoRefs" => Ok(GeneratedField::OriginInfoRefs),
                            "unitRefs" => Ok(GeneratedField::UnitRefs),
                            "timestamps" => Ok(GeneratedField::Timestamps),
                            "valsSint64" => Ok(GeneratedField::ValsSint64),
                            "valsFloat32" => Ok(GeneratedField::ValsFloat32),
                            "valsFloat64" => Ok(GeneratedField::ValsFloat64),
                            "sketchNumBins" => Ok(GeneratedField::SketchNumBins),
                            "sketchBinKeys" => Ok(GeneratedField::SketchBinKeys),
                            "sketchBinCnts" => Ok(GeneratedField::SketchBinCnts),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = MetricData;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadoghq.api.metrics.v3.MetricData")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<MetricData, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut dict_name_str__ = None;
                let mut dict_tag_str__ = None;
                let mut dict_tagsets__ = None;
                let mut dict_resource_str__ = None;
                let mut dict_resource_len__ = None;
                let mut dict_resource_type__ = None;
                let mut dict_resource_name__ = None;
                let mut dict_source_type_name__ = None;
                let mut dict_origin_info__ = None;
                let mut dict_unit_str__ = None;
                let mut types__ = None;
                let mut name_refs__ = None;
                let mut tagset_refs__ = None;
                let mut resources_refs__ = None;
                let mut intervals__ = None;
                let mut num_points__ = None;
                let mut source_type_name_refs__ = None;
                let mut origin_info_refs__ = None;
                let mut unit_refs__ = None;
                let mut timestamps__ = None;
                let mut vals_sint64__ = None;
                let mut vals_float32__ = None;
                let mut vals_float64__ = None;
                let mut sketch_num_bins__ = None;
                let mut sketch_bin_keys__ = None;
                let mut sketch_bin_cnts__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::DictNameStr => {
                            if dict_name_str__.is_some() {
                                return Err(serde::de::Error::duplicate_field("dictNameStr"));
                            }
                            dict_name_str__ = 
                                Some(map_.next_value::<::pbjson::private::BytesDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::DictTagStr => {
                            if dict_tag_str__.is_some() {
                                return Err(serde::de::Error::duplicate_field("dictTagStr"));
                            }
                            dict_tag_str__ = 
                                Some(map_.next_value::<::pbjson::private::BytesDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::DictTagsets => {
                            if dict_tagsets__.is_some() {
                                return Err(serde::de::Error::duplicate_field("dictTagsets"));
                            }
                            dict_tagsets__ = 
                                Some(map_.next_value::<Vec<::pbjson::private::NumberDeserialize<_>>>()?
                                    .into_iter().map(|x| x.0).collect())
                            ;
                        }
                        GeneratedField::DictResourceStr => {
                            if dict_resource_str__.is_some() {
                                return Err(serde::de::Error::duplicate_field("dictResourceStr"));
                            }
                            dict_resource_str__ = 
                                Some(map_.next_value::<::pbjson::private::BytesDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::DictResourceLen => {
                            if dict_resource_len__.is_some() {
                                return Err(serde::de::Error::duplicate_field("dictResourceLen"));
                            }
                            dict_resource_len__ = 
                                Some(map_.next_value::<Vec<::pbjson::private::NumberDeserialize<_>>>()?
                                    .into_iter().map(|x| x.0).collect())
                            ;
                        }
                        GeneratedField::DictResourceType => {
                            if dict_resource_type__.is_some() {
                                return Err(serde::de::Error::duplicate_field("dictResourceType"));
                            }
                            dict_resource_type__ = 
                                Some(map_.next_value::<Vec<::pbjson::private::NumberDeserialize<_>>>()?
                                    .into_iter().map(|x| x.0).collect())
                            ;
                        }
                        GeneratedField::DictResourceName => {
                            if dict_resource_name__.is_some() {
                                return Err(serde::de::Error::duplicate_field("dictResourceName"));
                            }
                            dict_resource_name__ = 
                                Some(map_.next_value::<Vec<::pbjson::private::NumberDeserialize<_>>>()?
                                    .into_iter().map(|x| x.0).collect())
                            ;
                        }
                        GeneratedField::DictSourceTypeName => {
                            if dict_source_type_name__.is_some() {
                                return Err(serde::de::Error::duplicate_field("dictSourceTypeName"));
                            }
                            dict_source_type_name__ = 
                                Some(map_.next_value::<::pbjson::private::BytesDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::DictOriginInfo => {
                            if dict_origin_info__.is_some() {
                                return Err(serde::de::Error::duplicate_field("dictOriginInfo"));
                            }
                            dict_origin_info__ = 
                                Some(map_.next_value::<Vec<::pbjson::private::NumberDeserialize<_>>>()?
                                    .into_iter().map(|x| x.0).collect())
                            ;
                        }
                        GeneratedField::DictUnitStr => {
                            if dict_unit_str__.is_some() {
                                return Err(serde::de::Error::duplicate_field("dictUnitStr"));
                            }
                            dict_unit_str__ = 
                                Some(map_.next_value::<::pbjson::private::BytesDeserialize<_>>()?.0)
                            ;
                        }
                        GeneratedField::Types => {
                            if types__.is_some() {
                                return Err(serde::de::Error::duplicate_field("types"));
                            }
                            types__ = 
                                Some(map_.next_value::<Vec<::pbjson::private::NumberDeserialize<_>>>()?
                                    .into_iter().map(|x| x.0).collect())
                            ;
                        }
                        GeneratedField::NameRefs => {
                            if name_refs__.is_some() {
                                return Err(serde::de::Error::duplicate_field("nameRefs"));
                            }
                            name_refs__ = 
                                Some(map_.next_value::<Vec<::pbjson::private::NumberDeserialize<_>>>()?
                                    .into_iter().map(|x| x.0).collect())
                            ;
                        }
                        GeneratedField::TagsetRefs => {
                            if tagset_refs__.is_some() {
                                return Err(serde::de::Error::duplicate_field("tagsetRefs"));
                            }
                            tagset_refs__ = 
                                Some(map_.next_value::<Vec<::pbjson::private::NumberDeserialize<_>>>()?
                                    .into_iter().map(|x| x.0).collect())
                            ;
                        }
                        GeneratedField::ResourcesRefs => {
                            if resources_refs__.is_some() {
                                return Err(serde::de::Error::duplicate_field("resourcesRefs"));
                            }
                            resources_refs__ = 
                                Some(map_.next_value::<Vec<::pbjson::private::NumberDeserialize<_>>>()?
                                    .into_iter().map(|x| x.0).collect())
                            ;
                        }
                        GeneratedField::Intervals => {
                            if intervals__.is_some() {
                                return Err(serde::de::Error::duplicate_field("intervals"));
                            }
                            intervals__ = 
                                Some(map_.next_value::<Vec<::pbjson::private::NumberDeserialize<_>>>()?
                                    .into_iter().map(|x| x.0).collect())
                            ;
                        }
                        GeneratedField::NumPoints => {
                            if num_points__.is_some() {
                                return Err(serde::de::Error::duplicate_field("numPoints"));
                            }
                            num_points__ = 
                                Some(map_.next_value::<Vec<::pbjson::private::NumberDeserialize<_>>>()?
                                    .into_iter().map(|x| x.0).collect())
                            ;
                        }
                        GeneratedField::SourceTypeNameRefs => {
                            if source_type_name_refs__.is_some() {
                                return Err(serde::de::Error::duplicate_field("sourceTypeNameRefs"));
                            }
                            source_type_name_refs__ = 
                                Some(map_.next_value::<Vec<::pbjson::private::NumberDeserialize<_>>>()?
                                    .into_iter().map(|x| x.0).collect())
                            ;
                        }
                        GeneratedField::OriginInfoRefs => {
                            if origin_info_refs__.is_some() {
                                return Err(serde::de::Error::duplicate_field("originInfoRefs"));
                            }
                            origin_info_refs__ = 
                                Some(map_.next_value::<Vec<::pbjson::private::NumberDeserialize<_>>>()?
                                    .into_iter().map(|x| x.0).collect())
                            ;
                        }
                        GeneratedField::UnitRefs => {
                            if unit_refs__.is_some() {
                                return Err(serde::de::Error::duplicate_field("unitRefs"));
                            }
                            unit_refs__ = 
                                Some(map_.next_value::<Vec<::pbjson::private::NumberDeserialize<_>>>()?
                                    .into_iter().map(|x| x.0).collect())
                            ;
                        }
                        GeneratedField::Timestamps => {
                            if timestamps__.is_some() {
                                return Err(serde::de::Error::duplicate_field("timestamps"));
                            }
                            timestamps__ = 
                                Some(map_.next_value::<Vec<::pbjson::private::NumberDeserialize<_>>>()?
                                    .into_iter().map(|x| x.0).collect())
                            ;
                        }
                        GeneratedField::ValsSint64 => {
                            if vals_sint64__.is_some() {
                                return Err(serde::de::Error::duplicate_field("valsSint64"));
                            }
                            vals_sint64__ = 
                                Some(map_.next_value::<Vec<::pbjson::private::NumberDeserialize<_>>>()?
                                    .into_iter().map(|x| x.0).collect())
                            ;
                        }
                        GeneratedField::ValsFloat32 => {
                            if vals_float32__.is_some() {
                                return Err(serde::de::Error::duplicate_field("valsFloat32"));
                            }
                            vals_float32__ = 
                                Some(map_.next_value::<Vec<::pbjson::private::NumberDeserialize<_>>>()?
                                    .into_iter().map(|x| x.0).collect())
                            ;
                        }
                        GeneratedField::ValsFloat64 => {
                            if vals_float64__.is_some() {
                                return Err(serde::de::Error::duplicate_field("valsFloat64"));
                            }
                            vals_float64__ = 
                                Some(map_.next_value::<Vec<::pbjson::private::NumberDeserialize<_>>>()?
                                    .into_iter().map(|x| x.0).collect())
                            ;
                        }
                        GeneratedField::SketchNumBins => {
                            if sketch_num_bins__.is_some() {
                                return Err(serde::de::Error::duplicate_field("sketchNumBins"));
                            }
                            sketch_num_bins__ = 
                                Some(map_.next_value::<Vec<::pbjson::private::NumberDeserialize<_>>>()?
                                    .into_iter().map(|x| x.0).collect())
                            ;
                        }
                        GeneratedField::SketchBinKeys => {
                            if sketch_bin_keys__.is_some() {
                                return Err(serde::de::Error::duplicate_field("sketchBinKeys"));
                            }
                            sketch_bin_keys__ = 
                                Some(map_.next_value::<Vec<::pbjson::private::NumberDeserialize<_>>>()?
                                    .into_iter().map(|x| x.0).collect())
                            ;
                        }
                        GeneratedField::SketchBinCnts => {
                            if sketch_bin_cnts__.is_some() {
                                return Err(serde::de::Error::duplicate_field("sketchBinCnts"));
                            }
                            sketch_bin_cnts__ = 
                                Some(map_.next_value::<Vec<::pbjson::private::NumberDeserialize<_>>>()?
                                    .into_iter().map(|x| x.0).collect())
                            ;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(MetricData {
                    dict_name_str: dict_name_str__.unwrap_or_default(),
                    dict_tag_str: dict_tag_str__.unwrap_or_default(),
                    dict_tagsets: dict_tagsets__.unwrap_or_default(),
                    dict_resource_str: dict_resource_str__.unwrap_or_default(),
                    dict_resource_len: dict_resource_len__.unwrap_or_default(),
                    dict_resource_type: dict_resource_type__.unwrap_or_default(),
                    dict_resource_name: dict_resource_name__.unwrap_or_default(),
                    dict_source_type_name: dict_source_type_name__.unwrap_or_default(),
                    dict_origin_info: dict_origin_info__.unwrap_or_default(),
                    dict_unit_str: dict_unit_str__.unwrap_or_default(),
                    types: types__.unwrap_or_default(),
                    name_refs: name_refs__.unwrap_or_default(),
                    tagset_refs: tagset_refs__.unwrap_or_default(),
                    resources_refs: resources_refs__.unwrap_or_default(),
                    intervals: intervals__.unwrap_or_default(),
                    num_points: num_points__.unwrap_or_default(),
                    source_type_name_refs: source_type_name_refs__.unwrap_or_default(),
                    origin_info_refs: origin_info_refs__.unwrap_or_default(),
                    unit_refs: unit_refs__.unwrap_or_default(),
                    timestamps: timestamps__.unwrap_or_default(),
                    vals_sint64: vals_sint64__.unwrap_or_default(),
                    vals_float32: vals_float32__.unwrap_or_default(),
                    vals_float64: vals_float64__.unwrap_or_default(),
                    sketch_num_bins: sketch_num_bins__.unwrap_or_default(),
                    sketch_bin_keys: sketch_bin_keys__.unwrap_or_default(),
                    sketch_bin_cnts: sketch_bin_cnts__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadoghq.api.metrics.v3.MetricData", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for Payload {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if self.metadata.is_some() {
            len += 1;
        }
        if self.metric_data.is_some() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadoghq.api.metrics.v3.Payload", len)?;
        if let Some(v) = self.metadata.as_ref() {
            struct_ser.serialize_field("metadata", v)?;
        }
        if let Some(v) = self.metric_data.as_ref() {
            struct_ser.serialize_field("metricData", v)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for Payload {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "metadata",
            "metricData",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
            Metadata,
            MetricData,
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
                            "metadata" => Ok(GeneratedField::Metadata),
                            "metricData" => Ok(GeneratedField::MetricData),
                            _ => Ok(GeneratedField::__SkipField__),
                        }
                    }
                }
                deserializer.deserialize_identifier(GeneratedVisitor)
            }
        }
        struct GeneratedVisitor;
        impl<'de> serde::de::Visitor<'de> for GeneratedVisitor {
            type Value = Payload;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadoghq.api.metrics.v3.Payload")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<Payload, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut metadata__ = None;
                let mut metric_data__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Metadata => {
                            if metadata__.is_some() {
                                return Err(serde::de::Error::duplicate_field("metadata"));
                            }
                            metadata__ = map_.next_value()?;
                        }
                        GeneratedField::MetricData => {
                            if metric_data__.is_some() {
                                return Err(serde::de::Error::duplicate_field("metricData"));
                            }
                            metric_data__ = map_.next_value()?;
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(Payload {
                    metadata: metadata__,
                    metric_data: metric_data__,
                })
            }
        }
        deserializer.deserialize_struct("datadoghq.api.metrics.v3.Payload", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for Response {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut len = 0;
        if !self.error.is_empty() {
            len += 1;
        }
        let mut struct_ser = serializer.serialize_struct("datadoghq.api.metrics.v3.Response", len)?;
        if !self.error.is_empty() {
            struct_ser.serialize_field("error", &self.error)?;
        }
        struct_ser.end()
    }
}
impl<'de> serde::Deserialize<'de> for Response {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "error",
        ];

        #[allow(clippy::enum_variant_names)]
        enum GeneratedField {
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
            type Value = Response;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct datadoghq.api.metrics.v3.Response")
            }

            fn visit_map<V>(self, mut map_: V) -> std::result::Result<Response, V::Error>
                where
                    V: serde::de::MapAccess<'de>,
            {
                let mut error__ = None;
                while let Some(k) = map_.next_key()? {
                    match k {
                        GeneratedField::Error => {
                            if error__.is_some() {
                                return Err(serde::de::Error::duplicate_field("error"));
                            }
                            error__ = Some(map_.next_value()?);
                        }
                        GeneratedField::__SkipField__ => {
                            let _ = map_.next_value::<serde::de::IgnoredAny>()?;
                        }
                    }
                }
                Ok(Response {
                    error: error__.unwrap_or_default(),
                })
            }
        }
        deserializer.deserialize_struct("datadoghq.api.metrics.v3.Response", FIELDS, GeneratedVisitor)
    }
}
impl serde::Serialize for MetricFlags {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let variant = match self {
            Self::FlagNone => "flagNone",
            Self::FlagNoIndex => "flagNoIndex",
            Self::FlagHasUnit => "flagHasUnit",
        };
        serializer.serialize_str(variant)
    }
}
impl<'de> serde::Deserialize<'de> for MetricFlags {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "flagNone",
            "flagNoIndex",
            "flagHasUnit",
        ];

        struct GeneratedVisitor;

        impl serde::de::Visitor<'_> for GeneratedVisitor {
            type Value = MetricFlags;

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
                    "flagNone" => Ok(MetricFlags::FlagNone),
                    "flagNoIndex" => Ok(MetricFlags::FlagNoIndex),
                    "flagHasUnit" => Ok(MetricFlags::FlagHasUnit),
                    _ => Err(serde::de::Error::unknown_variant(value, FIELDS)),
                }
            }
        }
        deserializer.deserialize_any(GeneratedVisitor)
    }
}
impl serde::Serialize for MetricType {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let variant = match self {
            Self::Unused => "UNUSED",
            Self::Count => "Count",
            Self::Rate => "Rate",
            Self::Gauge => "Gauge",
            Self::Sketch => "Sketch",
        };
        serializer.serialize_str(variant)
    }
}
impl<'de> serde::Deserialize<'de> for MetricType {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "UNUSED",
            "Count",
            "Rate",
            "Gauge",
            "Sketch",
        ];

        struct GeneratedVisitor;

        impl serde::de::Visitor<'_> for GeneratedVisitor {
            type Value = MetricType;

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
                    "UNUSED" => Ok(MetricType::Unused),
                    "Count" => Ok(MetricType::Count),
                    "Rate" => Ok(MetricType::Rate),
                    "Gauge" => Ok(MetricType::Gauge),
                    "Sketch" => Ok(MetricType::Sketch),
                    _ => Err(serde::de::Error::unknown_variant(value, FIELDS)),
                }
            }
        }
        deserializer.deserialize_any(GeneratedVisitor)
    }
}
impl serde::Serialize for ValueType {
    #[allow(deprecated)]
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let variant = match self {
            Self::Zero => "Zero",
            Self::Sint64 => "Sint64",
            Self::Float32 => "Float32",
            Self::Float64 => "Float64",
        };
        serializer.serialize_str(variant)
    }
}
impl<'de> serde::Deserialize<'de> for ValueType {
    #[allow(deprecated)]
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &[
            "Zero",
            "Sint64",
            "Float32",
            "Float64",
        ];

        struct GeneratedVisitor;

        impl serde::de::Visitor<'_> for GeneratedVisitor {
            type Value = ValueType;

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
                    "Zero" => Ok(ValueType::Zero),
                    "Sint64" => Ok(ValueType::Sint64),
                    "Float32" => Ok(ValueType::Float32),
                    "Float64" => Ok(ValueType::Float64),
                    _ => Err(serde::de::Error::unknown_variant(value, FIELDS)),
                }
            }
        }
        deserializer.deserialize_any(GeneratedVisitor)
    }
}
