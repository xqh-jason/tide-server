//! JWT 签发与校验（jsonwebtoken 10.x，HS256）。
//!
//! 登录成功签发 JWT 并携带用户身份与角色，
//! 认证中间件解析后写入 Depot，后续接口不再重复查库取身份。

use chrono::Utc;
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};

/// JWT 载荷。`iat`/`exp` 使用标准 claim 名（jsonwebtoken 校验过期依赖 exp）。
///
/// `refresh_token_id` 指向 `sys_refresh_token`（登录刷新凭证记录），认证中间件据此做会话有效性合并查询；
/// 旧版 token 无此字段，反序列化自然失败 → 发版后全员重登（预期迁移路径）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub user_id: u64,
    pub username: String,
    pub roles: Vec<String>,
    pub refresh_token_id: u64,
    pub iat: i64,
    pub exp: i64,
}

/// 签发 JWT（过期时间取 Config.jwt.ttl_seconds，由调用方传入）。
pub fn sign(
    user_id: u64,
    username: &str,
    roles: &[String],
    refresh_token_id: u64,
    secret: &str,
    ttl_seconds: i64,
) -> anyhow::Result<String> {
    let now = Utc::now().timestamp();
    let claims = Claims {
        user_id,
        username: username.to_string(),
        roles: roles.to_vec(),
        refresh_token_id,
        iat: now,
        exp: now + ttl_seconds,
    };
    encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|e| anyhow::anyhow!("jwt sign failed: {e}"))
}

/// 校验 JWT（签名 + 过期），返回载荷。
pub fn verify(token: &str, secret: &str) -> anyhow::Result<Claims> {
    decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::new(Algorithm::HS256),
    )
    .map(|data| data.claims)
    .map_err(|e| anyhow::anyhow!("jwt verify failed: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::system::permission::SUPER_ROLE_KEY;

    const SECRET: &str = "test-secret";

    #[test]
    fn sign_and_verify_roundtrip() {
        let token = sign(1, "admin", &[SUPER_ROLE_KEY.into()], 42, SECRET, 7200).unwrap();
        let claims = verify(&token, SECRET).unwrap();
        assert_eq!(claims.user_id, 1);
        assert_eq!(claims.username, "admin");
        assert_eq!(claims.roles, vec![SUPER_ROLE_KEY]);
        assert_eq!(
            claims.refresh_token_id, 42,
            "refresh_token_id 应进入载荷并原样往返"
        );
        assert!(claims.exp > claims.iat);
    }

    #[test]
    fn tampered_token_fails() {
        let token = sign(1, "admin", &[], 1, SECRET, 7200).unwrap();
        // 篡改载荷段，签名必然失配
        let tampered = format!("{}x", &token[..token.len() - 4]);
        assert!(verify(&tampered, SECRET).is_err());
    }

    #[test]
    fn wrong_secret_fails() {
        let token = sign(1, "admin", &[], 1, SECRET, 7200).unwrap();
        assert!(verify(&token, "another-secret").is_err());
    }

    #[test]
    fn expired_token_fails() {
        // 手工构造一个已过期的 token
        let now = Utc::now().timestamp();
        let claims = Claims {
            user_id: 1,
            username: "admin".into(),
            roles: vec![],
            refresh_token_id: 1,
            iat: now - 200,
            exp: now - 100,
        };
        let token = encode(
            &Header::new(Algorithm::HS256),
            &claims,
            &EncodingKey::from_secret(SECRET.as_bytes()),
        )
        .unwrap();
        assert!(verify(&token, SECRET).is_err());
    }

    #[test]
    fn legacy_token_without_refresh_token_id_fails() {
        // 旧版载荷（无 refresh_token_id 字段）：反序列化必须失败 → 发版后旧 token 全员失效，
        // 全员重登即迁移路径（无需兼容双轨）
        let now = Utc::now().timestamp();
        let legacy = serde_json::json!({
            "user_id": 1, "username": "admin", "roles": [], "iat": now, "exp": now + 3600
        });
        let token = encode(
            &Header::new(Algorithm::HS256),
            &legacy,
            &EncodingKey::from_secret(SECRET.as_bytes()),
        )
        .unwrap();
        assert!(
            verify(&token, SECRET).is_err(),
            "缺 refresh_token_id 的旧 token 应被拒绝"
        );
    }
}
