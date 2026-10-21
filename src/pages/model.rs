#[derive(serde::Serialize)]
pub struct PageSummary {
    pub title: String,
    pub slug: String,
}

#[derive(serde::Serialize)]
pub struct Page {
    pub id: i64,
    pub title: String,
    pub slug: String,
    pub body_md: String,
    pub updated_at: String,
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct PageForm {
    pub title: String,
    pub body_md: String,
}

impl PageForm {
    pub fn trim(&mut self) {
        self.title = self.title.trim().to_string();
        self.body_md = self.body_md.trim().to_string();
    }

    pub fn error(&self) -> Option<&'static str> {
        if self.title.is_empty() {
            Some("Fill in a title.")
        } else if self.title.chars().count() > 100 {
            Some("The title can be at most 100 characters long.")
        } else {
            None
        }
    }
}
