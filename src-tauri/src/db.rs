//! 本地存储：SQLite 单表 `items` + 图片文件目录。
//!
//! ```text
//! <app_data_dir>/
//!   history.db               历史记录
//!   images/<hash>.png        图片原图，文件名即内容哈希，同一张图只存一份
//!   images/<hash>.thumb.png  大图的缩略图，列表里显示它
//! ```
//!
//! 历史多了也不卡，靠这几点：
//! - 列表按 `(updated_at, id)` 游标分页，不用 OFFSET，翻到多深都是一次索引定位；
//! - 列表要的东西（预览、字数、大小）入库时算好放在前面的小字段里，长文本和富文本原文放在最后两列，
//!   列表查询碰不到它们的溢出页；
//! - 一个覆盖索引包住时间、类型和大小：各类型计数、清理预估、空间统计都只扫索引，不回表。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

const DB_FILE: &str = "history.db";
const IMAGES_DIR: &str = "images";
/// 列表里文本只回传前 N 个字符，长文本不必整段过 IPC。
const PREVIEW_CHARS: usize = 400;

/// 存在 `PRAGMA user_version` 里。v1 是最初的版本（大字段在中间、没有预览/大小列）。
const SCHEMA_VERSION: i64 = 2;

const CREATE_ITEMS: &str = "
CREATE TABLE items (
    id         TEXT    PRIMARY KEY,
    kind       TEXT    NOT NULL,            -- text | image | files
    sub_kind   TEXT,                        -- 仅 text：url | email | color | path | html | rtf
    hash       TEXT    NOT NULL UNIQUE,
    width      INTEGER,
    height     INTEGER,
    char_count INTEGER NOT NULL DEFAULT 0,  -- text: 字符数；files: 文件个数
    bytes      INTEGER NOT NULL DEFAULT 0,  -- 占用估算：文本按 UTF-8 字节，图片按文件大小
    created_at INTEGER NOT NULL,            -- 毫秒时间戳
    updated_at INTEGER NOT NULL,            -- 最近一次复制或粘贴，列表按它倒序
    preview    TEXT    NOT NULL,            -- text: 前 400 字符；image: 列表显示的图片文件名；files: 全部路径
    content    TEXT    NOT NULL,            -- text: 纯文本；image: 原图文件名；files: 换行分隔的路径
    rich       TEXT                         -- html / rtf 原文，粘贴时与纯文本一起写回
);
CREATE INDEX idx_items_recent ON items (updated_at, id, kind, sub_kind, bytes);
";

/// v1 → v2：旧表改名、建新表、搬数据。图片大小要读文件，在 Rust 里补。
const MIGRATE_FROM_V1: &str = "
INSERT INTO items (id, kind, sub_kind, hash, width, height, char_count, bytes,
                   created_at, updated_at, preview, content, rich)
SELECT id, kind, sub_kind, hash, width, height,
       CASE kind
           WHEN 'text'  THEN length(content)
           WHEN 'files' THEN length(content) - length(replace(content, char(10), '')) + 1
           ELSE 0
       END,
       CASE kind
           WHEN 'image' THEN 0
           ELSE length(CAST(content AS BLOB)) + ifnull(length(CAST(rich AS BLOB)), 0)
       END,
       created_at, updated_at,
       CASE kind WHEN 'text' THEN substr(content, 1, 400) ELSE content END,
       content, rich
FROM items_v1;
DROP TABLE items_v1;
";

/// 待入库的一条记录。用 [`NewItem::text`] 等构造，派生字段（预览、字数、大小）在构造时算好。
#[derive(Debug)]
pub struct NewItem {
    pub kind: &'static str,
    pub sub_kind: Option<&'static str>,
    pub hash: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub char_count: i64,
    pub bytes: i64,
    pub preview: String,
    pub content: String,
    pub rich: Option<String>,
}

impl NewItem {
    pub fn text(
        content: String,
        sub_kind: Option<&'static str>,
        rich: Option<String>,
        hash: String,
    ) -> Self {
        Self {
            kind: "text",
            sub_kind,
            hash,
            width: None,
            height: None,
            char_count: content.chars().count() as i64,
            bytes: (content.len() + rich.as_ref().map_or(0, String::len)) as i64,
            preview: content.chars().take(PREVIEW_CHARS).collect(),
            content,
            rich,
        }
    }

    pub fn files(paths: &[String], hash: String) -> Self {
        let content = paths.join("\n");
        Self {
            kind: "files",
            sub_kind: None,
            hash,
            width: None,
            height: None,
            char_count: paths.len() as i64,
            bytes: content.len() as i64,
            preview: content.clone(),
            content,
            rich: None,
        }
    }

    /// `thumb` 为缩略图文件名（小图没有缩略图），`bytes` 为原图与缩略图文件大小之和。
    pub fn image(
        file_name: String,
        thumb: Option<String>,
        (width, height): (u32, u32),
        bytes: u64,
        hash: String,
    ) -> Self {
        Self {
            kind: "image",
            sub_kind: None,
            hash,
            width: Some(width),
            height: Some(height),
            char_count: 0,
            bytes: bytes as i64,
            preview: thumb.unwrap_or_else(|| file_name.clone()),
            content: file_name,
            rich: None,
        }
    }
}

/// 粘贴写回需要的字段。
pub struct StoredItem {
    pub kind: String,
    pub sub_kind: Option<String>,
    pub content: String,
    pub rich: Option<String>,
}

/// 返回给前端的列表项。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemView {
    pub id: String,
    pub kind: String,
    pub sub_kind: Option<String>,
    /// text: 前 `PREVIEW_CHARS` 个字符；files: 换行分隔的全部路径。
    pub preview: String,
    /// text: 字符数；files: 文件个数。
    pub char_count: i64,
    pub image_path: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub updated_at: i64,
}

/// 面板顶部的类型筛选。与前端 `FILTERS` 一一对应。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Filter {
    All,
    Text,
    Url,
    Color,
    Image,
    Files,
}

impl Filter {
    const ALL: [Filter; 6] = [
        Filter::All,
        Filter::Text,
        Filter::Url,
        Filter::Color,
        Filter::Image,
        Filter::Files,
    ];

    fn key(self) -> &'static str {
        match self {
            Filter::All => "all",
            Filter::Text => "text",
            Filter::Url => "url",
            Filter::Color => "color",
            Filter::Image => "image",
            Filter::Files => "files",
        }
    }

    /// 列表的 WHERE 和计数的 SUM 共用这一份条件，两边口径不会不一致。
    fn condition(self) -> &'static str {
        match self {
            Filter::All => "1",
            // 「文本」= 链接、颜色以外的所有文字（含邮箱、路径、富文本）
            Filter::Text => "kind = 'text' AND ifnull(sub_kind, '') NOT IN ('url', 'color')",
            Filter::Url => "kind = 'text' AND sub_kind = 'url'",
            Filter::Color => "kind = 'text' AND sub_kind = 'color'",
            Filter::Image => "kind = 'image'",
            Filter::Files => "kind = 'files'",
        }
    }
}

/// 分页游标：上一页最后一条的 `(updated_at, id)`。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cursor {
    pub updated_at: i64,
    pub id: String,
}

/// 占用空间：数据库文件 + 图片文件。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Usage {
    pub count: i64,
    pub bytes: u64,
}

pub struct Db {
    conn: Mutex<Connection>,
    images_dir: PathBuf,
    /// 内存数据库（测试）为 None。
    db_path: Option<PathBuf>,
}

impl Db {
    pub fn open(app: &AppHandle) -> anyhow::Result<Self> {
        Self::open_dir(&app.path().app_data_dir()?)
    }

    fn open_dir(data_dir: &Path) -> anyhow::Result<Self> {
        let path = data_dir.join(DB_FILE);
        let images_dir = data_dir.join(IMAGES_DIR);
        std::fs::create_dir_all(&images_dir).with_context(|| format!("create {images_dir:?}"))?;

        let conn = Connection::open(&path).with_context(|| format!("open sqlite at {path:?}"))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        log::info!("history db at {path:?}");

        Self::with_connection(conn, images_dir, Some(path))
    }

    fn with_connection(
        mut conn: Connection,
        images_dir: PathBuf,
        db_path: Option<PathBuf>,
    ) -> anyhow::Result<Self> {
        migrate(&mut conn, &images_dir)?;
        Ok(Self {
            conn: Mutex::new(conn),
            images_dir,
            db_path,
        })
    }

    pub fn images_dir(&self) -> &Path {
        &self.images_dir
    }

    fn conn(&self) -> MutexGuard<'_, Connection> {
        self.conn
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// 入库。同内容再次复制不新增行，只把它顶到最前（并以最新一次的富文本格式为准）；
    /// 之后按 `max_items` 淘汰最久未用的记录。
    pub fn upsert(&self, item: &NewItem, max_items: usize) -> anyhow::Result<()> {
        self.upsert_at(item, max_items, now_ms())
    }

    fn upsert_at(&self, item: &NewItem, max_items: usize, now: i64) -> anyhow::Result<()> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO items (id, kind, sub_kind, hash, width, height, char_count, bytes,
                                created_at, updated_at, preview, content, rich)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9, ?10, ?11, ?12)
             ON CONFLICT(hash) DO UPDATE SET
                updated_at = excluded.updated_at,
                sub_kind   = excluded.sub_kind,
                rich       = excluded.rich,
                bytes      = excluded.bytes",
            params![
                uuid::Uuid::new_v4().to_string(),
                item.kind,
                item.sub_kind,
                item.hash,
                item.width,
                item.height,
                item.char_count,
                item.bytes,
                now,
                item.preview,
                item.content,
                item.rich,
            ],
        )?;

        let stale: Vec<(String, Option<String>, Option<String>)> = conn
            .prepare_cached(
                "SELECT id,
                        CASE WHEN kind = 'image' THEN content END,
                        CASE WHEN kind = 'image' THEN preview END
                 FROM items ORDER BY updated_at DESC, id DESC LIMIT -1 OFFSET ?1",
            )?
            .query_map([max_items as i64], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })?
            .collect::<Result<_, _>>()?;
        for (id, content, preview) in stale {
            conn.execute("DELETE FROM items WHERE id = ?1", [&id])?;
            if let (Some(content), Some(preview)) = (content, preview) {
                self.remove_image_files(&content, &preview);
            }
        }
        Ok(())
    }

    /// 一页列表：`since` 之后、命中 `filter`、排在 `after` 之后的最多 `limit` 条，最近的在前。
    pub fn list(
        &self,
        since: i64,
        filter: Filter,
        after: Option<&Cursor>,
        limit: u32,
    ) -> anyhow::Result<Vec<ItemView>> {
        let conn = self.conn();
        let sql = format!(
            "SELECT id, kind, sub_kind, preview, char_count, width, height, updated_at
             FROM items
             WHERE updated_at >= ?1 AND ({}) {}
             ORDER BY updated_at DESC, id DESC
             LIMIT ?2",
            filter.condition(),
            if after.is_some() {
                "AND (updated_at, id) < (?3, ?4)"
            } else {
                ""
            },
        );
        let mut stmt = conn.prepare_cached(&sql)?;
        let to_view = |r: &Row| self.item_view(r);
        let items = match after {
            Some(c) => stmt
                .query_map(params![since, limit, c.updated_at, c.id], to_view)?
                .collect::<Result<_, _>>()?,
            None => stmt
                .query_map(params![since, limit], to_view)?
                .collect::<Result<_, _>>()?,
        };
        Ok(items)
    }

    fn item_view(&self, r: &Row) -> rusqlite::Result<ItemView> {
        let kind: String = r.get(1)?;
        let preview: String = r.get(3)?;
        let image_path = (kind == "image").then(|| {
            self.images_dir
                .join(&preview)
                .to_string_lossy()
                .into_owned()
        });
        Ok(ItemView {
            id: r.get(0)?,
            kind,
            sub_kind: r.get(2)?,
            preview,
            char_count: r.get(4)?,
            image_path,
            width: r.get(5)?,
            height: r.get(6)?,
            updated_at: r.get(7)?,
        })
    }

    /// `since` 之后每个筛选类型各有多少条，一次扫描（只扫索引）全部算出。
    pub fn counts(&self, since: i64) -> anyhow::Result<BTreeMap<&'static str, i64>> {
        let sums: Vec<String> = Filter::ALL
            .iter()
            .map(|f| format!("ifnull(sum({}), 0)", f.condition()))
            .collect();
        let sql = format!(
            "SELECT {} FROM items WHERE updated_at >= ?1",
            sums.join(", ")
        );
        let conn = self.conn();
        let values = conn.prepare_cached(&sql)?.query_row([since], |r| {
            (0..Filter::ALL.len())
                .map(|i| r.get::<_, i64>(i))
                .collect::<Result<Vec<_>, _>>()
        })?;
        Ok(Filter::ALL.iter().map(|f| f.key()).zip(values).collect())
    }

    pub fn get(&self, id: &str) -> anyhow::Result<Option<StoredItem>> {
        let item = self
            .conn()
            .query_row(
                "SELECT kind, sub_kind, content, rich FROM items WHERE id = ?1",
                [id],
                |r| {
                    Ok(StoredItem {
                        kind: r.get(0)?,
                        sub_kind: r.get(1)?,
                        content: r.get(2)?,
                        rich: r.get(3)?,
                    })
                },
            )
            .optional()?;
        Ok(item)
    }

    /// 粘贴后把记录顶到最前。
    pub fn touch(&self, id: &str) -> anyhow::Result<()> {
        self.conn().execute(
            "UPDATE items SET updated_at = ?1 WHERE id = ?2",
            params![now_ms(), id],
        )?;
        Ok(())
    }

    pub fn delete(&self, id: &str) -> anyhow::Result<()> {
        let conn = self.conn();
        let image: Option<(String, String)> = conn
            .query_row(
                "SELECT content, preview FROM items WHERE id = ?1 AND kind = 'image'",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        conn.execute("DELETE FROM items WHERE id = ?1", [id])?;
        if let Some((content, preview)) = image {
            self.remove_image_files(&content, &preview);
        }
        Ok(())
    }

    /// 最近使用时间早于 `before` 的记录有多少条、大约占多少字节（清理前的确认提示用）。
    pub fn stats_before(&self, before: i64) -> anyhow::Result<(i64, u64)> {
        let (count, bytes): (i64, i64) = self.conn().query_row(
            "SELECT count(*), ifnull(sum(bytes), 0) FROM items WHERE updated_at < ?1",
            [before],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        Ok((count, bytes.max(0) as u64))
    }

    /// 删除最近使用时间早于 `before` 的记录及其图片，再 VACUUM 把空间真正还给磁盘。
    pub fn delete_before(&self, before: i64) -> anyhow::Result<usize> {
        let conn = self.conn();
        let images: Vec<(String, String)> = conn
            .prepare("SELECT content, preview FROM items WHERE kind = 'image' AND updated_at < ?1")?
            .query_map([before], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<_, _>>()?;
        let deleted = conn.execute("DELETE FROM items WHERE updated_at < ?1", [before])?;

        // SQLite 删行只是把页标记为空闲，文件不会变小；不 VACUUM 的话「已占用」看起来纹丝不动
        conn.execute("VACUUM", [])?;
        if self.db_path.is_some() {
            conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(()))?;
        }
        drop(conn);

        for (content, preview) in images {
            self.remove_image_files(&content, &preview);
        }
        Ok(deleted)
    }

    /// 总条数，以及数据库文件（含 WAL）与图片文件加起来占的磁盘空间。
    pub fn usage(&self) -> anyhow::Result<Usage> {
        let (count, image_bytes): (i64, i64) = self.conn().query_row(
            "SELECT count(*), ifnull(sum(CASE WHEN kind = 'image' THEN bytes END), 0) FROM items",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        let db_bytes: u64 = self.db_path.as_ref().map_or(0, |path| {
            ["", "-wal", "-shm"]
                .iter()
                .filter_map(|suffix| {
                    let mut file = path.clone().into_os_string();
                    file.push(suffix);
                    std::fs::metadata(file).ok()
                })
                .map(|m| m.len())
                .sum()
        });
        Ok(Usage {
            count,
            bytes: db_bytes + image_bytes.max(0) as u64,
        })
    }

    /// 图片文件名即哈希、表里 hash 唯一，所以删掉行后对应文件一定没人引用了。
    fn remove_image_files(&self, content: &str, preview: &str) {
        remove_file_quietly(&self.images_dir.join(content));
        if preview != content {
            remove_file_quietly(&self.images_dir.join(preview));
        }
    }
}

fn migrate(conn: &mut Connection, images_dir: &Path) -> anyhow::Result<()> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version >= SCHEMA_VERSION {
        return Ok(());
    }

    let tx = conn.transaction()?;
    let has_v1: bool = tx.query_row(
        "SELECT count(*) > 0 FROM sqlite_master WHERE type = 'table' AND name = 'items'",
        [],
        |r| r.get(0),
    )?;
    if has_v1 {
        tx.execute_batch("ALTER TABLE items RENAME TO items_v1;")?;
        tx.execute_batch(CREATE_ITEMS)?;
        tx.execute_batch(MIGRATE_FROM_V1)?;

        let images: Vec<(String, String)> = tx
            .prepare("SELECT id, content FROM items WHERE kind = 'image'")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<_, _>>()?;
        for (id, file) in images {
            let bytes = std::fs::metadata(images_dir.join(&file)).map_or(0, |m| m.len());
            tx.execute(
                "UPDATE items SET bytes = ?1 WHERE id = ?2",
                params![bytes as i64, id],
            )?;
        }
        log::info!("history db migrated to v{SCHEMA_VERSION}");
    } else {
        tx.execute_batch(CREATE_ITEMS)?;
    }
    tx.execute_batch(&format!("PRAGMA user_version = {SCHEMA_VERSION};"))?;
    tx.commit()?;
    Ok(())
}

fn remove_file_quietly(path: &Path) {
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => log::warn!("remove {path:?} failed: {err}"),
    }
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("minipaste-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn memory_db() -> Db {
        Db::with_connection(Connection::open_in_memory().unwrap(), temp_dir(), None).unwrap()
    }

    fn text(content: &str) -> NewItem {
        NewItem::text(content.to_owned(), None, None, format!("h:{content}"))
    }

    fn with_sub(content: &str, sub_kind: &'static str) -> NewItem {
        NewItem::text(
            content.to_owned(),
            Some(sub_kind),
            None,
            format!("h:{content}"),
        )
    }

    fn image(db: &Db, name: &str, with_thumb: bool) -> NewItem {
        std::fs::write(db.images_dir().join(format!("{name}.png")), b"png").unwrap();
        let thumb = with_thumb.then(|| {
            let thumb = format!("{name}.thumb.png");
            std::fs::write(db.images_dir().join(&thumb), b"t").unwrap();
            thumb
        });
        NewItem::image(
            format!("{name}.png"),
            thumb,
            (1, 1),
            4,
            format!("image:{name}"),
        )
    }

    fn previews(items: Vec<ItemView>) -> Vec<String> {
        items.into_iter().map(|i| i.preview).collect()
    }

    fn all(db: &Db) -> Vec<String> {
        previews(db.list(0, Filter::All, None, 1000).unwrap())
    }

    #[test]
    fn duplicate_moves_to_front_without_new_row() {
        let db = memory_db();
        db.upsert_at(&text("a"), 10, 1).unwrap();
        db.upsert_at(&text("b"), 10, 2).unwrap();
        db.upsert_at(&text("a"), 10, 3).unwrap();

        assert_eq!(all(&db), ["a", "b"]);
    }

    #[test]
    fn evicts_least_recent_beyond_limit() {
        let db = memory_db();
        for (t, s) in ["1", "2", "3", "4"].into_iter().enumerate() {
            db.upsert_at(&text(s), 3, t as i64).unwrap();
        }

        assert_eq!(all(&db), ["4", "3", "2"]);
    }

    #[test]
    fn delete_removes_row_and_image_files() {
        let db = memory_db();
        db.upsert_at(&image(&db, "abc", true), 10, 1).unwrap();
        let item = db.list(0, Filter::All, None, 10).unwrap().remove(0);
        assert!(item.image_path.unwrap().ends_with("abc.thumb.png"));

        db.delete(&item.id).unwrap();

        assert!(all(&db).is_empty());
        assert!(!db.images_dir().join("abc.png").exists());
        assert!(!db.images_dir().join("abc.thumb.png").exists());
    }

    #[test]
    fn preview_is_truncated_but_char_count_is_full() {
        let db = memory_db();
        db.upsert_at(&text(&"字".repeat(1000)), 10, 1).unwrap();
        let item = db.list(0, Filter::All, None, 10).unwrap().remove(0);

        assert_eq!(item.preview.chars().count(), PREVIEW_CHARS);
        assert_eq!(item.char_count, 1000);
    }

    #[test]
    fn cursor_pagination_has_no_gaps_even_with_equal_timestamps() {
        let db = memory_db();
        // 7 条里有 4 条时间戳相同：只按时间翻页会漏或重，必须靠 id 兜底
        for (s, t) in [
            ("a", 1),
            ("b", 2),
            ("c", 2),
            ("d", 2),
            ("e", 2),
            ("f", 3),
            ("g", 4),
        ] {
            db.upsert_at(&text(s), 100, t).unwrap();
        }

        let mut seen = Vec::new();
        let mut cursor: Option<Cursor> = None;
        loop {
            let page = db.list(0, Filter::All, cursor.as_ref(), 2).unwrap();
            let Some(last) = page.last() else { break };
            cursor = Some(Cursor {
                updated_at: last.updated_at,
                id: last.id.clone(),
            });
            seen.extend(previews(page));
        }

        assert_eq!(seen.len(), 7);
        assert_eq!(seen.first().map(String::as_str), Some("g"));
        assert_eq!(seen.last().map(String::as_str), Some("a"));
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), 7);
    }

    #[test]
    fn period_and_filter_apply_to_list_and_counts() {
        let db = memory_db();
        db.upsert_at(&text("old note"), 100, 10).unwrap();
        db.upsert_at(&with_sub("https://a.com", "url"), 100, 20)
            .unwrap();
        db.upsert_at(&with_sub("#fff", "color"), 100, 30).unwrap();
        db.upsert_at(&with_sub("a@b.com", "email"), 100, 40)
            .unwrap();
        db.upsert_at(&image(&db, "img", false), 100, 50).unwrap();
        db.upsert_at(
            &NewItem::files(&["/a".into(), "/b".into()], "files:ab".into()),
            100,
            60,
        )
        .unwrap();

        let counts = db.counts(15).unwrap();
        assert_eq!(counts["all"], 5);
        assert_eq!(counts["text"], 1, "email 算文本，url / color 不算");
        assert_eq!(counts["url"], 1);
        assert_eq!(counts["color"], 1);
        assert_eq!(counts["image"], 1);
        assert_eq!(counts["files"], 1);

        assert_eq!(
            previews(db.list(15, Filter::Text, None, 10).unwrap()),
            ["a@b.com"]
        );
        assert_eq!(
            previews(db.list(0, Filter::Text, None, 10).unwrap()),
            ["a@b.com", "old note"]
        );
        assert_eq!(db.counts(1000).unwrap()["all"], 0);

        let files = db.list(0, Filter::Files, None, 10).unwrap().remove(0);
        assert_eq!(files.char_count, 2);
    }

    #[test]
    fn delete_before_removes_old_rows_and_their_images() {
        let db = memory_db();
        db.upsert_at(&image(&db, "old", true), 100, 10).unwrap();
        db.upsert_at(&text("old"), 100, 20).unwrap();
        db.upsert_at(&text("new"), 100, 30).unwrap();

        assert_eq!(db.stats_before(25).unwrap(), (2, 4 + 3));
        assert_eq!(db.delete_before(25).unwrap(), 2);

        assert_eq!(all(&db), ["new"]);
        assert!(!db.images_dir().join("old.png").exists());
        assert!(!db.images_dir().join("old.thumb.png").exists());

        assert_eq!(db.delete_before(i64::MAX).unwrap(), 1);
        assert!(all(&db).is_empty());
    }

    #[test]
    fn usage_counts_rows_and_image_bytes() {
        let db = memory_db();
        db.upsert_at(&text("hello"), 100, 1).unwrap();
        db.upsert_at(&image(&db, "x", false), 100, 2).unwrap();

        assert_eq!(db.usage().unwrap(), Usage { count: 2, bytes: 4 });
    }

    #[test]
    fn hot_queries_only_touch_the_index() {
        let db = memory_db();
        let conn = db.conn();
        let plan = |sql: &str| -> String {
            conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
                .unwrap()
                .query_map([], |r| r.get::<_, String>(3))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
                .join(" | ")
        };

        let count_plan = plan(&format!(
            "SELECT ifnull(sum({}), 0) FROM items WHERE updated_at >= 0",
            Filter::Text.condition()
        ));
        assert!(
            count_plan.contains("COVERING INDEX idx_items_recent"),
            "{count_plan}"
        );

        let page_plan = plan(
            "SELECT id FROM items WHERE updated_at >= 0 AND (updated_at, id) < (5, 'x')
             ORDER BY updated_at DESC, id DESC LIMIT 60",
        );
        assert!(page_plan.contains("idx_items_recent"), "{page_plan}");
        assert!(
            !page_plan.contains("TEMP B-TREE"),
            "不应额外排序: {page_plan}"
        );

        let usage_plan = plan("SELECT count(*), sum(bytes) FROM items WHERE kind = 'image'");
        assert!(usage_plan.contains("COVERING INDEX"), "{usage_plan}");
    }

    #[test]
    fn migrates_v1_database() {
        let dir = temp_dir();
        std::fs::write(dir.join("pic.png"), b"12345").unwrap();
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE items (
                id TEXT PRIMARY KEY, kind TEXT NOT NULL, sub_kind TEXT, content TEXT NOT NULL,
                rich TEXT, hash TEXT NOT NULL UNIQUE, width INTEGER, height INTEGER,
                created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL);
             CREATE INDEX idx_items_updated_at ON items (updated_at DESC);",
        )
        .unwrap();
        let long = "长".repeat(500);
        for (id, kind, sub, content, rich, t) in [
            (
                "1",
                "text",
                Some("html"),
                long.as_str(),
                Some("<b>x</b>"),
                1,
            ),
            ("2", "files", None, "/a\n/b\n/c", None, 2),
            ("3", "image", None, "pic.png", None, 3),
        ] {
            conn.execute(
                "INSERT INTO items VALUES (?1, ?2, ?3, ?4, ?5, ?1, 10, 10, ?6, ?6)",
                params![id, kind, sub, content, rich, t],
            )
            .unwrap();
        }

        let db = Db::with_connection(conn, dir, None).unwrap();

        let items = db.list(0, Filter::All, None, 10).unwrap();
        assert_eq!(items.len(), 3);
        assert!(items[0].image_path.as_ref().unwrap().ends_with("pic.png"));
        assert_eq!(items[1].char_count, 3);
        assert_eq!(items[2].char_count, 500);
        assert_eq!(items[2].preview.chars().count(), PREVIEW_CHARS);
        assert_eq!(db.usage().unwrap().bytes, 5);
        assert_eq!(
            db.get("1").unwrap().unwrap().rich.as_deref(),
            Some("<b>x</b>")
        );
        let version: i64 = db
            .conn()
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
    }
}
