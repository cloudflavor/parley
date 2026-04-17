#[derive(Debug, Clone, Copy)]
pub(super) struct HelpDoc {
    pub title: &'static str,
    pub source_path: &'static str,
    pub body: &'static str,
}

pub(super) const HELP_DOCS: &[HelpDoc] = &[
    HelpDoc {
        title: "Keybindings",
        source_path: "docs/keybindings.md",
        body: include_str!("../../../docs/keybindings.md"),
    },
    HelpDoc {
        title: "Overview",
        source_path: "docs/overview.md",
        body: include_str!("../../../docs/overview.md"),
    },
    HelpDoc {
        title: "Quickstart",
        source_path: "docs/quickstart.md",
        body: include_str!("../../../docs/quickstart.md"),
    },
    HelpDoc {
        title: "Workflow",
        source_path: "docs/review-workflow.md",
        body: include_str!("../../../docs/review-workflow.md"),
    },
    HelpDoc {
        title: "MCP",
        source_path: "docs/mcp.md",
        body: include_str!("../../../docs/mcp.md"),
    },
];
