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

export type SoundName = "chime" | "bell" | "marimba" | "pop" | "soft" | "windows" | "silent";

export interface Settings {
  alwaysOnTop: boolean;
  popup: boolean;
  sound: boolean;
  notifyLeave: boolean;
  checkUpdates: boolean;
  currentWorldJoinsOnly: boolean;
  soundJoin: SoundName;
  soundLeave: SoundName;
  soundNotice: SoundName;
}

export type SoundSetting = "soundJoin" | "soundLeave" | "soundNotice";

export type SwitchSetting = Exclude<keyof Settings, SoundSetting>;

export interface SteamVrStatus {
  installed: boolean;
  connected: boolean;
  listed: boolean;
  autostart: boolean;
}

export type UpdateStatus =
  | { state: "none" }
  | { state: "available"; version: string; installable: boolean }
  | { state: "installing"; version: string; percent: number | null }
  | { state: "failed"; version: string; message: string };

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
  update: UpdateStatus;
  appVersion: string;
}
