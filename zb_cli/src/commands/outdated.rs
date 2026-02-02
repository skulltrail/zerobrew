use console::style;
use zb_core::Error;
use zb_io::{ApiClient, CaskInstaller, install::Installer};

pub struct OutdatedPackage {
    pub name: String,
    pub installed_version: String,
    pub latest_version: String,
    pub is_cask: bool,
}

/// Checks installed formulas and casks against the API and prints a summary of any outdated packages.
///
/// The function compares installed package versions to the latest versions obtained from the API,
/// collects packages whose installed version differs from the API version, and prints either a
/// message that all packages are up to date or a formatted list of outdated packages.
///
/// Parameters:
/// - `installer`: installer used to list installed formulas.
/// - `cask_installer`: optional installer used to list installed casks; when `None`, casks are not checked.
/// - `api_client`: client used to fetch latest formula and cask metadata from the API.
/// - `cask_only`: when `true`, skip checking formulas and only check casks (if a cask installer is provided).
///
/// # Returns
///
/// `Ok(())` on success, or an `Error` if listing installed packages or API lookups fail.
///
/// # Examples
///
/// ```no_run
/// # async fn try_main() -> Result<(), zb_core::Error> {
/// // assume `installer`, `cask_installer`, and `api_client` are available and configured
/// // execute(installer, Some(&cask_installer), &api_client, false).await?;
/// # Ok(())
/// # }
/// ```
pub async fn execute(
    installer: &mut Installer,
    cask_installer: Option<&CaskInstaller>,
    api_client: &ApiClient,
    cask_only: bool,
) -> Result<(), Error> {
    let mut outdated = Vec::new();

    // Check formulas
    if !cask_only {
        let installed_formulas = installer.list_installed()?;

        for keg in &installed_formulas {
            match api_client.get_formula(&keg.name).await {
                Ok(formula) => {
                    let latest_version = formula.effective_version();
                    if keg.version != latest_version {
                        outdated.push(OutdatedPackage {
                            name: keg.name.clone(),
                            installed_version: keg.version.clone(),
                            latest_version,
                            is_cask: false,
                        });
                    }
                }
                Err(Error::MissingFormula { .. }) => {
                    // Formula no longer exists in the API, skip
                }
                Err(e) => {
                    eprintln!(
                        "{} Failed to check {}: {}",
                        style("warning:").yellow(),
                        keg.name,
                        e
                    );
                }
            }
        }
    }

    // Check casks
    if let Some(cask_inst) = cask_installer {
        let installed_casks = cask_inst.list_installed()?;

        for cask in &installed_casks {
            match api_client.get_cask(&cask.token).await {
                Ok(latest_cask) => {
                    if cask.version != latest_cask.version {
                        outdated.push(OutdatedPackage {
                            name: cask.token.clone(),
                            installed_version: cask.version.clone(),
                            latest_version: latest_cask.version.clone(),
                            is_cask: true,
                        });
                    }
                }
                Err(Error::MissingCask { .. }) => {
                    // Cask no longer exists in the API, skip
                }
                Err(e) => {
                    eprintln!(
                        "{} Failed to check {}: {}",
                        style("warning:").yellow(),
                        cask.token,
                        e
                    );
                }
            }
        }
    }

    if outdated.is_empty() {
        println!("All packages are up to date.");
    } else {
        println!(
            "{} {} outdated package(s):",
            style("==>").cyan().bold(),
            outdated.len()
        );
        for pkg in &outdated {
            let type_indicator = if pkg.is_cask {
                style("(cask)").dim()
            } else {
                style("").dim()
            };
            println!(
                "    {} {} -> {} {}",
                style(&pkg.name).bold(),
                style(&pkg.installed_version).red(),
                style(&pkg.latest_version).green(),
                type_indicator
            );
        }
    }

    Ok(())
}