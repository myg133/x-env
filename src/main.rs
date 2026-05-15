use anyhow::{Context, Result};
use clap::Parser;
use std::collections::HashMap;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Run programs with environment variables from config files
#[derive(Parser, Debug)]
#[command(
    name = "x-env",
    version,
    author,
    about = "Run programs with environment variables from config files",
    after_help = "CONFIG FILE FORMAT:\n\
                  [env]\n\
                  KEY1=value1\n\
                  KEY2=value2\n\
                  \n\
                  [exe]\n\
                  program_name.exe\n\
                  \n\
                  [p-args]\n\
                  --argument1\n\
                  value1"
)]
struct Args {
    /// Path to a specific environment file to use
    #[arg(short = 'f', long, global = true)]
    env_file: Option<PathBuf>,

    /// Set the working directory for the program (useful for context menu integration)
    #[arg(short = 'd', long, global = true)]
    cwd: Option<PathBuf>,

    /// Program to execute, or use [exe] in config file
    program: Option<String>,

    /// Arguments to pass to the program
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    program_args: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Section {
    Env,
    PArgs,
    Exe,
}

impl Section {
    fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "env" => Some(Section::Env),
            "p-args" => Some(Section::PArgs),
            "exe" => Some(Section::Exe),
            _ => None,
        }
    }
}

#[derive(Debug, Default)]
struct ParsedConfig {
    env_vars: HashMap<String, String>,
    parsed_args: Vec<String>,
    exe: Option<String>,
}

fn parse_config_file(path: &Path) -> Result<ParsedConfig> {
    let content = fs::read_to_string(path).context(format!("Failed to read config file: {}", path.display()))?;

    let mut config = ParsedConfig::default();
    let mut current_section = Section::Env;

    for line in content.lines() {
        let line = line.trim();

        // Skip empty lines and comments
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        // Check for section header
        if let Some(stripped) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            if let Some(section) = Section::from_str(stripped) {
                current_section = section;
                continue;
            }
        }

        match current_section {
            Section::Env => {
                // Parse KEY=value
                if let Some((key, value)) = line.split_once('=') {
                    let key = key.trim().to_string();
                    let mut value = value.trim().to_string();

                    // Remove surrounding quotes
                    if (value.starts_with('"') && value.ends_with('"'))
                        || (value.starts_with('\'') && value.ends_with('\''))
                    {
                        value = value[1..value.len() - 1].to_string();
                    }

                    config.env_vars.insert(key, value);
                }
            }
            Section::PArgs => {
                if !line.is_empty() {
                    config.parsed_args.push(line.to_string());
                }
            }
            Section::Exe => {
                if !line.is_empty() {
                    config.exe = Some(line.to_string());
                }
            }
        }
    }

    Ok(config)
}

fn find_default_env_file(cwd: &Path) -> Option<PathBuf> {
    let candidates = [".env", "env", "args"];

    for name in &candidates {
        let path = cwd.join(name);
        if path.exists() {
            println!("Using default environment file: {}", path.display());
            return Some(path);
        }
    }
    None
}

/// Convert Windows path to msys compatible format
fn convert_to_valid_path(path: &str) -> String {
    let path = Path::new(path);

    // If relative, convert to absolute
    let absolute = if path.is_relative() {
        env::current_dir()
            .map(|cwd| cwd.join(path))
            .ok()
    } else {
        Some(path.to_path_buf())
    };

    let full_path = match absolute {
        Some(p) => p,
        None => return path.to_string_lossy().to_string(),
    };

    // Get full path (resolves short paths/aliases)
    let full_path = full_path
        .canonicalize()
        .unwrap_or(full_path);

    // Replace backslashes with forward slashes (for msys compatibility)
    full_path.to_string_lossy().replace('\\', "/")
}

/// Check if an argument looks like a path that needs conversion
fn needs_path_conversion(arg: &str) -> bool {
    // Contains backslash (Windows path separator)
    // or starts with .\ or ..\
    arg.contains('\\') || arg.starts_with(".\\") || arg.starts_with("..\\")
}

fn find_program(program_name: &str) -> Result<String> {
    // First try as direct path
    let path = Path::new(program_name);
    if path.exists() {
        return Ok(program_name.to_string());
    }

    // Try to find in PATH
    if let Some(found) = find_in_path(program_name)? {
        return Ok(found);
    }

    // On Windows, try to resolve via PowerShell (handles msys/git bash commands)
    if cfg!(target_os = "windows") {
        if let Some(found) = find_via_powershell(program_name)? {
            return Ok(found);
        }
    }

    // Command not found - but on Windows we'll try shell execution later
    if cfg!(target_os = "windows") {
        // Return the program name as-is; shell fallback will handle it
        return Ok(program_name.to_string());
    }

    anyhow::bail!("Program not found: {}", program_name)
}

fn find_in_path(program_name: &str) -> Result<Option<String>> {
    let path_var = env::var_os("PATH").unwrap_or_default();

    for dir in env::split_paths(&path_var) {
        let full_path = dir.join(program_name);
        if full_path.is_file() {
            return Ok(Some(full_path.to_string_lossy().into_owned()));
        }
        // On Windows, also check with .exe extension
        if cfg!(target_os = "windows") {
            let with_exe = full_path.with_extension("exe");
            if with_exe.is_file() {
                return Ok(Some(with_exe.to_string_lossy().into_owned()));
            }
        }
    }
    Ok(None)
}

#[cfg(target_os = "windows")]
fn find_via_powershell(program_name: &str) -> Result<Option<String>> {
    use std::process::Command;

    // Filter out aliases (like 'ls' -> Get-ChildItem) - only get actual files
    // Use -CommandType to exclude aliases and functions
    let ps_script = format!(
        "(Get-Command '{}' -CommandType Application,ExternalScript -ErrorAction SilentlyContinue).Source",
        program_name
    );

    let output = Command::new("pwsh")
        .args(["-NoProfile", "-Command", &ps_script])
        .output();

    // Fall back to powershell.exe if pwsh fails
    let output = match output {
        Ok(o) if o.status.success() => Some(o),
        _ => {
            let o = Command::new("powershell")
                .args(["-NoProfile", "-Command", &ps_script])
                .output();
            o.ok()
        }
    };

    if let Some(output) = output {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            // Only return if it's an actual file path (contains / or \)
            if !path.is_empty() && Path::new(&path).exists()
                && (path.contains('/') || path.contains('\\'))
            {
                return Ok(Some(path));
            }
        }
    }
    Ok(None)
}

#[cfg(not(target_os = "windows"))]
fn find_via_powershell(_program_name: &str) -> Result<Option<String>> {
    Ok(None)
}

/// Check if bash is WSL (which we want to avoid for running Windows programs)
#[cfg(target_os = "windows")]
fn is_wsl_bash() -> bool {
    use std::process::Command;

    // Check if bash exists and get its path
    let output = Command::new("where")
        .arg("bash")
        .output();

    if let Ok(output) = output {
        let path = String::from_utf8_lossy(&output.stdout);
        // WSL bash typically installed to System32 or Windows/System32
        // MSYS2 bash is in C:\msys64\...
        // Git Bash is in C:\Program Files\Git\...
        if path.to_lowercase().contains("system32")
            || path.to_lowercase().contains("windows\\system")
            || path.to_lowercase().contains("windows/system")
        {
            return true;
        }
    }

    // Also check via bash --version if it runs quickly
    let output = Command::new("bash")
        .args(["--version"])
        .output();

    if let Ok(output) = output {
        let version = String::from_utf8_lossy(&output.stdout);
        // WSL uses "linux" in version string
        if version.to_lowercase().contains("linux") {
            return true;
        }
    }

    false
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Determine working directory first: use --cwd argument if provided, otherwise current directory
    let cwd = if let Some(ref dir) = args.cwd {
        if !dir.exists() {
            anyhow::bail!("Specified working directory not found: {}", dir.display());
        }
        if !dir.is_dir() {
            anyhow::bail!("Specified path is not a directory: {}", dir.display());
        }
        dir.clone()
    } else {
        env::current_dir().context("Failed to get current directory")?
    };

    // Determine config file to use (search in cwd, not current directory)
    let config_path = if let Some(ref f) = args.env_file {
        if !f.exists() {
            anyhow::bail!("Specified environment file not found: {}", f.display());
        }
        f.clone()
    } else {
        match find_default_env_file(&cwd) {
            Some(path) => path,
            None => {
                println!("Warning: No environment file found (searched for: .env, env, args). Proceeding without environment variables.");
                cwd.join(".env")
            }
        }
    };

    // Parse config if exists
    let config = if config_path.exists() {
        parse_config_file(&config_path)?
    } else {
        ParsedConfig::default()
    };

    // Display parsed environment variables
    if !config.env_vars.is_empty() {
        println!("Environment variables found:");
        for (key, value) in &config.env_vars {
            println!("  {}={}", key, value);
        }
    } else {
        println!("No environment variables found in the file.");
    }

    // Determine executable to run
    // If program is provided as argument (and not empty), use it; otherwise check [exe] in config
    let exe_name = if let Some(ref p) = args.program {
        if p.is_empty() {
            // Empty string, check [exe] from config
            config.exe.clone()
        } else if p.starts_with('-') && config.exe.is_some() {
            // Looks like an option, fall back to [exe] from config
            config.exe.clone()
        } else {
            Some(p.clone())
        }
    } else {
        config.exe.clone()
    };

    let exe_name = match exe_name {
        Some(e) => e,
        None => {
            // No executable specified - open a terminal window in the specified directory
            println!("No executable specified, opening terminal...");
            #[cfg(target_os = "windows")]
            {
                use std::process::Command;

                // Try to open Windows Terminal first, then fall back to cmd
                let status = Command::new("wt")
                    .args(["--cwd", cwd.to_str().unwrap_or(".")])
                    .current_dir(&cwd)
                    .status()
                    .or_else(|_| {
                        Command::new("cmd")
                            .args(["/K", "cd /D", cwd.to_str().unwrap_or(".")])
                            .current_dir(&cwd)
                            .status()
                    });

                match status {
                    Ok(s) if s.success() => return Ok(()),
                    Ok(s) => std::process::exit(s.code().unwrap_or(1)),
                    Err(e) => {
                        eprintln!("Failed to open terminal: {}", e);
                        std::process::exit(1);
                    }
                }
            }

            #[cfg(not(target_os = "windows"))]
            {
                // On non-Windows, just show error
                anyhow::bail!("No executable specified. Provide program name as argument or [exe] in config file.");
            }
        }
    };

    // Find the actual program
    let resolved_exe = find_program(&exe_name)?;
    println!("Executable: {}", resolved_exe);

    // Display parsed args from config
    if !config.parsed_args.is_empty() {
        println!("Processed arguments from [p-args] section: {}", config.parsed_args.join(" "));
    }

    // Merge args: [p-args] from config first, then command line args
    let mut all_args: Vec<String> = Vec::new();

    // Add args from [p-args] section
    for arg in &config.parsed_args {
        if needs_path_conversion(arg) {
            all_args.push(convert_to_valid_path(arg));
        } else {
            all_args.push(arg.clone());
        }
    }

    // Add command line args
    let cli_args = &args.program_args;

    for arg in cli_args {
        if needs_path_conversion(arg) {
            all_args.push(convert_to_valid_path(arg));
        } else {
            all_args.push(arg.clone());
        }
    }

    println!("Executing program in current directory: {}", cwd.display());
    println!("Final execution command: {} {}", resolved_exe, all_args.join(" "));

    // Build environment with our vars
    let mut env_vars: HashMap<OsString, OsString> = env::vars_os()
        .map(|(k, v)| (k, v))
        .collect();

    for (key, value) in &config.env_vars {
        env_vars.insert(
            OsString::from(key),
            OsString::from(value),
        );
    }

    // Execute
    #[cfg(target_os = "windows")]
    let status = {
        // First try direct execution
        let mut cmd = Command::new(&resolved_exe);
        cmd.args(&all_args)
           .current_dir(&cwd)
           .env_clear()
           .envs(env_vars.clone());

        match cmd.status() {
            Ok(s) if s.success() => s,
            _ => {
                // Fall back to shell execution (for msys/git bash commands)
                // Skip bash if it's WSL (which would launch unnecessary Linux environment)
                let bash_status = if is_wsl_bash() {
                    None
                } else {
                    std::process::Command::new("bash")
                        .args(["-c", &format!("{} {}", resolved_exe, all_args.join(" "))])
                        .current_dir(&cwd)
                        .env_clear()
                        .envs(&env_vars)
                        .status()
                        .ok()
                };

                bash_status
                    .or_else(|| {
                        std::process::Command::new("pwsh")
                            .args(["-NoProfile", "-Command", &format!("{} {}", resolved_exe, all_args.join(" "))])
                            .current_dir(&cwd)
                            .env_clear()
                            .envs(&env_vars)
                            .status()
                            .ok()
                    })
                    .or_else(|| {
                        std::process::Command::new("powershell")
                            .args(["-NoProfile", "-Command", &format!("{} {}", resolved_exe, all_args.join(" "))])
                            .current_dir(&cwd)
                            .env_clear()
                            .envs(&env_vars)
                            .status()
                            .ok()
                    })
                    .unwrap_or_else(|| {
                        // Last resort: try direct execution again
                        std::process::Command::new(&resolved_exe)
                            .args(&all_args)
                            .current_dir(&cwd)
                            .env_clear()
                            .envs(env_vars)
                            .status()
                            .unwrap_or_else(|e| {
                                eprintln!("Failed to execute: {}", e);
                                std::process::exit(1);
                            })
                    })
            }
        }
    };

    #[cfg(not(target_os = "windows"))]
    let status = {
        let mut cmd = Command::new(&resolved_exe);
        cmd.args(&all_args)
           .current_dir(&cwd)
           .env_clear()
           .envs(env_vars);
        cmd.status()?
    };

    if status.success() {
        Ok(())
    } else {
        std::process::exit(status.code().unwrap_or(1));
    }
}
