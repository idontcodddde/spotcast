# Spotcast

A fast, Spotlight-inspired desktop launcher for Windows built with SvelteKit, Rust, and Tauri.

Spotcast provides a keyboard-driven interface for launching applications, opening projects, searching files, running commands, performing calculations and conversions, opening bookmarks, and searching the web.

## Features

### 🔎 Spotlight-style launcher

- `Alt + Space` opens Spotcast.
- Search field is focused automatically.
- `Enter` launches the selected result.
- `Arrow Up` / `Arrow Down` navigate results.
- `Escape` hides the launcher.
- Clicking outside the launcher hides it.
- Borderless native window.
- Transparent window with glassmorphism styling.
- Automatically expands when results are available.
- Hidden from the Windows taskbar.

### 📂 Fast persistent search index

Spotcast uses a persistent SQLite FTS5 search index instead of recursively scanning the filesystem for every search.

The index:

- Is created in the background.
- Is saved to disk.
- Is loaded on subsequent launches.
- Uses full-text indexing for fast searches.
- Watches filesystem changes while Spotcast is running.
- Searches applications, projects, files, and folders.

This means entering a query such as:

```text
discord
```

does not cause Spotcast to scan the entire drive again.

### 🖥️ Application launcher

Spotcast can discover Windows applications through Start Menu entries and launch them directly.

Example:

```text
discord
```

can return:

```text
Discord
Application
```

Press `Enter` to launch it.

### 🧑‍💻 Automatic development-environment detection

Spotcast detects common development projects and automatically chooses an appropriate IDE.

| Project | Application |
|---|---|
| Gradle / Java / Kotlin | IntelliJ IDEA |
| Android + Gradle | Android Studio |
| Python / `.venv` | PyCharm |
| Rust / `Cargo.toml` | RustRover |
| `.sln` / `.csproj` | Visual Studio |
| Node.js / Bun / Svelte / Vite | VS Code |

For example, selecting a Gradle project can open the entire project directly in IntelliJ IDEA instead of opening File Explorer.

### 📁 Files and folders

The search index includes files and folders from locations such as:

- `D:\`
- Documents
- Desktop
- Windows Start Menu application locations

Certain system and generated directories are excluded, including:

```text
$RECYCLE.BIN
System Volume Information
Recovery
Windows
WindowsApps
WpSystem
XboxGames
Program Files
Program Files (x86)
ProgramData
Temp
Tmp
Cache
Caches
node_modules
target
dist
build
```

Hidden dot-prefixed files and directories are also excluded.

### 💻 Command launcher

Prefix a command with `>`.

Examples:

```text
>ipconfig
```

```text
>ping google.com
```

```text
>ping https://google.com
```

Commands are launched in a real Windows `cmd.exe` window.

### 🧮 Calculator

Prefix mathematical expressions with `=`.

Examples:

```text
=5 * 10
```

```text
=(25 + 75) / 2
```

Calculation results can be copied by pressing `Enter`.

### 📏 Unit conversion

Spotcast supports common unit conversions, including:

- Feet ↔ meters
- Inches ↔ centimeters
- Miles ↔ kilometers
- Pounds ↔ kilograms
- Celsius ↔ Fahrenheit
- Pixels ↔ rem

Examples:

```text
10 ft
5 m
12 in
30 cm
2 mi
10 km
5 lb
70 f
32 px
2 rem
```

### 🔖 Bookmarks

Type `@` to access bookmarks.

```text
@
```

Search bookmarks by name or URL:

```text
@github
```

Press `Enter` to open the bookmark.

Bookmarks are stored persistently in `bookmarks.json`.

#### Edit bookmarks

```text
@edit
```

Opens the bookmark file.

#### Add a bookmark

```text
@add GitHub|https://github.com
```

#### Remove a bookmark

```text
@remove GitHub
```

#### Bookmark format

```json
[
  {
    "id": "github",
    "title": "GitHub",
    "url": "https://github.com"
  }
]
```

### 🌐 Google fallback

When a normal search has no local result, Spotcast provides a Google search result.

For example:

```text
quantum potato
```

can return:

```text
Search Google for "quantum potato"
```

Pressing `Enter` opens the search in the default browser.

During initial indexing, Spotcast displays an indexing status instead of treating the incomplete index as an empty search.

### 🪟 System tray

Spotcast can remain running in the Windows system tray.

- Closing the launcher hides it instead of exiting.
- Clicking the tray icon shows Spotcast.
- Tray menu includes **Show Spotcast** and **Quit Spotcast**.
- Uses a dedicated tray icon.

## Tech Stack

- [SvelteKit](https://kit.svelte.dev/)
- [Svelte](https://svelte.dev/)
- [Vite](https://vite.dev/)
- [Rust](https://www.rust-lang.org/)
- [Tauri 2](https://v2.tauri.app/)
- [SQLite](https://www.sqlite.org/)
- [SQLite FTS5](https://sqlite.org/fts5.html)
- [notify](https://docs.rs/notify/)
- [evalexpr](https://docs.rs/evalexpr/)
- [Bun](https://bun.sh/)

## Project Structure

```text
spotcast/
├── src/
│   └── routes/
│       └── +page.svelte
│
├── src-tauri/
│   ├── src/
│   │   ├── commands/
│   │   │   ├── converter.rs
│   │   │   ├── launcher.rs
│   │   │   └── mod.rs
│   │   │
│   │   └── lib.rs
│   │
│   ├── icons/
│   │   ├── TrayIcon.png
│   │   └── ...
│   │
│   ├── capabilities/
│   │   └── default.json
│   │
│   ├── tauri.conf.json
│   └── Cargo.toml
│
├── static/
├── package.json
├── svelte.config.js
├── vite.config.ts
└── README.md
```

## Development

### Requirements

You'll need:

- Windows
- [Bun](https://bun.sh/)
- Rust
- Cargo
- Tauri CLI

Install the frontend dependencies:

```powershell
bun install
```

Start the development server:

```powershell
bun tauri dev
```

### Rust checks

The Rust project is inside `src-tauri`:

```powershell
cd src-tauri
cargo check
```

Return to the project root afterward:

```powershell
cd ..
```

### Production build

From the project root:

```powershell
bun tauri build
```

The compiled executable is generated under:

```text
src-tauri/target/release/
```

Bundled installers are generated under:

```text
src-tauri/target/release/bundle/
```

## Search Index

The first launch creates a persistent SQLite search index.

The index is stored in the application's Tauri configuration directory and is reused when Spotcast starts again.

The general flow is:

```text
First launch
    ↓
Create SQLite database
    ↓
Background indexing
    ↓
FTS5 search index
    ↓
Persist to disk
```

Later launches:

```text
Launch Spotcast
    ↓
Load existing index
    ↓
Search immediately
    ↓
Watch filesystem changes
```

This avoids performing a complete filesystem scan on every keystroke and avoids rebuilding the entire index every time Spotcast starts.

## Current Platform

Spotcast is currently designed for **Windows**.

Several features depend on Windows-specific functionality, including:

- `cmd.exe`
- Windows Start Menu shortcuts
- `.lnk` application shortcuts
- Windows application installation paths
- Windows system tray behavior
- Windows-specific process flags

Tauri itself is cross-platform, so additional platform implementations could be added later.
