//! Intermediate structure produced by the importers (port of `importers/types.ts`).

#[derive(Debug, Clone, PartialEq)]
pub struct ImportedTab {
    pub url: String,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ImportedNode {
    Tab(ImportedTab),
    Folder { name: String, items: Vec<ImportedNode> },
    Split(Vec<ImportedTab>),
}

impl ImportedNode {
    pub fn tab(url: &str, title: &str) -> Self {
        ImportedNode::Tab(ImportedTab { url: url.into(), title: title.into() })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImportedSpace {
    pub name: String,
    pub icon: String,
    pub pinned: Vec<ImportedNode>,
    pub today: Vec<ImportedNode>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct SpacesImport {
    pub spaces: Vec<ImportedSpace>,
    pub favorites: Vec<ImportedTab>,
}

/// http(s) address only.
pub fn is_web_url(u: &str) -> bool {
    let l = u.trim_start().to_ascii_lowercase();
    l.starts_with("http://") || l.starts_with("https://")
}

/// Recursively removes empty folders.
pub fn prune_empty(nodes: Vec<ImportedNode>) -> Vec<ImportedNode> {
    nodes
        .into_iter()
        .filter_map(|n| match n {
            ImportedNode::Folder { name, items } => {
                let items = prune_empty(items);
                (!items.is_empty()).then_some(ImportedNode::Folder { name, items })
            }
            other => Some(other),
        })
        .collect()
}

pub fn count_tabs(nodes: &[ImportedNode]) -> usize {
    nodes
        .iter()
        .map(|n| match n {
            ImportedNode::Tab(_) => 1,
            ImportedNode::Split(t) => t.len(),
            ImportedNode::Folder { items, .. } => count_tabs(items),
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn tab(u: &str) -> ImportedNode { ImportedNode::tab(u, u) }

    #[test]
    fn web_url() {
        assert!(is_web_url("https://a.com"));
        assert!(is_web_url("  HTTP://a.com"));
        assert!(!is_web_url("javascript:1"));
        assert!(!is_web_url("file:///x"));
    }

    #[test]
    fn prune_and_count() {
        let tree = vec![
            tab("https://a.com"),
            ImportedNode::Folder { name: "vide".into(), items: vec![ImportedNode::Folder { name: "aussi vide".into(), items: vec![] }] },
            ImportedNode::Folder { name: "plein".into(), items: vec![tab("https://b.com"), ImportedNode::Folder { name: "x".into(), items: vec![] }] },
            ImportedNode::Split(vec![ImportedTab { url: "https://c.com".into(), title: "c".into() }, ImportedTab { url: "https://d.com".into(), title: "d".into() }]),
        ];
        let out = prune_empty(tree.clone());
        assert_eq!(out.len(), 3);
        match &out[1] {
            ImportedNode::Folder { items, .. } => assert_eq!(items.len(), 1),
            _ => panic!("folder expected"),
        }
        assert_eq!(count_tabs(&tree), 4);
        assert_eq!(count_tabs(&out), 4);
        assert_eq!(count_tabs(&[]), 0);
    }
}
