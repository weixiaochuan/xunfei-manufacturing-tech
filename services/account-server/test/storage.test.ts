import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";
import { resolveFileStorageRoot } from "../src/config.js";
import { LocalFilesystemStorage } from "../src/storage/local-filesystem-storage.js";
import { migrateFileStorage, writeStorageManifest } from "../src/storage/migration.js";

const sha256 = (value: Buffer | string) => createHash("sha256").update(value).digest("hex");

test("absolute Windows and Linux storage roots are accepted outside source trees", () => {
  assert.equal(
    resolveFileStorageRoot("D:\\PomegranateServer\\data\\user-files", { platform: "win32", repositoryRoot: "D:\\repo", systemRoot: "C:\\Windows" }),
    "D:\\PomegranateServer\\data\\user-files",
  );
  assert.equal(
    resolveFileStorageRoot("/srv/pomegranate/user-files", { platform: "linux", repositoryRoot: "/workspace/repo" }),
    "/srv/pomegranate/user-files",
  );
});

test("relative, source, dependency, and Windows system roots are rejected", () => {
  const context = { platform: "win32" as const, repositoryRoot: "D:\\repo", systemRoot: "C:\\Windows" };
  assert.throws(() => resolveFileStorageRoot("relative", context));
  assert.throws(() => resolveFileStorageRoot("D:\\repo\\services\\account-server", context));
  assert.throws(() => resolveFileStorageRoot("D:\\elsewhere\\node_modules\\files", context));
  assert.throws(() => resolveFileStorageRoot("C:\\Windows\\Temp\\files", context));
});

test("the exact legacy root is allowed only through the explicit development rollback option", () => {
  const context = { platform: "win32" as const, repositoryRoot: "D:\\repo", systemRoot: "C:\\Windows" };
  const legacyRoot = "D:\\repo\\services\\account-server\\.data\\user-files";
  assert.throws(() => resolveFileStorageRoot(legacyRoot, context));
  assert.equal(
    resolveFileStorageRoot(legacyRoot, context, { legacyRollbackRoot: legacyRoot }),
    legacyRoot,
  );
});

test("filesystem storage safely creates a missing root", async (t) => {
  const parent = await mkdtemp(join(tmpdir(), "pomegranate-storage-root-"));
  t.after(() => rm(parent, { recursive: true, force: true }));
  const root = join(parent, "missing", "user-files");
  const storage = new LocalFilesystemStorage(root);
  await storage.initialize();
  assert.deepEqual(await readdir(root), []);
});

test("filesystem storage writes atomically, reads, verifies, exists, and deletes", async (t) => {
  const root = await mkdtemp(join(tmpdir(), "pomegranate-storage-"));
  t.after(() => rm(root, { recursive: true, force: true }));
  const storage = new LocalFilesystemStorage(root);
  await storage.initialize();
  const content = Buffer.from("safe-storage-test");
  const stored = await storage.writeFile([content], 1_024);
  assert.match(stored.storageKey, /^[a-f0-9]{64}$/);
  assert.equal(stored.sha256, sha256(content));
  assert.equal(await storage.fileExists(stored.storageKey), true);
  assert.deepEqual(await storage.verifyFile(stored.storageKey, content.length, sha256(content)), {
    exists: true, sizeMatches: true, sha256Matches: true, actualSizeBytes: content.length, actualSha256: sha256(content),
  });
  const stream = await storage.createReadStream(stored.storageKey);
  const chunks: Buffer[] = [];
  for await (const chunk of stream) chunks.push(Buffer.from(chunk));
  assert.deepEqual(Buffer.concat(chunks), content);
  assert.deepEqual(await readdir(root), [stored.storageKey]);
  assert.equal(await storage.deleteFile(stored.storageKey), true);
  assert.equal(await storage.deleteFile(stored.storageKey), false);
});

test("storage keys reject traversal and absolute paths", async (t) => {
  const root = await mkdtemp(join(tmpdir(), "pomegranate-storage-key-"));
  t.after(() => rm(root, { recursive: true, force: true }));
  const storage = new LocalFilesystemStorage(root);
  await storage.initialize();
  for (const value of ["../secret", "C:\\secret", "/secret", "a".repeat(63), "A".repeat(64)]) {
    assert.throws(() => storage.resolveStorageKey(value));
  }
});

test("migration copies, verifies, skips identical retries, and never exposes private fields", async (t) => {
  const parent = await mkdtemp(join(tmpdir(), "pomegranate-migration-"));
  t.after(() => rm(parent, { recursive: true, force: true }));
  const sourceRoot = join(parent, "source");
  const targetRoot = join(parent, "target");
  const source = new LocalFilesystemStorage(sourceRoot);
  await source.initialize();
  const key = "a".repeat(64);
  const content = Buffer.from("migration-content");
  await writeFile(source.resolveStorageKey(key), content);
  const records = [{ storageKey: key, sizeBytes: content.length, sha256: sha256(content), deleted: false }];
  const first = await migrateFileStorage({ sourceRoot, targetRoot, records, mode: "copy" });
  assert.equal(first.copied, 1);
  assert.equal(first.failed, 0);
  const second = await migrateFileStorage({ sourceRoot, targetRoot, records, mode: "copy" });
  assert.equal(second.skipped, 1);
  const verified = await migrateFileStorage({ sourceRoot, targetRoot, records, mode: "verify" });
  assert.equal(verified.verified, 1);
  const manifestPath = join(parent, "manifests", "result.json");
  await writeStorageManifest(manifestPath, verified);
  const serialized = await readFile(manifestPath, "utf8");
  assert.doesNotMatch(serialized, /owner_user_id|username|email|session|password|client_secret/i);
});

test("migration detects a target hash conflict and does not overwrite it", async (t) => {
  const parent = await mkdtemp(join(tmpdir(), "pomegranate-conflict-"));
  t.after(() => rm(parent, { recursive: true, force: true }));
  const sourceRoot = join(parent, "source");
  const targetRoot = join(parent, "target");
  const source = new LocalFilesystemStorage(sourceRoot);
  const target = new LocalFilesystemStorage(targetRoot);
  await source.initialize(); await target.initialize();
  const key = "b".repeat(64);
  const expected = Buffer.from("expected");
  const conflict = Buffer.from("conflict");
  await writeFile(source.resolveStorageKey(key), expected);
  await writeFile(target.resolveStorageKey(key), conflict);
  const result = await migrateFileStorage({ sourceRoot, targetRoot, records: [{ storageKey: key, sizeBytes: expected.length, sha256: sha256(expected), deleted: false }], mode: "copy" });
  assert.equal(result.failed, 1);
  assert.deepEqual(await readFile(target.resolveStorageKey(key)), conflict);
});
