import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getVersion } from "@tauri-apps/api/app";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import { check } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

interface Server {
  id: string;
  name: string;
  host: string;
  username: string;
}

interface Settings {
  game_dir: string | null;
  servers: Server[];
  selected_server: string;
  close_on_launch: boolean;
}

interface VerifyResult {
  version: string;
  checked: number;
  missing: string[];
  corrupt: string[];
}

interface NewsItem {
  title: string;
  link: string;
  summary: string;
  date: number;
  category: string;
  patch_notes: boolean;
}

const $ = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T;
const el = <K extends keyof HTMLElementTagNameMap>(tag: K, props: Record<string, unknown> = {}) =>
  Object.assign(document.createElement(tag), props) as HTMLElementTagNameMap[K];

const statusEl = $("status");
const barFill = $("bar-fill");
const problemsEl = $<HTMLUListElement>("problems");
const playBtn = $<HTMLButtonElement>("play");
const serverSelect = $<HTMLSelectElement>("server");
const accountEl = $("account");

let settings: Settings;
let busy = false;
let filesOk = false;

// ---------- status / progress ----------

function setStatus(text: string, kind: "" | "ok" | "bad" = "") {
  statusEl.textContent = text;
  statusEl.className = kind;
}

function setProgress(fraction: number | null) {
  barFill.style.width = fraction === null ? "0%" : `${Math.round(fraction * 100)}%`;
}

function refreshPlay() {
  playBtn.disabled = busy || !filesOk || !currentServer();
}

// ---------- servers / account ----------

function currentServer(): Server | undefined {
  return settings.servers.find((s) => s.id === settings.selected_server);
}

async function renderServers() {
  serverSelect.replaceChildren(
    ...settings.servers.map((s) => el("option", { value: s.id, textContent: s.name })),
  );
  serverSelect.value = settings.selected_server;
  await renderAccount();
}

async function renderAccount() {
  const server = currentServer();
  if (!server) {
    accountEl.textContent = "No server selected.";
  } else if (!server.username) {
    accountEl.textContent = "Log in at the game's login screen.";
  } else {
    const saved = await invoke<boolean>("has_password", { serverId: server.id });
    accountEl.textContent = saved
      ? `Logging in as ${server.username} automatically.`
      : `Username ${server.username} — enter your password in-game.`;
  }
  refreshPlay();
}

serverSelect.addEventListener("change", async () => {
  settings.selected_server = serverSelect.value;
  await invoke("save_settings", { settings });
  await renderAccount();
});

// ---------- game files ----------

function showProblems(r: VerifyResult) {
  const lines = [...r.missing.map((p) => `Missing: ${p}`), ...r.corrupt.map((p) => `Modified: ${p}`)];
  const shown = lines.slice(0, 50).map((l) => el("li", { textContent: l }));
  if (lines.length > 50) shown.push(el("li", { textContent: `…and ${lines.length - 50} more` }));
  problemsEl.replaceChildren(...shown);
  problemsEl.hidden = lines.length === 0;
}

async function verify(full: boolean) {
  const dir = settings.game_dir;
  filesOk = false;
  problemsEl.hidden = true;
  if (!dir) {
    setStatus("Tabula Rasa 1.16.5.0 not found. Open Settings and choose your game folder.", "bad");
    refreshPlay();
    return;
  }

  busy = true;
  refreshPlay();
  setStatus(full ? "Verifying all game files (this can take several minutes)…" : "Checking game files…");
  try {
    const r = await invoke<VerifyResult>("verify_files", { dir, full });
    const bad = r.missing.length + r.corrupt.length;
    filesOk = bad === 0;
    if (filesOk) {
      setStatus(`Tabula Rasa ${r.version} — ${r.checked} files OK. Ready to play.`, "ok");
    } else {
      setStatus(`${bad} file(s) are missing or modified. Please reinstall a clean Tabula Rasa ${r.version} client.`, "bad");
      showProblems(r);
    }
  } catch (e) {
    setStatus(String(e), "bad");
  } finally {
    busy = false;
    refreshPlay();
  }
}

listen<{ done: number; total: number }>("verify-progress", ({ payload }) => {
  setProgress(payload.total ? payload.done / payload.total : 0);
});

playBtn.addEventListener("click", async () => {
  try {
    await invoke("launch_game");
    setStatus("Launching Tabula Rasa…", "ok");
  } catch (e) {
    setStatus(String(e), "bad");
  }
});

// ---------- news ----------

const newsList = $<HTMLUListElement>("news");
const tabs = document.querySelectorAll<HTMLButtonElement>(".tab");
let newsItems: NewsItem[] = [];
let activeTab: "news" | "patch" = "news";

tabs.forEach((t) =>
  t.addEventListener("click", () => {
    activeTab = t.dataset.tab as "news" | "patch";
    renderNews();
  }),
);

async function loadNews() {
  try {
    newsItems = await invoke<NewsItem[]>("get_news");
    // The Patch Notes tab only appears when the feed has some.
    tabs[1].hidden = !newsItems.some((n) => n.patch_notes);
    renderNews();
  } catch {
    newsList.replaceChildren(el("li", { className: "muted", textContent: "News is unavailable right now." }));
  }
}

function renderNews() {
  tabs.forEach((t) => t.classList.toggle("active", t.dataset.tab === activeTab));
  const items = newsItems.filter((n) => n.patch_notes === (activeTab === "patch"));
  if (items.length === 0) {
    newsList.replaceChildren(el("li", { className: "muted", textContent: "No news yet." }));
    return;
  }
  newsList.scrollTop = 0;
  newsList.replaceChildren(
    ...items.map((n) => {
      const li = el("li", { className: "news-item", tabIndex: 0 });
      const date = n.date ? new Date(n.date).toLocaleDateString(undefined, { dateStyle: "medium" }) : "";
      li.append(el("span", { className: "news-date", textContent: date }));
      if (n.category && !n.patch_notes) {
        li.append(el("span", { className: "news-tag", textContent: n.category }));
      }
      li.append(
        el("span", { className: "news-title", textContent: n.title }),
        el("p", { className: "news-summary", textContent: n.summary }),
      );
      const go = () => n.link && openUrl(n.link);
      li.addEventListener("click", go);
      li.addEventListener("keydown", (e) => e.key === "Enter" && go());
      return li;
    }),
  );
}

// ---------- window chrome & links ----------

const appWindow = getCurrentWindow();
$("minimize").addEventListener("click", () => appWindow.minimize());
$("close").addEventListener("click", () => appWindow.close());

document.querySelectorAll<HTMLButtonElement>("[data-url]").forEach((b) =>
  b.addEventListener("click", () => openUrl(b.dataset.url!)),
);

// ---------- settings dialog ----------

const settingsEl = $("settings");
const gameDirEl = $<HTMLInputElement>("game-dir");
const skipIntroEl = $<HTMLInputElement>("skip-intro");
const closeOnLaunchEl = $<HTMLInputElement>("close-on-launch");
const serverListEl = $("server-list");

/** Working copy edited by the dialog; only applied on Save. */
interface ServerRow extends Server {
  password: string;
  hasPassword: boolean;
  forget: boolean;
}
let draftDir: string | null = null;
let draftRows: ServerRow[] = [];
let removedIds: string[] = [];

function renderServerRows() {
  serverListEl.replaceChildren(
    ...draftRows.map((row) => {
      const line = el("div", { className: "server-row" });
      const field = (key: "name" | "host" | "username", placeholder: string) => {
        const input = el("input", { value: row[key], placeholder });
        input.addEventListener("input", () => (row[key] = input.value.trim()));
        return input;
      };
      const pw = el("input", {
        type: "password",
        value: row.password,
        placeholder: row.hasPassword && !row.forget ? "•••••• (saved)" : "not saved",
      });
      pw.addEventListener("input", () => (row.password = pw.value));
      const remove = el("button", { textContent: "✕", title: "Remove server or forget password" });
      remove.addEventListener("click", () => {
        if (row.hasPassword && !row.forget && !row.password) {
          row.forget = true; // first click forgets a saved password
        } else {
          removedIds.push(row.id);
          draftRows = draftRows.filter((r) => r !== row);
        }
        renderServerRows();
      });
      remove.title = row.hasPassword && !row.forget ? "Forget saved password" : "Remove server";
      line.append(field("name", "Name"), field("host", "host:port"), field("username", "optional"), pw, remove);
      return line;
    }),
  );
}

async function openSettings() {
  draftDir = settings.game_dir;
  gameDirEl.value = draftDir ?? "";
  skipIntroEl.checked = draftDir ? await invoke<boolean>("get_skip_intro", { dir: draftDir }) : false;
  closeOnLaunchEl.checked = settings.close_on_launch;
  removedIds = [];
  draftRows = await Promise.all(
    settings.servers.map(async (s) => ({
      ...s,
      password: "",
      forget: false,
      hasPassword: await invoke<boolean>("has_password", { serverId: s.id }),
    })),
  );
  renderServerRows();
  settingsEl.hidden = false;
}

$("open-settings").addEventListener("click", openSettings);
$("settings-cancel").addEventListener("click", () => (settingsEl.hidden = true));

$("browse").addEventListener("click", async () => {
  const dir = await open({ directory: true, title: "Select your Tabula Rasa folder" });
  if (typeof dir !== "string") return;
  if (!(await invoke<boolean>("is_game_dir", { dir }))) {
    alert("tabula_rasa.exe was not found in that folder.");
    return;
  }
  draftDir = dir;
  gameDirEl.value = dir;
  skipIntroEl.checked = await invoke<boolean>("get_skip_intro", { dir });
});

$("verify").addEventListener("click", async () => {
  settingsEl.hidden = true;
  await verify(true);
});

$("add-server").addEventListener("click", () => {
  draftRows.push({ id: crypto.randomUUID(), name: "Local", host: "localhost:2106", username: "", password: "", hasPassword: false, forget: false });
  renderServerRows();
});

$("settings-save").addEventListener("click", async () => {
  const invalid = draftRows.find((r) => !r.name || !/^[^\s:]+:\d+$/.test(r.host));
  if (invalid) {
    alert(`Each server needs a name and an address like host:2106.`);
    return;
  }
  if (draftRows.length === 0) {
    alert("Keep at least one server.");
    return;
  }

  try {
    for (const id of removedIds) await invoke("set_password", { serverId: id, password: null });
    for (const row of draftRows) {
      const old = settings.servers.find((s) => s.id === row.id);
      const usernameChanged = old && old.username !== row.username;
      if (row.password && row.username) {
        await invoke("set_password", { serverId: row.id, password: row.password });
      } else if (row.forget || usernameChanged || !row.username) {
        await invoke("set_password", { serverId: row.id, password: null });
      }
    }

    const dirChanged = draftDir !== settings.game_dir;
    settings = {
      game_dir: draftDir,
      servers: draftRows.map(({ id, name, host, username }) => ({ id, name, host, username })),
      selected_server: draftRows.some((r) => r.id === settings.selected_server) ? settings.selected_server : draftRows[0].id,
      close_on_launch: closeOnLaunchEl.checked,
    };
    await invoke("save_settings", { settings });
    if (draftDir) await invoke("set_skip_intro", { dir: draftDir, skip: skipIntroEl.checked }).catch(() => {});

    settingsEl.hidden = true;
    await renderServers();
    if (dirChanged) await verify(false);
  } catch (e) {
    alert(String(e));
  }
});

// ---------- self-update ----------

/** Installs a newer launcher if one is published. Returns true when the app is restarting. */
async function selfUpdate(): Promise<boolean> {
  if (!import.meta.env.PROD) return false;
  try {
    setStatus("Checking for launcher updates…");
    const update = await check();
    if (!update) return false;

    setStatus(`Updating launcher to ${update.version}…`);
    let total = 0;
    let received = 0;
    await update.downloadAndInstall((e) => {
      if (e.event === "Started") total = e.data.contentLength ?? 0;
      if (e.event === "Progress") {
        received += e.data.chunkLength;
        setProgress(total ? received / total : null);
      }
    });
    await relaunch();
    return true;
  } catch {
    return false; // offline or no release yet: carry on with the current version
  }
}

// ---------- startup ----------

(async () => {
  $("version").textContent = `v${await getVersion()}`;
  loadNews();
  if (await selfUpdate()) return;

  settings = await invoke<Settings>("get_settings");
  await renderServers();
  await verify(false);
})();
