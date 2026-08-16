pub mod converter;
pub mod launcher;

use converter::SearchResultItem;
use launcher::SearchIndexState;

const SEARCH_PAGE_SIZE: usize = 20;

#[tauri::command]
pub async fn global_search(
    app: tauri::AppHandle,
    state: tauri::State<'_, SearchIndexState>,
    query: String,
    offset: usize,
) -> Result<Vec<SearchResultItem>, String> {
    let query = query.trim().to_string();

    if query.is_empty() {
        return Ok(Vec::new());
    }

    if query.starts_with('@') {
        let bookmark_query = query.trim_start_matches('@').trim();

        if bookmark_query.is_empty() {
            let bookmarks = launcher::load_bookmarks(&app).unwrap_or_default();

            let mut results = Vec::new();

            for bookmark in bookmarks.into_iter().take(20) {
                let url = bookmark.url.clone();

                results.push(SearchResultItem {
                    id: format!("bookmark:{}", bookmark.id),
                    title: bookmark.title,
                    subtitle: url.clone(),
                    category: "bookmark".into(),
                    action_payload: url,
                });
            }

            results.push(SearchResultItem {
                id: "bookmark:edit".into(),
                title: "Edit bookmarks".into(),
                subtitle: "Open bookmarks.json".into(),
                category: "bookmark_edit".into(),
                action_payload: String::new(),
            });

            return Ok(results);
        }

        if bookmark_query.eq_ignore_ascii_case("edit") {
            return Ok(vec![SearchResultItem {
                id: "bookmark:edit".into(),
                title: "Edit bookmarks".into(),
                subtitle: "Open bookmarks.json".into(),
                category: "bookmark_edit".into(),
                action_payload: String::new(),
            }]);
        }

        if let Some(rest) = bookmark_query.strip_prefix("add ") {
            return Ok(vec![SearchResultItem {
                id: "bookmark:add".into(),
                title: "Add bookmark".into(),
                subtitle: "Name|https://example.com".into(),
                category: "bookmark_add".into(),
                action_payload: rest.trim().into(),
            }]);
        }

        if let Some(rest) = bookmark_query.strip_prefix("remove ") {
            return Ok(vec![SearchResultItem {
                id: "bookmark:remove".into(),
                title: "Remove bookmark".into(),
                subtitle: format!("Remove \"{}\"", rest.trim()),
                category: "bookmark_remove".into(),
                action_payload: rest.trim().into(),
            }]);
        }

        let search = bookmark_query.to_lowercase();
        let bookmarks = launcher::load_bookmarks(&app).unwrap_or_default();

        let mut results = Vec::new();

        for bookmark in bookmarks {
            let title_matches = bookmark.title.to_lowercase().contains(&search);
            let url_matches = bookmark.url.to_lowercase().contains(&search);

            if title_matches || url_matches {
                let url = bookmark.url.clone();

                results.push(SearchResultItem {
                    id: format!("bookmark:{}", bookmark.id),
                    title: bookmark.title,
                    subtitle: url.clone(),
                    category: "bookmark".into(),
                    action_payload: url,
                });
            }

            if results.len() >= 20 {
                break;
            }
        }

        return Ok(results);
    }

    if launcher::is_command(&query) {
        if let Some(command) = launcher::command_text(&query) {
            return Ok(vec![SearchResultItem {
                id: format!("command:{}", command),
                title: command.clone(),
                subtitle: "Run command".into(),
                category: "command".into(),
                action_payload: command,
            }]);
        }

        return Ok(Vec::new());
    }

    if let Some(conversion) = converter::evaluate_conversions(&query) {
        return Ok(vec![conversion]);
    }

    let search_state = state.inner().clone();
    let search_query = query.clone();

    let index_results = tauri::async_runtime::spawn_blocking(move || {
        launcher::search_index(&search_state, &search_query, offset, SEARCH_PAGE_SIZE)
    })
    .await
    .map_err(|error| format!("Search task failed: {error}"))?;

    if index_results.len() == 1 && index_results[0].category == "status" {
        return Ok(index_results);
    }

    if !index_results.is_empty() {
        return Ok(index_results);
    }

    if offset > 0 {
        return Ok(Vec::new());
    }

    let google_url = format!("https://www.google.com/search?q={}", google_encode(&query));

    Ok(vec![SearchResultItem {
        id: format!("google:{}", query),
        title: format!("Search Google for \"{}\"", query),
        subtitle: "Search the web".into(),
        category: "web".into(),
        action_payload: google_url,
    }])
}

fn google_encode(value: &str) -> String {
    let mut encoded = String::new();

    for byte in value.as_bytes() {
        match *byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(*byte as char);
            }

            b' ' => {
                encoded.push('+');
            }

            other => {
                encoded.push('%');

                encoded.push(
                    char::from_digit(((other >> 4) & 0xF) as u32, 16)
                        .unwrap()
                        .to_ascii_uppercase(),
                );

                encoded.push(
                    char::from_digit((other & 0xF) as u32, 16)
                        .unwrap()
                        .to_ascii_uppercase(),
                );
            }
        }
    }

    encoded
}
