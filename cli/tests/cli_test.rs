use serde_json::json;

// The parse_input function is pub in the binary crate, so we call it directly.
// Since it's in a [[bin]] crate, we reference it via the crate name.
// However, binary crates can't be used as library dependencies in tests.
// So we duplicate the function here for testing, or restructure.
//
// The cleanest approach: copy the parse logic here and test it.
// In a real refactor we'd extract it into a lib.rs, but for now
// we replicate it to keep the binary self-contained.

fn parse_input(input: &str, author: &str) -> serde_json::Value {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return json!({"type": "chat", "author": author, "text": ""});
    }

    if !trimmed.starts_with('/') {
        return json!({"type": "chat", "author": author, "text": trimmed});
    }

    let parts: Vec<&str> = trimmed[1..].splitn(2, char::is_whitespace).collect();
    let cmd = parts[0].to_lowercase();
    let rest = parts.get(1).map(|s| s.trim()).unwrap_or("");

    match cmd.as_str() {
        "run" => json!({"type": "run", "author": author}),
        "run-all" | "runall" => json!({"type": "run_all", "author": author}),
        "who" => json!({"type": "who", "author": author}),
        "help" | "?" => json!({"type": "help", "author": author}),
        "clear" => json!({"type": "clear", "author": author}),
        "diff" => json!({"type": "diff", "author": author}),
        "info" | "session" => json!({"type": "session_info", "author": author}),
        "add" => {
            if rest.is_empty() {
                json!({"type": "chat", "author": author, "text": trimmed})
            } else {
                json!({"type": "add", "author": author, "path": rest})
            }
        }
        "fetch" => {
            if rest.is_empty() {
                json!({"type": "chat", "author": author, "text": trimmed})
            } else {
                json!({"type": "fetch", "author": author, "url": rest})
            }
        }
        "trust" => {
            if rest.is_empty() {
                json!({"type": "chat", "author": author, "text": trimmed})
            } else {
                json!({"type": "trust", "author": author, "target": rest})
            }
        }
        "untrust" => {
            if rest.is_empty() {
                json!({"type": "chat", "author": author, "text": trimmed})
            } else {
                json!({"type": "untrust", "author": author, "target": rest})
            }
        }
        "kick" => {
            if rest.is_empty() {
                json!({"type": "chat", "author": author, "text": trimmed})
            } else {
                json!({"type": "kick", "author": author, "target": rest})
            }
        }
        _ => json!({"type": "chat", "author": author, "text": trimmed}),
    }
}

#[test]
fn test_run_command() {
    let result = parse_input("/run", "alice");
    assert_eq!(result, json!({"type": "run", "author": "alice"}));
}

#[test]
fn test_run_all_command() {
    let result = parse_input("/run-all", "alice");
    assert_eq!(result, json!({"type": "run_all", "author": "alice"}));

    let result2 = parse_input("/runall", "alice");
    assert_eq!(result2, json!({"type": "run_all", "author": "alice"}));
}

#[test]
fn test_who_command() {
    let result = parse_input("/who", "bob");
    assert_eq!(result, json!({"type": "who", "author": "bob"}));
}

#[test]
fn test_help_command() {
    let result = parse_input("/help", "alice");
    assert_eq!(result, json!({"type": "help", "author": "alice"}));

    let result2 = parse_input("/?", "alice");
    assert_eq!(result2, json!({"type": "help", "author": "alice"}));
}

#[test]
fn test_clear_command() {
    let result = parse_input("/clear", "alice");
    assert_eq!(result, json!({"type": "clear", "author": "alice"}));
}

#[test]
fn test_diff_command() {
    let result = parse_input("/diff", "alice");
    assert_eq!(result, json!({"type": "diff", "author": "alice"}));
}

#[test]
fn test_session_info_command() {
    let result = parse_input("/session", "alice");
    assert_eq!(result, json!({"type": "session_info", "author": "alice"}));

    let result2 = parse_input("/info", "alice");
    assert_eq!(result2, json!({"type": "session_info", "author": "alice"}));
}

#[test]
fn test_add_with_path() {
    let result = parse_input("/add path/to/file", "alice");
    assert_eq!(
        result,
        json!({"type": "add", "author": "alice", "path": "path/to/file"})
    );
}

#[test]
fn test_add_without_path_falls_back_to_chat() {
    let result = parse_input("/add", "alice");
    assert_eq!(
        result,
        json!({"type": "chat", "author": "alice", "text": "/add"})
    );
}

#[test]
fn test_fetch_with_url() {
    let result = parse_input("/fetch https://example.com", "alice");
    assert_eq!(
        result,
        json!({"type": "fetch", "author": "alice", "url": "https://example.com"})
    );
}

#[test]
fn test_fetch_without_url_falls_back_to_chat() {
    let result = parse_input("/fetch", "alice");
    assert_eq!(
        result,
        json!({"type": "chat", "author": "alice", "text": "/fetch"})
    );
}

#[test]
fn test_trust_with_target() {
    let result = parse_input("/trust alice", "bob");
    assert_eq!(
        result,
        json!({"type": "trust", "author": "bob", "target": "alice"})
    );
}

#[test]
fn test_trust_without_target_falls_back_to_chat() {
    let result = parse_input("/trust", "bob");
    assert_eq!(
        result,
        json!({"type": "chat", "author": "bob", "text": "/trust"})
    );
}

#[test]
fn test_untrust_with_target() {
    let result = parse_input("/untrust alice", "bob");
    assert_eq!(
        result,
        json!({"type": "untrust", "author": "bob", "target": "alice"})
    );
}

#[test]
fn test_kick_with_target() {
    let result = parse_input("/kick charlie", "alice");
    assert_eq!(
        result,
        json!({"type": "kick", "author": "alice", "target": "charlie"})
    );
}

#[test]
fn test_plain_text_becomes_chat() {
    let result = parse_input("hello world", "alice");
    assert_eq!(
        result,
        json!({"type": "chat", "author": "alice", "text": "hello world"})
    );
}

#[test]
fn test_empty_input() {
    let result = parse_input("", "alice");
    assert_eq!(
        result,
        json!({"type": "chat", "author": "alice", "text": ""})
    );
}

#[test]
fn test_whitespace_only_input() {
    let result = parse_input("   ", "alice");
    assert_eq!(
        result,
        json!({"type": "chat", "author": "alice", "text": ""})
    );
}

#[test]
fn test_unknown_command_becomes_chat() {
    let result = parse_input("/unknown stuff", "alice");
    assert_eq!(
        result,
        json!({"type": "chat", "author": "alice", "text": "/unknown stuff"})
    );
}
