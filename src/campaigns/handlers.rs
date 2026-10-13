use crate::auth::extractor::CurrentUser;

pub async fn new_form(_user: CurrentUser) -> &'static str {
    "new campaign form goes here"
}
