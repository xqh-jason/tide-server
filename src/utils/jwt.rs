//! JWT 签发与校验（jsonwebtoken 10.x，HS256）。
//!
//! 登录成功签发 JWT 并携带用户身份与角色，
//! 认证中间件解析后写入 Depot，后续接口不再重复查库取身份。

use chrono::Utc;
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};

/// JWT 载荷。`iat`/`exp` 使用标准 claim 名（jsonwebtoken 校验过期依赖 exp）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub user_id: u64,
    pub username: String,
    pub roles: Vec<String>,
    pub iat: i64,
    pub exp: i64,
}

/// 签发 JWT（过期时间取 Config.jwt.ttl_seconds，由调用方传入）。
pub fn sign(
    user_id: u64,
    username: &str,
    roles: &[String],
    secret: &str,
    ttl_seconds: i64,
) -> anyhow::Result<String> {
    let now = Utc::now().timestamp();
    let claims = Claims {
        user_id,
        username: username.to_string(),
        roles: roles.to_vec(),
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
    use crate::modules::permission::SUPER_ROLE_KEY;

    const SECRET: &str = "test-secret";

    #[test]
    fn sign_and_verify_roundtrip() {
        let token = sign(1, "admin", &[SUPER_ROLE_KEY.into()], SECRET, 7200).unwrap();
        let claims = verify(&token, SECRET).unwrap();
        assert_eq!(claims.user_id, 1);
        assert_eq!(claims.username, "admin");
        assert_eq!(claims.roles, vec![SUPER_ROLE_KEY]);
        assert!(claims.exp > claims.iat);
    }

    #[test]
    fn tampered_token_fails() {
        let token = sign(1, "admin", &[], SECRET, 7200).unwrap();
        // 篡改载荷段，签名必然失配
        let tampered = format!("{}x", &token[..token.len() - 4]);
        assert!(verify(&tampered, SECRET).is_err());
    }

    #[test]
    fn wrong_secret_fails() {
        let token = sign(1, "admin", &[], SECRET, 7200).unwrap();
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
}
