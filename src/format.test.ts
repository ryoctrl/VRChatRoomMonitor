import { describe, expect, it } from "vitest";
import {
  currentWorldFeed,
  describeInstance,
  describeStatus,
  describeSteamVr,
  describeUpdate,
  formatDuration,
  formatFeedTime,
  formatTimeOfDay,
  SOUND_OPTIONS,
} from "./format";
import type { FeedEntry, FeedKind } from "./types";

const at = (hour: number, minute: number, second = 0, day = 5): number =>
  new Date(2026, 10, day, hour, minute, second).getTime();

describe("formatDuration", () => {
  it("formats minutes and hours", () => {
    expect(formatDuration(281_000)).toBe("4:41");
    expect(formatDuration(3_725_000)).toBe("1:02:05");
  });

  it("treats a future time as zero", () => {
    expect(formatDuration(-5_000)).toBe("0:00");
  });
});

describe("formatTimeOfDay", () => {
  it("shows hours, minutes and seconds with leading zeros", () => {
    expect(formatTimeOfDay(at(9, 5, 7))).toBe("09:05:07");
    expect(formatTimeOfDay(at(23, 59, 59))).toBe("23:59:59");
  });
});

describe("formatFeedTime", () => {
  it("shows seconds for today", () => {
    expect(formatFeedTime(at(21, 5, 9), at(22, 0))).toBe("21:05:09");
  });

  it("shows the date for earlier days", () => {
    expect(formatFeedTime(at(21, 5, 9, 4), at(1, 0))).toBe("11/04 21:05");
  });
});

describe("currentWorldFeed", () => {
  const entry = (kind: FeedKind, text: string): FeedEntry => ({ time: 0, kind, text });
  const feed = [
    entry("leave", "Carol が退室"),
    entry("notice", "招待: Alice → 別のワールド"),
    entry("join", "Dave が入室"),
    entry("join", "Carol が入室"),
    entry("info", "入室完了（先客 1 人）"),
    entry("room", "今のワールド"),
    entry("join", "Bob が入室"),
    entry("room", "前のワールド"),
  ];
  const texts = (entries: FeedEntry[]): string[] => entries.map((item) => item.text);

  it("keeps the current room's lines, including its name, without notices", () => {
    expect(texts(currentWorldFeed({ worldName: "今のワールド", feed }))).toEqual([
      "Carol が退室",
      "Dave が入室",
      "Carol が入室",
      "入室完了（先客 1 人）",
      "今のワールド",
    ]);
  });

  it("shows nothing outside a room, even right after leaving one", () => {
    expect(currentWorldFeed({ worldName: null, feed })).toEqual([]);
  });

  it("shows only the room name right after entering a room", () => {
    const entered = [entry("room", "新しいワールド"), ...feed];
    expect(texts(currentWorldFeed({ worldName: "新しいワールド", feed: entered }))).toEqual([
      "新しいワールド",
    ]);
  });

  it("keeps the remaining lines when the room line has dropped out of the limit", () => {
    const lines = [
      entry("notice", "招待: Alice → 別のワールド"),
      entry("join", "Dave が入室"),
      entry("leave", "Carol が退室"),
    ];
    expect(texts(currentWorldFeed({ worldName: "今のワールド", feed: lines }))).toEqual([
      "Dave が入室",
      "Carol が退室",
    ]);
  });
});

describe("SOUND_OPTIONS", () => {
  it("offers every sound once", () => {
    const values = SOUND_OPTIONS.map((option) => option.value);
    expect(values).toEqual(["chime", "bell", "marimba", "pop", "soft", "windows", "silent"]);
    expect(new Set(SOUND_OPTIONS.map((option) => option.label)).size).toBe(values.length);
  });
});

describe("describeInstance", () => {
  it("joins location and elapsed time", () => {
    const location = { worldId: "w", instanceName: "10129", access: "Friends+", region: "JP" };
    expect(describeInstance(location, at(21, 0), at(21, 5))).toBe(
      "Friends+ · JP · #10129 · 入室 21:00（5:00）",
    );
  });

  it("is empty outside a room", () => {
    expect(describeInstance(null, null, at(21, 0))).toBe("");
  });
});

describe("describeStatus", () => {
  it("shows the problem message", () => {
    expect(describeStatus({ status: "noLog", problem: "ログが見つかりません" })).toEqual({
      text: "ログが見つかりません",
      className: "problem",
    });
  });

  it("marks the room state", () => {
    expect(describeStatus({ status: "inRoom", problem: null }).className).toBe("in-room");
  });
});

describe("describeSteamVr", () => {
  const status = { installed: true, connected: true, listed: true, autostart: false };

  it("reports connection and listing", () => {
    expect(describeSteamVr(status)).toBe("接続中・一覧に登録済み");
    expect(describeSteamVr({ ...status, connected: false, listed: false })).toBe(
      "未起動・一覧に未登録",
    );
  });

  it("reports a missing installation", () => {
    expect(describeSteamVr({ ...status, installed: false })).toBe("SteamVR が見つかりません");
  });
});

describe("describeUpdate", () => {
  it("shows nothing without an update", () => {
    expect(describeUpdate({ state: "none" })).toBeNull();
  });

  it("installs in place only for the installer version", () => {
    const installed = describeUpdate({ state: "available", version: "0.3.0", installable: true });
    expect(installed?.text).toBe("新しい版 0.3.0 があります");
    expect(installed?.action?.command).toBe("install_update");
    const portable = describeUpdate({ state: "available", version: "0.3.0", installable: false });
    expect(portable?.action?.command).toBe("open_releases");
  });

  it("shows progress without a button while installing", () => {
    const view = describeUpdate({ state: "installing", version: "0.3.0", percent: 42 });
    expect(view).toEqual({ text: "0.3.0 に更新中… 42%", action: null });
    expect(describeUpdate({ state: "installing", version: "0.3.0", percent: null })?.text).toBe(
      "0.3.0 に更新中…",
    );
  });

  it("offers a retry after a failure", () => {
    const view = describeUpdate({ state: "failed", version: "0.3.0", message: "signature" });
    expect(view?.text).toBe("更新に失敗しました: signature");
    expect(view?.action?.command).toBe("install_update");
  });
});
