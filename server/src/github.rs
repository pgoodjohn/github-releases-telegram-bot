use serde::Deserialize;

#[derive(Deserialize, Debug)]
struct ReleaseResponse {
    tag_name: String,
}

#[derive(Deserialize)]
struct TagResponse {
    name: String,
}

pub async fn fetch_latest_release_tag(
    client: &reqwest::Client,
    owner: &str,
    repo: &str,
    token: Option<&str>,
) -> Result<Option<String>, Box<dyn std::error::Error + Send + Sync>> {
    let base = "https://api.github.com";
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
    log::warn!("GitHub releases request failed for {owner}/{repo}: status={} body={}", status, body);
    Err("GitHub API returned non-success status".into())
}


