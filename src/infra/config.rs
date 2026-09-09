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
    /// 跨域访问控制（CORS）。缺省为拒绝所有跨源（allow_origins 为空）。
    #[serde(default)]
    pub cors: Cors,
}

fn default_env() -> String {
    "development".to_string()
}

/// CORS 白名单配置：只对 `allow_origins` 内的源返回跨域响应头，
/// 其余源的响应不含 CORS 头，浏览器会拦截（同源请求与 curl 等工具不受影响）。
#[derive(Debug, Clone, Deserialize)]
pub struct Cors {
    /// 允许的跨源来源（精确匹配请求 `Origin` 头，如 `http://localhost:5173`）。
    /// 空列表 = 拒绝所有跨源访问。
    #[serde(default)]
    pub allow_origins: Vec<String>,
    /// 允许的 HTTP 方法（大小写不敏感匹配）。
    #[serde(default = "default_allow_methods")]
    pub allow_methods: Vec<String>,
    /// 允许的请求头（预检 `Access-Control-Request-Headers` 白名单）。
    #[serde(default = "default_allow_headers")]
    pub allow_headers: Vec<String>,
    /// 是否允许携带凭据（Cookie / 客户端证书）。开启时 `allow_origins` 不能用 `*`，
    /// 本实现始终回显具体 origin，因此两者可安全共存。
    #[serde(default)]
    pub allow_credentials: bool,
    /// 预检请求结果缓存秒数（`Access-Control-Max-Age`）。
    #[serde(default = "default_max_age")]
    pub max_age: u64,
}

impl Default for Cors {
    fn default() -> Self {
        Self {
            allow_origins: vec![],
            allow_methods: default_allow_methods(),
            allow_headers: default_allow_headers(),
            allow_credentials: false,
            max_age: default_max_age(),
        }
    }
}

fn default_allow_methods() -> Vec<String> {
    ["GET", "POST", "PUT", "DELETE", "PATCH", "OPTIONS"]
        .into_iter()
        .map(String::from)
        .collect()
}

fn default_allow_headers() -> Vec<String> {
    ["Content-Type", "Authorization"]
        .into_iter()
        .map(String::from)
        .collect()
}

fn default_max_age() -> u64 {
    3600
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
