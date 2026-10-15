#[derive(serde::Deserialize, serde::Serialize)]
pub struct CampaignForm {
    pub name: String,
    pub system: String,
    pub tagline: String,
    pub description_md: String,
}

impl CampaignForm {
    pub fn trim(&mut self) {
        self.name = self.name.trim().to_string();
        self.system = self.system.trim().to_string();
        self.tagline = self.tagline.trim().to_string();
        self.description_md = self.description_md.trim().to_string();
    }

    pub fn error(&self) -> Option<&'static str> {
        (self.name.is_empty() || self.system.is_empty())
            .then_some("Fill campaign name and rule system!")
    }
}

#[derive(serde::Serialize)]
pub struct CampaignSummary {
    pub id: i64,
    pub name: String,
    pub system: String,
    pub tagline: String,
    pub is_gm: bool,
}

#[derive(serde::Serialize)]
pub struct Campaign {
    pub id: i64,
    pub name: String,
    pub system: String,
    pub tagline: String,
    pub description_md: String,
    pub gm_user_id: i64,
    pub gm_name: String,
}

pub enum Role {
    Gm,
    Member,
}

pub struct Access {
    pub role: Option<Role>,
    pub is_admin: bool,
}

impl Access {
    pub fn can_view(&self) -> bool {
        self.is_admin || self.role.is_some()
    }

    pub fn can_manage(&self) -> bool {
        self.is_admin || matches!(self.role, Some(Role::Gm))
    }
}

#[derive(serde::Serialize)]
pub struct Member {
    pub user_id: i64,
    pub username: String,
    pub display_name: String,
    pub joined_at: String,
}

#[derive(serde::Serialize)]
pub struct Invite {
    pub id: i64,
    pub token: String,
    pub expires_at: String,
    pub max_uses: i64,
    pub use_count: i64,
}

#[derive(serde::Deserialize)]
pub struct InviteForm {
    pub max_uses: i64,
}

#[derive(serde::Deserialize)]
pub struct GmForm {
    pub user_id: i64,
}
