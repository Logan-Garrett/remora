//! Unit tests for `is_safe_git_url`.
//!
//! These are pure `#[test]` tests — no database, no server, no `#[ignore]`.

use remora_server::is_safe_git_url;

// ── Safe URLs ───────────────────────────────────────────────────────

#[test]
fn https_github_url_is_safe() {
    assert!(is_safe_git_url("https://github.com/user/repo.git"));
}

#[test]
fn ssh_style_github_url_is_safe() {
    assert!(is_safe_git_url("git@github.com:user/repo.git"));
}

#[test]
fn ssh_scheme_url_is_safe() {
    assert!(is_safe_git_url("ssh://git@github.com/user/repo.git"));
}

#[test]
fn git_scheme_url_is_safe() {
    assert!(is_safe_git_url("git://github.com/user/repo.git"));
}

#[test]
fn https_gitlab_url_is_safe() {
    assert!(is_safe_git_url("https://gitlab.com/org/project.git"));
}

#[test]
fn ssh_style_gitlab_url_is_safe() {
    assert!(is_safe_git_url("git@gitlab.com:org/project.git"));
}

// ── Unsafe URLs ─────────────────────────────────────────────────────

#[test]
fn file_scheme_is_not_safe() {
    assert!(!is_safe_git_url("file:///etc/passwd"));
}

#[test]
fn ftp_scheme_is_not_safe() {
    assert!(!is_safe_git_url("ftp://example.com/repo.git"));
}

#[test]
fn empty_string_is_not_safe() {
    assert!(!is_safe_git_url(""));
}

#[test]
fn relative_path_traversal_is_not_safe() {
    assert!(!is_safe_git_url("../../../etc/passwd"));
}

#[test]
fn absolute_path_is_not_safe() {
    assert!(!is_safe_git_url("/tmp/evil-repo"));
}

#[test]
fn http_scheme_without_s_is_not_safe() {
    // http:// (not https://) should not be in the safe list
    assert!(!is_safe_git_url("http://example.com/repo.git"));
}

#[test]
fn file_scheme_with_tmp_is_not_safe() {
    assert!(!is_safe_git_url("file:///tmp/evil-repo"));
}
