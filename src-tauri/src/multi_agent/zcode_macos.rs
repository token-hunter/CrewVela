//! macOS Desktop discovery never resolves the independent PATH CLI.
use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

#[cfg(test)]
#[path = "zcode_macos_acceptance.rs"]
mod acceptance;

pub(super) struct Bundle {
    pub agent: PathBuf,
    pub main_program: PathBuf,
    pub program: PathBuf,
    pub builtin: PathBuf,
    pub version: Option<String>,
}

pub(super) fn bundle_root(path: &Path) -> Option<&Path> {
    path.ancestors().find(|parent| parent.extension().is_some_and(|ext| ext == "app"))
}

fn bundle_info(root: &Path, label: &str) -> Result<Value> {
    let output = Command::new("/usr/bin/plutil")
        .args(["-convert", "json", "-o", "-"])
        .arg(root.join("Contents/Info.plist"))
        .output().with_context(|| format!("无法读取 ZCode Desktop {label}元数据"))?;
    if !output.status.success() {
        bail!("ZCode Desktop {label} Info.plist 无效，请重新安装或指定正确应用包");
    }
    serde_json::from_slice(&output.stdout)
        .with_context(|| format!("ZCode Desktop {label}元数据格式无效"))
}

fn bundle_program(root: &Path, info: &Value, label: &str) -> Result<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let executable = info["CFBundleExecutable"].as_str().filter(|name| {
        let mut components = Path::new(name).components();
        matches!(components.next(), Some(Component::Normal(_))) && components.next().is_none()
    }).with_context(|| format!("ZCode Desktop {label}缺少有效 CFBundleExecutable"))?;
    let program = root.join("Contents/MacOS").join(executable);
    if !program.is_file() {
        bail!("ZCode Desktop {label}缺少可执行文件，请重新安装应用");
    }
    if std::fs::metadata(&program)?.permissions().mode() & 0o111 == 0 {
        bail!("ZCode Desktop {label}可执行文件缺少执行权限，请重新安装应用");
    }
    Ok(program)
}

pub(super) fn bundle(root: &Path) -> Result<Bundle> {
    if !root.is_absolute() || !root.is_dir() {
        bail!("未找到 ZCode Desktop 应用包：请指定现有的绝对 .app 路径");
    }
    let info = bundle_info(root, "应用包")?;
    if info["CFBundleIdentifier"] != "dev.zcode.app" {
        bail!("指定应用包不是 ZCode Desktop（Bundle Identifier 不匹配）");
    }
    let main_program = bundle_program(root, &info, "主程序")?;
    // LaunchServices treats the GUI executable as a foreground application once
    // the Agent changes process.title. Use the packaged background Electron
    // Helper for both the Agent and any process.execPath children instead.
    let helper = root.join("Contents/Frameworks")
        .join(format!("{} Helper.app", info["CFBundleExecutable"].as_str().unwrap()));
    if !helper.is_dir() {
        bail!("ZCode Desktop 缺少后台 Helper 应用包；请重新安装或更新应用，不能使用前台主程序运行 Agent");
    }
    let helper_info = bundle_info(&helper, "后台 Helper")?;
    if helper_info["CFBundleIdentifier"] != "dev.zcode.app.helper" {
        bail!("ZCode Desktop 后台 Helper Bundle Identifier 不匹配，请重新安装或更新应用");
    }
    if helper_info["LSUIElement"] != true && helper_info["LSBackgroundOnly"] != true {
        bail!("ZCode Desktop Helper 未声明后台模式（LSUIElement / LSBackgroundOnly）；请重新安装或更新应用");
    }
    let program = bundle_program(&helper, &helper_info, "后台 Helper")?;
    let resources = root.join("Contents/Resources");
    let agent = resources.join("glm/zcode.cjs");
    if !agent.is_file() {
        bail!("已找到 ZCode Desktop 应用，但缺少 Agent 资源 Contents/Resources/glm/zcode.cjs；请重新安装或更新应用");
    }
    let builtin = resources.join("config/provider/zcode-builtin.json");
    if !builtin.is_file() {
        bail!("ZCode Desktop 应用包缺少内置 Provider 配置；请重新安装或更新应用");
    }
    Ok(Bundle { agent, main_program, program, builtin, version: info["CFBundleShortVersionString"].as_str().map(str::to_owned) })
}

pub(super) fn explicit_executable(path: &Path) -> Result<PathBuf> {
    if let Some(root) = bundle_root(path) {
        let app = bundle(root)?;
        // Accept the .app itself, its declared executable, or its Agent only.
        if path == root || path == app.main_program || path == app.agent {
            return Ok(app.agent);
        }
        bail!("CREWVELA_ZCODE_EXECUTABLE 必须指向 ZCode Desktop 应用包、主程序或 Agent 入口");
    }
    if !path.is_absolute() || !path.is_file() {
        bail!("CREWVELA_ZCODE_EXECUTABLE 必须指向现有的绝对 Agent 可执行文件、预构建 cjs 或 .app 应用包");
    }
    Ok(path.to_path_buf())
}

pub(super) fn explicit_root(root: &Path) -> Result<PathBuf> {
    if !root.is_absolute() || !root.is_dir() {
        bail!("CREWVELA_ZCODE_ROOT 必须指向现有的绝对安装目录或 .app 应用包");
    }
    if let Some(app) = bundle_root(root) {
        if root != app && root != app.join("Contents") && root != app.join("Contents/Resources") {
            bail!("CREWVELA_ZCODE_ROOT 应指向 .app、Contents 或 Contents/Resources 目录");
        }
        return Ok(bundle(app)?.agent);
    }
    for relative in ["resources/glm/zcode.cjs", "apps/zcode-cli/packages/cli/dist/zcode.cjs"] {
        let entry = root.join(relative);
        if root.is_absolute() && entry.is_file() { return Ok(entry); }
    }
    bail!("CREWVELA_ZCODE_ROOT 必须指向 ZCode Desktop .app 应用包、桌面安装目录或已构建源码目录");
}

fn standard_candidates(system: &Path, home: Option<&Path>) -> Vec<PathBuf> {
    let mut paths = vec![system.join("ZCode.app")];
    if let Some(home) = home { paths.push(home.join("Applications/ZCode.app")); }
    paths
}

fn discover(paths: impl IntoIterator<Item = PathBuf>) -> Result<PathBuf> {
    let mut failure = None;
    for path in paths {
        if !path.is_dir() { continue; }
        match bundle(&path) {
            Ok(app) => return Ok(app.agent),
            Err(error) => if failure.is_none() { failure = Some(error); },
        }
    }
    if let Some(error) = failure { return Err(error); }
    bail!("未找到 ZCode Desktop 应用；请安装到 /Applications 或 ~/Applications，或指定 CREWVELA_ZCODE_EXECUTABLE / CREWVELA_ZCODE_ROOT 为 .app 路径");
}

fn overrides(executable: Option<&std::ffi::OsStr>, root: Option<&std::ffi::OsStr>) -> Option<Result<PathBuf>> {
    if let Some(path) = executable {
        return Some(explicit_executable(Path::new(path)));
    }
    if let Some(root) = root {
        return Some(explicit_root(Path::new(root)));
    }
    None
}

pub(super) fn executable() -> Result<PathBuf> {
    if let Some(result) = overrides(std::env::var_os("CREWVELA_ZCODE_EXECUTABLE").as_deref(), std::env::var_os("CREWVELA_ZCODE_ROOT").as_deref()) {
        return result;
    }
    let mut paths = standard_candidates(Path::new("/Applications"), dirs::home_dir().as_deref());
    // Spotlight knows renamed/moved bundles outside the standard application folders.
    if let Ok(output) = Command::new("/usr/bin/mdfind")
        .args(["-0", "kMDItemCFBundleIdentifier == 'dev.zcode.app'"]).output()
    {
        if output.status.success() {
            use std::os::unix::ffi::OsStrExt;
            paths.extend(output.stdout.split(|byte| *byte == 0).filter(|path| !path.is_empty())
                .map(|path| PathBuf::from(std::ffi::OsStr::from_bytes(path))));
        }
    }
    discover(paths)
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture(PathBuf);
    impl Drop for Fixture { fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.0); } }
    fn fixture() -> Fixture {
        Fixture(std::env::temp_dir().join(format!("crewvela-zcode-macos-{}", uuid::Uuid::new_v4())))
    }
    fn make_bundle(path: &Path) {
        use std::os::unix::fs::PermissionsExt;
        for relative in ["Contents/MacOS", "Contents/Resources/glm", "Contents/Resources/config/provider"] {
            std::fs::create_dir_all(path.join(relative)).unwrap();
        }
        std::fs::write(path.join("Contents/Info.plist"), r#"<?xml version="1.0"?><plist version="1.0"><dict><key>CFBundleIdentifier</key><string>dev.zcode.app</string><key>CFBundleExecutable</key><string>自带 Electron</string><key>CFBundleShortVersionString</key><string>3.17.2</string></dict></plist>"#).unwrap();
        for relative in ["Contents/MacOS/自带 Electron", "Contents/Resources/glm/zcode.cjs", "Contents/Resources/config/provider/zcode-builtin.json"] {
            std::fs::write(path.join(relative), "").unwrap();
        }
        std::fs::set_permissions(path.join("Contents/MacOS/自带 Electron"), std::fs::Permissions::from_mode(0o755)).unwrap();
        let helper = path.join("Contents/Frameworks/自带 Electron Helper.app");
        std::fs::create_dir_all(helper.join("Contents/MacOS")).unwrap();
        std::fs::write(helper.join("Contents/Info.plist"), r#"<?xml version="1.0"?><plist version="1.0"><dict><key>CFBundleIdentifier</key><string>dev.zcode.app.helper</string><key>CFBundleExecutable</key><string>后台 Electron 中文</string><key>LSUIElement</key><true/></dict></plist>"#).unwrap();
        std::fs::write(helper.join("Contents/MacOS/后台 Electron 中文"), "").unwrap();
        std::fs::set_permissions(helper.join("Contents/MacOS/后台 Electron 中文"), std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    #[test]
    fn zcode_macos_discovers_system_user_and_custom_bundle_without_path_cli() {
        let temp = fixture();
        let system = temp.0.join("系统 Applications");
        let home = temp.0.join("中文 用户");
        let paths = standard_candidates(&system, Some(&home));
        make_bundle(&paths[1]);
        assert_eq!(discover(paths.clone()).unwrap(), bundle(&paths[1]).unwrap().agent);
        make_bundle(&paths[0]);
        assert_eq!(discover(paths).unwrap(), bundle(&system.join("ZCode.app")).unwrap().agent);
        let custom = temp.0.join("其他位置/改名 ZCode.app");
        make_bundle(&custom);
        assert_eq!(discover([custom.clone()]).unwrap(), bundle(&custom).unwrap().agent);
    }
    #[test]
    fn zcode_macos_explicit_paths_and_launch_use_bundle_metadata() {
        let temp = fixture();
        let root = temp.0.join("中文 空格/ZCode.app");
        make_bundle(&root);
        let app = bundle(&root).unwrap();
        for entry in [&root, &app.main_program, &app.agent] {
            assert_eq!(explicit_executable(entry).unwrap(), app.agent);
        }
        for entry in [&root, &root.join("Contents"), &root.join("Contents/Resources")] {
            assert_eq!(explicit_root(entry).unwrap(), app.agent);
        }
        assert!(explicit_root(&root.join("不存在")).is_err());
        assert!(explicit_root(&root.join("Contents/Resources/glm")).is_err());
        let command = super::super::launch(&app.agent).unwrap();
        assert_eq!(command.get_program(), app.program.as_os_str());
        assert_ne!(command.get_program(), app.main_program.as_os_str());
        assert!(app.program.ends_with("Contents/MacOS/后台 Electron 中文"));
        assert_eq!(command.get_args().collect::<Vec<_>>(), [app.agent.as_os_str(), std::ffi::OsStr::new("app-server"), std::ffi::OsStr::new("--stdio")]);
        assert!(command.get_envs().any(|(key, value)| key == "ELECTRON_RUN_AS_NODE" && value == Some(std::ffi::OsStr::new("1"))));
        assert!(command.get_envs().any(|(key, value)| key == "NODE_OPTIONS" && value.is_none()));
        if std::env::var_os("ZCODE_BUILTIN_PROVIDER_CONFIG_FILE").is_none() {
            assert!(command.get_envs().any(|(key, value)| key == "ZCODE_BUILTIN_PROVIDER_CONFIG_FILE" && value == Some(app.builtin.as_os_str())));
        }
        if std::env::var_os("CREWVELA_ZCODE_APP_VERSION").is_none() {
            assert!(command.get_envs().any(|(key, value)| key == "CREWVELA_ZCODE_APP_VERSION" && value == Some(std::ffi::OsStr::new("3.17.2"))));
        }
        assert_eq!(super::super::node_helper_program(&command).unwrap(), app.program);
    }
    #[test]
    fn zcode_macos_invalid_bundle_reports_resources_not_missing_installation() {
        let temp = fixture();
        assert!(explicit_root(Path::new("relative.app")).is_err());
        assert!(explicit_executable(&temp.0.join("missing")).is_err());
        assert!(discover([temp.0.join("missing.app")]).unwrap_err().to_string().contains("未找到"));
        let root = temp.0.join("ZCode.app");
        make_bundle(&root);
        std::fs::remove_file(root.join("Contents/Resources/glm/zcode.cjs")).unwrap();
        assert!(discover([root.clone()]).unwrap_err().to_string().contains("缺少 Agent 资源"));
        assert!(explicit_root(&root).unwrap_err().to_string().contains("缺少 Agent 资源"));
    }
    #[test]
    fn zcode_macos_rejects_missing_provider_and_invalid_metadata() {
        let temp = fixture();
        let root = temp.0.join("ZCode.app");
        make_bundle(&root);
        let provider = root.join("Contents/Resources/config/provider/zcode-builtin.json");
        std::fs::remove_file(&provider).unwrap();
        assert!(bundle(&root).err().unwrap().to_string().contains("Provider 配置"));
        std::fs::write(provider, "").unwrap();
        let plist = root.join("Contents/Info.plist");
        let original = std::fs::read_to_string(&plist).unwrap();
        std::fs::write(&plist, original.replace("dev.zcode.app", "other.app")).unwrap();
        assert!(bundle(&root).err().unwrap().to_string().contains("不是 ZCode Desktop"));
        std::fs::write(&plist, original.replace("自带 Electron", "../../other")).unwrap();
        assert!(bundle(&root).err().unwrap().to_string().contains("CFBundleExecutable"));
        std::fs::write(&plist, "broken plist").unwrap();
        assert!(bundle(&root).err().unwrap().to_string().contains("Info.plist 无效"));
    }
    #[test]
    fn zcode_macos_environment_overrides_have_priority_and_do_not_fall_through() {
        let temp = fixture();
        let root = temp.0.join("ZCode.app");
        make_bundle(&root);
        let custom = temp.0.join("独立 编译入口.cjs");
        std::fs::write(&custom, "").unwrap();
        let missing = temp.0.join("missing.cjs");
        assert_eq!(overrides(Some(custom.as_os_str()), Some(root.as_os_str())).unwrap().unwrap(), custom);
        assert!(overrides(Some(missing.as_os_str()), Some(root.as_os_str())).unwrap().is_err());
        assert_eq!(overrides(None, Some(root.as_os_str())).unwrap().unwrap(), bundle(&root).unwrap().agent);
        assert!(overrides(None, Some(missing.as_os_str())).unwrap().is_err());
        assert!(overrides(None, None).is_none());
        let source = temp.0.join("源码 root");
        let source_agent = source.join("apps/zcode-cli/packages/cli/dist/zcode.cjs");
        std::fs::create_dir_all(source_agent.parent().unwrap()).unwrap();
        std::fs::write(&source_agent, "").unwrap();
        assert_eq!(explicit_root(&source).unwrap(), source_agent);
    }
    #[test]
    fn zcode_macos_requires_background_helper_without_gui_fallback() {
        use std::os::unix::fs::PermissionsExt;
        let temp = fixture();
        let root = temp.0.join("ZCode.app");
        make_bundle(&root);
        let helper = root.join("Contents/Frameworks/自带 Electron Helper.app");
        let plist = helper.join("Contents/Info.plist");
        let original = std::fs::read_to_string(&plist).unwrap();
        for (content, expected) in [
            (original.replace("dev.zcode.app.helper", "other.helper"), "Bundle Identifier"),
            (original.replace("<true/>", "<false/>"), "未声明后台模式"),
            (original.replace("<true/>", "<string>true</string>"), "未声明后台模式"),
            (original.replace("后台 Electron 中文", "../outside"), "CFBundleExecutable"),
            ("broken plist".to_owned(), "Info.plist 无效"),
        ] {
            std::fs::write(&plist, content).unwrap();
            assert!(bundle(&root).err().unwrap().to_string().contains(expected));
            assert!(super::super::launch(&root.join("Contents/Resources/glm/zcode.cjs")).is_err());
        }
        std::fs::write(&plist, original.replace("LSUIElement", "LSBackgroundOnly")).unwrap();
        assert!(bundle(&root).is_ok());
        std::fs::write(&plist, original).unwrap();
        let executable = helper.join("Contents/MacOS/后台 Electron 中文");
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(bundle(&root).err().unwrap().to_string().contains("执行权限"));
        std::fs::remove_file(executable).unwrap();
        assert!(bundle(&root).err().unwrap().to_string().contains("缺少可执行文件"));
        std::fs::remove_dir_all(helper).unwrap();
        assert!(bundle(&root).err().unwrap().to_string().contains("缺少后台 Helper"));
        assert!(explicit_executable(&root.join("Contents/MacOS/自带 Electron")).is_err());
    }
    #[test]
    fn zcode_macos_helper_preserves_explicit_provider_and_version_overrides() {
        const MARKER: &str = "CREWVELA_ZCODE_MACOS_OVERRIDE_TEST_ROOT";
        if let Some(root) = std::env::var_os(MARKER) {
            let app = bundle(Path::new(&root)).unwrap();
            let command = super::super::launch(&app.agent).unwrap();
            assert_eq!(command.get_program(), app.program.as_os_str());
            for key in ["ZCODE_BUILTIN_PROVIDER_CONFIG_FILE", "ZCODE_PERSONAL_PROVIDER_CONFIG_FILE", "CREWVELA_ZCODE_APP_VERSION"] {
                assert!(!command.get_envs().any(|(name, _)| name == key), "explicit inherited override must not be replaced: {key}");
            }
            assert!(command.get_envs().any(|(name, value)| name == "NODE_OPTIONS" && value.is_none()));
            return;
        }
        let temp = fixture();
        let root = temp.0.join("中文 空格/ZCode.app");
        make_bundle(&root);
        let name = concat!(module_path!(), "::zcode_macos_helper_preserves_explicit_provider_and_version_overrides");
        let status = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", name.split_once("::").unwrap().1])
            .env(MARKER, &root)
            .env("ZCODE_BUILTIN_PROVIDER_CONFIG_FILE", temp.0.join("自定义 builtin.json"))
            .env("ZCODE_PERSONAL_PROVIDER_CONFIG_FILE", temp.0.join("个人 provider.json"))
            .env("CREWVELA_ZCODE_APP_VERSION", "fixture.override")
            .env("NODE_OPTIONS", "fixture-options-must-be-removed")
            .status().unwrap();
        assert!(status.success());
    }
    #[test]
    #[ignore = "需要本机已安装 ZCode Desktop；仅发现与启动命令解析，不读取账号或调用模型"]
    fn zcode_macos_installed_desktop_without_path_cli() {
        const MARKER: &str = "CREWVELA_ZCODE_MACOS_DISCOVERY_TEST_CHILD";
        if std::env::var_os(MARKER).is_some() {
            let path = super::super::executable().unwrap();
            let app = bundle(bundle_root(&path).expect("Desktop must resolve to an app bundle")).unwrap();
            assert_eq!(path, app.agent);
            let command = super::super::launch(&path).unwrap();
            assert_eq!(command.get_program(), app.program.as_os_str());
            assert!(super::super::cli_executable().unwrap_err().to_string().contains("未找到 zcode CLI"));
            return;
        }
        let name = concat!(module_path!(), "::zcode_macos_installed_desktop_without_path_cli");
        let name = name.split_once("::").unwrap().1;
        let status = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", name, "--ignored", "--nocapture"])
            .env(MARKER, "1").env("PATH", "/usr/bin:/bin")
            .env_remove("CREWVELA_ZCODE_EXECUTABLE")
            .env_remove("CREWVELA_ZCODE_ROOT")
            .env_remove("CREWVELA_ZCODE_CLI_EXECUTABLE")
            .status().unwrap();
        assert!(status.success());
    }
}
