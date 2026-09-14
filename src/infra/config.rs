use serde::Deserialize;

/// 应用配置，从根目录 config.toml 加载。
///
/// 支持环境变量覆盖（容器化部署用）：`TIDE_` 前缀 + `__` 作层级分隔符，
/// 例如 `TIDE_DATABASE__URL` → `database.url`、`TIDE_JWT__SECRET` → `jwt.secret`、
/// `TIDE_ENV` → `env`；列表类取值用逗号分隔（`TIDE_CORS__ALLOW_ORIGINS=a,b`）。
/// 环境变量优先级高于 config.toml，未设置的字段仍取配置文件默认值。
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    /// 运行环境：`development` / `production`。开发种子数据（admin 弱口令重置）
    /// 仅在 `development` 下执行，缺省视为开发环境。
    #[serde(default = "default_env")]
    pub env: String,
    /// 启动种子开关（部署补充）：生产环境默认关闭；首次部署置
    /// `seed.enabled = true`（或 `TIDE_SEED__ENABLED=true`）一次性创建初始
    /// admin / super 角色 / 基础 RBAC，完成后应关闭并立即改密。
    /// `development` 环境恒执行，不受此开关影响。
    #[serde(default)]
    pub seed: Seed,
    pub server: Server,
    pub database: Database,
    pub jwt: Jwt,
    pub upload: Upload,
    /// 跨域访问控制（CORS）。缺省为拒绝所有跨源（allow_origins 为空）。
    #[serde(default)]
    pub cors: Cors,
}

/// 启动种子开关（见 [`Config::seed`]）。
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Seed {
    /// 是否在启动时执行幂等种子初始化（仅影响非 development 环境）。
    #[serde(default)]
    pub enabled: bool,
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
    /// access token 有效期（秒）；配合 refresh token 使用，建议小时级
    pub ttl_seconds: i64,
    /// refresh token（会话）有效期（秒）；缺省 7 天，兼容未加字段的旧 config.toml
    #[serde(default = "default_refresh_ttl_seconds")]
    pub refresh_ttl_seconds: i64,
}

/// `refresh_ttl_seconds` 缺省值：7 天。
fn default_refresh_ttl_seconds() -> i64 {
    604_800
}

/// 已知开发默认密钥：production 下命中即拒绝启动。公开弱密钥意味着任何人
/// 可自签合法 token；发版漏设 `TIDE_JWT__SECRET` 时静默回退是真实事故路径
/// （spec：2026-09-14-jwt-secret-production-guard-design.md）。
const KNOWN_DEV_SECRETS: [&str; 1] = ["dev-secret-change-me"];

/// production 环境禁用默认开发密钥（fail-fast）。抽成纯函数是为了可测性：
/// std::env 是进程全局资源，测试篡改 `TIDE_ENV` 会污染并行用例的
/// `Config::load()`（fail-fast 生效后连库测试会意外失败）。
fn ensure_production_secret(env: &str, secret: &str) -> anyhow::Result<()> {
    if env == "production" && KNOWN_DEV_SECRETS.contains(&secret) {
        anyhow::bail!(
            "production 环境禁止使用默认开发密钥 jwt.secret，\
             请设置专用密钥（TIDE_JWT__SECRET 或 config 覆盖）"
        );
    }
    Ok(())
}

impl Config {
    pub fn load() -> anyhow::Result<Self> {
        let cfg: Config = config::Config::builder()
            .add_source(config::File::with_name("config"))
            .add_source(
                config::Environment::with_prefix("TIDE")
                    // 前缀用单下划线收尾（TIDE_），嵌套层用 __ 分隔（TIDE_DATABASE__URL）
                    .prefix_separator("_")
                    .separator("__")
                    // 逗号分隔列表仅对白名单字段生效，其余环境变量保持字符串/自动类型
                    .list_separator(",")
                    .with_list_parse_key("cors.allow_origins")
                    .try_parsing(true),
            )
            .build()?
            .try_deserialize()?;
        if cfg.jwt.ttl_seconds <= 0 {
            anyhow::bail!("config.jwt.ttl_seconds 必须大于 0");
        }
        if cfg.jwt.refresh_ttl_seconds <= 0 {
            anyhow::bail!("config.jwt.refresh_ttl_seconds 必须大于 0");
        }
        if cfg.upload.max_size_mb == 0 {
            anyhow::bail!("config.upload.max_size_mb 必须大于 0");
        }
        if cfg.upload.dir.trim().is_empty() {
            anyhow::bail!("config.upload.dir 不能为空");
        }
        ensure_production_secret(&cfg.env, &cfg.jwt.secret)?;
        Ok(cfg)
    }
}

#[cfg(test)]
// 测试需操纵进程环境变量验证覆盖映射；edition 2024 中 set_var/remove_var 为 unsafe。
// 仅测试模块放行，生产代码仍受 unsafe_code 约束。
#[allow(unsafe_code)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    /// 进程环境变量是全局共享资源：本模块两个改 env 的测试互斥执行，
    /// 避免并行线程间相互踩踏。
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn lock_env() -> MutexGuard<'static, ()> {
        ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// 环境变量快照守卫：构造时记录原始值，Drop（含断言失败 panic 展开）时
    /// 精确还原——存在则写回、不存在则移除。
    ///
    /// 背景（2026-09-13 CI 排障）：旧写法进入即清变量、收尾无条件 remove_var，
    /// 会把 CI 经 TIDE_DATABASE__URL 注入的测试库连接串一并清掉，同进程后续
    /// 所有 Config::load 回落 config.toml（CI 上 localhost:3307 不可达），导致
    /// 全量连库测试固定 30s PoolTimedOut，且本地（3307 可达）无法复现。
    /// 测试改写进程级 env 必须可还原。
    struct EnvGuard(Vec<(&'static str, Option<String>)>);

    impl EnvGuard {
        fn snapshot<const N: usize>(keys: [&'static str; N]) -> Self {
            EnvGuard(keys.map(|key| (key, std::env::var(key).ok())).into())
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            for (key, original) in &self.0 {
                unsafe {
                    match original {
                        Some(value) => std::env::set_var(key, value),
                        None => std::env::remove_var(key),
                    }
                }
            }
        }
    }

    /// TIDE_ 环境变量覆盖 config.toml（单函数串行，避免 std::env 的进程级竞态；
    /// 结束时经 EnvGuard 精确还原原始快照，不残留、不冲掉 CI 注入的值）。
    #[test]
    fn env_overrides_override_file_fields() {
        let _guard = lock_env();
        const KEYS: [&str; 4] = [
            "TIDE_DATABASE__URL",
            "TIDE_JWT__SECRET",
            "TIDE_ENV",
            "TIDE_CORS__ALLOW_ORIGINS",
        ];

        // 覆盖阶段：快照原始值，结束（含 panic）由守卫还原
        {
            let _env = EnvGuard::snapshot(KEYS);

            // DB URL 覆盖值取 config.toml 默认（同进程并行测试读到的行为不变）
            let base = Config::load().unwrap();
            let default_url = base.database.url.clone();

            unsafe {
                std::env::set_var("TIDE_DATABASE__URL", &default_url);
                std::env::set_var("TIDE_JWT__SECRET", "env-secret-test");
                std::env::set_var("TIDE_ENV", "production");
                std::env::set_var(
                    "TIDE_CORS__ALLOW_ORIGINS",
                    "http://a.example,http://b.example",
                );
            }

            let cfg = Config::load().unwrap();
            assert_eq!(cfg.database.url, default_url, "URL 覆盖值生效");
            assert_eq!(cfg.jwt.secret, "env-secret-test", "secret 被环境变量覆盖");
            assert!(cfg.jwt.ttl_seconds > 0, "未覆盖字段沿用配置文件");
            assert_eq!(cfg.env, "production", "env 档位被环境变量覆盖");
        }

        // 还原阶段：守卫已按快照恢复（CI 恢复注入的连接串，本地未设置则保持
        // 未设置），未覆盖字段回落配置文件
        let restored = Config::load().unwrap();
        assert_eq!(
            restored.jwt.secret, "dev-secret-change-me",
            "还原后回落配置文件"
        );
        assert_eq!(restored.env, "development");
    }

    /// 数字字段经 try_parsing 自动识别为整数（而非字符串），覆盖类型转换路径。
    /// 注：故意不测"非法值应失败"——向进程级环境变量注入坏值会污染同进程并行
    /// 测试（如 seed 域经 Config::load 连库），代价高于收益。
    #[test]
    fn env_numeric_ttl_parses_to_integer() {
        let _guard = lock_env();
        let _env = EnvGuard::snapshot(["TIDE_JWT__TTL_SECONDS"]);
        unsafe { std::env::set_var("TIDE_JWT__TTL_SECONDS", "123") };
        let cfg = Config::load().unwrap();
        assert_eq!(cfg.jwt.ttl_seconds, 123, "环境变量数字应解析为整数");
    }

    // ===== production 禁用默认开发密钥（fail-fast）=====
    // 纯函数直测：不走 env 篡改路径，避免污染并行用例的 Config::load()

    /// production 命中已知开发默认密钥必须拒绝：公开弱密钥意味着任何人
    /// 可自签合法 token（spec：2026-09-14-jwt-secret-production-guard-design.md）。
    #[test]
    fn production_with_known_dev_secret_is_rejected() {
        let err = ensure_production_secret("production", "dev-secret-change-me").unwrap_err();
        assert!(
            err.to_string().contains("jwt.secret"),
            "错误应提示设置专用密钥，实际：{err}"
        );
    }

    /// production + 专用自定义密钥放行，fail-fast 不误伤。
    #[test]
    fn production_with_custom_secret_is_accepted() {
        ensure_production_secret("production", "prod-only-secret-not-in-dev-list").unwrap();
    }

    /// development 沿用开发默认密钥不受影响（本地开发无需额外配置）。
    #[test]
    fn development_with_dev_secret_is_accepted() {
        ensure_production_secret("development", "dev-secret-change-me").unwrap();
    }
}
