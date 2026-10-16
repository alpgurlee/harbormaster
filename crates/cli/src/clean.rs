//! The cleaner's dry run: what could go, and how much room it would make.

use std::path::Path;

use harbormaster_cleaner::{Finding, Group, Kind};

use crate::format;

const NAME_WIDTH: usize = 32;

/// The findings as the JSON a script reads; fails on a path JSON can't hold.
pub(crate) fn document(findings: &[Finding]) -> serde_json::Result<String> {
    let findings = serde_json::to_value(findings)?;
    serde_json::to_string_pretty(&serde_json::json!({
        "schema": "harbormaster/v1",
        "findings": findings,
    }))
}

/// The table of findings by group, then what picking would free.
pub(crate) fn lines(findings: &[Finding], home: &Path) -> Vec<String> {
    if findings.is_empty() {
        return vec!["Nothing to clean up.".into()];
    }
    let mut rows = vec![["WHAT", "SIZE", "WHERE", ""].map(String::from).to_vec()];
    let mut group = None;
    for finding in findings {
        if group != Some(finding.group) {
            group = Some(finding.group);
            rows.push(vec![title(finding.group).into()]);
        }
        rows.push(vec![
            format!("  {}", format::truncate(&finding.name, NAME_WIDTH)),
            format::bytes(finding.bytes),
            shown(&finding.path, home),
            note(finding).into(),
        ]);
    }
    let picked: u64 = findings
        .iter()
        .filter(|finding| finding.picked)
        .map(|finding| finding.bytes)
        .sum();
    let all: u64 = findings.iter().map(|finding| finding.bytes).sum();
    let mut lines: Vec<String> = format::table(&rows).lines().map(String::from).collect();
    lines.push(String::new());
    lines.push(format!(
        "Would free {} of {}. Nothing was removed: this is a dry run.",
        format::bytes(picked),
        format::bytes(all)
    ));
    lines
}

fn title(group: Group) -> &'static str {
    match group {
        Group::Packages => "Packages",
        Group::Xcode => "Xcode",
        Group::Apps => "App caches",
        Group::Logs => "Logs",
        Group::Trash => "Trash",
    }
}

/// Why a finding isn't picked, if it isn't.
fn note(finding: &Finding) -> &'static str {
    match (finding.kind, finding.picked) {
        (Kind::Rebuildable, true) => "",
        (Kind::Rebuildable, false) => "used last",
        (Kind::Personal, _) => "your own work",
        (Kind::Discarded, _) => "empties for good",
    }
}

/// `~/.npm/_cacache` for a path in the home folder.
fn shown(path: &Path, home: &Path) -> String {
    path.strip_prefix(home).map_or_else(
        |_| path.display().to_string(),
        |inside| format!("~/{}", inside.display()),
    )
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use harbormaster_cleaner::{Finding, Group, Kind};

    use super::lines;

    const GIB: u64 = 1024 * 1024 * 1024;

    fn finding(
        group: Group,
        kind: Kind,
        name: &str,
        path: &str,
        bytes: u64,
        picked: bool,
    ) -> Finding {
        Finding {
            group,
            kind,
            name: name.into(),
            path: PathBuf::from(path),
            bytes,
            picked,
        }
    }

    #[test]
    fn lists_each_group_and_what_picking_would_free() {
        let findings = [
            finding(
                Group::Packages,
                Kind::Rebuildable,
                "npm",
                "/Users/dev/.npm/_cacache",
                2 * GIB,
                true,
            ),
            finding(
                Group::Xcode,
                Kind::Rebuildable,
                "17.5 (21F79)",
                "/Users/dev/Library/Developer/Xcode/iOS DeviceSupport/17.5 (21F79)",
                GIB,
                false,
            ),
            finding(
                Group::Trash,
                Kind::Discarded,
                "Trash",
                "/Users/dev/.Trash",
                GIB,
                false,
            ),
        ];

        let text = lines(&findings, Path::new("/Users/dev")).join("\n");

        assert!(text.contains("Packages"), "{text}");
        assert!(
            text.contains("npm") && text.contains("2.0 GB") && text.contains("~/.npm/_cacache"),
            "{text}"
        );
        assert!(text.contains("used last"), "{text}");
        assert!(text.contains("empties for good"), "{text}");
        assert!(text.contains("Would free 2.0 GB of 4.0 GB"), "{text}");
        assert!(text.contains("dry run"), "{text}");
    }

    #[test]
    fn reports_a_path_json_cant_hold_instead_of_panicking() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;

        let mut odd = finding(Group::Logs, Kind::Rebuildable, "Logs", "/", 1, true);
        odd.path = PathBuf::from(OsStr::from_bytes(b"/Users/\xff/Library/Logs"));

        assert!(super::document(&[odd]).is_err());
    }

    #[test]
    fn says_so_when_there_is_nothing() {
        assert_eq!(
            lines(&[], Path::new("/Users/dev")),
            ["Nothing to clean up."]
        );
    }
}
