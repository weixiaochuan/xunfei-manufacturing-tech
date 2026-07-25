# 知识库 Win7 兼容分析

> **日期**: 2026-06-14
> **分支**: feature/win7-compat
> **基础**: Tauri 2.10.3 + Rust 1.96

---

## 一、已完成的兼容适配

| 适配 | 文件 | 说明 |
|------|------|------|
| CSS oklch/color-mix 回退 | src/styles/global.css | @supports not 块覆盖 Chromium 109 |
| WebView2 109 Fixed Runtime | src-tauri/tauri.conf.json | 捆绑 WebView2 109.x |
| Win7 专属配置 | src-tauri/tauri.win7.conf.json | 含 fixedRuntime 的完整配置副本 |
| 双分支构建脚本 | scripts/build-win7.mjs | 一键构建 Win7 版 |
| PE 子系统锁定 | src-tauri/build.rs | /SUBSYSTEM:WINDOWS,6.01 |
| NSIS OS 检测 | src-tauri/installer-hooks.nsi | Win7 SP1 检查 + Win10+ 警告 |
| 构建文档 | docs/win7-build.md | 构建流程与系统要求 |

---

## 二、Win7 运行时错误

### 错误 1: GetSystemTimePreciseAsFileTime

```
无法定位程序输入点 GetSystemTimePreciseAsFileTime 于动态链接库 kernel32.dll 上
```

**来源**: Rust 1.96 std / chrono 等依赖使用了 Win8+ 才引入的 `kernel32!GetSystemTimePreciseAsFileTime`

**Win7 替代**: `kernel32!GetSystemTimeAsFileTime` (签名相同)

### 错误 2 (待验证): api-ms-win-core-synch-l1-2-0.dll

PE 导入表包含:
- WaitOnAddress, WakeByAddressSingle, WakeByAddressAll

**来源**: Rust std 的 parking_lot_core / synchronization 依赖

### 错误 3 (待验证): combase.dll

PE 导入表包含:
- CoTaskMemFree, CoCreateFreeThreadedMarshaler, CoIncrementMTAUsage, CoTaskMemAlloc

**来源**: chrono → iana-time-zone → windows-core / windows-sys Win32_System_Com

---

## 三、Rust 1.75 严格路线验证结论

经过 `cargo metadata` 全依赖审计，当前项目有 **111 个包不兼容 Rust 1.75**，核心阻断:

```text
tauri 2.10.3      → MSRV 1.77.2
wry 0.54.4        → MSRV 1.77
zip 4.6.1         → MSRV 1.82
rmcp 1.5.0        → edition 2024
windows 0.62.2    → MSRV 1.82
time 0.3.47       → edition 2024
```

**结论**: 严格 Rust 1.75 需要大规模降级整个 Tauri 技术栈，风险极高，不推荐。

---

## 四、三方案对比

| 维度 | 方案A: Rust 1.75 | 方案B: Rust 1.77.2 | 方案C: Rust 1.96 + C stub |
|------|-------------|----------------|--------------------------|
| 依赖改动 | 100+ 个包降级 | 30+ 个包降级 | 0 个（无需降级） |
| Tauri 兼容 | 需降级 Tauri | ✅ 原生支持 | ✅ 原生支持 |
| 源码改动 | 大量 API 适配 | 少量适配 | 仅 build.rs |
| Win8+ API | 编译器消除 | 部分消除 | C stub + PE patch |
| 风险 | 极高 | 中 | 中 |
| 推荐 | ❌ | ⚠️ | ✅ (当前选择) |

---

## 五、方案C 修复策略

### 5.1 GetSystemTimePreciseAsFileTime

采用 PE 二进制补丁: 原地替换函数名为 GetSystemTimeAsFileTime

### 5.2 api-ms-win-core-synch-l1-2-0.dll

采用 DELAYLOAD 链接器选项 + C stub 兜底

### 5.3 combase.dll

采用 C stub 文件 + build.rs 编译链接

参考 deepseek-tui 项目的 win7_stubs.c 实现

---

## 六、后续编辑规范

1. Win7 构建使用 `pnpm build:win7`，脚本自动执行 PE patch + 导入扫描
2. 每次 Rust 依赖变更后必须扫描 EXE 导入表
3. 新增的 Win8+ API/DLL 导入必须在此文档记录
4. PE patch 只处理函数名替换，不用于 DLL 级伪装（避免引入不稳定导入表）
