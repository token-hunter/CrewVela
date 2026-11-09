use super::*;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginCandidateSnapshot {
    pub marketplaces: Vec<MarketplaceView>,
    pub selection_configured: bool,
    pub selected_plugin_ids: Vec<String>,
    pub component_hints: HashMap<String, String>,
}

fn market_header(record: &MarketplaceRecord) -> MarketplaceView {
    MarketplaceView { id: record.id.clone(), source: record.source.clone(), source_kind: record.source_kind.clone(),
        owner_name: record.owner_name.clone(), path: record.path.clone(), url: record.url.clone(),
        last_refreshed_at: record.last_refreshed_at, last_error: record.last_error.clone(), plugins: Vec::new() }
}

pub(super) fn curated_snapshot_at(paths: &Paths) -> Result<PluginMarketSnapshot> {
    let file = load_file_at(&paths.data_file)?;
    let selected = file.selected_plugin_ids.clone().unwrap_or_default().into_iter().collect::<HashSet<_>>();
    let mut markets = vec![targets::builtin_view()];
    let mut installed_catalog = Vec::new();
    let builtin_header = { let mut header = markets[0].clone(); header.plugins.clear(); header };
    for plugin in &markets[0].plugins {
        if file.installed.iter().any(|installed| installed.id == format!("{}@{}", plugin.name, builtin_header.id)) {
            installed_catalog.push(InstalledCatalogEntry { market: builtin_header.clone(), plugin: plugin.clone() });
        }
    }
    let builtin_id = markets[0].id.clone();
    markets[0].plugins.retain(|plugin| selected.contains(&format!("{}@{}", plugin.name, builtin_id)));
    for record in &file.marketplaces {
        let mut market = market_header(record);
        // Do not enumerate an unselected upstream catalogue on the ordinary
        // route. Parse only names needed for selected or installed metadata.
        let names = selected.iter().filter_map(|id| id.strip_suffix(&format!("@{}", record.id)).map(str::to_string))
            .chain(file.installed.iter().filter(|plugin| plugin.marketplace_id == record.id).map(|plugin| plugin.name.clone())).collect::<HashSet<_>>();
        if !names.is_empty() {
            match parse_marketplace_filtered(Path::new(&record.path), Some(&names)) {
                Ok((_, _, entries)) => for (plugin, _) in entries {
                    let id = format!("{}@{}", plugin.name, record.id);
                    if file.installed.iter().any(|installed| installed.id == id) {
                        installed_catalog.push(InstalledCatalogEntry { market: market.clone(), plugin: plugin.clone() });
                    }
                    if selected.contains(&id) { market.plugins.push(plugin); }
                },
                Err(error) => market.last_error = Some(format!("{error:#}")),
            }
        }
        markets.push(market);
    }
    Ok(PluginMarketSnapshot { marketplaces: markets, installed: file.installed,
        selection_configured: file.selected_plugin_ids.is_some(), selected_plugin_ids: file.selected_plugin_ids.unwrap_or_default(), installed_catalog })
}

fn cached_component_hint(root: &Path) -> String {
    match scan_plugin_components(root) {
        Ok(components) => {
            let mut parts = Vec::new();
            if !components.skill_dirs.is_empty() { parts.push(format!("Skill ×{}", components.skill_dirs.len())); }
            if let Some(servers) = components.mcp_servers { if !servers.is_empty() { parts.push(format!("MCP ×{}", servers.len())); } }
            if components.has_hooks { parts.push("Hooks".into()); }
            if components.has_agents { parts.push("子 Agent".into()); }
            if parts.is_empty() { "组件未知 · 缓存未识别到可分发组件，安装前核对".into() }
            else { format!("缓存已识别：{}；其他组件安装前核对", parts.join(" · ")) }
        }
        Err(_) => "组件未知 · 缓存无法检查，安装前核对".into(),
    }
}

fn candidates_at(paths: &Paths) -> Result<PluginCandidateSnapshot> {
    let file = load_file_at(&paths.data_file)?;
    let builtin = targets::builtin_view();
    let mut hints = builtin.plugins.iter().map(|plugin| (format!("{}@{}", plugin.name, builtin.id), "1 个 Skill 包 · 固定来源；不含原生插件".into())).collect::<HashMap<_, _>>();
    let mut markets = vec![builtin];
    for record in &file.marketplaces {
        let mut market = market_header(record);
        match parse_marketplace(Path::new(&record.path)) {
            Ok((_, _, entries)) => { market.last_error = None; for (plugin, source) in entries {
                let id = format!("{}@{}", plugin.name, record.id);
                let cached = file.installed.iter().find(|installed| installed.id == id).map(|installed| PathBuf::from(&installed.cache_path)).filter(|root| root.is_dir());
                let local = match source {
                    PluginSourceRef::Relative(relative) => ensure_within(Path::new(&record.path), &Path::new(&record.path).join(relative)).ok().filter(|root| root.is_dir()),
                    _ => None,
                };
                // Candidate browsing never materializes/clones remote sources.
                let hint = plugin.compatibility.as_ref().map(|compatibility| {
                    if compatibility.components.is_empty() { format!("组件未知 · {}", compatibility.note) }
                    else { format!("{}；{}", compatibility.components.join(" · "), compatibility.note) }
                }).unwrap_or_else(|| cached.or(local).map(|root| cached_component_hint(&root)).unwrap_or_else(|| "组件未知 · 尚未下载，安装前检查；挑选不会下载".into()));
                hints.insert(id, hint);
                market.plugins.push(plugin);
            }},
            Err(error) => market.last_error = Some(format!("{error:#}")),
        }
        markets.push(market);
    }
    Ok(PluginCandidateSnapshot { marketplaces: markets, selection_configured: file.selected_plugin_ids.is_some(),
        selected_plugin_ids: file.selected_plugin_ids.unwrap_or_default(), component_hints: hints })
}

fn save_at(paths: &Paths, ids: Vec<String>) -> Result<()> {
    let candidates = candidates_at(paths)?;
    if candidates.marketplaces.iter().any(|market| market.last_error.is_some()) {
        bail!("部分候选来源读取失败，精选名单未修改。请恢复来源后重新挑选");
    }
    let valid = candidates.marketplaces.iter().flat_map(|market| market.plugins.iter().map(|plugin| format!("{}@{}", plugin.name, market.id))).collect::<HashSet<_>>();
    let mut ids = ids;
    if let Some(id) = ids.iter().find(|id| !valid.contains(*id)) { bail!("所选插件不在当前候选目录中：{id}。请重新加载候选后选择"); }
    ids.sort(); ids.dedup();
    let mut file = load_file_at(&paths.data_file)?;
    file.selected_plugin_ids = Some(ids); // Some([]) is an explicit empty choice.
    save_file_at(&paths.data_file, &file)
}

pub fn candidate_snapshot() -> Result<PluginCandidateSnapshot> {
    let _guard = targets::OPERATIONS.lock().map_err(|_| anyhow!("插件操作锁不可用"))?;
    candidates_at(&Paths::standard()?)
}

pub fn save_selection(center: &SkillCenter, ids: Vec<String>) -> Result<PluginMarketSnapshot> {
    let _guard = targets::OPERATIONS.lock().map_err(|_| anyhow!("插件操作锁不可用"))?;
    let paths = Paths::standard()?;
    save_at(&paths, ids)?;
    // Selection does not refresh skill hashes, install anything or change MCP.
    let _ = center;
    curated_snapshot_at(&paths)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selection_is_persistent_explicit_and_preserves_installations() {
        let root = std::env::temp_dir().join(format!("crewvela-curation-{}", uuid::Uuid::new_v4()));
        let paths = Paths::from_config_dir(root.join("config"));
        let center = SkillCenter::open_at(root.join("center")).unwrap();
        let market = root.join("market");
        std::fs::create_dir_all(market.join(".claude-plugin")).unwrap();
        std::fs::create_dir_all(market.join("demo")).unwrap();
        std::fs::write(market.join(".claude-plugin/marketplace.json"), r#"{"name":"test","plugins":[{"name":"demo","source":"./demo"}]}"#).unwrap();
        std::fs::write(market.join("demo/SKILL.md"), "---\nname: demo\ndescription: test\n---\nTest").unwrap();
        add_marketplace_at(&paths, &market.display().to_string()).unwrap();
        let first = curated_snapshot_at(&paths).unwrap();
        assert!(!first.selection_configured);
        assert!(first.marketplaces.iter().all(|market| market.plugins.is_empty()));
        let installed = install_plugin_at(&paths, &center, "test", "demo").unwrap();
        save_at(&paths, vec!["demo@test".into(), "demo@test".into()]).unwrap();
        let selected = curated_snapshot_at(&paths).unwrap();
        assert_eq!(selected.selected_plugin_ids, vec!["demo@test"]);
        assert_eq!(selected.marketplaces.iter().map(|market| market.plugins.len()).sum::<usize>(), 1);
        assert!(save_at(&paths, vec!["unknown@test".into()]).is_err());
        assert_eq!(curated_snapshot_at(&paths).unwrap().selected_plugin_ids, vec!["demo@test"]);
        std::fs::write(market.join(".claude-plugin/marketplace.json"), "invalid").unwrap();
        assert!(save_at(&paths, vec![]).is_err());
        assert_eq!(load_file_at(&paths.data_file).unwrap().selected_plugin_ids.unwrap(), vec!["demo@test"]);
        std::fs::write(market.join(".claude-plugin/marketplace.json"), r#"{"name":"test","plugins":[{"name":"demo","source":"./demo"},{"name":"new","source":"./demo"}]}"#).unwrap();
        assert_eq!(curated_snapshot_at(&paths).unwrap().marketplaces.iter().map(|market| market.plugins.len()).sum::<usize>(), 1);
        let before_assets = center.snapshot().unwrap().assets.len();
        save_at(&paths, vec![]).unwrap();
        let empty = curated_snapshot_at(&paths).unwrap();
        assert!(empty.selection_configured);
        assert!(empty.marketplaces.iter().all(|market| market.plugins.is_empty()));
        assert_eq!(empty.installed[0].id, installed.id);
        assert_eq!(empty.installed_catalog[0].plugin.name, "demo");
        assert_eq!(center.snapshot().unwrap().assets.len(), before_assets);
        assert_eq!(candidates_at(&paths).unwrap().marketplaces.iter().map(|market| market.plugins.len()).sum::<usize>(), 8);
        drop(center);
        let checked_root = root.canonicalize().unwrap();
        assert!(checked_root.starts_with(std::env::temp_dir().canonicalize().unwrap()));
        std::fs::remove_dir_all(checked_root).unwrap();
    }
}
