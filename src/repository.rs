use url::Url;

/// A normalized github.com repository identity, compared case-insensitively.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct GithubRepo {
    owner: String,
    name: String,
}

impl GithubRepo {
    /// Accept web/clone URLs and `git@github.com:owner/repo.git` forms.
    /// Query strings, fragments and paths inside a repository are discarded.
    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim().strip_prefix("git+").unwrap_or(value.trim());
        let expanded = value
            .split_once(':')
            .filter(|(host, _)| host.eq_ignore_ascii_case("git@github.com"))
            .map(|(_, path)| format!("ssh://git@github.com/{path}"));
        let url = Url::parse(expanded.as_deref().unwrap_or(value)).ok()?;
        if !matches!(url.scheme(), "https" | "http" | "ssh" | "git")
            || !url.host_str()?.eq_ignore_ascii_case("github.com")
            || url.port().is_some()
            || url.password().is_some()
            || (!url.username().is_empty() && !(url.scheme() == "ssh" && url.username() == "git"))
        {
            return None;
        }
        let mut segments = url.path_segments()?;
        let owner = segments.next()?.to_ascii_lowercase();
        let name = segments.next()?.to_ascii_lowercase();
        let name = name.strip_suffix(".git").unwrap_or(&name).to_owned();
        if owner.is_empty()
            || !owner
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-')
            || name.is_empty()
            || matches!(name.as_str(), "." | "..")
            || !name
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"-._".contains(&c))
        {
            return None;
        }
        Some(Self { owner, name })
    }

    pub fn owner(&self) -> &str {
        &self.owner
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn url(&self) -> String {
        format!("https://github.com/{}/{}", self.owner, self.name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_clone_web_and_subdirectory_urls() {
        for url in [
            "https://GitHub.com/Owner/Repo.git/",
            "git@github.com:Owner/Repo.git",
            "git@GITHUB.COM:Owner/Repo.git",
            "git+https://github.com/Owner/Repo?rev=abc#def",
            "ssh://git@github.com/Owner/Repo.git",
            "git://github.com/Owner/Repo.git",
            "https://github.com/Owner/Repo/tree/main/crates/foo",
        ] {
            let repo = GithubRepo::parse(url).unwrap();
            assert_eq!(repo.url(), "https://github.com/owner/repo", "{url}");
        }
    }

    #[test]
    fn rejects_non_repository_urls_and_untrusted_hosts() {
        for url in [
            "https://gitlab.com/owner/repo",
            "https://github.com.evil.example/owner/repo",
            "https://github.com/owner",
            "https://github.com//repo",
            "https://github.com/owner/",
            "https://github.com/owner/.git",
            "https://github.com/owner/repo%2Fother",
            "https://user:secret@github.com/owner/repo",
            "https://github.com:8080/owner/repo",
            "file:///owner/repo",
            "X:/projects/repo",
        ] {
            assert!(GithubRepo::parse(url).is_none(), "{url}");
        }
    }
}
