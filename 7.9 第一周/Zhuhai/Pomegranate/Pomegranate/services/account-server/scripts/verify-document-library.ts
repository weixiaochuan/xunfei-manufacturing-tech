import { closeDatabasePool, createDatabasePool } from "../src/db.js";
import { loadConfig } from "../src/config.js";

interface SummaryRow {
  account_number: string;
  total: number;
  active: number;
  deleted: number;
  hidden: number;
  diary: number;
  pinned: number;
  folders: number;
  tags: number;
  links: number;
  invalid_hashes: number;
  content_hash_mismatches: number;
  legacy_hash_mismatches: number;
  word_count_mismatches: number;
  non_initial_revisions: number;
}

async function verify(): Promise<void> {
  const config = loadConfig();
  const pool = createDatabasePool(config.database);
  try {
    const result = await pool.query<SummaryRow>(`
      SELECT
        pu.account_number,
        count(d.id)::int AS total,
        count(d.id) FILTER (WHERE d.deleted_at IS NULL)::int AS active,
        count(d.id) FILTER (WHERE d.deleted_at IS NOT NULL)::int AS deleted,
        count(d.id) FILTER (WHERE d.is_hidden)::int AS hidden,
        count(d.id) FILTER (WHERE d.diary_date IS NOT NULL)::int AS diary,
        count(d.id) FILTER (WHERE d.is_pinned)::int AS pinned,
        (SELECT count(*)::int FROM document_folders f WHERE f.owner_user_id = pu.id AND f.deleted_at IS NULL) AS folders,
        (SELECT count(*)::int FROM document_tags t WHERE t.owner_user_id = pu.id AND t.deleted_at IS NULL) AS tags,
        (SELECT count(*)::int FROM document_tag_links link JOIN documents linked ON linked.id = link.document_id WHERE linked.owner_user_id = pu.id) AS links,
        count(d.id) FILTER (WHERE d.document_kind = 'markdown' AND d.content_sha256 !~ '^[0-9a-f]{64}$')::int AS invalid_hashes,
        count(d.id) FILTER (WHERE d.document_kind = 'markdown' AND d.content_sha256 <> encode(digest(d.markdown_content, 'sha256'), 'hex'))::int AS content_hash_mismatches,
        count(d.id) FILTER (
          WHERE d.source_local_document_id IS NOT NULL
            AND d.legacy_metadata->>'legacyContentHash' ~ '^[0-9a-f]{64}$'
            AND d.content_sha256 <> lower(d.legacy_metadata->>'legacyContentHash')
        )::int AS legacy_hash_mismatches,
        count(d.id) FILTER (
          WHERE d.source_local_document_id IS NOT NULL
            AND d.legacy_metadata->>'wordCount' ~ '^\\d+$'
            AND d.word_count <> (d.legacy_metadata->>'wordCount')::int
        )::int AS word_count_mismatches,
        count(d.id) FILTER (WHERE d.source_local_document_id IS NOT NULL AND d.revision <> 1)::int AS non_initial_revisions
      FROM platform_users pu
      LEFT JOIN documents d
        ON d.owner_user_id = pu.id
        AND d.document_kind = 'markdown'
        AND d.source_local_document_id IS NOT NULL
      WHERE pu.account_number IN ('POME-000001', 'POME-000004')
      GROUP BY pu.id, pu.account_number
      ORDER BY pu.account_number
    `);
    console.info(JSON.stringify({ status: "ok", accounts: result.rows }));
  } finally {
    await closeDatabasePool(pool);
  }
}

verify().catch(() => {
  console.error("统一文档元数据核验失败");
  process.exitCode = 1;
});
