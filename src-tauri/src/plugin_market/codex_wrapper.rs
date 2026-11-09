use super::*;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
pub(super) const ALIAS: &str = "crewvela-openai-curated";
const MARKER: &str = ".crewvela-market.json";
const CATALOG: &str = ".agents/plugins/marketplace.json";

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Ownership {
    managed_by: String,
    revision: String,
    catalog_hash: String,
    packages: BTreeMap<String, String>,
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let temp = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        std::fs::write(&temp, bytes)?;
        std::fs::rename(&temp, path)?;
        Ok(())
    })();
    if temp.exists() {
        let _ = std::fs::remove_file(temp);
    }
    result
}

fn commit_metadata(
    catalog_path: &Path,
    catalog: &[u8],
    marker_path: &Path,
    marker: &[u8],
) -> Result<()> {
    atomic_write(catalog_path, catalog)?;
    atomic_write(marker_path, marker)
}

pub(super) fn prepare(
    paths: &Paths,
    record: &MarketplaceRecord,
    name: &str,
    source: &PluginSourceRef,
) -> Result<PathBuf> {
    prepare_at_revision(paths, record, name, source, official::REVISION)
}

fn prepare_at_revision(
    paths: &Paths,
    record: &MarketplaceRecord,
    name: &str,
    source: &PluginSourceRef,
    revision: &str,
) -> Result<PathBuf> {
    prepare_with_commit(paths, record, name, source, revision, &mut commit_metadata)
}

fn prepare_with_commit(
    paths: &Paths,
    record: &MarketplaceRecord,
    name: &str,
    source: &PluginSourceRef,
    revision: &str,
    commit: &mut impl FnMut(&Path, &[u8], &Path, &[u8]) -> Result<()>,
) -> Result<PathBuf> {
    if name.is_empty()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        || matches!(name, "." | "..")
    {
        bail!("包装插件名称无效");
    }
    if record.id != official::MARKET
        || record.url.as_deref() != Some(official::URL)
        || record.pinned_revision.as_deref() != Some(revision)
    {
        bail!("保留名官方市场来源或固定提交不符，不能创建自有包装市场");
    }
    let PluginSourceRef::Relative(relative) = source else {
        bail!("官方包装市场仅支持可核验 local package");
    };
    let official_root = Path::new(&record.path);
    {
        let root = official_root.display().to_string();
        if run_git(&["-C", &root, "rev-parse", "HEAD"])?.trim() != revision
            || !run_git(&[
                "-C",
                &root,
                "status",
                "--porcelain",
                "--untracked-files=all",
            ])?
            .trim()
            .is_empty()
        {
            bail!("官方固定提交缓存已变化，拒绝将修改内容作为官方包安装");
        }
    }
    let source = ensure_within(official_root, &official_root.join(relative))?;
    let root = paths
        .market_root
        .join(format!("crewvela-openai-native-{}", &revision[..12]));
    if root.exists() {
        ensure_within(&paths.market_root, &root)?;
    }
    let new_root = !root.exists();
    let old_catalog = if new_root {
        None
    } else {
        Some(std::fs::read(root.join(CATALOG))?)
    };
    let old_marker = if new_root {
        None
    } else {
        Some(
            std::fs::read(root.join(MARKER))
                .context("包装市场目录存在但缺少所有权标记，拒绝修改")?,
        )
    };
    let mut ownership = if !new_root {
        let ownership: Ownership = serde_json::from_str(
            &std::fs::read_to_string(root.join(MARKER))
                .context("包装市场目录存在但缺少所有权标记，拒绝修改")?,
        )?;
        if ownership.managed_by != "crewvela"
            || ownership.revision != revision
            || ownership.catalog_hash != hash(&std::fs::read(root.join(CATALOG))?)
        {
            bail!("自有包装市场来源或目录被修改，拒绝覆盖");
        }
        for (package, digest) in &ownership.packages {
            let package_root = ensure_within(&root, &root.join("plugins").join(package))?;
            if &codex_native::content_hash(&package_root)? != digest {
                bail!("自有包装插件内容已被修改，拒绝覆盖");
            }
        }
        ownership
    } else {
        std::fs::create_dir_all(root.join("plugins"))?;
        std::fs::create_dir_all(root.join(".agents/plugins"))?;
        Ownership {
            managed_by: "crewvela".into(),
            revision: revision.into(),
            catalog_hash: String::new(),
            packages: BTreeMap::new(),
        }
    };
    let target = root.join("plugins").join(name);
    let source_hash = codex_native::content_hash(&source)?;
    if let Some(digest) = ownership.packages.get(name) {
        if digest != &source_hash {
            bail!("官方缓存包与包装目录来源不同，拒绝替换");
        }
        return Ok(root);
    }
    if target.exists() {
        bail!("包装目录有未受管同名插件，拒绝覆盖");
    }
    let staging = root
        .join("plugins")
        .join(format!(".tmp-{}", uuid::Uuid::new_v4()));
    let copy = (|| {
        copy_dir_recursive(&source, &staging)?;
        if codex_native::content_hash(&staging)? != source_hash {
            bail!("包装复制内容校验失败");
        }
        std::fs::rename(&staging, &target)?;
        Ok(())
    })();
    if let Err(error) = copy {
        if staging.exists() {
            ensure_within(&root, &staging)?;
            std::fs::remove_dir_all(&staging)?;
        }
        if new_root {
            ensure_within(&paths.market_root, &root)?;
            std::fs::remove_dir_all(&root)?;
        }
        return Err(error);
    }
    ownership.packages.insert(name.into(), source_hash);
    let plugins = ownership.packages.keys().map(|name| serde_json::json!({"name": name,"source":{"source":"local","path":format!("./plugins/{name}")}})).collect::<Vec<_>>();
    let catalog = serde_json::to_vec_pretty(
        &serde_json::json!({"name":ALIAS,"owner":{"name":"CrewVela"},"metadata":{"description":format!("CrewVela 自有包装目录；上游 OpenAI plugins @ {}，保留原包资源和许可",revision)},"plugins":plugins}),
    )?;
    ownership.catalog_hash = hash(&catalog);
    let marker = serde_json::to_vec_pretty(&ownership)?;
    if let Err(error) = commit(&root.join(CATALOG), &catalog, &root.join(MARKER), &marker) {
        // Roll back only the package just copied and the verified metadata bytes.
        // An existing package and other registered plugins are never touched.
        ensure_within(&root, &target)?;
        std::fs::remove_dir_all(&target)?;
        if let (Some(catalog), Some(marker)) = (old_catalog, old_marker) {
            atomic_write(&root.join(CATALOG), &catalog)?;
            atomic_write(&root.join(MARKER), &marker)?;
        } else {
            ensure_within(&paths.market_root, &root)?;
            std::fs::remove_dir_all(&root)?;
        }
        return Err(error.context("包装目录保存失败，已撤销本次追加"));
    }
    Ok(root)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wrapper_preserves_selected_packages_resources_and_ownership_without_impersonating_official()
    {
        let root = std::env::temp_dir().join(format!("crewvela-wrapper-{}", uuid::Uuid::new_v4()));
        let source = root.join("official");
        for name in ["remotion", "product-design"] {
            let package = source.join("plugins").join(name);
            std::fs::create_dir_all(package.join(".codex-plugin")).unwrap();
            std::fs::create_dir_all(package.join("references")).unwrap();
            std::fs::create_dir_all(package.join("skills/demo")).unwrap();
            std::fs::write(
                package.join(".codex-plugin/plugin.json"),
                format!(r#"{{"name":"{name}","version":"1.0.0"}}"#),
            )
            .unwrap();
            std::fs::write(
                package.join("references/guide.md"),
                "root references intact",
            )
            .unwrap();
            std::fs::write(
                package.join("skills/demo/SKILL.md"),
                "../../references/guide.md",
            )
            .unwrap();
        }
        run_git(&["-C", &source.display().to_string(), "init"]).unwrap();
        run_git(&[
            "-C",
            &source.display().to_string(),
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "add",
            ".",
        ])
        .unwrap();
        run_git(&[
            "-C",
            &source.display().to_string(),
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "-m",
            "fixture",
        ])
        .unwrap();
        let revision = run_git(&["-C", &source.display().to_string(), "rev-parse", "HEAD"])
            .unwrap()
            .trim()
            .to_string();
        let paths = Paths::from_config_dir(root.join("config"));
        let record = MarketplaceRecord {
            id: official::MARKET.into(),
            source: official::URL.into(),
            source_kind: "git".into(),
            owner_name: "OpenAI".into(),
            path: source.display().to_string(),
            url: Some(official::URL.into()),
            added_at: 0,
            last_refreshed_at: None,
            last_error: None,
            pinned_revision: Some(revision.clone()),
        };
        let wrapper = prepare_at_revision(
            &paths,
            &record,
            "remotion",
            &PluginSourceRef::Relative("plugins/remotion".into()),
            &revision,
        )
        .unwrap();
        let (id, _, first) = parse_marketplace(&wrapper).unwrap();
        assert_eq!(id, ALIAS);
        assert_ne!(id, official::MARKET);
        assert_eq!(first.len(), 1);
        assert!(!wrapper.join("plugins/product-design").exists());
        let before_catalog = std::fs::read(wrapper.join(CATALOG)).unwrap();
        let before_marker = std::fs::read(wrapper.join(MARKER)).unwrap();
        assert!(prepare_with_commit(
            &paths,
            &record,
            "product-design",
            &PluginSourceRef::Relative("plugins/product-design".into()),
            &revision,
            &mut |catalog_path, catalog, _, _| {
                atomic_write(catalog_path, catalog)?;
                bail!("injected marker write failure")
            }
        )
        .is_err());
        assert_eq!(
            std::fs::read(wrapper.join(CATALOG)).unwrap(),
            before_catalog
        );
        assert_eq!(std::fs::read(wrapper.join(MARKER)).unwrap(), before_marker);
        assert!(!wrapper.join("plugins/product-design").exists());
        assert!(wrapper
            .join("plugins/remotion/references/guide.md")
            .is_file());
        prepare_at_revision(
            &paths,
            &record,
            "product-design",
            &PluginSourceRef::Relative("plugins/product-design".into()),
            &revision,
        )
        .unwrap();
        assert_eq!(parse_marketplace(&wrapper).unwrap().2.len(), 2);
        assert_eq!(
            std::fs::read_to_string(wrapper.join("plugins/product-design/references/guide.md"))
                .unwrap(),
            "root references intact"
        );
        std::fs::write(
            wrapper.join("plugins/remotion/references/guide.md"),
            "external edit",
        )
        .unwrap();
        assert!(prepare_at_revision(
            &paths,
            &record,
            "remotion",
            &PluginSourceRef::Relative("plugins/remotion".into()),
            &revision
        )
        .unwrap_err()
        .to_string()
        .contains("修改"));
        std::fs::remove_dir_all(root).unwrap();
    }
}
