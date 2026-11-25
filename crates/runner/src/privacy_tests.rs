use super::*;

#[test]
fn hides_full_path_and_keeps_project_name() {
    let redactor = ProjectRedactor::new(b"device-secret").unwrap();
    let project = redactor.redact(r"C:\Users\alice\source\payments-api");

    assert_eq!(project.display_name, "payments-api");
    assert_eq!(project.fingerprint.len(), 64);
    assert!(!project.fingerprint.contains("alice"));
}

#[test]
fn fingerprints_are_stable_for_the_same_path() {
    let redactor = ProjectRedactor::new(b"device-secret").unwrap();
    assert_eq!(
        redactor.redact("/work/app").fingerprint,
        redactor.redact("/work/app").fingerprint
    );
}

#[test]
fn home_directory_is_not_used_as_a_display_name() {
    assert!(matches_home(r"C:\Users\alice", r"C:\Users\alice"));
    #[cfg(windows)]
    assert!(matches_home(r"C:\Users\Alice", r"c:/users/alice"));
    assert!(!matches_home(r"C:\Users\alice\project", r"C:\Users\alice"));
    assert!(!matches_home(r"C:\Users\alice", ""));
}

#[test]
fn workspace_id_is_stable_and_does_not_reveal_full_path() {
    let ws1 = workspace_id_for_path(r"C:\Users\alice\code\my-app");
    let ws2 = workspace_id_for_path(r"c:\users\alice\code\my-app\");
    assert_eq!(ws1, ws2);
    assert!(ws1.starts_with("ws-my-app-"));
    assert!(!ws1.contains("alice"));
    assert!(!ws1.contains("code"));
}

#[test]
fn redact_paths_inside_text_keeps_the_surrounding_explanation() {
    // The real failure the remote console used to lose entirely: a provider
    // error whose message names both the cause and a config file on disk.
    let redacted = redact_paths_inside_text(
        "The 'gpt-6-luna' model is not supported; check C:\\Users\\alice\\codex\\config.toml",
    );
    assert!(redacted.contains("gpt-6-luna"), "the cause must survive");
    assert!(
        redacted.contains("is not supported"),
        "the wording must survive"
    );
    assert!(!redacted.contains("alice"));
    assert!(!redacted.contains("config.toml"));
    assert!(redacted.contains("[local_path_redacted]"));

    // Trailing sentence punctuation must not defeat the match.
    assert_eq!(
        redact_paths_inside_text("failed to read /home/alice/.codex/auth.json."),
        "failed to read [local_path_redacted]."
    );
    // A path in parentheses stays bounded by its own punctuation.
    assert_eq!(
        redact_paths_inside_text("see (C:\\Users\\alice\\app) for detail"),
        "see ([local_path_redacted]) for detail"
    );
    // Two paths in one sentence are both rewritten.
    assert_eq!(
        redact_paths_inside_text("copy /home/alice/a to /home/alice/b"),
        "copy [local_path_redacted] to [local_path_redacted]"
    );
    // A token made only of delimiters counts as both a leading and a trailing
    // run, so the two overlap; it must not take the redaction down with it.
    assert_eq!(
        redact_paths_inside_text(r#"unexpected token " at C:\Users\alice\config"#),
        r#"unexpected token " at [local_path_redacted]"#
    );
    assert_eq!(
        redact_paths_inside_text(r#"see "" near C:\Users\alice\config"#),
        r#"see "" near [local_path_redacted]"#
    );
}

#[test]
fn redact_paths_inside_text_leaves_clean_text_untouched() {
    for clean in [
        "model is not supported",
        "HTTP 429 too many requests",
        "rate limit exceeded, retry later",
        "",
    ] {
        assert_eq!(
            redact_paths_inside_text(clean),
            clean,
            "text without an absolute path must not be redacted"
        );
    }
}

#[test]
fn sanitize_workspace_paths_in_json_replaces_cwd_and_nested_paths() {
    let mut sample = serde_json::json!({
        "type": "data",
        "provider_response": {
            "thread": {
                "id": "thread-1",
                "cwd": "C:\\Users\\alice\\projects\\super-app"
            }
        },
        "events": [
            {
                "raw": {
                    "params": {
                        "cwd": "/home/bob/secret-repo",
                        "turn": {
                            "id": "turn-1"
                        }
                    }
                },
                "binding": {
                    "workspace_path": "D:\\Workspaces\\team-portal"
                }
            }
        ]
    });
    sanitize_workspace_paths_in_json(&mut sample);
    let serialized = sample.to_string();
    assert!(!serialized.contains("alice"));
    assert!(!serialized.contains("bob"));
    assert!(!serialized.contains("Users"));
    assert!(!serialized.contains("home"));
    assert!(!serialized.contains("D:\\\\Workspaces"));
    assert_eq!(
        sample["provider_response"]["thread"]["cwd"]
            .as_str()
            .unwrap(),
        workspace_id_for_path("C:\\Users\\alice\\projects\\super-app")
    );
    assert_eq!(
        sample["events"][0]["raw"]["params"]["cwd"]
            .as_str()
            .unwrap(),
        workspace_id_for_path("/home/bob/secret-repo")
    );
    assert_eq!(
        sample["events"][0]["binding"]["workspace_path"]
            .as_str()
            .unwrap(),
        workspace_id_for_path("D:\\Workspaces\\team-portal")
    );
}

#[test]
fn sanitize_workspace_paths_in_json_redacts_embedded_paths_and_credentials() {
    let mut sample = serde_json::json!({
        "command": "cat C:\\Users\\alice\\repo\\.env",
        "stderr": "failed at /home/alice/private/config.json",
        "url": "https://example.com/private/resource",
        "Authorization": "Bearer usable-secret",
        "access_token": "usable-token",
        "environment": {"SAFE": "value"}
    });

    sanitize_workspace_paths_in_json(&mut sample);

    assert_eq!(sample["command"], "cat [local_path_redacted]");
    assert_eq!(sample["stderr"], "failed at [local_path_redacted]");
    assert_eq!(sample["url"], "https://example.com/private/resource");
    assert_eq!(sample["Authorization"], "[redacted]");
    assert_eq!(sample["access_token"], "[redacted]");
    assert_eq!(sample["environment"], "[redacted]");
    let serialized = sample.to_string();
    for forbidden in ["alice", "usable-secret", "usable-token"] {
        assert!(!serialized.contains(forbidden));
    }
}

#[test]
fn sanitize_workspace_paths_in_json_cleans_tool_events_and_arbitrary_paths() {
    let mut sample = serde_json::json!({
        "type": "event",
        "event": {
            "kind": "item/fileChange/requestApproval",
            "raw": {
                "method": "item/fileChange/requestApproval",
                "params": {
                    "filePath": "C:\\Users\\admin\\repo\\src\\main.rs",
                    "diff": "--- a/src/main.rs\n+++ b/src/main.rs",
                    "files": [
                        "/Users/developer/code/backend/server.go",
                        "D:\\Shared\\docs\\spec.pdf"
                    ]
                }
            }
        }
    });
    sanitize_workspace_paths_in_json(&mut sample);
    let serialized = sample.to_string();
    assert!(!serialized.contains("admin"));
    assert!(!serialized.contains("developer"));
    assert!(!serialized.contains("C:\\\\Users"));
    assert!(!serialized.contains("/Users/"));
    assert!(!serialized.contains("D:\\\\Shared"));
    assert_eq!(
        sample["event"]["raw"]["params"]["filePath"]
            .as_str()
            .unwrap(),
        workspace_id_for_path("C:\\Users\\admin\\repo\\src\\main.rs")
    );
    assert_eq!(
        sample["event"]["raw"]["params"]["files"][0]
            .as_str()
            .unwrap(),
        workspace_id_for_path("/Users/developer/code/backend/server.go")
    );
    assert_eq!(
        sample["event"]["raw"]["params"]["files"][1]
            .as_str()
            .unwrap(),
        workspace_id_for_path("D:\\Shared\\docs\\spec.pdf")
    );
}

#[test]
fn sanitize_workspace_paths_in_json_preserves_full_sentences_with_embedded_paths() {
    let mut sample = serde_json::json!({
        "message": "Please review the code in /home/alice/project/src/lib.rs and tell me what you think.",
        "actions": [
            "run pytest in C:\\Users\\alice\\test",
            "normal text without path"
        ]
    });
    sanitize_workspace_paths_in_json(&mut sample);
    assert_eq!(
        sample["message"],
        "Please review the code in [local_path_redacted] and tell me what you think."
    );
    assert_eq!(sample["actions"][0], "run pytest in [local_path_redacted]");
    assert_eq!(sample["actions"][1], "normal text without path");
}

#[test]
fn redact_paths_inside_text_handles_equals_quotes_file_uri_single_level_and_unc() {
    assert_eq!(
        redact_paths_inside_text("--config=C:\\Users\\alice\\app.toml"),
        "--config=[local_path_redacted]"
    );
    assert_eq!(
        redact_paths_inside_text(r#"open "C:\Program Files\My App\secret.txt" failed"#),
        r#"open "[local_path_redacted]" failed"#
    );
    assert_eq!(
        redact_paths_inside_text("open file:///Users/alice/repo/src/main.rs now"),
        "open [local_path_redacted] now"
    );
    assert_eq!(
        redact_paths_inside_text("wrote /tmp and /etc"),
        "wrote [local_path_redacted] and [local_path_redacted]"
    );
    assert_eq!(
        redact_paths_inside_text(r"copy \\fileserver\team\secret.txt"),
        "copy [local_path_redacted]"
    );
}

#[test]
fn redact_paths_inside_text_handles_markdown_inline_code_and_braces() {
    assert_eq!(
        redact_paths_inside_text("inspect `/Users/alice/secret/config.toml` first"),
        "inspect `[local_path_redacted]` first"
    );
    assert_eq!(
        redact_paths_inside_text("open `/Users/alice/My App/secret.txt` failed"),
        "open `[local_path_redacted]` failed"
    );
    assert_eq!(
        redact_paths_inside_text("path {/home/bob/private/key.pem} rejected"),
        "path {[local_path_redacted]} rejected"
    );
}

#[test]
fn workspace_display_path_hides_username_and_stays_safe() {
    let win_home =
        workspace_display_path_with_home(r"C:\Users\alice\Desktop", Some(r"C:\Users\alice"));
    assert_eq!(win_home, "~/Desktop");
    assert!(!is_absolute_filesystem_path(&win_home));
    assert!(!contains_absolute_filesystem_path(&win_home));

    let win_drive = workspace_display_path_with_home(
        r"\\?\D:\ClaudeSpace\Project\Agent-Nexus",
        Some(r"C:\Users\alice"),
    );
    assert_eq!(win_drive, "D: ClaudeSpace/Project/Agent-Nexus");
    assert!(!is_absolute_filesystem_path(&win_drive));
    assert!(!contains_absolute_filesystem_path(&win_drive));

    let unix_server =
        workspace_display_path_with_home("/www/workspace/Project/agenthub", Some("/home/alice"));
    assert_eq!(unix_server, "www/workspace/Project/agenthub");
    assert!(!is_absolute_filesystem_path(&unix_server));
    assert!(!contains_absolute_filesystem_path(&unix_server));

    let unix_home = workspace_display_path_with_home("/home/bob/work/app", None);
    assert_eq!(unix_home, "~/work/app");
    assert!(!unix_home.contains("bob"));
}
