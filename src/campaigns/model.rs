#[derive(serde::Deserialize, serde::Serialize)]
pub struct CampaignForm {
    pub name: String,
    pub system: String,
    pub tagline: String,
    pub description_md: String,
}
