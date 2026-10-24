#![cfg(windows)]
//! Uninstall entries of this user's, made for the test and taken away after.

use std::process::Command;

use harbormaster_platform::{Registry, Scope, entry, installed};

const ENTRIES: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Uninstall";

/// An uninstall entry with `values`, removed when dropped.
struct Entry(String);

impl Entry {
    fn made(name: &str, values: &[(&str, &str, &str)]) -> Self {
        let key = format!(r"{ENTRIES}\{name}-{}", std::process::id());
        for (value, kind, data) in values {
            let made = Command::new("reg")
                .args(["add", &key, "/v", value, "/t", kind, "/d", data, "/f"])
                .output()
                .expect("reg");
            assert!(made.status.success(), "{made:?}");
        }
        Self(key)
    }

    fn name(&self) -> &str {
        self.0.rsplit('\\').next().expect("a key name")
    }
}

impl Drop for Entry {
    fn drop(&mut self) {
        let _ = Command::new("reg").args(["delete", &self.0, "/f"]).output();
    }
}

#[test]
fn lists_a_program_installed_for_this_user_but_not_a_part_of_windows() {
    let tool = Entry::made(
        "HarbormasterTool",
        &[
            ("DisplayName", "REG_SZ", "Harbormaster Test Tool"),
            (
                "UninstallString",
                "REG_SZ",
                r#""C:\Tools\uninstall.exe" /remove"#,
            ),
            ("Publisher", "REG_SZ", "Example"),
            ("EstimatedSize", "REG_DWORD", "2048"),
        ],
    );
    let part = Entry::made(
        "HarbormasterPart",
        &[
            ("DisplayName", "REG_SZ", "Harbormaster Test Part"),
            ("UninstallString", "REG_SZ", r#""C:\Tools\part.exe""#),
            ("SystemComponent", "REG_DWORD", "1"),
        ],
    );

    let found = installed().unwrap();

    let listed = found
        .iter()
        .find(|program| program.key == tool.name())
        .expect("the tool");
    assert_eq!(listed.name, "Harbormaster Test Tool");
    assert_eq!(listed.uninstall, r#""C:\Tools\uninstall.exe" /remove"#);
    assert_eq!(listed.publisher.as_deref(), Some("Example"));
    assert_eq!(listed.estimated, Some(2048 * 1024));
    assert_eq!(
        listed.registry,
        Registry {
            scope: Scope::User,
            wide: true
        }
    );
    assert!(found.iter().all(|program| program.key != part.name()));
    assert_eq!(
        entry(listed.registry, tool.name()).unwrap().as_ref(),
        Some(listed)
    );
}
