#[derive(Clone)]
pub struct Configuration {
    pub database_path: String,
    pub teloxide_token: String,
    pub interval_secs: u64,
    pub github_token: Option<String>,
}

impl Configuration {
    pub fn from_env() -> Self {
        Self {
            database_path: std::env::var("DATABASE_PATH")
                .expect("DATABASE_PATH environment variable is required"),
            teloxide_token: std::env::var("TELOXIDE_TOKEN")
                .expect("TELOXIDE_TOKEN environment variable is required"),
            interval_secs: std::env::var("POLL_INTERVAL_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(60),
            github_token: std::env::var("GITHUB_TOKEN").ok(),
        }
    }
}
