use anyhow::{Context, Result};

use filesystem;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

pub type Files = BTreeMap<PathBuf, FileTarget>;
pub type Variables = toml::value::Table;
pub type Helpers = BTreeMap<String, PathBuf>;

#[derive(Debug, Clone)]
pub struct Configuration {
    pub files: Files,
    pub variables: Variables,
    pub helpers: Helpers,
}

fn merge_configuration_tables(
    mut global: GlobalConfig,
    local: LocalConfig,
) -> Result<Configuration, LoadConfigFailType> {
    // Apply packages filter
    global.packages = global
        .packages
        .into_iter()
        .filter(|(k, _)| local.packages.contains(&k))
        .collect();

    // Patch each package with included.toml's
    for included_path in &local.includes {
        let mut included: IncludedConfig =
            filesystem::load_file(&included_path).map_err(|e| LoadConfigFailType::Parse {
                file: included_path.into(),
                source: e,
            })?;

        // If package isn't filtered it's ignored, if package isn't included it's ignored
        for (package_name, package_global) in global.packages.iter_mut() {
            if let Some(package_included) = included.remove(package_name) {
                package_global.files.extend(package_included.files);
                package_global.variables.extend(package_included.variables);
            }
        }

        if !included.is_empty() {
            todo!("extra packages");
        }
    }

    let mut output = Configuration {
        helpers: global.helpers,
        files: Files::default(),
        variables: Variables::default(),
    };

    // Merge all the packages
    let mut configuration_packages = global.packages.into_iter();
    let mut first_package = configuration_packages
        .next()
        .unwrap_or_else(|| (String::new(), Package::default()))
        .1;
    for (_, v) in configuration_packages {
        for (file_name, file_target) in v.files {
            if first_package.files.contains_key(&file_name) {
                todo!("duplicate file");
            } else {
                first_package.files.insert(file_name, file_target);
            }
        }

        for (variable_name, variable_value) in v.variables {
            if first_package.variables.contains_key(&variable_name) {
                todo!("duplicate variable");
            } else {
                first_package.variables.insert(variable_name, variable_value);
            }
        }
    }
    output.files = first_package.files;
    output.variables = first_package.variables;

    // Add local.toml's patches
    output.files.extend(local.files);
    output.variables.extend(local.variables);

    // Remove files with target = ""
    output.files = output
        .files
        .into_iter()
        .filter(|(_, v)| v.path().to_string_lossy() != "")
        .collect();

    Ok(output)
}

#[derive(Error, Debug)]
pub enum LoadConfigFailType {
    #[error("find config files")]
    Find,

    #[error("parse config file {file}")]
    Parse {
        file: PathBuf,
        source: filesystem::FileLoadError,
    },

    #[error("inspect source files")]
    InvalidSourceTree { source: anyhow::Error },
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct TemplateTarget {
    pub target: PathBuf,
    pub append: Option<String>,
    pub prepend: Option<String>,
}

// Deserialize implemented manually
#[derive(Debug, Clone, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(untagged)]
pub enum FileTarget {
    Automatic(PathBuf),
    Symbolic(PathBuf),
    ComplexTemplate(TemplateTarget),
}

impl<'de> serde::Deserialize<'de> for FileTarget {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(field_identifier, rename_all = "snake_case")]
        enum Field {
            Target,
            Append,
            Prepend,
            Type,
        }

        struct FileTargetVisitor;

        impl<'de> serde::de::Visitor<'de> for FileTargetVisitor {
            type Value = FileTarget;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a string or a map")
            }

            fn visit_str<E>(self, s: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(FileTarget::Automatic(s.into()))
            }

            fn visit_map<V>(self, mut map: V) -> Result<Self::Value, V::Error>
            where
                V: serde::de::MapAccess<'de>,
            {
                let mut file_type = None;
                let mut target = None;
                let mut append = None;
                let mut prepend = None;

                while let Some(key) = map.next_key()? {
                    match key {
                        Field::Type => {
                            if file_type.is_some() {
                                return Err(serde::de::Error::duplicate_field("type"));
                            }
                            file_type = Some(map.next_value()?);
                        }
                        Field::Target => {
                            if target.is_some() {
                                return Err(serde::de::Error::duplicate_field("target"));
                            }
                            target = Some(map.next_value()?);
                        }
                        Field::Append => {
                            if append.is_some() {
                                return Err(serde::de::Error::duplicate_field("append"));
                            }
                            append = Some(map.next_value()?);
                        }
                        Field::Prepend => {
                            if prepend.is_some() {
                                return Err(serde::de::Error::duplicate_field("prepend"));
                            }
                            prepend = Some(map.next_value()?);
                        }
                    }
                }

                let file_type = file_type.ok_or_else(|| serde::de::Error::missing_field("type"))?;
                let target = target.ok_or_else(|| serde::de::Error::missing_field("target"))?;
                let ans = match file_type {
                    "symbolic" => {
                        if append.is_some() || prepend.is_some() {
                            return Err(serde::de::Error::custom(
                                "invalid use of `append` or `prepend` on a symbolic target",
                            ));
                        }
                        FileTarget::Symbolic(target)
                    }
                    "template" => FileTarget::ComplexTemplate(TemplateTarget {
                        append,
                        prepend,
                        target,
                    }),
                    other_type => {
                        return Err(serde::de::Error::invalid_value(
                            serde::de::Unexpected::Str(other_type),
                            &"`symbolic` or `template`",
                        ))
                    }
                };

                Ok(ans)
            }
        }

        deserializer.deserialize_any(FileTargetVisitor)
    }
}

impl FileTarget {
    fn map<F: FnOnce(PathBuf) -> PathBuf>(self, func: F) -> Self {
        match self {
            FileTarget::Automatic(path) => FileTarget::Automatic(func(path)),
            FileTarget::Symbolic(path) => FileTarget::Symbolic(func(path)),
            FileTarget::ComplexTemplate(mut t) => {
                t.target = func(t.target);
                FileTarget::ComplexTemplate(t)
            }
        }
    }

    pub fn path(&self) -> &Path {
        match self {
            FileTarget::Automatic(path) => &path,
            FileTarget::Symbolic(path) => &path,
            FileTarget::ComplexTemplate(TemplateTarget { target, .. }) => &target,
        }
    }
}

impl<T: Into<PathBuf>> From<T> for FileTarget {
    fn from(input: T) -> Self {
        FileTarget::Automatic(input.into())
    }
}

#[derive(Debug, Deserialize, Serialize, Default)]
struct Package {
    #[serde(default)]
    files: Files,
    #[serde(default)]
    variables: Variables,
}

#[derive(Debug, Deserialize, Serialize)]
struct GlobalConfig {
    #[serde(default)]
    helpers: Helpers,
    #[serde(flatten)]
    packages: BTreeMap<String, Package>,
}

type IncludedConfig = BTreeMap<String, Package>;

#[derive(Debug, Deserialize, Serialize)]
struct LocalConfig {
    #[serde(default)]
    includes: Vec<PathBuf>,
    packages: Vec<String>,
    files: Files,
    variables: Variables,
}

fn try_load_configuration(
    local_config: &Path,
    global_config: &Path,
) -> Result<Configuration, LoadConfigFailType> {
    let global: GlobalConfig = match filesystem::load_file(global_config) {
        Err(filesystem::FileLoadError::Open { .. }) => Err(LoadConfigFailType::Find),
        Err(e) => Err(LoadConfigFailType::Parse {
            file: global_config.into(),
            source: e,
        }),
        Ok(global) => Ok(global),
    }?;

    trace!("Global config: {:#?}", global);

    let local: LocalConfig =
        filesystem::load_file(local_config).map_err(|e| LoadConfigFailType::Parse {
            file: local_config.into(),
            source: e,
        })?;
    trace!("Local config: {:#?}", local);

    let mut merged_config = merge_configuration_tables(global, local)?;
    trace!("Merged config: {:#?}", merged_config);

    debug!("Expanding files which are directories...");
    merged_config.files = expand_directories(merged_config.files)
        .map_err(|e| LoadConfigFailType::InvalidSourceTree { source: e })?;

    trace!("Final files: {:#?}", merged_config.files);
    trace!("Final variables: {:#?}", merged_config.variables);
    trace!("Final helpers: {:?}", merged_config.helpers);

    Ok(merged_config)
}

fn expand_directories(files: Files) -> Result<Files> {
    let expanded = files
        .into_iter()
        .map(|(from, to)| expand_directory(&from, to).context(format!("expand file {:?}", from)))
        .collect::<Result<Vec<Files>>>()?;
    Ok(expanded.into_iter().flatten().collect::<Files>())
}

/// If a file is given, it will return a map of one element
/// Otherwise, returns recursively all the children and their targets
///  in relation to parent target
fn expand_directory(source: &Path, target: FileTarget) -> Result<Files> {
    if fs::metadata(source)
        .context("read file's metadata")?
        .is_file()
    {
        let mut map = Files::new();
        map.insert(source.into(), target);
        Ok(map)
    } else {
        let target = match target {
            FileTarget::Automatic(target) => target,
            // TODO: test this
            _ => bail!("Complex file target not implemented for directories yet."),
        };
        let expanded = fs::read_dir(source)
            .context("read contents of directory")?
            .map(|child| -> Result<Files> {
                let child = child?.file_name();
                let child_source = PathBuf::from(source).join(&child);
                let child_target = target.clone().join(&child);
                expand_directory(&child_source, child_target.into())
                    .context(format!("expand file {:?}", child_source))
            })
            .collect::<Result<Vec<Files>>>()?; // Use transposition of Iterator<Result<T,E>> -> Result<Sequence<T>, E>
        Ok(expanded.into_iter().flatten().collect())
    }
}

pub fn load_configuration(
    local_config: &Path,
    global_config: &Path,
) -> Result<Configuration, LoadConfigFailType> {
    debug!("Loading configuration...");
    let mut parent = ::std::env::current_dir().expect("Failed to get current directory.");
    let mut configuration = loop {
        match try_load_configuration(local_config, global_config) {
            Ok(conf) => break Ok(conf),
            Err(LoadConfigFailType::Find) => {
                if let Some(new_parent) = parent.parent().map(|p| p.into()) {
                    parent = new_parent;
                    warn!(
                        "Didn't find configuration in current directory. Going one up to {:?}",
                        parent
                    );
                } else {
                    warn!("Reached root.");
                    break Err(LoadConfigFailType::Find);
                }
                ::std::env::set_current_dir(&parent).expect("Failed to move up a directory");
            }
            Err(e) => break Err(e),
        }
    }?;
    debug!("Loaded configuration. Expanding tildes to home directory...");

    configuration.files = configuration.files
        .into_iter()
        .map(|(k, v)| {
            (
                k,
                v.map(|path| {
                    shellexpand::tilde(&path.to_string_lossy())
                        .to_string()
                        .into()
                }),
            )
        })
        .collect();

    trace!("Expanded files: {:#?}", configuration.files);
    Ok(configuration)
}

pub fn save_dummy_config(
    files: Vec<String>,
    local_config_path: &Path,
    global_config_path: &Path,
) -> Result<()> {
    debug!("Saving dummy config...");
    let package = Package {
        files: files.into_iter().map(|f| (f.into(), "".into())).collect(),
        variables: Variables::new(),
    };
    trace!("Default package: {:#?}", package);

    let mut packages = BTreeMap::new();
    packages.insert("default".into(), package);
    let global_config = GlobalConfig {
        helpers: Helpers::new(),
        packages,
    };
    debug!("Saving global config...");
    filesystem::save_file(global_config_path, global_config).context("save global config")?;

    let local_config = LocalConfig {
        includes: vec![],
        packages: vec!["default".into()],
        files: Files::default(),
        variables: Variables::default(),
    };
    trace!("Local config: {:#?}", local_config);
    filesystem::save_file(local_config_path, local_config).context("save local config")?;

    Ok(())
}

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Cache {
    pub symlinks: BTreeMap<PathBuf, PathBuf>,
    pub templates: BTreeMap<PathBuf, PathBuf>,
}

pub fn load_cache(cache: &Path) -> Result<Option<Cache>> {
    debug!("Loading cache...");

    let cache = match filesystem::load_file(cache) {
        Ok(cache) => Some(cache),
        Err(filesystem::FileLoadError::Open { .. }) => None,
        Err(e) => Err(e).context("load cache file")?,
    };

    trace!("Cache: {:#?}", cache);

    Ok(cache)
}

pub fn save_cache(cache_file: &Path, cache: Cache) -> Result<()> {
    debug!("Saving cache...");
    filesystem::save_file(cache_file, cache)?;

    Ok(())
}
