import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  describeInstance,
  describeStatus,
  describeSteamVr,
  FEED_ICONS,
  formatClock,
  formatDuration,
  formatFeedTime,
  RECENT_MILLISECONDS,
} from "./format";
import type { Player, Settings, Snapshot } from "./types";

function element<T extends HTMLElement>(id: string): T {
  const found = document.getElementById(id);
  if (!found) {
    throw new Error(`missing element: ${id}`);
  }
  return found as T;
}

const statusLine = element("status");
const world = element("world");
const instance = element("instance");
const count = element("count");
const playerRows = element<HTMLTableSectionElement>("players");
const feedRows = element<HTMLTableSectionElement>("feed");
const steamVrAutostart = element<HTMLInputElement>("steamvr-autostart");
const steamVrStatus = element("steamvr-status");
const steamVrRemove = element<HTMLButtonElement>("steamvr-remove");
const licenses = element<HTMLButtonElement>("licenses");
const settingInputs = document.querySelectorAll<HTMLInputElement>("input[data-setting]");

let current: Snapshot | null = null;
/** 行の内容とは別に、毎秒更新する経過時間のセルを覚えておく。 */
let elapsedCells: { cell: HTMLTableCellElement; row: HTMLTableRowElement; player: Player }[] = [];

/** 表示名やワールド名には任意の文字が入るので、HTML として解釈させず必ず文字列として入れる。 */
function cell(text: string): HTMLTableCellElement {
  const td = document.createElement("td");
  td.textContent = text;
  td.title = text;
  return td;
}

function playerClass(player: Player, now: number): string {
  if (player.isLocal) {
    return "local";
  }
  return !player.initial && now - player.joinedAt < RECENT_MILLISECONDS ? "recent" : "";
}

function renderPlayers(players: Player[], now: number): void {
  elapsedCells = [];
  const rows = players.map((player) => {
    const row = document.createElement("tr");
    const elapsed = cell(formatDuration(now - player.joinedAt));
    row.append(
      cell(player.isLocal ? `${player.name}（自分）` : player.name),
      cell(player.initial ? "先客" : formatClock(player.joinedAt)),
      elapsed,
    );
    row.className = playerClass(player, now);
    elapsedCells.push({ cell: elapsed, row, player });
    return row;
  });
  playerRows.replaceChildren(...rows);
}

function renderFeed(snapshot: Snapshot, now: number): void {
  const rows = snapshot.feed.map((entry) => {
    const row = document.createElement("tr");
    row.className = entry.kind;
    row.append(
      cell(formatFeedTime(entry.time, now)),
      cell(`${FEED_ICONS[entry.kind]} ${entry.text}`),
    );
    return row;
  });
  feedRows.replaceChildren(...rows);
}

function renderOptions(snapshot: Snapshot): void {
  for (const input of settingInputs) {
    const key = input.dataset.setting as keyof Settings;
    input.checked = snapshot.settings[key];
  }
  const { steamvr } = snapshot;
  steamVrAutostart.checked = steamvr.autostart;
  steamVrAutostart.disabled = !steamvr.installed;
  steamVrStatus.textContent = describeSteamVr(steamvr);
  steamVrRemove.hidden = !steamvr.listed;
}

function renderInstance(snapshot: Snapshot, now: number): void {
  instance.textContent = describeInstance(snapshot.location, snapshot.enteredAt, now);
}

function render(snapshot: Snapshot): void {
  current = snapshot;
  const now = Date.now();
  const status = describeStatus(snapshot);
  statusLine.textContent = status.text;
  statusLine.className = `status ${status.className}`;
  world.textContent = snapshot.worldName ?? "—";
  count.textContent =
    snapshot.status === "inRoom" ? `ルーム内 ${snapshot.players.length} 人` : "ルーム内 —";
  renderInstance(snapshot, now);
  renderPlayers(snapshot.players, now);
  renderFeed(snapshot, now);
  renderOptions(snapshot);
}

/** 経過時間だけを更新する。ウィンドウが隠れている間は何もしない。 */
function tick(): void {
  if (document.hidden || !current) {
    return;
  }
  const now = Date.now();
  renderInstance(current, now);
  for (const { cell: elapsed, row, player } of elapsedCells) {
    elapsed.textContent = formatDuration(now - player.joinedAt);
    row.className = playerClass(player, now);
  }
}

for (const input of settingInputs) {
  input.addEventListener("change", () => {
    const key = input.dataset.setting as keyof Settings;
    void invoke("update_settings", { patch: { [key]: input.checked } });
  });
}

/** SteamVR の操作中はボタンを押せなくし、失敗したら理由を見せて表示を元に戻す。 */
async function runSteamVrAction(action: () => Promise<unknown>): Promise<void> {
  steamVrAutostart.disabled = true;
  steamVrRemove.disabled = true;
  steamVrStatus.textContent = "SteamVR に接続中…";
  try {
    await action();
  } catch (error) {
    window.alert(`SteamVR の設定を変更できませんでした。\n${String(error)}`);
  }
  steamVrRemove.disabled = false;
  render(await invoke<Snapshot>("get_snapshot"));
}

steamVrAutostart.addEventListener("change", () => {
  const enabled = steamVrAutostart.checked;
  void runSteamVrAction(() => invoke("set_steamvr_autostart", { enabled }));
});

steamVrRemove.addEventListener("click", () => {
  void runSteamVrAction(() => invoke("remove_steamvr_entry"));
});

licenses.addEventListener("click", () => {
  void invoke("open_licenses");
});

await listen<Snapshot>("snapshot", (event) => render(event.payload));
render(await invoke<Snapshot>("get_snapshot"));
setInterval(tick, 1000);
