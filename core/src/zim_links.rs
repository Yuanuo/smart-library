//! URL classification shared by ZIM navigation and article rendering.

/// Accept only local URL references; decoded paths are checked again on use.
pub fn is_internal_href(href: &str) -> bool {
    let href = href.trim();
    if href.is_empty()
        || href.starts_with("//")
        || href.starts_with('\\')
        || href.contains('\\')
        || href.chars().any(char::is_control)
    {
        return false;
    }
    let first = href.split(['/', '?', '#']).next().unwrap_or("");
    if let Some((scheme, _)) = first.split_once(':') {
        if !scheme.is_empty()
            && scheme
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic())
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c))
        {
            return false;
        }
    }
    true
}

/// Accept HTTP(S) URLs only, without embedded credentials or control characters.
pub fn external_url(href: &str) -> Option<url::Url> {
    if href.chars().any(char::is_control) {
        return None;
    }
    let url = url::Url::parse(href).ok()?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return None;
    }
    Some(url)
}
