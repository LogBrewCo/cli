//! Shared privacy-safe product-analytics property-key contract.

/// Returns whether one version-1 property key is supported and privacy-safe.
#[must_use]
pub(crate) fn is_safe_key(value: &str) -> bool {
    STANDARD_PROPERTY_KEYS.contains(&value)
        || value
            .strip_prefix("tag.")
            .is_some_and(is_safe_custom_tag_key)
}

/// Returns whether one custom tag key is bounded and not identity or credential-like.
fn is_safe_custom_tag_key(value: &str) -> bool {
    let mut bytes = value.bytes();
    value.len() <= 64
        && bytes.next().is_some_and(|byte| byte.is_ascii_alphabetic())
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
        && !is_sensitive_tag_key(value)
}

/// Excludes direct identity, credential, and network-address property keys.
fn is_sensitive_tag_key(value: &str) -> bool {
    let sensitive_token = value
        .split(['.', '_', '-'])
        .map(str::to_ascii_lowercase)
        .any(|token| {
            matches!(
                token.as_str(),
                "email"
                    | "phone"
                    | "password"
                    | "secret"
                    | "token"
                    | "cookie"
                    | "auth"
                    | "authorization"
                    | "credential"
                    | "apikey"
                    | "accesskey"
                    | "bearer"
                    | "jwt"
                    | "ip"
            )
        });
    let compact = value
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|character| character.to_ascii_lowercase())
        .collect::<String>();
    sensitive_token
        || matches!(
            compact.as_str(),
            "userid"
                | "sessionid"
                | "distinctid"
                | "subjectid"
                | "traceid"
                | "ipaddress"
                | "emailaddress"
                | "phonenumber"
                | "apikey"
                | "accesskey"
        )
        || compact.ends_with("email")
        || compact.ends_with("phone")
}

/// Version-1 typed-context keys accepted by exact analytics filters.
const STANDARD_PROPERTY_KEYS: [&str; 14] = [
    "resource.service.version",
    "resource.runtime.name",
    "resource.runtime.version",
    "resource.framework.name",
    "resource.framework.version",
    "resource.operating_system.name",
    "resource.operating_system.version",
    "resource.operating_system.build",
    "resource.device.family",
    "resource.device.model",
    "resource.device.architecture",
    "resource.application.name",
    "resource.application.version",
    "resource.application.build",
];

#[cfg(test)]
mod tests {
    use super::is_safe_key;

    #[test]
    fn accepts_only_standard_or_non_sensitive_custom_keys() {
        assert!(is_safe_key("resource.framework.name"));
        assert!(is_safe_key("tag.plan"));
        assert!(!is_safe_key("resource.framework.unknown"));
        assert!(!is_safe_key("tag.user_id"));
        assert!(!is_safe_key("tag.api-token"));
        assert!(!is_safe_key("tag.1region"));
    }
}
