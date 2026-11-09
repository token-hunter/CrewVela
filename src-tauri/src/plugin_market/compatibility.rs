use super::*;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginCompatibility {
    pub format: String,
    pub status: String,
    pub native_codex: bool,
    pub native_claude: bool,
    pub skill_compatible: bool,
    pub components: Vec<String>,
    pub note: String,
    pub dependencies: Vec<String>,
    pub license: Option<String>,
}

impl PluginCompatibility {
    pub(super) fn builtin_skill() -> Self {
        Self {
            format: "skill".into(),
            status: "skill_compatible".into(),
            native_codex: false,
            native_claude: false,
            skill_compatible: true,
            components: vec!["1 个 Skill 包".into()],
            note: "已验证独立 Skill 目录；依赖和账号需按上游说明配置".into(),
            dependencies: Vec::new(),
            license: None,
        }
    }
}

pub(super) fn read_manifest(root: &Path) -> Result<(String, Option<Value>)> {
    for (format, relative) in [
        ("codex", ".codex-plugin/plugin.json"),
        ("portable", ".agents/plugins/plugin.json"),
        ("claude", ".claude-plugin/plugin.json"),
        ("portable", "plugin.json"),
    ] {
        let path = root.join(relative);
        if path.is_file() {
            let value = serde_json::from_str(&std::fs::read_to_string(&path)?)
                .context("插件 manifest JSON 无效")?;
            return Ok((format.into(), Some(value)));
        }
    }
    Ok(("unknown".into(), None))
}

fn present(value: Option<&Value>) -> bool {
    value.is_some_and(|value| match value {
        Value::Null => false,
        Value::Object(items) => !items.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::String(value) => !value.is_empty(),
        Value::Bool(value) => *value,
        _ => true,
    })
}

pub(super) fn inspect(root: &Path) -> Result<PluginCompatibility> {
    let (format, manifest) = read_manifest(root)?;
    let manifest = manifest.unwrap_or(Value::Null);
    let components = scan_plugin_components(root)?;
    let mut labels = Vec::new();
    if !components.skill_dirs.is_empty() {
        labels.push(format!("Skill ×{}", components.skill_dirs.len()));
    }
    let has_mcp = root.join(".mcp.json").is_file() || root.join("mcp.json").is_file() || components
        .mcp_servers
        .as_ref()
        .is_some_and(|servers| !servers.is_empty())
        || present(manifest.get("mcpServers"))
        || present(manifest.get("mcp"));
    let has_apps = root.join(".app.json").is_file()
        || root.join(".apps.json").is_file()
        || present(manifest.get("apps"));
    let hooks = components.has_hooks || present(manifest.get("hooks"));
    let agents = components.has_agents || present(manifest.get("agents"));
    let commands = root.join("commands").is_dir() || present(manifest.get("commands"));
    if has_mcp {
        labels.push("MCP（未连接）".into());
    }
    if has_apps {
        labels.push("Apps / 连接器（待授权）".into());
    }
    if hooks {
        labels.push("Hooks".into());
    }
    if agents {
        labels.push("子 Agent".into());
    }
    if commands {
        labels.push("Commands".into());
    }
    let is_codex = format == "codex";
    let is_claude = format == "claude";
    let standalone_skills = !components.skill_dirs.is_empty()
        && components.skill_dirs.iter().all(|directory| {
            locate_skill(directory)
                .and_then(|path| std::fs::read_to_string(path).ok())
                .is_some_and(|text| {
                    !text.contains("../") && !text.contains("..\\") && !text.contains("PLUGIN_ROOT")
                })
        });
    let pure_skill = standalone_skills && !has_mcp && !has_apps && !hooks && !agents && !commands;
    // Codex/portable bundles may reference plugin-root files or siblings and may
    // require host-only tools. Do not flatten them into independent Skill assets.
    let skill_compatible = !is_codex && format != "portable" && pure_skill;
    let status = if has_apps || has_mcp {
        "needs_auth"
    } else if is_codex || is_claude {
        "native"
    } else if skill_compatible {
        "skill_compatible"
    } else {
        "unsupported"
    };
    let note = if has_apps || has_mcp {
        "需要账号或服务授权；本应用只展示，不自动安装连接器、登录或启用 MCP"
    } else if is_codex {
        "Codex 原生整包安装保留根目录和相对引用；其他 Agent 的工具映射与兼容 Skill 尚未验证"
    } else if format == "portable" {
        "Portable 插件需宿主适配；当前尚未验证安装通路"
    } else if skill_compatible {
        "可按独立 Skill 分发；不代表外部依赖或工具已配置"
    } else if is_claude {
        "Claude 原生整包由 CLI 校验；组件分发仅覆盖可验证的独立 Skill"
    } else {
        "组件或宿主依赖尚未适配，不能据此判断可安装"
    };
    let name = manifest
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let dependencies = if name == "product-design" {
        vec![
            "需要 Codex 宿主的 Browser、ImageGen、Sites 工具".into(),
            "需要正确的用户上下文与账号授权；其它 Agent 工具映射未适配".into(),
        ]
    } else if name == "remotion" {
        vec![
            "部分流程需要 Codex in-app browser 与配套宿主工具".into(),
            "Node/Remotion 与渲染依赖需按包内说明安装；插件安装不代表成片验收".into(),
        ]
    } else {
        Vec::new()
    };
    let license = manifest
        .get("license")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            std::fs::read_to_string(root.join("LICENSE"))
                .ok()
                .and_then(|text| {
                    if text.contains("MIT License") {
                        Some("MIT".into())
                    } else if text.contains("Apache License") {
                        Some("Apache-2.0".into())
                    } else {
                        None
                    }
                })
        })
        .or_else(|| {
            (name == "product-design").then(|| "专有许可 · 请先查阅上游 LICENSE 和使用范围".into())
        });
    Ok(PluginCompatibility {
        format,
        status: status.into(),
        native_codex: is_codex && !has_apps && !has_mcp,
        native_claude: is_claude && !has_apps && !has_mcp,
        skill_compatible,
        components: labels,
        note: note.into(),
        dependencies,
        license,
    })
}

pub(super) fn enrich_view(market: &Path, source: &PluginSourceRef, view: &mut MarketPluginView) {
    let PluginSourceRef::Relative(relative) = source else {
        return;
    };
    let root = match ensure_within(market, &market.join(relative)) {
        Ok(root) if root.is_dir() => root,
        _ => {
            view.installable = false;
            view.note = Some("本地来源目录无效或越过市场边界".into());
            return;
        }
    };
    match inspect(&root) {
        Ok(compatibility) => {
            if let Ok((_, Some(manifest))) = read_manifest(&root) {
                view.version = manifest
                    .get("version")
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .or(view.version.take());
                let interface = manifest.get("interface");
                view.display_name = interface
                    .and_then(|value| value.get("displayName"))
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .or(view.display_name.take());
                if view.description.is_empty() {
                    view.description = interface
                        .and_then(|value| value.get("shortDescription"))
                        .or_else(|| manifest.get("description"))
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .into();
                }
            }
            view.installable = compatibility.native_codex
                || compatibility.native_claude
                || compatibility.skill_compatible;
            view.note = Some(compatibility.note.clone());
            view.compatibility = Some(compatibility);
        }
        Err(_) => {
            view.installable = false;
            view.note = Some("manifest 或组件无法解析，尚未适配".into());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_codex_manifest_metadata_connector_boundary_and_relative_paths_are_honest() {
        let root = std::env::temp_dir().join(format!("crewvela-manifest-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join(".agents/plugins")).unwrap();
        for name in ["remotion", "product-design", "github"] {
            let plugin = root.join("plugins").join(name);
            std::fs::create_dir_all(plugin.join(".codex-plugin")).unwrap();
            std::fs::create_dir_all(plugin.join("skills/demo")).unwrap();
            std::fs::write(
                plugin.join("skills/demo/SKILL.md"),
                "Uses ../../references/guide.md",
            )
            .unwrap();
            std::fs::create_dir_all(plugin.join("references")).unwrap();
            std::fs::write(plugin.join("references/guide.md"), "root resource").unwrap();
            std::fs::write(plugin.join(".codex-plugin/plugin.json"), serde_json::to_vec(&serde_json::json!({"name":name,"version":"1.2.3","interface":{"displayName":format!("Display {name}"),"shortDescription":"Short description"},"skills":"./skills/","hooks":{}})).unwrap()).unwrap();
        }
        std::fs::write(root.join("plugins/github/.app.json"), "{}").unwrap();
        std::fs::write(root.join(".agents/plugins/marketplace.json"), r#"{"name":"test","plugins":[{"name":"remotion","source":{"source":"local","path":"./plugins/remotion"}},{"name":"product-design","source":{"source":"local","path":"./plugins/product-design"}},{"name":"github","source":{"source":"local","path":"./plugins/github"}},{"name":"escape","source":{"source":"local","path":"../outside"}}]}"#).unwrap();
        let (_, _, entries) = parse_marketplace(&root).unwrap();
        let remotion = &entries[0].0;
        assert_eq!(remotion.display_name.as_deref(), Some("Display remotion"));
        assert_eq!(remotion.version.as_deref(), Some("1.2.3"));
        assert_eq!(remotion.description, "Short description");
        let compatibility = remotion.compatibility.as_ref().unwrap();
        assert!(
            compatibility.native_codex
                && !compatibility.native_claude
                && !compatibility.skill_compatible
        );
        assert_eq!(compatibility.components, ["Skill ×1"]);
        assert!(!entries[1]
            .0
            .compatibility
            .as_ref()
            .unwrap()
            .dependencies
            .is_empty());
        assert!(entries[1]
            .0
            .compatibility
            .as_ref()
            .unwrap()
            .license
            .as_ref()
            .unwrap()
            .contains("专有"));
        assert_eq!(
            entries[2].0.compatibility.as_ref().unwrap().status,
            "needs_auth"
        );
        assert!(!entries[2].0.installable);
        assert!(!entries[3].0.installable);
        let paths = Paths::from_config_dir(root.join("config"));
        let cache = materialize_plugin(&paths, &root, "remotion", &entries[0].1).unwrap();
        assert_eq!(
            std::fs::read_to_string(cache.join("references/guide.md")).unwrap(),
            "root resource"
        );
        assert!(materialize_plugin(&paths, &root, "escape", &entries[3].1).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
