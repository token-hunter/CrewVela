use super::*;

fn temporary(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("crewvela-three-install-{label}-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&path).unwrap(); path
}
fn github(kind: AgentRuntimeKind, platform: &str) -> serde_json::Value {
    let (repo, name) = github_spec(kind, platform).unwrap();
    let tag = if kind == AgentRuntimeKind::Pi { "v1.5.2" } else { "@moonshot-ai/kimi-code@2.6.2" };
    serde_json::json!({"draft":false,"prerelease":false,"tag_name":tag,"assets":[{"name":name,"browser_download_url":format!("https://github.com/{repo}/releases/download/{tag}/{name}"),"digest":format!("sha256:{}","a".repeat(64))}]})
}
#[test]
fn additional_release_rejects_wrong_repo_unstable_version_platform_and_missing_hash() {
    for kind in [AgentRuntimeKind::KimiCode, AgentRuntimeKind::Pi] {
        for platform in ["win32-x64", "win32-arm64"] {
            let valid = github(kind, platform);
            assert!(parse_github_release(&valid, kind, platform).is_ok());
            for (key, value) in [("draft", serde_json::json!(true)), ("prerelease", serde_json::json!(true)), ("tag_name", serde_json::json!("v1.2.3-beta"))] {
                let mut bad = valid.clone(); bad[key] = value;
                assert!(parse_github_release(&bad, kind, platform).is_err());
            }
            for (key, value) in [("name", "other.zip"), ("digest", "sha1:bad"), ("browser_download_url", "https://github.com/attacker/project/releases/download/v1.5.2/pi-windows-x64.zip")] {
                let mut bad = valid.clone(); bad["assets"][0][key] = value.into();
                assert!(parse_github_release(&bad, kind, platform).is_err());
            }
            let mut bad = valid.clone(); bad["assets"][0].as_object_mut().unwrap().remove("digest");
            assert!(parse_github_release(&bad, kind, platform).is_err());
        }
    }
    assert!(github_spec(AgentRuntimeKind::ZCodeCli, "win32-x64").is_err());
    assert!(github_spec(AgentRuntimeKind::Pi, "linux-x64").is_err());
}
#[test]
fn additional_opencode_package_stays_on_existing_channel_and_checks_owner_url_and_integrity() {
    for platform in ["win32-x64", "win32-arm64"] {
        let package = opencode_package(platform).unwrap();
        let valid = serde_json::json!({"name":package,"version":"1.19.5","dist":{"tarball":format!("https://registry.npmjs.org/{package}/-/{package}-1.19.5.tgz"),"integrity":format!("sha512-{}",base64::engine::general_purpose::STANDARD.encode([1;64]))}});
        assert!(parse_opencode_release(&valid, "1.19.5", platform).is_ok());
        for key in ["name", "version"] {
            let mut bad = valid.clone(); bad[key] = "other".into();
            assert!(parse_opencode_release(&bad, "1.19.5", platform).is_err());
        }
        for key in ["tarball", "integrity"] {
            let mut bad = valid.clone(); bad["dist"][key] = "invalid".into();
            assert!(parse_opencode_release(&bad, "1.19.5", platform).is_err());
        }
        assert!(parse_opencode_release(&valid, "2.0.0", platform).is_err());
    }
}
#[test]
fn additional_help_and_login_are_specific_to_each_cli_and_do_not_issue_model_prompts() {
    for (kind, help) in [(AgentRuntimeKind::KimiCode, "Usage: kimi --output-format stream-json --prompt prompt acp"), (AgentRuntimeKind::OpenCode, "Usage: opencode run acp"), (AgentRuntimeKind::Pi, "Usage: pi --mode rpc")] {
        verify_help(kind, help).unwrap();
        assert!(verify_help(kind, "Usage: unrelated").is_err());
    }
    assert!(login_script(AgentRuntimeKind::KimiCode, "C:/kimi.exe").unwrap().ends_with("' login"));
    assert!(login_script(AgentRuntimeKind::OpenCode, "C:/opencode.exe").unwrap().ends_with("' auth login"));
    let pi = login_script(AgentRuntimeKind::Pi, "C:/pi.exe").unwrap();
    assert!(pi.contains("Write-Host")); assert!(pi.contains("/login"));
    assert!(pi.ends_with("& 'C:/pi.exe' ")); // /login is a visible hint, never a model prompt argument.
    assert!(login_script(AgentRuntimeKind::ZCodeCli, "C:/zcode.exe").is_err());
}
#[test]
fn additional_zip_rejects_path_escape_links_duplicates_and_cancel_without_writing() {
    let root = temporary("zip");
    let write_zip = |path: &Path, names: &[&str], link: bool| {
        let mut zip = zip::ZipWriter::new(fs::File::create(path).unwrap());
        for name in names {
            let options = zip::write::SimpleFileOptions::default();
            if link { zip.add_symlink(*name, "../../escape", options).unwrap(); }
            else { zip.start_file(*name, options).unwrap(); zip.write_all(b"fixture").unwrap(); }
        }
        zip.finish().unwrap();
    };
    for (index, name) in ["../escape.exe", "/escape.exe", "C:/escape.exe", "dir/../../escape.exe", "dir\\escape.exe"].iter().enumerate() {
        let archive = root.join(format!("bad-{index}.zip")); write_zip(&archive, &[*name], false);
        let dest = root.join(format!("dest-{index}")); fs::create_dir(&dest).unwrap();
        assert!(extract_zip(&archive, &dest, &AtomicBool::new(false)).is_err());
        assert_eq!(fs::read_dir(dest).unwrap().count(), 0);
    }
    let archive = root.join("link.zip"); write_zip(&archive, &["link"], true);
    let dest = root.join("links"); fs::create_dir(&dest).unwrap();
    assert!(extract_zip(&archive, &dest, &AtomicBool::new(false)).is_err());
    let archive = root.join("normal.zip"); write_zip(&archive, &["pi.exe", "theme/dark.json"], false);
    let dest = root.join("normal"); fs::create_dir(&dest).unwrap();
    assert!(extract_zip(&archive, &dest, &AtomicBool::new(true)).is_err());
    assert_eq!(fs::read_dir(&dest).unwrap().count(), 0);
    extract_zip(&archive, &dest, &AtomicBool::new(false)).unwrap();
    assert!(dest.join("pi.exe").is_file()); assert!(dest.join("theme/dark.json").is_file());
    // Repeated extraction cannot overwrite an existing file.
    assert!(extract_zip(&archive, &dest, &AtomicBool::new(false)).is_err());
    fs::remove_dir_all(root).unwrap();
}
#[cfg(windows)]
#[test]
fn additional_publication_preserves_old_entry_for_all_three_runtimes() {
    let base = temporary("publish");
    for kind in [AgentRuntimeKind::OpenCode, AgentRuntimeKind::KimiCode, AgentRuntimeKind::Pi] {
        let folder = base.join(kind.as_str()); fs::create_dir_all(&folder).unwrap();
        let prepared = folder.join("first"); let entry = prepared.join(relative_executable(kind).unwrap());
        fs::create_dir_all(entry.parent().unwrap()).unwrap(); fs::write(entry, "first").unwrap();
        let first = publish(&folder, &prepared, kind, "1.2.3").unwrap();
        assert_eq!(installed_at(&base, kind).unwrap(), Some(first.clone()));
        let original = fs::read(folder.join("current.json")).unwrap();
        let prepared = folder.join("second"); let entry = prepared.join(relative_executable(kind).unwrap());
        fs::create_dir_all(entry.parent().unwrap()).unwrap(); fs::write(entry, "second").unwrap();
        assert!(publish_with(&folder, &prepared, kind, "1.2.4", |_, _| bail!("fixture pointer failure")).is_err());
        assert_eq!(fs::read(folder.join("current.json")).unwrap(), original);
        assert_eq!(installed_at(&base, kind).unwrap(), Some(first));
    }
    fs::remove_dir_all(base).unwrap();
}
#[cfg(windows)]
#[test]
fn additional_publication_retries_only_temporary_windows_access_errors() {
    let mut calls = 0;
    rename_for_publication_with(|| { calls += 1; if calls == 1 { Err(std::io::Error::from_raw_os_error(32)) } else { Ok(()) } }).unwrap();
    assert_eq!(calls, 2);
    let mut calls = 0;
    assert!(rename_for_publication_with(|| { calls += 1; Err(std::io::Error::from(std::io::ErrorKind::NotFound)) }).is_err());
    assert_eq!(calls, 1);
}

#[cfg(windows)]
#[test]
#[ignore = "downloads official packages into isolated storage; version/help only, no login or model calls"]
fn additional_live_official_packages_work_in_isolated_storage() {
    let base = temporary("official"); let runtime = tokio::runtime::Runtime::new().unwrap();
    let mut records = Vec::new();
    for kind in [AgentRuntimeKind::OpenCode, AgentRuntimeKind::KimiCode, AgentRuntimeKind::Pi] {
        eprintln!("verifying official native package: {}", kind.as_str());
        let release = runtime.block_on(super::super::release(kind)).unwrap();
        let directory = base.join(kind.as_str()); fs::create_dir_all(&directory).unwrap();
        let package = directory.join("download");
        runtime.block_on(download(&release, &package, "live-fixture", &AtomicBool::new(false), &tokio::sync::Notify::new())).unwrap();
        let prepared = directory.join("prepared"); fs::create_dir(&prepared).unwrap();
        match release.archive {
            ArchiveFormat::TarGz => extract_archive(&package, &prepared, &AtomicBool::new(false)).unwrap(),
            ArchiveFormat::Zip => extract_zip(&package, &prepared, &AtomicBool::new(false)).unwrap(),
            _ => panic!("expected native archive"),
        }
        let executable = prepared.join(relative_executable(kind).unwrap());
        let home = directory.join("test-home"); fs::create_dir(&home).unwrap();
        for args in [["--version"], ["--help"]] {
            let mut command = crate::multi_agent::command_for_path(&executable);
            command.args(args).current_dir(&home).env("HOME", &home).env("USERPROFILE", &home)
                .env("XDG_CONFIG_HOME", &home).env("XDG_DATA_HOME", &home).env("XDG_CACHE_HOME", &home)
                .env("APPDATA", &home).env("LOCALAPPDATA", &home).env("PI_CODING_AGENT_DIR", &home)
                .env("KIMI_CODE_HOME", &home).env("KIMI_HOME", &home);
            let output = crate::title_summary::run_cli_with_timeout(command, None, Duration::from_secs(45), 256*1024, "isolated CLI verification").unwrap();
            assert!(output.status.success(), "{} {:?}: {}", kind.as_str(), args, String::from_utf8_lossy(&output.stderr));
            let text = cli_text(&output);
            if args[0] == "--version" { assert_eq!(crate::multi_agent::version_number(&text).as_deref(), Some(release.version.as_str())); }
            else { verify_help(kind, &text).unwrap(); }
        }
        let path = publish(&directory, &prepared, kind, &release.version).unwrap();
        assert_eq!(installed_at(&base, kind).unwrap(), Some(path));
        records.push(format!("{} {}", kind.as_str(), release.version));
        eprintln!("verified and published isolated package: {} {}", kind.as_str(), release.version);
    }
    eprintln!("official native packages verified: {}", records.join(", "));
    fs::remove_dir_all(base).unwrap();
}
#[test]
fn antigravity_manifest_requires_the_requested_windows_artifact_and_sha512() {
    let mut manifest = serde_json::json!({
        "version": "1.5.15",
        "url": "https://storage.googleapis.com/antigravity-public/antigravity-cli/1.5.15-123456789/windows-x64/cli_windows_x64.exe",
        "sha512": "00".repeat(64)
    });
    let release = parse_antigravity_release(&manifest, "win32-x64").unwrap();
    assert_eq!(release.version, "1.5.15");
    assert!(matches!(release.archive, ArchiveFormat::File));
    assert_eq!(release.integrity, format!("sha512-{}", base64::engine::general_purpose::STANDARD.encode([0u8; 64])));
    assert!(parse_antigravity_release(&manifest, "win32-arm64").is_err());
    manifest["url"] = serde_json::json!("https://storage.googleapis.com/antigravity-public/antigravity-cli/1.5.15-123456789/windows-arm/cli_windows_arm64.exe");
    assert!(parse_antigravity_release(&manifest, "win32-arm64").is_ok());
    for url in [
        "http://storage.googleapis.com/antigravity-public/antigravity-cli/1.5.15-123/windows-arm/cli_windows_arm64.exe",
        "https://storage.googleapis.com/unrelated/antigravity-cli/1.5.15-123/windows-arm/cli_windows_arm64.exe",
        "https://storage.googleapis.com/antigravity-public/antigravity-cli/1.5.150-123/windows-arm/cli_windows_arm64.exe",
        "https://storage.googleapis.com/antigravity-public/antigravity-cli/1.5.15-123/windows-arm/cli_windows_arm64.exe?redirect=1",
    ] {
        let mut invalid = manifest.clone();
        invalid["url"] = serde_json::json!(url);
        assert!(parse_antigravity_release(&invalid, "win32-arm64").is_err());
    }
    manifest["sha512"] = serde_json::json!("zz".repeat(64));
    assert!(parse_antigravity_release(&manifest, "win32-arm64").is_err());
}
