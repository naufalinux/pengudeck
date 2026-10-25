pub const API_BASE: &str = "http://127.0.0.1:8787";

fn tasks_url(slug: &str) -> Option<String> {
    (!slug.is_empty() && slug.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-'))
        .then(|| format!("{API_BASE}/api/boards/{slug}/tasks"))
}

fn main() {}

#[cfg(test)]
mod tests {
    #[test]
    fn requests_use_fixed_loopback_and_only_valid_slugs() {
        assert_eq!(super::API_BASE, "http://127.0.0.1:8787");
        assert_eq!(super::tasks_url("team-A_1"), Some("http://127.0.0.1:8787/api/boards/team-A_1/tasks".into()));
        for slug in ["", "../secrets", "a/b", "a?b", "a\"b", "é", "a b"] {
            assert_eq!(super::tasks_url(slug), None);
        }
    }
}
