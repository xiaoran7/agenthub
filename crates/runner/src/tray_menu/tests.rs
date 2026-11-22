use super::*;
use crate::claude_gateway::{CcSwitchActiveProvider, ClaudeGatewayModel};
use crate::usage_ledger::{Breakdown, LedgerDigest, RankRow};

fn sample_menu() -> Menu {
    let detail_routed = TrayAgentDetail {
        endpoint: "http://127.0.0.1:8317".to_string(),
        source: "CC Switch (cc-switch.db)".to_string(),
        models: vec![
            "claude-opus-5".to_string(),
            "claude-sonnet-5".to_string(),
            "claude-haiku-5".to_string(),
        ],
        extra_model_count: 1,
        console_action_id: ACTION_OPEN_SETTINGS,
        console_label: "在主控台配置网关".to_string(),
    };
    let detail_direct = TrayAgentDetail {
        endpoint: "https://api.openai.com".to_string(),
        source: "OpenAI 官方默认".to_string(),
        models: vec!["gpt-5-codex".to_string()],
        extra_model_count: 0,
        console_action_id: ACTION_OPEN_AGENTS,
        console_label: "在主控台查看智能体".to_string(),
    };

    Menu {
        version: "0.1.0".to_string(),
        host: "127.0.0.1:4200".to_string(),
        dark: false,
        is_loading: false,
        period: TrayUsagePeriod::Today,
        agents: vec![
            TrayAgentRow {
                action_id: ACTION_OPEN_SETTINGS,
                client_id: "claude_code".to_string(),
                display_name: "Claude".to_string(),
                icon_id: TrayIconId::ClaudeCode,
                full_bleed_icon: false,
                badge_bg_light: (0xff, 0xf1, 0xe8),
                badge_bg_dark: (0x38, 0x26, 0x1c),
                provider_label: "Custom Router".to_string(),
                route_mode: "routed".to_string(),
                detail: detail_routed,
            },
            TrayAgentRow {
                action_id: ACTION_OPEN_AGENTS,
                client_id: "codex".to_string(),
                display_name: "Codex".to_string(),
                icon_id: TrayIconId::Codex,
                full_bleed_icon: false,
                badge_bg_light: (0xf0, 0xf2, 0xf7),
                badge_bg_dark: (0x26, 0x28, 0x33),
                provider_label: "OpenAI".to_string(),
                route_mode: "direct".to_string(),
                detail: detail_direct,
            },
            TrayAgentRow {
                action_id: ACTION_OPEN_AGENTS,
                client_id: "gemini_cli".to_string(),
                display_name: "Gemini".to_string(),
                icon_id: TrayIconId::GeminiCli,
                full_bleed_icon: false,
                badge_bg_light: (0xe8, 0xf1, 0xff),
                badge_bg_dark: (0x1e, 0x2b, 0x45),
                provider_label: "Google".to_string(),
                route_mode: "direct".to_string(),
                detail: TrayAgentDetail::default(),
            },
            TrayAgentRow {
                action_id: ACTION_OPEN_AGENTS,
                client_id: "zcode".to_string(),
                display_name: "ZCode".to_string(),
                icon_id: TrayIconId::ZCode,
                full_bleed_icon: false,
                badge_bg_light: (0xe9, 0xef, 0xff),
                badge_bg_dark: (0x20, 0x28, 0x42),
                provider_label: "ZCode".to_string(),
                route_mode: "direct".to_string(),
                detail: TrayAgentDetail::default(),
            },
        ],
        usage_rows: build_usage_rows_from_digest(
            TrayUsagePeriod::Today,
            &sample_digest("today", 18, 42_000, 1.25),
            false,
        ),
    }
}

fn sample_digest(period: &'static str, calls: u64, tokens: u64, cost: f64) -> LedgerDigest {
    LedgerDigest {
        period,
        calls,
        sessions: 3,
        input: tokens / 2,
        output: tokens / 4,
        cache_read: tokens / 4,
        cache_write: 0,
        tokens,
        cost,
        errors: 1,
        by: Breakdown {
            model: vec![RankRow {
                id: "claude-opus-5".to_string(),
                name: "claude-opus-5".to_string(),
                calls,
                tokens,
                cost,
                ..RankRow::default()
            }],
            ..Breakdown::default()
        },
        ..LedgerDigest::default()
    }
}

#[test]
fn embedded_icon_bank_contains_all_twenty_seven_64x64_rgba_icons() {
    let bank = icon_bank();
    assert_eq!(bank.len(), 27 * ICON_FRAME_BYTES);
    for id in [
        TrayIconId::AgentHub,
        TrayIconId::ClaudeCode,
        TrayIconId::ClaudeDesktop,
        TrayIconId::Codex,
        TrayIconId::GeminiCli,
        TrayIconId::GeminiStar,
        TrayIconId::ZCode,
        TrayIconId::Grok,
        TrayIconId::Antigravity,
        TrayIconId::Gear,
        TrayIconId::TabConfig,
        TrayIconId::TabUsage,
        TrayIconId::UsageReq,
        TrayIconId::UsageTok,
        TrayIconId::UsageCost,
        TrayIconId::BtnWindow,
        TrayIconId::BtnExit,
        TrayIconId::ChevronRight,
        TrayIconId::ArrowLeft,
        TrayIconId::Check,
        TrayIconId::DeepSeek,
        TrayIconId::Qwen,
        TrayIconId::Zhipu,
        TrayIconId::Kimi,
        TrayIconId::Minimax,
        TrayIconId::Mistral,
        TrayIconId::OpenAi,
    ] {
        let frame = icon_frame(id).expect("icon frame must exist");
        let non_transparent = frame
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|px| px[3] > 16)
            .count();
        assert!(
            non_transparent > 60,
            "icon {:?} must have visible opaque pixels (got {})",
            id,
            non_transparent
        );
    }
}

#[test]
fn card_layout_hits_tabs_gear_rows_and_footer() {
    let menu = sample_menu();
    let layout = CardLayout::compute(&menu, TrayView::Home, TrayTab::Config, menu.period, 144);
    let gear_mid = (
        (layout.gear_rect.left + layout.gear_rect.right) / 2,
        (layout.gear_rect.top + layout.gear_rect.bottom) / 2,
    );
    assert_eq!(
        layout.hit(gear_mid.0, gear_mid.1),
        Some(HitTarget::GearSettings)
    );
    assert_eq!(
        layout.outcome_for(HitTarget::GearSettings, &menu),
        TargetOutcome::InvokeAction(ACTION_OPEN_SETTINGS)
    );

    let usage_tab_mid = (
        (layout.tab_usage.left + layout.tab_usage.right) / 2,
        (layout.tab_usage.top + layout.tab_usage.bottom) / 2,
    );
    assert_eq!(
        layout.hit(usage_tab_mid.0, usage_tab_mid.1),
        Some(HitTarget::Tab(TrayTab::Usage))
    );
    assert_eq!(
        layout.outcome_for(HitTarget::Tab(TrayTab::Usage), &menu),
        TargetOutcome::SwitchTab(TrayTab::Usage)
    );

    let row0 = layout.row_rects[0].0;
    let row0_mid = ((row0.left + row0.right) / 2, (row0.top + row0.bottom) / 2);
    assert_eq!(layout.hit(row0_mid.0, row0_mid.1), Some(HitTarget::Row(0)));
    assert_eq!(
        layout.outcome_for(HitTarget::Row(0), &menu),
        TargetOutcome::OpenDetail(0)
    );

    let main_btn_mid = (
        (layout.footer_main.left + layout.footer_main.right) / 2,
        (layout.footer_main.top + layout.footer_main.bottom) / 2,
    );
    assert_eq!(
        layout.hit(main_btn_mid.0, main_btn_mid.1),
        Some(HitTarget::FooterMain)
    );
    assert_eq!(
        layout.outcome_for(HitTarget::FooterMain, &menu),
        TargetOutcome::InvokeAction(ACTION_OPEN_MAIN)
    );

    let exit_btn_mid = (
        (layout.footer_exit.left + layout.footer_exit.right) / 2,
        (layout.footer_exit.top + layout.footer_exit.bottom) / 2,
    );
    assert_eq!(
        layout.hit(exit_btn_mid.0, exit_btn_mid.1),
        Some(HitTarget::FooterExit)
    );
    assert_eq!(
        layout.outcome_for(HitTarget::FooterExit, &menu),
        TargetOutcome::InvokeAction(ACTION_EXIT)
    );
}

#[test]
fn card_layout_usage_period_bar_and_detail_view_hit_targets() {
    let menu = sample_menu();
    let usage_layout = CardLayout::compute(
        &menu,
        TrayView::Home,
        TrayTab::Usage,
        TrayUsagePeriod::Days7,
        96,
    );
    assert!(usage_layout.period_track.is_some());
    for (expected, rect) in usage_layout.period_rects {
        let cx = (rect.left + rect.right) / 2;
        let cy = (rect.top + rect.bottom) / 2;
        assert_eq!(usage_layout.hit(cx, cy), Some(HitTarget::Period(expected)));
        assert_eq!(
            usage_layout.outcome_for(HitTarget::Period(expected), &menu),
            TargetOutcome::SwitchPeriod(expected)
        );
    }

    let detail_layout = CardLayout::compute(
        &menu,
        TrayView::Detail(0),
        TrayTab::Config,
        menu.period,
        144,
    );
    let back_rect = detail_layout.detail_back_rect.expect("back button exists");
    assert_eq!(
        detail_layout.hit(
            (back_rect.left + back_rect.right) / 2,
            (back_rect.top + back_rect.bottom) / 2
        ),
        Some(HitTarget::DetailBack)
    );
    assert_eq!(
        detail_layout.outcome_for(HitTarget::DetailBack, &menu),
        TargetOutcome::BackHome
    );

    assert_eq!(detail_layout.detail_model_chips.len(), 3);

    let cta_rect = detail_layout
        .detail_console_rect
        .expect("console CTA exists");
    assert_eq!(
        detail_layout.hit(
            (cta_rect.left + cta_rect.right) / 2,
            (cta_rect.top + cta_rect.bottom) / 2
        ),
        Some(HitTarget::DetailOpenConsole(0))
    );
    assert_eq!(
        detail_layout.outcome_for(HitTarget::DetailOpenConsole(0), &menu),
        TargetOutcome::InvokeAction(ACTION_OPEN_SETTINGS)
    );
    assert_eq!(
        detail_layout.outcome_for(HitTarget::DetailOpenConsole(1), &menu),
        TargetOutcome::InvokeAction(ACTION_OPEN_AGENTS)
    );
}

#[test]
fn keyboard_navigation_covers_tabs_periods_detail_and_escape() {
    let menu = sample_menu();
    let home_cfg = CardLayout::compute(&menu, TrayView::Home, TrayTab::Config, menu.period, 96);
    assert_eq!(
        handle_key(
            KeyInput::Escape,
            TrayView::Home,
            TrayTab::Config,
            menu.period,
            None,
            &home_cfg,
            &menu,
        ),
        KeyCommand::Hide
    );
    assert_eq!(
        handle_key(
            KeyInput::Tab,
            TrayView::Home,
            TrayTab::Config,
            menu.period,
            None,
            &home_cfg,
            &menu,
        ),
        KeyCommand::SetFocus(HitTarget::Tab(TrayTab::Config))
    );
    assert_eq!(
        handle_key(
            KeyInput::Activate,
            TrayView::Home,
            TrayTab::Config,
            menu.period,
            Some(HitTarget::Row(0)),
            &home_cfg,
            &menu,
        ),
        KeyCommand::Apply(
            TargetOutcome::OpenDetail(0),
            Some(HitTarget::DetailOpenConsole(0))
        )
    );

    let detail = CardLayout::compute(&menu, TrayView::Detail(0), TrayTab::Config, menu.period, 96);
    assert_eq!(
        handle_key(
            KeyInput::Escape,
            TrayView::Detail(0),
            TrayTab::Config,
            menu.period,
            Some(HitTarget::DetailOpenConsole(0)),
            &detail,
            &menu,
        ),
        KeyCommand::Apply(TargetOutcome::BackHome, None)
    );
    assert_eq!(
        handle_key(
            KeyInput::Backspace,
            TrayView::Detail(0),
            TrayTab::Config,
            menu.period,
            Some(HitTarget::DetailOpenConsole(0)),
            &detail,
            &menu,
        ),
        KeyCommand::Apply(TargetOutcome::BackHome, None)
    );

    let home_usage = CardLayout::compute(
        &menu,
        TrayView::Home,
        TrayTab::Usage,
        TrayUsagePeriod::Today,
        96,
    );
    assert_eq!(
        handle_key(
            KeyInput::Right,
            TrayView::Home,
            TrayTab::Usage,
            TrayUsagePeriod::Today,
            Some(HitTarget::Period(TrayUsagePeriod::Today)),
            &home_usage,
            &menu,
        ),
        KeyCommand::Apply(
            TargetOutcome::SwitchPeriod(TrayUsagePeriod::Days7),
            Some(HitTarget::Period(TrayUsagePeriod::Days7))
        )
    );
}

#[test]
fn sanitize_endpoint_and_local_detail_inspection_never_expose_secrets() {
    assert_eq!(
        detail_inspect::sanitize_endpoint_display(
            "https://sk-secret:pass123@relay.example.com/v1/?key=abc#frag"
        ),
        "https://relay.example.com/v1"
    );
    assert_eq!(
        detail_inspect::sanitize_endpoint_display(""),
        "官方默认端点"
    );

    let cc = CcSwitchActiveProvider {
        id: "p1".to_string(),
        name: "MyRelay".to_string(),
        app_type: "claude".to_string(),
        upstream_base_url: Some(
            "https://user:secret-key@gw.example.com:8443/api?token=xyz".to_string(),
        ),
        upstream_auth_token: Some("sk-ant-super-secret".to_string()),
        models: vec![
            ClaudeGatewayModel {
                name: "claude-opus-5".to_string(),
                label: None,
                supports_1m: true,
            },
            ClaudeGatewayModel {
                name: "claude-sonnet-5".to_string(),
                label: None,
                supports_1m: false,
            },
            ClaudeGatewayModel {
                name: "claude-haiku-5".to_string(),
                label: None,
                supports_1m: false,
            },
            ClaudeGatewayModel {
                name: "claude-fable-5-2".to_string(),
                label: None,
                supports_1m: false,
            },
        ],
        upstream_models: Vec::new(),
        is_routed: true,
        proxy_takeover: true,
    };

    let detail = detail_inspect::inspect_agent_detail_from_sources(
        "claude_code",
        true,
        None,
        &[],
        Some(&cc),
        None,
    );
    assert_eq!(detail.endpoint, "https://gw.example.com:8443/api");
    assert!(!detail.endpoint.contains("secret"));
    assert!(!detail.endpoint.contains("token"));
    assert_eq!(detail.models.len(), 3);
    assert_eq!(detail.extra_model_count, 1);
    assert_eq!(detail.console_action_id, ACTION_OPEN_SETTINGS);
}

#[test]
fn usage_rows_builder_reflects_selected_period_and_model_breakdown() {
    let rows_today = build_usage_rows_from_digest(
        TrayUsagePeriod::Today,
        &sample_digest("today", 12, 25_000, 0.75),
        false,
    );
    assert_eq!(rows_today[0].label, "今日请求与会话");
    assert_eq!(rows_today[0].value, "12 次");
    assert_eq!(rows_today[2].value, "$0.75");
    assert_eq!(rows_today[3].label, "claude-opus-5");

    let rows_30d = build_usage_rows_from_digest(
        TrayUsagePeriod::Days30,
        &sample_digest("30d", 340, 1_500_000, 42.5),
        false,
    );
    assert_eq!(rows_30d[0].label, "30日请求与会话");
    assert_eq!(rows_30d[1].label, "30日 Token 消耗");
    assert_eq!(rows_30d[2].label, "30日预估成本");
    assert_eq!(rows_30d[2].value, "$42.50");
}

#[test]
fn antigravity_detail_inspection_reads_daemon_and_redacts_cc_switch() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let home = tmp.path();
    std::fs::create_dir_all(home.join(".gemini").join("config")).unwrap();
    std::fs::create_dir_all(home.join(".gemini").join("antigravity").join("daemon")).unwrap();
    std::fs::write(
        home.join(".gemini").join("config").join("config.json"),
        "\u{feff}{\"model\":\"gemini-3-pro\",\"plugins\":{\"p1\":{\"enabled\":true}}}",
    )
    .unwrap();
    std::fs::write(
        home.join(".gemini")
            .join("antigravity")
            .join("daemon")
            .join("hub_demo.json"),
        r#"{"pid":1234,"httpsPort":45123,"lsVersion":"1.22.0"}"#,
    )
    .unwrap();

    let local_detail = detail_inspect::inspect_agent_detail_from_sources(
        "antigravity",
        false,
        Some(home),
        &[],
        None,
        None,
    );
    assert_eq!(local_detail.endpoint, "https://127.0.0.1:45123");
    assert_eq!(local_detail.source, "Antigravity Daemon (1.22.0)");
    assert!(local_detail.models.iter().any(|m| m == "gemini-3-pro"));

    let cc = CcSwitchActiveProvider {
        id: "ag-relay".to_string(),
        name: "AgRelay".to_string(),
        app_type: "antigravity".to_string(),
        upstream_base_url: Some("https://u:sec@ag-gw.example.com/v1?api_key=leak".to_string()),
        upstream_auth_token: Some("secret-token".to_string()),
        models: vec![ClaudeGatewayModel {
            name: "gemini-3.8-pro".to_string(),
            label: None,
            supports_1m: true,
        }],
        upstream_models: Vec::new(),
        is_routed: true,
        proxy_takeover: true,
    };
    let routed_detail = detail_inspect::inspect_agent_detail_from_sources(
        "antigravity",
        true,
        Some(home),
        &[],
        Some(&cc),
        None,
    );
    assert_eq!(routed_detail.endpoint, "https://ag-gw.example.com/v1");
    assert!(!routed_detail.endpoint.contains("sec"));
    assert!(!routed_detail.endpoint.contains("leak"));
    assert_eq!(routed_detail.source, "CC Switch (cc-switch.db)");
    assert_eq!(routed_detail.models[0], "gemini-3.8-pro");
}
#[test]
fn claude_desktop_detail_prefers_config_library_over_stale_cc_switch() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().join("Claude-3p");
    let lib_dir = root.join("configLibrary");
    std::fs::create_dir_all(&lib_dir).unwrap();
    std::fs::write(
        lib_dir.join("_meta.json"),
        r#"{"appliedId":"lib-magpie","entries":[{"id":"lib-magpie","name":"magpie"}]}"#,
    )
    .unwrap();
    std::fs::write(
        lib_dir.join("lib-magpie.json"),
        r#"{"inferenceGatewayBaseUrl":"http://127.0.0.1:3425","inferenceModels":[{"name":"model-alpha"},{"name":"model-beta"}]}"#,
    )
    .unwrap();

    let stale_cc = CcSwitchActiveProvider {
        id: "old".to_string(),
        name: "OldRelay".to_string(),
        app_type: "claude-desktop".to_string(),
        upstream_base_url: Some("https://old.example.com:8317".to_string()),
        upstream_auth_token: Some("sk-old".to_string()),
        models: vec![ClaudeGatewayModel {
            name: "stale-model".to_string(),
            label: None,
            supports_1m: false,
        }],
        upstream_models: Vec::new(),
        is_routed: true,
        proxy_takeover: false,
    };

    let detail = detail_inspect::inspect_agent_detail_from_sources(
        "claude_desktop",
        true,
        None,
        &[root],
        Some(&stale_cc),
        None,
    );
    assert_eq!(detail.endpoint, "http://127.0.0.1:3425");
    assert_eq!(detail.source, "configLibrary:lib-magpie");
    assert_eq!(detail.models, vec!["model-alpha", "model-beta"]);
}

#[test]
fn claude_code_detail_prefers_live_settings_over_stale_cc_switch() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let home = tmp.path();
    std::fs::create_dir_all(home.join(".claude")).unwrap();
    std::fs::write(
        home.join(".claude").join("settings.json"),
        r#"{"env":{"ANTHROPIC_BASE_URL":"http://127.0.0.1:3425","ANTHROPIC_DEFAULT_OPUS_MODEL":"live-opus","ANTHROPIC_SMALL_FAST_MODEL":"live-fast"}}"#,
    )
    .unwrap();

    let stale_cc = CcSwitchActiveProvider {
        id: "default".to_string(),
        name: "default".to_string(),
        app_type: "claude".to_string(),
        upstream_base_url: Some("https://stale.example.com".to_string()),
        upstream_auth_token: None,
        models: vec![ClaudeGatewayModel {
            name: "stale-claude".to_string(),
            label: None,
            supports_1m: false,
        }],
        upstream_models: Vec::new(),
        is_routed: true,
        proxy_takeover: false,
    };

    let detail = detail_inspect::inspect_agent_detail_from_sources(
        "claude_code",
        true,
        Some(home),
        &[],
        Some(&stale_cc),
        None,
    );
    assert_eq!(detail.endpoint, "http://127.0.0.1:3425");
    assert_eq!(detail.source, "~/.claude/settings.json");
    assert_eq!(detail.models, vec!["live-opus", "live-fast"]);
}

#[test]
fn codex_detail_ignores_unselected_model_provider_sections() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let home = tmp.path();
    std::fs::create_dir_all(home.join(".codex")).unwrap();
    std::fs::write(
        home.join(".codex").join("config.toml"),
        "model = \"gpt-5-codex\"\n\n[model_providers.custom]\nbase_url = \"https://unused.example.com:15721/v1\"\n",
    )
    .unwrap();

    let detail = detail_inspect::inspect_agent_detail_from_sources(
        "codex",
        false,
        Some(home),
        &[],
        None,
        None,
    );
    assert_eq!(detail.endpoint, "https://api.openai.com");
    assert_eq!(detail.source, "~/.codex/config.toml");
    assert_eq!(detail.models, vec!["gpt-5-codex"]);
}

#[test]
fn card_layout_empty_agents_renders_fallback_row_to_open_agents() {
    let mut menu = sample_menu();
    menu.agents.clear();
    let layout = CardLayout::compute(&menu, TrayView::Home, TrayTab::Config, menu.period, 96);
    assert_eq!(layout.row_rects.len(), 1);
    let row_mid = (
        (layout.row_rects[0].0.left + layout.row_rects[0].0.right) / 2,
        (layout.row_rects[0].0.top + layout.row_rects[0].0.bottom) / 2,
    );
    assert_eq!(layout.hit(row_mid.0, row_mid.1), Some(HitTarget::Row(0)));
    assert_eq!(
        layout.outcome_for(HitTarget::Row(0), &menu),
        TargetOutcome::InvokeAction(ACTION_OPEN_AGENTS)
    );
}
