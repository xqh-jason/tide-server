# 安全政策

## 支持的版本

本项目处于持续开发阶段，只为 `main` 分支上的最新提交提供安全修复。

## 报告安全漏洞

**请不要通过公开 Issue 报告安全漏洞。**

优先使用 GitHub 的私下报告通道：仓库页面 → **Security** 标签 → **Report a vulnerability**
（即 [Private vulnerability reporting](https://docs.github.com/code-security/security-advisories/guidance-on-reporting-and-writing-information-about-vulnerabilities/privately-reporting-a-security-vulnerability)）。
若该通道不可用，也可直接发邮件至 **1325072898@qq.com**。

报告里请尽量包含：

- 受影响的端点或文件（含行号更佳）
- 复现步骤（可直接复制的 `curl` 命令最有价值）
- 影响范围与利用前提（是否需要已登录、需要什么角色）

## 我们的响应

- 3 个工作日内确认收到
- 修复发布前不公开细节，修复后在本仓库披露并致谢（除非你希望匿名）

## 部署方的安全责任

本项目是**应用后端**，默认配置面向本地开发（详见 README「Docker 一键部署」）。部署到公网前请自行确认：

- `TIDE_ENV` 显式设为 `production`，且 `TIDE_JWT__SECRET` 为强随机串 ——
  未设为开发档位时程序会拒绝以弱密钥启动，但**密钥强度由你负责**
- MySQL 不使用默认口令，且不对外网暴露端口（`docker-compose.yml` 把 3307 映射到宿主机仅为本机开发复用）
- 首次部署 bootstrap 后立即修改 `admin` 初始口令，并关闭 `TIDE_SEED__ENABLED`
- 反向代理层启用 HTTPS；`TIDE_CORS__ALLOW_ORIGINS` 只列真实前端来源

## 已知的非目标

以下不属于安全漏洞，请走普通 Issue：

- 开发档位（`TIDE_ENV=development`）下的弱口令种子与默认密钥 —— 这是刻意设计
- 契约决定「HTTP 恒 200、仅认证失败 401」（见 README），因此业务错误回 200 不是缺陷
- 缺少速率限制 / WAF / 审计合规等由部署层承担的能力
