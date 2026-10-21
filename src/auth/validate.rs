//! Account rules shared by registration (invites) and admin-created accounts.

/// 3-32 characters: ASCII letters, digits, `-` and `_`.
pub fn username(username: &str) -> Option<&'static str> {
    if !(3..=32).contains(&username.chars().count()) {
        Some("The username must be 3 to 32 characters long.")
    } else if !username
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        Some("The username may contain only letters, digits, - and _.")
    } else {
        None
    }
}

pub fn display_name(display_name: &str) -> Option<&'static str> {
    (display_name.is_empty() || display_name.chars().count() > 50)
        .then_some("Fill in a display name (at most 50 characters).")
}

/// At least 8 characters (counted as characters, not bytes).
pub fn password(password: &str) -> Option<&'static str> {
    (password.chars().count() < 8).then_some("The password must be at least 8 characters long.")
}
