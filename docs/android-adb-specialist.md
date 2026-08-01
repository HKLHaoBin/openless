# Android ADB 特种 Debug 包

> **临时特种包**（分支 `special/android-adb-debug-kit`）。  
> 仅给「无法用 UI 导出日志 / 设置页配置服务失败」的用户排查用。  
> **不要当日常包**，**不要合并到 `beta` / 正式发版分支**。问题结束后废弃本分支。

包名：`com.openless.app`  
主 Activity：`com.openless.app/.MainActivity`  
ADB 日志标签：`OpenLessAdb`

---

## 1. 安装

先装本分支 CI 产出的 **debug** APK（artifact：`openless-android-debug-arm64-v8a`）。

```powershell
adb install -r .\OpenLess-android-debug-*.apk
```

- 覆盖安装**同签名**的 debug 包可用 `-r`，保留数据。
- 若手机上已是**正式签名** release，可能无法直接覆盖，需用户同意后 `adb uninstall com.openless.app`（会丢本地数据）再装。
- 装好后先冷启动一次（初始化 `files/OpenLess` 与 `files/logs`）：

```powershell
adb shell am start -n com.openless.app/.MainActivity
```

冷启动后预期私有目录布局：

| 路径（相对 `run-as` cwd） | 用途 |
|--------------------------|------|
| `files/OpenLess/preferences.json` | 用户偏好（切 ASR/LLM、悬浮窗等） |
| `files/OpenLess/credentials.enc.json` | 加密凭据 |
| `files/logs/openless.log` | 会话文件日志（UI 导出 / `ADB_DUMP_LOG` 源） |

**不要**再出现 `/data/local/tmp/openless_prefs_fallback.json`（旧 bug：设置保存必失败）。

App 内「高级 → 调试工具」会显示红字 **ADB Specialist Build**，确认装的是特种包。

---

## 2. 导出会话日志（推荐）

不经过设置页「导出」、不走 SAF。源文件应为 `files/logs/openless.log`。

```powershell
adb logcat -c
adb shell am broadcast -n com.openless.app/.OpenLessAdbDebugReceiver -a com.openless.app.ADB_DUMP_LOG
adb logcat -d -s OpenLessAdb:I OpenLessAdb:E
adb pull /sdcard/Android/data/com.openless.app/files/openless-adb-export.log .
```

成功时 logcat 类似：

```text
I/OpenLessAdb: DUMP_OK path=.../openless-adb-export.log src=.../files/logs/openless.log
```

把电脑上的 `openless-adb-export.log` 发给支持方。

### 备用：`run-as`（仅 debuggable debug 包）

```powershell
adb exec-out run-as com.openless.app sh -c "pwd; ls -la files/OpenLess; ls -la files/logs; find . -name openless.log 2>/dev/null"
adb exec-out run-as com.openless.app cat files/logs/openless.log > openless.log
```

若 `run-as: Package ... is not debuggable`，说明装的不是本特种 debug 包。

---

## 3. ADB 配置 ASR / LLM（绕过设置页）

均需先冷启动过 App。结果看：

```powershell
adb logcat -d -s OpenLessAdb:I OpenLessAdb:E
```

### 3.1 切换当前供应商

```powershell
adb shell am broadcast -n com.openless.app/.OpenLessAdbDebugReceiver -a com.openless.app.ADB_SET_ASR_PROVIDER --es provider deepseek
adb shell am broadcast -n com.openless.app/.OpenLessAdbDebugReceiver -a com.openless.app.ADB_SET_LLM_PROVIDER --es provider deepseek
```

（把 `deepseek` 换成实际 provider id。）

### 3.2 写入单条凭据

`account` 与设置页 IPC 一致，例如：

| account | 含义 |
|---------|------|
| `asr.api_key` | ASR API Key |
| `asr.endpoint` | ASR Base URL |
| `asr.model` | ASR 模型 |
| `asr.vocabulary_id` | ASR 热词表（如有） |
| `ark.api_key` | LLM API Key |
| `ark.model_id` | LLM 模型 |
| `ark.endpoint` | LLM Base URL |
| `volcengine.app_key` / `access_key` / `resource_id` | 火山 |

ASR 按供应商隔离的字段可带 `--es provider <id>`：

```powershell
adb shell am broadcast -n com.openless.app/.OpenLessAdbDebugReceiver `
  -a com.openless.app.ADB_SET_CREDENTIAL `
  --es account asr.api_key `
  --es value "YOUR_KEY" `
  --es provider deepseek
```

LLM 示例：

```powershell
adb shell am broadcast -n com.openless.app/.OpenLessAdbDebugReceiver `
  -a com.openless.app.ADB_SET_CREDENTIAL `
  --es account ark.api_key `
  --es value "YOUR_KEY"
```

成功：`SET_OK account=...`

### 3.3 批量 JSON

1. 在电脑上建 `openless-creds.json`（**勿提交 git**）：

```json
{
  "active_asr": "deepseek",
  "active_llm": "deepseek",
  "credentials": [
    { "account": "asr.api_key", "value": "YOUR_ASR_KEY", "provider": "deepseek" },
    { "account": "asr.model", "value": "YOUR_ASR_MODEL", "provider": "deepseek" },
    { "account": "ark.api_key", "value": "YOUR_LLM_KEY" },
    { "account": "ark.model_id", "value": "YOUR_LLM_MODEL" }
  ]
}
```

2. 推到应用外部目录（推荐，避免 Download 权限问题）：

```powershell
adb push .\openless-creds.json /sdcard/Android/data/com.openless.app/files/openless-creds.json
adb shell am broadcast -n com.openless.app/.OpenLessAdbDebugReceiver `
  -a com.openless.app.ADB_APPLY_CREDS_JSON `
  --es path /sdcard/Android/data/com.openless.app/files/openless-creds.json
```

成功：`APPLY_OK`。然后重启 App 或回到设置页刷新。

### 3.4 连通性检查

```powershell
adb shell am broadcast -n com.openless.app/.OpenLessAdbDebugReceiver -a com.openless.app.ADB_VALIDATE --es kind asr
adb shell am broadcast -n com.openless.app/.OpenLessAdbDebugReceiver -a com.openless.app.ADB_VALIDATE --es kind llm
adb logcat -d -s OpenLessAdb:I OpenLessAdb:E
```

`VALIDATE_OK` / `VALIDATE_ERR ...` 会打出失败原因。

---

## 4. 安全说明

- Receiver 在运行时检查 `FLAG_DEBUGGABLE`；正式 release 包会打 `REJECTED not debuggable`。
- 含密钥的 JSON 只放本机临时目录，用完删除，不要发到公开 Issue。
- 本分支 CI **禁止**打 `v*-tauri` tag 发正式包。

---

## 5. CI 触发（维护者）

```powershell
git push -u origin special/android-adb-debug-kit
gh workflow run "Android APK (ADB specialist debug)" --repo HKLHaoBin/openless --ref special/android-adb-debug-kit
```

Artifact：`openless-android-debug-arm64-v8a`。
