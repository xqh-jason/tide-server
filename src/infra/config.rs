use serde::Deserialize;

/// 应用配置，从根目录 config.toml 加载。
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub server: Server,
    pub database: Database,
    pub jwt: Jwt,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Server {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Database {
    pub url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Jwt {
    pub secret: String,
    pub ttl_seconds: i64,
}

impl Config {
    pub fn load() -> anyhow::Result<Self> {
        let cfg = config::Config::builder()
            .add_source(config::File::with_name("config"))
            .build()?
            .try_deserialize()?;
        Ok(cfg)
    }
}
