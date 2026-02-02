use console::style;
use std::time::Instant;
use zb_core::Error;
use zb_io::{ApiClient, CaskInstaller, install::Installer};

use crate::commands::outdated::OutdatedPackage;

/// Checks installed formulas and casks for newer versions and upgrades any that are outdated.
///
/// If `cask_only` is true, only casks are inspected. If `formulas` is non-empty, only packages whose name (for formulas) or token (for casks) appears in that list are considered. Progress and per-package results are printed to stdout/stderr; installer and API errors are propagated.
///
/// # Examples
///
/// ```no_run
/// # async fn example() -> Result<(), zb_cli::Error> {
/// # let mut installer = unimplemented!(); // Installer
/// # let mut cask_inst = unimplemented!(); // CaskInstaller
/// # let api_client = unimplemented!(); // ApiClient
/// // Check and upgrade all formulas and casks
/// zb_cli::commands::upgrade::execute(&mut installer, Some(&mut cask_inst), &api_client, Vec::new(), false).await?;
/// # Ok(())
/// # }
/// ```
pub async fn execute(
    installer: &mut Installer,
    cask_installer: Option<&mut CaskInstaller>,
    api_client: &ApiClient,
    formulas: Vec<String>,
    cask_only: bool,
) -> Result<(), Error> {
    let start = Instant::now();

    // Find outdated packages
    let mut to_upgrade: Vec<OutdatedPackage> = Vec::new();

    // Check formulas
    if !cask_only {
        let installed_formulas = installer.list_installed()?;

        for keg in &installed_formulas {
            // If specific formulas provided, filter
            if !formulas.is_empty() && !formulas.contains(&keg.name) {
                continue;
            }

            match api_client.get_formula(&keg.name).await {
                Ok(formula) => {
                    let latest_version = formula.effective_version();
                    if keg.version != latest_version {
                        to_upgrade.push(OutdatedPackage {
                            name: keg.name.clone(),
                            installed_version: keg.version.clone(),
                            latest_version,
                            is_cask: false,
                        });
                    }
                }
                Err(Error::MissingFormula { .. }) => {}
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
    if let Some(ref cask_inst) = cask_installer {
        let installed_casks = cask_inst.list_installed()?;

        for cask in &installed_casks {
            // If specific formulas provided, filter
            if !formulas.is_empty() && !formulas.contains(&cask.token) {
                continue;
            }

            match api_client.get_cask(&cask.token).await {
                Ok(latest_cask) => {
                    if cask.version != latest_cask.version {
                        to_upgrade.push(OutdatedPackage {
                            name: cask.token.clone(),
                            installed_version: cask.version.clone(),
                            latest_version: latest_cask.version.clone(),
                            is_cask: true,
                        });
                    }
                }
                Err(Error::MissingCask { .. }) => {}
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

    if to_upgrade.is_empty() {
        println!("All packages are up to date.");
        return Ok(());
    }

    println!(
        "{} Upgrading {} package(s)...",
        style("==>").cyan().bold(),
        to_upgrade.len()
    );

    for pkg in &to_upgrade {
        println!(
            "    {} {} -> {}",
            style(&pkg.name).bold(),
            style(&pkg.installed_version).red(),
            style(&pkg.latest_version).green()
        );
    }

    let mut upgraded_count = 0;

    // Upgrade formulas
    let formula_names: Vec<String> = to_upgrade
        .iter()
        .filter(|p| !p.is_cask)
        .map(|p| p.name.clone())
        .collect();

    if !formula_names.is_empty() {
        println!(
            "\n{} Upgrading formulas...",
            style("==>").cyan().bold()
        );

        // Uninstall old versions first
        for name in &formula_names {
            if let Err(e) = installer.uninstall(name) {
                eprintln!(
                    "{} Failed to uninstall old version of {}: {}",
                    style("warning:").yellow(),
                    name,
                    e
                );
            }
        }

        // Install new versions
        let plan = installer.plan(&formula_names).await?;
        installer.execute(plan, true).await?;
        upgraded_count += formula_names.len();
    }

    // Upgrade casks
    if let Some(cask_inst) = cask_installer {
        let cask_names: Vec<&OutdatedPackage> = to_upgrade.iter().filter(|p| p.is_cask).collect();

        if !cask_names.is_empty() {
            println!("\n{} Upgrading casks...", style("==>").cyan().bold());

            for pkg in cask_names {
                // Uninstall old version
                if let Err(e) = cask_inst.uninstall(&pkg.name) {
                    eprintln!(
                        "{} Failed to uninstall old version of {}: {}",
                        style("warning:").yellow(),
                        pkg.name,
                        e
                    );
                    continue;
                }

                // Install new version
                match cask_inst.install(&pkg.name, None).await {
                    Ok(_) => {
                        println!(
                            "    {} {} upgraded",
                            style("✓").green(),
                            style(&pkg.name).bold()
                        );
                        upgraded_count += 1;
                    }
                    Err(e) => {
                        eprintln!(
                            "{} Failed to upgrade {}: {}",
                            style("error:").red(),
                            pkg.name,
                            e
                        );
                    }
                }
            }
        }
    }

    let elapsed = start.elapsed();
    println!(
        "\n{} Upgraded {} package(s) in {:.2}s",
        style("==>").cyan().bold(),
        style(upgraded_count).green().bold(),
        elapsed.as_secs_f64()
    );

    Ok(())
}