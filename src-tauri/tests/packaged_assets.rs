//! Run with production protocol enabled, even when using Cargo's test profile.
//! This catches release binaries that still depend on the Vite dev server.
#[test]
#[ignore = "production packaging check: npm run test:desktop-assets"]
fn packaged_desktop_embeds_entry_and_referenced_assets() {
    assert!(!tauri::is_dev(), "发布程序仍使用开发服务器；构建时必须启用 custom-protocol");
    let context: tauri::Context<tauri::Wry> = tauri::generate_context!();
    let html = context.assets().get(&"index.html".into()).expect("发布程序未内嵌 index.html");
    let html = std::str::from_utf8(&html).expect("入口页面不是 UTF-8");
    assert!(!html.contains("localhost:1420") && !html.contains("/@vite/client"), "发布入口引用了开发服务器");
    let mut count = 0;
    for token in html.split('"') {
        if token.starts_with("/assets/") && (token.ends_with(".js") || token.ends_with(".css")) {
            assert!(context.assets().get(&token[1..].into()).is_some(), "发布程序缺少入口资源：{token}");
            count += 1;
        }
    }
    assert!(count >= 2, "入口未包含预期的 JS/CSS 发布资源");
}
