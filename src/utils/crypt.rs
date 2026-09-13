//! 哈希工具：密码（argon2id）与通用 SHA-256。
//!
//! 密码只暴露 `hash_password`（生成 PHC 字符串，可直接存库）与
//! `verify_password`（明文 + PHC 哈希校验）。盐由 OS 随机源生成，无需手动管理。
//! SHA-256 用于 refresh token 落库哈希（明文只存 HttpOnly Cookie）。

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

/// 计算十六进制 SHA-256（refresh token 落库哈希；密码不走此函数，用 argon2）。
pub fn sha256_hex(input: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    format!("{:x}", hasher.finalize())
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

    #[test]
    fn sha256_hex_matches_known_vector() {
        // 标准测试向量：空串与 "abc"
        assert_eq!(
            sha256_hex(""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
