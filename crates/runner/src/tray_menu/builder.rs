use super::*;

/// Builds live tray card data for the default `Today` usage period.
pub fn build_live_menu(host: String) -> Menu {
    build_live_menu_for_period(host, TrayUsagePeriod::Today)
}

/// Builds live tray card data from local route discovery, cached usage ledger for `period`, and theme.
pub fn build_live_menu_for_period(host: String, period: TrayUsagePeriod) -> Menu {
    let dark = crate::tray::is_dark_theme();
    let is_warm =
        crate::usage_ledger::is_ledger_warm() && crate::claude_gateway::is_route_summaries_warm();
    let summaries = crate::claude_gateway::discover_all_client_route_summaries();
    let mut agents = Vec::with_capacity(summaries.len());
    for s in &summaries {
        let (brand_name, icon_id, full_bleed_icon, bg_light, bg_dark) = brand_visual(&s.agent_id);
        let action_id = if s.is_routed {
            ACTION_OPEN_SETTINGS
        } else {
            ACTION_OPEN_AGENTS
        };
        let route_mode = if s.is_routed {
            "routed".to_string()
        } else {
            "direct".to_string()
        };
        let detail = detail_inspect::inspect_agent_detail_local(&s.agent_id, s.is_routed);
        agents.push(TrayAgentRow {
            action_id,
            client_id: s.agent_id.clone(),
            display_name: brand_name
                .map(str::to_string)
                .unwrap_or_else(|| s.title.clone()),
            icon_id,
            full_bleed_icon,
            badge_bg_light: bg_light,
            badge_bg_dark: bg_dark,
            provider_label: if s.provider_name.trim().is_empty() {
                "Official".to_string()
            } else {
                s.provider_name.clone()
            },
            route_mode,
            detail,
        });
    }

    let is_loading = !is_warm;

    let usage_rows = build_usage_rows_for_period(period, is_loading);

    Menu {
        version: env!("CARGO_PKG_VERSION").to_string(),
        host,
        dark,
        is_loading,
        period,
        agents,
        usage_rows,
    }
}

/// Builds usage rows for `period` from the cached usage ledger without re-discovering agents.
pub fn build_usage_rows_for_period(period: TrayUsagePeriod, is_loading: bool) -> Vec<TrayUsageRow> {
    let (_, digest) = crate::usage_ledger::digest_cached(period.to_ledger_period(), 20);
    build_usage_rows_from_digest(period, &digest, is_loading)
}

pub(crate) fn build_usage_rows_from_digest(
    period: TrayUsagePeriod,
    digest: &crate::usage_ledger::LedgerDigest,
    is_loading: bool,
) -> Vec<TrayUsageRow> {
    let prefix = period.heading_prefix();
    let prompt_total = digest
        .input
        .saturating_add(digest.cache_read)
        .saturating_add(digest.cache_write);
    let hit_pct = if prompt_total > 0 {
        ((digest.cache_read as f64 / prompt_total as f64) * 100.0).round() as u32
    } else {
        0
    };

    let mut usage_rows = vec![
        TrayUsageRow {
            action_id: ACTION_OPEN_USAGE,
            label: format!("{prefix}请求与会话"),
            sublabel: if is_loading && digest.calls == 0 {
                "正在同步会话记录...".to_string()
            } else {
                format!(
                    "活跃会话 {} 个 · 失败 {} 次",
                    digest.sessions, digest.errors
                )
            },
            value: if is_loading && digest.calls == 0 {
                "加载中...".to_string()
            } else {
                format!("{} 次", digest.calls)
            },
            icon_id: TrayIconId::UsageReq,
            full_bleed_icon: false,
            badge_bg_light: (0xeb, 0xee, 0xff),
            badge_bg_dark: (0x25, 0x28, 0x46),
        },
        TrayUsageRow {
            action_id: ACTION_OPEN_USAGE,
            label: format!("{prefix} Token 消耗"),
            sublabel: format!(
                "输出 {} · 缓存命中 {}%",
                compact_tokens(digest.output),
                hit_pct
            ),
            value: compact_tokens(digest.tokens),
            icon_id: TrayIconId::UsageTok,
            full_bleed_icon: false,
            badge_bg_light: (0xf3, 0xe8, 0xff),
            badge_bg_dark: (0x2d, 0x21, 0x42),
        },
        TrayUsageRow {
            action_id: ACTION_OPEN_USAGE,
            label: format!("{prefix}预估成本"),
            sublabel: "按官方标准费率折算".to_string(),
            value: format!("${:.2}", digest.cost),
            icon_id: TrayIconId::UsageCost,
            full_bleed_icon: false,
            badge_bg_light: (0xe2, 0xf6, 0xec),
            badge_bg_dark: (0x1b, 0x36, 0x28),
        },
    ];

    for top in digest.by.model.iter().take(2) {
        let (icon_id, full_bleed_icon, badge_bg_light, badge_bg_dark) =
            model_brand_visual(&top.name);
        usage_rows.push(TrayUsageRow {
            action_id: ACTION_OPEN_USAGE,
            label: top.name.clone(),
            sublabel: format!("{} 次请求 · ${:.2}", top.calls, top.cost),
            value: compact_tokens(top.tokens),
            icon_id,
            full_bleed_icon,
            badge_bg_light,
            badge_bg_dark,
        });
    }

    usage_rows
}
