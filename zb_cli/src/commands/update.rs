use console::style;
use std::time::Instant;
use zb_core::Error;
use zb_io::{ApiCache, ApiClient};

/// Updates the Homebrew formula database and reports progress to stdout.
///
/// If an API cache is provided, it will be cleared before testing connectivity.
/// On success, prints status messages including the elapsed time and a tip.
///
/// # Parameters
///
/// - `api_client`: Homebrew API client used to verify connectivity.
/// - `cache`: Optional API cache; when `Some`, the cache is cleared prior to the connectivity check.
///
/// # Errors
///
/// Returns `Error::StoreCorruption` if clearing the provided cache fails.
/// Returns `Error::NetworkFailure` if a connectivity check to the Homebrew API fails.
///
/// # Examples
///
/// ```
/// # async fn example(api_client: &crate::ApiClient) {
/// // Run from an async context:
/// let _ = crate::commands::update::execute(api_client, None).await;
/// # }
/// ```
pub async fn execute(api_client: &ApiClient, cache: Option<&ApiCache>) -> Result<(), Error> {
    let start = Instant::now();

    println!(
        "{} Updating Homebrew formula database...",
        style("==>").cyan().bold()
    );

    // Clear the API cache if available
    if let Some(cache) = cache {
        cache.clear().map_err(|e| Error::StoreCorruption {
            message: format!("failed to clear API cache: {e}"),
        })?;
        println!("    {} API cache cleared", style("✓").green());
    }

    // Test connectivity by fetching a known formula
    match api_client.get_formula("curl").await {
        Ok(_) => {
            println!(
                "    {} Connected to Homebrew API",
                style("✓").green()
            );
        }
        Err(e) => {
            return Err(Error::NetworkFailure {
                message: format!("failed to connect to Homebrew API: {e}"),
            });
        }
    }

    let elapsed = start.elapsed();
    println!(
        "{} Updated in {:.2}s",
        style("==>").cyan().bold(),
        elapsed.as_secs_f64()
    );

    println!();
    println!(
        "{}",
        style("Tip: Run 'zb outdated' to see available updates").dim()
    );

    Ok(())
}