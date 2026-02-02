use console::style;
use std::path::Path;
use zb_core::Error;
use zb_io::install::Installer;

#[derive(Default)]
struct DiagnosticResults {
    warnings: Vec<String>,
    errors: Vec<String>,
}

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

#[cfg(test)]
#[path = "doctor_test.rs"]
mod doctor_test;