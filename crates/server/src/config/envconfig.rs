use std::path::Path;

use ::config as config_rs;
use anyhow::{Context, Result};
use serde::de::DeserializeOwned;

pub trait EnvConfig: Sized + DeserializeOwned {
    const PREFIX: &'static str = "APP";
    const SEPARATOR: &'static str = "__";

    fn load_dotenv() {
        // Load .env from crate root (falls back to current dir if missing)
        let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
        let _ = dotenvy::from_filename(manifest_dir.join(".env")).or_else(|_| dotenvy::dotenv());
    }

    fn validate(&self) -> Result<()> {
        Ok(())
    }

    fn from_env() -> Result<Self> {
        Self::load_dotenv();

        let settings = config_rs::Config::builder()
            .add_source(
                config_rs::Environment::with_prefix(Self::PREFIX)
                    .prefix_separator("_")
                    .separator(Self::SEPARATOR)
                    .try_parsing(true),
            )
            .build()
            .context("failed to read environment variables for config")?;

        let cfg = settings
            .try_deserialize::<Self>()
            .context("failed to deserialize environment into config")?;

        cfg.validate()?;
        Ok(cfg)
    }
}
