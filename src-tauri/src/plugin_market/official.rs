use super::*;
pub(super) const URL: &str = "https://github.com/openai/plugins.git";
pub(super) const REVISION: &str = "0722921d5542fc593105c27bd52630babd8b8c2a";
pub(super) const MARKET: &str = "openai-curated";

fn add_at(paths: &Paths) -> Result<MarketplaceView> {
    let mut file = load_file_at(&paths.data_file)?;
    if let Some(existing) = file.marketplaces.iter().find(|record| record.id == MARKET) {
        if existing.url.as_deref() != Some(URL)
            || existing.pinned_revision.as_deref() != Some(REVISION)
        {
            bail!("已有同名市场不是此固定官方来源；请先在高级来源中检查，应用不会覆盖它");
        }
        return view_of(existing);
    }
    std::fs::create_dir_all(&paths.market_root)?;
    let temp = paths
        .market_root
        .join(format!("official-{}", uuid::Uuid::new_v4()));
    let destination = paths
        .market_root
        .join(format!("openai-curated-{}", &REVISION[..12]));
    let mut owns_destination = false;
    let result = (|| {
        clone_repo(URL, Some(REVISION), &temp)?;
        let (id, owner, _) = parse_marketplace(&temp)?;
        if id != MARKET {
            bail!("官方市场标识与固定版本不符");
        }
        if destination.exists() {
            bail!("官方候选缓存目录已存在但未受管，拒绝覆盖");
        }
        std::fs::rename(&temp, &destination)?;
        owns_destination = true;
        let record = MarketplaceRecord {
            id,
            source: format!("openai/plugins @ {REVISION}"),
            source_kind: "git".into(),
            owner_name: owner,
            path: destination.display().to_string(),
            url: Some(URL.into()),
            added_at: now_millis(),
            last_refreshed_at: Some(now_millis()),
            last_error: None,
            pinned_revision: Some(REVISION.into()),
        };
        file.marketplaces.push(record.clone());
        save_file_at(&paths.data_file, &file)?;
        view_of(&record)
    })();
    if temp.exists() {
        let _ = std::fs::remove_dir_all(&temp);
    }
    if result.is_err() && owns_destination && destination.exists() && !load_file_at(&paths.data_file)?.marketplaces.iter().any(|record| record.path == destination.display().to_string()) {
        ensure_within(&paths.market_root, &destination)?;
        std::fs::remove_dir_all(&destination)?;
    }
    result
}

pub fn add_official() -> Result<MarketplaceView> {
    let _guard = targets::OPERATIONS
        .lock()
        .map_err(|_| anyhow!("插件操作锁不可用"))?;
    add_at(&Paths::standard()?)
}

#[cfg(all(test, windows))]
mod long_path_remote_tests {
    use super::*;

    #[test]
    #[ignore = "requires GitHub network; run explicitly for Windows long-path acceptance"]
    fn actual_official_market_long_path_add_and_repeat() {
        let fixture = super::super::long_path_tests::Fixture::new();
        let paths = Paths::from_config_dir(fixture.config.clone());
        let first = add_at(&paths).unwrap();
        assert_eq!(first.id, MARKET);
        assert!(first.plugins.iter().any(|plugin| plugin.name == "remotion"));
        assert!(first.plugins.iter().any(|plugin| plugin.name == "product-design"));
        let root = Path::new(&first.path);
        let deep = root.join("plugins/nvidia/skills/omniverse-usd-performance-tuning/references/usd-structure-assessment/references/usd-hierarchy-dedupe-candidates/references/instance-candidate-finder-spec.md");
        assert!(deep.as_os_str().len() > 260);
        assert!(!std::fs::read(&deep).unwrap().is_empty());
        let repeat = add_at(&paths).unwrap();
        assert_eq!(first.path, repeat.path);
        assert_eq!(first.plugins.len(), repeat.plugins.len());
        let file = load_file_at(&paths.data_file).unwrap();
        assert_eq!(file.marketplaces.len(), 1);
        assert!(file.selected_plugin_ids.is_none());
        assert!(std::fs::read_dir(&paths.market_root).unwrap().all(|entry| !entry.unwrap().file_name().to_string_lossy().starts_with("official-")));
        println!("Official add/repeat PASS: {} candidates; deep file {} characters", first.plugins.len(), deep.as_os_str().len());
    }
}
