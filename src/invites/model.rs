#[derive(serde::Serialize)]
pub struct InviteInfo {
    pub campaign_id: i64,
    pub campaign_name: String,
    pub gm_name: String,
}

pub enum JoinOutcome {
    Joined(i64),
    AlreadyMember(i64),
    InvalidInvite,
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct RegisterForm {
    pub username: String,
    pub display_name: String,
    #[serde(skip_serializing)]
    pub password: String,
    #[serde(skip_serializing)]
    pub password_confirm: String,
}

impl RegisterForm {
    pub fn trim(&mut self) {
        self.username = self.username.trim().to_string();
        self.display_name = self.display_name.trim().to_string();
    }

    pub fn error(&self) -> Option<&'static str> {
        let username_len = self.username.chars().count();
        if !(3..=32).contains(&username_len) {
            Some("The username must be 3 to 32 characters long.")
        } else if !self
            .username
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            Some("The username may contain only letters, digits, - and _.")
        } else if self.display_name.is_empty() || self.display_name.chars().count() > 50 {
            Some("Fill in a display name (at most 50 characters).")
        } else if self.password.chars().count() < 8 {
            Some("The password must be at least 8 characters long.")
        } else if self.password != self.password_confirm {
            Some("The passwords don't match.")
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form(username: &str, password: &str, confirm: &str) -> RegisterForm {
        RegisterForm {
            username: username.to_string(),
            display_name: "Bob".to_string(),
            password: password.to_string(),
            password_confirm: confirm.to_string(),
        }
    }

    #[test]
    fn register_form_validation() {
        assert!(form("bob_1-x", "password", "password").error().is_none());
        assert!(form("bo", "password", "password").error().is_some());
        assert!(form("bob smith", "password", "password").error().is_some());
        assert!(form("bob", "short", "short").error().is_some());
        assert!(form("bob", "password", "passw0rd").error().is_some());
    }
}
