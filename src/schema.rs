use anyhow::{Context, Result, bail};
use dotnetdll::prelude::{
    AlwaysFailsResolver, BaseType, FixedArg, MemberType, ReadOptions, Resolution, ResolvedDebug,
    TypeSource, UserType,
};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

const ASSEMBLY_NAME: &str = "Assembly-CSharp.dll";
const DEF_TYPE: &str = "Verse.Def";
const CUSTOM_LOADER_METHOD: &str = "LoadDataFromXmlCustom";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ManagedType {
    Primitive,
    Named(String),
    List(Box<ManagedType>),
    Dictionary(Box<ManagedType>, Box<ManagedType>),
    Array(Box<ManagedType>),
    Unknown,
}

#[derive(Debug, Clone)]
pub(crate) struct ManagedField {
    pub name: String,
    pub aliases: Vec<String>,
    pub value_type: ManagedType,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ManagedTypeInfo {
    pub base_type: Option<String>,
    pub fields: Vec<ManagedField>,
    pub has_custom_loader: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CustomLoader {
    None,
    Known(CustomLoaderRule),
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CustomLoaderRule {
    ElementName(&'static str),
    ElementTextTypeFromName,
    ThingDefCount,
    NoReferences,
}

#[derive(Debug, Clone, Default)]
pub struct ReferenceSchema {
    types: HashMap<String, ManagedTypeInfo>,
    types_by_short_name: HashMap<String, Vec<String>>,
}

impl ReferenceSchema {
    pub fn from_rimworld_path(rimworld_path: &Path) -> Result<Self> {
        let assembly_path = find_game_assembly(rimworld_path)?;
        Self::from_assembly(&assembly_path)
            .with_context(|| format!("failed to read metadata from {}", assembly_path.display()))
    }

    pub fn from_assembly(assembly_path: &Path) -> Result<Self> {
        let bytes = fs::read(assembly_path)?;
        let resolution = Resolution::parse(
            &bytes,
            ReadOptions {
                skip_method_bodies: true,
                lazy_property_signatures: true,
                ..ReadOptions::default()
            },
        )?;

        let mut types = HashMap::new();
        for definition in &resolution.type_definitions {
            let name = definition.nested_type_name(&resolution);
            if name == "<Module>" {
                continue;
            }

            let base_type = definition
                .extends
                .as_ref()
                .map(|source| type_source_name(source, &resolution));
            let fields = definition
                .fields
                .iter()
                .filter(|field| !field.static_member)
                .map(|field| ManagedField {
                    name: field.name.to_string(),
                    aliases: load_aliases(field, &resolution),
                    value_type: managed_type(&field.return_type, &resolution),
                })
                .collect();
            let has_custom_loader = definition
                .methods
                .iter()
                .any(|method| method.name == CUSTOM_LOADER_METHOD);

            types.insert(
                name,
                ManagedTypeInfo {
                    base_type,
                    fields,
                    has_custom_loader,
                },
            );
        }

        Ok(Self::from_types(types))
    }

    pub(crate) fn resolve_type(&self, name: &str, default_type: Option<&str>) -> Option<String> {
        if self.types.contains_key(name) {
            return Some(name.to_string());
        }

        if let Some(namespace) = default_type.and_then(|name| name.rsplit_once('.')) {
            let scoped_name = format!("{}.{}", namespace.0, name);
            if self.types.contains_key(&scoped_name) {
                return Some(scoped_name);
            }
        }

        let short_name = short_type_name(name);
        let matches = self.types_by_short_name.get(short_name)?;
        if matches.len() == 1 {
            return matches.first().cloned();
        }

        ["Verse", "RimWorld"]
            .iter()
            .map(|namespace| format!("{namespace}.{name}"))
            .find(|candidate| matches.contains(candidate))
    }

    pub(crate) fn find_field(&self, type_name: &str, xml_name: &str) -> Option<ManagedField> {
        let hierarchy = self.hierarchy(type_name);

        for owner in &hierarchy {
            if let Some(field) = self
                .types
                .get(*owner)?
                .fields
                .iter()
                .find(|field| field.name == xml_name)
            {
                return Some(field.clone());
            }
        }
        for owner in &hierarchy {
            if let Some(field) = self
                .types
                .get(*owner)?
                .fields
                .iter()
                .find(|field| field.name.eq_ignore_ascii_case(xml_name))
            {
                return Some(field.clone());
            }
        }
        for owner in hierarchy {
            if let Some(field) = self.types.get(owner)?.fields.iter().find(|field| {
                field
                    .aliases
                    .iter()
                    .any(|alias| alias.eq_ignore_ascii_case(xml_name))
            }) {
                return Some(field.clone());
            }
        }

        None
    }

    pub(crate) fn is_def_type(&self, type_name: &str) -> bool {
        self.is_assignable(type_name, DEF_TYPE)
    }

    pub(crate) fn is_enum_type(&self, type_name: &str) -> bool {
        self.is_assignable(type_name, "System.Enum")
    }

    pub(crate) fn is_assignable(&self, concrete_type: &str, expected_type: &str) -> bool {
        let mut current = Some(concrete_type);
        let mut visited = HashSet::new();
        while let Some(type_name) = current {
            if type_name == expected_type {
                return true;
            }
            if !visited.insert(type_name) {
                return false;
            }
            current = self
                .types
                .get(type_name)
                .and_then(|info| info.base_type.as_deref());
        }
        false
    }

    pub(crate) fn custom_loader(&self, type_name: &str) -> CustomLoader {
        let Some(loader_type) = self.hierarchy(type_name).into_iter().find(|name| {
            self.types
                .get(*name)
                .is_some_and(|info| info.has_custom_loader)
        }) else {
            return CustomLoader::None;
        };

        let rule = match loader_type {
            "RimWorld.StatModifier" => CustomLoaderRule::ElementName("RimWorld.StatDef"),
            "RimWorld.SkillGain" => CustomLoaderRule::ElementName("RimWorld.SkillDef"),
            "RimWorld.PawnGenOption" | "RimWorld.BiomeAnimalRecord" => {
                CustomLoaderRule::ElementName("Verse.PawnKindDef")
            }
            "RimWorld.MutatorChance" => CustomLoaderRule::ElementName("RimWorld.TileMutatorDef"),
            "Verse.DefHyperlink" => CustomLoaderRule::ElementTextTypeFromName,
            "Verse.ThingDefCountClass" => CustomLoaderRule::ThingDefCount,
            "Verse.ShaderParameter" => CustomLoaderRule::NoReferences,
            _ => return CustomLoader::Unknown,
        };
        CustomLoader::Known(rule)
    }

    pub(crate) fn contains_type(&self, type_name: &str) -> bool {
        self.types.contains_key(type_name)
    }

    pub(crate) fn from_types(types: HashMap<String, ManagedTypeInfo>) -> Self {
        let mut types_by_short_name: HashMap<String, Vec<String>> = HashMap::new();
        for name in types.keys() {
            types_by_short_name
                .entry(short_type_name(name).to_string())
                .or_default()
                .push(name.clone());
        }
        Self {
            types,
            types_by_short_name,
        }
    }

    fn hierarchy<'a>(&'a self, type_name: &'a str) -> Vec<&'a str> {
        let mut hierarchy = Vec::new();
        let mut current = Some(type_name);
        let mut visited = HashSet::new();
        while let Some(name) = current {
            if !visited.insert(name) {
                break;
            }
            let Some(info) = self.types.get(name) else {
                break;
            };
            hierarchy.push(name);
            current = info.base_type.as_deref();
        }
        hierarchy
    }
}

fn find_game_assembly(rimworld_path: &Path) -> Result<PathBuf> {
    let mut matches = Vec::new();
    for entry in WalkDir::new(rimworld_path).max_depth(4) {
        let entry = entry.with_context(|| {
            format!(
                "failed while searching {} for {ASSEMBLY_NAME}",
                rimworld_path.display()
            )
        })?;
        if entry.file_type().is_file() && entry.file_name() == ASSEMBLY_NAME {
            matches.push(entry.into_path());
        }
    }

    match matches.as_slice() {
        [path] => Ok(path.clone()),
        [] => bail!(
            "{ASSEMBLY_NAME} was not found below {}",
            rimworld_path.display()
        ),
        _ => bail!(
            "multiple {ASSEMBLY_NAME} files were found below {}",
            rimworld_path.display()
        ),
    }
}

fn short_type_name(name: &str) -> &str {
    name.rsplit(['.', '/']).next().unwrap_or(name)
}

fn user_type_name(user_type: &UserType, resolution: &Resolution<'_>) -> String {
    let resolved_name = user_type.show(resolution);
    if let Some((_, type_name)) = resolved_name.rsplit_once(']') {
        type_name.to_string()
    } else {
        resolved_name
    }
}

fn type_source_name(source: &TypeSource<MemberType>, resolution: &Resolution<'_>) -> String {
    match source {
        TypeSource::User(user_type) => user_type_name(user_type, resolution),
        TypeSource::Generic { base, .. } => user_type_name(base, resolution),
    }
}

fn managed_type(member_type: &MemberType, resolution: &Resolution<'_>) -> ManagedType {
    let MemberType::Base(base_type) = member_type else {
        return ManagedType::Unknown;
    };

    match base_type.as_ref() {
        BaseType::Type { source, .. } => match source {
            TypeSource::User(user_type) => {
                ManagedType::Named(user_type_name(user_type, resolution))
            }
            TypeSource::Generic { base, parameters } => managed_generic_type(
                user_type_name(base, resolution),
                parameters
                    .iter()
                    .map(|parameter| managed_type(parameter, resolution))
                    .collect(),
            ),
        },
        BaseType::Vector(_, item_type) | BaseType::Array(item_type, _) => {
            ManagedType::Array(Box::new(managed_type(item_type, resolution)))
        }
        BaseType::Boolean
        | BaseType::Char
        | BaseType::Int8
        | BaseType::UInt8
        | BaseType::Int16
        | BaseType::UInt16
        | BaseType::Int32
        | BaseType::UInt32
        | BaseType::Int64
        | BaseType::UInt64
        | BaseType::Float32
        | BaseType::Float64
        | BaseType::IntPtr
        | BaseType::UIntPtr
        | BaseType::Object
        | BaseType::String => ManagedType::Primitive,
        BaseType::ValuePointer(_, _) | BaseType::FunctionPointer(_) => ManagedType::Unknown,
    }
}

fn managed_generic_type(base_name: String, parameters: Vec<ManagedType>) -> ManagedType {
    let mut parameters = parameters.into_iter();
    if base_name == "System.Collections.Generic.List`1" {
        ManagedType::List(Box::new(parameters.next().unwrap_or(ManagedType::Unknown)))
    } else if base_name == "System.Collections.Generic.Dictionary`2" {
        ManagedType::Dictionary(
            Box::new(parameters.next().unwrap_or(ManagedType::Unknown)),
            Box::new(parameters.next().unwrap_or(ManagedType::Unknown)),
        )
    } else if base_name.ends_with(".SlateRef`1") {
        parameters.next().unwrap_or(ManagedType::Unknown)
    } else {
        ManagedType::Named(base_name)
    }
}

fn load_aliases(field: &dotnetdll::prelude::Field<'_>, resolution: &Resolution<'_>) -> Vec<String> {
    field
        .attributes
        .iter()
        .filter(|attribute| {
            attribute
                .constructor
                .show(resolution)
                .contains("Verse.LoadAliasAttribute")
        })
        .filter_map(|attribute| {
            let data = attribute
                .instantiation_data(&AlwaysFailsResolver, resolution)
                .ok()?;
            match data.constructor_args.first()? {
                FixedArg::String(Some(alias)) => Some(alias.to_string()),
                _ => None,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_schema() -> ReferenceSchema {
        ReferenceSchema::from_types(HashMap::from([
            ("Verse.Def".to_string(), ManagedTypeInfo::default()),
            (
                "Verse.ThingDef".to_string(),
                ManagedTypeInfo {
                    base_type: Some("Verse.Def".to_string()),
                    fields: vec![ManagedField {
                        name: "designationCategory".to_string(),
                        aliases: vec!["designationCat".to_string()],
                        value_type: ManagedType::Named("Verse.DesignationCategoryDef".to_string()),
                    }],
                    has_custom_loader: false,
                },
            ),
            (
                "Verse.SpecialThingDef".to_string(),
                ManagedTypeInfo {
                    base_type: Some("Verse.ThingDef".to_string()),
                    ..ManagedTypeInfo::default()
                },
            ),
            (
                "Verse.DesignationCategoryDef".to_string(),
                ManagedTypeInfo {
                    base_type: Some("Verse.Def".to_string()),
                    ..ManagedTypeInfo::default()
                },
            ),
            (
                "Verse.WorkTags".to_string(),
                ManagedTypeInfo {
                    base_type: Some("System.Enum".to_string()),
                    ..ManagedTypeInfo::default()
                },
            ),
        ]))
    }

    #[test]
    fn resolves_short_names_and_inherited_fields() {
        let schema = test_schema();

        assert_eq!(
            schema.resolve_type("SpecialThingDef", Some("Verse.ThingDef")),
            Some("Verse.SpecialThingDef".to_string())
        );
        assert_eq!(
            schema
                .find_field("Verse.SpecialThingDef", "designationCat")
                .unwrap()
                .name,
            "designationCategory"
        );
    }

    #[test]
    fn recognizes_definition_assignability() {
        let schema = test_schema();

        assert!(schema.is_def_type("Verse.SpecialThingDef"));
        assert!(schema.is_assignable("Verse.SpecialThingDef", "Verse.ThingDef"));
        assert!(!schema.is_assignable("Verse.ThingDef", "Verse.SpecialThingDef"));
        assert!(schema.is_enum_type("Verse.WorkTags"));
        assert!(!schema.is_enum_type("Verse.ThingDef"));
    }

    #[test]
    fn distinguishes_known_and_unknown_custom_loaders() {
        let schema = ReferenceSchema::from_types(HashMap::from([
            (
                "RimWorld.StatModifier".to_string(),
                ManagedTypeInfo {
                    has_custom_loader: true,
                    ..ManagedTypeInfo::default()
                },
            ),
            (
                "Example.ModLoader".to_string(),
                ManagedTypeInfo {
                    has_custom_loader: true,
                    ..ManagedTypeInfo::default()
                },
            ),
        ]));

        assert_eq!(
            schema.custom_loader("RimWorld.StatModifier"),
            CustomLoader::Known(CustomLoaderRule::ElementName("RimWorld.StatDef"))
        );
        assert_eq!(
            schema.custom_loader("Example.ModLoader"),
            CustomLoader::Unknown
        );
    }

    #[test]
    fn unwraps_slate_ref_generic_values() {
        let list_type = ManagedType::List(Box::new(ManagedType::Named(
            "RimWorld.QuestGen.PawnKindOption".to_string(),
        )));

        assert_eq!(
            managed_generic_type(
                "RimWorld.QuestGen.SlateRef`1".to_string(),
                vec![list_type.clone()],
            ),
            list_type
        );
    }
}
