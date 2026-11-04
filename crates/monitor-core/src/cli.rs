//! コマンドライン引数。利用者向けではなく、SteamVR からの起動と動作確認に使う。

use std::path::PathBuf;

pub const FROM_STEAMVR: &str = "--steamvr";
pub const QUIT: &str = "--quit";

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Args {
    /// VRChat のログフォルダ（既定は標準の場所）
    pub log_dir: Option<PathBuf>,
    /// 設定などの保存先（既定はアプリのデータフォルダ）
    pub data_dir: Option<PathBuf>,
    /// SteamVR から起動された。SteamVR の終了時に一緒に終了する
    pub from_steamvr: bool,
    /// 起動中のアプリを終了させる
    pub quit: bool,
    /// 同梱ライブラリを読み込めるかだけを確かめて終了する
    pub self_check: bool,
    /// SteamVR と連携しない（動作確認で実際の SteamVR に登録しないため）
    pub no_steamvr: bool,
}

impl Args {
    pub fn parse(args: impl IntoIterator<Item = String>) -> Self {
        let mut parsed = Self::default();
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--log-dir" => parsed.log_dir = args.next().map(PathBuf::from),
                "--data-dir" => parsed.data_dir = args.next().map(PathBuf::from),
                FROM_STEAMVR => parsed.from_steamvr = true,
                QUIT => parsed.quit = true,
                "--self-check" => parsed.self_check = true,
                "--no-steamvr" => parsed.no_steamvr = true,
                _ => {}
            }
        }
        parsed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Args {
        Args::parse(args.iter().map(ToString::to_string))
    }

    #[test]
    fn no_arguments_use_defaults() {
        assert_eq!(parse(&[]), Args::default());
    }

    #[test]
    fn reads_values_and_flags() {
        let args = parse(&["--log-dir", r"C:\logs", "--steamvr", "--no-steamvr"]);
        assert_eq!(args.log_dir, Some(PathBuf::from(r"C:\logs")));
        assert!(args.from_steamvr);
        assert!(args.no_steamvr);
        assert!(!args.quit);
    }

    #[test]
    fn ignores_unknown_arguments_and_missing_values() {
        let args = parse(&["--unknown", "--data-dir"]);
        assert_eq!(args, Args::default());
    }
}
