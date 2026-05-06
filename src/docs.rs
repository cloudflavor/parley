#[derive(Debug, Clone, Copy)]
pub struct ParleyDoc {
    pub title: &'static str,
    pub slug: &'static str,
    pub source_path: &'static str,
    pub uri: &'static str,
    pub body: &'static str,
}

pub const PARLEY_DOCS: &[ParleyDoc] = &[
    ParleyDoc {
        title: "Keybindings",
        slug: "keybindings",
        source_path: "docs/keybindings.md",
        uri: "parley://docs/keybindings",
        body: include_str!("../docs/keybindings.md"),
    },
    ParleyDoc {
        title: "Overview",
        slug: "overview",
        source_path: "docs/overview.md",
        uri: "parley://docs/overview",
        body: include_str!("../docs/overview.md"),
    },
    ParleyDoc {
        title: "Quickstart",
        slug: "quickstart",
        source_path: "docs/quickstart.md",
        uri: "parley://docs/quickstart",
        body: include_str!("../docs/quickstart.md"),
    },
    ParleyDoc {
        title: "Workflow",
        slug: "review-workflow",
        source_path: "docs/review-workflow.md",
        uri: "parley://docs/review-workflow",
        body: include_str!("../docs/review-workflow.md"),
    },
    ParleyDoc {
        title: "MCP",
        slug: "mcp",
        source_path: "docs/mcp.md",
        uri: "parley://docs/mcp",
        body: include_str!("../docs/mcp.md"),
    },
];

pub fn find_doc(value: &str) -> Option<&'static ParleyDoc> {
    let normalized = value.trim();
    PARLEY_DOCS.iter().find(|doc| {
        doc.uri == normalized
            || doc.slug == normalized
            || doc.source_path == normalized
            || doc.title.eq_ignore_ascii_case(normalized)
    })
}
