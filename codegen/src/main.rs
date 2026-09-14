//! 代码生成器 CLI：从域定义 JSON 生成 entity + 每域四件套。

mod def;
mod generate;
mod templates;
mod typing;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 || args[1] != "generate" {
        anyhow::bail!("用法: codegen generate <def.json> [root]（root 缺省为上一级，即项目根）");
    }
    let json = std::fs::read_to_string(&args[2])?;
    let def = def::DomainDef::from_json(&json)?;
    println!("定义校验通过: {} ({})", def.domain, def.table);

    // 生成到项目根（root 缺省为 codegen/ 的上一级；验证生成时传临时目录避免误覆盖手写模块）
    let root = args
        .get(3)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(".."));
    let files = generate::write_all(&def, &root)?;
    for f in &files {
        println!("生成: {f}");
    }
    println!(
        "提示: src/entity/mod.rs 加 `pub mod {};`；src/modules/{}/mod.rs 加 `pub mod {};`；src/modules/mod.rs 的 DOMAINS 登记表追加一行（挂 /{} 路由）",
        def.table, def.group, def.domain, def.domain
    );
    Ok(())
}
