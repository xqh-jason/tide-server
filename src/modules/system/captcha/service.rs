//! 验证码业务：生成（渲染 + 存 cache）与校验（一次性消费）。
//! 纯 cache 操作，不连数据库。

use std::time::Duration;

use captcha::Captcha;
use captcha::filters::{Noise, Wave};

use crate::modules::system::captcha::dto::CaptchaGenerateResp;
use crate::utils::cache::Cache;
use crate::utils::error::AppError;

/// cache key 前缀（与 token 黑名单等其他用途隔离）
const KEY_PREFIX: &str = "captcha:";
/// 答案有效期（秒）
const TTL_SECS: u64 = 180;
/// 验证码位数（4 位数字）
const CHARS: usize = 4;

/// 生成验证码：渲染 PNG → 存 cache → 返回 id 与裸 base64。
///
/// 注意：captcha crate 渲染的字符无法从外部读取（`chars` 私有），
/// 必须先生成答案、再逐位喂给渲染器（`set_chars(&[ch]) + add_chars(1)`），
/// 保证「图上字符 == 答案 == cache 里的值」三者严格一致。
pub fn generate_captcha(cache: &dyn Cache) -> Result<CaptchaGenerateResp, AppError> {
    // 1. 生成 4 位数字答案：uuid v4 前 4 字节各 % 10。
    //    不为此引入 rand 依赖——验证码答案对分布均匀性要求极低，取模偏差无安全影响。
    let uuid_val = uuid::Uuid::new_v4();
    let bytes = uuid_val.as_bytes();
    let answer: String = bytes[..CHARS]
        .iter()
        .map(|b| char::from(b'0' + b % 10))
        .collect();

    // 2. 渲染：候选池每次只放当前位的字符，add_chars(1) 必然选中它，
    //    四次循环后图上字符序列与 answer 完全相同；再加噪点与波浪干扰。
    let mut c = Captcha::new();
    for ch in answer.chars() {
        c.set_chars(&[ch]);
        c.add_chars(1);
    }
    c.apply_filter(Noise::new(0.3))
        .apply_filter(Wave::new(2.0, 10.0))
        .view(180, 44);

    // 3. 出图：as_base64 失败（字体缺字/编码异常，理论概率极低）按内部错误处理
    let image = c
        .as_base64()
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("验证码图像生成失败")))?;

    // 4. 存 cache：key 带前缀隔离，TTL 到期自动失效（MemoryCache 惰性过期）
    let captcha_id = uuid::Uuid::new_v4().simple().to_string();
    cache.set(
        &format!("{KEY_PREFIX}{captcha_id}"),
        answer,
        Duration::from_secs(TTL_SECS),
    );

    Ok(CaptchaGenerateResp { captcha_id, image })
}

/// 校验验证码：一次性消费——无论成败，校验后立即删除，防重放。
///
/// 未命中（未生成 / 过期）→ Biz("验证码已过期，请刷新")；
/// 答案不匹配（trim + 小写比对）→ Biz("验证码错误")。
pub fn verify_captcha(
    cache: &dyn Cache,
    captcha_id: &str,
    captcha_value: &str,
) -> Result<(), AppError> {
    let key = format!("{KEY_PREFIX}{captcha_id}");
    // 未命中：从没生成过，或已超 TTL 被惰性清理，统一按过期引导用户刷新
    let Some(expected) = cache.get(&key) else {
        return Err(AppError::Biz("验证码已过期，请刷新".into()));
    };
    // 取到即删（一次性消费）：错误答案也不能留下来被反复试探
    cache.remove(&key);
    // trim 容忍前后空格、小写化统一比较——数字集无感，为未来字母集预留行为
    if expected != captcha_value.trim().to_lowercase() {
        return Err(AppError::Biz("验证码错误".into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::cache::MemoryCache;

    /// 从 cache 直读答案（生成接口不回传答案，测试走存储层拿）。
    fn cached_answer(cache: &MemoryCache, captcha_id: &str) -> String {
        cache
            .get(&format!("{KEY_PREFIX}{captcha_id}"))
            .expect("答案应已入 cache")
    }

    #[test]
    fn generate_captcha_returns_png_base64_and_stores_answer() {
        let cache = MemoryCache::new();
        let resp = generate_captcha(&cache).unwrap();

        assert!(!resp.captcha_id.is_empty());
        assert!(resp.image.len() > 100, "base64 应为完整 PNG");
        assert!(resp.image.starts_with("iVBOR"), "PNG 的 base64 固定前缀");
        assert!(
            cache.exists(&format!("{KEY_PREFIX}{}", resp.captcha_id)),
            "答案应已入 cache"
        );
        assert_eq!(
            cached_answer(&cache, &resp.captcha_id).chars().count(),
            CHARS
        );
    }

    #[test]
    fn verify_correct_answer_passes_and_single_use() {
        let cache = MemoryCache::new();
        let resp = generate_captcha(&cache).unwrap();
        let answer = cached_answer(&cache, &resp.captcha_id);

        verify_captcha(&cache, &resp.captcha_id, &answer).unwrap();
        assert!(
            cache
                .get(&format!("{KEY_PREFIX}{}", resp.captcha_id))
                .is_none(),
            "一次性消费：验证通过后立即失效"
        );
        let second = verify_captcha(&cache, &resp.captcha_id, &answer);
        assert!(matches!(second, Err(AppError::Biz(ref m)) if m.contains("已过期")));
    }

    #[test]
    fn verify_wrong_answer_fails_and_consumes() {
        let cache = MemoryCache::new();
        let resp = generate_captcha(&cache).unwrap();
        let key = format!("{KEY_PREFIX}{}", resp.captcha_id);
        let answer = cached_answer(&cache, &resp.captcha_id);
        let wrong = if answer == "0000" { "1111" } else { "0000" };

        let first = verify_captcha(&cache, &resp.captcha_id, wrong);
        assert!(matches!(first, Err(AppError::Biz(ref m)) if m.contains("验证码错误")));
        assert!(cache.get(&key).is_none(), "失败同样消费，防反复试同一码");
    }

    #[test]
    fn verify_unknown_id_reports_expired() {
        let cache = MemoryCache::new();
        let err = verify_captcha(&cache, "ghost-id", "1234");
        assert!(matches!(err, Err(AppError::Biz(ref m)) if m.contains("已过期")));
    }

    #[test]
    fn verify_is_case_insensitive_and_trims() {
        let cache = MemoryCache::new();
        let resp = generate_captcha(&cache).unwrap();
        let answer = cached_answer(&cache, &resp.captcha_id);
        // 数字集下大小写无感，此用例锁定「trim + 小写」行为，为未来字母集留余地
        let padded = format!("  {}  ", answer.to_uppercase());
        verify_captcha(&cache, &resp.captcha_id, &padded).unwrap();
    }
}
