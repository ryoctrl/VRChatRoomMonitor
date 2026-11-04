# OpenVR API

`openvr_api.dll` は [ValveSoftware/openvr](https://github.com/ValveSoftware/openvr) の `v2.15.6` に含まれる `bin/win64/openvr_api.dll` です。ライセンスは同じフォルダの `LICENSE`（BSD 3-Clause）です。

更新するときは DLL を差し替え、`src/api.rs` の `OPENVR_VERSION`、インターフェースの版、関数の番号を、同じ版の `headers/openvr_capi.h` に合わせてください。
