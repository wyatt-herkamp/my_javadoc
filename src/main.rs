pub(crate) mod web;
pub(crate) mod project_processor;
pub(crate) mod single;
pub(crate) mod multi;
pub(crate) mod repository;
pub(crate) mod project;

use std::collections::HashMap;
use std::env::{current_dir, set_var};
use std::fs::read_to_string;
use std::io::ErrorKind;
use std::path::PathBuf;
use std::process::exit;
use serde::{Deserialize, Serialize};
use clap::{Parser, Subcommand};
use log::info;
use maven_rs::quick_xml::DeError;
use nitro_log::LoggerBuilders;
use rust_embed::RustEmbed;
use this_actix_error::ActixError;
use thiserror::Error;

static CONFIG: &str = "my_javadoc.toml";

#[derive(RustEmbed)]
#[folder = "$CARGO_MANIFEST_DIR/resources"]
pub struct Resources;


#[derive(Debug, Serialize, Deserialize)]
pub struct Config {
    pub bind_address: String,
    pub cache: PathBuf,
    /// If true only one repository can be used
    pub single_repo: bool,
    pub repositories: HashMap<String, ConfigRepository>,
    pub log_location: Option<PathBuf>,
    #[cfg(feature = "ssl")]
    pub ssl_private_key: Option<PathBuf>,
    #[cfg(feature = "ssl")]
    pub ssl_cert_key: Option<PathBuf>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ConfigRepository {
    pub address: String,
    /// Does this repository allow redeploy of artifacts
    /// If true every 24 hours a query to update the cache is made
    #[serde(default)]
    pub allows_redeploy: bool,

}

#[derive(Debug, Serialize, Deserialize)]
pub struct SiteSettings {
    pub title: String,
    pub description: String,
}

#[derive(Parser)]
#[clap(author, version, about, long_about = None)]
pub struct MyJavaDoc {
    #[clap(subcommand)]
    command: MyJavaDocSubCommand,
}

#[derive(Subcommand)]
enum MyJavaDocSubCommand {
    /// Run the server
    Run,

}

#[derive(Debug, Error, ActixError)]
pub enum Error {
    #[error(transparent)]
    IO(#[from] std::io::Error),
    #[error(transparent)]
    Reqwest(#[from] reqwest::Error),
    #[error(transparent)]
    Maven(#[from] maven_rs::Error),
    #[error(transparent)]
    Serde(#[from] serde_json::Error),
    #[error(transparent)]
    XMLError(#[from] DeError),
}


#[tokio::main]
async fn main() -> std::io::Result<()> {
    let args: MyJavaDoc = MyJavaDoc::parse();
    let main_config = current_dir()?.join(CONFIG);
    if !main_config.exists() {
        println!("Config file not found");

        let config = Config {
            bind_address: "127.0.0.1:9090".to_string(),
            repositories: HashMap::new(),
            cache: current_dir()?.join("cache"),
            single_repo: false,
            log_location: None,
        };
        let config = toml::to_string_pretty(&config).unwrap();
        std::fs::write(&main_config, config)?;
    }
    let init_settings: Config =
        toml::from_str(&read_to_string(&main_config)?).map_err(|v| std::io::Error::new(ErrorKind::InvalidData, v))?;
    if init_settings.single_repo {
        if init_settings.repositories.len() == 1 {
            println!("Single Repo is set to true but more than one repo is set");
            exit(1);
        }
    }
    if let Some(log_location) = init_settings.log_location.as_ref() {
        set_var("LOG_LOCATION", log_location.as_os_str());
    } else {
        set_var("LOG_LOCATION", current_dir().unwrap().join("logs").as_os_str());
    }
    match args.command {
        MyJavaDocSubCommand::Run => {
            let logger: nitro_log::config::Config = serde_json::from_slice(Resources::get("log.json").unwrap().data.as_ref()).unwrap();
            nitro_log::NitroLogger::load(logger, LoggerBuilders::default()).unwrap();
            info!("Starting server");
            web::start(init_settings).await?;
        }
    }
    Ok(())
}