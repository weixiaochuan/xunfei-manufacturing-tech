// 此文件由 scripts/plugin-capabilities.mjs 生成，请勿手工修改。
pub(crate) const VALID_PERMISSIONS: &[&str] = &[
    "notes.read",
    "notes.write",
    "document.read",
    "document.write",
    "tasks.read",
    "tasks.write",
    "ai.invoke",
    "ai.context.read",
    "ai.context.augment",
    "ai.session.read",
    "ui.editor.toolbar",
    "ui.chat.toolbar",
    "ui.chat.panel",
    "planning.files.read",
    "planning.files.write",
    "network.request",
    "files.readSelected",
    "files.writeSelected",
    "prompts.register",
    "views.register",
    "mcp.connect",
    "credentials.use",
    "credentials.configure",
    "network.xingchen",
    "agents.invoke",
    "editor:read",
    "editor:write",
    "workspace:read",
    "workspace:write",
    "notes:read",
    "notes:write",
    "settings:read",
    "settings:write",
    "files:read",
    "files:write",
    "network:request",
    "clipboard:read",
    "clipboard:write",
    "tasks.subscribe",
    "taskViews.register",
    "ai:chat",
    "ai:models",
];

pub(crate) const V3_MANIFEST_PERMISSIONS: &[&str] = &[
    "document.read",
    "document.write",
    "tasks.read",
    "tasks.write",
    "ai.invoke",
    "ai.context.read",
    "ai.context.augment",
    "ai.session.read",
    "ui.editor.toolbar",
    "ui.chat.toolbar",
    "ui.chat.panel",
    "planning.files.read",
    "planning.files.write",
    "network.request",
    "files.writeSelected",
    "prompts.register",
    "mcp.connect",
    "credentials.use",
    "network.xingchen",
    "agents.invoke",
];

pub(crate) const V3_PERMISSION_RUNTIME_KINDS: &[(&str, &[&str])] = &[
    ("document.read", &["declarative-ui", "xingchen-agent", "xingchen-workflow"]),
    ("document.write", &["declarative-ui", "xingchen-agent", "xingchen-workflow"]),
    ("tasks.read", &["legacy-js"]),
    ("tasks.write", &["legacy-js"]),
    ("ai.invoke", &["declarative-ui", "xingchen-agent", "xingchen-workflow"]),
    ("ai.context.read", &["prompt-pack", "declarative-ui"]),
    ("ai.context.augment", &["prompt-pack", "declarative-ui", "xingchen-agent", "xingchen-workflow"]),
    ("ai.session.read", &["prompt-pack", "declarative-ui"]),
    ("ui.editor.toolbar", &["declarative-ui"]),
    ("ui.chat.toolbar", &["prompt-pack", "declarative-ui"]),
    ("ui.chat.panel", &["prompt-pack", "declarative-ui"]),
    ("planning.files.read", &["prompt-pack", "declarative-ui"]),
    ("planning.files.write", &["prompt-pack", "declarative-ui"]),
    ("network.request", &["xingchen-agent", "xingchen-workflow", "mcp-connector"]),
    ("files.writeSelected", &["xingchen-agent", "xingchen-workflow"]),
    ("prompts.register", &["prompt-pack"]),
    ("mcp.connect", &["mcp-connector"]),
    ("credentials.use", &["xingchen-agent", "xingchen-workflow", "mcp-connector"]),
    ("network.xingchen", &["xingchen-agent", "xingchen-workflow"]),
    ("agents.invoke", &["xingchen-agent", "xingchen-workflow"]),
];

pub(crate) const V3_PERMISSION_RUNTIME_COMPATIBILITY_EXCEPTIONS: &[&str] = &["tasks.read", "tasks.write", "mcp.connect"];

pub(crate) const V3_CLASSIFICATION_CONTRIBUTION_RULES: &[(&str, &[&str], &[&str])] = &[
    ("feature", &["feature"], &["enhancement"]),
    ("enhancement", &["enhancement"], &["feature"]),
    ("hybrid", &["feature", "enhancement"], &[]),
];

pub(crate) const V3_RUNTIME_CLASSIFICATION_RULES: &[(&str, &[&str])] = &[
    ("declarative-ui", &["feature"]),
    ("prompt-pack", &["enhancement"]),
    ("xingchen-agent", &["feature", "hybrid"]),
    ("xingchen-workflow", &["feature", "hybrid"]),
];

pub(crate) const V3_CONTRIBUTION_REQUIRED_PERMISSIONS: &[(&str, &[&str])] = &[
    ("enhancement", &["ai.context.augment"]),
];

pub(crate) const V3_RUNTIME_CONTRIBUTION_REQUIRED_PERMISSIONS: &[(&[&str], &str, &[&str])] = &[
    (&["xingchen-agent", "xingchen-workflow"], "feature", &["credentials.use", "agents.invoke", "network.xingchen", "ai.invoke"]),
];

pub(crate) const V3_FEATURE_CAPABILITY_REQUIRED_PERMISSIONS: &[(&str, &[&str])] = &[
    ("file.docx.output", &["files.writeSelected"]),
];
