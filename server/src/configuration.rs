#[derive(Clone)]
pub struct Configuration {
    pub database_path: String,
    pub teloxide_token: String,
    pub interval_secs: u64,
    pub github_token: Option<String>,
}

impl Configuration {
    fn resolve_secret_value(key: &str, value: String) -> Result<String, String> {
        const SECRET_PREFIX: &str = "secret:";
        if let Some(rest) = value.strip_prefix(SECRET_PREFIX) {
            log::debug!("Resolving secret value for {}", key);
            let path = rest.trim();
            if path.is_empty() {
                return Err(format!(
                    "{} is using 'secret:' prefix but no file path was provided",
                    key
                ));
            }
            let content = std::fs::read_to_string(path).map_err(|e| {
                format!(
                    "Failed to read secret file for {} from '{}': {}",
                    key, path, e
                )
            })?;
            let content = content.trim_end_matches(&['\n', '\r'][..]).to_string();
            log::debug!("Resolved secret value for {} to {}", key, content);
            Ok(content)
        } else {
            Ok(value)
        }
    }

    fn resolve_env_or_panic(key: &str) -> String {
        let raw = std::env::var(key)
            .unwrap_or_else(|_| panic!("{} environment variable is required", key));
        Self::resolve_secret_value(key, raw).unwrap_or_else(|e| panic!("{}", e))
    }

    pub fn from_env() -> Self {
        let database_path = Self::resolve_env_or_panic("DATABASE_PATH");
        let teloxide_token = Self::resolve_env_or_panic("TELOXIDE_TOKEN");

        let interval_secs = match std::env::var("POLL_INTERVAL_SECS") {
            Ok(raw) => {
                let resolved = Self::resolve_secret_value("POLL_INTERVAL_SECS", raw)
                    .unwrap_or_else(|e| panic!("{}", e));
                resolved.trim().parse::<u64>().unwrap_or_else(|e| {
                    panic!("POLL_INTERVAL_SECS must be a positive integer: {}", e)
                })
            }
            Err(_) => 60,
        };

        let github_token = match std::env::var("GITHUB_TOKEN") {
            Ok(raw) => {
                let resolved = Self::resolve_secret_value("GITHUB_TOKEN", raw)
                    .unwrap_or_else(|e| panic!("{}", e));
                Some(resolved)
            }
            Err(_) => None,
        };

        Self {
            database_path,
            teloxide_token,
            interval_secs,
            github_token,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use uuid::Uuid;

    fn write_temp_file_with_contents(contents: &str) -> String {
        let mut path = std::env::temp_dir();
        path.push(format!("github_release_bot_test_{}", Uuid::new_v4()));
        fs::write(&path, contents).expect("failed to write temp file");
        path.to_string_lossy().into_owned()
    }

    fn save_env_var(key: &str) -> Option<String> {
        std::env::var(key).ok()
    }

    fn restore_env_var(key: &str, previous: Option<String>) {
        match previous {
            Some(value) => unsafe { std::env::set_var(key, value) },
            None => unsafe { std::env::remove_var(key) },
        }
    }

    #[test]
    fn from_env_reads_secret_files_successfully() {
        let prev_db = save_env_var("DATABASE_PATH");
        let prev_token = save_env_var("TELOXIDE_TOKEN");
        let prev_interval = save_env_var("POLL_INTERVAL_SECS");
        let prev_gh = save_env_var("GITHUB_TOKEN");

        let token_file = write_temp_file_with_contents("my-telegram-token\n");
        let interval_file = write_temp_file_with_contents("90\n");
        let gh_file = write_temp_file_with_contents("gh-secret-token");

        unsafe {
            std::env::set_var("DATABASE_PATH", "db-path.db");
            std::env::set_var("TELOXIDE_TOKEN", format!("secret:{}", token_file));
            std::env::set_var("POLL_INTERVAL_SECS", format!("secret:{}", interval_file));
            std::env::set_var("GITHUB_TOKEN", format!("secret:{}", gh_file));
        }

        let cfg = Configuration::from_env();

        assert_eq!(cfg.database_path, "db-path.db");
        assert_eq!(cfg.teloxide_token, "my-telegram-token");
        assert_eq!(cfg.interval_secs, 90);
        assert_eq!(cfg.github_token.as_deref(), Some("gh-secret-token"));

        let _ = fs::remove_file(&token_file);
        let _ = fs::remove_file(&interval_file);
        let _ = fs::remove_file(&gh_file);

        restore_env_var("DATABASE_PATH", prev_db);
        restore_env_var("TELOXIDE_TOKEN", prev_token);
        restore_env_var("POLL_INTERVAL_SECS", prev_interval);
        restore_env_var("GITHUB_TOKEN", prev_gh);
    }

    #[test]
    fn resolve_secret_value_requires_non_empty_path() {
        let err = Configuration::resolve_secret_value("SOME_KEY", "secret:".to_string())
            .expect_err("expected error for empty secret path");
        assert!(err.contains("no file path"));
    }
}
