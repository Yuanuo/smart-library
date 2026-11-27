//! Full application backup & staged restore (`.smartbak`, standard ZIP).
//!
//! - Hot backup uses SQLite `VACUUM INTO` so live databases (`history.db`,
//!   `marks.db`, `props.db`, `ai_chat.db`, `snapshots.db`, and internal
//!   `repos/<id>/notes.db`) are snapshotted consistently without closing
//!   active connections or copying WAL/SHM sidecars.
//! - Rebuildable caches (`repos/*/index`, `titles.bin`, `zim.bin`), machine-local
//!   files (`window.json`, `git.json`), and WebView data are excluded.
//! - Restore stages extracted files into `<data_dir>/.pending_restore/` and
//!   writes `ready.json` last; `apply_pending_restore` swaps them into place on
//!   the next launch before any database is opened (avoiding Windows file locks)
//!   while saving overwritten files in `<data_dir>/.restore_bak/`.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

pub const BACKUP_FORMAT: &str = "smart-library-backup";
pub const BACKUP_VERSION: u32 = 1;

const PENDING_DIR: &str = ".pending_restore";
const BACKUP_OLD_DIR: &str = ".restore_bak";
const READY_FILE: &str = "ready.json";

const CONFIG_FILES: &[&str] = &[
    "settings.json",
    "libraries.json",
    "dynasties.txt",
    "pinyin.txt",
    "dicts.json",
    "mcp.json",
];

const TOP_SQLITE_DBS: &[&str] = &[
    "history.db",
    "marks.db",
    "props.db",
    "ai_chat.db",
    "snapshots.db",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupOptions {
    pub include_secrets: bool,
    pub include_dicts: bool,
}

impl Default for BackupOptions {
    fn default() -> Self {
        Self {
            include_secrets: true,
            include_dicts: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupLibrarySummary {
    pub id: String,
    pub title: String,
    pub kind: String,
    pub internal: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupFileEntry {
    pub path: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupManifest {
    pub format: String,
    pub version: u32,
    pub app_version: String,
    pub created_at: i64,
    pub source_os: String,
    pub include_secrets: bool,
    pub include_dicts: bool,
    pub libraries: Vec<BackupLibrarySummary>,
    pub files: Vec<BackupFileEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupResult {
    pub path: String,
    pub archive_bytes: u64,
    pub file_count: usize,
    pub manifest: BackupManifest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupInspection {
    pub manifest: BackupManifest,
    pub archive_bytes: u64,
    pub total_bytes: u64,
    pub has_settings: bool,
    pub has_history: bool,
    pub has_marks: bool,
    pub has_ai_chat: bool,
    pub has_snapshots: bool,
    pub internal_notes_count: usize,
    pub external_libs_count: usize,
    pub cross_os: bool,
}

struct TempDirGuard(PathBuf);

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn make_temp_dir(prefix: &str) -> Result<TempDirGuard, String> {
    let pid = std::process::id();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("{}-{}-{}", prefix, pid, nanos));
    fs::create_dir_all(&dir).map_err(|e| format!("无法创建临时目录：{}", e))?;
    Ok(TempDirGuard(dir))
}

fn snapshot_sqlite(src: &Path, dst: &Path) -> Result<(), String> {
    if dst.exists() {
        let _ = fs::remove_file(dst);
    }
    let conn = rusqlite::Connection::open_with_flags(
        src,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| format!("打开数据库失败（{}）：{}", src.display(), e))?;
    let _ = conn.busy_timeout(std::time::Duration::from_secs(5));
    let dst_str = dst
        .to_str()
        .ok_or_else(|| "临时快照路径包含非 UTF-8 字符".to_string())?;
    conn.execute("VACUUM INTO ?1", rusqlite::params![dst_str])
        .map_err(|e| format!("导出数据库快照失败（{}）：{}", src.display(), e))?;
    Ok(())
}

fn strip_settings_secrets(bytes: &[u8]) -> Vec<u8> {
    let Ok(mut val) = serde_json::from_slice::<serde_json::Value>(bytes) else {
        return bytes.to_vec();
    };
    if let Some(presets) = val.get_mut("aiPresets").and_then(|v| v.as_array_mut()) {
        for p in presets {
            if let Some(obj) = p.as_object_mut() {
                obj.insert("apiKey".to_string(), serde_json::Value::String(String::new()));
            }
        }
    }
    serde_json::to_vec_pretty(&val).unwrap_or_else(|_| bytes.to_vec())
}

fn strip_mcp_secrets(bytes: &[u8]) -> Vec<u8> {
    let Ok(mut val) = serde_json::from_slice::<serde_json::Value>(bytes) else {
        return bytes.to_vec();
    };
    if let Some(obj) = val.as_object_mut() {
        obj.insert("token".to_string(), serde_json::Value::String(String::new()));
    }
    serde_json::to_vec_pretty(&val).unwrap_or_else(|_| bytes.to_vec())
}

fn read_library_summaries(config_dir: &Path) -> Vec<BackupLibrarySummary> {
    let Ok(text) = fs::read_to_string(config_dir.join("libraries.json")) else {
        return Vec::new();
    };
    let Ok(val) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Vec::new();
    };
    let Some(arr) = val.as_array() else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|item| {
            let id = item.get("id")?.as_str()?.to_string();
            let title = item
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or(&id)
                .to_string();
            let kind = item
                .get("kind")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            let internal = kind == "notes";
            Some(BackupLibrarySummary {
                id,
                title,
                kind,
                internal,
            })
        })
        .collect()
}

fn add_bytes_to_zip(
    zip: &mut ZipWriter<File>,
    archive_path: &str,
    bytes: &[u8],
    method: CompressionMethod,
    files: &mut Vec<BackupFileEntry>,
) -> Result<(), String> {
    let opts = SimpleFileOptions::default().compression_method(method);
    zip.start_file(archive_path, opts)
        .map_err(|e| format!("写入备份条目失败（{}）：{}", archive_path, e))?;
    zip.write_all(bytes)
        .map_err(|e| format!("写入备份内容失败（{}）：{}", archive_path, e))?;
    files.push(BackupFileEntry {
        path: archive_path.to_string(),
        bytes: bytes.len() as u64,
    });
    Ok(())
}

fn add_file_to_zip(
    zip: &mut ZipWriter<File>,
    archive_path: &str,
    disk_path: &Path,
    method: CompressionMethod,
    files: &mut Vec<BackupFileEntry>,
) -> Result<(), String> {
    let mut f = File::open(disk_path)
        .map_err(|e| format!("读取文件失败（{}）：{}", disk_path.display(), e))?;
    let opts = SimpleFileOptions::default().compression_method(method);
    zip.start_file(archive_path, opts)
        .map_err(|e| format!("写入备份条目失败（{}）：{}", archive_path, e))?;
    let written = std::io::copy(&mut f, zip)
        .map_err(|e| format!("复制文件内容失败（{}）：{}", archive_path, e))?;
    files.push(BackupFileEntry {
        path: archive_path.to_string(),
        bytes: written,
    });
    Ok(())
}

fn collect_dir_files(root: &Path, cur: &Path, out: &mut Vec<(String, PathBuf)>) -> Result<(), String> {
    let rd = match fs::read_dir(cur) {
        Ok(rd) => rd,
        Err(_) => return Ok(()),
    };
    let mut entries: Vec<_> = rd.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let p = entry.path();
        let Ok(ft) = entry.file_type() else { continue };
        if ft.is_symlink() {
            continue;
        }
        if ft.is_dir() {
            collect_dir_files(root, &p, out)?;
        } else if ft.is_file() {
            let rel = p
                .strip_prefix(root)
                .map_err(|e| e.to_string())?
                .to_string_lossy()
                .replace('\\', "/");
            out.push((rel, p));
        }
    }
    Ok(())
}

fn compression_for_ext(path: &Path) -> CompressionMethod {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
        .as_deref()
    {
        Some("png" | "jpg" | "jpeg" | "webp" | "gif" | "avif" | "zip" | "gz" | "zst" | "mp3" | "mp4" | "pdf") => {
            CompressionMethod::Stored
        }
        _ => CompressionMethod::Deflated,
    }
}

/// Create a consistent `.smartbak` archive from `config_dir` and `data_dir`.
pub fn create_backup(
    config_dir: &Path,
    data_dir: &Path,
    dest: &Path,
    app_version: &str,
    options: &BackupOptions,
) -> Result<BackupResult, String> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("无法创建目标目录：{}", e))?;
    }
    let tmp_dir = make_temp_dir("sl-backup-snap")?;
    let tmp_zip_path = dest.with_extension("smartbak.tmp");
    let _ = fs::remove_file(&tmp_zip_path);

    let result = (|| -> Result<BackupResult, String> {
        let file = File::create(&tmp_zip_path)
            .map_err(|e| format!("无法创建备份文件（{}）：{}", tmp_zip_path.display(), e))?;
        let mut zip = ZipWriter::new(file);
        let mut files: Vec<BackupFileEntry> = Vec::new();

        // 1. Config files
        for name in CONFIG_FILES {
            let p = config_dir.join(name);
            if !p.is_file() {
                continue;
            }
            let raw = fs::read(&p)
                .map_err(|e| format!("读取配置文件失败（{}）：{}", p.display(), e))?;
            let bytes = if !options.include_secrets && *name == "settings.json" {
                strip_settings_secrets(&raw)
            } else if !options.include_secrets && *name == "mcp.json" {
                strip_mcp_secrets(&raw)
            } else {
                raw
            };
            add_bytes_to_zip(
                &mut zip,
                &format!("config/{}", name),
                &bytes,
                CompressionMethod::Deflated,
                &mut files,
            )?;
        }

        // 2. Top-level SQLite databases (hot snapshot via VACUUM INTO)
        for db_name in TOP_SQLITE_DBS {
            let src = data_dir.join(db_name);
            if !src.is_file() {
                continue;
            }
            let snap = tmp_dir.0.join(db_name);
            snapshot_sqlite(&src, &snap)?;
            add_file_to_zip(
                &mut zip,
                &format!("data/{}", db_name),
                &snap,
                CompressionMethod::Deflated,
                &mut files,
            )?;
            let _ = fs::remove_file(&snap);
        }

        // 3. Workspace layout
        let ws = data_dir.join("workspace.json");
        if ws.is_file() {
            add_file_to_zip(
                &mut zip,
                "data/workspace.json",
                &ws,
                CompressionMethod::Deflated,
                &mut files,
            )?;
        }

        // 4. Internal notes libraries (`repos/<id>/notes.db` + `attachments/`)
        let repos_dir = data_dir.join("repos");
        if repos_dir.is_dir() {
            let mut repo_entries: Vec<_> = fs::read_dir(&repos_dir)
                .map_err(|e| format!("读取知识库目录失败：{}", e))?
                .filter_map(|e| e.ok())
                .collect();
            repo_entries.sort_by_key(|e| e.file_name());
            for entry in repo_entries {
                let repo_path = entry.path();
                if !repo_path.is_dir() {
                    continue;
                }
                let Some(repo_id) = entry.file_name().to_str().map(|s| s.to_string()) else {
                    continue;
                };
                let notes_db = repo_path.join("notes.db");
                if notes_db.is_file() {
                    let snap = tmp_dir.0.join(format!("repo-{}-notes.db", repo_id));
                    snapshot_sqlite(&notes_db, &snap)?;
                    add_file_to_zip(
                        &mut zip,
                        &format!("data/repos/{}/notes.db", repo_id),
                        &snap,
                        CompressionMethod::Deflated,
                        &mut files,
                    )?;
                    let _ = fs::remove_file(&snap);
                }
                let attach_dir = repo_path.join("attachments");
                if attach_dir.is_dir() {
                    let mut att_files = Vec::new();
                    collect_dir_files(&attach_dir, &attach_dir, &mut att_files)?;
                    for (rel, full) in att_files {
                        add_file_to_zip(
                            &mut zip,
                            &format!("data/repos/{}/attachments/{}", repo_id, rel),
                            &full,
                            compression_for_ext(&full),
                            &mut files,
                        )?;
                    }
                }
            }
        }

        // 5. Optional imported dictionaries (`data/dicts/`)
        if options.include_dicts {
            let dicts_dir = data_dir.join("dicts");
            if dicts_dir.is_dir() {
                let mut dict_files = Vec::new();
                collect_dir_files(&dicts_dir, &dicts_dir, &mut dict_files)?;
                for (rel, full) in dict_files {
                    add_file_to_zip(
                        &mut zip,
                        &format!("data/dicts/{}", rel),
                        &full,
                        compression_for_ext(&full),
                        &mut files,
                    )?;
                }
            }
        }

        let manifest = BackupManifest {
            format: BACKUP_FORMAT.to_string(),
            version: BACKUP_VERSION,
            app_version: app_version.to_string(),
            created_at: now_unix(),
            source_os: std::env::consts::OS.to_string(),
            include_secrets: options.include_secrets,
            include_dicts: options.include_dicts,
            libraries: read_library_summaries(config_dir),
            files,
        };
        let manifest_bytes =
            serde_json::to_vec_pretty(&manifest).map_err(|e| format!("序列化清单失败：{}", e))?;
        let opts = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        zip.start_file("manifest.json", opts)
            .map_err(|e| format!("写入清单失败：{}", e))?;
        zip.write_all(&manifest_bytes)
            .map_err(|e| format!("写入清单内容失败：{}", e))?;

        let mut finished = zip
            .finish()
            .map_err(|e| format!("完成备份归档失败：{}", e))?;
        finished
            .flush()
            .map_err(|e| format!("刷新备份文件失败：{}", e))?;
        drop(finished);

        fs::rename(&tmp_zip_path, dest)
            .map_err(|e| format!("保存备份文件失败（{}）：{}", dest.display(), e))?;
        let archive_bytes = fs::metadata(dest).map(|m| m.len()).unwrap_or(0);
        let file_count = manifest.files.len();
        Ok(BackupResult {
            path: dest.display().to_string(),
            archive_bytes,
            file_count,
            manifest,
        })
    })();

    if result.is_err() {
        let _ = fs::remove_file(&tmp_zip_path);
    }
    result
}

fn validate_archive_rel_path(rel: &str) -> Result<PathBuf, String> {
    if rel.is_empty() || rel.starts_with('/') || rel.starts_with('\\') || rel.contains('\\') {
        return Err(format!("备份包包含非法路径：{}", rel));
    }
    let p = Path::new(rel);
    for comp in p.components() {
        match comp {
            Component::Normal(_) => {}
            _ => return Err(format!("备份包包含非法路径组件：{}", rel)),
        }
    }
    Ok(p.to_path_buf())
}

/// Inspect and validate a `.smartbak` archive without modifying any local files.
pub fn inspect_backup(archive_path: &Path) -> Result<BackupInspection, String> {
    let archive_bytes = fs::metadata(archive_path)
        .map_err(|e| format!("无法访问备份文件：{}", e))?
        .len();
    let f = File::open(archive_path).map_err(|e| format!("无法打开备份文件：{}", e))?;
    let mut zip =
        ZipArchive::new(f).map_err(|_| "这不是有效的 Smart Library 备份包（非 ZIP 格式）".to_string())?;

    let manifest: BackupManifest = {
        let mut mf = zip
            .by_name("manifest.json")
            .map_err(|_| "备份包缺少 manifest.json，不是有效的迁移备份包".to_string())?;
        let mut s = String::new();
        mf.read_to_string(&mut s)
            .map_err(|e| format!("读取备份清单失败：{}", e))?;
        serde_json::from_str(&s).map_err(|e| format!("解析备份清单失败：{}", e))?
    };

    if manifest.format != BACKUP_FORMAT {
        return Err(format!("不支持的备份格式：{}", manifest.format));
    }
    if manifest.version != BACKUP_VERSION {
        return Err(format!(
            "备份包版本（v{}）与当前程序（v{}）不兼容",
            manifest.version, BACKUP_VERSION
        ));
    }

    let mut names_in_zip = HashSet::new();
    for i in 0..zip.len() {
        let entry = zip
            .by_index(i)
            .map_err(|e| format!("读取备份包索引失败：{}", e))?;
        let name = entry.name().to_string();
        if name != "manifest.json" {
            validate_archive_rel_path(&name)?;
        }
        names_in_zip.insert(name);
    }

    let mut total_bytes: u64 = 0;
    for fe in &manifest.files {
        validate_archive_rel_path(&fe.path)?;
        if !names_in_zip.contains(&fe.path) {
            return Err(format!("备份包不完整，缺少文件：{}", fe.path));
        }
        total_bytes = total_bytes.saturating_add(fe.bytes);
    }

    let has_settings = manifest.files.iter().any(|f| f.path == "config/settings.json");
    let has_history = manifest.files.iter().any(|f| f.path == "data/history.db");
    let has_marks = manifest.files.iter().any(|f| f.path == "data/marks.db");
    let has_ai_chat = manifest.files.iter().any(|f| f.path == "data/ai_chat.db");
    let has_snapshots = manifest.files.iter().any(|f| f.path == "data/snapshots.db");
    let internal_notes_count = manifest
        .files
        .iter()
        .filter(|f| f.path.starts_with("data/repos/") && f.path.ends_with("/notes.db"))
        .count();
    let external_libs_count = manifest.libraries.iter().filter(|l| !l.internal).count();
    let cross_os = !manifest.source_os.is_empty() && manifest.source_os != std::env::consts::OS;

    Ok(BackupInspection {
        manifest,
        archive_bytes,
        total_bytes,
        has_settings,
        has_history,
        has_marks,
        has_ai_chat,
        has_snapshots,
        internal_notes_count,
        external_libs_count,
        cross_os,
    })
}

/// Extract and validate a `.smartbak` archive into `<data_dir>/.pending_restore/`.
/// The actual swap happens on next startup via `apply_pending_restore` before
/// any SQLite connection is opened.
pub fn stage_restore(archive_path: &Path, data_dir: &Path) -> Result<BackupManifest, String> {
    let inspection = inspect_backup(archive_path)?;
    let pending = data_dir.join(PENDING_DIR);
    if pending.exists() {
        fs::remove_dir_all(&pending)
            .map_err(|e| format!("清理旧的待还原暂存目录失败：{}", e))?;
    }
    fs::create_dir_all(&pending)
        .map_err(|e| format!("创建待还原暂存目录失败：{}", e))?;

    let res = (|| -> Result<BackupManifest, String> {
        let f = File::open(archive_path).map_err(|e| format!("无法打开备份文件：{}", e))?;
        let mut zip = ZipArchive::new(f).map_err(|e| format!("打开备份压缩包失败：{}", e))?;

        for fe in &inspection.manifest.files {
            if !fe.path.starts_with("config/") && !fe.path.starts_with("data/") {
                continue;
            }
            let rel = validate_archive_rel_path(&fe.path)?;
            let out_path = pending.join(&rel);
            if let Some(parent) = out_path.parent() {
                fs::create_dir_all(parent)
                    .map_err(|e| format!("创建暂存子目录失败：{}", e))?;
            }
            let mut entry = zip
                .by_name(&fe.path)
                .map_err(|e| format!("读取备份条目失败（{}）：{}", fe.path, e))?;
            let mut out = File::create(&out_path)
                .map_err(|e| format!("写入暂存文件失败（{}）：{}", out_path.display(), e))?;
            std::io::copy(&mut entry, &mut out)
                .map_err(|e| format!("解压备份文件失败（{}）：{}", fe.path, e))?;
        }

        // Write `ready.json` last as the atomic commit marker for startup.
        let ready_tmp = pending.join("ready.json.tmp");
        let ready = pending.join(READY_FILE);
        let bytes = serde_json::to_vec_pretty(&inspection.manifest)
            .map_err(|e| format!("序列化还原标记失败：{}", e))?;
        fs::write(&ready_tmp, bytes).map_err(|e| format!("写入还原标记失败：{}", e))?;
        fs::rename(&ready_tmp, &ready).map_err(|e| format!("提交还原标记失败：{}", e))?;
        Ok(inspection.manifest)
    })();

    if res.is_err() {
        let _ = fs::remove_dir_all(&pending);
    }
    res
}

fn backup_existing_file(src: &Path, bak_dst: &Path) -> Result<(), String> {
    if !src.is_file() {
        return Ok(());
    }
    if let Some(parent) = bak_dst.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::copy(src, bak_dst).map_err(|e| format!("备份现有文件失败（{}）：{}", src.display(), e))?;
    Ok(())
}

fn remove_sqlite_sidecars(db_path: &Path) {
    for suffix in ["-wal", "-shm", "-journal"] {
        let mut s = db_path.as_os_str().to_os_string();
        s.push(suffix);
        let _ = fs::remove_file(PathBuf::from(s));
    }
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<(), String> {
    fs::create_dir_all(dst).map_err(|e| format!("创建目录失败（{}）：{}", dst.display(), e))?;
    for entry in fs::read_dir(src).map_err(|e| e.to_string())?.flatten() {
        let p = entry.path();
        let target = dst.join(entry.file_name());
        let Ok(ft) = entry.file_type() else { continue };
        if ft.is_dir() {
            copy_dir_recursive(&p, &target)?;
        } else if ft.is_file() {
            fs::copy(&p, &target)
                .map_err(|e| format!("复制文件失败（{}）：{}", p.display(), e))?;
        }
    }
    Ok(())
}

/// Apply a staged restore if `<data_dir>/.pending_restore/ready.json` exists.
/// MUST be called at startup before `AppState::new` opens any SQLite databases.
pub fn apply_pending_restore(
    config_dir: &Path,
    data_dir: &Path,
) -> Result<Option<BackupManifest>, String> {
    let pending = data_dir.join(PENDING_DIR);
    let ready = pending.join(READY_FILE);
    if !ready.is_file() {
        return Ok(None);
    }

    let manifest: BackupManifest = serde_json::from_str(
        &fs::read_to_string(&ready).map_err(|e| format!("读取待还原清单失败：{}", e))?,
    )
    .map_err(|e| format!("解析待还原清单失败：{}", e))?;

    let bak_root = data_dir.join(BACKUP_OLD_DIR);
    if bak_root.exists() {
        let _ = fs::remove_dir_all(&bak_root);
    }
    fs::create_dir_all(&bak_root).map_err(|e| format!("创建回滚备份目录失败：{}", e))?;
    fs::create_dir_all(config_dir).map_err(|e| format!("创建配置目录失败：{}", e))?;
    fs::create_dir_all(data_dir).map_err(|e| format!("创建数据目录失败：{}", e))?;

    // 1. Restore config files
    let pending_cfg = pending.join("config");
    if pending_cfg.is_dir() {
        let mut cfg_files = Vec::new();
        collect_dir_files(&pending_cfg, &pending_cfg, &mut cfg_files)?;
        for (rel, src) in cfg_files {
            let dst = config_dir.join(&rel);
            let bak = bak_root.join("config").join(&rel);
            backup_existing_file(&dst, &bak)?;
            if let Some(parent) = dst.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            fs::copy(&src, &dst)
                .map_err(|e| format!("还原配置文件失败（{}）：{}", rel, e))?;
        }
    }

    // 2. Restore top-level data files (`*.db`, `workspace.json`)
    let pending_data = pending.join("data");
    for db_name in TOP_SQLITE_DBS {
        let src = pending_data.join(db_name);
        if src.is_file() {
            let dst = data_dir.join(db_name);
            backup_existing_file(&dst, &bak_root.join("data").join(db_name))?;
            remove_sqlite_sidecars(&dst);
            fs::copy(&src, &dst)
                .map_err(|e| format!("还原数据库失败（{}）：{}", db_name, e))?;
        }
    }
    let pending_ws = pending_data.join("workspace.json");
    if pending_ws.is_file() {
        let dst = data_dir.join("workspace.json");
        backup_existing_file(&dst, &bak_root.join("data").join("workspace.json"))?;
        fs::copy(&pending_ws, &dst).map_err(|e| format!("还原工作区失败：{}", e))?;
    }

    // 3. Restore internal note repos (`repos/<id>/notes.db` + `attachments/`)
    let pending_repos = pending_data.join("repos");
    if pending_repos.is_dir() {
        for entry in fs::read_dir(&pending_repos).map_err(|e| e.to_string())?.flatten() {
            let src_repo = entry.path();
            if !src_repo.is_dir() {
                continue;
            }
            let repo_name = entry.file_name();
            let dst_repo = data_dir.join("repos").join(&repo_name);
            let bak_repo = bak_root.join("data").join("repos").join(&repo_name);
            fs::create_dir_all(&dst_repo).map_err(|e| e.to_string())?;

            let src_db = src_repo.join("notes.db");
            if src_db.is_file() {
                let dst_db = dst_repo.join("notes.db");
                backup_existing_file(&dst_db, &bak_repo.join("notes.db"))?;
                remove_sqlite_sidecars(&dst_db);
                fs::copy(&src_db, &dst_db)
                    .map_err(|e| format!("还原笔记库失败（{}）：{}", dst_db.display(), e))?;
                // Drop stale rebuildable index/title caches for this repo.
                let _ = fs::remove_dir_all(dst_repo.join("index"));
                let _ = fs::remove_file(dst_repo.join("titles.bin"));
            }

            let src_att = src_repo.join("attachments");
            if src_att.is_dir() {
                let dst_att = dst_repo.join("attachments");
                if dst_att.is_dir() {
                    let _ = copy_dir_recursive(&dst_att, &bak_repo.join("attachments"));
                    let _ = fs::remove_dir_all(&dst_att);
                }
                copy_dir_recursive(&src_att, &dst_att)?;
            }
        }
    }

    // 4. Optional dictionaries
    let pending_dicts = pending_data.join("dicts");
    if manifest.include_dicts && pending_dicts.is_dir() {
        let dst_dicts = data_dir.join("dicts");
        if dst_dicts.is_dir() {
            let _ = copy_dir_recursive(&dst_dicts, &bak_root.join("data").join("dicts"));
            let _ = fs::remove_dir_all(&dst_dicts);
        }
        copy_dir_recursive(&pending_dicts, &dst_dicts)?;
    }

    let _ = fs::remove_dir_all(&pending);
    Ok(Some(manifest))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::History;
    use crate::notes::NotesStore;

    #[test]
    fn backup_inspect_stage_and_apply_roundtrip() {
        let src_root = make_temp_dir("sl-bak-test-src").unwrap();
        let src_cfg = src_root.0.join("config");
        let src_data = src_root.0.join("data");
        fs::create_dir_all(&src_cfg).unwrap();
        fs::create_dir_all(src_data.join("repos/notes-1/attachments")).unwrap();
        fs::create_dir_all(src_data.join("repos/notes-1/index")).unwrap();

        fs::write(
            src_cfg.join("settings.json"),
            r#"{"theme":"dark","aiPresets":[{"id":"p1","apiKey":"secret-key-123"}]}"#,
        )
        .unwrap();
        fs::write(
            src_cfg.join("libraries.json"),
            r#"[{"id":"notes-1","title":"我的笔记","kind":"notes"}]"#,
        )
        .unwrap();
        fs::write(src_cfg.join("mcp.json"), r#"{"enabled":true,"token":"tok-999"}"#).unwrap();
        fs::write(src_cfg.join("git.json"), r#"{"git_path":"/usr/bin/git"}"#).unwrap();
        fs::write(src_data.join("workspace.json"), r#"{"version":2}"#).unwrap();
        fs::write(src_data.join("repos/notes-1/attachments/pic.png"), b"PNGDATA").unwrap();
        fs::write(src_data.join("repos/notes-1/index/meta.json"), b"STALE").unwrap();

        // Keep SQLite connections open while backing up to prove hot snapshot works.
        let history = History::open(&src_data.join("history.db")).unwrap();
        history.touch("notes-1", "1", None, "测试文档").unwrap();
        history.set_progress("notes-1", "1", "p10", Some("orig")).unwrap();

        let notes = NotesStore::open(&src_data.join("repos/notes-1/notes.db")).unwrap();
        let note_id = notes.create(None, "第一篇笔记", "正文内容").unwrap();

        let archive = src_root.0.join("out.smartbak");
        let res = create_backup(
            &src_cfg,
            &src_data,
            &archive,
            "26.11.25",
            &BackupOptions {
                include_secrets: false,
                include_dicts: false,
            },
        )
        .unwrap();
        assert!(res.archive_bytes > 0);
        assert!(res.manifest.files.iter().all(|f| !f.path.contains("/index/")));
        assert!(res.manifest.files.iter().all(|f| f.path != "config/git.json"));

        let insp = inspect_backup(&archive).unwrap();
        assert!(insp.has_settings);
        assert!(insp.has_history);
        assert_eq!(insp.internal_notes_count, 1);

        // Stage and apply into a fresh target directory.
        let dst_root = make_temp_dir("sl-bak-test-dst").unwrap();
        let dst_cfg = dst_root.0.join("config");
        let dst_data = dst_root.0.join("data");
        fs::create_dir_all(&dst_cfg).unwrap();
        fs::create_dir_all(&dst_data).unwrap();
        fs::write(dst_cfg.join("settings.json"), r#"{"theme":"light"}"#).unwrap();

        stage_restore(&archive, &dst_data).unwrap();
        let applied = apply_pending_restore(&dst_cfg, &dst_data).unwrap();
        assert!(applied.is_some());
        assert!(!dst_data.join(PENDING_DIR).exists());

        // Secrets were stripped because include_secrets = false.
        let restored_settings: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(dst_cfg.join("settings.json")).unwrap()).unwrap();
        assert_eq!(restored_settings["theme"], "dark");
        assert_eq!(restored_settings["aiPresets"][0]["apiKey"], "");

        let restored_mcp: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(dst_cfg.join("mcp.json")).unwrap()).unwrap();
        assert_eq!(restored_mcp["token"], "");

        // History & reading progress survived intact.
        let h2 = History::open(&dst_data.join("history.db")).unwrap();
        let list = h2.list("", 10).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].lb.as_deref(), Some("p10"));

        // Internal notes & attachments survived intact.
        let n2 = NotesStore::open(&dst_data.join("repos/notes-1/notes.db")).unwrap();
        let doc = n2.get(note_id).unwrap().unwrap();
        assert_eq!(doc.content, "正文内容");
        assert_eq!(
            fs::read(dst_data.join("repos/notes-1/attachments/pic.png")).unwrap(),
            b"PNGDATA"
        );

        // Old settings were backed up to .restore_bak.
        assert!(dst_data.join(BACKUP_OLD_DIR).join("config/settings.json").is_file());
    }
}
