import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

import { resolveDocumentSource } from "./documentSourcePolicy.ts";
import { documentErrorMessage } from "./documentError.ts";
import {
  assertCurrentDocumentRequest,
  captureDocumentRequest,
  changeDocumentAccount,
} from "./documentSession.ts";

test("account is the default and local requires an exact explicit opt-in", () => {
  assert.equal(resolveDocumentSource(undefined), "account");
  assert.equal(resolveDocumentSource(""), "account");
  assert.equal(resolveDocumentSource("LOCAL"), "account");
  assert.equal(resolveDocumentSource("local"), "local");
});

test("an account switch invalidates in-flight document requests", () => {
  changeDocumentAccount("platform-user-a");
  const request = captureDocumentRequest();
  changeDocumentAccount("platform-user-b");
  assert.throws(() => assertCurrentDocumentRequest(request), (error) => {
    assert.equal(error.code, "staleRequest");
    return true;
  });
});

test("restoring the same account does not invalidate a request", () => {
  changeDocumentAccount("platform-user-c");
  const request = captureDocumentRequest();
  changeDocumentAccount("platform-user-c");
  assert.doesNotThrow(() => assertCurrentDocumentRequest(request));
});

test("the React account documents bridge exposes no token or authorization surface", async () => {
  const source = await readFile(new URL("./accountDocumentsApi.ts", import.meta.url), "utf8");
  for (const forbidden of ["sessionToken", "accessToken", "refreshToken", "idToken", "Authorization"]) {
    assert.equal(source.includes(forbidden), false, `${forbidden} must remain Rust-only`);
  }
});

test("structured document errors always become safe Chinese text", () => {
  assert.equal(documentErrorMessage({ code: "signedOut", message: "ignored" }), "请先登录");
  assert.equal(documentErrorMessage({ error: "document_conflict" }), "文档已在其他位置更新");
  assert.equal(documentErrorMessage({ code: "notFound" }), "文档不存在或无权访问");
  assert.equal(documentErrorMessage({ message: "Bearer secret-token" }), "操作失败，请稍后重试");
  assert.notEqual(documentErrorMessage({ unexpected: true }), "[object Object]");
  assert.equal(documentErrorMessage({ code: "titleInvalid" }), "文档标题缺失");
  assert.equal(documentErrorMessage({ code: "markdownContentInvalid" }), "文档正文格式无效");
  assert.equal(documentErrorMessage({ code: "folderInvalid" }), "文件夹信息无效");
  assert.equal(documentErrorMessage({ code: "tagsInvalid" }), "标签信息无效");
  assert.equal(documentErrorMessage({ code: "requestShapeInvalid" }), "新建参数格式错误");
});

test("PPT internal materials use the unified repository, not SQLite note APIs", async () => {
  const source = await readFile(new URL("../../pages/ppt-generation/index.tsx", import.meta.url), "utf8");
  assert.match(source, /from "@\/lib\/documents\/repository"/);
  assert.doesNotMatch(source, /import \{[^}]*\b(?:noteApi|folderApi)\b[^}]*\} from "@\/lib\/api"/s);
  assert.match(source, /prepareAccountUploadedMaterial/);
  assert.match(source, /subscribeDocumentAccountReset/);
});

test("account document creation is guarded against duplicate clicks and stale accounts", async () => {
  const creator = await readFile(new URL("../noteCreator.tsx", import.meta.url), "utf8");
  const repository = await readFile(new URL("./repository.ts", import.meta.url), "utf8");
  assert.match(creator, /accountCreateInFlight/);
  assert.match(repository, /guarded\(\(\) =>\s*accountDocumentsApi\.createMarkdown/s);
  assert.match(repository, /mapAccountDocument\(document\)/);
});

test("the unified document create menu exposes the three distinct account entries", async () => {
  const button = await readFile(new URL("../../components/NewNoteButton.tsx", import.meta.url), "utf8");
  for (const label of ["新建 Markdown", "上传文件", "导入为可编辑 Markdown"]) {
    assert.match(button, new RegExp(label));
  }
  assert.match(button, /uploadAccountDocument/);
  assert.match(button, /importEditableMarkdownFile/);
  assert.match(button, /原样保存到独立文件存储/);
  assert.match(button, /转换为内部 Markdown/);
});

test("the legacy account files route still redirects to the unified documents page", async () => {
  const router = await readFile(new URL("../../Router.tsx", import.meta.url), "utf8");
  assert.match(router, /path:\s*["']account\/files["'][\s\S]*?<Navigate to=["']\/notes["'] replace \/>/);
});

test("upload and editable Markdown import both reject stale account results", async () => {
  const creator = await readFile(new URL("../noteCreator.tsx", import.meta.url), "utf8");
  const repository = await readFile(new URL("./repository.ts", import.meta.url), "utf8");
  assert.match(creator, /const identity = captureDocumentRequest\(\)/);
  assert.match(creator, /assertCurrentDocumentRequest\(identity\)/);
  assert.match(repository, /guarded\(\(\) => accountDocumentsApi\.importEditableMarkdown\(\)\)/);
});

test("React account document models expose neither local paths nor private storage metadata", async () => {
  const api = await readFile(new URL("./accountDocumentsApi.ts", import.meta.url), "utf8");
  for (const forbidden of ["ownerUserId", "owner_user_id", "storageKey", "storage_key", "absolutePath", "localPath"]) {
    assert.equal(api.includes(forbidden), false, `${forbidden} must stay behind Rust`);
  }
  assert.match(api, /account_import_markdown_file/);
});

test("file upload and Markdown import failures use specific safe Chinese messages", () => {
  assert.equal(documentErrorMessage({ code: "fileTypeRejected" }), "不支持上传此文件类型");
  assert.equal(documentErrorMessage({ code: "tooLarge" }), "文件超过允许大小");
  assert.equal(documentErrorMessage({ code: "markdownEncodingUnsupported" }), "暂不支持该文件编码");
  assert.equal(documentErrorMessage({ code: "markdownTooLarge" }), "Markdown 文件超过允许大小");
  assert.equal(documentErrorMessage({ code: "markdownReadFailed" }), "无法读取所选文件");
});

test("account uploaded PPT material stays behind the Rust cache bridge", async () => {
  const api = await readFile(new URL("./accountDocumentsApi.ts", import.meta.url), "utf8");
  assert.match(api, /account_prepare_uploaded_document_material/);
  for (const forbidden of ["ownerUserId", "owner_user_id", "storageKey", "storage_key", "serverPath"]) {
    assert.equal(api.includes(forbidden), false, `${forbidden} must not enter the React model`);
  }
});

test("AI document handoff and archive do not persist account document ids in SQLite", async () => {
  const attach = await readFile(new URL("../aiAttach.ts", import.meta.url), "utf8");
  const page = await readFile(new URL("../../pages/ai/index.tsx", import.meta.url), "utf8");
  assert.match(attach, /isAccountDocumentSource/);
  assert.match(attach, /accountDocumentIds/);
  assert.match(page, /accountDocumentIds/);
  assert.match(page, /isAccountDocumentSource\s*\?\s*await noteApi\.create/s);
});

test("mobile account summary uses the unified trash and note repositories", async () => {
  const source = await readFile(new URL("../../pages/settings/MobileMe.tsx", import.meta.url), "utf8");
  assert.match(source, /from "@\/lib\/documents\/repository"/);
  assert.doesNotMatch(source, /\btrashApi,\s*\n\s*aiModelApi/);
});
