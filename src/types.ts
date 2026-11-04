/** Rust 側の Snapshot と同じ形。 */
export type Status = "loading" | "noLog" | "error" | "offline" | "inRoom" | "moving";

export type FeedKind = "join" | "leave" | "room" | "notice" | "info";

export interface Location {
  worldId: string;
  instanceName: string;
  access: string;
  region: string;
}

export interface Player {
  key: string;
  name: string;
  joinedAt: number;
  initial: boolean;
  isLocal: boolean;
}

export interface FeedEntry {
  time: number;
  kind: FeedKind;
  text: string;
}

export interface Settings {
  alwaysOnTop: boolean;
  popup: boolean;
  sound: boolean;
  notifyLeave: boolean;
}

export interface SteamVrStatus {
  installed: boolean;
  connected: boolean;
  listed: boolean;
  autostart: boolean;
}

export interface Snapshot {
  status: Status;
  problem: string | null;
  worldName: string | null;
  location: Location | null;
  enteredAt: number | null;
  players: Player[];
  feed: FeedEntry[];
  settings: Settings;
  steamvr: SteamVrStatus;
}
