import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  currentWorldFeed,
  describeInstance,
  describeStatus,
  describeSteamVr,
  describeUpdate,
  FEED_ICONS,
  formatClock,
  formatDuration,
  formatFeedTime,
  RECENT_MILLISECONDS,
  SOUND_OPTIONS,
} from "./format";
import type { Player, Snapshot, SoundSetting, SwitchSetting } from "./types";

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
const feedEmpty = element("feed-empty");
const steamVrAutostart = element<HTMLInputElement>("steamvr-autostart");
const steamVrStatus = element("steamvr-status");
const steamVrRemove = element<HTMLButtonElement>("steamvr-remove");
const licenses = element<HTMLButtonElement>("licenses");
const updateBanner = element("update");
const updateText = element("update-text");
const updateAction = element<HTMLButtonElement>("update-action");
const version = element("version");
const settingInputs = document.querySelectorAll<HTMLInputElement>("input[data-setting]");
const soundSelects = document.querySelectorAll<HTMLSelectElement>("select[data-sound]");

let current: Snapshot | null = null;
let updateCommand: string | null = null;
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
  const currentOnly = snapshot.settings.currentWorldOnly;
  const entries = currentOnly ? currentWorldFeed(snapshot) : snapshot.feed;
  feedEmpty.hidden = !currentOnly || entries.length > 0;
  const rows = entries.map((entry) => {
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
    const key = input.dataset.setting as SwitchSetting;
    input.checked = snapshot.settings[key];
  }
  for (const select of soundSelects) {
    const key = select.dataset.sound as SoundSetting;
    select.value = snapshot.settings[key];
    // 退室の音は「退室も通知」がオンのときだけ鳴る
    select.disabled = key === "soundLeave" && !snapshot.settings.notifyLeave;
  }
  const { steamvr } = snapshot;
  steamVrAutostart.checked = steamvr.autostart;
  steamVrAutostart.disabled = !steamvr.installed;
  steamVrStatus.textContent = describeSteamVr(steamvr);
  steamVrRemove.hidden = !steamvr.listed;
}

function renderUpdate(snapshot: Snapshot): void {
  version.textContent = `v${snapshot.appVersion}`;
  const view = describeUpdate(snapshot.update);
  updateBanner.hidden = view === null;
  updateText.textContent = view?.text ?? "";
  updateAction.hidden = !view?.action;
  updateAction.textContent = view?.action?.label ?? "";
  updateCommand = view?.action?.command ?? null;
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
  renderUpdate(snapshot);
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
    const key = input.dataset.setting as SwitchSetting;
    void invoke("update_settings", { patch: { [key]: input.checked } });
  });
}

for (const select of soundSelects) {
  select.append(...SOUND_OPTIONS.map(({ value, label }) => new Option(label, value)));
  select.addEventListener("change", () => {
    const key = select.dataset.sound as SoundSetting;
    void invoke("update_settings", { patch: { [key]: select.value } });
    // 選んだ音をその場で確かめられるように鳴らす
    void invoke("preview_sound", { sound: select.value });
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

updateAction.addEventListener("click", () => {
  if (updateCommand) {
    updateAction.disabled = true;
    void invoke(updateCommand)
      .catch(() => {
        // 失敗の内容は、状態の更新として画面に届く
      })
      .finally(() => {
        updateAction.disabled = false;
      });
  }
});

licenses.addEventListener("click", () => {
  void invoke("open_licenses");
});

await listen<Snapshot>("snapshot", (event) => render(event.payload));
render(await invoke<Snapshot>("get_snapshot"));
setInterval(tick, 1000);
