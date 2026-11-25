use super::*;

#[test]
fn model_resolution_exact_and_fuzzy() {
    let profile = ClaudeGatewayProfile::new(
        "http://127.0.0.1:15721/claude-desktop",
        "ccs-test",
        "configLibrary:test",
        vec![
            ClaudeGatewayModel {
                name: "claude-fable-5".into(),
                label: Some("gemini-3.8-flash-high".into()),
                supports_1m: true,
            },
            ClaudeGatewayModel {
                name: "claude-opus-5".into(),
                label: Some("alpha".into()),
                supports_1m: true,
            },
            ClaudeGatewayModel {
                name: "claude-sonnet-5".into(),
                label: Some("claude-opus-4-6".into()),
                supports_1m: true,
            },
        ],
    )
    .with_model_namespace(GatewayModelNamespace::RouteAlias);

    // 1. Exact match on router model name
    assert_eq!(
        profile.resolve_model(Some("claude-fable-5")),
        Some("claude-fable-5".into())
    );

    // 2. Exact match on backend label
    assert_eq!(
        profile.resolve_model(Some("gemini-3.8-flash-high")),
        Some("claude-fable-5".into())
    );

    // 3. Prefix match on backend label from transcript ("gemini-3.8-flash")
    assert_eq!(
        profile.resolve_model(Some("gemini-3.8-flash")),
        Some("claude-fable-5".into())
    );

    // 4. Family keyword match ("alpha")
    assert_eq!(
        profile.resolve_model(Some("alpha")),
        Some("claude-opus-5".into())
    );

    // 5. Default fallback on empty/none prioritizes flagship opus
    assert_eq!(profile.resolve_model(None), Some("claude-opus-5".into()));

    // 6. Unknown model fallback to flagship opus
    assert_eq!(
        profile.resolve_model(Some("unrecognized-custom-model")),
        Some("claude-opus-5".into())
    );
}

#[test]
fn inject_into_command_sets_env_and_resolves_model() {
    let profile = ClaudeGatewayProfile::new(
        "http://127.0.0.1:15721/claude-desktop",
        "ccs-token-xyz",
        "configLibrary:test",
        vec![ClaudeGatewayModel {
            name: "claude-fable-5".into(),
            label: Some("gemini-3.8-flash".into()),
            supports_1m: true,
        }],
    )
    .with_model_namespace(GatewayModelNamespace::RouteAlias);

    let mut cmd = Command::new("claude");
    let mut model = Some("gemini-3.8-flash".to_string());
    profile.inject_into_command(&mut cmd, &mut model);

    assert_eq!(model, Some("claude-fable-5".to_string()));
}

#[test]
fn direct_upstream_keeps_real_model_id() {
    let profile = ClaudeGatewayProfile::new(
        "https://cpa.example/v1",
        "upstream-token",
        "cc-switch:upstream:CPA",
        vec![ClaudeGatewayModel {
            name: "claude-opus-5".into(),
            label: Some("deepseek-v4.1-flash".into()),
            supports_1m: false,
        }],
    );

    let mut cmd = Command::new("claude");
    let mut model = Some("step-5-preview".to_string());
    profile.inject_into_command(&mut cmd, &mut model);
    assert_eq!(model, Some("step-5-preview".to_string()));

    let mut marker_model = Some("gemini-3.8-flash-high[1M]".to_string());
    profile.inject_into_command(&mut cmd, &mut marker_model);
    assert_eq!(marker_model, Some("gemini-3.8-flash-high".to_string()));
}

#[test]
fn namespace_detects_ccs_route_and_direct_upstream() {
    let ccs = ClaudeGatewayProfile::new(
        "https://host.example:15721/claude-desktop",
        "ccs-token",
        "environment",
        vec![],
    );
    assert_eq!(ccs.model_namespace, GatewayModelNamespace::RouteAlias);

    let upstream = ClaudeGatewayProfile::new(
        "https://cpa.example",
        "upstream-token",
        "cc-switch:upstream:CPA",
        vec![],
    );
    assert_eq!(upstream.model_namespace, GatewayModelNamespace::Upstream);
}

#[test]
fn official_oauth_keeps_native_model_alias() {
    let profile = ClaudeGatewayProfile::new(
        "https://cpa.example/v1",
        "upstream-token",
        "cc-switch:upstream:CPA",
        vec![ClaudeGatewayModel {
            name: "claude-opus-5".into(),
            label: Some("deepseek-v4.1-flash".into()),
            supports_1m: false,
        }],
    );

    let mut cmd = Command::new("claude");
    let mut model = Some("opus".to_string());
    profile.inject_into_command(&mut cmd, &mut model);
    assert_eq!(model, Some("opus".to_string()));
}

#[test]
fn direct_upstream_maps_stale_desktop_route_alias_to_real_model() {
    let profile = ClaudeGatewayProfile::new(
        "https://cpa.example/v1",
        "upstream-token",
        "cc-switch:upstream:CPA",
        vec![ClaudeGatewayModel {
            name: "gemini-3.8-flash-high".into(),
            label: Some("claude-sonnet-5".into()),
            supports_1m: false,
        }],
    )
    .with_model_namespace(GatewayModelNamespace::Upstream);
    let route_models = vec![ClaudeGatewayModel {
        name: "claude-sonnet-5".into(),
        label: Some("gemini-3.8-flash-high".into()),
        supports_1m: true,
    }];
    let upstream_models = vec![ClaudeGatewayModel {
        name: "gemini-3.8-flash-high".into(),
        label: None,
        supports_1m: true,
    }];
    assert_eq!(
        profile.upstream_model_for_route_alias(
            Some("claude-sonnet-5"),
            &route_models,
            &upstream_models,
        ),
        Some("gemini-3.8-flash-high".to_string())
    );
    assert_eq!(
        profile.upstream_model_for_route_alias(
            Some("step-5-preview"),
            &route_models,
            &upstream_models,
        ),
        None
    );
}

#[test]
fn direct_upstream_default_never_returns_a_route_alias() {
    let models = vec![
        ClaudeGatewayModel {
            name: "claude-opus-5".into(),
            label: Some("deepseek-v4.1-flash".into()),
            supports_1m: false,
        },
        ClaudeGatewayModel {
            name: "gemini-3.8-flash-high".into(),
            label: None,
            supports_1m: true,
        },
    ];
    assert_eq!(
        preferred_upstream_default(&models, None, Some("gemini-3.8-flash-high")),
        Some("gemini-3.8-flash-high".to_string())
    );
    assert_eq!(
        preferred_upstream_default(&models, Some("claude-opus-5"), None),
        Some("gemini-3.8-flash-high".to_string())
    );
    assert_eq!(preferred_upstream_default(&models[..1], None, None), None);
}

#[test]
fn upstream_that_catalogs_route_aliases_keeps_them() {
    let profile = ClaudeGatewayProfile::new(
        "https://relay.example/v1",
        "relay-token",
        "cc-switch:Relay",
        vec![ClaudeGatewayModel {
            name: "claude-sonnet-5".into(),
            label: None,
            supports_1m: true,
        }],
    )
    .with_model_namespace(GatewayModelNamespace::Upstream);
    assert!(
        profile
            .models
            .iter()
            .any(|m| m.name.to_ascii_lowercase().starts_with("claude-"))
    );
    let mut cmd = Command::new("claude");
    let mut model = Some("claude-sonnet-5".to_string());
    profile.inject_into_command(&mut cmd, &mut model);
    assert_eq!(model, Some("claude-sonnet-5".to_string()));
}

#[test]
fn ccs_endpoint_recognizes_loopback_and_tailnet() {
    assert!(is_cc_switch_endpoint(
        "http://127.0.0.1:15721/claude-desktop"
    ));
    assert!(is_cc_switch_endpoint("https://localhost:15721"));
    assert!(is_cc_switch_endpoint(
        "https://portable-pc.tail462ee4.ts.net:15721/claude-desktop"
    ));
    assert!(is_cc_switch_endpoint(
        "https://my-host.tail462ee4.ts.net/claude-desktop"
    ));
    assert!(!is_cc_switch_endpoint("https://api.anthropic.com"));
    assert!(!is_cc_switch_endpoint("https://cpa.example.com/v1"));
}

#[test]
fn direct_upstream_bypass_keeps_upstream_namespace() {
    let profile = ClaudeGatewayProfile::new(
        "https://cpa.example/v1",
        "upstream-token",
        "cc-switch:upstream:CPA",
        vec![],
    )
    .with_model_namespace(GatewayModelNamespace::Upstream);
    assert_eq!(profile.model_namespace, GatewayModelNamespace::Upstream);
    assert_eq!(profile.source, "cc-switch:upstream:CPA");
}

#[test]
fn gateway_pair_requires_both_endpoint_and_credential() {
    assert_eq!(
        gateway_pair_from_values(
            Some(" https://gateway.example "),
            Some(" explicit-token "),
            None
        ),
        Some((
            "https://gateway.example".to_string(),
            "explicit-token".to_string()
        ))
    );
    assert_eq!(
        gateway_pair_from_values(Some("https://gateway.example"), None, Some("api-key")),
        Some(("https://gateway.example".to_string(), "api-key".to_string()))
    );
    assert_eq!(
        gateway_pair_from_values(Some("https://orphaned.example"), None, None),
        None
    );
    assert_eq!(gateway_pair_from_values(None, Some("token"), None), None);
    assert_eq!(
        gateway_pair_from_values(Some("  "), Some("token"), None),
        None
    );
}

#[test]
fn discovered_gateway_pair_is_applied_to_child_env() {
    let profile = ClaudeGatewayProfile::new(
        "https://cpa.example",
        "discovered-token",
        "settings",
        vec![],
    );
    let mut command = Command::new("claude");
    let mut model = None;
    profile.inject_into_command(&mut command, &mut model);
    let envs: std::collections::HashMap<String, String> = command
        .get_envs()
        .map(|(key, value)| {
            (
                key.to_string_lossy().into_owned(),
                value.unwrap_or_default().to_string_lossy().into_owned(),
            )
        })
        .collect();
    assert_eq!(
        envs.get("ANTHROPIC_BASE_URL").map(String::as_str),
        Some("https://cpa.example")
    );
    assert_eq!(
        envs.get("ANTHROPIC_AUTH_TOKEN").map(String::as_str),
        Some("discovered-token")
    );
}

#[test]
fn merge_gateway_models_deduplicates_by_name_and_label() {
    let mut target = vec![
        ClaudeGatewayModel {
            name: "claude-opus-5".into(),
            label: Some("claude-opus-5-5-high".into()),
            supports_1m: true,
        },
        ClaudeGatewayModel {
            name: "claude-sonnet-5".into(),
            label: Some("claude-sonnet-5-5-high".into()),
            supports_1m: true,
        },
    ];

    let incoming = vec![
        ClaudeGatewayModel {
            name: "claude-opus-5-5-high".into(),
            label: None,
            supports_1m: true,
        },
        ClaudeGatewayModel {
            name: "claude-opus-4-6-thinking".into(),
            label: None,
            supports_1m: true,
        },
        ClaudeGatewayModel {
            name: "gemini-3.8-flash-high".into(),
            label: None,
            supports_1m: true,
        },
    ];

    merge_gateway_models(&mut target, incoming);

    // claude-opus-5-5-high should NOT be duplicated since claude-opus-5 routes to it
    assert_eq!(target.len(), 4);
    assert_eq!(target[0].name, "claude-opus-5");
    assert_eq!(target[1].name, "claude-sonnet-5");
    assert_eq!(target[2].name, "claude-opus-4-6-thinking");
    assert_eq!(target[3].name, "gemini-3.8-flash-high");
}

#[test]
fn codex_and_antigravity_gateway_profiles_inject_expected_envs() {
    let codex_profile = CodexGatewayProfile::new(
        "https://cpa.example.com/v1",
        "sk-codex-test-key",
        "test",
        vec![ClaudeGatewayModel {
            name: "o3-mini".into(),
            label: None,
            supports_1m: true,
        }],
    );
    let mut cmd = Command::new("codex");
    let mut model_override = Some("o3-mini".to_string());
    codex_profile.inject_into_command_with_session(
        &mut cmd,
        &mut model_override,
        Some("codex_session_123"),
    );

    let envs: std::collections::HashMap<String, String> = cmd
        .get_envs()
        .filter_map(|(k, v)| {
            v.map(|val| {
                (
                    k.to_string_lossy().into_owned(),
                    val.to_string_lossy().into_owned(),
                )
            })
        })
        .collect();

    assert_eq!(
        envs.get("OPENAI_BASE_URL").map(String::as_str),
        Some("https://cpa.example.com/v1")
    );
    assert_eq!(
        envs.get("OPENAI_API_KEY").map(String::as_str),
        Some("sk-codex-test-key")
    );
    assert_eq!(
        envs.get("X_MAGPIE_SESSION").map(String::as_str),
        Some("session_123")
    );
    assert_eq!(model_override.as_deref(), Some("o3-mini"));

    let ag_profile = AntigravityGatewayProfile::new(
        "http://127.0.0.1:8316",
        "gemini-test-token",
        "test",
        vec![ClaudeGatewayModel {
            name: "gemini-3.8-flash-high[1M]".into(),
            label: None,
            supports_1m: true,
        }],
    );
    let mut ag_cmd = Command::new("antigravity");
    let mut ag_model = Some("gemini-3.8-flash-high[1M]".to_string());
    ag_profile.inject_into_command_with_session(
        &mut ag_cmd,
        &mut ag_model,
        Some("antigravity_sess_456"),
    );

    let ag_envs: std::collections::HashMap<String, String> = ag_cmd
        .get_envs()
        .filter_map(|(k, v)| {
            v.map(|val| {
                (
                    k.to_string_lossy().into_owned(),
                    val.to_string_lossy().into_owned(),
                )
            })
        })
        .collect();

    assert_eq!(
        ag_envs.get("GOOGLE_GEMINI_BASE_URL").map(String::as_str),
        Some("http://127.0.0.1:8316")
    );
    assert_eq!(
        ag_envs.get("GEMINI_API_KEY").map(String::as_str),
        Some("gemini-test-token")
    );
    assert_eq!(
        ag_envs.get("GEMINI_MODEL").map(String::as_str),
        Some("gemini-3.8-flash-high[1M]")
    );
    assert_eq!(
        ag_envs.get("X_MAGPIE_SESSION").map(String::as_str),
        Some("sess_456")
    );
}

#[test]
fn discover_cc_switch_reads_active_provider_if_present() {
    if cc_switch_db_path().is_some() {
        let active = discover_cc_switch_active_provider();
        if let Some(prov) = active {
            assert!(!prov.id.is_empty());
            assert!(!prov.name.is_empty());
        }
    }
}

#[test]
fn token_usability_and_endpoint_compatibility_filters_placeholders() {
    assert!(!is_usable_gateway_token(""));
    assert!(!is_usable_gateway_token("   \t  "));
    assert!(!is_usable_gateway_token("__sealed__:v1:ciphertext"));
    assert!(!is_usable_gateway_token("PROXY_MANAGED"));
    assert!(!is_usable_gateway_token("proxy_managed"));
    assert!(is_usable_gateway_token("sk-live-123"));
    assert!(is_usable_gateway_token("ccs-local-token"));

    assert!(!is_token_compatible_with_endpoint(
        "http://127.0.0.1:8316",
        "ccs-local-token"
    ));
    assert!(!is_token_compatible_with_endpoint(
        "http://127.0.0.1:11434",
        "ccs-local-token"
    ));
    assert!(!is_token_compatible_with_endpoint(
        "https://portable-pc.tailc42b47.ts.net/v1",
        "ccs-local-token"
    ));
    assert!(is_token_compatible_with_endpoint(
        "http://127.0.0.1:15721",
        "ccs-local-token"
    ));
    assert!(is_token_compatible_with_endpoint(
        "https://portable-pc.tailc42b47.ts.net/claude-desktop",
        "ccs-local-token"
    ));
    assert!(is_token_compatible_with_endpoint(
        "http://127.0.0.1:8316",
        "sk-live-123"
    ));
}

#[test]
fn cc_switch_endpoint_excludes_unrelated_loopback_and_tailnet_ports() {
    assert!(is_cc_switch_endpoint("http://127.0.0.1:15721"));
    assert!(is_cc_switch_endpoint("http://[::1]:15721/v1"));
    assert!(is_cc_switch_endpoint(
        "https://portable-pc.tailc42b47.ts.net/claude-desktop"
    ));
    assert!(is_cc_switch_endpoint(
        "https://portable-pc.tailc42b47.ts.net/claude-desktop/v1"
    ));

    assert!(!is_cc_switch_endpoint("http://127.0.0.1:8316"));
    assert!(!is_cc_switch_endpoint("http://127.0.0.1:11434"));
    assert!(!is_cc_switch_endpoint("http://127.0.0.1:3425/v1"));
    assert!(!is_cc_switch_endpoint(
        "https://portable-pc.tailc42b47.ts.net/v1"
    ));
    assert!(!is_cc_switch_endpoint("ftp://127.0.0.1:15721"));
}

#[test]
fn namespace_for_endpoint_requires_ccs_endpoint_and_ccs_token() {
    let valid_ccs = ClaudeGatewayProfile::new(
        "http://127.0.0.1:15721/claude-desktop",
        "ccs-123",
        "test",
        vec![],
    );
    assert_eq!(valid_ccs.model_namespace, GatewayModelNamespace::RouteAlias);

    let sealed_on_ccs = ClaudeGatewayProfile::new(
        "http://127.0.0.1:15721",
        "__sealed__:v1:opaque",
        "test",
        vec![],
    );
    assert_eq!(
        sealed_on_ccs.model_namespace,
        GatewayModelNamespace::Upstream
    );

    let ccs_tok_on_cpa =
        ClaudeGatewayProfile::new("http://127.0.0.1:8316", "ccs-123", "test", vec![]);
    assert_eq!(
        ccs_tok_on_cpa.model_namespace,
        GatewayModelNamespace::Upstream
    );

    let upstream_tok_on_path = ClaudeGatewayProfile::new(
        "https://portable-pc.ts.net/claude-desktop",
        "sk-real",
        "test",
        vec![],
    );
    assert_eq!(
        upstream_tok_on_path.model_namespace,
        GatewayModelNamespace::Upstream
    );
}

#[test]
fn gateway_reachability_and_host_classification_cover_ipv4_ipv6_and_tailnet() {
    assert!(is_loopback_host("localhost"));
    assert!(is_loopback_host("api.localhost"));
    assert!(is_loopback_host("127.0.0.42"));
    assert!(is_loopback_host("::1"));
    assert!(is_loopback_host("[::1]"));
    assert!(is_loopback_host("::ffff:127.0.0.1"));

    assert!(is_private_or_tailnet_host("100.64.0.42"));
    assert!(is_private_or_tailnet_host("169.254.1.2"));
    assert!(is_private_or_tailnet_host("fd7a:115c:a1e0::1"));
    assert!(is_private_or_tailnet_host("fe80::1"));
    assert!(is_private_or_tailnet_host("::ffff:100.64.0.99"));
    assert!(is_private_or_tailnet_host("portable-pc.tail000001.ts.net"));
    assert!(is_private_or_tailnet_host("nas.local"));
    assert!(is_private_or_tailnet_host("gateway"));

    assert!(!is_private_or_tailnet_host("2001:4860:4860::8888"));
    assert!(!is_private_or_tailnet_host("[2001:4860:4860::8888]"));
    assert!(!is_private_or_tailnet_host("::ffff:8.8.8.8"));
    assert!(!is_private_or_tailnet_host("8.8.8.8"));
    assert!(!is_private_or_tailnet_host("api.anthropic.com"));

    // Public WAN literals return immediately without local port binding.
    assert!(is_gateway_url_reachable(
        "https://[2001:4860:4860::8888]/v1"
    ));
    assert!(is_gateway_url_reachable("https://1.1.1.1/v1"));

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let url = format!("http://127.0.0.1:{port}");
    assert!(is_gateway_url_reachable(&url));
    drop(listener);
    assert!(!is_gateway_url_reachable(&url));
}

#[test]
fn fetch_live_upstream_models_caches_per_token_and_never_caches_401() {
    use std::io::{BufRead, BufReader, Write};
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://127.0.0.1:{}", addr.port());

    let done = Arc::new(AtomicBool::new(false));
    let allow_bad_token = Arc::new(AtomicBool::new(false));
    let bad_hits = Arc::new(AtomicUsize::new(0));
    let good_hits = Arc::new(AtomicUsize::new(0));

    let done_srv = Arc::clone(&done);
    let allow_bad_srv = Arc::clone(&allow_bad_token);
    let bad_hits_srv = Arc::clone(&bad_hits);
    let good_hits_srv = Arc::clone(&good_hits);

    let handle = std::thread::spawn(move || {
        while !done_srv.load(Ordering::Acquire) {
            let Ok((mut stream, _)) = listener.accept() else {
                break;
            };
            if done_srv.load(Ordering::Acquire) {
                break;
            }
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut headers = Vec::new();
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        if line == "\r\n" || line == "\n" {
                            break;
                        }
                        headers.push(line);
                    }
                }
            }
            if headers.is_empty() {
                // Reachability probe closes immediately without HTTP request.
                continue;
            }
            let auth = headers
                .iter()
                .find_map(|h| {
                    let (k, v) = h.split_once(':')?;
                    if k.trim().eq_ignore_ascii_case("authorization") {
                        Some(v.trim().to_string())
                    } else {
                        None
                    }
                })
                .unwrap_or_default();

            if auth == "Bearer sk-bad-test" {
                bad_hits_srv.fetch_add(1, Ordering::SeqCst);
                if allow_bad_srv.load(Ordering::Acquire) {
                    let body = r#"{"data":[{"id":"claude-opus-5-5"}]}"#;
                    let resp = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = stream.write_all(resp.as_bytes());
                } else {
                    let resp = "HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                    let _ = stream.write_all(resp.as_bytes());
                }
            } else if auth == "Bearer sk-good-test" {
                good_hits_srv.fetch_add(1, Ordering::SeqCst);
                let body = r#"{"data":[{"id":"claude-sonnet-5-5"},{"id":"dall-e-3-image"}]}"#;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(resp.as_bytes());
            } else {
                let resp =
                    "HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                let _ = stream.write_all(resp.as_bytes());
            }
            let _ = stream.flush();
        }
    });

    // Unusable placeholders and mismatched ccs-* must short-circuit before hitting HTTP.
    assert!(fetch_live_upstream_models(&base_url, "PROXY_MANAGED").is_empty());
    assert!(fetch_live_upstream_models(&base_url, "ccs-orphan").is_empty());
    assert_eq!(bad_hits.load(Ordering::SeqCst), 0);
    assert_eq!(good_hits.load(Ordering::SeqCst), 0);

    // 401 on bad token returns empty and must not poison the same base_url for good token.
    assert!(fetch_live_upstream_models(&base_url, "sk-bad-test").is_empty());
    assert_eq!(bad_hits.load(Ordering::SeqCst), 1);

    let good_models = fetch_live_upstream_models(&base_url, "sk-good-test");
    assert_eq!(good_models.len(), 1);
    assert_eq!(good_models[0].name, "claude-sonnet-5-5");
    assert_eq!(good_hits.load(Ordering::SeqCst), 1);

    // Good token hits cache (still 1 HTTP hit).
    let cached_good = fetch_live_upstream_models(&base_url, "sk-good-test");
    assert_eq!(cached_good.len(), 1);
    assert_eq!(good_hits.load(Ordering::SeqCst), 1);

    // Bad token was NOT cached on 401, so fixing auth succeeds on the next call.
    allow_bad_token.store(true, Ordering::Release);
    let recovered = fetch_live_upstream_models(&base_url, "sk-bad-test");
    assert_eq!(recovered.len(), 1);
    assert_eq!(recovered[0].name, "claude-opus-5-5");
    assert_eq!(bad_hits.load(Ordering::SeqCst), 2);

    done.store(true, Ordering::Release);
    let _ = std::net::TcpStream::connect(addr);
    handle.join().unwrap();
}

#[test]
fn discover_config_library_with_rejects_unusable_and_unsealed_tokens() {
    let dir = tempfile::tempdir().unwrap();
    let cfg_dir = dir.path().join("configLibrary");
    fs::create_dir_all(&cfg_dir).unwrap();
    fs::write(cfg_dir.join("_meta.json"), r#"{"appliedId":"prof-1"}"#).unwrap();

    // Sealed token on non-CCS endpoint must be rejected even if CCS token exists.
    fs::write(
            cfg_dir.join("prof-1.json"),
            r#"{"inferenceGatewayBaseUrl":"http://127.0.0.1:8316","inferenceGatewayApiKey":"__sealed__:v1:abc"}"#,
        )
        .unwrap();
    assert!(
        discover_config_library_with(&[dir.path().to_path_buf()], || Some("ccs-real".to_string()))
            .is_none()
    );

    // Sealed token on CCS endpoint without CCS lookup must NOT fall back to other keys.
    fs::write(
            cfg_dir.join("prof-1.json"),
            r#"{"inferenceGatewayBaseUrl":"http://127.0.0.1:15721/claude-desktop","inferenceGatewayApiKey":"__sealed__:v1:abc"}"#,
        )
        .unwrap();
    assert!(discover_config_library_with(&[dir.path().to_path_buf()], || None).is_none());

    // Sealed token on CCS endpoint WITH CCS lookup restores plaintext token.
    let found =
        discover_config_library_with(&[dir.path().to_path_buf()], || Some("ccs-real".to_string()))
            .unwrap();
    assert_eq!(found.0, "http://127.0.0.1:15721/claude-desktop");
    assert_eq!(found.1, "ccs-real");

    // PROXY_MANAGED placeholder is rejected.
    fs::write(
            cfg_dir.join("prof-1.json"),
            r#"{"inferenceGatewayBaseUrl":"http://127.0.0.1:15721","inferenceGatewayApiKey":"PROXY_MANAGED"}"#,
        )
        .unwrap();
    assert!(
        discover_config_library_with(&[dir.path().to_path_buf()], || Some("ccs-real".to_string()))
            .is_none()
    );
}

#[test]
fn windows_msix_claude_roots_in_discovers_packaged_profiles() {
    let dir = tempfile::tempdir().unwrap();
    let packages = dir.path().join("Packages");
    fs::create_dir_all(packages.join("AnthropicPBC.Claude_p234/LocalCache/Roaming")).unwrap();
    fs::create_dir_all(packages.join("Microsoft.WindowsTerminal_123/LocalCache")).unwrap();

    let roots = windows_msix_claude_roots_in(dir.path());
    assert_eq!(roots.len(), 4);
    assert!(
        roots
            .iter()
            .all(|p| p.to_string_lossy().contains("AnthropicPBC.Claude_p234"))
    );
    assert!(roots.iter().any(|p| {
        p.ends_with("LocalCache/Roaming/Claude-3p")
            || p.ends_with(
                PathBuf::from("LocalCache")
                    .join("Roaming")
                    .join("Claude-3p"),
            )
    }));
}
#[test]
fn codex_and_antigravity_gateways_skip_invalid_session_id_injection() {
    let codex = CodexGatewayProfile::new("http://127.0.0.1:3425/v1", "sk-test", "test", vec![]);
    let mut codex_cmd = Command::new("codex");
    let mut m = None;
    codex.inject_into_command_with_session(&mut codex_cmd, &mut m, Some("codex_../evil"));
    assert!(
        codex_cmd
            .get_envs()
            .all(|(k, _)| k != "X_MAGPIE_SESSION" && k != "X_CONVERSATION_ID")
    );

    let ag = AntigravityGatewayProfile::new("http://127.0.0.1:3425", "sk-test", "test", vec![]);
    let mut ag_cmd = Command::new("antigravity");
    ag.inject_into_command_with_session(&mut ag_cmd, &mut m, Some("antigravity_bad\nheader"));
    assert!(
        ag_cmd
            .get_envs()
            .all(|(k, _)| k != "X_MAGPIE_SESSION" && k != "X_CONVERSATION_ID")
    );
}
