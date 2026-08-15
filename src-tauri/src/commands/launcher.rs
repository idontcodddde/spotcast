use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, RwLock,
};
use std::thread;
use std::time::Duration;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use notify::{EventKind, RecursiveMode, Watcher};
use rusqlite::{params, Connection, Transaction};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use super::converter::SearchResultItem;

#[cfg(windows)]
const CREATE_NEW_CONSOLE: u32 = 0x00000010;

const COMMAND_PREFIX: char = '>';
const BOOKMARKS_FILE: &str = "bookmarks.json";
const INDEX_FILE: &str = "search-index.sqlite3";
const INDEX_VERSION: &str = "2";

const EXCLUDED_DIRECTORIES: &[&str] = &[
    "$RECYCLE.BIN",
    "SYSTEM VOLUME INFORMATION",
    "RECOVERY",
    "CONFIG.MSI",
    "MSOCACHE",
    "PERFLOGS",
    "WINDOWS",
    "WINDOWSAPPS",
    "WPSYSTEM",
    "XBOXGAMES",
    "PROGRAM FILES",
    "PROGRAM FILES (X86)",
    "PROGRAMDATA",
    "TEMP",
    "TMP",
    "CACHE",
    "CACHES",
    "NODE_MODULES",
    "TARGET",
    "DIST",
    "BUILD",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bookmark {
    pub id: String,
    pub title: String,
    pub url: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectApp {
    VsCode,
    IntelliJ,
    PyCharm,
    RustRover,
    VisualStudio,
    AndroidStudio,
}

impl ProjectApp {
    pub fn name(self) -> &'static str {
        match self {
            Self::VsCode => "VS Code",
            Self::IntelliJ => "IntelliJ IDEA",
            Self::PyCharm => "PyCharm",
            Self::RustRover => "RustRover",
            Self::VisualStudio => "Visual Studio",
            Self::AndroidStudio => "Android Studio",
        }
    }
}

#[derive(Clone)]
pub struct SearchIndexState {
    db_path: Arc<RwLock<Option<PathBuf>>>,
    ready: Arc<AtomicBool>,
    building: Arc<AtomicBool>,
}

impl SearchIndexState {
    pub fn new() -> Self {
        Self {
            db_path: Arc::new(RwLock::new(None)),
            ready: Arc::new(AtomicBool::new(false)),
            building: Arc::new(AtomicBool::new(false)),
        }
    }

    fn set_db_path(&self, path: PathBuf) {
        if let Ok(mut db_path) = self.db_path.write() {
            *db_path = Some(path);
        }
    }

    fn get_db_path(&self) -> Option<PathBuf> {
        self.db_path.read().ok().and_then(|path| path.clone())
    }

    pub fn is_ready(&self) -> bool {
        self.ready.load(Ordering::Acquire)
    }

    pub fn is_building(&self) -> bool {
        self.building.load(Ordering::Acquire)
    }
}

pub fn initialize_search_index(state: &SearchIndexState, app: &AppHandle) -> Result<(), String> {
    let config_dir = app.path().app_config_dir().map_err(|e| e.to_string())?;

    fs::create_dir_all(&config_dir).map_err(|e| e.to_string())?;

    let db_path = config_dir.join(INDEX_FILE);

    let connection =
        Connection::open(&db_path).map_err(|e| format!("Failed to open search index: {e}"))?;

    configure_database(&connection)?;

    create_schema(&connection)?;

    let version = connection
        .query_row(
            "SELECT value FROM index_meta WHERE key = 'version'",
            [],
            |row| row.get::<_, String>(0),
        )
        .ok();

    if version.as_deref() != Some(INDEX_VERSION) {
        connection
            .execute("DELETE FROM search_entries", [])
            .map_err(|e| e.to_string())?;

        connection
            .execute("DELETE FROM index_meta", [])
            .map_err(|e| e.to_string())?;

        connection
            .execute(
                "INSERT INTO index_meta (key, value) VALUES ('version', ?1)",
                params![INDEX_VERSION],
            )
            .map_err(|e| e.to_string())?;
    }

    let count: i64 = connection
        .query_row("SELECT count(*) FROM search_entries", [], |row| row.get(0))
        .map_err(|e| e.to_string())?;

    state.set_db_path(db_path);

    if count > 0 {
        state.ready.store(true, Ordering::Release);

        println!("Spotcast search index loaded: {} entries", count);
    }

    Ok(())
}

pub fn is_command(query: &str) -> bool {
    query.trim_start().starts_with(COMMAND_PREFIX)
}

pub fn command_text(query: &str) -> Option<String> {
    let command = query.trim().strip_prefix(COMMAND_PREFIX)?.trim();

    if command.is_empty() {
        return None;
    }

    let mut parts = command.split_whitespace();

    let executable = parts.next()?;

    if executable.eq_ignore_ascii_case("ping") {
        let target = parts.next()?;

        let target = target
            .strip_prefix("https://")
            .or_else(|| target.strip_prefix("http://"))
            .unwrap_or(target);

        let target = target.split('/').next().unwrap_or(target);

        return Some(format!("ping {target}"));
    }

    Some(command.to_string())
}

pub fn detect_best_app(path: &Path) -> Option<ProjectApp> {
    if has_any(path, &["AndroidManifest.xml"])
        && has_any(
            path,
            &[
                "build.gradle",
                "build.gradle.kts",
                "settings.gradle",
                "settings.gradle.kts",
                "gradle",
                "gradlew",
                "gradlew.bat",
            ],
        )
    {
        return Some(ProjectApp::AndroidStudio);
    }

    if has_any(
        path,
        &[
            ".gradle",
            "gradle",
            "gradle.properties",
            "settings.gradle",
            "settings.gradle.kts",
            "build.gradle",
            "build.gradle.kts",
            "gradlew",
            "gradlew.bat",
            "pom.xml",
            "mvnw",
            "mvnw.cmd",
        ],
    ) {
        return Some(ProjectApp::IntelliJ);
    }

    if has_any(
        path,
        &[
            ".venv",
            "venv",
            "pyproject.toml",
            "requirements.txt",
            "Pipfile",
            "setup.py",
            "setup.cfg",
        ],
    ) {
        return Some(ProjectApp::PyCharm);
    }

    if path.join("Cargo.toml").is_file() {
        return Some(ProjectApp::RustRover);
    }

    if has_any(path, &[".sln", ".slnx", ".csproj", ".fsproj", ".vbproj"]) {
        return Some(ProjectApp::VisualStudio);
    }

    if has_any(
        path,
        &[
            "node_modules",
            "package.json",
            "bun.lock",
            "bun.lockb",
            "pnpm-lock.yaml",
            "yarn.lock",
            "package-lock.json",
            "vite.config.ts",
            "vite.config.js",
            "svelte.config.js",
            "svelte.config.ts",
            "next.config.js",
            "next.config.ts",
            "astro.config.js",
            "astro.config.ts",
        ],
    ) {
        return Some(ProjectApp::VsCode);
    }

    if has_any(path, &["src", "README.md", "README"]) {
        return Some(ProjectApp::VsCode);
    }

    None
}

fn has_any(path: &Path, names: &[&str]) -> bool {
    names.iter().any(|name| path.join(name).exists())
}

pub fn start_search_indexer(state: SearchIndexState, app: AppHandle) {
    let Some(db_path) = state.get_db_path() else {
        eprintln!("Spotcast: search index path is unavailable");
        return;
    };

    if state.is_ready() {
        start_watcher(state, db_path);

        return;
    }

    state.building.store(true, Ordering::Release);

    let build_state = state.clone();

    thread::Builder::new()
        .name("spotcast-index-builder".into())
        .spawn(move || {
            println!("Spotcast: building search index...");

            match rebuild_index(&db_path) {
                Ok(count) => {
                    println!("Spotcast: search index built: {} entries", count);

                    build_state.ready.store(true, Ordering::Release);
                }

                Err(error) => {
                    eprintln!("Spotcast: search index build failed: {}", error);
                }
            }

            build_state.building.store(false, Ordering::Release);

            start_watcher(build_state, db_path);
        })
        .expect("failed to start search index builder");

    let _ = app;
}

pub fn search_index(state: &SearchIndexState, query: &str) -> Vec<SearchResultItem> {
    if !state.is_ready() {
        return vec![SearchResultItem {
            id: "indexing".into(),
            title: if state.is_building() {
                "Building search index...".into()
            } else {
                "Search index unavailable".into()
            },
            subtitle: if state.is_building() {
                "Your files are being indexed in the background".into()
            } else {
                "Try again in a moment".into()
            },
            category: "status".into(),
            action_payload: String::new(),
        }];
    }

    let Some(db_path) = state.get_db_path() else {
        return Vec::new();
    };

    let connection = match Connection::open(db_path) {
        Ok(connection) => connection,
        Err(error) => {
            eprintln!("Spotcast: search database open failed: {}", error);

            return Vec::new();
        }
    };

    search_database(&connection, query)
}

fn configure_database(connection: &Connection) -> Result<(), String> {
    connection
        .execute_batch(
            "
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            PRAGMA temp_store = MEMORY;
            PRAGMA cache_size = -16000;
            ",
        )
        .map_err(|e| e.to_string())
}

fn create_schema(connection: &Connection) -> Result<(), String> {
    connection
        .execute_batch(
            "
            CREATE TABLE IF NOT EXISTS index_meta (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );

            CREATE VIRTUAL TABLE IF NOT EXISTS search_entries
            USING fts5(
                path UNINDEXED,
                title,
                subtitle,
                category UNINDEXED,
                tokenize = 'unicode61'
            );
            ",
        )
        .map_err(|e| e.to_string())
}

fn rebuild_index(db_path: &Path) -> Result<usize, String> {
    let connection = Connection::open(db_path).map_err(|e| e.to_string())?;

    configure_database(&connection)?;

    create_schema(&connection)?;

    let transaction = connection
        .unchecked_transaction()
        .map_err(|e| e.to_string())?;

    transaction
        .execute("DELETE FROM search_entries", [])
        .map_err(|e| e.to_string())?;

    let mut count = 0usize;

    for root in search_roots() {
        if root.is_dir() {
            index_tree(&transaction, &root, 0, &mut count)?;
        }
    }

    index_start_menu_apps(&transaction, &mut count)?;

    transaction
        .execute("DELETE FROM index_meta", [])
        .map_err(|e| e.to_string())?;

    transaction
        .execute(
            "INSERT INTO index_meta (key, value) VALUES ('version', ?1)",
            params![INDEX_VERSION],
        )
        .map_err(|e| e.to_string())?;

    transaction.commit().map_err(|e| e.to_string())?;

    Ok(count)
}

fn search_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();

    let d_drive = Path::new(r"D:\");

    if d_drive.is_dir() {
        roots.push(d_drive.to_path_buf());
    }

    if let Some(documents) = dirs::document_dir() {
        if documents.is_dir() {
            roots.push(documents);
        }
    }

    if let Some(desktop) = dirs::desktop_dir() {
        if desktop.is_dir() {
            roots.push(desktop);
        }
    }

    roots
}

fn index_tree(
    transaction: &Transaction<'_>,
    directory: &Path,
    depth: usize,
    count: &mut usize,
) -> Result<(), String> {
    if depth > 64 {
        return Ok(());
    }

    if should_exclude_directory(directory) {
        return Ok(());
    }

    if let Ok(metadata) = fs::symlink_metadata(directory) {
        if metadata.file_type().is_symlink() {
            return Ok(());
        }
    }

    index_entry(transaction, directory, "project", count)?;

    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(_) => return Ok(()),
    };

    for entry in entries.flatten() {
        let path = entry.path();

        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(_) => continue,
        };

        if file_type.is_symlink() {
            continue;
        }

        let name = match path.file_name() {
            Some(name) => name.to_string_lossy().to_string(),
            None => continue,
        };

        if name.starts_with('.') {
            continue;
        }

        if file_type.is_dir() {
            if should_exclude_directory(&path) {
                continue;
            }

            index_tree(transaction, &path, depth + 1, count)?;
        } else {
            let extension = path
                .extension()
                .map(|extension| extension.to_string_lossy().to_lowercase());

            if matches!(extension.as_deref(), Some("lnk") | Some("url")) {
                continue;
            }

            index_entry(transaction, &path, "file", count)?;
        }
    }

    Ok(())
}

fn index_start_menu_apps(transaction: &Transaction<'_>, count: &mut usize) -> Result<(), String> {
    if let Some(app_data) = std::env::var_os("APPDATA") {
        let path = PathBuf::from(app_data)
            .join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs");

        index_app_tree(transaction, &path, count, 0)?;
    }

    if let Some(program_data) = std::env::var_os("PROGRAMDATA") {
        let path = PathBuf::from(program_data)
            .join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs");

        index_app_tree(transaction, &path, count, 0)?;
    }

    if let Some(desktop) = dirs::desktop_dir() {
        index_app_tree(transaction, &desktop, count, 0)?;
    }

    Ok(())
}

fn index_app_tree(
    transaction: &Transaction<'_>,
    directory: &Path,
    count: &mut usize,
    depth: usize,
) -> Result<(), String> {
    if depth > 12 || !directory.is_dir() {
        return Ok(());
    }

    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(_) => return Ok(()),
    };

    for entry in entries.flatten() {
        let path = entry.path();

        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(_) => continue,
        };

        if file_type.is_symlink() {
            continue;
        }

        if file_type.is_dir() {
            index_app_tree(transaction, &path, count, depth + 1)?;

            continue;
        }

        let extension = path
            .extension()
            .map(|extension| extension.to_string_lossy().to_lowercase());

        let is_app = matches!(extension.as_deref(), Some("lnk") | Some("exe"));

        if !is_app {
            continue;
        }

        index_entry(transaction, &path, "app", count)?;
    }

    Ok(())
}

fn index_entry(
    transaction: &Transaction<'_>,
    path: &Path,
    category: &str,
    count: &mut usize,
) -> Result<(), String> {
    let path_string = path.to_string_lossy().into_owned();

    let title = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| path_string.clone());

    transaction
        .execute(
            "INSERT INTO search_entries
             (path, title, subtitle, category)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                path_string,
                title,
                path.to_string_lossy().into_owned(),
                category,
            ],
        )
        .map_err(|e| e.to_string())?;

    *count += 1;

    Ok(())
}

fn search_database(connection: &Connection, query: &str) -> Vec<SearchResultItem> {
    let Some(fts_query) = make_fts_query(query) else {
        return Vec::new();
    };

    let mut statement = match connection.prepare(
        "
            SELECT
                path,
                title,
                subtitle,
                category,
                bm25(search_entries)
            FROM search_entries
            WHERE search_entries MATCH ?1
            ORDER BY bm25(search_entries)
            LIMIT 100
            ",
    ) {
        Ok(statement) => statement,
        Err(error) => {
            eprintln!("Spotcast: search prepare failed: {}", error);

            return Vec::new();
        }
    };

    let rows = match statement.query_map(params![fts_query], |row| {
        let path: String = row.get(0)?;

        let title: String = row.get(1)?;

        let subtitle: String = row.get(2)?;

        let category: String = row.get(3)?;

        let rank: f64 = row.get(4)?;

        Ok((path, title, subtitle, category, rank))
    }) {
        Ok(rows) => rows,
        Err(error) => {
            eprintln!("Spotcast: search query failed: {}", error);

            return Vec::new();
        }
    };

    let query_lower = query.to_lowercase();

    let mut results: Vec<(i32, f64, SearchResultItem)> = Vec::new();

    for row in rows.flatten() {
        let (path, title, subtitle, category, rank) = row;

        let title_lower = title.to_lowercase();

        let category_priority = match category.as_str() {
            "app" => 0,
            "project" => 1,
            "file" => 2,
            _ => 3,
        };

        let title_priority = if title_lower == query_lower {
            0
        } else if title_lower.starts_with(&query_lower) {
            1
        } else {
            2
        };

        let score = category_priority * 10 + title_priority;

        results.push((
            score,
            rank,
            SearchResultItem {
                id: format!("{}:{}", category, path),
                title,
                subtitle,
                category,
                action_payload: path,
            },
        ));
    }

    results.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then_with(|| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .then_with(|| a.2.title.to_lowercase().cmp(&b.2.title.to_lowercase()))
    });

    results
        .into_iter()
        .take(50)
        .map(|(_, _, mut result)| {
            if result.category == "project" {
                let path_string = result.action_payload.clone();

                let path = Path::new(&path_string);

                if let Some(app) = detect_best_app(path) {
                    result.subtitle = format!("{} • Open with {}", path.display(), app.name());
                }
            }

            result
        })
        .collect()
}

fn make_fts_query(query: &str) -> Option<String> {
    let tokens: Vec<String> = query
        .split_whitespace()
        .filter_map(|token| {
            let cleaned: String = token
                .chars()
                .filter(|character| character.is_alphanumeric())
                .collect();

            if cleaned.is_empty() {
                None
            } else {
                Some(format!("{}*", cleaned))
            }
        })
        .take(8)
        .collect();

    if tokens.is_empty() {
        return None;
    }

    Some(tokens.join(" OR "))
}

fn start_watcher(state: SearchIndexState, db_path: PathBuf) {
    let roots = search_roots_with_start_menu();

    thread::Builder::new()
        .name("spotcast-index-watcher".into())
        .spawn(move || {
            let (sender, receiver) = std::sync::mpsc::channel();

            let mut watcher = match notify::recommended_watcher(sender) {
                Ok(watcher) => watcher,

                Err(error) => {
                    eprintln!("Spotcast: watcher failed: {}", error);

                    return;
                }
            };

            for root in &roots {
                if root.is_dir() {
                    if let Err(error) = watcher.watch(root, RecursiveMode::Recursive) {
                        eprintln!("Spotcast: failed to watch {}: {}", root.display(), error);
                    }
                }
            }

            loop {
                let first = match receiver.recv() {
                    Ok(event) => event,
                    Err(_) => break,
                };

                let mut events = vec![first];

                while let Ok(event) = receiver.recv_timeout(Duration::from_millis(150)) {
                    events.push(event);
                }

                let paths = events
                    .into_iter()
                    .filter_map(|result| match result {
                        Ok(event) => Some((event.kind, event.paths)),

                        Err(error) => {
                            eprintln!("Spotcast: filesystem watcher error: {}", error);

                            None
                        }
                    })
                    .flat_map(|(kind, paths)| {
                        paths.into_iter().map(move |path| (kind.clone(), path))
                    })
                    .collect::<Vec<_>>();

                if !state.is_ready() {
                    continue;
                }

                if let Err(error) = apply_filesystem_events(&db_path, &paths) {
                    eprintln!("Spotcast: index update failed: {}", error);
                }
            }
        })
        .expect("failed to start search watcher");
}

fn search_roots_with_start_menu() -> Vec<PathBuf> {
    let mut roots = search_roots();

    if let Some(app_data) = std::env::var_os("APPDATA") {
        roots.push(
            PathBuf::from(app_data)
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs"),
        );
    }

    if let Some(program_data) = std::env::var_os("PROGRAMDATA") {
        roots.push(
            PathBuf::from(program_data)
                .join("Microsoft")
                .join("Windows")
                .join("Start Menu")
                .join("Programs"),
        );
    }

    roots
}

fn apply_filesystem_events(db_path: &Path, events: &[(EventKind, PathBuf)]) -> Result<(), String> {
    let connection = Connection::open(db_path).map_err(|e| e.to_string())?;

    configure_database(&connection)?;

    let transaction = connection
        .unchecked_transaction()
        .map_err(|e| e.to_string())?;

    for (kind, path) in events {
        match kind {
            EventKind::Remove(_) => {
                delete_path(&transaction, path)?;
            }

            EventKind::Create(_) | EventKind::Modify(_) => {
                if path.is_dir() {
                    delete_path(&transaction, path)?;

                    let mut count = 0usize;

                    index_tree(&transaction, path, 0, &mut count)?;
                } else if path.is_file() {
                    delete_path(&transaction, path)?;

                    if !should_index_file(path) {
                        continue;
                    }

                    let category = if is_application_path(path) {
                        "app"
                    } else {
                        "file"
                    };

                    let mut count = 0usize;

                    index_entry(&transaction, path, category, &mut count)?;
                }
            }

            _ => {}
        }
    }

    transaction.commit().map_err(|e| e.to_string())
}

fn should_index_file(path: &Path) -> bool {
    if let Some(name) = path.file_name() {
        if name.to_string_lossy().starts_with('.') {
            return false;
        }
    }

    if let Some(extension) = path.extension() {
        let extension = extension.to_string_lossy().to_lowercase();

        if matches!(extension.as_str(), "lnk" | "url") {
            return true;
        }
    }

    true
}

fn is_application_path(path: &Path) -> bool {
    let extension = path
        .extension()
        .map(|extension| extension.to_string_lossy().to_lowercase());

    matches!(extension.as_deref(), Some("lnk") | Some("exe"))
        && (path
            .to_string_lossy()
            .to_lowercase()
            .contains(r"\start menu\programs\")
            || path
                .parent()
                .and_then(|parent| dirs::desktop_dir().map(|desktop| parent == desktop))
                .unwrap_or(false))
}

fn delete_path(transaction: &Transaction<'_>, path: &Path) -> Result<(), String> {
    let path_string = path.to_string_lossy().into_owned();

    transaction
        .execute(
            "
            DELETE FROM search_entries
            WHERE path = ?1
               OR substr(
                    path,
                    1,
                    length(?1) + 1
                  ) = ?1 || '\'
            ",
            params![path_string],
        )
        .map_err(|e| e.to_string())?;

    Ok(())
}

fn should_exclude_directory(path: &Path) -> bool {
    let name = match path.file_name() {
        Some(name) => name.to_string_lossy().to_uppercase(),

        None => return false,
    };

    EXCLUDED_DIRECTORIES
        .iter()
        .any(|excluded| name == *excluded)
}

#[tauri::command]
pub fn open_path(path: String) -> Result<(), String> {
    let path_obj = Path::new(&path);

    if !path_obj.exists() {
        return Err(format!("Path does not exist: {path}"));
    }

    if !path_obj.is_dir() {
        return open::that(path_obj).map_err(|e| e.to_string());
    }

    let preferred_app = detect_best_app(path_obj);

    println!("Launcher: {} -> {:?}", path_obj.display(), preferred_app);

    match preferred_app {
        Some(app) => {
            println!("Launcher: opening with {}", app.name());

            launch_project(app, path_obj)
                .map_err(|error| format!("Could not launch {}: {}", app.name(), error))
        }

        None => launch_vscode(path_obj).or_else(|vscode_error| {
            open::that(path_obj).map_err(|explorer_error| {
                format!("VS Code failed: {vscode_error}; Explorer failed: {explorer_error}")
            })
        }),
    }
}

fn launch_project(app: ProjectApp, path: &Path) -> Result<(), String> {
    match app {
        ProjectApp::VsCode => launch_vscode(path),

        ProjectApp::IntelliJ => launch_jetbrains(
            path,
            &["idea64.exe", "idea.exe", "idea.bat"],
            "IntelliJ IDEA",
        ),

        ProjectApp::PyCharm => launch_jetbrains(
            path,
            &["pycharm64.exe", "pycharm.exe", "pycharm.bat"],
            "PyCharm",
        ),

        ProjectApp::RustRover => launch_jetbrains(
            path,
            &["rustrover64.exe", "rustrover.exe", "rustrover.bat"],
            "RustRover",
        ),

        ProjectApp::AndroidStudio => launch_jetbrains(
            path,
            &["studio64.exe", "studio.exe", "studio.bat"],
            "Android Studio",
        ),

        ProjectApp::VisualStudio => launch_visual_studio(path),
    }
}

fn launch_vscode(path: &Path) -> Result<(), String> {
    let path_string = path.to_string_lossy();

    let output = Command::new("cmd")
        .args(["/C", "code", path_string.as_ref()])
        .output();

    if let Ok(output) = output {
        if output.status.success() {
            return Ok(());
        }
    }

    if let Some(code) = find_vscode() {
        Command::new(code)
            .arg(path)
            .spawn()
            .map_err(|e| e.to_string())?;

        return Ok(());
    }

    Err("VS Code could not be found".into())
}

fn launch_jetbrains(path: &Path, executable_names: &[&str], app_name: &str) -> Result<(), String> {
    println!("Launcher: searching for {}", app_name);

    for executable in executable_names {
        if command_exists(executable) {
            return launch_gui_executable(executable, path);
        }
    }

    if let Some(executable) = find_jetbrains_local_programs(executable_names) {
        return launch_gui_executable(&executable, path);
    }

    if let Some(executable) = find_jetbrains_program_files(executable_names) {
        return launch_gui_executable(&executable, path);
    }

    if let Some(executable) = find_jetbrains_toolbox_executable(executable_names) {
        return launch_gui_executable(&executable, path);
    }

    Err(format!("{} could not be found", app_name))
}

fn launch_gui_executable(executable: impl AsRef<Path>, project_path: &Path) -> Result<(), String> {
    let executable = executable.as_ref();

    if !executable.exists() {
        return Err(format!(
            "Executable does not exist: {}",
            executable.display()
        ));
    }

    Command::new(executable)
        .arg(project_path)
        .spawn()
        .map_err(|e| format!("Failed to launch {}: {}", executable.display(), e))?;

    Ok(())
}

fn launch_visual_studio(path: &Path) -> Result<(), String> {
    if command_exists("devenv") {
        return launch_gui_executable("devenv", path);
    }

    let possible_paths = [
        r"C:\Program Files\Microsoft Visual Studio\2022\Community\Common7\IDE\devenv.exe",
        r"C:\Program Files\Microsoft Visual Studio\2022\Professional\Common7\IDE\devenv.exe",
        r"C:\Program Files\Microsoft Visual Studio\2022\Enterprise\Common7\IDE\devenv.exe",
        r"C:\Program Files\Microsoft Visual Studio\2019\Community\Common7\IDE\devenv.exe",
        r"C:\Program Files\Microsoft Visual Studio\2019\Professional\Common7\IDE\devenv.exe",
        r"C:\Program Files\Microsoft Visual Studio\2019\Enterprise\Common7\IDE\devenv.exe",
    ];

    for executable in possible_paths {
        let executable = Path::new(executable);

        if executable.is_file() {
            return launch_gui_executable(executable, path);
        }
    }

    Err("Visual Studio could not be found".into())
}

fn find_vscode() -> Option<PathBuf> {
    let local_app_data = std::env::var_os("LOCALAPPDATA")?;

    let local_app_data = PathBuf::from(local_app_data);

    let possible_paths = [
        local_app_data
            .join("Programs")
            .join("Microsoft VS Code")
            .join("Code.exe"),
        local_app_data
            .join("Programs")
            .join("Microsoft VS Code Insiders")
            .join("Code - Insiders.exe"),
    ];

    possible_paths.into_iter().find(|path| path.is_file())
}

fn find_jetbrains_local_programs(executable_names: &[&str]) -> Option<PathBuf> {
    let local_app_data = std::env::var_os("LOCALAPPDATA")?;

    let programs = PathBuf::from(local_app_data).join("Programs");

    if !programs.is_dir() {
        return None;
    }

    let entries = fs::read_dir(&programs).ok()?;

    for entry in entries.flatten() {
        let product_dir = entry.path();

        if !product_dir.is_dir() {
            continue;
        }

        let product_name = product_dir
            .file_name()
            .map(|name| name.to_string_lossy().to_lowercase())
            .unwrap_or_default();

        if !product_name.contains("intellij")
            && !product_name.contains("pycharm")
            && !product_name.contains("rustrover")
            && !product_name.contains("android")
            && !product_name.contains("jetbrains")
        {
            continue;
        }

        let bin = product_dir.join("bin");

        for executable in executable_names {
            let candidate = bin.join(executable);

            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    None
}

fn find_jetbrains_program_files(executable_names: &[&str]) -> Option<PathBuf> {
    let roots = [
        PathBuf::from(r"C:\Program Files\JetBrains"),
        PathBuf::from(r"C:\Program Files (x86)\JetBrains"),
    ];

    for root in roots {
        if !root.is_dir() {
            continue;
        }

        let products = fs::read_dir(&root).ok()?;

        for product_entry in products.flatten() {
            let product_dir = product_entry.path();

            if !product_dir.is_dir() {
                continue;
            }

            let bin = product_dir.join("bin");

            for executable in executable_names {
                let candidate = bin.join(executable);

                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
    }

    None
}

fn find_jetbrains_toolbox_executable(executable_names: &[&str]) -> Option<PathBuf> {
    let local_app_data = std::env::var_os("LOCALAPPDATA")?;

    let apps_dir = PathBuf::from(local_app_data)
        .join("JetBrains")
        .join("Toolbox")
        .join("apps");

    if !apps_dir.is_dir() {
        return None;
    }

    let products = fs::read_dir(&apps_dir).ok()?;

    for product_entry in products.flatten() {
        let product_dir = product_entry.path();

        if !product_dir.is_dir() {
            continue;
        }

        let channels = match fs::read_dir(&product_dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        for channel_entry in channels.flatten() {
            let channel_dir = channel_entry.path();

            if !channel_dir.is_dir() {
                continue;
            }

            let versions = match fs::read_dir(&channel_dir) {
                Ok(entries) => entries,
                Err(_) => continue,
            };

            for version_entry in versions.flatten() {
                let version_dir = version_entry.path();

                if !version_dir.is_dir() {
                    continue;
                }

                let bin = version_dir.join("bin");

                for executable in executable_names {
                    let candidate = bin.join(executable);

                    if candidate.is_file() {
                        return Some(candidate);
                    }
                }
            }
        }
    }

    None
}

fn command_exists(command: &str) -> bool {
    Command::new("where.exe")
        .arg(command)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

#[tauri::command]
pub fn run_command(command: String) -> Result<(), String> {
    let command = command.trim();

    if command.is_empty() {
        return Err("Command cannot be empty".into());
    }

    println!("Launcher: running command: {}", command);

    #[cfg(windows)]
    {
        Command::new("cmd.exe")
            .arg("/K")
            .arg(command)
            .creation_flags(CREATE_NEW_CONSOLE)
            .spawn()
            .map_err(|e| format!("Failed to open cmd.exe: {e}"))?;

        return Ok(());
    }

    #[cfg(not(windows))]
    {
        Command::new("sh")
            .arg("-c")
            .arg(command)
            .spawn()
            .map_err(|e| format!("Failed to run command: {e}"))?;

        Ok(())
    }
}

#[tauri::command]
pub fn open_url(url: String) -> Result<(), String> {
    open::that(&url).map_err(|e| e.to_string())
}

fn get_bookmarks_path(app: &AppHandle) -> Result<PathBuf, String> {
    let config_dir = app.path().app_config_dir().map_err(|e| e.to_string())?;

    fs::create_dir_all(&config_dir).map_err(|e| e.to_string())?;

    Ok(config_dir.join(BOOKMARKS_FILE))
}

pub fn load_bookmarks(app: &AppHandle) -> Result<Vec<Bookmark>, String> {
    let path = get_bookmarks_path(app)?;

    if !path.exists() {
        fs::write(&path, "[]").map_err(|e| e.to_string())?;

        return Ok(Vec::new());
    }

    let contents = fs::read_to_string(&path).map_err(|e| e.to_string())?;

    serde_json::from_str::<Vec<Bookmark>>(&contents)
        .map_err(|e| format!("Invalid bookmarks.json: {e}"))
}

fn save_bookmarks(app: &AppHandle, bookmarks: &[Bookmark]) -> Result<(), String> {
    let path = get_bookmarks_path(app)?;

    let json = serde_json::to_string_pretty(bookmarks).map_err(|e| e.to_string())?;

    fs::write(path, json).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn add_bookmark(app: AppHandle, title: String, url: String) -> Result<(), String> {
    let title = title.trim();

    let mut url = url.trim().to_string();

    if title.is_empty() {
        return Err("Bookmark title cannot be empty".into());
    }

    if url.is_empty() {
        return Err("Bookmark URL cannot be empty".into());
    }

    if !url.starts_with("http://") && !url.starts_with("https://") {
        url = format!("https://{url}");
    }

    let mut bookmarks = load_bookmarks(&app)?;

    if let Some(existing) = bookmarks
        .iter_mut()
        .find(|bookmark| bookmark.title.eq_ignore_ascii_case(title))
    {
        existing.url = url;
    } else {
        bookmarks.push(Bookmark {
            id: format!(
                "bookmark-{}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH,)
                    .map(|duration| { duration.as_millis() })
                    .unwrap_or_default()
            ),
            title: title.to_string(),
            url,
        });
    }

    save_bookmarks(&app, &bookmarks)
}

#[tauri::command]
pub fn remove_bookmark(app: AppHandle, title: String) -> Result<(), String> {
    let mut bookmarks = load_bookmarks(&app)?;

    let before = bookmarks.len();

    bookmarks.retain(|bookmark| !bookmark.title.eq_ignore_ascii_case(title.trim()));

    if bookmarks.len() == before {
        return Err(format!("Bookmark '{}' was not found", title));
    }

    save_bookmarks(&app, &bookmarks)
}

#[tauri::command]
pub fn open_bookmarks_file(app: AppHandle) -> Result<(), String> {
    let path = get_bookmarks_path(&app)?;

    if !path.exists() {
        fs::write(&path, "[]").map_err(|e| e.to_string())?;
    }

    open::that(&path).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_launcher_height(app: AppHandle, height: f64) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "Launcher window not found".to_string())?;

    let size = window.inner_size().map_err(|e| e.to_string())?;

    let scale_factor = window.scale_factor().map_err(|e| e.to_string())?;

    let physical_height = (height * scale_factor) as u32;

    window
        .set_size(tauri::Size::Physical(tauri::PhysicalSize {
            width: size.width,
            height: physical_height,
        }))
        .map_err(|e| e.to_string())?;

    Ok(())
}
