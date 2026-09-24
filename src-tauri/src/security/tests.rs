#[cfg(test)]
#[allow(clippy::module_inception)]
pub mod tests {
    use crate::security::{constant_time_eq, SecurityManager};

    #[test]
    fn constant_time_eq_matches_correct_token() {
        assert!(constant_time_eq("axf_live_abc", "axf_live_abc"));
        assert!(constant_time_eq("", ""));
    }

    #[test]
    fn constant_time_eq_rejects_wrong_token_same_length() {
        assert!(!constant_time_eq("axf_live_abc", "axf_live_abd"));
        assert!(!constant_time_eq("axf_live_abc", "bxg_live_abc"));
    }

    #[test]
    fn constant_time_eq_rejects_wrong_length() {
        assert!(!constant_time_eq("axf_live_abc", "axf_live_abcd"));
        assert!(!constant_time_eq("axf_live_abcd", "axf_live_abc"));
        assert!(!constant_time_eq("", "a"));
    }

    #[test]
    fn validate_token_accepts_only_exact_token() {
        let manager = SecurityManager::new_with_token("axf_live_secret".to_string());
        assert!(manager.validate_token("axf_live_secret"));
        assert!(!manager.validate_token("axf_live_secre"));
        assert!(!manager.validate_token("axf_live_secret2"));
    }
}
