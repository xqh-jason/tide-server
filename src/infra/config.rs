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
    /// 是否在控制台打印每条 SQL 语句（默认 false）。
    /// sea-orm 默认以 INFO 级别输出，会刷屏；调试时改为 true 即可。
    #[serde(default)]
    pub log_sql: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Jwt {
    pub secret: String,
    pub ttl_seconds: i64,
}

impl Config {
    pub fn load() -> anyhow::Result<Self> {
        let cfg: Config = config::Config::builder()
            .add_source(config::File::with_name("config"))
            .build()?
            .try_deserialize()?;
        if cfg.jwt.ttl_seconds <= 0 {
            anyhow::bail!("config.jwt.ttl_seconds 必须大于 0");
        }
        Ok(cfg)
    }
}
