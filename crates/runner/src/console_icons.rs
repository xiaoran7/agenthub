//! The model and product marks the console draws beside a name.
//!
//! Every file here is copied verbatim from magpie's own icon set
//! (`internal/gui/assets/icons/`, MIT, Copyright (c) 2026 yetone) so the two
//! consoles show one vendor the same way. Nothing here is hand-drawn: the
//! console's old inline approximations drew vendor logos the vendors never
//! shipped, which is exactly what made them look wrong.
//!
//! `dashboard.js` picks one by the model's family through the same prefix
//! table magpie uses (`internal/agent/icons.go`).

/// One bundled mark: the name the page asks for and the bytes to answer with.
pub struct Icon {
    pub name: &'static str,
    pub mime: &'static str,
    pub bytes: &'static [u8],
}

macro_rules! svg {
    ($name:literal) => {
        Icon {
            name: $name,
            mime: "image/svg+xml",
            bytes: include_bytes!(concat!("../resources/icons/", $name, ".svg")),
        }
    };
}

/// Every mark the console can ask for, served under `/icons/<name>.svg`.
pub const ICONS: &[Icon] = &[
    svg!("claude-color"),
    svg!("claudecode-color"),
    svg!("codex-color"),
    svg!("openai"),
    svg!("gemini-color"),
    svg!("geminicli-color"),
    svg!("grok"),
    svg!("xai"),
    svg!("deepseek-color"),
    svg!("zhipu-color"),
    svg!("qwen-color"),
    svg!("kimi"),
    svg!("minimax-color"),
    svg!("mistral-color"),
    svg!("stepfun-color"),
    svg!("mimocode"),
    svg!("antigravity-color"),
    svg!("chatglm-color"),
    svg!("moonshot"),
    Icon {
        name: "zcode",
        mime: "image/png",
        bytes: include_bytes!("../resources/icons/zcode.png"),
    },
];

/// The icon a path names, if it is one of the bundled marks.
pub fn find(path: &str) -> Option<&'static Icon> {
    let file = path.strip_prefix("/icons/")?;
    let (stem, ext) = file.rsplit_once('.')?;
    ICONS
        .iter()
        .find(|icon| icon.name == stem && ext == extension(icon.mime))
}

fn extension(mime: &str) -> &'static str {
    if mime == "image/png" { "png" } else { "svg" }
}
