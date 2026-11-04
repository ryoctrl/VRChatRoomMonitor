import type { FeedKind, Location, Snapshot, SteamVrStatus } from "./types";

/** 入室からこの時間のあいだは、プレイヤー一覧で強調する。 */
export const RECENT_MILLISECONDS = 2 * 60 * 1000;

export const FEED_ICONS: Record<FeedKind, string> = {
  join: "＋",
  leave: "－",
  room: "■",
  notice: "✉",
  info: "・",
};

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
