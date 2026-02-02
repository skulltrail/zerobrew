use console::style;
use zb_core::Error;
use zb_io::{ApiClient, CaskInstaller, install::Installer};

pub struct OutdatedPackage {
    pub name: String,
    pub installed_version: String,
    pub latest_version: String,
    pub is_cask: bool,
}

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
