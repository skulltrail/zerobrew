use crate::cache::{ApiCache, CacheEntry};
use zb_core::{Cask, Error, Formula};

pub struct ApiClient {
    formula_base_url: String,
    cask_base_url: String,
    client: reqwest::Client,
    cache: Option<ApiCache>,
}

impl ApiClient {
    /// Creates an ApiClient configured with the official Homebrew formula and cask API endpoints.
    ///
    /// # Examples
    ///
    /// ```
    /// let client = ApiClient::new();
    /// assert!(client.formula_base_url.ends_with("/api/formula"));
    /// assert!(client.cask_base_url.ends_with("/api/cask"));
    /// ```
    pub fn new() -> Self {
        Self::with_base_urls(
            "https://formulae.brew.sh/api/formula".to_string(),
            "https://formulae.brew.sh/api/cask".to_string(),
        )
    }

    /// Creates an ApiClient using the provided formula base URL and derives the cask base URL by replacing "/formula" with "/cask".
    ///
    /// The derived cask base URL is produced by performing a literal replacement of the first occurrence of `/formula` with `/cask` in `formula_base_url`.
    ///
    /// # Examples
    ///
    /// ```
    /// let client = ApiClient::with_base_url("https://formulae.brew.sh/api/formula".to_string());
    /// assert!(client.formula_base_url.contains("/formula"));
    /// assert!(client.cask_base_url.contains("/cask"));
    /// ```
    pub fn with_base_url(formula_base_url: String) -> Self {
        // For backward compatibility, derive cask URL from formula URL
        let cask_base_url = formula_base_url.replace("/formula", "/cask");
        Self::with_base_urls(formula_base_url, cask_base_url)
    }

    /// Creates an ApiClient configured with explicit formula and cask base URLs.
    ///
    /// The returned client uses a reqwest client configured with a user agent and
    /// a connection pool optimized for multiplexing parallel requests.
    ///
    /// # Parameters
    ///
    /// - `formula_base_url`: Base URL for formula endpoints (e.g. "https://example/api/formula").
    /// - `cask_base_url`: Base URL for cask endpoints (e.g. "https://example/api/cask").
    ///
    /// # Returns
    ///
    /// An ApiClient instance configured with the provided base URLs and no cache.
    ///
    /// # Examples
    ///
    /// ```
    /// let client = ApiClient::with_base_urls(
    ///     "https://formulae.brew.sh/api/formula".to_string(),
    ///     "https://formulae.brew.sh/api/cask".to_string(),
    /// );
    /// assert_eq!(client.formula_base_url, "https://formulae.brew.sh/api/formula");
    /// assert_eq!(client.cask_base_url, "https://formulae.brew.sh/api/cask");
    /// ```
    pub fn with_base_urls(formula_base_url: String, cask_base_url: String) -> Self {
        // Use HTTP/2 with connection pooling for better multiplexing of parallel requests
        let client = reqwest::Client::builder()
            .user_agent("zerobrew/0.1")
            .pool_max_idle_per_host(20)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());

        Self {
            formula_base_url,
            cask_base_url,
            client,
            cache: None,
        }
    }

    /// Enable in-memory response caching for the client.
    ///
    /// Returns the modified client to allow method chaining.
    ///
    /// # Parameters
    ///
    /// - `cache`: An ApiCache instance to store cached responses (ETag, Last-Modified and body).
    ///
    /// # Examples
    ///
    /// ```
    /// let cache = ApiCache::new();
    /// let client = ApiClient::new().with_cache(cache);
    /// ```
    pub fn with_cache(mut self, cache: ApiCache) -> Self {
        self.cache = Some(cache);
        self
    }

    /// Fetches a Formula by name from the configured formula API, using cached responses and HTTP conditional requests when available.
    ///
    /// If a cached entry exists the client will send `If-None-Match` and/or `If-Modified-Since` headers and will return the cached body when the server responds with `304 Not Modified`.
    ///
    /// # Returns
    ///
    /// `Ok(Formula)` parsed from the fetched (or cached on `304`) JSON; `Err(Error)` if the formula is missing (`404`), if the response has a non-success status, or if a network or JSON parse error occurs.
    ///
    /// # Examples
    ///
    /// ```
    /// # use zb_api::ApiClient;
    /// # async fn doc_example() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = ApiClient::new();
    /// let formula = client.get_formula("wget").await?;
    /// println!("{}", formula.name);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn get_formula(&self, name: &str) -> Result<Formula, Error> {
        let url = format!("{}/{}.json", self.formula_base_url, name);

        let cached_entry = self.cache.as_ref().and_then(|c| c.get(&url));

        let mut request = self.client.get(&url);

        if let Some(ref entry) = cached_entry {
            if let Some(ref etag) = entry.etag {
                request = request.header("If-None-Match", etag.as_str());
            }
            if let Some(ref last_modified) = entry.last_modified {
                request = request.header("If-Modified-Since", last_modified.as_str());
            }
        }

        let response = request.send().await.map_err(|e| Error::NetworkFailure {
            message: e.to_string(),
        })?;

        if response.status() == reqwest::StatusCode::NOT_MODIFIED
            && let Some(entry) = cached_entry
        {
            let formula: Formula =
                serde_json::from_str(&entry.body).map_err(|e| Error::NetworkFailure {
                    message: format!("failed to parse cached formula JSON: {e}"),
                })?;
            return Ok(formula);
        }

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(Error::MissingFormula {
                name: name.to_string(),
            });
        }

        if !response.status().is_success() {
            return Err(Error::NetworkFailure {
                message: format!("HTTP {}", response.status()),
            });
        }

        let etag = response
            .headers()
            .get("etag")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        let last_modified = response
            .headers()
            .get("last-modified")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        let body = response.text().await.map_err(|e| Error::NetworkFailure {
            message: format!("failed to read response body: {e}"),
        })?;

        if let Some(ref cache) = self.cache {
            let entry = CacheEntry {
                etag,
                last_modified,
                body: body.clone(),
            };
            let _ = cache.put(&url, &entry);
        }

        let formula: Formula = serde_json::from_str(&body).map_err(|e| Error::NetworkFailure {
            message: format!("failed to parse formula JSON: {e}"),
        })?;

        Ok(formula)
    }

    /// Fetches a cask by name from the configured cask API, using conditional requests and optional caching.
    ///
    /// This will perform an HTTP GET to "{cask_base_url}/{name}.json", attach `If-None-Match` / `If-Modified-Since` headers when a cached entry exists, and update the cache with `etag`, `last-modified`, and body when a fresh response is returned.
    ///
    /// # Returns
    ///
    /// `Cask` parsed from the response body on success. Returns `Error::MissingCask` if the server responds with 404, and `Error::NetworkFailure` for network, HTTP (non-success) or JSON parse errors.
    ///
    /// # Examples
    ///
    /// ```
    /// # use crate::ApiClient;
    /// # fn run() {
    /// #   let _ = futures::executor::block_on(async {
    /// let client = ApiClient::new();
    /// let _ = client.get_cask("example-cask").await;
    /// #   });
    /// # }
    /// ```
    pub async fn get_cask(&self, name: &str) -> Result<Cask, Error> {
        let url = format!("{}/{}.json", self.cask_base_url, name);

        let cached_entry = self.cache.as_ref().and_then(|c| c.get(&url));

        let mut request = self.client.get(&url);

        if let Some(ref entry) = cached_entry {
            if let Some(ref etag) = entry.etag {
                request = request.header("If-None-Match", etag.as_str());
            }
            if let Some(ref last_modified) = entry.last_modified {
                request = request.header("If-Modified-Since", last_modified.as_str());
            }
        }

        let response = request.send().await.map_err(|e| Error::NetworkFailure {
            message: e.to_string(),
        })?;

        if response.status() == reqwest::StatusCode::NOT_MODIFIED
            && let Some(entry) = cached_entry
        {
            let cask: Cask =
                serde_json::from_str(&entry.body).map_err(|e| Error::NetworkFailure {
                    message: format!("failed to parse cached cask JSON: {e}"),
                })?;
            return Ok(cask);
        }

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(Error::MissingCask {
                name: name.to_string(),
            });
        }

        if !response.status().is_success() {
            return Err(Error::NetworkFailure {
                message: format!("HTTP {}", response.status()),
            });
        }

        let etag = response
            .headers()
            .get("etag")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        let last_modified = response
            .headers()
            .get("last-modified")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());

        let body = response.text().await.map_err(|e| Error::NetworkFailure {
            message: format!("failed to read response body: {e}"),
        })?;

        if let Some(ref cache) = self.cache {
            let entry = CacheEntry {
                etag,
                last_modified,
                body: body.clone(),
            };
            let _ = cache.put(&url, &entry);
        }

        let cask: Cask = serde_json::from_str(&body).map_err(|e| Error::NetworkFailure {
            message: format!("failed to parse cask JSON: {e}"),
        })?;

        Ok(cask)
    }
}

impl Default for ApiClient {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn fetches_formula_from_mock_server() {
        let mock_server = MockServer::start().await;

        let fixture = include_str!("../../zb_core/fixtures/formula_foo.json");

        Mock::given(method("GET"))
            .and(path("/foo.json"))
            .respond_with(ResponseTemplate::new(200).set_body_string(fixture))
            .mount(&mock_server)
            .await;

        let client = ApiClient::with_base_url(mock_server.uri());
        let formula = client.get_formula("foo").await.unwrap();

        assert_eq!(formula.name, "foo");
        assert_eq!(formula.versions.stable, "1.2.3");
    }

    #[tokio::test]
    async fn returns_missing_formula_on_404() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/nonexistent.json"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&mock_server)
            .await;

        let client = ApiClient::with_base_url(mock_server.uri());
        let err = client.get_formula("nonexistent").await.unwrap_err();

        assert!(matches!(
            err,
            Error::MissingFormula { name } if name == "nonexistent"
        ));
    }

    #[tokio::test]
    async fn first_request_stores_etag() {
        let mock_server = MockServer::start().await;
        let fixture = include_str!("../../zb_core/fixtures/formula_foo.json");

        Mock::given(method("GET"))
            .and(path("/foo.json"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(fixture)
                    .insert_header("etag", "\"abc123\""),
            )
            .mount(&mock_server)
            .await;

        let cache = ApiCache::in_memory().unwrap();
        let client = ApiClient::with_base_url(mock_server.uri()).with_cache(cache);

        let _ = client.get_formula("foo").await.unwrap();

        let cached = client
            .cache
            .as_ref()
            .unwrap()
            .get(&format!("{}/foo.json", mock_server.uri()))
            .unwrap();
        assert_eq!(cached.etag, Some("\"abc123\"".to_string()));
    }

    #[tokio::test]
    async fn second_request_sends_if_none_match() {
        let mock_server = MockServer::start().await;
        let fixture = include_str!("../../zb_core/fixtures/formula_foo.json");

        // First request returns 200 with ETag
        Mock::given(method("GET"))
            .and(path("/foo.json"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(fixture)
                    .insert_header("etag", "\"abc123\""),
            )
            .expect(1)
            .mount(&mock_server)
            .await;

        let cache = ApiCache::in_memory().unwrap();
        let client = ApiClient::with_base_url(mock_server.uri()).with_cache(cache);

        // First request
        let _ = client.get_formula("foo").await.unwrap();

        // Reset mocks for second request
        mock_server.reset().await;

        // Second request should send If-None-Match and receive 304
        Mock::given(method("GET"))
            .and(path("/foo.json"))
            .and(header("If-None-Match", "\"abc123\""))
            .respond_with(ResponseTemplate::new(304))
            .expect(1)
            .mount(&mock_server)
            .await;

        let formula = client.get_formula("foo").await.unwrap();
        assert_eq!(formula.name, "foo");
    }

    #[tokio::test]
    async fn uses_cached_body_on_304() {
        let mock_server = MockServer::start().await;
        let fixture = include_str!("../../zb_core/fixtures/formula_foo.json");

        // First request returns 200 with ETag
        Mock::given(method("GET"))
            .and(path("/foo.json"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(fixture)
                    .insert_header("etag", "\"abc123\""),
            )
            .mount(&mock_server)
            .await;

        let cache = ApiCache::in_memory().unwrap();
        let client = ApiClient::with_base_url(mock_server.uri()).with_cache(cache);

        // First request populates cache
        let _ = client.get_formula("foo").await.unwrap();

        mock_server.reset().await;

        // Second request returns 304 (no body)
        Mock::given(method("GET"))
            .and(path("/foo.json"))
            .and(header("If-None-Match", "\"abc123\""))
            .respond_with(ResponseTemplate::new(304))
            .mount(&mock_server)
            .await;

        // Should return cached formula
        let formula = client.get_formula("foo").await.unwrap();
        assert_eq!(formula.name, "foo");
        assert_eq!(formula.versions.stable, "1.2.3");
    }

    #[tokio::test]
    async fn fetches_cask_from_mock_server() {
        let mock_server = MockServer::start().await;

        let fixture = include_str!("../../zb_core/fixtures/cask_vscode.json");

        Mock::given(method("GET"))
            .and(path("/visual-studio-code.json"))
            .respond_with(ResponseTemplate::new(200).set_body_string(fixture))
            .mount(&mock_server)
            .await;

        let client = ApiClient::with_base_urls(
            format!("{}/formula", mock_server.uri()),
            mock_server.uri(),
        );
        let cask = client.get_cask("visual-studio-code").await.unwrap();

        assert_eq!(cask.token, "visual-studio-code");
        assert_eq!(cask.version, "1.85.0");
    }

    #[tokio::test]
    async fn returns_missing_cask_on_404() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/nonexistent.json"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&mock_server)
            .await;

        let client = ApiClient::with_base_urls(
            format!("{}/formula", mock_server.uri()),
            mock_server.uri(),
        );
        let err = client.get_cask("nonexistent").await.unwrap_err();

        assert!(matches!(
            err,
            Error::MissingCask { name } if name == "nonexistent"
        ));
    }
}