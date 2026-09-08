use serde::Deserialize;

/// 应用配置，从根目录 config.toml 加载。
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    /// 运行环境：`development` / `production`。开发种子数据（admin 弱口令重置）
    /// 仅在 `development` 下执行，缺省视为开发环境。
    #[serde(default = "default_env")]
    pub env: String,
    pub server: Server,
    pub database: Database,
    pub jwt: Jwt,
    pub upload: Upload,
}

fn default_env() -> String {
    "development".to_string()
}

#[derive(Debug, Clone, Deserialize)]
pub struct Server {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Upload {
    /// 文件落盘目录（相对进程工作目录或绝对路径均可）
    #[serde(default = "default_upload_dir")]
    pub dir: String,
    /// 单文件大小上限（MB）
    #[serde(default = "default_max_size_mb")]
    pub max_size_mb: u64,
    /// 扩展名白名单（小写、去点）；必填，缺失时启动即失败，避免静默拒绝所有上传
    pub allows: Vec<String>,
}

fn default_upload_dir() -> String {
    "./uploads".to_string()
}

fn default_max_size_mb() -> u64 {
    10
}

impl Upload {
    pub fn max_size_bytes(&self) -> u64 {
        self.max_size_mb * 1024 * 1024
    }
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
        if cfg.upload.max_size_mb == 0 {
            anyhow::bail!("config.upload.max_size_mb 必须大于 0");
        }
        if cfg.upload.dir.trim().is_empty() {
            anyhow::bail!("config.upload.dir 不能为空");
        }
        Ok(cfg)
    }
}
