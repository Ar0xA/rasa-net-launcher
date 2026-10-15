import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";

interface VerifyResult {
  version: string;
  checked: number;
  missing: string[];
  corrupt: string[];
}

const $ = <T extends HTMLElement>(id: string) => document.getElementById(id) as T;
const statusEl = $("status");
const progressEl = $<HTMLProgressElement>("progress");
const problemsEl = $<HTMLUListElement>("problems");
const playBtn = $<HTMLButtonElement>("play");
const verifyBtn = $<HTMLButtonElement>("verify");
const browseBtn = $<HTMLButtonElement>("browse");
const pathEl = $("path");
const skipIntroEl = $<HTMLInputElement>("skip-intro");

let gameDir: string | null = null;
let busy = false;

function setStatus(text: string, kind: "" | "ok" | "bad" = "") {
  statusEl.textContent = text;
  statusEl.className = kind;
}

function refreshButtons(valid: boolean) {
  playBtn.disabled = busy || !valid;
  verifyBtn.disabled = busy || !gameDir;
  browseBtn.disabled = busy;
}

function showProblems(r: VerifyResult) {
  const lines = [
    ...r.missing.map((p) => `Missing: ${p}`),
    ...r.corrupt.map((p) => `Modified: ${p}`),
  ];
  problemsEl.replaceChildren(
    ...lines.slice(0, 50).map((l) => Object.assign(document.createElement("li"), { textContent: l })),
  );
  if (lines.length > 50) {
    problemsEl.append(Object.assign(document.createElement("li"), { textContent: `…and ${lines.length - 50} more` }));
  }
  problemsEl.hidden = lines.length === 0;
}

async function verify(full: boolean) {
  if (!gameDir) return;
  skipIntroEl.checked = await invoke<boolean>("get_skip_intro", { dir: gameDir });
  busy = true;
  refreshButtons(false);
  problemsEl.hidden = true;
  progressEl.hidden = false;
  progressEl.removeAttribute("value");
  setStatus(full ? "Verifying all game files (this can take several minutes)…" : "Checking game files…");

  try {
    const r = await invoke<VerifyResult>("verify_files", { dir: gameDir, full });
    const bad = r.missing.length + r.corrupt.length;
    if (bad === 0) {
      setStatus(`Tabula Rasa ${r.version} — ${r.checked} files OK. Ready to play.`, "ok");
    } else {
      setStatus(
        `${bad} file(s) are missing or modified. Please reinstall a clean Tabula Rasa ${r.version} client.`,
        "bad",
      );
      showProblems(r);
    }
    busy = false;
    refreshButtons(bad === 0);
  } catch (e) {
    busy = false;
    setStatus(String(e), "bad");
    refreshButtons(false);
  } finally {
    progressEl.hidden = true;
  }
}

async function useFolder(dir: string) {
  await invoke("set_game_dir", { dir });
  gameDir = dir;
  pathEl.textContent = dir;
  await verify(false);
}

browseBtn.addEventListener("click", async () => {
  const dir = await open({ directory: true, title: "Select your Tabula Rasa folder" });
  if (typeof dir !== "string") return;
  try {
    await useFolder(dir);
  } catch (e) {
    setStatus(String(e), "bad");
  }
});

verifyBtn.addEventListener("click", () => verify(true));

playBtn.addEventListener("click", async () => {
  if (!gameDir) return;
  try {
    await invoke("launch_game", { dir: gameDir, skipIntro: skipIntroEl.checked });
    setStatus("Launching Tabula Rasa…", "ok");
  } catch (e) {
    setStatus(String(e), "bad");
  }
});

listen<{ done: number; total: number }>("verify-progress", ({ payload }) => {
  progressEl.max = payload.total;
  progressEl.value = payload.done;
});

(async () => {
  gameDir = await invoke<string | null>("get_game_dir");
  if (gameDir) {
    pathEl.textContent = gameDir;
    await verify(false);
  } else {
    setStatus("Tabula Rasa 1.16.5.0 is required. Choose the folder where it is installed.");
    refreshButtons(false);
  }
})();
