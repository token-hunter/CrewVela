use super::native::NativePluginReceipt;
use super::*;
use sha2::{Digest, Sha256};
use std::process::Stdio;
use std::time::Duration;

pub(super) fn run_codex(args: &[String]) -> Result<Value> {
    let executable =
        crate::multi_agent::resolve_runtime_executable(crate::multi_agent::AgentRuntimeKind::Codex)
            .ok_or_else(|| anyhow!("未检测到 Codex 原生 CLI"))?;
    #[cfg(windows)]
    if !executable
        .extension()
        .is_some_and(|value| value.eq_ignore_ascii_case("exe"))
    {
        bail!("Codex 插件安装需要原生 exe，不使用 shell shim。请更新 Codex CLI");
    }
    let mut command = Command::new(executable);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    super::native::run_json_command(command, Duration::from_secs(90))
        .map_err(|error| anyhow!("Codex 插件操作未完成：{error}"))
}

fn config_root() -> Result<PathBuf> {
    let root = std::env::var_os("CODEX_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".codex")))
        .ok_or_else(|| anyhow!("无法确定 CODEX_HOME"))?;
    if !root.is_absolute() {
        bail!("CODEX_HOME 必须为绝对路径");
    }
    Ok(root)
}

fn list_args() -> Vec<String> {
    vec!["plugin".into(), "list".into(), "--json".into()]
}
fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value != "."
        && value != ".."
        && !value.starts_with('-')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

fn installed_entry<'a>(list: &'a Value, selector: &str) -> Result<Option<&'a Value>> {
    let entries = list
        .get("installed")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("Codex 插件列表格式不兼容"))?;
    let matching = entries
        .iter()
        .filter(|entry| entry.get("pluginId").and_then(Value::as_str) == Some(selector))
        .collect::<Vec<_>>();
    if matching.len() > 1 {
        bail!("Codex 返回重复插件记录，拒绝认领");
    }
    Ok(matching.first().copied())
}

pub(super) fn content_hash(root: &Path) -> Result<String> {
    fn walk(root: &Path, current: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
        for entry in std::fs::read_dir(current)? {
            let entry = entry?;
            if entry.file_name() == ".git" {
                continue;
            }
            let kind = entry.file_type()?;
            if kind.is_symlink() {
                bail!("原生插件缓存包含链接，无法安全核验");
            }
            if kind.is_dir() {
                walk(root, &entry.path(), files)?;
            } else if kind.is_file() {
                files.push(entry.path().strip_prefix(root)?.to_path_buf());
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    walk(root, root, &mut files)?;
    files.sort();
    let mut hasher = Sha256::new();
    for path in files {
        let name = path.to_string_lossy().replace('\\', "/");
        hasher.update((name.len() as u64).to_le_bytes());
        hasher.update(name.as_bytes());
        let bytes = std::fs::read(root.join(path))?;
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(bytes);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn verify(receipt: &NativePluginReceipt, list: &Value, home: &Path) -> Result<bool> {
    if receipt.config_root.as_deref() != Some(home.display().to_string().as_str()) {
        bail!("CODEX_HOME 已变化，拒绝操作另一配置的插件");
    }
    let Some(entry) = installed_entry(list, &receipt.selector)? else {
        return Ok(false);
    };
    if !receipt.verified {
        bail!("Codex 安装结果未核验，不能自动认领或删除；请在 Codex 插件管理中处理");
    }
    if entry.get("version").and_then(Value::as_str) != receipt.version.as_deref()
        || entry.get("source") != receipt.source.as_ref()
        || entry.get("marketplaceSource") != receipt.marketplace_source.as_ref()
    {
        bail!("Codex 插件来源或版本已变化，拒绝卸载外部安装");
    }
    let path = receipt
        .install_path
        .as_deref()
        .ok_or_else(|| anyhow!("安装记录缺少路径"))?;
    let root = ensure_within(&home.join("plugins/cache"), Path::new(path))?;
    if Some(content_hash(&root)?) != receipt.content_hash {
        bail!("Codex 插件内容已变化，拒绝卸载修改后的包");
    }
    Ok(true)
}

fn receipt_from_add(
    add: &Value,
    list: &Value,
    selector: &str,
    name: &str,
    marketplace: &str,
    home: &Path,
    expected_source: &Path,
) -> Result<NativePluginReceipt> {
    let entry =
        installed_entry(list, selector)?.ok_or_else(|| anyhow!("Codex 未返回精确安装记录"))?;
    if add.get("pluginId").and_then(Value::as_str) != Some(selector)
        || add.get("name").and_then(Value::as_str) != Some(name)
        || add.get("marketplaceName").and_then(Value::as_str) != Some(marketplace)
        || entry.get("name").and_then(Value::as_str) != Some(name)
        || entry.get("marketplaceName").and_then(Value::as_str) != Some(marketplace)
        || add.get("version") != entry.get("version")
    {
        bail!("Codex 安装回执标识或版本不匹配");
    }
    let path = add
        .get("installedPath")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("Codex 安装回执缺少 installedPath"))?;
    let root = ensure_within(&home.join("plugins/cache"), Path::new(path))?;
    let version = add
        .get("version")
        .and_then(Value::as_str)
        .filter(|version| valid_identifier(version))
        .ok_or_else(|| anyhow!("Codex 插件版本缺失或不安全"))?;
    let expected_path = home
        .join("plugins/cache")
        .join(marketplace)
        .join(name)
        .join(version);
    if expected_path.canonicalize()? != root {
        bail!("Codex 安装缓存路径不匹配所选市场、名称与版本");
    }
    let (_, manifest) = compatibility::read_manifest(&root)?;
    let manifest = manifest.ok_or_else(|| anyhow!("Codex 原生缓存缺少插件 manifest"))?;
    if manifest.get("name").and_then(Value::as_str) != Some(name)
        || manifest.get("version") != add.get("version")
    {
        bail!("Codex 缓存 manifest 与回执不符");
    }
    let source = entry
        .get("source")
        .cloned()
        .ok_or_else(|| anyhow!("Codex 列表缺少插件来源"))?;
    let source_path = source
        .get("path")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("Codex 本地插件来源缺少路径"))?;
    if source.get("source").and_then(Value::as_str) != Some("local")
        || Path::new(source_path).canonicalize()? != expected_source.canonicalize()?
    {
        bail!("Codex 插件来源不是所选官方缓存目录，拒绝认领");
    }
    let marketplace_source = entry.get("marketplaceSource").cloned();
    Ok(NativePluginReceipt {
        selector: selector.into(),
        scope: "global".into(),
        install_path: Some(root.display().to_string()),
        verified: true,
        version: add
            .get("version")
            .and_then(Value::as_str)
            .map(str::to_string),
        installed_at: None,
        last_updated: None,
        runtime: Some("codex".into()),
        source: Some(source),
        marketplace_source,
        content_hash: Some(content_hash(&root)?),
        config_root: Some(home.display().to_string()),
    })
}

pub(super) fn install_at(
    paths: &Paths,
    center: &SkillCenter,
    input: &targets::PluginInstallInput,
    run: &mut impl FnMut(&[String]) -> Result<Value>,
) -> Result<InstalledPlugin> {
    install_at_home(paths, center, input, &config_root()?, run)
}

fn install_at_home(
    paths: &Paths,
    center: &SkillCenter,
    input: &targets::PluginInstallInput,
    home: &Path,
    run: &mut impl FnMut(&[String]) -> Result<Value>,
) -> Result<InstalledPlugin> {
    if !input.trust_confirmed || input.scope != "global" || input.runtimes != ["codex"] {
        bail!("Codex 原生插件仅支持已确认信任的 Codex 全局安装");
    }
    if !valid_identifier(&input.plugin_name) || !valid_identifier(&input.marketplace_id) {
        bail!("Codex 插件标识无效");
    }
    let mut file = load_file_at(&paths.data_file)?;
    let record = file
        .marketplaces
        .iter()
        .find(|record| record.id == input.marketplace_id)
        .ok_or_else(|| anyhow!("市场不存在"))?;
    let market = PathBuf::from(&record.path);
    let (id, _, entries) = parse_marketplace(&market)?;
    if id != input.marketplace_id {
        bail!("市场标识已变化，请刷新来源");
    }
    let (view, source) = entries
        .iter()
        .find(|(view, _)| view.name == input.plugin_name)
        .ok_or_else(|| anyhow!("插件不存在"))?;
    if !view
        .compatibility
        .as_ref()
        .is_some_and(|compatibility| compatibility.native_codex)
    {
        bail!("当前包尚无可验证的 Codex 原生安装适配，或包含待授权连接器");
    }
    let plugin_id = format!("{}@{id}", input.plugin_name);
    if mcp::load_at(&paths.mcp_file)?
        .iter()
        .any(|server| server.source_plugin.as_deref() == Some(&plugin_id) && server.enabled)
    {
        bail!("历史全局 MCP 已启用，无法限定到 Codex；请先自行审阅旧配置");
    }
    let existing = file
        .installed
        .iter()
        .find(|plugin| plugin.id == plugin_id)
        .cloned();
    if let Some(plugin) = &existing {
        if center.snapshot()?.assets.iter().any(|asset| {
            plugin.skill_asset_ids.contains(&asset.id) && asset.trust_state == "blocked"
        }) {
            bail!("该插件包含被阻止的 Skill，原生安装同样拒绝");
        }
    }
    let target_id = "codex|global|native";
    // Codex reserves openai-curated in the installed CLI. Use an explicitly
    // named personal market without impersonating or editing upstream metadata.
    let native_market = if id == official::MARKET {
        codex_wrapper::prepare(paths, record, &input.plugin_name, source)?
    } else {
        market.clone()
    };
    let native_id = if id == official::MARKET {
        codex_wrapper::ALIAS
    } else {
        &id
    };
    let selector = format!("{}@{native_id}", input.plugin_name);
    let previous = existing
        .as_ref()
        .and_then(|plugin| plugin.targets.iter().find(|target| target.id == target_id))
        .and_then(|target| target.native.as_ref());
    let list = run(&list_args())?;
    if let Some(receipt) = previous {
        if verify(receipt, &list, home)? {
            // An exact existing app-owned install is already the requested
            // outcome. Do not replace its verified receipt with a pending one.
            return Ok(existing
                .clone()
                .ok_or_else(|| anyhow!("已有安装记录缺失"))?);
        }
    } else if installed_entry(&list, &selector)?.is_some() {
        bail!("Codex 已有同名插件安装，应用不会认领或覆盖");
    }
    let marketplace_list = run(&[
        "plugin".into(),
        "marketplace".into(),
        "list".into(),
        "--json".into(),
    ])?;
    let registered = marketplace_list
        .get("marketplaces")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("Codex 市场列表格式不兼容"))?;
    let mut matching = registered
        .iter()
        .filter(|entry| entry.get("name").and_then(Value::as_str) == Some(native_id));
    if let Some(entry) = matching.next() {
        let root = entry
            .get("root")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("已有 Codex 市场缺少目录"))?;
        if matching.next().is_some()
            || Path::new(root).canonicalize()? != native_market.canonicalize()?
        {
            bail!("Codex 已有同名市场但目录不同，拒绝更新或认领外部来源");
        }
    }
    let cache = materialize_plugin(paths, &market, &input.plugin_name, source)?;
    let expected_source = match source {
        PluginSourceRef::Relative(relative) => {
            ensure_within(&native_market, &native_market.join(relative))?
        }
        _ => bail!("Codex 当前仅适配可核验的本地市场条目"),
    };
    let components = scan_plugin_components(&cache)?;
    let mut plugin = existing.unwrap_or_else(|| InstalledPlugin {
        id: plugin_id.clone(),
        marketplace_id: id.clone(),
        name: input.plugin_name.clone(),
        version: view.version.clone(),
        description: view.description.clone(),
        cache_path: cache.display().to_string(),
        skill_asset_ids: Vec::new(),
        skill_names: components
            .skill_dirs
            .iter()
            .filter_map(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .map(str::to_string)
            })
            .collect(),
        mcp_server_names: Vec::new(),
        has_hooks: components.has_hooks,
        has_agents: components.has_agents,
        installed_at: now_millis(),
        targets: Vec::new(),
    });
    plugin.cache_path = cache.display().to_string();
    let mut target = targets::PluginTarget {
        id: target_id.into(),
        runtime: "codex".into(),
        scope: "global".into(),
        project_path: None,
        deployment_ids: Vec::new(),
        owned_deployment_ids: Vec::new(),
        native: Some(NativePluginReceipt {
            selector: selector.clone(),
            scope: "global".into(),
            install_path: None,
            verified: false,
            version: None,
            installed_at: None,
            last_updated: None,
            runtime: Some("codex".into()),
            source: None,
            marketplace_source: None,
            content_hash: None,
            config_root: Some(home.display().to_string()),
        }),
        status: "needs_attention".into(),
        last_error: Some("Codex 原生安装进行中；中断时请到原生插件管理核对".into()),
    };
    plugin.targets.retain(|entry| entry.id != target_id);
    plugin.targets.push(target.clone());
    file.installed.retain(|entry| entry.id != plugin_id);
    file.installed.push(plugin.clone());
    save_file_at(&paths.data_file, &file)?;
    let result = (|| {
        let registered = run(&[
            "plugin".into(),
            "marketplace".into(),
            "add".into(),
            native_market.display().to_string(),
            "--json".into(),
        ])?;
        if registered.get("marketplaceName").and_then(Value::as_str) != Some(native_id) {
            bail!("Codex 注册市场回执标识不匹配");
        }
        let add = run(&[
            "plugin".into(),
            "add".into(),
            selector.clone(),
            "--json".into(),
        ])?;
        let list = run(&list_args())?;
        receipt_from_add(
            &add,
            &list,
            &selector,
            &input.plugin_name,
            native_id,
            home,
            &expected_source,
        )
    })();
    match result {
        Ok(receipt) => {
            target.native = Some(receipt);
            target.status = "installed".into();
            target.last_error = None;
        }
        Err(_) => {
            target.status = "needs_attention".into();
            target.last_error = Some("Codex 原生安装未核验。可能有部分副作用，请在 Codex 插件管理查看诊断；应用不会自动认领或删除未知安装。".into());
        }
    }
    plugin.targets.retain(|entry| entry.id != target_id);
    plugin.targets.push(target);
    file.installed.retain(|entry| entry.id != plugin_id);
    file.installed.push(plugin.clone());
    save_file_at(&paths.data_file, &file)?;
    Ok(plugin)
}

pub(super) fn remove(
    receipt: &NativePluginReceipt,
    run: &mut impl FnMut(&[String]) -> Result<Value>,
) -> Result<()> {
    remove_home(receipt, &config_root()?, run)
}

fn remove_home(
    receipt: &NativePluginReceipt,
    home: &Path,
    run: &mut impl FnMut(&[String]) -> Result<Value>,
) -> Result<()> {
    if !verify(receipt, &run(&list_args())?, home)? {
        return Ok(());
    }
    run(&[
        "plugin".into(),
        "remove".into(),
        receipt.selector.clone(),
        "--json".into(),
    ])?;
    if installed_entry(&run(&list_args())?, &receipt.selector)?.is_some() {
        bail!("Codex 仍有该插件记录，未完成移除，目标记录已保留");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (
        PathBuf,
        Paths,
        SkillCenter,
        targets::PluginInstallInput,
        PathBuf,
        Value,
        Value,
    ) {
        let root =
            std::env::temp_dir().join(format!("crewvela-codex-adapter-{}", uuid::Uuid::new_v4()));
        let market = root.join("market");
        let source = market.join("plugins/remotion");
        std::fs::create_dir_all(market.join(".agents/plugins")).unwrap();
        std::fs::write(market.join(".agents/plugins/marketplace.json"), r#"{"name":"test","plugins":[{"name":"remotion","source":{"source":"local","path":"./plugins/remotion"}}]}"#).unwrap();
        std::fs::create_dir_all(source.join(".codex-plugin")).unwrap();
        std::fs::create_dir_all(source.join("skills/demo")).unwrap();
        std::fs::create_dir_all(source.join("references")).unwrap();
        std::fs::write(
            source.join(".codex-plugin/plugin.json"),
            r#"{"name":"remotion","version":"1.2.3","skills":"./skills/"}"#,
        )
        .unwrap();
        std::fs::write(
            source.join("skills/demo/SKILL.md"),
            "../../references/guide.md",
        )
        .unwrap();
        std::fs::write(
            source.join("references/guide.md"),
            "preserve root references",
        )
        .unwrap();
        let paths = Paths::from_config_dir(root.join("config"));
        add_marketplace_at(&paths, &market.display().to_string()).unwrap();
        let center = SkillCenter::open_at(root.join("center")).unwrap();
        let home = root.join("codex-home");
        let installed = home.join("plugins/cache/test/remotion/1.2.3");
        copy_dir_recursive(&source, &installed).unwrap();
        let input = targets::PluginInstallInput {
            marketplace_id: "test".into(),
            plugin_name: "remotion".into(),
            runtimes: vec!["codex".into()],
            scope: "global".into(),
            project_path: None,
            trust_confirmed: true,
            mode: Some("native_codex".into()),
        };
        let add = serde_json::json!({"pluginId":"remotion@test","name":"remotion","marketplaceName":"test","version":"1.2.3","installedPath":installed});
        let list = serde_json::json!({"installed":[{"pluginId":"remotion@test","name":"remotion","marketplaceName":"test","version":"1.2.3","source":{"source":"local","path":source}}],"available":[]});
        (root, paths, center, input, home, add, list)
    }
    #[test]
    fn codex_add_is_verified_and_removal_is_exact_with_root_resources_preserved() {
        let (root, paths, center, input, home, add, list) = fixture();
        let mut calls = Vec::new();
        let plugin = install_at_home(&paths, &center, &input, &home, &mut |args| {
            calls.push(args.to_vec());
            match calls.len() {
                1 => Ok(serde_json::json!({"installed":[],"available":[]})),
                2 => Ok(serde_json::json!({"marketplaces":[]})),
                3 => {
                    assert_eq!(
                        load_file_at(&paths.data_file).unwrap().installed[0].targets[0].status,
                        "needs_attention"
                    );
                    Ok(serde_json::json!({"marketplaceName":"test"}))
                }
                4 => Ok(add.clone()),
                5 => Ok(list.clone()),
                _ => panic!("Unexpected call"),
            }
        })
        .unwrap();
        assert_eq!(plugin.targets[0].status, "installed");
        let idempotent = install_at_home(&paths, &center, &input, &home, &mut |args| {
            assert_eq!(
                args,
                &list_args(),
                "已核验安装重试不能重走 add 以丢失旧回执"
            );
            Ok(list.clone())
        })
        .unwrap();
        assert!(idempotent.targets[0].native.as_ref().unwrap().verified);
        assert_eq!(
            idempotent.targets[0].native.as_ref().unwrap().content_hash,
            plugin.targets[0].native.as_ref().unwrap().content_hash
        );
        let receipt = plugin.targets[0].native.as_ref().unwrap();
        assert!(Path::new(receipt.install_path.as_ref().unwrap())
            .join("references/guide.md")
            .is_file());
        assert!(center.snapshot().unwrap().assets.is_empty());
        let mut count = 0;
        remove_home(receipt, &home, &mut |args| {
            count += 1;
            match count {
                1 => Ok(list.clone()),
                2 => {
                    assert_eq!(args[2], "remotion@test");
                    Ok(serde_json::json!({}))
                }
                3 => Ok(serde_json::json!({"installed":[],"available":[]})),
                _ => panic!("Unexpected"),
            }
        })
        .unwrap();
        drop(center);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn codex_rejects_preexisting_wrong_home_and_modified_content() {
        let (root, paths, center, input, home, add, list) = fixture();
        assert!(
            install_at_home(&paths, &center, &input, &home, &mut |_| Ok(list.clone()))
                .unwrap_err()
                .to_string()
                .contains("已有")
        );
        let source = root.join("market/plugins/remotion");
        let receipt = receipt_from_add(
            &add,
            &list,
            "remotion@test",
            "remotion",
            "test",
            &home,
            &source,
        )
        .unwrap();
        assert!(verify(&receipt, &list, &root.join("other-home")).is_err());
        std::fs::write(
            Path::new(receipt.install_path.as_ref().unwrap()).join("references/guide.md"),
            "external change",
        )
        .unwrap();
        assert!(remove_home(&receipt, &home, &mut |_| Ok(list.clone()))
            .unwrap_err()
            .to_string()
            .contains("内容已变化"));
        drop(center);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn codex_failure_is_pending_and_unknown_existing_receipt_is_not_claimed() {
        let (root, paths, center, input, home, _, list) = fixture();
        let mut count = 0;
        let plugin = install_at_home(&paths, &center, &input, &home, &mut |_| {
            count += 1;
            match count {
                1 => Ok(serde_json::json!({"installed":[],"available":[]})),
                2 => Ok(serde_json::json!({"marketplaces":[]})),
                _ => Err(anyhow!("secret mock error")),
            }
        })
        .unwrap();
        assert_eq!(plugin.targets[0].status, "needs_attention");
        assert!(!plugin.targets[0]
            .last_error
            .as_ref()
            .unwrap()
            .contains("secret"));
        assert!(
            install_at_home(&paths, &center, &input, &home, &mut |_| Ok(list.clone())).is_err()
        );
        drop(center);
        std::fs::remove_dir_all(root).unwrap();
    }
}
