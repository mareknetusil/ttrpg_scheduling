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
