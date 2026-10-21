/// The card border colours a GM can pick ('' = the default border). Must match the CHECK in the
/// `campaign_card_style` migration and the `.accent--*` classes in style.css.
pub const ACCENTS: [&str; 10] = [
    "red", "orange", "amber", "green", "teal", "blue", "indigo", "purple", "pink", "slate",
];

#[derive(serde::Deserialize, serde::Serialize)]
pub struct CampaignForm {
    pub name: String,
    pub system: String,
    pub tagline: String,
    pub description_md: String,
    #[serde(default)]
    pub accent: String,
}

impl CampaignForm {
    pub fn trim(&mut self) {
        self.name = self.name.trim().to_string();
        self.system = self.system.trim().to_string();
        self.tagline = self.tagline.trim().to_string();
        self.description_md = self.description_md.trim().to_string();
    }

    pub fn error(&self) -> Option<&'static str> {
        if self.name.is_empty() || self.system.is_empty() {
            Some("Fill campaign name and rule system!")
        } else if !self.accent.is_empty() && !ACCENTS.contains(&self.accent.as_str()) {
            Some("Pick one of the colours.")
        } else {
            None
        }
    }
}

#[derive(serde::Serialize)]
pub struct CampaignSummary {
    pub id: i64,
    pub name: String,
    pub system: String,
    pub tagline: String,
    pub is_gm: bool,
    pub accent: String,
    /// The banner's `updated_at`, for cache busting (`?v=`); `None` without a banner
    pub banner_version: Option<String>,
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
    pub accent: String,
    pub banner_version: Option<String>,
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
    /// Members, and site admins (who can look at any campaign).
    pub fn can_view(&self) -> bool {
        self.is_admin || self.role.is_some()
    }

    /// Running the campaign (edit, invites, polls, attendance, …) is the GM's alone. A site admin
    /// who isn't the GM can't, but can take the GM role over (`take_gm`) or delete the campaign.
    pub fn can_manage(&self) -> bool {
        matches!(self.role, Some(Role::Gm))
    }
}

#[derive(serde::Serialize)]
pub struct Member {
    pub user_id: i64,
    pub username: String,
    pub display_name: String,
    pub joined_at: String,
    pub deactivated: bool,
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

pub struct Banner {
    pub content_type: String,
    pub data: Vec<u8>,
}

/// Banners larger than this are refused (they're shown as a small strip).
pub const MAX_BANNER_BYTES: usize = 1024 * 1024;

/// The image type, from the file's first bytes. The browser-sent content type and file name
/// aren't trusted; anything that isn't PNG, JPEG or WebP (e.g. SVG, which can carry scripts) is
/// refused.
pub fn sniff_image(data: &[u8]) -> Option<&'static str> {
    if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if data.len() >= 12 && &data[0..4] == b"RIFF" && &data[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sniffs_allowed_images_only() {
        assert_eq!(sniff_image(b"\x89PNG\r\n\x1a\nrest"), Some("image/png"));
        assert_eq!(sniff_image(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("image/jpeg"));
        assert_eq!(sniff_image(b"RIFF\0\0\0\0WEBPVP8 "), Some("image/webp"));
        assert_eq!(
            sniff_image(b"<svg xmlns=\"http://www.w3.org/2000/svg\">"),
            None
        );
        assert_eq!(sniff_image(b"GIF89a"), None);
        assert_eq!(sniff_image(b""), None);
    }
}
