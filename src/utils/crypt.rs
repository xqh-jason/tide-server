//! 密码哈希（argon2）：注册/登录共用。
//!
//! 只暴露两个函数：`hash_password`（生成 PHC 字符串，可直接存库）与
//! `verify_password`（明文 + PHC 哈希校验）。盐由 OS 随机源生成，无需手动管理。

use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng},
};

/// 对明文密码做 argon2id 哈希，返回 PHC 字符串（如 `$argon2id$v=19$m=19456,t=2,p=1$...`）。
pub fn hash_password(password: &str) -> anyhow::Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| anyhow::anyhow!("password hash failed: {e}"))
}

/// 校验明文密码与存储的 PHC 哈希是否匹配（失败统一返回 false，不暴露错误细节）。
pub fn verify_password(password: &str, password_hash: &str) -> bool {
    PasswordHash::new(password_hash)
        .map(|parsed| {
            Argon2::default()
                .verify_password(password.as_bytes(), &parsed)
                .is_ok()
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_and_verify_roundtrip() {
        let hash = hash_password("admin123").unwrap();
        assert!(
            hash.starts_with("$argon2id$"),
            "哈希应为 argon2id PHC 格式: {hash}"
        );
        assert!(verify_password("admin123", &hash));
    }

    #[test]
    fn wrong_password_fails() {
        let hash = hash_password("admin123").unwrap();
        assert!(!verify_password("wrong", &hash));
    }

    #[test]
    fn same_password_different_salt() {
        let a = hash_password("admin123").unwrap();
        let b = hash_password("admin123").unwrap();
        assert_ne!(a, b, "每次哈希应使用不同盐，密文不得相同");
    }

    #[test]
    fn malformed_hash_fails() {
        assert!(!verify_password("admin123", "not-a-phc-hash"));
    }
}
