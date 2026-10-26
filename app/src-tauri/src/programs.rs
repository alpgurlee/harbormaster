//! The programs installed on Windows, as the cleaner window lists them, and
//! starting the uninstaller of one it then names, once the user confirmed.
//! None of them is ever taken to be removed the way a finding is. Which
//! uninstallers were started is remembered, so that a scan can look for
//! what an app gone since left behind.

use std::path::Path;
use std::sync::{Mutex, MutexGuard, PoisonError};

use harbormaster_cleaner::{App, Apps};
use harbormaster_platform::{Installed, NotStarted, Scope};
use serde::Serialize;

/// The programs last listed to the window, under the token it holds, and
/// those whose uninstaller was started from here.
#[derive(Debug, Default)]
pub(crate) struct Programs {
    last: Mutex<Listing>,
}

#[derive(Debug, Default)]
struct Listing {
    token: u64,
    listed: Vec<Installed>,
    started: Vec<Installed>,
}

/// The programs installed on this PC as the window shows them, under the
/// token starting one's uninstaller takes.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Listed {
    token: u64,
    programs: Vec<Program>,
}

/// A program as the window shows it; the window names it by `id` alone.
#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Program {
    id: u32,
    name: String,
    publisher: Option<String>,
    version: Option<String>,
    /// How much room its installer says it takes, if it says.
    estimated: Option<u64>,
    /// Installed for everyone on this PC rather than for this user alone,
    /// so its uninstaller may ask for an administrator.
    everyone: bool,
    /// The program its uninstaller is, as Windows reads its entry; none
    /// when Windows can't be sure, and then it isn't started.
    uninstaller: Option<String>,
}

/// Why an uninstaller wasn't started.
#[derive(Debug, PartialEq, Eq, Serialize)]
#[serde(
    tag = "code",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum Refusal {
    /// The listing the window names was replaced by a newer one.
    Expired,
    /// The window named a program its listing doesn't have.
    Unlisted,
    /// It is no longer installed.
    Gone,
    /// Its uninstall entry changed since it was listed.
    Changed,
    /// Windows can't tell for sure which program its uninstaller is.
    Unsure,
    /// It asks for an administrator, which an app of this user's alone shouldn't.
    WantsAdministrator,
    /// It asks for an administrator, yet isn't kept where only
    /// administrators can change it.
    Unprotected,
    /// The user declined Windows' prompt for an administrator.
    Declined,
    Failed {
        message: String,
    },
}

impl From<NotStarted> for Refusal {
    fn from(why: NotStarted) -> Self {
        match why {
            NotStarted::Gone => Self::Gone,
            NotStarted::Changed => Self::Changed,
            NotStarted::Unsure => Self::Unsure,
            NotStarted::WantsAdministrator => Self::WantsAdministrator,
            NotStarted::Unprotected => Self::Unprotected,
            NotStarted::Declined => Self::Declined,
            NotStarted::Failed(error) => Self::Failed {
                message: error.to_string(),
            },
        }
    }
}

impl Programs {
    /// Lists the programs installed on this PC in place of the last
    /// listing, under a new token.
    pub(crate) fn list(&self) -> std::io::Result<Listed> {
        Ok(self.keep(harbormaster_platform::installed()?))
    }

    /// Starts the uninstaller of the program `id` of the listing under
    /// `token`, if its entry is still as listed. Windows' prompt for an
    /// administrator, when it asks, belongs to the window `owner`.
    pub(crate) fn uninstall(
        &self,
        token: u64,
        id: u32,
        owner: Option<isize>,
    ) -> Result<(), Refusal> {
        let listed = self.listed(token, id)?;
        // Started once the lock is let go: Windows may wait for the user to answer it.
        harbormaster_platform::start_uninstaller(&listed, owner)?;
        self.remember(listed);
        Ok(())
    }

    /// What a scan needs to know of the PC's apps: which were removed from
    /// here, which are still installed, and what else names them. Nothing,
    /// when what is installed can't be read in full: a scan told less than
    /// that could take an installed app's data for a removed app's.
    pub(crate) fn known(&self) -> Option<Apps> {
        let installed = harbormaster_platform::every_installed().ok()?;
        let mut names = harbormaster_platform::app_paths().ok()?;
        for folder in harbormaster_platform::program_folders() {
            names.extend(names_in(&folder, 2));
        }
        Some(Apps {
            removed: self.removed(|app| harbormaster_platform::still_there(app.registry, &app.key)),
            installed: installed.iter().map(described).collect(),
            names,
        })
    }

    /// Keeps `installed` as one whose uninstaller was started.
    fn remember(&self, installed: Installed) {
        let mut last = self.lock();
        let same =
            |other: &Installed| other.registry == installed.registry && other.key == installed.key;
        if !last.started.iter().any(same) {
            last.started.push(installed);
        }
    }

    /// Of the apps whose uninstaller was started, those gone from the
    /// installed apps since, by `still_there`, as the cleaner names them.
    fn removed(&self, still_there: impl Fn(&Installed) -> std::io::Result<bool>) -> Vec<App> {
        let started = self.lock().started.clone();
        started
            .iter()
            // One Windows won't say about is taken to be there still.
            .filter(|app| !still_there(app).unwrap_or(true))
            .map(described)
            .collect()
    }

    /// Keeps `listed` as the listing shown from now on, and hands it back as
    /// the window shows it.
    fn keep(&self, listed: Vec<Installed>) -> Listed {
        let programs = listed
            .iter()
            .zip(0..)
            .map(|(installed, id)| Program::of(id, installed))
            .collect();
        let mut last = self.lock();
        last.token += 1;
        last.listed = listed;
        Listed {
            token: last.token,
            programs,
        }
    }

    fn listed(&self, token: u64, id: u32) -> Result<Installed, Refusal> {
        let last = self.lock();
        if last.token != token {
            return Err(Refusal::Expired);
        }
        let id = usize::try_from(id).map_err(|_| Refusal::Unlisted)?;
        last.listed.get(id).cloned().ok_or(Refusal::Unlisted)
    }

    fn lock(&self) -> MutexGuard<'_, Listing> {
        self.last.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// `installed` as the cleaner names an app: by its entry's name and
/// publisher, and the folders it was installed in and ran from.
fn described(installed: &Installed) -> App {
    App {
        name: installed.name.clone(),
        publisher: installed.publisher.clone(),
        folders: installed
            .location
            .iter()
            .cloned()
            .chain(harbormaster_platform::uninstaller(installed))
            .chain(installed.icon.iter().cloned())
            .collect(),
    }
}

/// The names of what is in `folder`, `levels` deep: its folders, and its
/// files without their extension, as a shortcut is named after its program.
fn names_in(folder: &Path, levels: usize) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return Vec::new();
    };
    let mut names = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if let Some(stem) = path.file_stem() {
            names.push(stem.to_string_lossy().into_owned());
        }
        if levels > 1 && entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            names.extend(names_in(&path, levels - 1));
        }
    }
    names
}

impl Program {
    fn of(id: u32, installed: &Installed) -> Self {
        Self {
            id,
            name: installed.name.clone(),
            publisher: installed.publisher.clone(),
            version: installed.version.clone(),
            estimated: installed.estimated,
            everyone: installed.registry.scope == Scope::Machine,
            uninstaller: harbormaster_platform::uninstaller(installed)
                .map(|program| program.to_string_lossy().into_owned()),
        }
    }
}

#[cfg(test)]
mod tests {
    use harbormaster_platform::Registry;

    use super::*;

    fn installed(key: &str, scope: Scope) -> Installed {
        Installed {
            registry: Registry { scope, wide: true },
            key: key.to_owned(),
            name: format!("{key} App"),
            publisher: Some("Example".to_owned()),
            version: Some("1.2".to_owned()),
            location: None,
            estimated: Some(4096),
            icon: None,
            uninstall: r#""C:\Windows\System32\whoami.exe""#.to_owned(),
        }
    }

    #[test]
    fn hands_out_only_a_program_of_the_last_listing() {
        let programs = Programs::default();
        let first = programs.keep(vec![installed("Old", Scope::User)]);
        let last = programs.keep(vec![
            installed("New", Scope::User),
            installed("Other", Scope::Machine),
        ]);

        assert_eq!(programs.listed(first.token, 0), Err(Refusal::Expired));
        assert_eq!(programs.listed(last.token, 2), Err(Refusal::Unlisted));
        assert_eq!(
            programs.listed(last.token, 1).map(|program| program.key),
            Ok("Other".to_owned())
        );
    }

    #[test]
    fn remembers_what_was_started_and_tells_what_is_gone_since() {
        let programs = Programs::default();
        programs.remember(installed("Gone", Scope::User));
        programs.remember(installed("Gone", Scope::User));
        programs.remember(installed("Kept", Scope::Machine));
        programs.keep(Vec::new());

        let removed = programs.removed(|app| Ok(app.key == "Kept"));

        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0].name, "Gone App");
        assert_eq!(removed[0].publisher.as_deref(), Some("Example"));
        assert_eq!(
            removed[0]
                .folders
                .iter()
                .map(|folder| folder.to_string_lossy().to_lowercase())
                .collect::<Vec<_>>(),
            [r"c:\windows\system32\whoami.exe"]
        );
        assert!(
            programs
                .removed(|_| Err(std::io::Error::other("no registry")))
                .is_empty()
        );
    }

    #[test]
    fn names_what_a_folder_holds_two_levels_down() {
        let place = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(place.path().join("Docker").join("Docker")).unwrap();
        std::fs::write(place.path().join("Docker Desktop.lnk"), b"").unwrap();
        std::fs::create_dir_all(place.path().join("Vendor").join("Deep").join("Deeper")).unwrap();

        let mut names = names_in(place.path(), 2);
        names.sort();

        assert_eq!(
            names,
            ["Deep", "Docker", "Docker", "Docker Desktop", "Vendor"]
        );
    }

    #[test]
    fn tells_the_window_what_it_expects() {
        let programs = Programs::default();
        let listed = programs.keep(vec![installed("Tool", Scope::Machine)]);

        let mut told = serde_json::to_value(&listed).unwrap();
        // Windows may spell the folders it finds its own way.
        let uninstaller = told["programs"][0]["uninstaller"].take();
        assert_eq!(
            uninstaller.as_str().map(str::to_lowercase).as_deref(),
            Some(r"c:\windows\system32\whoami.exe")
        );
        assert_eq!(
            told,
            serde_json::json!({
                "token": 1,
                "programs": [{
                    "id": 0,
                    "name": "Tool App",
                    "publisher": "Example",
                    "version": "1.2",
                    "estimated": 4096,
                    "everyone": true,
                    "uninstaller": null,
                }],
            })
        );
        assert_eq!(
            serde_json::to_value(Refusal::WantsAdministrator).unwrap(),
            serde_json::json!({"code": "wantsAdministrator"})
        );
    }
}
