//! W4 代码生成器 CLI：从域定义 JSON 生成 entity + 每域四件套。

mod def;
mod generate;
mod templates;
mod typing;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 || args[1] != "generate" {
        anyhow::bail!("用法: codegen generate <def.json>");
    }
    let json = std::fs::read_to_string(&args[2])?;
    let def = def::DomainDef::from_json(&json)?;
    println!("定义校验通过: {} ({})", def.domain, def.table);

    // 生成到项目根（codegen/ 的上一级）
    let files = generate::write_all(&def, std::path::Path::new(".."))?;
    for f in &files {
        println!("生成: {f}");
    }
    println!(
        "提示: src/modules/mod.rs 加 `pub mod {};`，src/infra/router.rs 挂 /{} 路由（AuthRequired）",
        def.domain, def.domain
    );
    Ok(())
}
