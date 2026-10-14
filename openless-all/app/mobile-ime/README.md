# OpenLess 移动端 Rime

Android 和鸿蒙直接链接 librime 1.17.0，共用本目录 C ABI 会话实现；不依赖 Fcitx / Fcitx5。Linux 现有插件不在本次迁移范围。

`rime_engine.cpp` 提供初始化、创建会话、按键、候选选择、确认、读取状态、取消、提交确认和销毁。原生客户端对象不跨平台传递。待提交文本保留至平台写入成功后 ack；重试或取消由宿主处理。密码框不进入中文路径，禁用学习时使用 `enable_user_dict: false` 的独立拼音方案。

鸿蒙通过 `libopenless_harmony.so` 的 `rimeAsync` 在工作线程调用 C ABI；Android 通过 `libopenless_rime.so` 的 JNI 调用。两端都保留各自原生 IME 生命周期及文本提交接口。

构建使用 `scripts/build-mobile-rime.py`，依赖、词库、编译产物在 `/data-win/openless-cache/mobile-rime`，可通过 `OPENLESS_CACHE_ROOT` 覆盖。引擎版本和输入资源修订固定在脚本中；开发构建不下载个人词库或上传按键数据。

首次构建：

```bash
python3 scripts/build-mobile-rime.py harmony
python3 scripts/build-mobile-rime.py android --ndk /path/to/android-ndk
```

Android 支持 `--abi arm64-v8a|armeabi-v7a|x86|x86_64`。`--stage` 将对应 ABI 原生库、libc++ 和词库复制到已生成的 Android 工程；正常 `npm run tauri:android:build` 自动执行该步骤。

鸿蒙签名包和真机自检已通过；Android arm64 JNI 已实际编译并核对 ELF 架构、依赖及导出符号。其余 ABI、完整 Android APK 和旧学习词库迁移尚待验证。

当前使用官方 `pinyin_simp` 方案，保留上游版权和资源许可。它不是 Linux 所有 Rime 方案与自定义词库的完整替代；更多方案、用户词库迁移和键盘体验需要逐步完善。
