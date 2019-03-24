use std::io;
use std::fs;

use std::fs::File;
use std::io::prelude::*;
use std::path::PathBuf;
use std::collections::HashMap;

pub struct Config {
    value: Option<toml::Value>,
    storage: StorageConfig,
    programs: HashMap<String, Vec<String>>,
}

pub struct StorageConfig {
    default: String,
    storages: HashMap<String, PathBuf>,
}

#[derive(Debug)]
pub enum ConfigError {
    Read(io::Error),
    Parse(toml::de::Error),
    InvalidStorage(String),
    NoStorage,
    NoStorages,
    NoDefaultStorage,
    InvalidPrograms,
    InvalidDefaultStorage
}

// TODO: how is config usually handled? when e.g. config file does only
// set programs but doesn't include any storages should be still use
// default storages? or distribute (and install) default config file?
impl Config {
    /// Load the configuration from the default location.
    /// Will return the default configuration if the file in
    /// the default location does not exist.
    /// Will only fail if the config file is invalid.
    pub fn load_default() -> Result<Config, ConfigError> {
        use toml::value::Value;

        // when config file doesn't exist, return default config
        let mut f = match File::open(Config::config_path()) {
            Ok(f) => f,
            Err(_) => return Ok(Config::default_config())
        };

        let mut s = String::new();
        if let Err(e) = f.read_to_string(&mut s) {
            return Err(ConfigError::Read(e));
        }

        let mut config: Value = match toml::from_str(&s) {
            Ok(c) => c,
            Err(e) => return Err(ConfigError::Parse(e)),
        };

        // storage
        let storage = match config.get_mut("storage").
                map(Config::parse_storage_config) {
            Some(Ok(s)) => s,
            Some(Err(e)) => return Err(e),
            None => return Err(ConfigError::NoStorage),
        };


        // programs
        let programs = match config.get("programs") {
            Some(t) => match t.clone().try_into() {
                Ok(v) => v,
                Err(_) => return Err(ConfigError::InvalidPrograms),
            }, None => HashMap::new(),
        };

        Ok(Config{
            value: Some(config),
            programs: programs,
            storage: storage})
    }

    pub fn config_folder() -> PathBuf {
        let mut p = dirs::config_dir().unwrap();
        p.push("nodes");
        p
    }

    pub fn config_path() -> PathBuf {
        let mut p = Config::config_folder();
        p.push("config");
        p
    }

    /// Returns the path of the storage with the given name, if present.
    pub fn storage_folder(&self, name: &str) -> Option<&PathBuf> {
        self.storage.storages.get(name)
    }

    /// Returns the path of the default storage.
    pub fn default_storage_folder(&self) -> &PathBuf {
        self.storage_folder(&self.storage.default).unwrap()
    }

    /// Returns the parsed config file as value
    pub fn value(&self) -> &Option<toml::Value> {
        &self.value
    }

    fn parse_storage_config(storage_val: &mut toml::Value)
            -> Result<StorageConfig, ConfigError> {
        use toml::value::Value;
        let storage: &mut toml::value::Table = match storage_val.as_table_mut() {
            Some(s) => s,
            None => return Err(
                ConfigError::InvalidStorage("Not a table".into()))
        };

        let mut default = match storage.get("default") {
            Some(Value::String(d)) => Some(d.clone()),
            None => None,
            _ => return Err(ConfigError::InvalidDefaultStorage),
        };

        if storage.len() == 0 {
            return Err(ConfigError::NoStorages);
        } else if default.is_none() && storage.len() != 1 {
            return Err(ConfigError::NoDefaultStorage);
        }

        if default.is_none() {
            // choose the only storage as default
            default = Some(storage.keys().next().unwrap().to_string());
        } else {
            storage.remove("default").unwrap();
        }

        let paths: HashMap<String, PathBuf> = match storage_val.clone().try_into() {
            Ok(p) => p,
            Err(err) => return Err(ConfigError::InvalidStorage(
                format!("Could not convert to hashmap: {}", err)))
        };

        let default = default.unwrap();
        if !paths.contains_key(&default) {
            return Err(ConfigError::InvalidDefaultStorage);
        }

        Ok(StorageConfig {
            default: default.clone(),
            storages: paths,
        })
    }

    fn default_config() -> Config {
        let mut storages = HashMap::new();

        // we make sure that the default storage exists
        // when running nodes for the first time this assures that
        // it can already be used
        let storage = Config::default_storage_path();
        if !storage.is_dir() {
            fs::create_dir_all(&storage)
                .expect("Failed to create default storage path");
        }

        storages.insert("default".to_string(), storage);
        Config {
            value: None,
            programs: HashMap::new(),
            storage: StorageConfig {
                default: "default".to_string(),
                storages,
            }
        }
    }

    fn default_storage_path() -> PathBuf {
        let mut p = dirs::data_local_dir().unwrap();
        p.push("nodes");
        p
    }
}
