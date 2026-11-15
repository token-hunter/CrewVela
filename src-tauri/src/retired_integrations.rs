//! One-way compatibility cleanup for app-owned registrations; never edits native CLI configuration.
use crate::skill_center::SkillCenter;
use std::path::Path;

pub(crate) fn retire_owned_skills(config_dir: &Path, center: &SkillCenter) -> anyhow::Result<Vec<String>> {
    let mut warnings = Vec::new();
    let source = config_dir.join("computer-control-skill").display().to_string();
    for asset in center.asset_ids_by_source_ref(&source)? {
        // The central library deduplicates identical Skill imports. Keep its
        // historical asset: deleting it could also remove a user's same-name
        // import. Only withdraw the managed deployments; user source files stay.
        let result = center.deployment_ids_for_skill(&asset)
            .and_then(|ids| center.undeploy_batch(ids));
        if let Err(error) = result { warnings.push(format!("保留已修改的旧桌面工具 Skill，未删除用户内容：{error:#}")); }
    }
    Ok(warnings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skill_center::{DeploySkillInput, ImportSkillInput};
    #[test]
    fn retires_owned_deployments_but_preserves_same_name_user_skill() {
        let root = std::env::temp_dir().join(format!("crewvela-retired-tools-{}", uuid::Uuid::new_v4()));
        let config = root.join("config");
        let center = SkillCenter::open_at(root.join("center")).unwrap();
        let project = root.join("project");
        std::fs::create_dir_all(&project).unwrap();
        let mut ids = Vec::new();
        for source in [config.join("computer-control-skill"), root.join("user-skill")] {
            std::fs::create_dir_all(&source).unwrap();
            std::fs::write(source.join("SKILL.md"), "---\nname: computer-control\n---\nlocal").unwrap();
            let asset = center.import(ImportSkillInput { source_type:"local".into(),source:source.display().to_string(),branch:None,subpath:None,revision:None }).unwrap().remove(0);
            ids.push(asset.id);
        }
        let deployed = center.deploy_batch(vec![DeploySkillInput {skill_id:ids[0].clone(),runtime:"codex".into(),scope:"project".into(),project_path:Some(project.display().to_string()),task_id:None,agent_id:None,trust_confirmed:true}]).unwrap();
        assert!(retire_owned_skills(&config, &center).unwrap().is_empty());
        assert!(!Path::new(&deployed[0].target_path).exists());
        let snapshot = center.snapshot().unwrap();
        assert!(snapshot.assets.iter().any(|asset| asset.id == ids[1]));
        assert!(root.join("user-skill/SKILL.md").exists());
        assert!(retire_owned_skills(&config, &center).unwrap().is_empty());
        drop(center);
        std::fs::remove_dir_all(root).unwrap();
    }
}
