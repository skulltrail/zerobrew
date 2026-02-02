use clap::Parser;
use console::style;
use zb_cli::{
    cli::{Cli, Commands},
    commands,
    init::ensure_init,
    utils::get_root_path,
};
use zb_io::{ApiClient, create_cask_installer, install::create_installer};

/// Program entry point for the CLI application.
///
/// Parses command-line arguments, delegates execution to `run`, prints a styled
/// error message on failure, and exits with status code 1.
///
/// # Examples
///
/// ```no_run
/// // The binary invokes this as its entry point.
/// main();
/// ```
#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    if let Err(e) = run(cli).await {
        eprintln!("{} {}", style("error:").red().bold(), e);
        std::process::exit(1);
    }
}

/// Entrypoint for executing the CLI command represented by `cli`.
///
/// Resolves paths and initialization state, constructs installers and clients as needed,
/// and dispatches to the appropriate command handler (including cask-aware paths).
///
/// # Returns
///
/// `Ok(())` on success, or a `zb_core::Error` describing the failure.
///
/// # Examples
///
/// ```no_run
/// # use zb_cli::cli::Cli;
/// # async fn _example(cli: Cli) -> Result<(), zb_core::Error> {
/// run(cli).await
/// # }
/// ```
async fn run(cli: Cli) -> Result<(), zb_core::Error> {
    if let Commands::Completion { shell } = cli.command {
        return commands::completion::execute(shell);
    }

    let root = get_root_path(cli.root);
    let prefix = cli.prefix.unwrap_or_else(|| root.join("prefix"));

    if matches!(cli.command, Commands::Init) {
        return commands::init::execute(&root, &prefix);
    }

    if !matches!(cli.command, Commands::Reset { .. }) {
        ensure_init(&root, &prefix)?;
    }

    let mut installer = create_installer(&root, &prefix, cli.concurrency)?;

    match cli.command {
        Commands::Init => unreachable!(),
        Commands::Completion { .. } => unreachable!(),
        Commands::Install {
            formulas,
            no_link,
            cask,
        } => {
            if cask {
                let mut cask_installer = create_cask_installer(&root, None, cli.concurrency)?;
                for formula in formulas {
                    println!(
                        "{} Installing cask {}...",
                        style("==>").cyan().bold(),
                        style(&formula).bold()
                    );
                    cask_installer.install(&formula, None).await?;
                    println!(
                        "{} {} installed successfully",
                        style("✓").green(),
                        style(&formula).bold()
                    );
                }
                Ok(())
            } else {
                commands::install::execute(&mut installer, formulas, no_link).await
            }
        }
        Commands::Bundle { file, no_link } => {
            commands::bundle::execute(&mut installer, &file, no_link).await
        }
        Commands::Uninstall { formula, cask } => {
            if cask {
                let mut cask_installer = create_cask_installer(&root, None, cli.concurrency)?;
                if let Some(name) = formula {
                    cask_installer.uninstall(&name)?;
                    println!(
                        "{} {} uninstalled successfully",
                        style("✓").green(),
                        style(&name).bold()
                    );
                }
                Ok(())
            } else {
                commands::uninstall::execute(&mut installer, formula)
            }
        }
        Commands::Migrate { yes, force } => {
            commands::migrate::execute(&mut installer, yes, force).await
        }
        Commands::List { cask } => {
            if cask {
                let cask_installer = create_cask_installer(&root, None, cli.concurrency)?;
                let installed = cask_installer.list_installed()?;
                if installed.is_empty() {
                    println!("No casks installed.");
                } else {
                    for c in installed {
                        println!("{} {}", style(&c.token).bold(), style(&c.version).dim());
                    }
                }
                Ok(())
            } else {
                commands::list::execute(&mut installer)
            }
        }
        Commands::Info { formula, cask } => {
            if cask {
                let cask_installer = create_cask_installer(&root, None, cli.concurrency)?;
                if let Some(c) = cask_installer.get_installed(&formula) {
                    println!("{}       {}", style("Token:").dim(), style(&c.token).bold());
                    println!("{}     {}", style("Version:").dim(), c.version);
                    if let Some(app_path) = c.app_path {
                        println!("{}    {}", style("App Path:").dim(), app_path);
                    }
                    println!(
                        "{}   {}s since epoch",
                        style("Installed:").dim(),
                        c.installed_at
                    );
                } else {
                    return Err(zb_core::Error::CaskNotInstalled { name: formula });
                }
                Ok(())
            } else {
                commands::info::execute(&mut installer, formula)
            }
        }
        Commands::Gc => commands::gc::execute(&mut installer),
        Commands::Reset { yes } => commands::reset::execute(&root, &prefix, yes),
        Commands::Run { formula, args } => {
            commands::run::execute(&mut installer, formula, args).await
        }
        Commands::Outdated { cask } => {
            let api_client = ApiClient::new();
            let cask_installer = if cask || true {
                // Always create cask installer to check both
                Some(create_cask_installer(&root, None, cli.concurrency)?)
            } else {
                None
            };
            commands::outdated::execute(
                &mut installer,
                cask_installer.as_ref(),
                &api_client,
                cask,
            )
            .await
        }
        Commands::Upgrade { formulas, cask } => {
            let api_client = ApiClient::new();
            let mut cask_installer = if cask || true {
                Some(create_cask_installer(&root, None, cli.concurrency)?)
            } else {
                None
            };
            commands::upgrade::execute(
                &mut installer,
                cask_installer.as_mut(),
                &api_client,
                formulas,
                cask,
            )
            .await
        }
        Commands::Update => {
            let api_client = ApiClient::new();
            commands::update::execute(&api_client, None).await
        }
        Commands::Cleanup { dry_run, scrub } => {
            commands::cleanup::execute(&mut installer, &root, dry_run, scrub)
        }
        Commands::Doctor => commands::doctor::execute(&installer, &root, &prefix),
    }
}