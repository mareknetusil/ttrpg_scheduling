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
        use crate::auth::validate;
        validate::username(&self.username)
            .or_else(|| validate::display_name(&self.display_name))
            .or_else(|| validate::password(&self.password))
            .or_else(|| {
                (self.password != self.password_confirm).then_some("The passwords don't match.")
            })
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
