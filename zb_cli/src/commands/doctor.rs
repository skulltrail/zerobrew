use console::style;
use std::path::Path;
use zb_core::Error;
use zb_io::install::Installer;

#[derive(Default)]
struct DiagnosticResults {
    warnings: Vec<String>,
    errors: Vec<String>,
}

/// Runs a sequence of system diagnostic checks for zerobrew and prints a human-readable report.
///
/// The function performs directory, database, store, symlink, permission, and PATH checks,
/// aggregates warnings and errors, prints per-check status and a final summary, and never
/// modifies system state beyond temporary permission test files used by the permission check.
///
/// Returns `Ok(())` when the diagnostic run completes; individual problems are reported via
/// printed output and collected into the displayed summary (they are not returned as `Err`).
///
/// # Examples
///
/// ```
/// // Assume `installer`, `root`, and `prefix` are available in your context.
/// // `execute` prints a report and returns Ok when finished.
/// # use std::path::Path;
/// # use zb_cli::installer::Installer;
/// # use zb_cli::commands::doctor::execute;
/// let installer: Installer = unimplemented!();
/// let root = Path::new("/usr/local/zerobrew");
/// let prefix = Path::new("/usr/local");
/// let _ = execute(&installer, root, prefix).unwrap();
/// ```
pub fn execute(installer: &Installer, root: &Path, prefix: &Path) -> Result<(), Error> {
    println!(
        "{} Checking system for potential problems...\n",
        style("==>").cyan().bold()
    );

    let mut results = DiagnosticResults::default();

    // Check directory structure
    check_directories(root, prefix, &mut results);

    // Check database integrity
    check_database(installer, &mut results);

    // Check store integrity
    check_store(root, installer, &mut results);

    // Check symlinks
    check_symlinks(prefix, &mut results);

    // Check permissions
    check_permissions(root, prefix, &mut results);

    // Check PATH
    check_path(prefix, &mut results);

    // Print results
    println!();
    if results.errors.is_empty() && results.warnings.is_empty() {
        println!(
            "{} Your system is ready to brew!",
            style("✓").green().bold()
        );
    } else {
        if !results.errors.is_empty() {
            println!("{} Errors:", style("✗").red().bold());
            for error in &results.errors {
                println!("    {} {}", style("•").red(), error);
            }
            println!();
        }

        if !results.warnings.is_empty() {
            println!("{} Warnings:", style("!").yellow().bold());
            for warning in &results.warnings {
                println!("    {} {}", style("•").yellow(), warning);
            }
            println!();
        }

        let total_issues = results.errors.len() + results.warnings.len();
        println!(
            "Found {} issue(s). Please address them to ensure zerobrew works correctly.",
            total_issues
        );
    }

    Ok(())
}

/// Checks that zerobrew's required directories exist and records any missing ones.
///
/// This function verifies presence of the standard zerobrew directories (root,
/// root/store, root/cache, root/db, prefix, prefix/bin). For each missing
/// directory it appends a descriptive warning to `results`.
///
/// # Parameters
///
/// - `root`: Filesystem path to the zerobrew root directory.
/// - `prefix`: Installation prefix path.
/// - `results`: Mutable diagnostics collector that receives a warning per missing directory.
///
/// # Examples
///
/// ```
/// use std::path::Path;
/// // Minimal illustrative example — in real tests use a temp directory.
/// let root = Path::new("/nonexistent/zbroot");
/// let prefix = Path::new("/nonexistent/zbprefix");
/// let mut results = crate::commands::doctor::DiagnosticResults { warnings: Vec::new(), errors: Vec::new() };
/// crate::commands::doctor::check_directories(root, prefix, &mut results);
/// assert!(results.warnings.iter().any(|w| w.contains("Missing directory")));
/// ```
fn check_directories(root: &Path, prefix: &Path, results: &mut DiagnosticResults) {
    print!("Checking zerobrew directories...");

    let required_dirs = [
        root.to_path_buf(),
        root.join("store"),
        root.join("cache"),
        root.join("db"),
        prefix.to_path_buf(),
        prefix.join("bin"),
    ];

    let mut missing = Vec::new();
    for dir in &required_dirs {
        if !dir.exists() {
            missing.push(dir.display().to_string());
        }
    }

    if missing.is_empty() {
        println!(" {}", style("OK").green());
    } else {
        println!(" {}", style("ISSUES").yellow());
        for dir in missing {
            results.warnings.push(format!("Missing directory: {}", dir));
        }
    }
}

/// Checks the installed formulas database and records any integrity errors.
///
/// Prints a status line ("OK" with the number of formulas or "ERROR") to stdout.
/// On failure, appends a descriptive error message to `results.errors`.
///
/// # Parameters
///
/// - `installer`: used to query the list of installed kegs.
/// - `results`: collection where detected warnings and errors are recorded.
///
/// # Examples
///
/// ```
/// # use zb_cli::commands::doctor::{check_database, DiagnosticResults};
/// # use zb_core::installer::Installer;
/// # let installer: Installer = unimplemented!();
/// let mut results = DiagnosticResults::default();
/// check_database(&installer, &mut results);
/// ```
fn check_database(installer: &Installer, results: &mut DiagnosticResults) {
    print!("Checking database integrity...");

    match installer.list_installed() {
        Ok(kegs) => {
            println!(" {} ({} formulas)", style("OK").green(), kegs.len());
        }
        Err(e) => {
            println!(" {}", style("ERROR").red());
            results.errors.push(format!("Database error: {}", e));
        }
    }
}

/// Verifies the integrity of the zerobrew store directory and records any issues.
///
/// This function checks whether the store directory exists, skips the check if it does not,
/// enumerates installed packages via the provided installer, and records an error for each
/// installed package that lacks a corresponding store entry. Findings are appended to
/// `results.errors`. The function prints a brief status indicator (OK, SKIP, or ISSUES).
///
/// # Parameters
///
/// - `root` — Filesystem path to the zerobrew root directory (contains the `store` subdirectory).
/// - `installer` — Installer used to obtain the list of installed packages.
/// - `results` — Mutable accumulator for diagnostic warnings and errors; missing store entries
///   are appended to `results.errors`.
///
/// # Examples
///
/// ```
/// // Example (illustrative): create a mock installer that reports no installed packages
/// // and run the check against a temporary root path.
/// use std::path::Path;
///
/// struct MockInstaller;
/// impl MockInstaller {
///     fn list_installed(&self) -> Result<Vec<()>, ()> { Ok(vec![]) }
/// }
///
/// // Assuming DiagnosticResults is available in scope:
/// // let mut results = DiagnosticResults { warnings: vec![], errors: vec![] };
/// // check_store(Path::new("/tmp/zerobrew"), &mock_installer, &mut results);
/// ```
fn check_store(root: &Path, installer: &Installer, results: &mut DiagnosticResults) {
    print!("Checking store integrity...");

    let store_path = root.join("store");
    if !store_path.exists() {
        println!(" {}", style("SKIP").dim());
        return;
    }

    let installed = match installer.list_installed() {
        Ok(kegs) => kegs,
        Err(_) => {
            println!(" {}", style("SKIP").dim());
            return;
        }
    };

    let orphaned_count = 0;
    let mut missing_count = 0;

    // Check for orphaned store entries
    if let Ok(entries) = std::fs::read_dir(&store_path) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            // Skip non-store entries
            if name.starts_with('.') {
                continue;
            }
            // This is a simplified check - in reality we'd check refcount
        }
    }

    // Check for missing store entries for installed packages
    for keg in &installed {
        let store_entry = store_path.join(&keg.store_key);
        if !store_entry.exists() {
            missing_count += 1;
            results.errors.push(format!(
                "Missing store entry for {}: {}",
                keg.name, &keg.store_key[..12]
            ));
        }
    }

    if missing_count == 0 && orphaned_count == 0 {
        println!(" {}", style("OK").green());
    } else {
        println!(" {}", style("ISSUES").yellow());
    }
}

/// Checks for broken symbolic links inside `prefix/bin` and appends warnings for each broken link to `results`.
///
/// If `prefix/bin` does not exist the check is skipped.
///
/// # Examples
///
/// ```
/// use std::path::Path;
///
/// // Assume DiagnosticResults and check_symlinks are in scope
/// let mut results = DiagnosticResults { warnings: Vec::new(), errors: Vec::new() };
/// check_symlinks(Path::new("/unlikely/to/exist/for/tests"), &mut results);
/// assert!(results.warnings.is_empty());
/// ```
fn check_symlinks(prefix: &Path, results: &mut DiagnosticResults) {
    print!("Checking symlinks...");

    let bin_path = prefix.join("bin");
    if !bin_path.exists() {
        println!(" {}", style("SKIP").dim());
        return;
    }

    let mut broken_count = 0;

    if let Ok(entries) = std::fs::read_dir(&bin_path) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_symlink() {
                if let Ok(target) = std::fs::read_link(&path) {
                    // Check if target exists
                    let absolute_target = if target.is_absolute() {
                        target.clone()
                    } else {
                        path.parent().unwrap_or(prefix).join(&target)
                    };

                    if !absolute_target.exists() {
                        broken_count += 1;
                        results.warnings.push(format!(
                            "Broken symlink: {} -> {}",
                            path.display(),
                            target.display()
                        ));
                    }
                }
            }
        }
    }

    if broken_count == 0 {
        println!(" {}", style("OK").green());
    } else {
        println!(
            " {} ({} broken)",
            style("ISSUES").yellow(),
            broken_count
        );
    }
}

/// Checks write permissions for the zerobrew root and the prefix's bin directory and records any issues.
///
/// If writing a temporary file to `root` or to `prefix/bin` (when it exists) fails, a warning describing
/// the unreadable location is appended to `results.warnings`. Prints a per-step status ("OK" or "ISSUES") to stdout.
///
/// # Examples
///
/// ```
/// use std::path::Path;
/// use std::fs;
/// use std::env;
///
/// // Minimal DiagnosticResults clone for the example.
/// #[derive(Default)]
/// struct DiagnosticResults {
///     warnings: Vec<String>,
///     errors: Vec<String>,
/// }
///
/// // Assume check_permissions is in scope.
/// let tmp = env::temp_dir();
/// let root = tmp.join("zb_example_root");
/// let prefix = tmp.join("zb_example_prefix");
///
/// // Prepare directories
/// let _ = fs::remove_dir_all(&root);
/// let _ = fs::remove_dir_all(&prefix);
/// fs::create_dir_all(&root).unwrap();
/// fs::create_dir_all(prefix.join("bin")).unwrap();
///
/// let mut results = DiagnosticResults::default();
/// // call the function (uncomment when check_permissions is available)
/// // check_permissions(&root, &prefix, &mut results);
///
/// // cleanup
/// let _ = fs::remove_dir_all(&root);
/// let _ = fs::remove_dir_all(&prefix);
/// ```
fn check_permissions(root: &Path, prefix: &Path, results: &mut DiagnosticResults) {
    print!("Checking permissions...");

    let mut issues = Vec::new();

    // Check if we can write to root
    let test_file = root.join(".zb_permission_test");
    match std::fs::write(&test_file, "test") {
        Ok(_) => {
            let _ = std::fs::remove_file(&test_file);
        }
        Err(_) => {
            issues.push(format!(
                "Cannot write to zerobrew root: {}",
                root.display()
            ));
        }
    }

    // Check if we can write to prefix/bin
    let bin_path = prefix.join("bin");
    if bin_path.exists() {
        let test_file = bin_path.join(".zb_permission_test");
        match std::fs::write(&test_file, "test") {
            Ok(_) => {
                let _ = std::fs::remove_file(&test_file);
            }
            Err(_) => {
                issues.push(format!(
                    "Cannot write to prefix bin: {}",
                    bin_path.display()
                ));
            }
        }
    }

    if issues.is_empty() {
        println!(" {}", style("OK").green());
    } else {
        println!(" {}", style("ISSUES").yellow());
        for issue in issues {
            results.warnings.push(issue);
        }
    }
}

/// Verifies whether the installation prefix's `bin` directory is present in the user's PATH and records a warning if it is not.
///
/// If the PATH environment variable is available and contains the prefix's `bin` directory (prefix/bin), this function prints an OK status. If PATH is available but does not contain prefix/bin, it prints NOT IN PATH and appends a warning to `results` containing a suggested export line to add to the user's shell profile. If PATH is unavailable, the check is skipped.
///
/// # Parameters
///
/// - `prefix`: Filesystem path to the installation prefix whose `bin` directory should be checked.
/// - `results`: Mutable collector for diagnostic warnings and errors; a warning is appended when prefix/bin is not found in PATH.
///
/// # Examples
///
/// ```
/// let mut results = DiagnosticResults { warnings: Vec::new(), errors: Vec::new() };
/// check_path(std::path::Path::new("/usr/local"), &mut results);
/// // After running, `results.warnings` may contain an entry suggesting to add "/usr/local/bin" to PATH.
/// ```
fn check_path(prefix: &Path, results: &mut DiagnosticResults) {
    print!("Checking PATH...");

    let bin_path = prefix.join("bin");
    let bin_str = bin_path.to_string_lossy();

    if let Ok(path_var) = std::env::var("PATH") {
        let paths: Vec<&str> = path_var.split(':').collect();
        if paths.iter().any(|p| *p == bin_str.as_ref()) {
            println!(" {}", style("OK").green());
        } else {
            println!(" {}", style("NOT IN PATH").yellow());
            results.warnings.push(format!(
                "{} is not in your PATH. Add it to your shell profile:\n      export PATH=\"{}:$PATH\"",
                bin_str, bin_str
            ));
        }
    } else {
        println!(" {}", style("SKIP").dim());
    }
}