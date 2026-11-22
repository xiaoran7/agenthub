//! CC Switch-style layered system tray popup card (`UpdateLayeredWindow` + SDF + ClearType GDI).
//!
//! Renders a crisp DPI-scaled rounded card synchronized with the active console theme:
//! - Header: real AgentHub 3D cube logo + `"AgentHub"` title + `"运行中"` status pill + vector gear button
//! - Interactive segmented switcher: `[sliders] 配置` / `[chart] 用量`
//! - `配置` tab: per-agent elevated cards with real multi-color brand icons and click-through
//!   local config detail drill-down (`TrayView::Detail`)
//! - `用量` tab: segmented period switcher (`今日` / `7天` / `30天` / `全部`) with live requests,
//!   tokens, estimated cost, and top models with brand icons
//! - Footer: `[window] 打开主窗口` primary button and `[power] 退出` secondary button

use std::sync::OnceLock;

/// Action IDs returned to the tray host when a command target is clicked.
pub const ACTION_OPEN_MAIN: usize = 2001;
pub const ACTION_OPEN_SETTINGS: usize = 2002;
pub const ACTION_OPEN_AGENTS: usize = 2003;
pub const ACTION_OPEN_USAGE: usize = 2004;
pub const ACTION_EXIT: usize = 2005;

pub(crate) const ICON_SIDE: usize = 64;
pub(crate) const ICON_FRAME_BYTES: usize = ICON_SIDE * ICON_SIDE * 4;
const TRAY_ICONS_ZST: &[u8] = include_bytes!("../../resources/tray_icons_64.bin.zst");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
pub enum TrayIconId {
    AgentHub = 0,
    ClaudeCode = 1,
    ClaudeDesktop = 2,
    Codex = 3,
    GeminiCli = 4,
    GeminiStar = 5,
    ZCode = 6,
    Grok = 7,
    Antigravity = 8,
    Gear = 9,
    TabConfig = 10,
    TabUsage = 11,
    UsageReq = 12,
    UsageTok = 13,
    UsageCost = 14,
    BtnWindow = 15,
    BtnExit = 16,
    ChevronRight = 17,
    ArrowLeft = 18,
    Check = 19,
    DeepSeek = 20,
    Qwen = 21,
    Zhipu = 22,
    Kimi = 23,
    Minimax = 24,
    Mistral = 25,
    OpenAi = 26,
}

pub(crate) fn icon_bank() -> &'static [u8] {
    static BANK: OnceLock<Vec<u8>> = OnceLock::new();
    BANK.get_or_init(|| zstd::decode_all(TRAY_ICONS_ZST).unwrap_or_default())
}

pub(crate) fn icon_frame(id: TrayIconId) -> Option<&'static [u8]> {
    let bank = icon_bank();
    let start = (id as usize).checked_mul(ICON_FRAME_BYTES)?;
    let end = start.checked_add(ICON_FRAME_BYTES)?;
    bank.get(start..end)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TrayTab {
    #[default]
    Config,
    Usage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TrayUsagePeriod {
    #[default]
    Today,
    Days7,
    Days30,
    All,
}

impl TrayUsagePeriod {
    pub const ALL: [Self; 4] = [Self::Today, Self::Days7, Self::Days30, Self::All];

    pub fn label(self) -> &'static str {
        match self {
            Self::Today => "今日",
            Self::Days7 => "7天",
            Self::Days30 => "30天",
            Self::All => "全部",
        }
    }

    pub fn heading_prefix(self) -> &'static str {
        match self {
            Self::Today => "今日",
            Self::Days7 => "7日",
            Self::Days30 => "30日",
            Self::All => "累计",
        }
    }

    pub fn index(self) -> usize {
        match self {
            Self::Today => 0,
            Self::Days7 => 1,
            Self::Days30 => 2,
            Self::All => 3,
        }
    }

    pub fn prev(self) -> Self {
        let idx = self.index();
        Self::ALL[(idx + Self::ALL.len() - 1) % Self::ALL.len()]
    }

    pub fn next(self) -> Self {
        let idx = self.index();
        Self::ALL[(idx + 1) % Self::ALL.len()]
    }

    pub fn to_ledger_period(self) -> crate::usage_ledger::Period {
        match self {
            Self::Today => crate::usage_ledger::Period::Today,
            Self::Days7 => crate::usage_ledger::Period::Days(7),
            Self::Days30 => crate::usage_ledger::Period::Days(30),
            Self::All => crate::usage_ledger::Period::All,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TrayView {
    #[default]
    Home,
    Detail(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayAgentDetail {
    pub endpoint: String,
    pub source: String,
    pub models: Vec<String>,
    pub extra_model_count: usize,
    pub console_action_id: usize,
    pub console_label: String,
}

impl Default for TrayAgentDetail {
    fn default() -> Self {
        Self {
            endpoint: "官方默认端点".to_string(),
            source: "默认配置".to_string(),
            models: Vec::new(),
            extra_model_count: 0,
            console_action_id: ACTION_OPEN_AGENTS,
            console_label: "在主控台管理".to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct TrayAgentRow {
    pub action_id: usize,
    pub client_id: String,
    pub display_name: String,
    pub icon_id: TrayIconId,
    pub full_bleed_icon: bool,
    pub badge_bg_light: (u8, u8, u8),
    pub badge_bg_dark: (u8, u8, u8),
    pub provider_label: String,
    /// `"direct"` -> green `直连`, `"routed"` -> indigo `路由`, `"loading"` -> amber `加载中`
    pub route_mode: String,
    pub detail: TrayAgentDetail,
}

#[derive(Debug, Clone)]
pub struct TrayUsageRow {
    pub action_id: usize,
    pub label: String,
    pub sublabel: String,
    pub value: String,
    pub icon_id: TrayIconId,
    pub full_bleed_icon: bool,
    pub badge_bg_light: (u8, u8, u8),
    pub badge_bg_dark: (u8, u8, u8),
}

#[derive(Debug, Clone)]
pub struct Menu {
    pub version: String,
    pub host: String,
    pub dark: bool,
    pub is_loading: bool,
    pub period: TrayUsagePeriod,
    pub agents: Vec<TrayAgentRow>,
    pub usage_rows: Vec<TrayUsageRow>,
}

impl Default for Menu {
    fn default() -> Self {
        Self {
            version: env!("CARGO_PKG_VERSION").to_string(),
            host: "127.0.0.1:4200".to_string(),
            dark: false,
            is_loading: false,
            period: TrayUsagePeriod::Today,
            agents: Vec::new(),
            usage_rows: Vec::new(),
        }
    }
}

pub(crate) fn brand_visual(
    client_id: &str,
) -> (
    Option<&'static str>,
    TrayIconId,
    bool,
    (u8, u8, u8),
    (u8, u8, u8),
) {
    match client_id {
        "claude_code" => (
            Some("Claude"),
            TrayIconId::ClaudeCode,
            false,
            (0xfd, 0xf0, 0xe7),
            (0x33, 0x22, 0x1b),
        ),
        "claude_desktop" => (
            Some("Claude Desktop"),
            TrayIconId::ClaudeDesktop,
            false,
            (0xfc, 0xee, 0xe4),
            (0x31, 0x20, 0x19),
        ),
        "codex" => (
            Some("Codex"),
            TrayIconId::Codex,
            true,
            (0xee, 0xf2, 0xff),
            (0x23, 0x25, 0x3a),
        ),
        "gemini_cli" => (
            Some("Gemini"),
            TrayIconId::GeminiCli,
            true,
            (0xef, 0xf6, 0xff),
            (0x1f, 0x29, 0x3d),
        ),
        "zcode" => (
            Some("ZCode"),
            TrayIconId::ZCode,
            true,
            (0xf3, 0xf4, 0xf6),
            (0x27, 0x27, 0x2f),
        ),
        "grok_build" => (
            Some("Grok"),
            TrayIconId::Grok,
            false,
            (0xf3, 0xe8, 0xff),
            (0x2b, 0x1f, 0x3d),
        ),
        "antigravity" => (
            Some("Antigravity"),
            TrayIconId::Antigravity,
            false,
            (0xef, 0xf6, 0xff),
            (0x1e, 0x29, 0x3b),
        ),
        _ => (
            None,
            TrayIconId::AgentHub,
            false,
            (0xee, 0xf2, 0xff),
            (0x25, 0x28, 0x3d),
        ),
    }
}

pub(crate) fn model_brand_visual(model: &str) -> (TrayIconId, bool, (u8, u8, u8), (u8, u8, u8)) {
    let lower = model.to_ascii_lowercase();
    if lower.contains("claude")
        || lower.contains("sonnet")
        || lower.contains("opus")
        || lower.contains("haiku")
        || lower.contains("fable")
        || lower.contains("mythos")
    {
        (
            TrayIconId::ClaudeDesktop,
            false,
            (0xfd, 0xf0, 0xe7),
            (0x33, 0x22, 0x1b),
        )
    } else if lower.contains("gemini") || lower.contains("gemma") || lower.contains("argon") {
        (
            TrayIconId::GeminiStar,
            false,
            (0xef, 0xf6, 0xff),
            (0x1e, 0x29, 0x3b),
        )
    } else if lower.contains("deepseek") {
        (
            TrayIconId::DeepSeek,
            false,
            (0xed, 0xf2, 0xff),
            (0x1e, 0x26, 0x3d),
        )
    } else if lower.contains("qwen") || lower.contains("qwq") {
        (
            TrayIconId::Qwen,
            false,
            (0xf0, 0xed, 0xff),
            (0x26, 0x20, 0x3d),
        )
    } else if lower.contains("glm") || lower.contains("zhipu") {
        (
            TrayIconId::Zhipu,
            false,
            (0xec, 0xf3, 0xff),
            (0x1d, 0x28, 0x3d),
        )
    } else if lower.contains("kimi") || lower.contains("moonshot") {
        (
            TrayIconId::Kimi,
            false,
            (0xf1, 0xf3, 0xf7),
            (0x27, 0x28, 0x30),
        )
    } else if lower.contains("minimax") || lower.contains("abab") {
        (
            TrayIconId::Minimax,
            false,
            (0xff, 0xee, 0xf0),
            (0x38, 0x1f, 0x24),
        )
    } else if lower.contains("mistral")
        || lower.contains("codestral")
        || lower.contains("devstral")
        || lower.contains("magistral")
    {
        (
            TrayIconId::Mistral,
            false,
            (0xff, 0xf2, 0xe8),
            (0x36, 0x24, 0x19),
        )
    } else if lower.contains("codex") {
        (
            TrayIconId::Codex,
            true,
            (0xee, 0xf2, 0xff),
            (0x23, 0x25, 0x3a),
        )
    } else if lower.contains("gpt")
        || lower.starts_with("o1")
        || lower.starts_with("o3")
        || lower.starts_with("o4")
        || lower.contains("openai")
    {
        (
            TrayIconId::OpenAi,
            false,
            (0xe6, 0xf7, 0xf2),
            (0x19, 0x33, 0x2b),
        )
    } else if lower.contains("grok") {
        (
            TrayIconId::Grok,
            false,
            (0xf3, 0xe8, 0xff),
            (0x2b, 0x1f, 0x3d),
        )
    } else if lower.contains("antigravity") {
        (
            TrayIconId::Antigravity,
            false,
            (0xef, 0xf6, 0xff),
            (0x1e, 0x29, 0x3b),
        )
    } else if lower.contains("zcode") {
        (
            TrayIconId::ZCode,
            true,
            (0xf3, 0xf4, 0xf6),
            (0x27, 0x27, 0x2f),
        )
    } else {
        (
            TrayIconId::UsageTok,
            false,
            (0xf3, 0xe8, 0xff),
            (0x2d, 0x21, 0x42),
        )
    }
}

pub(crate) fn compact_tokens(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}
