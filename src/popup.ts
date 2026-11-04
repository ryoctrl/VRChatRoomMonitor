import { invoke } from "@tauri-apps/api/core";

interface Popup {
  title: string;
  body: string;
  kind: string;
}

/** Rust 側がウィンドウの作成時に渡す内容。 */
const popup = (window as unknown as { __POPUP__: Popup }).__POPUP__;

const root = document.body;
const title = document.getElementById("title");
const body = document.getElementById("body");
if (title && body) {
  // 表示名やワールド名には任意の文字が入るので、HTML として解釈させず必ず文字列として入れる
  title.textContent = popup.title;
  body.textContent = popup.body;
}
root.className = popup.kind;
root.addEventListener("click", () => {
  void invoke("dismiss_popup");
});
