use std::io::Read;
use std::path::{Path, PathBuf};

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub name: String,
    pub root: PathBuf,
    pub repo_root: Option<PathBuf>,
    pub framework: Option<Framework>,
    pub branch: Option<String>,
    pub worktree: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Framework {
    pub name: String,
    pub major: Option<u32>,
}

/// The project a working directory belongs to: the nearest manifest, plus the
/// git repository around it. Never looks at `home` itself or above it.
pub(crate) fn resolve(start: &Path, home: &Path) -> Option<Project> {
    if is_tool_installation(start) {
        return None;
    }
    let mut manifest: Option<(PathBuf, Manifest)> = None;
    let mut git: Option<(PathBuf, GitInfo)> = None;
    let mut folder = Some(start);
    while let Some(current) = folder {
        if current == home || current.parent().is_none() {
            break;
        }
        if manifest.is_none() {
            manifest = read_manifest(current).map(|found| (current.to_path_buf(), found));
        }
        if git.is_none() {
            git = read_git(current).map(|found| (current.to_path_buf(), found));
        }
        if manifest.is_some() && git.is_some() {
            break;
        }
        folder = current.parent();
    }

    let (root, manifest) = match manifest {
        Some(found) => found,
        None => (git.as_ref()?.0.clone(), Manifest::default()),
    };
    let name = manifest.name.unwrap_or_else(|| folder_name(&root));
    let (repo_root, git) = git.unzip();
    Some(Project {
        name: shortened(&name, NAME_LIMIT),
        root,
        repo_root,
        framework: manifest.framework,
        branch: git
            .as_ref()
            .and_then(|git| git.branch.as_deref())
            .map(|branch| shortened(branch, BRANCH_LIMIT)),
        worktree: git.is_some_and(|git| git.worktree),
    })
}

/// A manifest or HEAD file can hold anything; nothing longer is shown.
const NAME_LIMIT: usize = 100;
const BRANCH_LIMIT: usize = 255;

fn shortened(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

/// Folders that hold installed tools rather than projects. Homebrew, for one,
/// is a git repository, so a database started from it must not look like a
/// project on the `stable` branch.
const TOOL_FOLDERS: &[&str] = &[
    "homebrew",
    "cellar",
    "program files",
    "program files (x86)",
    "programdata",
    ".cargo",
    ".rustup",
];
const SYSTEM_ROOTS: &[&str] = &[
    "/usr",
    "/bin",
    "/sbin",
    "/System",
    "/Library",
    "/Applications",
];

fn is_tool_installation(path: &Path) -> bool {
    SYSTEM_ROOTS.iter().any(|root| path.starts_with(root))
        || path.components().any(|component| {
            let name = component.as_os_str().to_string_lossy().to_lowercase();
            TOOL_FOLDERS.contains(&name.as_str())
        })
}

#[derive(Debug, Default)]
struct Manifest {
    name: Option<String>,
    framework: Option<Framework>,
}

#[derive(Debug)]
struct GitInfo {
    branch: Option<String>,
    worktree: bool,
}

fn folder_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Manifests and git files are small; anything bigger is not one we trust.
const SMALL_FILE_LIMIT: u64 = 1024 * 1024;

/// Reads a file from a folder another process chose, so it may be a FIFO, a
/// device or something huge. Opening never blocks and only regular files under
/// the size limit are read.
fn read_small_file(path: &Path) -> Option<String> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = options.open(path).ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() || metadata.len() > SMALL_FILE_LIMIT {
        return None;
    }
    let mut text = String::new();
    file.take(SMALL_FILE_LIMIT).read_to_string(&mut text).ok()?;
    Some(text)
}

fn read_manifest(folder: &Path) -> Option<Manifest> {
    let text = |file: &str| read_small_file(&folder.join(file));
    // A manifest that exists but can't be trusted still marks the folder as a project.
    let parsed = |file: &str, parse: fn(&str) -> Manifest| {
        folder.join(file).exists().then(|| {
            text(file)
                .map(|contents| parse(&contents))
                .unwrap_or_default()
        })
    };
    for (file, parse) in [
        ("package.json", package_json as fn(&str) -> Manifest),
        ("Cargo.toml", cargo_toml),
        ("go.mod", go_mod),
        ("pyproject.toml", pyproject),
    ] {
        if let Some(manifest) = parsed(file, parse) {
            return Some(manifest);
        }
    }
    if let Some(requirements) = text("requirements.txt") {
        return Some(Manifest {
            name: None,
            framework: python_framework(requirements.lines()),
        });
    }
    if let Some(gemfile) = text("Gemfile") {
        let rails = gemfile.lines().any(|line| {
            let line = line.trim_start();
            line.starts_with("gem \"rails\"") || line.starts_with("gem 'rails'")
        });
        return Some(Manifest {
            name: None,
            framework: Some(named(if rails { "Rails" } else { "Ruby" })),
        });
    }
    if let Some(json) = text("composer.json") {
        return Some(composer_json(&json));
    }
    if text("deno.json").is_some() || text("deno.jsonc").is_some() {
        return Some(Manifest {
            name: None,
            framework: Some(named("Deno")),
        });
    }
    if let Some(mix) = text("mix.exs") {
        let phoenix = mix.contains(":phoenix");
        return Some(Manifest {
            name: None,
            framework: Some(named(if phoenix { "Phoenix" } else { "Elixir" })),
        });
    }
    let java = ["pom.xml", "build.gradle", "build.gradle.kts"]
        .into_iter()
        .find_map(text);
    if let Some(build) = java {
        let spring = build.contains("spring-boot");
        return Some(Manifest {
            name: None,
            framework: Some(named(if spring { "Spring Boot" } else { "Java" })),
        });
    }
    dotnet_project(folder)
}

fn package_json(json: &str) -> Manifest {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
        return Manifest::default();
    };
    let name = value
        .get("name")
        .and_then(|name| name.as_str())
        .map(str::to_string);
    let version_of = |package: &str| {
        ["dependencies", "devDependencies"]
            .iter()
            .find_map(|section| {
                value
                    .get(section)?
                    .get(package)?
                    .as_str()
                    .map(str::to_string)
            })
    };
    let framework = JS_FRAMEWORKS.iter().find_map(|(package, label)| {
        version_of(package).map(|version| Framework {
            name: (*label).to_string(),
            major: major(&version),
        })
    });
    Manifest { name, framework }
}

const JS_FRAMEWORKS: &[(&str, &str)] = &[
    ("next", "Next.js"),
    ("nuxt", "Nuxt"),
    ("@react-router/dev", "React Router"),
    ("@remix-run/dev", "Remix"),
    ("astro", "Astro"),
    ("@sveltejs/kit", "SvelteKit"),
    ("expo", "Expo"),
    ("@angular/core", "Angular"),
    ("@nestjs/core", "NestJS"),
    ("vite", "Vite"),
    ("react-scripts", "Create React App"),
    ("hono", "Hono"),
    ("fastify", "Fastify"),
    ("express", "Express"),
    ("storybook", "Storybook"),
];

fn cargo_toml(text: &str) -> Manifest {
    let Ok(table) = text.parse::<toml::Table>() else {
        return Manifest::default();
    };
    let name = table
        .get("package")
        .and_then(|package| package.get("name"))
        .and_then(|name| name.as_str());
    let dependencies = table.get("dependencies").and_then(|deps| deps.as_table());
    let framework = [
        ("axum", "Axum"),
        ("actix-web", "Actix Web"),
        ("rocket", "Rocket"),
        ("leptos", "Leptos"),
    ]
    .iter()
    .find_map(|(crate_name, label)| {
        let spec = dependencies?.get(*crate_name)?;
        let version = spec
            .as_str()
            .or_else(|| spec.get("version")?.as_str())
            .unwrap_or_default();
        Some(Framework {
            name: (*label).to_string(),
            major: major(version),
        })
    })
    .or_else(|| Some(named("Rust")));
    Manifest {
        name: name.map(str::to_string),
        framework,
    }
}

fn go_mod(text: &str) -> Manifest {
    let module = text
        .lines()
        .find_map(|line| line.trim().strip_prefix("module "))
        .map(str::trim);
    let name = module.and_then(|module| {
        let mut segments = module.rsplit('/');
        let last = segments.next()?;
        // `example.com/gateway/v2` is still called gateway.
        let is_major_suffix = last.len() > 1
            && last.starts_with('v')
            && last[1..].chars().all(|c| c.is_ascii_digit());
        Some(
            if is_major_suffix {
                segments.next().unwrap_or(last)
            } else {
                last
            }
            .to_string(),
        )
    });
    let framework = [
        ("github.com/gin-gonic/gin", "Gin"),
        ("github.com/labstack/echo", "Echo"),
        ("github.com/gofiber/fiber", "Fiber"),
        ("github.com/go-chi/chi", "Chi"),
    ]
    .iter()
    .find_map(|(path, label)| {
        let line = text.lines().find(|line| line.contains(path))?;
        let version = line
            .split_whitespace()
            .find(|word| word.starts_with('v'))
            .unwrap_or_default();
        Some(Framework {
            name: (*label).to_string(),
            major: major(version),
        })
    })
    .or_else(|| Some(named("Go")));
    Manifest { name, framework }
}

fn pyproject(text: &str) -> Manifest {
    let Ok(table) = text.parse::<toml::Table>() else {
        return Manifest::default();
    };
    let project = table.get("project");
    let poetry = table.get("tool").and_then(|tool| tool.get("poetry"));
    let name = project
        .and_then(|project| project.get("name"))
        .or_else(|| poetry.and_then(|poetry| poetry.get("name")))
        .and_then(|name| name.as_str())
        .map(str::to_string);
    let mut requirements: Vec<String> = project
        .and_then(|project| project.get("dependencies")?.as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(|requirement| requirement.as_str().map(str::to_string))
        .collect();
    if let Some(dependencies) = poetry.and_then(|poetry| poetry.get("dependencies")?.as_table()) {
        requirements.extend(
            dependencies
                .iter()
                .map(|(name, spec)| format!("{name}{}", spec.as_str().unwrap_or_default())),
        );
    }
    let framework =
        python_framework(requirements.iter().map(String::as_str)).or_else(|| Some(named("Python")));
    Manifest { name, framework }
}

fn python_framework<'a>(requirements: impl Iterator<Item = &'a str>) -> Option<Framework> {
    let requirements: Vec<String> = requirements
        .map(|line| line.trim().to_lowercase())
        .collect();
    [
        ("django", "Django"),
        ("fastapi", "FastAPI"),
        ("flask", "Flask"),
        ("starlette", "Starlette"),
        ("litestar", "Litestar"),
    ]
    .iter()
    .find_map(|(package, label)| {
        let requirement = requirements.iter().find(|line| {
            line.strip_prefix(package).is_some_and(|rest| {
                rest.is_empty()
                    || !rest.starts_with(|c: char| c.is_alphanumeric() || c == '-' || c == '_')
            })
        })?;
        Some(Framework {
            name: (*label).to_string(),
            major: major(&requirement[package.len()..]),
        })
    })
}

fn composer_json(json: &str) -> Manifest {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
        return Manifest::default();
    };
    let name = value
        .get("name")
        .and_then(|name| name.as_str())
        .map(|name| name.rsplit('/').next().unwrap_or(name).to_string());
    let requires = |package: &str| {
        value
            .get("require")
            .and_then(|require| require.get(package))
            .is_some()
    };
    let framework = if requires("laravel/framework") {
        "Laravel"
    } else if requires("symfony/framework-bundle") {
        "Symfony"
    } else {
        "PHP"
    };
    Manifest {
        name,
        framework: Some(named(framework)),
    }
}

fn dotnet_project(folder: &Path) -> Option<Manifest> {
    let entries = std::fs::read_dir(folder).ok()?;
    let project = first_project_file(entries.filter_map(Result::ok).map(|entry| entry.path()))?;
    let contents = read_small_file(&project).unwrap_or_default();
    let web = contents.contains("Microsoft.NET.Sdk.Web");
    Some(Manifest {
        name: project
            .file_stem()
            .map(|stem| stem.to_string_lossy().into_owned()),
        framework: Some(named(if web { "ASP.NET Core" } else { ".NET" })),
    })
}

/// How many directory entries to look through for a .NET project. Every
/// folder from the working directory up is searched, and some are huge.
const DIRECTORY_BUDGET: usize = 256;

fn first_project_file(entries: impl Iterator<Item = PathBuf>) -> Option<PathBuf> {
    entries.take(DIRECTORY_BUDGET).find(|path| {
        path.extension()
            .is_some_and(|extension| extension == "csproj" || extension == "sln")
    })
}

fn named(name: &str) -> Framework {
    Framework {
        name: name.to_string(),
        major: None,
    }
}

/// The first number in a version requirement, if it is a real major version:
/// `^16.0.1` → 16, `>=0.115` → none, `workspace:*` → none.
fn major(spec: &str) -> Option<u32> {
    let digits: String = spec
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok().filter(|&major| major >= 1)
}

fn read_git(folder: &Path) -> Option<GitInfo> {
    let dot_git = folder.join(".git");
    let (git_dir, worktree) = if dot_git.is_dir() {
        (dot_git, false)
    } else {
        // Worktrees and submodules point at their real git directory.
        let pointer = read_small_file(&dot_git)?;
        let target = pointer
            .lines()
            .find_map(|line| line.strip_prefix("gitdir:"))?
            .trim();
        let target = folder.join(target);
        let worktree = target
            .to_string_lossy()
            .replace('\\', "/")
            .contains("/worktrees/");
        (target, worktree)
    };
    let head = read_small_file(&git_dir.join("HEAD"));
    Some(GitInfo {
        branch: head.as_deref().and_then(branch_from_head),
        worktree,
    })
}

fn branch_from_head(head: &str) -> Option<String> {
    let head = head.trim();
    if let Some(branch) = head.strip_prefix("ref: refs/heads/") {
        return Some(branch.to_string());
    }
    let is_hash = head.len() >= 7 && head.chars().all(|c| c.is_ascii_hexdigit());
    is_hash.then(|| head[..7].to_string())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use std::path::PathBuf;

    use super::{DIRECTORY_BUDGET, Framework, first_project_file, resolve};

    fn write(path: &Path, contents: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn framework(name: &str, major: Option<u32>) -> Option<Framework> {
        Some(Framework {
            name: name.to_string(),
            major,
        })
    }

    #[test]
    fn reads_name_and_framework_from_the_nearest_package_json() {
        let home = tempfile::tempdir().unwrap();
        let app = home.path().join("dev/acme-store");
        write(
            &app.join("package.json"),
            r#"{"name":"acme-store","dependencies":{"next":"^16.0.1","react":"19.1.0"}}"#,
        );
        fs::create_dir_all(app.join("src/app")).unwrap();

        let project = resolve(&app.join("src/app"), home.path()).unwrap();

        assert_eq!(project.name, "acme-store");
        assert_eq!(project.root, app);
        assert_eq!(project.framework, framework("Next.js", Some(16)));
    }

    #[test]
    fn keeps_names_and_branches_to_a_readable_length() {
        let home = tempfile::tempdir().unwrap();
        let repo = home.path().join("acme");
        let long = "x".repeat(10_000);
        write(
            &repo.join("package.json"),
            &format!(r#"{{"name":"{long}"}}"#),
        );
        write(
            &repo.join(".git/HEAD"),
            &format!("ref: refs/heads/{long}\n"),
        );

        let project = resolve(&repo, home.path()).unwrap();

        assert_eq!(project.name.chars().count(), 100);
        assert_eq!(project.branch.unwrap().chars().count(), 255);
    }

    #[test]
    fn looks_at_a_bounded_number_of_entries_for_a_dotnet_project() {
        let crowd = (0..DIRECTORY_BUDGET).map(|n| PathBuf::from(format!("file{n}.txt")));
        let late = crowd.chain([PathBuf::from("Api.csproj")]);

        assert_eq!(first_project_file(late), None);
        assert_eq!(
            first_project_file([PathBuf::from("Api.csproj")].into_iter()),
            Some(PathBuf::from("Api.csproj"))
        );
    }

    #[test]
    fn falls_back_to_the_folder_name() {
        let home = tempfile::tempdir().unwrap();
        let app = home.path().join("sandbox");
        write(&app.join("package.json"), "{}");

        assert_eq!(resolve(&app, home.path()).unwrap().name, "sandbox");
    }

    #[test]
    fn reads_the_branch_from_git_head() {
        let home = tempfile::tempdir().unwrap();
        let repo = home.path().join("acme");
        write(&repo.join("package.json"), r#"{"name":"acme"}"#);
        write(&repo.join(".git/HEAD"), "ref: refs/heads/feat/checkout\n");

        let project = resolve(&repo, home.path()).unwrap();

        assert_eq!(project.branch.as_deref(), Some("feat/checkout"));
        assert_eq!(project.repo_root.as_deref(), Some(repo.as_path()));
        assert!(!project.worktree);
    }

    #[test]
    fn shows_a_short_hash_for_a_detached_head() {
        let home = tempfile::tempdir().unwrap();
        let repo = home.path().join("acme");
        write(&repo.join("package.json"), "{}");
        write(
            &repo.join(".git/HEAD"),
            "4f9c2ab17d0e5e1a9b2c3d4e5f60718293a4b5c6\n",
        );

        assert_eq!(
            resolve(&repo, home.path()).unwrap().branch.as_deref(),
            Some("4f9c2ab")
        );
    }

    #[test]
    fn recognises_a_git_worktree() {
        let home = tempfile::tempdir().unwrap();
        let git_dir = home.path().join("acme/.git/worktrees/checkout");
        write(&git_dir.join("HEAD"), "ref: refs/heads/feat/checkout\n");
        let worktree = home.path().join("checkout");
        write(&worktree.join("package.json"), r#"{"name":"acme"}"#);
        write(
            &worktree.join(".git"),
            &format!("gitdir: {}\n", git_dir.display()),
        );

        let project = resolve(&worktree, home.path()).unwrap();

        assert!(project.worktree);
        assert_eq!(project.branch.as_deref(), Some("feat/checkout"));
    }

    #[test]
    fn uses_the_workspace_package_inside_a_monorepo() {
        let home = tempfile::tempdir().unwrap();
        let repo = home.path().join("acme");
        write(
            &repo.join("package.json"),
            r#"{"name":"acme","private":true}"#,
        );
        write(&repo.join(".git/HEAD"), "ref: refs/heads/main\n");
        let web = repo.join("apps/web");
        write(
            &web.join("package.json"),
            r#"{"name":"@acme/web","devDependencies":{"vite":"7.1.2"}}"#,
        );

        let project = resolve(&web, home.path()).unwrap();

        assert_eq!(project.name, "@acme/web");
        assert_eq!(project.root, web);
        assert_eq!(project.repo_root.as_deref(), Some(repo.as_path()));
        assert_eq!(project.framework, framework("Vite", Some(7)));
    }

    #[test]
    fn reads_rust_go_and_python_manifests() {
        let home = tempfile::tempdir().unwrap();
        let rust = home.path().join("gateway-rs");
        write(
            &rust.join("Cargo.toml"),
            "[package]\nname = \"gateway\"\n\n[dependencies]\naxum = \"1.2\"\n",
        );
        let go = home.path().join("gateway-go");
        write(
            &go.join("go.mod"),
            "module github.com/acme/gateway/v2\n\nrequire github.com/gin-gonic/gin v1.10.0\n",
        );
        let python = home.path().join("billing");
        write(
            &python.join("pyproject.toml"),
            "[project]\nname = \"billing-api\"\ndependencies = [\"fastapi>=0.115\", \"uvicorn\"]\n",
        );

        let rust = resolve(&rust, home.path()).unwrap();
        let go = resolve(&go, home.path()).unwrap();
        let python = resolve(&python, home.path()).unwrap();

        assert_eq!(
            (rust.name.as_str(), rust.framework),
            ("gateway", framework("Axum", Some(1)))
        );
        assert_eq!(
            (go.name.as_str(), go.framework),
            ("gateway", framework("Gin", Some(1)))
        );
        assert_eq!(
            (python.name.as_str(), python.framework),
            ("billing-api", framework("FastAPI", None))
        );
    }

    #[test]
    fn uses_the_repository_when_there_is_no_manifest() {
        let home = tempfile::tempdir().unwrap();
        let repo = home.path().join("scripts");
        write(&repo.join(".git/HEAD"), "ref: refs/heads/main\n");

        let project = resolve(&repo.join("tools"), home.path());

        let project = project.unwrap();
        assert_eq!(project.name, "scripts");
        assert_eq!(project.framework, None);
    }

    #[test]
    fn does_not_hang_on_a_fifo_posing_as_a_manifest() {
        let home = tempfile::tempdir().unwrap();
        let app = home.path().join("trap");
        fs::create_dir_all(&app).unwrap();
        let status = std::process::Command::new("mkfifo")
            .arg(app.join("package.json"))
            .status()
            .unwrap();
        assert!(status.success());

        let (done, finished) = std::sync::mpsc::channel();
        let (start, home_path) = (app.clone(), home.path().to_path_buf());
        std::thread::spawn(move || done.send(resolve(&start, &home_path)).unwrap());

        let project = finished.recv_timeout(std::time::Duration::from_secs(3));
        assert!(
            project.is_ok(),
            "resolving a folder with a FIFO manifest blocked"
        );
    }

    #[test]
    fn ignores_manifests_too_large_to_be_real() {
        let home = tempfile::tempdir().unwrap();
        let app = home.path().join("bloated");
        let padding = " ".repeat(2 * 1024 * 1024);
        write(
            &app.join("package.json"),
            &format!(r#"{{"name":"bloated-name"{padding}}}"#),
        );

        let project = resolve(&app, home.path()).unwrap();

        assert_eq!(project.name, "bloated");
    }

    #[test]
    fn ignores_tool_installations_that_happen_to_be_git_repositories() {
        let home = tempfile::tempdir().unwrap();
        let homebrew = home.path().join("opt/homebrew");
        write(&homebrew.join(".git/HEAD"), "ref: refs/heads/stable\n");
        fs::create_dir_all(homebrew.join("var/mongodb")).unwrap();

        assert!(resolve(&homebrew.join("var/mongodb"), home.path()).is_none());
    }

    #[test]
    fn never_treats_the_home_folder_as_a_project() {
        let home = tempfile::tempdir().unwrap();
        write(&home.path().join("package.json"), r#"{"name":"dotfiles"}"#);
        fs::create_dir_all(home.path().join("downloads")).unwrap();

        assert!(resolve(&home.path().join("downloads"), home.path()).is_none());
    }
}
