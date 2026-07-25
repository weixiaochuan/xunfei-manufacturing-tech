#!/usr/bin/env node
// scripts/patch-win7-pe.mjs
// Tauri beforeBundleCommand: 在 cargo build 后、makensis 前执行
//
// 对主 EXE 和 sidecar 执行 PE 补丁：
// 1. PE 子系统版本锁定为 6.1 (Win7)
// 2. GetSystemTimePreciseAsFileTime → GetSystemTimeAsFileTime
// 3. delay-load DLL 名重映射：
//    api-ms-win-core-synch-l1-2-0.dll → bcryptprimitives.dll
//    (bcrypt 统一 shim 同时提供 ProcessPrng + WaitOnAddress + WakeByAddress*)
//    api-ms-win-core-winrt-string-l1-1-0.dll → api-ms-win-core-winrt-l1-1-0.dll
//    (Win7 上不存在 winrt-string API Set，重映射到 WinRT shim DLL)

import { existsSync, readFileSync, writeFileSync } from "node:fs";

// ─── 补丁目标列表 ────────────────────────────────
const TARGETS = [
  "src-tauri/target/release/IntelligentNoteBook.exe",
  "src-tauri/target/release/Pomegranate.exe",
  "src-tauri/binaries/kb-mcp-x86_64-pc-windows-msvc.exe",
];

// ─── 对单个 PE 文件执行所有补丁 ──────────────────
function patchOne(path) {
  if (!existsSync(path)) {
    console.warn(`[patch-win7-pe] SKIP (not found): ${path}`);
    return null;
  }

  const buffer = readFileSync(path);
  const peOffset = buffer.readUInt32LE(0x3c);
  const opt = peOffset + 24;

  // Patch 1: 子系统类型 → GUI (2)，消除 Console (3) 产生的 DOS 窗口
  // PE Optional Header offset 0x44(68): Subsystem field
  const curSubsystem = buffer.readUInt16LE(opt + 68);
  if (curSubsystem === 3) {
    buffer.writeUInt16LE(2, opt + 68);
  }

  // Patch 2: 子系统版本 → 6.1
  buffer.writeUInt16LE(6, opt + 48);
  buffer.writeUInt16LE(1, opt + 50);

  // Patch 2: GetSystemTimePreciseAsFileTime → GetSystemTimeAsFileTime
  const fromGST = Buffer.from("GetSystemTimePreciseAsFileTime\0", "ascii");
  const toGST = Buffer.from("GetSystemTimeAsFileTime\0", "ascii");
  let gstPatched = 0, off = buffer.indexOf(fromGST);
  while (off !== -1) {
    toGST.copy(buffer, off);
    buffer.fill(0, off + toGST.length, off + fromGST.length);
    gstPatched++;
    off = buffer.indexOf(fromGST, off + fromGST.length);
  }

  // Patch 3: delay-load DLL 名重映射
  // api-ms-win-core-synch-l1-2-0.dll → bcryptprimitives.dll (39→20 chars)
  // 原因: Win7 的 kernel32.dll 不导出 WaitOnAddress（Win8+ API），
  //       必须重映射到 bcryptprimitives.dll（统一 shim 同时提供
  //       ProcessPrng + WaitOnAddress + WakeByAddressSingle/All）
  const synchFrom = Buffer.from("api-ms-win-core-synch-l1-2-0.dll\0", "ascii");
  const synchTo   = Buffer.from("bcryptprimitives.dll\0", "ascii");
  let synchPatched = 0;
  let off2 = buffer.indexOf(synchFrom);
  while (off2 !== -1) {
    synchTo.copy(buffer, off2);
    buffer.fill(0, off2 + synchTo.length, off2 + synchFrom.length);
    synchPatched++;
    off2 = buffer.indexOf(synchFrom, off2 + synchFrom.length);
  }

  // Patch 4: api-ms-win-core-winrt-string-l1-1-0.dll → api-ms-win-core-winrt-l1-1-0.dll
  // Win7 不存在 winrt-string API Set，重映射到已有的 WinRT shim DLL (39→32 chars)
  const wrtStrFrom = Buffer.from("api-ms-win-core-winrt-string-l1-1-0.dll\0", "ascii");
  const wrtStrTo   = Buffer.from("api-ms-win-core-winrt-l1-1-0.dll\0", "ascii");
  let wrtStrPatched = 0;
  let off3 = buffer.indexOf(wrtStrFrom);
  while (off3 !== -1) {
    wrtStrTo.copy(buffer, off3);
    buffer.fill(0, off3 + wrtStrTo.length, off3 + wrtStrFrom.length);
    wrtStrPatched++;
    off3 = buffer.indexOf(wrtStrFrom, off3 + wrtStrFrom.length);
  }

  writeFileSync(path, buffer);

  // 验证：确认 GetSystemTimePreciseAsFileTime 已不在二进制中
  const verify = readFileSync(path);
  const leftover = verify.indexOf(fromGST);
  if (leftover !== -1) {
    console.error(`[patch-win7-pe] ERROR: ${path} 仍残留 GetSystemTimePreciseAsFileTime 引用! 可能导致 Win7 无法启动`);
  }

  return { path, gst: gstPatched, synch: synchPatched, wrtStr: wrtStrPatched, gui: curSubsystem === 3 };
}

// ─── 主流程 ──────────────────────────────────────
let ok = 0;
for (const path of TARGETS) {
  const r = patchOne(path);
  if (r) {
    console.log(`[patch-win7-pe] ${path} (gui=${r.gui ? 'YES' : 'no'}, subsys=6.1, GST=${r.gst}, synch→bcrypt=${r.synch}, wrtStr→winrt=${r.wrtStr})`);
    ok++;
  }
}
if (ok === 0) {
  console.warn("[patch-win7-pe] WARNING: no targets found");
}
