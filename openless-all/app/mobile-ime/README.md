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

鸿蒙签名包和真机自检已通过；Android arm64 Release APK 已构建，x86_64 的真实 JNI、词库及 Android 控制器通过 CI 模拟器测试。Android 实机手感、其余 ABI 和旧学习词库迁移尚待验证。

当前使用官方 `pinyin_simp` 方案，保留上游版权和资源许可。它不是 Linux 所有 Rime 方案与自定义词库的完整替代；更多方案、用户词库迁移和键盘体验需要逐步完善。

Android 保留现有键盘布局和候选展开列表。后端通过 `candidates` 返回原生顺序的前 64 个候选，通过 `select_absolute` 按绝对索引及版本选择；鸿蒙现有的分页 `select` 接口继续按当前页索引工作。只读 `state` / `candidates` 不增加候选版本，避免后台刷新使可见候选失效。候选展开有数量上限；不代表穷举词库或增加词条。

Android 的空格、删除、回车与字母在同一个串行队列中执行，是否仍有拼音组合由执行时的 Rime 状态决定；拼音中的单引号作为音节分隔符处理。刷新键盘重新读取组合和候选。上屏失败保留待提交文字及其后的标点，成功才确认，输入会话切换则清除队列所属状态。回归测试覆盖快打后连续空格/删除/回车、首屏以外的候选选择、只读刷新、分隔符、写入失败重试及编辑器切换。
