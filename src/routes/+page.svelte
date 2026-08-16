<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { onMount } from "svelte";

  interface SearchResult {
    id: string;
    title: string;
    subtitle: string;
    category: string;
    action_payload: string;
  }

  const PAGE_SIZE = 20;
  const COLLAPSED_HEIGHT = 76;
  const EXPANDED_HEIGHT = 430;

  const currentWindow = getCurrentWindow();

  let query = $state("");
  let results = $state<SearchResult[]>([]);
  let selectedIndex = $state(0);
  let inputElement = $state<HTMLInputElement | null>(null);
  let loadingMore = $state(false);
  let hasMore = $state(true);
  let currentOffset = $state(0);

  let searchTimeout: ReturnType<typeof setTimeout> | undefined;
  let searchGeneration = 0;
  let launcherExpanded = false;

  function invalidateSearch() {
    searchGeneration++;

    if (searchTimeout) {
      clearTimeout(searchTimeout);
      searchTimeout = undefined;
    }

    loadingMore = false;
  }

  async function resizeLauncher(height: number) {
    try {
      await invoke("set_launcher_height", {
        height,
      });
    } catch (error) {
      console.error("Failed to resize launcher:", error);
    }
  }

  function expandLauncher() {
    if (launcherExpanded) {
      return;
    }

    launcherExpanded = true;
    void resizeLauncher(EXPANDED_HEIGHT);
  }

  function collapseLauncher() {
    if (!launcherExpanded) {
      return;
    }

    launcherExpanded = false;
    void resizeLauncher(COLLAPSED_HEIGHT);
  }

  function clearLocalState() {
    invalidateSearch();

    query = "";
    results = [];
    selectedIndex = 0;
    currentOffset = 0;
    hasMore = true;
  }

  async function closeLauncher() {
    clearLocalState();
    collapseLauncher();

    try {
      await invoke("hide_launcher");
    } catch {
      try {
        await currentWindow.hide();
      } catch (error) {
        console.error("Failed to hide launcher:", error);
      }
    }
  }

  function clearSearch() {
    clearLocalState();
    collapseLauncher();

    requestAnimationFrame(() => {
      inputElement?.focus();
    });
  }

  function normalizeCommand(command: string): string {
    const trimmed = command.trim();

    if (!trimmed) {
      return "";
    }

    const parts = trimmed.split(/\s+/);

    if (parts.length >= 2 && parts[0].toLowerCase() === "ping") {
      let target = parts[1];

      target = target.replace(/^https?:\/\//i, "");
      target = target.split("/")[0];

      return `ping ${target}`;
    }

    return trimmed;
  }

  async function runCurrentCommand() {
    const raw = query.trim().replace(/^>/, "").trim();

    if (!raw) {
      return;
    }

    const command = normalizeCommand(raw);

    if (!command) {
      return;
    }

    try {
      await invoke("run_command", {
        command,
      });

      await closeLauncher();
    } catch (error) {
      console.error("Failed to run command:", error);
    }
  }

  async function addBookmarkFromResult(payload: string) {
    const separator = payload.indexOf("|");

    if (separator === -1) {
      return;
    }

    const title = payload.slice(0, separator).trim();
    const url = payload.slice(separator + 1).trim();

    if (!title || !url) {
      return;
    }

    try {
      await invoke("add_bookmark", {
        title,
        url,
      });

      clearSearch();
    } catch (error) {
      console.error("Failed to add bookmark:", error);
    }
  }

  async function executeResult(item: SearchResult) {
    try {
      switch (item.category) {
        case "status":
          return;

        case "command":
          await invoke("run_command", {
            command: normalizeCommand(item.action_payload),
          });

          await closeLauncher();
          return;

        case "app":
        case "project":
        case "file":
          await invoke("open_path", {
            path: item.action_payload,
          });

          await closeLauncher();
          return;

        case "bookmark":
        case "web":
          await invoke("open_url", {
            url: item.action_payload,
          });

          await closeLauncher();
          return;

        case "bookmark_edit":
          await invoke("open_bookmarks_file");
          await closeLauncher();
          return;

        case "bookmark_add":
          await addBookmarkFromResult(item.action_payload);
          return;

        case "bookmark_remove":
          await invoke("remove_bookmark", {
            title: item.action_payload,
          });

          clearSearch();
          return;

        default:
          await navigator.clipboard.writeText(item.action_payload);

          clearSearch();
      }
    } catch (error) {
      console.error("Failed to execute result:", error);
    }
  }

  async function search(value: string, generation: number) {
    const trimmed = value.trim();

    if (!trimmed) {
      return;
    }

    try {
      const response = await invoke<SearchResult[]>("global_search", {
        query: trimmed,
        offset: 0,
      });

      if (generation !== searchGeneration) {
        return;
      }

      if (query.trim() !== trimmed) {
        return;
      }

      results = response;
      selectedIndex = 0;
      currentOffset = response.length;
      hasMore = response.length >= PAGE_SIZE;

      if (response.length > 0) {
        expandLauncher();
      } else {
        collapseLauncher();
      }
    } catch (error) {
      if (generation !== searchGeneration) {
        return;
      }

      console.error("Search failed:", error);

      results = [
        {
          id: "search-error",
          title: "Search failed",
          subtitle: String(error),
          category: "status",
          action_payload: "",
        },
      ];

      selectedIndex = 0;
      currentOffset = 0;
      hasMore = false;

      expandLauncher();
    }
  }

  function scheduleSearch(value: string) {
    if (searchTimeout) {
      clearTimeout(searchTimeout);
      searchTimeout = undefined;
    }

    const generation = ++searchGeneration;

    searchTimeout = setTimeout(() => {
      searchTimeout = undefined;
      void search(value, generation);
    }, 100);
  }

  async function loadMore() {
    const trimmed = query.trim();

    if (
      !trimmed ||
      loadingMore ||
      !hasMore ||
      trimmed.startsWith(">") ||
      trimmed.startsWith("@")
    ) {
      return;
    }

    const generation = searchGeneration;
    const offset = currentOffset;

    loadingMore = true;

    try {
      const response = await invoke<SearchResult[]>("global_search", {
        query: trimmed,
        offset,
      });

      if (generation !== searchGeneration) {
        return;
      }

      if (query.trim() !== trimmed) {
        return;
      }

      if (response.length === 0) {
        hasMore = false;
        return;
      }

      const existingIds = new Set(results.map((item) => item.id));

      const newResults = response.filter((item) => !existingIds.has(item.id));

      results = [...results, ...newResults];

      currentOffset = results.length;
      hasMore = response.length >= PAGE_SIZE;
    } catch (error) {
      console.error("Failed to load more results:", error);
    } finally {
      if (generation === searchGeneration) {
        loadingMore = false;
      }
    }
  }

  function handleResultsScroll(event: Event) {
    const element = event.currentTarget as HTMLDivElement;

    const remaining =
      element.scrollHeight - element.scrollTop - element.clientHeight;

    if (remaining < 100) {
      void loadMore();
    }
  }

  $effect(() => {
    const currentQuery = query.trim();

    if (searchTimeout) {
      clearTimeout(searchTimeout);
      searchTimeout = undefined;
    }

    ++searchGeneration;

    if (!currentQuery) {
      results = [];
      selectedIndex = 0;
      currentOffset = 0;
      hasMore = true;
      loadingMore = false;

      collapseLauncher();
      return;
    }

    if (currentQuery.startsWith(">")) {
      results = [];
      selectedIndex = 0;
      currentOffset = 0;
      hasMore = false;
      loadingMore = false;

      expandLauncher();
      return;
    }

    scheduleSearch(currentQuery);
  });

  async function handleKeyDown(event: KeyboardEvent) {
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();

      await closeLauncher();
      return;
    }

    if (event.key === "ArrowDown" && results.length > 0) {
      event.preventDefault();

      selectedIndex = (selectedIndex + 1) % results.length;

      return;
    }

    if (event.key === "ArrowUp" && results.length > 0) {
      event.preventDefault();

      selectedIndex = (selectedIndex - 1 + results.length) % results.length;

      return;
    }

    if (event.key === "Enter") {
      event.preventDefault();

      if (query.trimStart().startsWith(">")) {
        await runCurrentCommand();
        return;
      }

      const selected = results[selectedIndex];

      if (selected) {
        await executeResult(selected);
      }
    }
  }

  function getCategoryIcon(category: string) {
    switch (category) {
      case "project":
        return "folder";
      case "file":
        return "file";
      case "app":
        return "app";
      case "conversion":
        return "arrow";
      case "command":
        return "command";
      case "bookmark":
        return "bookmark";
      case "bookmark_edit":
        return "edit";
      case "web":
        return "web";
      case "status":
        return "status";
      default:
        return "search";
    }
  }

  onMount(() => {
    requestAnimationFrame(() => {
      inputElement?.focus();
    });

    void resizeLauncher(COLLAPSED_HEIGHT);

    const handleGlobalKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        event.stopPropagation();
        void closeLauncher();
      }
    };

    window.addEventListener("keydown", handleGlobalKeyDown, true);

    let unlistenFocus: (() => void) | undefined;

    void currentWindow
      .onFocusChanged(({ payload }) => {
        if (!payload) {
          clearLocalState();
          collapseLauncher();
        } else {
          requestAnimationFrame(() => {
            inputElement?.focus();
          });
        }
      })
      .then((unlisten) => {
        unlistenFocus = unlisten;
      });

    return () => {
      window.removeEventListener("keydown", handleGlobalKeyDown, true);

      unlistenFocus?.();

      invalidateSearch();
    };
  });
</script>

<svelte:head>
  <title>spotcast</title>
</svelte:head>

<main class="spotlight-container">
  <div class="search-section">
    <div class="search-icon-wrapper">
      <svg
        class="search-icon"
        viewBox="0 0 24 24"
        width="22"
        height="22"
        stroke="currentColor"
        stroke-width="2"
        fill="none"
        aria-hidden="true"
      >
        <circle cx="11" cy="11" r="7.5" />
        <line x1="16.5" y1="16.5" x2="21" y2="21" />
      </svg>
    </div>

    <input
      bind:this={inputElement}
      bind:value={query}
      onkeydown={handleKeyDown}
      type="text"
      placeholder="Search"
      spellcheck="false"
      autocomplete="off"
      aria-label="Search"
    />

    {#if query}
      <button
        class="clear-button"
        type="button"
        onclick={clearSearch}
        aria-label="Clear search"
      >
        <svg
          viewBox="0 0 20 20"
          width="15"
          height="15"
          fill="none"
          stroke="currentColor"
          stroke-width="2"
        >
          <line x1="5" y1="5" x2="15" y2="15" />
          <line x1="15" y1="5" x2="5" y2="15" />
        </svg>
      </button>
    {/if}
  </div>

  {#if query.trimStart().startsWith(">") && query.trim().length > 1}
    <div class="results-divider"></div>

    <button
      class="result-item command-item active"
      type="button"
      onclick={runCurrentCommand}
    >
      <div class="result-icon">
        <svg
          viewBox="0 0 24 24"
          width="18"
          height="18"
          fill="none"
          stroke="currentColor"
          stroke-width="1.8"
        >
          <polyline points="7 8 11 12 7 16" />
          <line x1="13" y1="16" x2="18" y2="16" />
        </svg>
      </div>

      <div class="text-group">
        <span class="title">
          {normalizeCommand(query.trim().slice(1))}
        </span>

        <span class="subtitle"> Press Enter to run command </span>
      </div>

      <span class="category"> Command </span>
    </button>
  {:else if results.length > 0}
    <div class="results-divider"></div>

    <div class="results-list" onscroll={handleResultsScroll}>
      {#each results as item, idx (item.id)}
        <button
          class:active={idx === selectedIndex}
          class:status-item={item.category === "status"}
          class="result-item"
          type="button"
          onclick={() => {
            selectedIndex = idx;
            void executeResult(item);
          }}
          onmouseenter={() => {
            if (item.category !== "status") {
              selectedIndex = idx;
            }
          }}
        >
          <div class="result-icon">
            {#if getCategoryIcon(item.category) === "folder"}
              <svg
                viewBox="0 0 24 24"
                width="18"
                height="18"
                fill="none"
                stroke="currentColor"
                stroke-width="1.8"
              >
                <path
                  d="M3 6.5A2.5 2.5 0 0 1 5.5 4H10l2 2h6.5A2.5 2.5 0 0 1 21 8.5v9A2.5 2.5 0 0 1 18.5 20h-13A2.5 2.5 0 0 1 3 17.5v-11Z"
                />
              </svg>
            {:else if getCategoryIcon(item.category) === "file"}
              <svg
                viewBox="0 0 24 24"
                width="18"
                height="18"
                fill="none"
                stroke="currentColor"
                stroke-width="1.8"
              >
                <path d="M6 3.5h8l4 4V20H6V3.5Z" />
                <path d="M14 3.5V8h4" />
              </svg>
            {:else if getCategoryIcon(item.category) === "app"}
              <svg
                viewBox="0 0 24 24"
                width="18"
                height="18"
                fill="none"
                stroke="currentColor"
                stroke-width="1.8"
              >
                <rect x="4" y="4" width="16" height="16" rx="4" />
                <path d="M8 8h8" />
                <path d="M8 12h8" />
                <path d="M8 16h5" />
              </svg>
            {:else if getCategoryIcon(item.category) === "arrow"}
              <svg
                viewBox="0 0 24 24"
                width="18"
                height="18"
                fill="none"
                stroke="currentColor"
                stroke-width="1.8"
              >
                <path d="M5 8h14" />
                <path d="m15 4 4 4-4 4" />
                <path d="M19 16H5" />
                <path d="m9 12-4 4 4 4" />
              </svg>
            {:else if getCategoryIcon(item.category) === "command"}
              <svg
                viewBox="0 0 24 24"
                width="18"
                height="18"
                fill="none"
                stroke="currentColor"
                stroke-width="1.8"
              >
                <polyline points="7 8 11 12 7 16" />
                <line x1="13" y1="16" x2="18" y2="16" />
              </svg>
            {:else if getCategoryIcon(item.category) === "bookmark"}
              <svg
                viewBox="0 0 24 24"
                width="18"
                height="18"
                fill="none"
                stroke="currentColor"
                stroke-width="1.8"
              >
                <path
                  d="M6 4.5A2.5 2.5 0 0 1 8.5 2h7A2.5 2.5 0 0 1 18 4.5V21l-6-3.5L6 21V4.5Z"
                />
              </svg>
            {:else if getCategoryIcon(item.category) === "edit"}
              <svg
                viewBox="0 0 24 24"
                width="18"
                height="18"
                fill="none"
                stroke="currentColor"
                stroke-width="1.8"
              >
                <path d="m14 5 5 5" />
                <path d="M4 20h5l10-10a3.54 3.54 0 0 0-5-5L4 15v5Z" />
              </svg>
            {:else if getCategoryIcon(item.category) === "web"}
              <svg
                viewBox="0 0 24 24"
                width="18"
                height="18"
                fill="none"
                stroke="currentColor"
                stroke-width="1.8"
              >
                <circle cx="12" cy="12" r="9" />
                <path d="M3 12h18" />
                <path d="M12 3a14 14 0 0 1 0 18" />
                <path d="M12 3a14 14 0 0 0 0 18" />
              </svg>
            {:else if getCategoryIcon(item.category) === "status"}
              <svg
                viewBox="0 0 24 24"
                width="18"
                height="18"
                fill="none"
                stroke="currentColor"
                stroke-width="1.8"
              >
                <circle cx="12" cy="12" r="8" />
                <path d="M12 8v4l2.5 2.5" />
              </svg>
            {:else}
              <svg
                viewBox="0 0 24 24"
                width="18"
                height="18"
                fill="none"
                stroke="currentColor"
                stroke-width="1.8"
              >
                <circle cx="11" cy="11" r="7" />
                <line x1="16.5" y1="16.5" x2="21" y2="21" />
              </svg>
            {/if}
          </div>

          <div class="text-group">
            <span class="title">
              {item.title}
            </span>

            {#if item.subtitle}
              <span class="subtitle">
                {item.subtitle}
              </span>
            {/if}
          </div>

          {#if item.category !== "status"}
            <span class="category">
              {item.category}
            </span>
          {/if}
        </button>
      {/each}

      {#if loadingMore}
        <div class="loading-more">Loading more...</div>
      {/if}
    </div>
  {/if}
</main>

<style lang="scss">
  :global(html),
  :global(body),
  :global(#app) {
    width: 100%;
    height: 100%;
    margin: 0;
    padding: 0;
    overflow: hidden;
    background: transparent !important;
  }

  :global(body) {
    font-family: -apple-system, BlinkMacSystemFont, "SF Pro Display",
      "SF Pro Text", "Segoe UI", Roboto, Helvetica, Arial, sans-serif;

    -webkit-font-smoothing: antialiased;
    text-rendering: optimizeLegibility;
  }

  :global(*) {
    box-sizing: border-box;
  }

  .spotlight-container {
    position: relative;
    width: 100%;
    height: 100%;

    display: flex;
    flex-direction: column;

    overflow: hidden;

    border-radius: 30px;

    background: rgba(35, 35, 40, 0.82);

    backdrop-filter: blur(42px) saturate(175%) brightness(1.08);

    -webkit-backdrop-filter: blur(42px) saturate(175%) brightness(1.08);

    border: 1px solid rgba(255, 255, 255, 0.16);

    box-shadow:
      0 28px 80px rgba(0, 0, 0, 0.34),
      0 8px 24px rgba(0, 0, 0, 0.18),
      inset 0 1px 0 rgba(255, 255, 255, 0.15),
      inset 0 -1px 0 rgba(0, 0, 0, 0.08);

    color: rgba(255, 255, 255, 0.96);

    isolation: isolate;
  }

  .search-section,
  .results-divider,
  .results-list,
  .result-item {
    position: relative;
    z-index: 2;
  }

  .search-section {
    height: 74px;
    min-height: 74px;

    display: flex;
    align-items: center;

    gap: 13px;

    padding: 0 21px;
  }

  .search-icon-wrapper {
    width: 24px;
    height: 24px;

    display: flex;
    align-items: center;
    justify-content: center;

    flex-shrink: 0;
  }

  .search-icon {
    width: 21px;
    height: 21px;

    color: rgba(255, 255, 255, 0.6);
  }

  input {
    flex: 1;
    min-width: 0;

    height: 100%;

    padding: 0;

    border: none;
    outline: none;

    background: transparent;

    color: white;

    font-family: inherit;

    font-size: 24px;
    font-weight: 400;

    letter-spacing: -0.02em;

    &::placeholder {
      color: rgba(255, 255, 255, 0.42);
    }

    &::selection {
      background: rgba(110, 160, 255, 0.4);
    }
  }

  .clear-button {
    width: 27px;
    height: 27px;

    display: flex;
    align-items: center;
    justify-content: center;

    flex-shrink: 0;

    padding: 0;

    border: 1px solid rgba(255, 255, 255, 0.08);

    border-radius: 50%;

    background: rgba(255, 255, 255, 0.08);

    color: rgba(255, 255, 255, 0.65);

    cursor: pointer;
  }

  .clear-button:hover {
    background: rgba(255, 255, 255, 0.15);
  }

  .results-divider {
    height: 1px;

    margin: 0 18px;

    background: rgba(255, 255, 255, 0.075);
  }

  .results-list {
    display: flex;
    flex-direction: column;

    gap: 4px;

    max-height: 340px;

    padding: 9px;

    overflow-y: auto;

    overscroll-behavior: contain;
  }

  .results-list::-webkit-scrollbar {
    width: 5px;
  }

  .results-list::-webkit-scrollbar-track {
    background: transparent;
  }

  .results-list::-webkit-scrollbar-thumb {
    background: rgba(255, 255, 255, 0.11);

    border-radius: 10px;
  }

  .results-list::-webkit-scrollbar-thumb:hover {
    background: rgba(255, 255, 255, 0.17);
  }

  .result-item {
    width: 100%;
    min-height: 59px;

    display: flex;
    align-items: center;

    gap: 12px;

    padding: 8px 11px;

    border: 1px solid rgba(255, 255, 255, 0.035);

    border-radius: 15px;

    background: rgba(255, 255, 255, 0.025);

    backdrop-filter: blur(16px) saturate(145%);

    -webkit-backdrop-filter: blur(16px) saturate(145%);

    color: rgba(255, 255, 255, 0.94);

    text-align: left;

    font-family: inherit;

    cursor: pointer;

    box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.025);

    transition:
      background 120ms ease,
      border-color 120ms ease;
  }

  .result-item:hover {
    background: rgba(255, 255, 255, 0.095);

    border-color: rgba(255, 255, 255, 0.085);
  }

  .result-item.active {
    background: rgba(100, 145, 245, 0.26);

    border-color: rgba(165, 195, 255, 0.17);
  }

  .result-item.active .result-icon {
    background: rgba(255, 255, 255, 0.12);

    color: white;
  }

  .result-item.active .title {
    color: white;
  }

  .result-item.active .subtitle {
    color: rgba(255, 255, 255, 0.72);
  }

  .result-icon {
    width: 39px;
    height: 39px;

    display: flex;
    align-items: center;
    justify-content: center;

    flex-shrink: 0;

    border: 1px solid rgba(255, 255, 255, 0.045);

    border-radius: 12px;

    background: rgba(255, 255, 255, 0.05);

    color: rgba(255, 255, 255, 0.66);
  }

  .text-group {
    min-width: 0;

    flex: 1;

    display: flex;
    flex-direction: column;

    gap: 2px;
  }

  .title {
    overflow: hidden;

    color: rgba(255, 255, 255, 0.95);

    font-size: 14px;
    line-height: 18px;

    font-weight: 500;

    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .subtitle {
    overflow: hidden;

    color: rgba(255, 255, 255, 0.41);

    font-size: 12px;
    line-height: 16px;

    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .category {
    flex-shrink: 0;

    padding: 4px 7px;

    border: 1px solid rgba(255, 255, 255, 0.04);

    border-radius: 8px;

    background: rgba(255, 255, 255, 0.04);

    color: rgba(255, 255, 255, 0.35);

    font-size: 9px;

    text-transform: uppercase;
  }

  .command-item {
    margin: 8px;

    width: calc(100% - 16px);
  }

  .loading-more {
    padding: 8px;

    text-align: center;

    color: rgba(255, 255, 255, 0.35);

    font-size: 11px;
  }
</style>
