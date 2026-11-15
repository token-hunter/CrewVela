use super::*;
use crate::skill_center::{runtime_adapters, DeploySkillInput};
use std::sync::Mutex;

pub(super) static OPERATIONS: Mutex<()> = Mutex::new(());
const BUILTIN: &str = "crewvela精选";
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginTarget {
    pub id: String,
    pub runtime: String,
    pub scope: String,
    pub project_path: Option<String>,
    pub deployment_ids: Vec<String>,
    #[serde(default)]
    pub owned_deployment_ids: Vec<String>,
    #[serde(default)]
    pub native: Option<super::native::NativePluginReceipt>,
    pub status: String,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginInstallInput {
    pub marketplace_id: String,
    pub plugin_name: String,
    pub runtimes: Vec<String>,
    pub scope: String,
    pub project_path: Option<String>,
    pub trust_confirmed: bool,
    #[serde(default)]
    pub mode: Option<String>,
}

pub(super) fn builtin_view() -> MarketplaceView {
    MarketplaceView {
        id: BUILTIN.into(), source: "https://github.com/anthropics/skills".into(),
        source_kind: "builtin".into(), owner_name: "Anthropic".into(), path: String::new(),
        url: Some("https://github.com/anthropics/skills".into()), last_refreshed_at: None,
        last_error: None,
        plugins: super::catalog::entries().iter().map(|entry| {
            let source = super::catalog::source(entry);
            let mut compatibility = super::compatibility::PluginCompatibility::builtin_skill();
            compatibility.dependencies = entry.dependencies.clone();
            compatibility.license = Some(entry.license.clone());
            MarketPluginView {
                name: entry.name.clone(), display_name: Some(entry.display_name.clone()), description: entry.description.clone(),
                category: Some(entry.category.clone()), author: Some(source.author.clone()),
                version: Some(source.revision[..12].into()), source_display: format!("{} / {} @ {}", source.label, entry.subpath, source.revision),
                installable: true, note: Some("精选 Skill 包；不包含原生插件安装。首次安装需要网络与 Git；依赖由使用者按上游说明配置。".into()),
                compatibility: Some(compatibility),
            }
        }).collect(),
    }
}

fn snapshot_at_with_targets(paths: &Paths, center: &SkillCenter) -> Result<PluginMarketSnapshot> {
    let mut snapshot = super::curation::curated_snapshot_at(paths)?;
    let deployments = center.snapshot()?.deployments;
    for plugin in &mut snapshot.installed {
        for target in &mut plugin.targets {
            if target.native.is_some() {
                continue;
            }
            if target.status == "failed" {
                continue;
            }
            let healthy = !target.deployment_ids.is_empty()
                && target.deployment_ids.iter().all(|id| {
                    deployments
                        .iter()
                        .any(|deployment| &deployment.id == id && deployment.status == "synced")
                });
            if !healthy {
                target.status = "needs_attention".into();
                target.last_error =
                    Some("Skill 部署缺失或已被修改，请检查 Skill 中心后重试。".into());
            }
        }
    }
    Ok(snapshot)
}

pub fn snapshot_with_targets(center: &SkillCenter) -> Result<PluginMarketSnapshot> {
    let _guard = OPERATIONS.lock().map_err(|_| anyhow!("插件操作锁不可用"))?;
    snapshot_at_with_targets(&Paths::standard()?, center)
}

fn prepare_plugin(
    paths: &Paths,
    center: &SkillCenter,
    market: &str,
    name: &str,
) -> Result<InstalledPlugin> {
    let id = format!("{name}@{market}");
    let existing = load_file_at(&paths.data_file)?
        .installed
        .into_iter()
        .find(|plugin| plugin.id == id);
    if let Some(existing) = &existing {
        if mcp::load_at(&paths.mcp_file)?
            .iter()
            .any(|server| server.source_plugin.as_deref() == Some(&id) && server.enabled)
        {
            bail!("该历史插件有全局启用的 MCP，不能按所选 Agent 限定安装。请先到 MCP 页审阅并自行调整旧配置；本操作未修改旧 MCP。");
        }
        if !existing.skill_asset_ids.is_empty() {
            let compatibility = super::compatibility::inspect(Path::new(&existing.cache_path))?;
            if !compatibility.skill_compatible || compatibility.status == "needs_auth" {
                bail!("历史插件缓存尚未验证为独立 Skill 兼容包；不复用旧资产绕过格式与授权边界");
            }
            return Ok(existing.clone());
        }
    }
    let cache = if market == BUILTIN {
        let entry = super::catalog::find(name).ok_or_else(|| anyhow!("未知精选插件"))?;
        let source = super::catalog::source(entry);
        materialize_plugin(
            paths,
            &paths.market_root.join("crewvela-curated"),
            name,
            &PluginSourceRef::Git {
                url: source.url.clone(),
                subpath: Some(entry.subpath.clone()),
                reference: Some(source.revision.clone()),
            },
        )?
    } else {
        let file = load_file_at(&paths.data_file)?;
        let record = file
            .marketplaces
            .iter()
            .find(|entry| entry.id == market)
            .ok_or_else(|| anyhow!("市场不存在"))?;
        let root = PathBuf::from(&record.path);
        let (_, _, entries) = parse_marketplace(&root)?;
        let (_, source) = entries
            .iter()
            .find(|(view, _)| view.name == name)
            .ok_or_else(|| anyhow!("插件不存在"))?;
        materialize_plugin(paths, &root, name, source)?
    };
    let components = scan_plugin_components(&cache)?;
    let (format, _) = super::compatibility::read_manifest(&cache)?;
    if matches!(format.as_str(), "codex" | "portable") {
        bail!("该包包含 Codex/portable manifest，跨 Skill 根引用和宿主工具尚未验证。请使用支持的原生整包安装，不做有损 Skill 拆分");
    }
    let compatibility = super::compatibility::inspect(&cache)?;
    if !compatibility.skill_compatible || compatibility.status == "needs_auth" {
        bail!("该包尚未验证为可独立分发的 Skill，或包含待授权连接器。未分发组件，也未启用 MCP；请使用已适配的原生通路");
    }
    if components.skill_dirs.is_empty() {
        bail!("该插件没有可分发的 Skill；当前适配器不支持安装其原生插件、MCP、Hooks 或子 Agent。未启用任何组件。");
    }
    let mut assets = Vec::new();
    let mut names = Vec::new();
    for root in &components.skill_dirs {
        for asset in center.import(ImportSkillInput {
            source_type: "local".into(),
            source: root.display().to_string(),
            branch: None,
            subpath: None,
            revision: None,
        })? {
            assets.push(asset.id);
            names.push(asset.name);
        }
    }
    // Only inspect MCP definitions. The existing registry is global and must not
    // expose a selected plugin to agents the user did not select.
    let servers = components
        .mcp_servers
        .map(|servers| servers.keys().cloned().collect())
        .unwrap_or_default();
    Ok(InstalledPlugin {
        id,
        marketplace_id: market.into(),
        name: name.into(),
        version: components
            .version
            .or_else(|| {
                if market == BUILTIN {
                    super::catalog::find(name).map(|entry| super::catalog::source(entry).revision[..12].into())
                } else { None }
            }),
        description: components.description.unwrap_or_default(),
        cache_path: cache.display().to_string(),
        skill_asset_ids: assets,
        skill_names: names,
        mcp_server_names: servers,
        has_hooks: components.has_hooks,
        has_agents: components.has_agents,
        installed_at: now_millis(),
        targets: existing.map(|plugin| plugin.targets).unwrap_or_default(),
    })
}

fn install_at(
    paths: &Paths,
    center: &SkillCenter,
    input: PluginInstallInput,
) -> Result<InstalledPlugin> {
    if !input.trust_confirmed {
        bail!("请确认信任插件来源与代码后安装");
    }
    if input.runtimes.is_empty() {
        bail!("至少选择一个 Agent");
    }
    if input.mode.as_deref() == Some("native_claude") {
        return install_native_at(paths, center, &input, &mut super::native::run_claude);
    }
    if input.mode.as_deref() == Some("native_codex") {
        return super::codex_native::install_at(paths, center, &input, &mut super::codex_native::run_codex);
    }
    if input.mode.as_deref().is_some_and(|mode| mode != "skills") {
        bail!("不支持的安装方式");
    }
    if !matches!(input.scope.as_str(), "global" | "project") {
        bail!("仅支持全局或项目安装");
    }
    if input.scope == "project" {
        let project = input
            .project_path
            .as_deref()
            .ok_or_else(|| anyhow!("请选择项目"))?;
        if !Path::new(project).is_absolute() || !Path::new(project).is_dir() {
            bail!("项目必须为存在的绝对目录");
        }
    }
    let adapters = runtime_adapters()?;
    for runtime in &input.runtimes {
        if !adapters.iter().any(|adapter| &adapter.runtime == runtime) {
            bail!("{} 的 Skill 安装适配尚未支持", runtime);
        }
    }
    let mut plugin = prepare_plugin(paths, center, &input.marketplace_id, &input.plugin_name)?;
    let mut file = load_file_at(&paths.data_file)?;
    // Cache/import success is persisted before deployment. A failed deployment is
    // an explicit target state, never reported as an installed agent plugin.
    file.installed.retain(|entry| entry.id != plugin.id);
    file.installed.push(plugin.clone());
    save_file_at(&paths.data_file, &file)?;
    let mut runtimes = input.runtimes;
    runtimes.sort();
    runtimes.dedup();
    for runtime in runtimes {
        let project_path = if input.scope == "project" {
            input.project_path.clone()
        } else {
            None
        };
        let target_id = format!(
            "{runtime}|{}|{}",
            input.scope,
            project_path.as_deref().unwrap_or_default()
        );
        let previous = plugin
            .targets
            .iter()
            .find(|target| target.id == target_id)
            .cloned();
        let mut target = PluginTarget {
            id: target_id.clone(),
            runtime: runtime.clone(),
            scope: input.scope.clone(),
            project_path: project_path.clone(),
            deployment_ids: previous
                .as_ref()
                .map(|item| item.deployment_ids.clone())
                .unwrap_or_default(),
            owned_deployment_ids: previous
                .map(|item| item.owned_deployment_ids)
                .unwrap_or_default(),
            native: None,
            status: "installed".into(),
            last_error: None,
        };
        let active_deployments = center.snapshot()?.deployments;
        target.deployment_ids.retain(|id| {
            active_deployments
                .iter()
                .any(|deployment| &deployment.id == id)
        });
        target.owned_deployment_ids.retain(|id| {
            active_deployments
                .iter()
                .any(|deployment| &deployment.id == id)
        });
        // Keep each successful deployment id, including when a later component
        // fails. Retrying is idempotent; removal can clean up partial installs.
        for skill_id in &plugin.skill_asset_ids {
            match center.deploy_batch_with_ownership(vec![DeploySkillInput {
                skill_id: skill_id.clone(),
                runtime: runtime.clone(),
                scope: input.scope.clone(),
                project_path: project_path.clone(),
                task_id: None,
                agent_id: None,
                trust_confirmed: input.trust_confirmed,
            }]) {
                Ok(deployments) => {
                    for (deployment, created) in deployments {
                        let id = &deployment.id;
                        if !target.deployment_ids.contains(id) {
                            target.deployment_ids.push(id.clone());
                        }
                        if created && !target.owned_deployment_ids.contains(id) {
                            target.owned_deployment_ids.push(id.clone());
                        }
                    }
                }
                Err(error) => {
                    target.status = "failed".into();
                    target.last_error = Some(error.to_string());
                    break;
                }
            }
        }
        plugin.targets.retain(|entry| entry.id != target_id);
        plugin.targets.push(target);
        file.installed.retain(|entry| entry.id != plugin.id);
        file.installed.push(plugin.clone());
        save_file_at(&paths.data_file, &file)?;
    }
    Ok(plugin)
}

pub fn install_targets(center: &SkillCenter, input: PluginInstallInput) -> Result<InstalledPlugin> {
    let _guard = OPERATIONS.lock().map_err(|_| anyhow!("插件操作锁不可用"))?;
    install_at(&Paths::standard()?, center, input)
}

fn install_native_at(
    paths: &Paths,
    center: &SkillCenter,
    input: &PluginInstallInput,
    run: &mut impl FnMut(&[String]) -> Result<Value>,
) -> Result<InstalledPlugin> {
    use super::native::{find_receipt, install_args, list_args, verify_owned, NativePluginReceipt};
    if input.scope != "global" || input.runtimes != ["claude_code"] {
        bail!("Claude 原生插件目前只支持全局安装且只能选择 Claude Code；项目和其他 Agent 请使用 Skill 组件分发");
    }
    if input.marketplace_id == BUILTIN {
        bail!("内置目录为 Skill 包，请使用组件分发");
    }
    let identifier = |value: &str| {
        !value.is_empty()
            && value != "."
            && value != ".."
            && !value.starts_with('-')
            && value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    };
    if !identifier(&input.plugin_name) || !identifier(&input.marketplace_id) {
        bail!("该名称不适用于原生 CLI 安装：标识仅支持 ASCII 字母、数字、点、下划线和连字符，不能以连字符开头");
    }
    let mut file = load_file_at(&paths.data_file)?;
    let record = file
        .marketplaces
        .iter()
        .find(|entry| entry.id == input.marketplace_id)
        .ok_or_else(|| anyhow!("市场不存在"))?;
    let market = PathBuf::from(&record.path);
    let (manifest_id, _, entries) = parse_marketplace(&market)?;
    if manifest_id != input.marketplace_id {
        bail!("市场标识已变化，请先刷新来源");
    }
    let (view, source) = entries
        .iter()
        .find(|(view, _)| view.name == input.plugin_name)
        .ok_or_else(|| anyhow!("插件不存在"))?;
    let selector = format!("{}@{manifest_id}", input.plugin_name);
    let plugin_id = selector.clone();
    if mcp::load_at(&paths.mcp_file)?
        .iter()
        .any(|server| server.source_plugin.as_deref() == Some(&plugin_id) && server.enabled)
    {
        bail!("该历史插件有全局启用的 MCP，不能限定安装到 Claude。请先到 MCP 页审阅并自行调整旧配置；本操作未修改旧 MCP。");
    }
    let target_id = "claude_code|global|native";
    let existing = file
        .installed
        .iter()
        .find(|plugin| plugin.id == plugin_id)
        .cloned();
    if let Some(plugin) = &existing {
        if center.snapshot()?.assets.iter().any(|asset| {
            plugin.skill_asset_ids.contains(&asset.id) && asset.trust_state == "blocked"
        }) {
            bail!("该插件包含已在 Skill 中心被阻止的组件，原生安装同样被拒绝");
        }
    }
    if view.compatibility.as_ref().is_some_and(|compatibility| !compatibility.native_claude) {
        bail!("该包不是已适配的 Claude 原生插件（Codex 专属/连接器/未知格式），不能传给 Claude 安装");
    }
    let previous = existing
        .as_ref()
        .and_then(|plugin| plugin.targets.iter().find(|target| target.id == target_id))
        .and_then(|target| target.native.as_ref());
    let list = run(&list_args())?;
    if let Some(receipt) = previous {
        // An interrupted operation cannot silently claim somebody else's install.
        verify_owned(receipt, &list)?;
    } else if find_receipt(&list, &selector)?.is_some() {
        bail!("Claude 已有同名插件安装，当前应用不会认领或覆盖它。请在 Claude 原生插件管理中处理");
    }
    let cache = materialize_plugin(paths, &market, &input.plugin_name, source)?;
    if !super::compatibility::inspect(&cache)?.native_claude {
        bail!("缓存插件并非已适配的 Claude 原生包，或需要连接器授权；未调用 Claude CLI");
    }
    let components = scan_plugin_components(&cache)?;
    let mut plugin = existing.unwrap_or_else(|| InstalledPlugin {
        id: plugin_id.clone(),
        marketplace_id: input.marketplace_id.clone(),
        name: input.plugin_name.clone(),
        version: components.version,
        description: view.description.clone(),
        cache_path: cache.display().to_string(),
        skill_asset_ids: Vec::new(),
        skill_names: components
            .skill_dirs
            .iter()
            .filter_map(|root| {
                root.file_name()
                    .and_then(|value| value.to_str())
                    .map(str::to_string)
            })
            .collect(),
        mcp_server_names: components
            .mcp_servers
            .map(|servers| servers.keys().cloned().collect())
            .unwrap_or_default(),
        has_hooks: components.has_hooks,
        has_agents: components.has_agents,
        installed_at: now_millis(),
        targets: Vec::new(),
    });
    plugin.cache_path = cache.display().to_string();
    let mut target = PluginTarget {
        id: target_id.into(),
        runtime: "claude_code".into(),
        scope: "global".into(),
        project_path: None,
        deployment_ids: Vec::new(),
        owned_deployment_ids: Vec::new(),
        native: Some(NativePluginReceipt {
            selector: selector.clone(),
            scope: "user".into(),
            install_path: None,
            verified: false,
            version: None,
            installed_at: None,
            last_updated: None,
            runtime: None, source: None, marketplace_source: None, content_hash: None, config_root: None,
        }),
        status: "needs_attention".into(),
        last_error: Some("原生安装处理中，若中断请在 Claude 插件管理中检查安装状态。".into()),
    };
    // Persist the attempt before invoking the external CLI; a timeout/crash may
    // leave native side effects even though no successful response was received.
    plugin.targets.retain(|entry| entry.id != target_id);
    plugin.targets.push(target.clone());
    file.installed.retain(|entry| entry.id != plugin_id);
    file.installed.push(plugin.clone());
    save_file_at(&paths.data_file, &file)?;
    let install_result = run(&install_args(&input.plugin_name, &market));
    let verification = run(&list_args()).and_then(|list| find_receipt(&list, &selector));
    match (install_result, verification) {
        (Ok(_), Ok(Some(receipt))) => {
            target.native = Some(receipt);
            target.status = "installed".into();
            target.last_error = None;
        }
        (Err(error), Ok(Some(receipt))) => {
            target.native = Some(receipt);
            target.status = "failed".into();
            target.last_error = Some(format!(
                "命令报错但已检测到安装，可移除此目标后重试：{error}"
            ));
        }
        (Err(error), _) => {
            target.status = "needs_attention".into();
            target.last_error = Some(format!("原生安装未核验：{error}。请在 Claude 插件管理中检查；为避免认领外部安装，应用不会自动重试或删除。"));
        }
        (Ok(_), _) => {
            target.status = "needs_attention".into();
            target.last_error = Some("CLI 返回成功，但精确安装记录未核验。请在 Claude 插件管理中检查；应用不会自动重试或删除。".into());
        }
    }
    plugin.targets.retain(|entry| entry.id != target_id);
    plugin.targets.push(target);
    file.installed.retain(|entry| entry.id != plugin_id);
    file.installed.push(plugin.clone());
    save_file_at(&paths.data_file, &file)?;
    Ok(plugin)
}

fn remove_native(
    receipt: &super::native::NativePluginReceipt,
    run: &mut impl FnMut(&[String]) -> Result<Value>,
) -> Result<()> {
    use super::native::{find_receipt, list_args, remove_args, verify_owned};
    if verify_owned(receipt, &run(&list_args())?)?.is_none() {
        return Ok(());
    }
    run(&remove_args(receipt))?;
    if find_receipt(&run(&list_args())?, &receipt.selector)?.is_some() {
        bail!("Claude 仍有该插件记录，卸载未完成；安装目标记录已保留");
    }
    Ok(())
}

pub(super) fn remove_at(
    paths: &Paths,
    center: &SkillCenter,
    installed_id: &str,
    target_id: &str,
) -> Result<()> {
    let mut file = load_file_at(&paths.data_file)?;
    let plugin = file
        .installed
        .iter()
        .find(|plugin| plugin.id == installed_id)
        .ok_or_else(|| anyhow!("插件不存在"))?;
    let target = plugin
        .targets
        .iter()
        .find(|target| target.id == target_id)
        .ok_or_else(|| anyhow!("安装目标不存在"))?
        .clone();
    if let Some(receipt) = &target.native {
        if receipt.runtime.as_deref() == Some("codex") {
            super::codex_native::remove(receipt, &mut super::codex_native::run_codex)?;
        } else { remove_native(receipt, &mut super::native::run_claude)?; }
    }
    let deployments = center.snapshot()?.deployments;
    for id in &target.owned_deployment_ids {
        let mut shared = false;
        'plugins: for plugin in &mut file.installed {
            for entry in &mut plugin.targets {
                if !(plugin.id == installed_id && entry.id == target_id)
                    && entry.deployment_ids.contains(id)
                {
                    if !entry.owned_deployment_ids.contains(id) {
                        entry.owned_deployment_ids.push(id.clone());
                    }
                    shared = true;
                    break 'plugins;
                }
            }
        }
        if !shared && deployments.iter().any(|deployment| &deployment.id == id) {
            center.undeploy(id)?;
        }
    }
    let plugin = file
        .installed
        .iter_mut()
        .find(|plugin| plugin.id == installed_id)
        .unwrap();
    plugin.targets.retain(|target| target.id != target_id);
    save_file_at(&paths.data_file, &file)?;
    Ok(())
}

pub fn remove_target(center: &SkillCenter, installed_id: &str, target_id: &str) -> Result<()> {
    let _guard = OPERATIONS.lock().map_err(|_| anyhow!("插件操作锁不可用"))?;
    remove_at(&Paths::standard()?, center, installed_id, target_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture {
        root: PathBuf,
        paths: Paths,
        center: SkillCenter,
    }
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir()
                .join(format!("crewvela-plugin-target-{}", uuid::Uuid::new_v4()));
            let paths = Paths::from_config_dir(root.join("config"));
            let market = root.join("market");
            std::fs::create_dir_all(market.join(".claude-plugin")).unwrap();
            std::fs::write(market.join(".claude-plugin/marketplace.json"), r#"{"name":"test-market","plugins":[{"name":"one","source":"./one"},{"name":"two","source":"./two"},{"name":"mcp-only","source":"./mcp-only"}]}"#).unwrap();
            for name in ["one", "two"] {
                std::fs::create_dir_all(market.join(name).join(".claude-plugin")).unwrap();
                std::fs::write(market.join(name).join(".claude-plugin/plugin.json"), format!(r#"{{"name":"{name}","version":"1.0.0"}}"#)).unwrap();
                std::fs::create_dir_all(market.join(name).join("skills/shared")).unwrap();
                std::fs::write(
                    market.join(name).join("skills/shared/SKILL.md"),
                    "---\nname: shared\ndescription: test\n---\n# Test",
                )
                .unwrap();
            }
            std::fs::create_dir_all(market.join("mcp-only")).unwrap();
            std::fs::write(
                market.join("mcp-only/.mcp.json"),
                r#"{"mcpServers":{"test":{"command":"node"}}}"#,
            )
            .unwrap();
            add_marketplace_at(&paths, &market.display().to_string()).unwrap();
            std::fs::create_dir_all(root.join("project")).unwrap();
            let center = SkillCenter::open_at(root.join("center")).unwrap();
            Self {
                root,
                paths,
                center,
            }
        }
        fn input(&self, name: &str, runtimes: &[&str]) -> PluginInstallInput {
            PluginInstallInput {
                marketplace_id: "test-market".into(),
                plugin_name: name.into(),
                runtimes: runtimes.iter().map(|value| (*value).into()).collect(),
                scope: "project".into(),
                project_path: Some(self.root.join("project").display().to_string()),
                trust_confirmed: true,
                mode: None,
            }
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn targeted_install_is_real_idempotent_and_does_not_enable_global_mcp() {
        let f = Fixture::new();
        let result = install_at(
            &f.paths,
            &f.center,
            f.input("one", &["claude_code", "codex"]),
        )
        .unwrap();
        assert_eq!(result.targets.len(), 2);
        assert!(result
            .targets
            .iter()
            .all(|target| target.status == "installed"));
        assert!(mcp::load_at(&f.paths.mcp_file).unwrap().is_empty());
        let deployments = f.center.snapshot().unwrap().deployments;
        assert_eq!(deployments.len(), 2);
        assert!(deployments
            .iter()
            .all(|item| Path::new(&item.target_path).join("SKILL.md").is_file()));
        let result = install_at(&f.paths, &f.center, f.input("one", &["codex"])).unwrap();
        assert_eq!(result.targets.len(), 2);
        assert_eq!(f.center.snapshot().unwrap().deployments.len(), 2);
        let claude = result
            .targets
            .iter()
            .find(|target| target.runtime == "claude_code")
            .unwrap();
        remove_at(&f.paths, &f.center, &result.id, &claude.id).unwrap();
        let deployments = f.center.snapshot().unwrap().deployments;
        assert_eq!(deployments.len(), 1);
        assert_eq!(deployments[0].runtime, "codex");
        assert!(Path::new(&deployments[0].target_path)
            .join("SKILL.md")
            .is_file());
    }

    #[test]
    fn connector_and_cross_root_skill_are_not_distributed_as_verified_compatibility() {
        let f = Fixture::new();
        std::fs::write(f.root.join("market/one/.mcp.json"), r#"{"mcpServers":{"oauth":{"url":"https://example.invalid/mcp"}}}"#).unwrap();
        assert!(install_at(&f.paths, &f.center, f.input("one", &["codex"])).unwrap_err().to_string().contains("待授权"));
        assert!(mcp::load_at(&f.paths.mcp_file).unwrap().is_empty());
        assert!(f.center.snapshot().unwrap().deployments.is_empty());
        std::fs::remove_file(f.root.join("market/one/.mcp.json")).unwrap();
        std::fs::write(f.root.join("market/one/skills/shared/SKILL.md"), "Uses ../../references/guide.md").unwrap();
        assert!(install_at(&f.paths, &f.center, f.input("one", &["codex"])).is_err());
        assert!(f.center.snapshot().unwrap().deployments.is_empty());
    }

    #[test]
    fn shared_plugin_deployment_is_retained_until_last_reference() {
        let f = Fixture::new();
        let one = install_at(&f.paths, &f.center, f.input("one", &["codex"])).unwrap();
        let two = install_at(&f.paths, &f.center, f.input("two", &["codex"])).unwrap();
        assert_eq!(one.targets[0].deployment_ids, two.targets[0].deployment_ids);
        remove_at(&f.paths, &f.center, &one.id, &one.targets[0].id).unwrap();
        assert_eq!(f.center.snapshot().unwrap().deployments.len(), 1);
        remove_at(&f.paths, &f.center, &two.id, &two.targets[0].id).unwrap();
        assert!(f.center.snapshot().unwrap().deployments.is_empty());
    }

    #[test]
    fn preexisting_manual_deployment_is_not_owned_or_removed() {
        let f = Fixture::new();
        let prepared = prepare_plugin(&f.paths, &f.center, "test-market", "one").unwrap();
        let deployment = f
            .center
            .deploy_batch(vec![DeploySkillInput {
                skill_id: prepared.skill_asset_ids[0].clone(),
                runtime: "codex".into(),
                scope: "project".into(),
                project_path: f.input("one", &[]).project_path,
                task_id: None,
                agent_id: None,
                trust_confirmed: true,
            }])
            .unwrap();
        let one = install_at(&f.paths, &f.center, f.input("one", &["codex"])).unwrap();
        assert!(one.targets[0].owned_deployment_ids.is_empty());
        remove_at(&f.paths, &f.center, &one.id, &one.targets[0].id).unwrap();
        assert!(Path::new(&deployment[0].target_path)
            .join("SKILL.md")
            .is_file());
    }

    #[test]
    fn trust_unsupported_and_mcp_only_inputs_fail_without_target_success() {
        let f = Fixture::new();
        let mut input = f.input("one", &["codex"]);
        input.trust_confirmed = false;
        assert!(install_at(&f.paths, &f.center, input).is_err());
        assert!(f.center.snapshot().unwrap().assets.is_empty());
        assert!(install_at(&f.paths, &f.center, f.input("one", &["zcode"])).is_err());
        assert!(install_at(&f.paths, &f.center, f.input("mcp-only", &["codex"])).is_err());
        assert!(mcp::load_at(&f.paths.mcp_file).unwrap().is_empty());
        let prepared = prepare_plugin(&f.paths, &f.center, "test-market", "one").unwrap();
        for id in &prepared.skill_asset_ids {
            f.center.set_trust(id, "blocked").unwrap();
        }
        let result = install_at(&f.paths, &f.center, f.input("one", &["codex"])).unwrap();
        assert_eq!(result.targets[0].status, "failed");
        assert!(result.targets[0]
            .last_error
            .as_ref()
            .unwrap()
            .contains("阻止"));
        assert!(f.center.snapshot().unwrap().deployments.is_empty());
    }

    #[test]
    fn external_target_replacement_is_not_deleted() {
        let f = Fixture::new();
        let one = install_at(&f.paths, &f.center, f.input("one", &["codex"])).unwrap();
        let deployment = f.center.snapshot().unwrap().deployments.remove(0);
        f.center.undeploy(&deployment.id).unwrap();
        let path = Path::new(&deployment.target_path);
        std::fs::create_dir_all(path).unwrap();
        std::fs::write(path.join("SKILL.md"), "manual replacement").unwrap();
        let result = install_at(&f.paths, &f.center, f.input("one", &["codex"])).unwrap();
        assert_eq!(result.targets[0].status, "failed");
        remove_at(&f.paths, &f.center, &one.id, &one.targets[0].id).unwrap();
        assert_eq!(
            std::fs::read_to_string(path.join("SKILL.md")).unwrap(),
            "manual replacement"
        );
    }

    #[test]
    fn retry_after_manual_undeploy_replaces_stale_ids_and_recovers_status() {
        let f = Fixture::new();
        let one = install_at(&f.paths, &f.center, f.input("one", &["codex"])).unwrap();
        let old_id = &one.targets[0].deployment_ids[0];
        f.center.undeploy(old_id).unwrap();
        let retried = install_at(&f.paths, &f.center, f.input("one", &["codex"])).unwrap();
        assert!(!retried.targets[0].deployment_ids.contains(old_id));
        assert_eq!(retried.targets[0].deployment_ids.len(), 1);
        assert_eq!(
            snapshot_at_with_targets(&f.paths, &f.center)
                .unwrap()
                .installed[0]
                .targets[0]
                .status,
            "installed"
        );
    }

    #[test]
    fn partial_failure_retains_created_ids_for_safe_removal() {
        let f = Fixture::new();
        let root = f.root.join("market/one/skills/z-blocked");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("SKILL.md"), "---\nname: z-blocked\n---\nBlocked").unwrap();
        let prepared = prepare_plugin(&f.paths, &f.center, "test-market", "one").unwrap();
        f.center
            .set_trust(&prepared.skill_asset_ids[1], "blocked")
            .unwrap();
        let one = install_at(&f.paths, &f.center, f.input("one", &["codex"])).unwrap();
        assert_eq!(one.targets[0].status, "failed");
        assert_eq!(one.targets[0].owned_deployment_ids.len(), 1);
        assert_eq!(f.center.snapshot().unwrap().deployments.len(), 1);
        remove_at(&f.paths, &f.center, &one.id, &one.targets[0].id).unwrap();
        assert!(f.center.snapshot().unwrap().deployments.is_empty());
    }

    #[test]
    fn full_uninstall_transfers_shared_ownership_to_remaining_plugin() {
        let f = Fixture::new();
        let one = install_at(&f.paths, &f.center, f.input("one", &["codex"])).unwrap();
        let two = install_at(&f.paths, &f.center, f.input("two", &["codex"])).unwrap();
        uninstall_plugin_at(&f.paths, &f.center, &one.id).unwrap();
        assert_eq!(f.center.snapshot().unwrap().deployments.len(), 1);
        uninstall_plugin_at(&f.paths, &f.center, &two.id).unwrap();
        assert!(f.center.snapshot().unwrap().deployments.is_empty());
        assert!(!f.center.snapshot().unwrap().assets.is_empty());
    }

    #[test]
    fn native_install_refuses_existing_and_persists_before_cli_then_verifies() {
        let f = Fixture::new();
        let mut input = f.input("one", &["claude_code"]);
        input.scope = "global".into();
        input.project_path = None;
        input.mode = Some("native_claude".into());
        let list = serde_json::json!([{ "id": "one@test-market", "scope": "user", "installPath": "C:/native/one" }]);
        assert!(
            install_native_at(&f.paths, &f.center, &input, &mut |_| Ok(list.clone()))
                .unwrap_err()
                .to_string()
                .contains("已有")
        );
        let mut count = 0;
        let result = install_native_at(&f.paths, &f.center, &input, &mut |args| {
            count += 1;
            match count {
                1 => Ok(serde_json::json!([])),
                2 => {
                    assert_eq!(args[1], "install");
                    assert_eq!(
                        load_file_at(&f.paths.data_file).unwrap().installed[0].targets[0].status,
                        "needs_attention"
                    );
                    Ok(serde_json::json!({"success":true}))
                }
                3 => Ok(list.clone()),
                _ => panic!("unexpected command"),
            }
        })
        .unwrap();
        assert_eq!(result.targets[0].status, "installed");
        assert!(result.targets[0].native.as_ref().unwrap().verified);
        let mut count = 0;
        remove_native(result.targets[0].native.as_ref().unwrap(), &mut |args| {
            count += 1;
            match count {
                1 => Ok(list.clone()),
                2 => {
                    assert!(args.contains(&"--keep-data".into()));
                    Ok(serde_json::json!({"success":true}))
                }
                3 => Ok(serde_json::json!([])),
                _ => panic!("unexpected"),
            }
        })
        .unwrap();
    }

    #[test]
    fn native_failed_response_is_not_success_and_unverified_side_effect_is_retained() {
        let f = Fixture::new();
        let mut input = f.input("one", &["claude_code"]);
        input.scope = "global".into();
        input.project_path = None;
        let mut count = 0;
        let result = install_native_at(&f.paths, &f.center, &input, &mut |_| {
            count += 1;
            if count == 1 {
                Ok(serde_json::json!([]))
            } else {
                Err(anyhow!("test timeout"))
            }
        })
        .unwrap();
        assert_eq!(result.targets[0].status, "needs_attention");
        assert!(!result.targets[0].native.as_ref().unwrap().verified);
        assert_eq!(
            load_file_at(&f.paths.data_file).unwrap().installed[0].targets[0].status,
            "needs_attention"
        );
        let existing = serde_json::json!([{ "id": "one@test-market", "scope": "user", "installPath": "C:/unknown" }]);
        assert!(
            install_native_at(&f.paths, &f.center, &input, &mut |_| Ok(existing.clone())).is_err()
        );
        assert!(
            remove_native(result.targets[0].native.as_ref().unwrap(), &mut |_| Ok(
                existing.clone()
            ))
            .is_err()
        );
        remove_native(result.targets[0].native.as_ref().unwrap(), &mut |_| {
            Ok(serde_json::json!([]))
        })
        .unwrap();
    }

    #[test]
    fn native_mode_cannot_bypass_global_mcp_or_known_blocked_skills() {
        let f = Fixture::new();
        std::fs::write(f.root.join("market/one/.mcp.json"), r#"{"mcpServers":{"test":{"command":"node"}}}"#).unwrap();
        install_plugin_at(&f.paths, &f.center, "test-market", "one").unwrap();
        let mut input = f.input("one", &["claude_code"]);
        input.scope = "global".into();
        input.project_path = None;
        assert!(
            install_native_at(&f.paths, &f.center, &input, &mut |_| panic!(
                "must reject before CLI"
            ))
            .unwrap_err()
            .to_string()
            .contains("全局启用")
        );
        mcp::remove_plugin_servers_at(&f.paths.mcp_file, "one@test-market").unwrap();
        std::fs::remove_file(f.root.join("market/one/.mcp.json")).unwrap();
        let asset = f.center.snapshot().unwrap().assets.remove(0);
        f.center.set_trust(&asset.id, "blocked").unwrap();
        assert!(
            install_native_at(&f.paths, &f.center, &input, &mut |_| panic!(
                "must reject before CLI"
            ))
            .unwrap_err()
            .to_string()
            .contains("阻止")
        );
    }

    #[test]
    fn native_mode_rejects_cli_option_and_path_identifiers() {
        let f = Fixture::new();
        let mut input = f.input("--unsafe", &["claude_code"]);
        input.scope = "global".into();
        input.project_path = None;
        assert!(
            install_native_at(&f.paths, &f.center, &input, &mut |_| panic!("must reject")).is_err()
        );
        input.plugin_name = "one".into();
        input.marketplace_id = "../market".into();
        assert!(
            install_native_at(&f.paths, &f.center, &input, &mut |_| panic!("must reject")).is_err()
        );
    }
}
