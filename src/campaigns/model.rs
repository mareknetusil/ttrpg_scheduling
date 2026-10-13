#[derive(serde::Deserialize, serde::Serialize)]
pub struct CampaignForm {
    pub name: String,
    pub system: String,
    pub tagline: String,
    pub description_md: String,
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
