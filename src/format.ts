import type {
  FeedEntry,
  FeedKind,
  Location,
  Snapshot,
  SoundName,
  SteamVrStatus,
  UpdateStatus,
} from "./types";

/** 入室からこの時間のあいだは、プレイヤー一覧で強調する。 */
export const RECENT_MILLISECONDS = 2 * 60 * 1000;

export const FEED_ICONS: Record<FeedKind, string> = {
  join: "＋",
  leave: "－",
  room: "■",
  notice: "✉",
  info: "・",
};

export const SOUND_OPTIONS: { value: SoundName; label: string }[] = [
  { value: "chime", label: "チャイム" },
  { value: "bell", label: "ベル" },
  { value: "marimba", label: "マリンバ" },
  { value: "pop", label: "ポップ" },
  { value: "soft", label: "ソフト" },
  { value: "windows", label: "Windows 標準" },
  { value: "silent", label: "なし" },
];

const pad = (value: number): string => String(value).padStart(2, "0");

export function formatDuration(milliseconds: number): string {
  const total = Math.max(0, Math.floor(milliseconds / 1000));
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const seconds = total % 60;
  return hours > 0 ? `${hours}:${pad(minutes)}:${pad(seconds)}` : `${minutes}:${pad(seconds)}`;
}

export function formatClock(time: number): string {
  const date = new Date(time);
  return `${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

/** 今日の記録は秒まで、それ以前の記録は日付つきで表示する。 */
export function formatFeedTime(time: number, now: number): string {
  const date = new Date(time);
  if (date.toDateString() === new Date(now).toDateString()) {
    return `${formatClock(time)}:${pad(date.getSeconds())}`;
  }
  return `${pad(date.getMonth() + 1)}/${pad(date.getDate())} ${formatClock(time)}`;
}

/**
 * 今いるルームの記録だけを、ルーム名の行も含めて返す。記録は新しいものが先頭なので、最初に見つかる
 * ルーム名の行までが今のルームの分。その行が上限で消えていれば、残りはすべて今のルームの分。
 * 招待などの通知は、差出人や別のワールド名が配信に映らないよう除く。
 */
export function currentWorldFeed(snapshot: Pick<Snapshot, "worldName" | "feed">): FeedEntry[] {
  if (snapshot.worldName === null) {
    return [];
  }
  const roomLine = snapshot.feed.findIndex((entry) => entry.kind === "room");
  const current = roomLine === -1 ? snapshot.feed : snapshot.feed.slice(0, roomLine + 1);
  return current.filter((entry) => entry.kind !== "notice");
}

export function describeInstance(
  location: Location | null,
  enteredAt: number | null,
  now: number,
): string {
  const parts: string[] = [];
  if (location) {
    parts.push(location.access, location.region, `#${location.instanceName}`);
  }
  if (enteredAt !== null) {
    parts.push(`入室 ${formatClock(enteredAt)}（${formatDuration(now - enteredAt)}）`);
  }
  return parts.join(" · ");
}

export interface StatusView {
  text: string;
  className: string;
}

export function describeStatus(snapshot: Pick<Snapshot, "status" | "problem">): StatusView {
  switch (snapshot.status) {
    case "loading":
      return { text: "過去のログを読み込み中…", className: "" };
    case "noLog":
    case "error":
      return { text: snapshot.problem ?? "エラーが発生しました", className: "problem" };
    case "offline":
      return { text: "● VRChat は起動していません", className: "offline" };
    case "inRoom":
      return { text: "● ルーム内", className: "in-room" };
    case "moving":
      return { text: "● ワールド移動中", className: "moving" };
  }
}

export function describeSteamVr(status: SteamVrStatus): string {
  if (!status.installed) {
    return "SteamVR が見つかりません";
  }
  const listing = status.listed ? "一覧に登録済み" : "一覧に未登録";
  return status.connected ? `接続中・${listing}` : `未起動・${listing}`;
}

export interface UpdateView {
  text: string;
  /** ボタンの動作。無いときはボタンを出さない */
  action: { label: string; command: "install_update" | "open_releases" } | null;
}

export function describeUpdate(update: UpdateStatus): UpdateView | null {
  switch (update.state) {
    case "none":
      return null;
    case "available":
      return {
        text: `新しい版 ${update.version} があります`,
        action: update.installable
          ? { label: "更新して再起動", command: "install_update" }
          : { label: "配布ページを開く", command: "open_releases" },
      };
    case "installing": {
      const progress = update.percent === null ? "" : ` ${update.percent}%`;
      return { text: `${update.version} に更新中…${progress}`, action: null };
    }
    case "failed":
      return {
        text: `更新に失敗しました: ${update.message}`,
        action: { label: "再試行", command: "install_update" },
      };
  }
}
