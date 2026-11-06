//! VRChat のログを解析・監視して、ルームの状態と入退室の記録を管理する。GUI には依存しない。

pub mod cli;
pub mod location;
pub mod monitor;
pub mod parser;
pub mod room;
pub mod settings;
pub mod sound;
pub mod watcher;
