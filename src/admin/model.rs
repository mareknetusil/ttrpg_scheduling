#[derive(serde::Serialize)]
pub struct UserRow {
    pub id: i64,
    pub username: String,
    pub display_name: String,
    pub is_admin: bool,
    pub deactivated: bool,
    pub created_at: String,
    pub campaign_count: i64,
}

#[derive(serde::Serialize)]
pub struct CampaignRow {
    pub id: i64,
    pub name: String,
    pub system: String,
    pub gm_name: String,
    pub member_count: i64,
    pub session_count: i64,
    pub created_at: String,
}

/// What deleting a campaign would remove, shown on the confirmation page.
#[derive(serde::Serialize)]
pub struct CampaignContents {
    pub members: i64,
    pub sessions: i64,
    pub pages: i64,
    pub polls: i64,
    pub comments: i64,
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct NewUserForm {
    pub username: String,
    pub display_name: String,
    #[serde(skip_serializing)]
    pub password: String,
}

impl NewUserForm {
    pub fn error(&mut self) -> Option<&'static str> {
        use crate::auth::validate;
        self.username = self.username.trim().to_string();
        self.display_name = self.display_name.trim().to_string();
        validate::username(&self.username)
            .or_else(|| validate::display_name(&self.display_name))
            .or_else(|| validate::password(&self.password))
    }
}

#[derive(serde::Deserialize)]
pub struct PasswordForm {
    pub password: String,
}

/// `PUT /admin/users/{id}/admin` and `…/active`: `value=1` or `value=0`.
#[derive(serde::Deserialize)]
pub struct FlagForm {
    pub value: u8,
}

/// `DELETE /admin/campaigns/{id}?confirm_name=…` (htmx sends DELETE parameters in the URL).
#[derive(serde::Deserialize)]
pub struct DeleteQuery {
    #[serde(default)]
    pub confirm_name: String,
}
