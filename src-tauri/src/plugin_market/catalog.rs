//! Bundled catalogue shared by native installation and the read-only UI preview.
use serde::Deserialize;
use std::sync::LazyLock;

#[derive(Deserialize)]
struct Catalog {
    version: u32,
    sources: Vec<Source>,
    plugins: Vec<Entry>,
}

#[derive(Deserialize)]
pub(super) struct Source {
    id: String,
    pub label: String,
    pub author: String,
    pub url: String,
    pub revision: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Entry {
    pub name: String,
    pub display_name: String,
    pub description: String,
    pub category: String,
    source_id: String,
    pub subpath: String,
    pub dependencies: Vec<String>,
    pub license: String,
}

static CATALOG: LazyLock<Catalog> = LazyLock::new(|| {
    let catalog: Catalog = serde_json::from_str(include_str!("../../../src/data/curated-plugins.json"))
        .expect("bundled catalogue must be valid JSON; validated by catalogue tests");
    assert_eq!(catalog.version, 1, "unsupported bundled catalogue version");
    catalog
});

pub(super) fn entries() -> &'static [Entry] { &CATALOG.plugins }
pub(super) fn find(name: &str) -> Option<&'static Entry> { entries().iter().find(|entry| entry.name == name) }
pub(super) fn source(entry: &Entry) -> &'static Source {
    CATALOG.sources.iter().find(|source| source.id == entry.source_id)
        .expect("bundled entries must reference a declared source")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalogue_has_unique_entries_with_pinned_sources_and_safe_subpaths() {
        assert_eq!(CATALOG.version, 1);
        assert_eq!(entries().len(), 16);
        let mut names = std::collections::HashSet::new();
        for entry in entries() {
            assert!(names.insert(&entry.name));
            assert!(!entry.display_name.is_empty() && !entry.description.is_empty() && !entry.dependencies.is_empty());
            assert!(entry.name.bytes().all(|byte| byte.is_ascii_lowercase() || byte == b'-'));
            assert_eq!(entry.subpath, format!("skills/{}", entry.name));
            let source = source(entry);
            assert!(source.url.starts_with("https://github.com/"));
            assert_eq!(source.revision.len(), 40);
            assert!(source.revision.bytes().all(|byte| byte.is_ascii_hexdigit()));
            assert!(!entry.license.is_empty());
        }
        for retained in ["frontend-design", "webapp-testing", "pdf", "docx", "pptx", "xlsx"] { assert!(find(retained).is_some()); }
        assert!(find("missing").is_none());
    }
}
