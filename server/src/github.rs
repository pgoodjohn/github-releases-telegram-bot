use serde::Deserialize;

#[derive(Deserialize, Debug)]
struct ReleaseResponse {
    tag_name: String,
}

#[derive(Deserialize)]
struct TagResponse {
    name: String,
}

fn github_api_base() -> String {
    std::env::var("GITHUB_API_BASE").unwrap_or_else(|_| "https://api.github.com".to_string())
}

pub(crate) async fn fetch_latest_release_tag_with_base(
    client: &reqwest::Client,
    owner: &str,
    repo: &str,
    token: Option<&str>,
    base: &str,
) -> Result<Option<String>, Box<dyn std::error::Error + Send + Sync>> {
    let release_url = format!("{}/repos/{}/{}/releases/latest", base, owner, repo);

    let mut req = client
        .get(release_url)
        .header("User-Agent", "github-release-bot/0.1")
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28");
    if let Some(t) = token {
        req = req.bearer_auth(t);
    }
    let resp = req.send().await?;

    if resp.status().is_success() {
        let release: ReleaseResponse = resp.json().await?;
        log::debug!("Latest release for {owner}/{repo} is {release:?}");

        if release.tag_name.is_empty() {
            log::debug!("Latest release for {owner}/{repo} is empty");
            return Ok(None);
        }

        return Ok(Some(release.tag_name));
    } else if resp.status().as_u16() == 404 {
        // Fallback: try tags
        let tags_url = format!("{}/repos/{}/{}/tags?per_page=1", base, owner, repo);
        let mut req = client
            .get(tags_url)
            .header("User-Agent", "github-release-bot/0.1")
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28");
        if let Some(t) = token {
            req = req.bearer_auth(t);
        }
        let resp = req.send().await?;
        if resp.status().is_success() {
            let tags: Vec<TagResponse> = resp.json().await?;
            if let Some(first) = tags.into_iter().next() {
                return Ok(Some(first.name));
            }
        }
        return Ok(None);
    }

    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    log::warn!(
        "GitHub releases request failed for {owner}/{repo}: status={} body={}",
        status,
        body
    );
    Err("GitHub API returned non-success status".into())
}

pub async fn fetch_latest_release_tag(
    client: &reqwest::Client,
    owner: &str,
    repo: &str,
    token: Option<&str>,
) -> Result<Option<String>, Box<dyn std::error::Error + Send + Sync>> {
    let base = github_api_base();
    fetch_latest_release_tag_with_base(client, owner, repo, token, &base).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::{Matcher, Server};

    fn client() -> reqwest::Client {
        reqwest::Client::new()
    }

    #[tokio::test]
    async fn latest_release_success() {
        let mut server = Server::new_async().await;
        let _m = server
            .mock("GET", "/repos/owner/repo/releases/latest")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(serde_json::json!({"tag_name":"v1.2.3"}).to_string())
            .create_async()
            .await;

        let tag =
            fetch_latest_release_tag_with_base(&client(), "owner", "repo", None, &server.url())
                .await
                .expect("ok");

        assert_eq!(tag, Some("v1.2.3".to_string()));
    }

    #[tokio::test]
    async fn latest_release_empty_tag_returns_none() {
        let mut server = Server::new_async().await;
        let _m = server
            .mock("GET", "/repos/owner/repo/releases/latest")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(serde_json::json!({"tag_name":""}).to_string())
            .create_async()
            .await;

        let tag =
            fetch_latest_release_tag_with_base(&client(), "owner", "repo", None, &server.url())
                .await
                .expect("ok");

        assert_eq!(tag, None);
    }

    #[tokio::test]
    async fn fallback_to_tags_on_404_success() {
        let mut server = Server::new_async().await;
        let _m1 = server
            .mock("GET", "/repos/owner/repo/releases/latest")
            .with_status(404)
            .create_async()
            .await;

        let _m2 = server
            .mock("GET", Matcher::Exact("/repos/owner/repo/tags".to_string()))
            .match_query(Matcher::UrlEncoded("per_page".into(), "1".into()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(serde_json::json!([{ "name": "v0.9.0" }]).to_string())
            .create_async()
            .await;

        let tag =
            fetch_latest_release_tag_with_base(&client(), "owner", "repo", None, &server.url())
                .await
                .expect("ok");

        assert_eq!(tag, Some("v0.9.0".to_string()));
    }

    #[tokio::test]
    async fn fallback_to_tags_empty_returns_none() {
        let mut server = Server::new_async().await;
        let _m1 = server
            .mock("GET", "/repos/owner/repo/releases/latest")
            .with_status(404)
            .create_async()
            .await;

        let _m2 = server
            .mock("GET", Matcher::Exact("/repos/owner/repo/tags".to_string()))
            .match_query(Matcher::UrlEncoded("per_page".into(), "1".into()))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(serde_json::json!([]).to_string())
            .create_async()
            .await;

        let tag =
            fetch_latest_release_tag_with_base(&client(), "owner", "repo", None, &server.url())
                .await
                .expect("ok");

        assert_eq!(tag, None);
    }

    #[tokio::test]
    async fn non_success_errors() {
        let mut server = Server::new_async().await;
        let _m = server
            .mock("GET", "/repos/owner/repo/releases/latest")
            .with_status(500)
            .with_body("err")
            .create_async()
            .await;

        let res =
            fetch_latest_release_tag_with_base(&client(), "owner", "repo", None, &server.url())
                .await;
        assert!(res.is_err());
    }
}
