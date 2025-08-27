
#[derive(Clone)]
pub struct Configuration {
    pub database_path: String,
    pub teloxide_token: String,
}

impl Configuration {
    pub fn from_env() -> Self {
        Self {
            database_path: std::env::var("DATABASE_PATH").expect("DATABASE_PATH environment variable is required"),
            teloxide_token: std::env::var("TELOXIDE_TOKEN").expect("TELOXIDE_TOKEN environment variable is required"),
        }
    }
}
