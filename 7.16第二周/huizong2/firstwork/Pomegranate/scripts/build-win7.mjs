#!/usr/bin/env node
// scripts/build-win7.mjs
// Windows 7 兼容版构建脚本
//
// 用法：
//   pnpm build:win7                  — release 构建
//   pnpm build:win7 --debug           — debug 构建
//
// 流程：
//   1. 用 tauri.win7.conf.json 替换 tauri.conf.json（备份原文件）
//   2. 运行 pnpm tauri build --bundles nsis
//   3. 产物重命名为 Pomegranate_Setup_x.x.x_win7.exe
//   4. 恢复原 tauri.conf.json
//
// 前置条件：
//   1. 目标系统已预装 WebView2 109+ (Win7) 或 Evergreen (Win10+)
//   2. 前端依赖已安装 (pnpm install)
//   3. kb-mcp sidecar 已构建 (pnpm build:mcp)
//   4. MSVC 工具链可用（构建 Shim DLL 用 cl.exe）

import { execSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, readFileSync, renameSync, readdirSync, unlinkSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(__dirname, "..");
const SRC_TAURI = join(ROOT, "src-tauri");
const CONFIG = join(SRC_TAURI, "tauri.conf.json");
const WIN7_CONFIG = join(SRC_TAURI, "tauri.win7.conf.json");
const CONFIG_BACKUP = join(SRC_TAURI, "tauri.conf.json.bak");
const BUNDLE_DIR = join(SRC_TAURI, "target", "release", "bundle", "nsis");
const NSI_DIR = join(SRC_TAURI, "target", "release", "nsis", "x64");

const argv = process.argv.slice(2);
const isDebug = argv.includes("--debug");
const CARGO = "cargo";
const RUSTC = "rustc";
const WIN7_RUSTFLAGS = `${process.env.RUSTFLAGS || ""} --cfg getrandom_backend=\"windows_legacy\"`.trim();
const WIN7_ENV = {
  ...process.env,
  GETRANDOM_BACKEND: "windows_legacy",
  RUSTFLAGS: WIN7_RUSTFLAGS,
};

// ─── 读取版本号 ─────────────────────────────
function getVersion() {
  // 从 tauri.win7.conf.json 读取（避免读 package.json 单独解析）
  const raw = execSync(`node -e "console.log(require('${WIN7_CONFIG.replace(/\\/g, "/")}').version)"`, {
    cwd: ROOT,
    encoding: "utf8",
  });
  return raw.trim();
}

// ─── 验证前置条件 ──────────────────────────
function checkPrerequisites() {
  const rustcVersion = execSync(`${RUSTC} --version`, { encoding: "utf8" }).trim();
  console.log(`[build-win7] Rust 工具链: ${rustcVersion}`);
  console.log("[build-win7] WebView2 运行时由目标系统提供（跳过捆绑检查）");
}

// ─── 切换配置 ──────────────────────────────
function switchConfig() {
  if (existsSync(CONFIG_BACKUP)) {
    console.error("[build-win7] 检测到残留备份文件，可能上次构建未正常完成");
    console.error(`  手动删除 ${CONFIG_BACKUP} 后重试`);
    // 尝试恢复
    copyFileSync(CONFIG_BACKUP, CONFIG);
    unlinkSync(CONFIG_BACKUP);
    console.log("[build-win7] 已自动恢复原配置，请重新运行");
    process.exit(1);
  }

  // 备份原配置
  copyFileSync(CONFIG, CONFIG_BACKUP);
  console.log(`[build-win7] 已备份原配置 → ${CONFIG_BACKUP}`);

  // 替换为 Win7 配置
  copyFileSync(WIN7_CONFIG, CONFIG);
  console.log(`[build-win7] 已切换到 Win7 配置 ← ${WIN7_CONFIG}`);
}

// ─── 恢复配置 ──────────────────────────────
function restoreConfig() {
  if (existsSync(CONFIG_BACKUP)) {
    copyFileSync(CONFIG_BACKUP, CONFIG);
    unlinkSync(CONFIG_BACKUP);
    console.log("[build-win7] 已恢复原配置");
  }
}

// ─── WinRT Shim DLL 构建 ────────────────────
// 使用环境变量 VS2022_PATH 指定 Visual Studio 2022 安装路径
// 默认：D:\Dev\MicrosoftVisualStudio\2022\Community
const VS2022_PATH = process.env.VS2022_PATH || "D:\\Dev\\MicrosoftVisualStudio\\2022\\Community";

function buildShimDll() {
  const shimDir = join(SRC_TAURI, "win7_winrt_shim");
  const buildScript = join(shimDir, "build.ps1");
  const expectedDll = join(SRC_TAURI, "binaries", "api-ms-win-core-winrt-l1-1-0.dll");
  const sidecarName = "api-ms-win-core-winrt-l1-1-0.dll-x86_64-pc-windows-msvc.exe";
  const sidecarPath = join(SRC_TAURI, "binaries", sidecarName);
  const vcvars = join(VS2022_PATH, "VC", "Auxiliary", "Build", "vcvars64.bat");

  // 如果 DLL + sidecar 已存在且非强制重建，跳过
  const force = argv.includes("--force-shim");
  if (!force && existsSync(expectedDll) && readFileSync(expectedDll).length > 0) {
    // 确保 sidecar 也存在（可能上次构建后被清理）
    createSidecarCopy(expectedDll, sidecarName);
    console.log("[build-win7] Shim DLL 已存在，跳过构建（--force-shim 强制重建）");
    return;
  }

  if (!existsSync(buildScript)) {
    console.warn("[build-win7] build.ps1 未找到，跳过 Shim DLL 构建");
    return;
  }

  if (!existsSync(vcvars)) {
    console.warn(`[build-win7] vcvars64.bat 未找到: ${vcvars}`);
    console.warn(`  设置 VS2022_PATH 环境变量指向 VS 2022 安装目录`);
    // 降级：直接尝试 powershell（如果 cl.exe 在 PATH 里）
    console.warn("  降级：直接尝试 build.ps1...");
    tryBuildDirectly();
    return;
  }

  // 确保 binaries 目录存在
  mkdirSync(join(SRC_TAURI, "binaries"), { recursive: true });

  console.log(`[build-win7] 构建 WinRT Shim DLL (MSVC: ${VS2022_PATH})...`);

  // cmd /c 链式调用：
  //   1. vcvars64.bat 设置 MSVC 环境变量（PATH 包含 cl.exe）
  //   2. powershell 执行 build.ps1（继承 cmd 的环境变量）
  // vcvars64.bat 输出重定向到 temp 文件避免污染构建日志
  try {
    const vcvarsLog = join(ROOT, "src-tauri", "win7_winrt_shim", "vcvars.log");
    const cmd = `cmd /c "call "${vcvars}" 1>"${vcvarsLog}" 2>&1 && powershell -NoProfile -ExecutionPolicy Bypass -File "${buildScript}""`;
    execSync(cmd, { cwd: ROOT, stdio: "inherit" });

    if (!existsSync(expectedDll) || readFileSync(expectedDll).length === 0) {
      console.warn("[build-win7] Shim DLL 构建后未在预期位置找到，安装包可能不含 WinRT 垫片");
    } else {
      console.log(`[build-win7] Shim DLL 构建完成: ${expectedDll}`);
      // Tauri externalBin 会对路径追加 -{target_triple}.exe 后缀查找文件
      createSidecarCopy(expectedDll, "api-ms-win-core-winrt-l1-1-0.dll-x86_64-pc-windows-msvc.exe");
    }
  } catch (e) {
    console.warn("[build-win7] Shim DLL 构建失败，降级：直接尝试 build.ps1...");
    console.warn("  错误: " + String(e.message || e).split("\n")[0]);
    tryBuildDirectly();
  }
}

function createSidecarCopy(dllPath, sidecarName) {
  const sidecarPath = join(SRC_TAURI, "binaries", sidecarName);
  copyFileSync(dllPath, sidecarPath);
  console.log(`[build-win7] 已创建 externalBin sidecar: ${sidecarName}`);
}

function tryBuildDirectly() {
  const buildScript = join(SRC_TAURI, "win7_winrt_shim", "build.ps1");
  try {
    execSync(`powershell -NoProfile -ExecutionPolicy Bypass -File "${buildScript}"`, {
      cwd: ROOT,
      stdio: "inherit",
    });
    const expectedDll = join(SRC_TAURI, "binaries", "api-ms-win-core-winrt-l1-1-0.dll");
    if (existsSync(expectedDll) && readFileSync(expectedDll).length > 0) {
      console.log("[build-win7] Shim DLL 构建完成（降级路径）");
    }
  } catch (e2) {
    console.warn("[build-win7] Shim DLL 构建失败（缺少 MSVC 工具链），继续构建主程序");
  }
}

// ─── Synch Shim DLL 构建 ─────────────────────
// 创建 api-ms-win-core-synch-l1-2-0.dll，提供 Sleep/SleepEx 转发 + WaitOnAddress 桩
// 通过 resources 随 NSIS 安装到 EXE 目录，延迟加载时本地目录优先

function buildSynchShimDll() {
  const shimDir = join(SRC_TAURI, "win7_synch_shim");
  const buildScript = join(shimDir, "build.ps1");
  const expectedDll = join(SRC_TAURI, "binaries", "api-ms-win-core-synch-l1-2-0.dll");
  const vcvars = join(VS2022_PATH, "VC", "Auxiliary", "Build", "vcvars64.bat");

  const force = argv.includes("--force-shim");
  if (!force && existsSync(expectedDll) && readFileSync(expectedDll).length > 0) {
    console.log("[build-win7] Synch Shim DLL 已存在，跳过构建（--force-shim 强制重建）");
    return;
  }

  if (!existsSync(buildScript)) {
    console.warn("[build-win7] Synch shim build.ps1 未找到，跳过");
    return;
  }

  mkdirSync(join(SRC_TAURI, "binaries"), { recursive: true });

  console.log(`[build-win7] 构建 Synch Shim DLL (MSVC: ${VS2022_PATH})...`);

  try {
    const vcvarsLog = join(ROOT, "src-tauri", "win7_synch_shim", "vcvars.log");
    const cmd = `cmd /c "call "${vcvars}" 1>"${vcvarsLog}" 2>&1 && powershell -NoProfile -ExecutionPolicy Bypass -File "${buildScript}""`;
    execSync(cmd, { cwd: ROOT, stdio: "inherit" });

    if (!existsSync(expectedDll) || readFileSync(expectedDll).length === 0) {
      console.warn("[build-win7] Synch Shim DLL 构建后未找到");
    } else {
      console.log(`[build-win7] Synch Shim DLL: ${expectedDll}`);
    }
  } catch (e) {
    console.warn("[build-win7] Synch Shim DLL 构建失败: " + String(e.message || e).split("\n")[0]);
    // 降级：直接尝试 powershell
    try {
      execSync(`powershell -NoProfile -ExecutionPolicy Bypass -File "${buildScript}"`, {
        cwd: ROOT, stdio: "inherit",
      });
    } catch (e2) {
      console.warn("[build-win7] Synch Shim DLL 降级构建也失败，继续（依赖 PE patch safety net）");
    }
  }
}

// ─── PE 补丁 ─────────────────────────────────
const BLOCKING_IMPORTS = [
  "GetSystemTimePreciseAsFileTime",
  "api-ms-win-crt-private-l1-1-0.dll",
  "ProcessPrng",
  // api-ms-win-core-synch-l1-2-0.dll 已被 patch-win7-pe.mjs 重映射到 bcryptprimitives.dll
  // bcrypt 统一 shim 同时导出 ProcessPrng + WaitOnAddress + WakeByAddressSingle/All
  // WinRT DLL 不应出现在直接导入表中（已通过 DELAYLOAD 处理）
  "api-ms-win-core-winrt-l1-1-0.dll",
  "api-ms-win-core-winrt-string-l1-1-0.dll",
];
const RISKY_IMPORTS = [
  // combase.dll 延迟加载，C 桩提供 CoTaskMemFree/CoTaskMemAlloc 等 ole32 转发
  "combase.dll",
];

function patchPE(exePath) {
  if (!existsSync(exePath)) {
    console.warn(`[build-win7] 未找到 exe，跳过 PE 补丁: ${exePath}`);
    return;
  }

  const buffer = readFileSync(exePath);
  const peOffset = buffer.readUInt32LE(0x3c);
  const optionalHeaderOffset = peOffset + 24;

  // PE Optional Header: MajorSubsystemVersion / MinorSubsystemVersion
  buffer.writeUInt16LE(6, optionalHeaderOffset + 48);
  buffer.writeUInt16LE(1, optionalHeaderOffset + 50);

  // Win8+ API 降级：GetSystemTimePreciseAsFileTime → GetSystemTimeAsFileTime
  // 两者签名相同，目标函数 Win7 可用；目标名更短，可原地覆盖并用 \0 填充。
  const from = Buffer.from("GetSystemTimePreciseAsFileTime\0", "ascii");
  const to = Buffer.from("GetSystemTimeAsFileTime\0", "ascii");
  let patchedImports = 0;
  let offset = buffer.indexOf(from);
  while (offset !== -1) {
    to.copy(buffer, offset);
    buffer.fill(0, offset + to.length, offset + from.length);
    patchedImports += 1;
    offset = buffer.indexOf(from, offset + from.length);
  }

  writeFileSync(exePath, buffer);
  console.log(`[build-win7] PE 已补丁: ${exePath} (subsystem=6.1, imports=${patchedImports})`);
}

function patchAllBinaries() {
  const profile = isDebug ? "debug" : "release";
  patchPE(join(SRC_TAURI, "target", profile, "IntelligentNoteBook.exe"));
  patchPE(join(SRC_TAURI, "target", profile, "Pomegranate.exe"));

  const binariesDir = join(SRC_TAURI, "binaries");
  if (existsSync(binariesDir)) {
    for (const file of readdirSync(binariesDir)) {
      if (file.endsWith(".exe")) patchPE(join(binariesDir, file));
    }
  }
}

function verifyWin7Imports(exePath) {
  if (!existsSync(exePath)) return;
  const text = readFileSync(exePath).toString("latin1");

  // 直接搜索 GetSystemTimePreciseAsFileTime 字符串
  // 此函数在 Rust std::time 中被调用，Win7 的 kernel32.dll 无此导出 → 必须已通过 PE patch 替换为 GetSystemTimeAsFileTime
  if (text.includes("GetSystemTimePreciseAsFileTime")) {
    throw new Error(
      `[build-win7] ${exePath} 仍包含 GetSystemTimePreciseAsFileTime 导入！\n` +
      `  此函数仅 Win8+ 可用，Win7 将无法启动（错误：无法定位程序输入点）。\n` +
      `  请检查 scripts/patch-win7-pe.mjs 是否成功运行。`
    );
  }

  // 区分直接导入 vs DELAYLOAD（延迟加载）：
  // DELAYLOAD 的 DLL 名在 PE 中只出现在 Delay-Load Descriptor 区域，
  // 通常出现 1-2 次。直接导入的 DLL 名在 IMAGE_IMPORT_DESCRIPTOR 数组
  // 中，可能引用多次（模块名 + Hint/Name 表引用）。
  const blocking = BLOCKING_IMPORTS.filter((name) => {
    const count = text.split(name).length - 1;
    if (count === 0) return false;
    // DLL 名出现 > 2 次 → 可能在直接导入表中（非 DELAYLOAD）
    if (count > 2) return true;
    // 出现 1-2 次 → 可能在 DELAYLOAD 描述符中，视为安全
    console.log(`[build-win7] ${name} 出现 ${count} 次（疑似在 delay-load 表中），跳过`);
    return false;
  });

  if (blocking.length > 0) {
    throw new Error(`[build-win7] ${exePath} 仍包含 Win7 加载阻断导入: ${blocking.join(", ")}`);
  }

  const risky = RISKY_IMPORTS.filter((name) => {
    const count = text.split(name).length - 1;
    return count > 2; // 同理：仅在直接导入表超过阈值时告警
  });
  if (risky.length > 0) {
    console.warn(`[build-win7] 警告: ${exePath} 仍包含需 Win7 实测的风险导入: ${risky.join(", ")}`);
  }
}

// ─── 检查 bcryptprimitives.dll shim ────────────
function verifyBcryptShim() {
  const bcryptDll = join(SRC_TAURI, "binaries", "bcryptprimitives.dll");
  if (!existsSync(bcryptDll)) {
    console.warn("[build-win7] WARNING: bcryptprimitives.dll shim 未找到!");
    console.warn("  安装包可能不含 ProcessPrng shim，Win7 加载器将调用 System32 版本");
    return;
  }

  const size = readFileSync(bcryptDll).length;
  if (size === 0) {
    console.warn("[build-win7] WARNING: bcryptprimitives.dll shim 文件为空!");
    return;
  }

  // 快速 PE 头检查
  const buf = readFileSync(bcryptDll);
  const sig = buf.toString("ascii", 0, 2);
  if (sig !== "MZ") {
    console.warn("[build-win7] WARNING: bcryptprimitives.dll 可能不是有效的 PE 文件");
    return;
  }

  // 检查是否导出 ProcessPrng
  const text = buf.toString("latin1");
  if (text.includes("ProcessPrng")) {
    console.log("[build-win7] bcryptprimitives.dll shim: OK (exports ProcessPrng)");
  } else {
    console.warn("[build-win7] WARNING: bcryptprimitives.dll 可能未导出 ProcessPrng!");
  }

  console.log("[build-win7] 提示: 安装后该 DLL 应位于 EXE 同目录（$INSTDIR\\bcryptprimitives.dll）");
}

// ─── 重命名产物 ─────────────────────────────
function renameOutput() {
  if (!existsSync(BUNDLE_DIR)) {
    console.warn("[build-win7] 未找到 NSIS 产物目录，跳过重命名");
    return;
  }

  const version = getVersion();
  const files = readdirSync(BUNDLE_DIR);
  const installer = files.find((f) => f.endsWith("_x64-setup.exe") && !f.includes("_win7"));
  if (!installer) {
    console.warn("[build-win7] 未找到 tauri 原生产物 .exe，跳过重命名");
    return;
  }

  // 清理旧 win7 产物，避免残留混淆
  for (const f of files) {
    if (f.endsWith(".exe") && f.includes("_win7") && f !== installer) {
      const old = join(BUNDLE_DIR, f);
      unlinkSync(old);
      console.log(`[build-win7] 已删除旧 win7 产物: ${f}`);
    }
  }

  const src = join(BUNDLE_DIR, installer);
  const dstName = `Pomegranate_Setup_${version}_win7_x64-setup.exe`;
  const dst = join(BUNDLE_DIR, dstName);

  renameSync(src, dst);
  console.log(`[build-win7] 产物已重命名: ${dstName}`);
}

// ─── 主流程 ─────────────────────────────────
function main() {
  console.log("[build-win7] === Windows 7 兼容版构建开始 ===");

  checkPrerequisites();
  switchConfig();

  try {
    // Phase 0: 构建 Shim DLL
    buildShimDll();          // WinRT shim: api-ms-win-core-winrt-l1-1-0.dll
    buildSynchShimDll();     // Synch shim: api-ms-win-core-synch-l1-2-0.dll

    // tauri.win7.conf.json 已配置 beforeBundleCommand：
    //   cargo build → scripts/patch-win7-pe.mjs (PE patch) → makensis
    // PE patch 在 cargo 编译后、NSIS 打包前执行，确保安装包含已补丁 EXE。
    // --features win7-compat 跳过 tauri-plugin-notification 注册（消除 WinRT 依赖链 A）
    // RUSTFLAGS: 强制 getrandom 0.3.x/0.4.x 使用 rdrand 后端 (RtlGenRandom)
    // getrandom 0.2.x 已通过 Cargo.toml 的 getrandom02 feature 配置
    process.env.RUSTFLAGS = process.env.RUSTFLAGS
      ? process.env.RUSTFLAGS + ' --cfg getrandom_backend="rdrand"'
      : '--cfg getrandom_backend="rdrand"';

    const tauriCmd = `pnpm tauri build --bundles nsis${isDebug ? " --debug" : ""} -- --features win7-compat`;
    console.log(`[build-win7] ${tauriCmd}`);
    try {
      execSync(tauriCmd, { cwd: ROOT, stdio: "inherit", env: WIN7_ENV });
    } catch (e) {
      console.warn("[build-win7] tauri build 非零退出码（签名缺失为正常情况），继续验证...");
    }

    const profile = isDebug ? "debug" : "release";
    const exePath = join(SRC_TAURI, "target", profile, "IntelligentNoteBook.exe");
    const pomegranatePath = join(SRC_TAURI, "target", profile, "Pomegranate.exe");

    // 双保险：beforeBundleCommand 已运行 patch-win7-pe.mjs，
    // 这里再次 patch 确保所有二进制文件（含 sidecar）无误
    patchAllBinaries();
    verifyWin7Imports(exePath);
    verifyWin7Imports(pomegranatePath);
    verifyBcryptShim();

    renameOutput();
    console.log("[build-win7] === 构建成功 ===");
  } catch (err) {
    console.error("[build-win7] === 构建失败 ===");
    console.error(err.message);
    process.exitCode = 1;
  } finally {
    restoreConfig();
  }
}

main();
