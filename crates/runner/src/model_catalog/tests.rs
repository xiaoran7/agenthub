use super::*;

#[test]
fn direct_upstream_desktop_catalog_exposes_real_models_only() {
    let profile = crate::claude_gateway::ClaudeGatewayProfile::new(
        "https://cpa.example/v1",
        "upstream-token",
        "cc-switch:upstream:CPA",
        vec![],
    )
    .with_model_namespace(crate::claude_gateway::GatewayModelNamespace::Upstream);
    let route_models = vec![crate::claude_gateway::ClaudeGatewayModel {
        name: "claude-fable-5".into(),
        label: Some("workbuddy-cn/hy4free:0x".into()),
        supports_1m: true,
    }];
    let upstream_catalogs = vec![(
        "claude_code".to_string(),
        vec![json!({
            "id": "step-5-preview",
            "name": "step-5-preview",
            "display_name": "Step-5 Preview"
        })],
    )];
    let models = desktop_models_for_gateway(&profile, &upstream_catalogs, &route_models);
    let ids: Vec<&str> = models
        .iter()
        .filter_map(|item| item.get("id").and_then(Value::as_str))
        .collect();
    assert_eq!(ids, vec!["step-5-preview"]);
}

#[test]
fn test_format_context_tokens() {
    assert_eq!(format_context_tokens(1_000_000), "1M");
    assert_eq!(format_context_tokens(2_000_000), "2M");
    assert_eq!(format_context_tokens(200_000), "200K");
    assert_eq!(format_context_tokens(128_000), "128K");
    assert_eq!(format_context_tokens(4096), "4K");
}

#[test]
fn test_infer_context_window() {
    let (tokens, str_rep) = infer_context_window("claude-sonnet-5-5-high-1m");
    assert_eq!(tokens, 1_000_000);
    assert_eq!(str_rep, "1M");

    // Models without explicit 1m/2m or upstream context length return 0 / empty string
    let (tokens, str_rep) = infer_context_window("claude-fable-5");
    assert_eq!(tokens, 0);
    assert_eq!(str_rep, "");
}

#[test]
fn reversed_relay_model_ids_use_display_name_as_real_id() {
    let val = json!({
        "data": [{
            "id": "claude-fable-5-dd-weiverp-5-pets",
            "name": "claude-fable-5-dd-weiverp-5-pets",
            "displayName": "Step-5-Preview"
        }, {
            "id": "step-5-preview",
            "name": "step-5-preview",
            "displayName": "step-5-preview"
        }]
    });
    let models = parse_models_response(&val, "CPA");
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].id, "step-5-preview");
    assert!(!models.iter().any(|m| m.id.contains("weiverp")));
}

#[test]
fn live_catalog_json_retains_real_model_ids() {
    let model = LiveCatalogModel {
        id: "step-5-preview".into(),
        name: "step-5-preview".into(),
        display_name: "Step-5-Preview".into(),
        context_length: 128000,
        context_window_str: "128K".into(),
        max_output_tokens: Some(8192),
        input_modalities: vec!["text".into()],
        output_modalities: vec!["text".into()],
        supports_reasoning: true,
        supports_tools: true,
        supports_vision: false,
        upstream_provider: "CPA".into(),
    };
    let value = live_catalog_model_json(&model);
    assert_eq!(value["id"], "step-5-preview");
    assert_eq!(value["display_name"], "Step-5-Preview");
    assert_eq!(value["context_window_str"], "128K");
}

#[test]
fn legacy_disk_cache_without_schema_version_is_rejected() {
    let legacy = json!({
        "fetched_at_ms": 1,
        "base_url": "https://cpa.example",
        "provider_name": "CPA",
        "models": [{
            "id": "claude-fable-5-dd-weiverp-5-pets",
            "name": "claude-fable-5-dd-weiverp-5-pets",
            "display_name": "Step-5-Preview",
            "context_length": 0,
            "context_window_str": "",
            "max_output_tokens": 64000,
            "input_modalities": ["text"],
            "output_modalities": ["text"],
            "supports_reasoning": false,
            "supports_tools": true,
            "supports_vision": true,
            "upstream_provider": "CPA"
        }]
    });
    let cache: DiskModelCache = serde_json::from_value(legacy).unwrap();
    assert_ne!(cache.schema_version, DISK_MODEL_CACHE_VERSION);
    assert_eq!(cache.models[0].id, "claude-fable-5-dd-weiverp-5-pets");
}

#[test]
fn test_parse_models_response() {
    let val = json!({
        "data": [
            {
                "id": "claude-sonnet-5-5-high",
                "displayName": "Claude Sonnet 5.5 High",
                "context_length": 1000000
            },
            {
                "id": "dall-e-3",
                "displayName": "DALL-E 3"
            }
        ]
    });

    let models = parse_models_response(&val, "TestGateway");
    assert_eq!(models.len(), 1);
    assert_eq!(models[0].id, "claude-sonnet-5-5-high");
    assert_eq!(models[0].context_length, 1_000_000);
    assert_eq!(models[0].context_window_str, "1M");
    assert_eq!(models[0].upstream_provider, "TestGateway");
}

#[test]
fn validated_upstream_candidate_filters_placeholders_mismatched_ccs_and_unreachable_loopback() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let open_base = format!("http://127.0.0.1:{port}");
    let ccs_base = format!("http://127.0.0.1:{port}/claude-desktop");

    assert!(validated_upstream_candidate(&open_base, "PROXY_MANAGED", "src").is_none());
    assert!(validated_upstream_candidate(&open_base, "__sealed__:v1:x", "src").is_none());
    assert!(validated_upstream_candidate(&open_base, "ccs-123", "src").is_none());
    assert!(validated_upstream_candidate("", "sk-live", "src").is_none());

    let valid = validated_upstream_candidate(&format!(" {open_base} "), " sk-live ", "CPA");
    assert_eq!(
        valid,
        Some((open_base.clone(), "sk-live".to_string(), "CPA".to_string()))
    );

    let valid_ccs = validated_upstream_candidate(&ccs_base, "ccs-123", "CCS");
    assert_eq!(
        valid_ccs,
        Some((ccs_base, "ccs-123".to_string(), "CCS".to_string()))
    );

    drop(listener);
    assert!(validated_upstream_candidate(&open_base, "sk-live", "CPA").is_none());
}
#[test]
fn save_disk_cache_to_writes_atomically_without_leaving_temp_files() {
    let dir = tempfile::tempdir().unwrap();
    let cache_path = dir.path().join("models_cache.json");
    let model = LiveCatalogModel {
        id: "claude-opus-4-6".to_owned(),
        name: "claude-opus-4-6".to_owned(),
        display_name: "Claude Opus 4.6".to_owned(),
        context_length: 200_000,
        context_window_str: "200K".to_owned(),
        max_output_tokens: Some(32_000),
        input_modalities: vec!["text".to_owned()],
        output_modalities: vec!["text".to_owned()],
        supports_reasoning: true,
        supports_tools: true,
        supports_vision: true,
        upstream_provider: "Magpie".to_owned(),
    };
    disk_cache::save_disk_cache_to(
        &cache_path,
        "http://127.0.0.1:3425",
        "Magpie",
        std::slice::from_ref(&model),
    )
    .unwrap();
    disk_cache::save_disk_cache_to(&cache_path, "http://127.0.0.1:3425", "Magpie", &[model])
        .unwrap();

    let raw = fs::read_to_string(&cache_path).unwrap();
    let parsed: DiskModelCache = serde_json::from_str(&raw).unwrap();
    assert_eq!(parsed.schema_version, DISK_MODEL_CACHE_VERSION);
    assert_eq!(parsed.models.len(), 1);
    assert_eq!(parsed.models[0].id, "claude-opus-4-6");

    let entries: Vec<_> = fs::read_dir(dir.path()).unwrap().flatten().collect();
    assert_eq!(
        entries.len(),
        1,
        "no temporary files may linger after persist"
    );
}
