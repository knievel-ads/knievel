//! Read-only validation for the tag release boundary.
//!
//! This command never edits the checkout. The release workflow performs an
//! equivalent repository-data check before it executes any repository code;
//! this implementation gives maintainers a locally runnable, unit-tested
//! version of the same contract.

use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{bail, Context, Result};
use regex::Regex;

#[derive(Debug)]
pub struct Args {
    pub tag: String,
    pub main_ref: String,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ReleaseVersion {
    major: u64,
    minor: u64,
    patch: u64,
}

impl fmt::Display for ReleaseVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

impl ReleaseVersion {
    fn parse_tag(tag: &str) -> Result<Self> {
        let version = tag
            .strip_prefix('v')
            .with_context(|| format!("release tag must start with `v` (got `{tag}`)"))?;
        Self::parse(version).with_context(|| format!("invalid release tag `{tag}`"))
    }

    fn parse(version: &str) -> Result<Self> {
        let parts: Vec<_> = version.split('.').collect();
        if parts.len() != 3 {
            bail!("version must be exactly `MAJOR.MINOR.PATCH`");
        }
        let mut values = [0_u64; 3];
        for (index, part) in parts.iter().enumerate() {
            if part.is_empty()
                || !part.bytes().all(|byte| byte.is_ascii_digit())
                || (part.len() > 1 && part.starts_with('0'))
            {
                bail!("version components must be decimal integers without leading zeros");
            }
            values[index] = part
                .parse::<u64>()
                .with_context(|| format!("version component `{part}` is out of range"))?;
        }
        Ok(Self {
            major: values[0],
            minor: values[1],
            patch: values[2],
        })
    }
}

pub fn run(args: Args) -> Result<()> {
    let version = ReleaseVersion::parse_tag(&args.tag)?;
    let head = git_output(&["rev-parse", "HEAD"])?;
    let tagged = git_output(&["rev-parse", &format!("{}^{{commit}}", args.tag)])?;
    if head != tagged {
        bail!(
            "tag {} resolves to {}, but the checked-out commit is {}",
            args.tag,
            tagged,
            head
        );
    }

    let ancestry = Command::new("git")
        .args(["merge-base", "--is-ancestor", &head, &args.main_ref])
        .status()
        .with_context(|| format!("checking whether {head} is on {}", args.main_ref))?;
    if !ancestry.success() {
        bail!(
            "tagged commit {head} is not reachable from {}; refusing release",
            args.main_ref
        );
    }

    let tags = git_output(&["tag", "--list", "v*"])?;
    ensure_newer_than_existing(version, &args.tag, tags.lines())?;

    validate_release_files(Path::new("."), &args.tag)?;
    println!(
        "release preflight passed: {} at {} is on {} and all release metadata agrees",
        args.tag, head, args.main_ref
    );
    Ok(())
}

pub(crate) fn validate_proposed_tag(tag: &str) -> Result<()> {
    let target = ReleaseVersion::parse_tag(tag)?;
    let tags = git_output(&["tag", "--list", "v*"])?;
    if tags.lines().any(|existing| existing == tag) {
        bail!("release tag {tag} already exists");
    }
    ensure_newer_than_existing(target, "", tags.lines())
}

fn ensure_newer_than_existing<'a>(
    target: ReleaseVersion,
    current_tag: &str,
    tags: impl Iterator<Item = &'a str>,
) -> Result<()> {
    let highest_prior = tags
        .filter(|tag| *tag != current_tag)
        .filter_map(|tag| ReleaseVersion::parse_tag(tag).ok())
        .max();
    if let Some(highest) = highest_prior {
        if target <= highest {
            bail!("release {target} is not newer than highest existing release {highest}");
        }
    }
    Ok(())
}

/// Validate all repository files that must agree with a canonical release tag.
///
/// Kept separate from the git checks so release preparation can verify its
/// generated files before a tag exists and fixture tests can exercise failures.
pub(crate) fn validate_release_files(root: &Path, tag: &str) -> Result<()> {
    let version = ReleaseVersion::parse_tag(tag)?;
    let version_text = version.to_string();

    let root_manifest_path = root.join("Cargo.toml");
    let root_manifest = read_toml(&root_manifest_path)?;
    let workspace_package = table_at(&root_manifest, &["workspace", "package"])?;
    require_string(
        workspace_package,
        "version",
        "workspace.package.version",
        &version_text,
    )?;
    require_string(
        workspace_package,
        "license",
        "workspace.package.license",
        "MIT",
    )?;

    let mut manifests = vec![root_manifest_path];
    let members = table_at(&root_manifest, &["workspace"])?
        .get("members")
        .and_then(toml::Value::as_array)
        .context("Cargo.toml workspace.members must be an array")?;
    for member in members {
        let member = member
            .as_str()
            .context("Cargo.toml workspace member must be a string")?;
        if member.contains(['*', '?', '[']) {
            bail!("release preflight does not accept globbed workspace member `{member}`");
        }
        manifests.push(root.join(member).join("Cargo.toml"));
    }

    let mut local_packages = BTreeSet::new();
    for manifest_path in manifests {
        let manifest = read_toml(&manifest_path)?;
        let package = table_at(&manifest, &["package"])
            .with_context(|| format!("{} has no [package] table", manifest_path.display()))?;
        let name = package
            .get("name")
            .and_then(toml::Value::as_str)
            .with_context(|| format!("{} package.name is missing", manifest_path.display()))?;
        if !local_packages.insert(name.to_owned()) {
            bail!("duplicate local package name `{name}`");
        }
        require_workspace_or_value(
            package,
            "version",
            &version_text,
            &format!("{} package.version", manifest_path.display()),
        )?;
        require_workspace_or_value(
            package,
            "license",
            "MIT",
            &format!("{} package.license", manifest_path.display()),
        )?;
    }

    let lock_path = root.join("Cargo.lock");
    let lock = read_toml(&lock_path)?;
    let lock_packages = lock
        .get("package")
        .and_then(toml::Value::as_array)
        .context("Cargo.lock has no package array")?;
    for name in &local_packages {
        let matches: Vec<_> = lock_packages
            .iter()
            .filter_map(toml::Value::as_table)
            .filter(|package| {
                package.get("name").and_then(toml::Value::as_str) == Some(name.as_str())
                    && package.get("source").is_none()
            })
            .collect();
        if matches.len() != 1 {
            bail!(
                "Cargo.lock must contain exactly one source-free local package `{name}`; found {}",
                matches.len()
            );
        }
        require_string(
            matches[0],
            "version",
            &format!("Cargo.lock package `{name}` version"),
            &version_text,
        )?;
    }

    let openapi_path = root.join("openapi.yaml");
    let openapi: serde_yaml::Value = serde_yaml::from_str(
        &fs::read_to_string(&openapi_path)
            .with_context(|| format!("reading {}", openapi_path.display()))?,
    )
    .with_context(|| format!("parsing {}", openapi_path.display()))?;
    let openapi_version = openapi
        .get("info")
        .and_then(|value| value.get("version"))
        .and_then(serde_yaml::Value::as_str)
        .context("openapi.yaml info.version must be a string")?;
    if openapi_version != version_text {
        bail!("openapi.yaml info.version is `{openapi_version}`, expected `{version_text}`");
    }

    validate_changelog(root, &version_text)?;
    validate_license(root)?;
    Ok(())
}

fn validate_changelog(root: &Path, version: &str) -> Result<()> {
    let path = root.join("CHANGELOG.md");
    let changelog =
        fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let escaped = regex::escape(version);
    let heading = Regex::new(&format!(
        r"(?m)^## \[{escaped}\] — [0-9]{{4}}-[0-9]{{2}}-[0-9]{{2}}$"
    ))?;
    if !heading.is_match(&changelog) {
        bail!("CHANGELOG.md is missing the dated `## [{version}]` release heading");
    }

    let version_link = Regex::new(&format!(
        r"(?m)^\[{escaped}\]: https://github\.com/knievel-ads/knievel/(?:compare/[^[:space:]]+\.\.\.v{escaped}|releases/tag/v{escaped})$"
    ))?;
    if !version_link.is_match(&changelog) {
        bail!("CHANGELOG.md is missing a canonical [{version}] release link");
    }

    let expected_unreleased =
        format!("[Unreleased]: https://github.com/knievel-ads/knievel/compare/v{version}...HEAD");
    if !changelog.lines().any(|line| line == expected_unreleased) {
        bail!("CHANGELOG.md [Unreleased] link must start at v{version}");
    }
    Ok(())
}

fn validate_license(root: &Path) -> Result<()> {
    let path = root.join("LICENSE");
    let license = fs::read_to_string(&path)
        .with_context(|| format!("MIT LICENSE is required at {}", path.display()))?;
    for marker in [
        "MIT License",
        "Permission is hereby granted, free of charge",
        "THE SOFTWARE IS PROVIDED \"AS IS\"",
    ] {
        if !license.contains(marker) {
            bail!("LICENSE is not recognizable MIT text (missing `{marker}`)");
        }
    }
    Ok(())
}

fn read_toml(path: &Path) -> Result<toml::Value> {
    let text = fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))
}

fn table_at<'a>(value: &'a toml::Value, path: &[&str]) -> Result<&'a toml::Table> {
    let mut current = value;
    for key in path {
        current = current
            .get(*key)
            .with_context(|| format!("missing TOML key {}", path.join(".")))?;
    }
    current
        .as_table()
        .with_context(|| format!("TOML key {} must be a table", path.join(".")))
}

fn require_string(table: &toml::Table, key: &str, label: &str, expected: &str) -> Result<()> {
    let actual = table
        .get(key)
        .and_then(toml::Value::as_str)
        .with_context(|| format!("{label} must be a string"))?;
    if actual != expected {
        bail!("{label} is `{actual}`, expected `{expected}`");
    }
    Ok(())
}

fn require_workspace_or_value(
    table: &toml::Table,
    key: &str,
    expected: &str,
    label: &str,
) -> Result<()> {
    match table.get(key) {
        Some(toml::Value::String(actual)) if actual == expected => Ok(()),
        Some(toml::Value::Table(inheritance))
            if inheritance.get("workspace").and_then(toml::Value::as_bool) == Some(true) =>
        {
            Ok(())
        }
        Some(other) => {
            bail!("{label} must be `{expected}` or inherit workspace metadata (got {other})")
        }
        None => bail!("{label} is missing"),
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT_ID: AtomicU64 = AtomicU64::new(0);

    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn valid() -> Self {
            let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "knievel-release-preflight-{}-{id}",
                std::process::id()
            ));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(root.join("xtask")).unwrap();
            fs::create_dir_all(root.join("testlib")).unwrap();
            fs::write(
                root.join("Cargo.toml"),
                r#"[workspace]
members = ["xtask", "testlib"]
[workspace.package]
version = "1.2.3"
license = "MIT"
[package]
name = "knievel"
version.workspace = true
license.workspace = true
"#,
            )
            .unwrap();
            for member in ["xtask", "testlib"] {
                fs::write(
                    root.join(member).join("Cargo.toml"),
                    format!(
                        "[package]\nname = \"{member}\"\nversion.workspace = true\nlicense.workspace = true\n"
                    ),
                )
                .unwrap();
            }
            fs::write(
                root.join("Cargo.lock"),
                r#"version = 4
[[package]]
name = "knievel"
version = "1.2.3"
[[package]]
name = "testlib"
version = "1.2.3"
[[package]]
name = "xtask"
version = "1.2.3"
"#,
            )
            .unwrap();
            fs::write(
                root.join("openapi.yaml"),
                "openapi: 3.0.0\ninfo:\n  title: knievel\n  version: 1.2.3\n",
            )
            .unwrap();
            fs::write(
                root.join("CHANGELOG.md"),
                "## [Unreleased]\n\n## [1.2.3] — 2026-08-20\n\n[Unreleased]: https://github.com/knievel-ads/knievel/compare/v1.2.3...HEAD\n[1.2.3]: https://github.com/knievel-ads/knievel/compare/v1.2.2...v1.2.3\n",
            )
            .unwrap();
            fs::write(
                root.join("LICENSE"),
                "MIT License\n\nPermission is hereby granted, free of charge, to any person obtaining a copy.\n\nTHE SOFTWARE IS PROVIDED \"AS IS\".\n",
            )
            .unwrap();
            Self { root }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn release_preflight_accepts_consistent_fixture() {
        let fixture = Fixture::valid();
        validate_release_files(&fixture.root, "v1.2.3").unwrap();
    }

    #[test]
    fn release_preflight_rejects_noncanonical_tags() {
        for tag in [
            "1.2.3",
            "v01.2.3",
            "v1.02.3",
            "v1.2.03",
            "v1.2",
            "v1.2.3-rc.1",
        ] {
            assert!(ReleaseVersion::parse_tag(tag).is_err(), "accepted {tag}");
        }
    }

    #[test]
    fn release_preflight_rejects_version_drift() {
        let fixture = Fixture::valid();
        fs::write(
            fixture.root.join("openapi.yaml"),
            "openapi: 3.0.0\ninfo:\n  title: knievel\n  version: 1.2.2\n",
        )
        .unwrap();
        let error = validate_release_files(&fixture.root, "v1.2.3").unwrap_err();
        assert!(error.to_string().contains("openapi.yaml info.version"));
    }

    #[test]
    fn release_preflight_rejects_missing_license() {
        let fixture = Fixture::valid();
        fs::remove_file(fixture.root.join("LICENSE")).unwrap();
        let error = validate_release_files(&fixture.root, "v1.2.3").unwrap_err();
        assert!(error.to_string().contains("MIT LICENSE is required"));
    }

    #[test]
    fn release_preflight_rejects_local_lock_version_drift() {
        let fixture = Fixture::valid();
        let lock = fs::read_to_string(fixture.root.join("Cargo.lock"))
            .unwrap()
            .replacen("version = \"1.2.3\"", "version = \"1.2.2\"", 1);
        fs::write(fixture.root.join("Cargo.lock"), lock).unwrap();
        let error = validate_release_files(&fixture.root, "v1.2.3").unwrap_err();
        assert!(error
            .to_string()
            .contains("Cargo.lock package `knievel` version"));
    }

    #[test]
    fn release_preflight_rejects_changelog_link_drift() {
        let fixture = Fixture::valid();
        let changelog = fs::read_to_string(fixture.root.join("CHANGELOG.md"))
            .unwrap()
            .replace("v1.2.2...v1.2.3", "v1.2.2...v1.2.4");
        fs::write(fixture.root.join("CHANGELOG.md"), changelog).unwrap();
        let error = validate_release_files(&fixture.root, "v1.2.3").unwrap_err();
        assert!(error.to_string().contains("canonical [1.2.3] release link"));
    }

    #[test]
    fn release_preflight_rejects_non_monotonic_version() {
        let target = ReleaseVersion::parse_tag("v1.2.3").unwrap();
        let error = ensure_newer_than_existing(
            target,
            "v1.2.3",
            ["v1.2.2", "v1.3.0", "not-semver"].into_iter(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("highest existing release 1.3.0"));
    }

    #[test]
    fn release_preflight_orders_versions_numerically() {
        assert!(
            ReleaseVersion::parse_tag("v2.0.0").unwrap()
                > ReleaseVersion::parse_tag("v1.99.99").unwrap()
        );
    }
}
