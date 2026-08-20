//! Release preparation workflow.
//!
//! `cargo xtask release-tag X.Y.Z` creates (or resumes) the local branch
//! `release/vX.Y.Z`, runs gates, updates release metadata, and optionally makes
//! a local commit with `--commit`. Despite the historical command name, it
//! never creates a tag and never pushes. The operator reviews a PR, merges it
//! through protected `main`, and only then creates the tag from the merged SHA.

use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};

use crate::release_preflight;

#[derive(Debug)]
pub struct Args {
    pub version: String,
    pub skip_gates: bool,
    pub commit: bool,
}

pub fn run(args: Args) -> Result<()> {
    parse_semver(&args.version)?;
    ensure_clean_tree()?;

    let tag = format!("v{}", args.version);
    release_preflight::validate_proposed_tag(&tag)?;
    let branch = format!("release/{tag}");
    enter_release_branch(&branch)?;
    println!("xtask release-tag: preparing {tag} on {branch}");

    if !args.skip_gates {
        run_gates()?;
    } else {
        eprintln!("WARN: --skip-gates set; the release PR must still pass every CI lane");
    }

    bump_cargo_toml(&args.version)?;
    regen_openapi()?;
    refresh_cargo_lock()?;
    roll_changelog(&args.version)?;
    release_preflight::validate_release_files(Path::new("."), &tag)
        .context("generated release files failed the release preflight")?;
    run_post_update_checks()?;

    if args.commit {
        commit_preparation(&args.version, &branch)?;
    } else {
        println!("xtask release-tag: files prepared but not committed (pass --commit to commit)");
    }

    print_next_steps(&args.version, &branch);
    Ok(())
}

fn parse_semver(version: &str) -> Result<()> {
    let parts: Vec<_> = version.split('.').collect();
    if parts.len() != 3
        || parts.iter().any(|part| {
            part.is_empty()
                || !part.bytes().all(|byte| byte.is_ascii_digit())
                || (part.len() > 1 && part.starts_with('0'))
                || part.parse::<u64>().is_err()
        })
    {
        bail!(
            "version must be canonical `MAJOR.MINOR.PATCH` without leading zeros (got `{version}`)"
        );
    }
    Ok(())
}

fn ensure_clean_tree() -> Result<()> {
    let status = git_output(&["status", "--porcelain"])?;
    if !status.trim().is_empty() {
        bail!("working tree is not clean — commit or stash before release preparation:\n{status}");
    }
    Ok(())
}

fn enter_release_branch(branch: &str) -> Result<()> {
    let current = git_output(&["branch", "--show-current"])?;
    if current == branch {
        return Ok(());
    }
    if current != "main" {
        bail!(
            "release preparation must start on `main` or resume `{branch}` (currently `{current}`)"
        );
    }
    let head = git_output(&["rev-parse", "HEAD"])?;
    let origin_main = git_output(&["rev-parse", "origin/main"])
        .context("origin/main is unavailable; fetch origin before release preparation")?;
    if head != origin_main {
        bail!(
            "local main ({head}) is not exactly origin/main ({origin_main}); fetch and fast-forward first"
        );
    }

    let exists = Command::new("git")
        .args([
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch}"),
        ])
        .status()
        .context("checking for an existing release branch")?;
    if exists.success() {
        bail!("local branch `{branch}` already exists; switch to it explicitly to resume");
    }

    run_command("git switch", "git", &["switch", "-c", branch])
}

fn run_gates() -> Result<()> {
    println!("xtask release-tag: running pre-update gates…");
    for args in [
        vec!["fmt", "--all", "--check"],
        vec!["xtask", "openapi", "--check"],
        vec!["xtask", "lint-migrations"],
        vec!["xtask", "check-cross-tenant"],
        vec!["xtask", "test-shape"],
        vec!["xtask", "check-doc-fences"],
        vec!["xtask", "check-api-doc"],
        vec!["xtask", "check-snake-case"],
        vec![
            "clippy",
            "--workspace",
            "--all-targets",
            "--locked",
            "--",
            "-D",
            "warnings",
        ],
        vec!["test", "--workspace", "--locked"],
    ] {
        run_command(&format!("cargo {}", args.join(" ")), "cargo", &args)?;
    }
    Ok(())
}

fn run_post_update_checks() -> Result<()> {
    println!("xtask release-tag: checking generated files…");
    for args in [
        vec!["fmt", "--all", "--check"],
        vec!["xtask", "openapi", "--check"],
    ] {
        run_command(&format!("cargo {}", args.join(" ")), "cargo", &args)?;
    }
    Ok(())
}

fn bump_cargo_toml(version: &str) -> Result<()> {
    bump_cargo_toml_at(Path::new("Cargo.toml"), version)
}

fn bump_cargo_toml_at(path: &Path, version: &str) -> Result<()> {
    let content =
        fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let mut updated = false;
    let mut output = String::with_capacity(content.len());
    let mut in_workspace_package = false;

    for line in content.lines() {
        let trimmed = line.trim_start();
        if trimmed == "[workspace.package]" {
            in_workspace_package = true;
        } else if trimmed.starts_with('[') && in_workspace_package {
            in_workspace_package = false;
        }

        if in_workspace_package && !updated && trimmed.starts_with("version") {
            let key_end = line
                .find("version")
                .context("locating workspace version key")?
                + "version".len();
            output.push_str(&line[..key_end]);
            output.push_str(&format!("    = \"{version}\"\n"));
            updated = true;
        } else {
            output.push_str(line);
            output.push('\n');
        }
    }

    if !updated {
        bail!(
            "could not find workspace.package version line in {}",
            path.display()
        );
    }
    fs::write(path, output).with_context(|| format!("writing {}", path.display()))?;
    println!("xtask release-tag: bumped Cargo.toml to {version}");
    Ok(())
}

fn regen_openapi() -> Result<()> {
    run_command("cargo xtask openapi", "cargo", &["xtask", "openapi"])
}

fn refresh_cargo_lock() -> Result<()> {
    println!("xtask release-tag: refreshing local package versions in Cargo.lock");
    let status = Command::new("cargo")
        .args(["metadata", "--offline", "--format-version", "1"])
        .stdout(Stdio::null())
        .status()
        .context("running cargo metadata --offline to refresh Cargo.lock")?;
    if !status.success() {
        bail!("cargo metadata --offline failed (Cargo.lock may need a network refresh)");
    }
    Ok(())
}

fn roll_changelog(version: &str) -> Result<()> {
    let path = "CHANGELOG.md";
    let content = fs::read_to_string(path).with_context(|| format!("reading {path}"))?;
    let today = today_iso()?;

    let mut output = String::with_capacity(content.len() + 256);
    let mut rolled_section = false;
    let mut rolled_links = false;
    for line in content.lines() {
        if !rolled_section && line.trim() == "## [Unreleased]" {
            output.push_str(
                "## [Unreleased]\n\n### Added\n\n(none)\n\n### Changed\n\n(none)\n\n### Fixed\n\n(none)\n\n",
            );
            output.push_str(&format!("## [{version}] — {today}\n"));
            rolled_section = true;
            continue;
        }
        if !rolled_links && line.starts_with("[Unreleased]: ") {
            output.push_str(&format!(
                "[Unreleased]: https://github.com/knievel-ads/knievel/compare/v{version}...HEAD\n"
            ));
            let previous = previous_tag_or_default()?;
            output.push_str(&format!(
                "[{version}]: https://github.com/knievel-ads/knievel/compare/{previous}...v{version}\n"
            ));
            rolled_links = true;
            continue;
        }
        output.push_str(line);
        output.push('\n');
    }

    if !rolled_section {
        bail!("could not find `## [Unreleased]` heading in {path}");
    }
    if !rolled_links {
        bail!("could not find `[Unreleased]: …` link in {path}");
    }
    fs::write(path, output).with_context(|| format!("writing {path}"))?;
    println!("xtask release-tag: rolled CHANGELOG.md to [{version}] — {today}");
    Ok(())
}

fn today_iso() -> Result<String> {
    let output = Command::new("date")
        .arg("+%Y-%m-%d")
        .output()
        .context("reading the current date")?;
    if !output.status.success() {
        bail!("date +%Y-%m-%d failed");
    }
    let date = String::from_utf8(output.stdout).context("date returned non-UTF-8 output")?;
    let date = date.trim();
    if date.len() != 10 {
        bail!("date returned unexpected value `{date}`");
    }
    Ok(date.to_owned())
}

fn previous_tag_or_default() -> Result<String> {
    let tags = git_output(&["tag", "--list", "v*", "--sort=-v:refname"])?;
    Ok(tags
        .lines()
        .find(|tag| {
            tag.strip_prefix('v')
                .is_some_and(|version| parse_semver(version).is_ok())
        })
        .unwrap_or("v0.0.0")
        .to_owned())
}

fn commit_preparation(version: &str, branch: &str) -> Result<()> {
    let current = git_output(&["branch", "--show-current"])?;
    if current != branch || current == "main" {
        bail!("refusing to commit release files outside `{branch}`");
    }
    run_command(
        "git add release files",
        "git",
        &[
            "add",
            "Cargo.toml",
            "Cargo.lock",
            "openapi.yaml",
            "CHANGELOG.md",
        ],
    )?;
    run_command(
        "git commit release preparation",
        "git",
        &["commit", "-m", &format!("release: prepare v{version}")],
    )
}

fn print_next_steps(version: &str, branch: &str) {
    println!();
    println!("Review the generated diff, then publish the release PR:");
    println!("    git push -u origin {branch}");
    println!("    gh pr create --base main --head {branch} --title 'release: prepare v{version}'");
    println!();
    println!("Only after that PR is reviewed, green, and merged into protected main:");
    println!("    git switch main");
    println!("    git pull --ff-only origin main");
    println!("    git tag -a v{version} -m 'Release v{version}'");
    println!("    cargo xtask release-preflight v{version}");
    println!("    git push origin v{version}");
    println!();
    println!("Tag creation is non-idempotent and starts release side effects.");
}

fn git_output(args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .output()
        .with_context(|| format!("running git {}", args.join(" ")))?;
    if !output.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn run_command(label: &str, program: &str, args: &[&str]) -> Result<()> {
    eprintln!("  $ {program} {}", args.join(" "));
    let status = Command::new(program)
        .args(args)
        .status()
        .with_context(|| format!("running {label}"))?;
    if !status.success() {
        bail!("command failed: {label}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_ID: AtomicU64 = AtomicU64::new(0);

    #[test]
    fn semver_accepts_canonical_xyz() {
        parse_semver("0.1.0").unwrap();
        parse_semver("10.20.300").unwrap();
    }

    #[test]
    fn semver_rejects_noncanonical_input() {
        for version in ["0.1", "v0.1.0", "0.1.0-rc.1", "0.1.x", "00.1.0", "0.01.0"] {
            assert!(parse_semver(version).is_err(), "accepted {version}");
        }
    }

    #[test]
    fn cargo_toml_bump_only_changes_workspace_version() {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "knievel-release-tag-bump-{}-{id}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Cargo.toml");
        fs::write(
            &path,
            "[workspace]\nresolver = \"2\"\n\n[workspace.package]\nedition = \"2021\"\nversion    = \"0.1.0\"\n\n[package]\nname = \"x\"\nversion.workspace = true\n",
        )
        .unwrap();

        bump_cargo_toml_at(&path, "0.1.7").unwrap();
        let updated = fs::read_to_string(&path).unwrap();
        assert!(updated.contains("version    = \"0.1.7\""));
        assert!(updated.contains("version.workspace = true"));
        fs::remove_dir_all(dir).unwrap();
    }
}
