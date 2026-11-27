use super::*;
use std::io::Read;
use std::process::{Command, Stdio};

#[derive(Serialize, Deserialize, Default)]
struct GitConfig {
    #[serde(default)]
    git_path: String,
}

fn git_config_path(config_dir: &Path) -> PathBuf {
    config_dir.join("git.json")
}

fn load_git_config(config_dir: &Path) -> GitConfig {
    std::fs::read_to_string(git_config_path(config_dir))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_git_config(config_dir: &Path, cfg: &GitConfig) -> Result<(), String> {
    std::fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    let text = serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?;
    std::fs::write(git_config_path(config_dir), text).map_err(|e| e.to_string())
}

fn new_cmd(program: &str) -> Command {
    let mut cmd = Command::new(program);
    cmd.env("GIT_TERMINAL_PROMPT", "0");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    cmd
}

fn probe_git_version(program: &str) -> Option<String> {
    let out = new_cmd(program).arg("--version").output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.to_ascii_lowercase().contains("git version") {
        Some(s)
    } else {
        None
    }
}

#[cfg(target_os = "macos")]
fn macos_has_clt() -> bool {
    new_cmd("xcode-select")
        .arg("-p")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Find a working git binary: saved custom path first, then OS-appropriate candidates.
fn resolve_git(config_dir: &Path) -> Option<(String, String)> {
    let saved = load_git_config(config_dir).git_path;
    let saved = saved.trim();
    if !saved.is_empty() {
        if let Some(ver) = probe_git_version(saved) {
            return Some((saved.to_string(), ver));
        }
    }

    let mut candidates: Vec<&str> = Vec::new();
    #[cfg(target_os = "macos")]
    {
        candidates.push("/opt/homebrew/bin/git");
        candidates.push("/usr/local/bin/git");
        // Avoid invoking /usr/bin/git when Command Line Tools are absent (triggers system install dialog).
        if macos_has_clt() {
            candidates.push("/usr/bin/git");
            candidates.push("git");
        }
    }
    #[cfg(target_os = "windows")]
    {
        candidates.push("git");
        candidates.push(r"C:\Program Files\Git\cmd\git.exe");
        candidates.push(r"C:\Program Files (x86)\Git\cmd\git.exe");
    }
    #[cfg(all(not(target_os = "macos"), not(target_os = "windows")))]
    {
        candidates.push("git");
        candidates.push("/usr/bin/git");
        candidates.push("/usr/local/bin/git");
    }

    for c in candidates {
        if let Some(ver) = probe_git_version(c) {
            return Some((c.to_string(), ver));
        }
    }
    None
}

fn repo_name_from_url(url: &str) -> String {
    let trimmed = url.trim().trim_end_matches('/').trim_end_matches('\\');
    let last = trimmed
        .rsplit(['/', '\\', ':'])
        .next()
        .unwrap_or("")
        .strip_suffix(".git")
        .unwrap_or_else(|| trimmed.rsplit(['/', '\\', ':']).next().unwrap_or(""));
    let clean: String = last
        .trim()
        .chars()
        .map(|c| {
            if matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    let trimmed = clean.trim().trim_end_matches(['.', ' ']);
    if trimmed.is_empty() || folder::check_name(trimmed).is_err() {
        "remote-repo".to_string()
    } else {
        trimmed.to_string()
    }
}

fn default_remote_dir(data_dir: &Path, name: &str) -> String {
    let root = data_dir.join("remotes");
    let mut cand = root.join(name);
    let mut n = 2usize;
    while cand.exists() {
        cand = root.join(format!("{}-{}", name, n));
        n += 1;
    }
    cand.to_string_lossy().into_owned()
}

fn git_stderr_or_stdout(out: &std::process::Output) -> String {
    let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
    if !err.is_empty() {
        err
    } else {
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }
}

fn git_path(p: &str) -> String {
    folder::strip_win_prefix(p)
}

fn git_stdout(git: &str, dir: &str, args: &[&str]) -> Option<String> {
    let dir = git_path(dir);
    let out = new_cmd(git).arg("-C").arg(&dir).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}

/// Inspect a local directory to see if it is the root of an existing Git repository.
/// Returns `Some((remote_url, branch))` when `canon_dir` is a Git work-tree root
/// (`remote_url` is empty if no remote is configured).
fn detect_local_git_repo(git: &str, canon_dir: &str) -> Option<(String, String)> {
    let top = git_stdout(git, canon_dir, &["rev-parse", "--show-toplevel"])?;
    let top_canon = folder::canonical(&top).ok()?;
    if !super::same_path(&top_canon, canon_dir) {
        return None;
    }
    let branch = git_stdout(git, canon_dir, &["symbolic-ref", "--short", "HEAD"]).unwrap_or_default();
    let remote_name = if !branch.is_empty() {
        git_stdout(git, canon_dir, &["config", "--get", &format!("branch.{}.remote", branch)])
    } else {
        None
    };
    let remote_url = remote_name
        .as_deref()
        .and_then(|r| git_stdout(git, canon_dir, &["remote", "get-url", r]))
        .or_else(|| git_stdout(git, canon_dir, &["remote", "get-url", "origin"]))
        .or_else(|| {
            let remotes = git_stdout(git, canon_dir, &["remote"])?;
            let first = remotes.lines().next()?.trim();
            if first.is_empty() {
                None
            } else {
                git_stdout(git, canon_dir, &["remote", "get-url", first])
            }
        })
        .unwrap_or_default();
    Some((remote_url, branch))
}

fn normalize_git_url(u: &str) -> String {
    let s = u.trim().trim_end_matches(['/', '\\']);
    let s = if s.len() > 4 && s[s.len() - 4..].eq_ignore_ascii_case(".git") {
        &s[..s.len() - 4]
    } else {
        s
    };
    s.trim_end_matches(['/', '\\']).to_ascii_lowercase()
}

fn same_git_url(a: &str, b: &str) -> bool {
    normalize_git_url(a) == normalize_git_url(b)
}

pub(super) fn validate_relinked_repo(state: &AppState, path: &str, remote: &str) -> Result<(), String> {
    let (git, _) = resolve_git(&state.config_dir).ok_or("找不到 Git，无法验证新的仓库位置")?;
    match detect_local_git_repo(&git, path) {
        Some((url, _)) if same_git_url(&url, remote) => Ok(()),
        _ => Err("请选择同一远程仓库的本地克隆根目录".into()),
    }
}

fn is_shallow_repo(git: &str, dir: &str) -> bool {
    git_stdout(git, dir, &["rev-parse", "--is-shallow-repository"])
        .map(|s| s == "true")
        .unwrap_or(false)
}

fn is_dirty(git: &str, dir: &str) -> Result<bool, String> {
    let dir = git_path(dir);
    let out = new_cmd(git)
        .args(["-C", &dir, "status", "--porcelain"])
        .output()
        .map_err(|e| format!("无法执行 git status：{}", e))?;
    if !out.status.success() {
        return Err(git_stderr_or_stdout(&out));
    }
    Ok(!String::from_utf8_lossy(&out.stdout).trim().is_empty())
}

fn rev_parse(git: &str, dir: &str, rev: &str) -> Option<String> {
    let dir = git_path(dir);
    let out = new_cmd(git).args(["-C", &dir, "rev-parse", rev]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() { None } else { Some(s) }
}

#[derive(Serialize)]
pub struct GitCheckDto {
    available: bool,
    path: String,
    version: String,
}

/// Check whether `git` is available (optionally validating and saving a user-supplied executable path).
#[tauri::command]
pub async fn git_check(custom_path: Option<String>, state: State<'_, AppState>) -> Result<GitCheckDto, String> {
    let config_dir = state.config_dir.clone();
    tauri::async_runtime::spawn_blocking(move || {
        if let Some(p) = custom_path.map(|s| s.trim().trim_matches('"').to_string()).filter(|s| !s.is_empty()) {
            let ver = probe_git_version(&p).ok_or_else(|| format!("指定的路径不是有效的 git 可执行文件：{}", p))?;
            save_git_config(&config_dir, &GitConfig { git_path: p.clone() })?;
            return Ok(GitCheckDto { available: true, path: p, version: ver });
        }
        match resolve_git(&config_dir) {
            Some((path, version)) => Ok(GitCheckDto { available: true, path, version }),
            None => Ok(GitCheckDto { available: false, path: String::new(), version: String::new() }),
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Serialize)]
pub struct GitDefaultDirDto {
    title: String,
    dest: String,
}

/// Suggest a default library title and an unused local clone directory for a Git URL.
#[tauri::command]
pub async fn git_default_dir(url: String, state: State<'_, AppState>) -> Result<GitDefaultDirDto, String> {
    let title = repo_name_from_url(&url);
    let dest = default_remote_dir(&state.data_dir, &title);
    Ok(GitDefaultDirDto { title, dest })
}

#[derive(Serialize)]
pub struct GitInspectDirDto {
    path: String,
    exists: bool,
    is_empty: bool,
    is_git_repo: bool,
    remote_url: String,
    branch: String,
    title: String,
    conflict: Option<String>,
}

/// Inspect a user-chosen local directory for the Remote Git dialog:
/// distinguishes an empty/new directory (to clone into) from an existing local Git clone (to bind directly).
#[tauri::command]
pub async fn git_inspect_dir(path: String, state: State<'_, AppState>) -> Result<GitInspectDirDto, String> {
    let trimmed = path.trim().trim_matches('"').to_string();
    if trimmed.is_empty() {
        return Err("请指定本地存放目录".into());
    }
    let p = PathBuf::from(&trimmed);
    if !p.exists() {
        return Ok(GitInspectDirDto {
            path: folder::strip_win_prefix(&trimmed),
            exists: false,
            is_empty: true,
            is_git_repo: false,
            remote_url: String::new(),
            branch: String::new(),
            title: String::new(),
            conflict: None,
        });
    }
    if !p.is_dir() {
        return Err(format!("目标路径不是文件夹：{}", trimmed));
    }
    let canon = folder::canonical(&trimmed)?;
    let conflict = {
        let libs = state.libraries.read().unwrap();
        let existing: Vec<&str> = libs.iter().filter(|l| l.kind == "folder").map(|l| l.source.as_str()).collect();
        folder::conflict(&existing, &canon)
    };
    let is_empty = std::fs::read_dir(&canon)
        .map_err(|e| format!("{}: {}", canon, e))?
        .next()
        .is_none();
    if is_empty {
        return Ok(GitInspectDirDto {
            path: canon,
            exists: true,
            is_empty: true,
            is_git_repo: false,
            remote_url: String::new(),
            branch: String::new(),
            title: String::new(),
            conflict,
        });
    }
    let config_dir = state.config_dir.clone();
    let canon_clone = canon.clone();
    let detected = tauri::async_runtime::spawn_blocking(move || {
        let (git_bin, _) = resolve_git(&config_dir)?;
        detect_local_git_repo(&git_bin, &canon_clone)
    })
    .await
    .map_err(|e| e.to_string())?;

    let (is_git_repo, remote_url, branch, title) = match detected {
        Some((remote_url, branch)) => {
            let title = if !remote_url.is_empty() {
                repo_name_from_url(&remote_url)
            } else {
                Path::new(&canon)
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default()
            };
            (true, remote_url, branch, title)
        }
        None => (false, String::new(), String::new(), String::new()),
    };
    Ok(GitInspectDirDto {
        path: canon,
        exists: true,
        is_empty: false,
        is_git_repo,
        remote_url,
        branch,
        title,
        conflict,
    })
}

fn parse_git_progress(line: &str) -> Option<(String, usize, usize)> {
    let s = line.trim().strip_prefix("remote:").map(str::trim).unwrap_or(line.trim());
    let (phase_raw, rest) = s.split_once(':')?;
    let phase = match phase_raw.trim() {
        "Enumerating objects" => "正在枚举对象",
        "Counting objects" => "正在统计对象",
        "Compressing objects" => "正在压缩对象",
        "Receiving objects" => "正在下载对象",
        "Resolving deltas" => "正在解析增量",
        "Updating files" | "Checking out files" => "正在检出文件",
        _ => return None,
    };
    let lp = rest.find('(')?;
    let rp = rest[lp + 1..].find(')')? + lp + 1;
    let (done_s, total_s) = rest[lp + 1..rp].split_once('/')?;
    let done = done_s.trim().parse::<usize>().ok()?;
    let total = total_s.trim().parse::<usize>().ok()?;
    if total == 0 {
        return None;
    }
    let extra = rest[rp + 1..]
        .trim_start_matches([',', ' '])
        .trim_end_matches(", done.")
        .trim_end_matches("done.")
        .trim();
    let label = if !extra.is_empty() && extra.contains('B') {
        format!("{}（{}/{} · {}）", phase, done, total, extra)
    } else {
        format!("{}（{}/{}）", phase, done, total)
    };
    Some((label, done, total))
}

/// Clone a remote Git repository (shallow `--depth 1`) — or bind an already-cloned local
/// Git repository directly — as a read-only folder library.
#[tauri::command]
pub async fn add_git_library(
    url: String,
    dest: String,
    title: Option<String>,
    branch: Option<String>,
    exts: String,
    max_mb: u64,
    library_id: Option<String>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<StatusDto, String> {
    let url = url.trim().to_string();
    let exts = FolderFilter::parse_exts(&exts);
    if exts.is_empty() {
        return Err("请至少填写一种文件后缀".into());
    }
    let (git_bin, _) = resolve_git(&state.config_dir).ok_or("未检测到可用的 git 命令，请先指定本机 git 位置")?;

    let dest_trimmed = dest.trim().trim_matches('"').to_string();
    if dest_trimmed.is_empty() {
        return Err("请指定本地存放目录".into());
    }
    let dest_path = PathBuf::from(&dest_trimmed);
    let (created, is_non_empty) = if dest_path.exists() {
        if !dest_path.is_dir() {
            return Err(format!("目标路径不是文件夹：{}", dest_trimmed));
        }
        let mut iter = std::fs::read_dir(&dest_path).map_err(|e| format!("{}: {}", dest_trimmed, e))?;
        (false, iter.next().is_some())
    } else {
        if url.is_empty() {
            return Err("请填写 Git 仓库地址".into());
        }
        std::fs::create_dir_all(&dest_path).map_err(|e| format!("无法创建目标文件夹 {}：{}", dest_trimmed, e))?;
        (true, false)
    };

    let canon = match folder::canonical(&dest_trimmed) {
        Ok(c) => c,
        Err(e) => {
            if created {
                let _ = std::fs::remove_dir(&dest_path);
            }
            return Err(e);
        }
    };

    // Validate folder conflict and library_id before cloning or binding.
    let check_res: Result<(), String> = {
        let libs = state.libraries.read().unwrap();
        let existing: Vec<&str> = libs.iter().filter(|l| l.kind == "folder").map(|l| l.source.as_str()).collect();
        if let Some(c) = folder::conflict(&existing, &canon) {
            Err(c)
        } else {
            super::library_id(library_id.as_deref(), "remote", &libs).map(|_| ())
        }
    };
    if let Err(e) = check_res {
        if created {
            let _ = std::fs::remove_dir(&canon);
        }
        return Err(e);
    }

    let effective_url = if is_non_empty {
        // Reuse an already-cloned local Git repository if it has a configured remote.
        let detected = tauri::async_runtime::spawn_blocking({
            let git_bin = git_bin.clone();
            let canon = canon.clone();
            move || detect_local_git_repo(&git_bin, &canon)
        })
        .await
        .map_err(|e| e.to_string())?;

        let Some((detected_url, _)) = detected else {
            return Err("目标文件夹不为空且不是 Git 仓库根目录，请选择空目录或已克隆的 Git 仓库".into());
        };
        if detected_url.is_empty() {
            return Err("该 Git 仓库未配置远程地址（remote），无法绑定为远程知识库，请改用「本地文件夹」添加".into());
        }
        if !url.is_empty() && !same_git_url(&url, &detected_url) {
            return Err(format!(
                "目标文件夹已是另一个 Git 仓库（远程地址：{}），与填写的仓库地址不一致。如需直接绑定此仓库，请清空或更正仓库地址。",
                detected_url
            ));
        }
        detected_url
    } else {
        if url.is_empty() {
            if created {
                let _ = std::fs::remove_dir(&canon);
            }
            return Err("请填写 Git 仓库地址".into());
        }
        let branch = branch.map(|b| b.trim().to_string()).filter(|b| !b.is_empty());
        let clone_res = tauri::async_runtime::spawn_blocking({
            let url = url.clone();
            let canon = canon.clone();
            let git_bin = git_bin.clone();
            let app = app.clone();
            move || {
                let clone_dest = git_path(&canon);
                super::bind_progress(&app, "正在连接远程仓库…", 0, 0);
                let mut cmd = new_cmd(&git_bin);
                cmd.args(["clone", "--depth", "1", "--progress"]);
                if let Some(b) = &branch {
                    cmd.args(["-b", b]);
                }
                cmd.arg(&url)
                    .arg(&clone_dest)
                    .stdout(Stdio::null())
                    .stderr(Stdio::piped());
                let mut child = cmd.spawn().map_err(|e| format!("执行 git clone 失败：{}", e))?;
                let mut err_lines: Vec<String> = Vec::new();
                if let Some(mut stderr) = child.stderr.take() {
                    let mut buf = [0u8; 4096];
                    let mut line_buf: Vec<u8> = Vec::new();
                    while let Ok(n) = stderr.read(&mut buf) {
                        if n == 0 {
                            break;
                        }
                        for &b in &buf[..n] {
                            if b == b'\r' || b == b'\n' {
                                if !line_buf.is_empty() {
                                    let line = String::from_utf8_lossy(&line_buf).trim().to_string();
                                    line_buf.clear();
                                    if let Some((label, done, total)) = parse_git_progress(&line) {
                                        super::bind_progress(&app, &label, done, total);
                                    } else if !line.is_empty() && !line.starts_with("Cloning into") {
                                        err_lines.push(line);
                                    }
                                }
                            } else {
                                line_buf.push(b);
                            }
                        }
                    }
                    if !line_buf.is_empty() {
                        let line = String::from_utf8_lossy(&line_buf).trim().to_string();
                        if let Some((label, done, total)) = parse_git_progress(&line) {
                            super::bind_progress(&app, &label, done, total);
                        } else if !line.is_empty() && !line.starts_with("Cloning into") {
                            err_lines.push(line);
                        }
                    }
                }
                let status = child.wait().map_err(|e| format!("等待 git clone 失败：{}", e))?;
                if !status.success() {
                    let detail = err_lines.join("\n");
                    return Err(if detail.is_empty() {
                        "git clone 失败".into()
                    } else {
                        format!("git clone 失败：{}", detail)
                    });
                }
                Ok::<_, String>(())
            }
        })
        .await
        .map_err(|e| e.to_string())?;

        if let Err(e) = clone_res {
            if created {
                let _ = std::fs::remove_dir_all(&canon);
            }
            return Err(e);
        }
        url
    };

    let fallback_title = repo_name_from_url(&effective_url);
    let lib_title = title.filter(|t| !t.trim().is_empty()).map(|t| t.trim().to_string()).unwrap_or(fallback_title);
    {
        let mut libs = state.libraries.write().unwrap();
        let existing: Vec<&str> = libs.iter().filter(|l| l.kind == "folder").map(|l| l.source.as_str()).collect();
        if let Some(c) = folder::conflict(&existing, &canon) {
            if created {
                let _ = std::fs::remove_dir_all(&canon);
            }
            return Err(c);
        }
        let id = super::library_id(library_id.as_deref(), "remote", &libs)?;
        libs.push(Library {
            id,
            kind: "folder".into(),
            title: lib_title,
            source: canon,
            tree: String::new(),
            exts,
            max_mb,
            writable: false,
            remote: effective_url,
        });
    }
    state.save_registry()?;
    Ok(status_of(&state))
}

#[derive(Serialize)]
pub struct GitRepoStatus {
    lib: String,
    dirty: bool,
    has_update: bool,
    error: Option<String>,
}

/// Check local dirty state (and optionally fetch remote) for all remote git libraries.
#[tauri::command]
pub async fn git_status_all(fetch: Option<bool>, state: State<'_, AppState>) -> Result<Vec<GitRepoStatus>, String> {
    let remotes: Vec<(String, String)> = state
        .libraries
        .read()
        .unwrap()
        .iter()
        .filter(|l| l.kind == "folder" && !l.remote.is_empty() && Path::new(&l.source).is_dir())
        .map(|l| (l.id.clone(), l.source.clone()))
        .collect();
    if remotes.is_empty() {
        return Ok(Vec::new());
    }
    let config_dir = state.config_dir.clone();
    let do_fetch = fetch.unwrap_or(false);
    tauri::async_runtime::spawn_blocking(move || {
        let Some((git_bin, _)) = resolve_git(&config_dir) else {
            return Ok(Vec::new());
        };
        let mut out = Vec::with_capacity(remotes.len());
        for (lib, source) in remotes {
            let dirty = match is_dirty(&git_bin, &source) {
                Ok(d) => d,
                Err(e) => {
                    out.push(GitRepoStatus { lib, dirty: false, has_update: false, error: Some(e) });
                    continue;
                }
            };
            let mut has_update = false;
            let mut err = None;
            if do_fetch {
                let repo_dir = git_path(&source);
                let mut cmd = new_cmd(&git_bin);
                cmd.args(["-C", &repo_dir, "fetch"]);
                if is_shallow_repo(&git_bin, &source) {
                    cmd.args(["--depth", "1"]);
                }
                let f = cmd.output();
                match f {
                    Ok(o) if o.status.success() => {
                        let head = rev_parse(&git_bin, &source, "HEAD");
                        let upstream = rev_parse(&git_bin, &source, "@{u}");
                        if let (Some(h), Some(u)) = (head, upstream) {
                            has_update = h != u;
                        }
                    }
                    Ok(o) => {
                        err = Some(git_stderr_or_stdout(&o));
                    }
                    Err(e) => {
                        err = Some(e.to_string());
                    }
                }
            } else {
                // Cheap local comparison against last fetched upstream ref.
                let head = rev_parse(&git_bin, &source, "HEAD");
                let upstream = rev_parse(&git_bin, &source, "@{u}");
                if let (Some(h), Some(u)) = (head, upstream) {
                    has_update = h != u;
                }
            }
            out.push(GitRepoStatus { lib, dirty, has_update, error: err });
        }
        Ok(out)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Serialize)]
pub struct GitPullResult {
    updated: bool,
    title: String,
    message: String,
}

/// Update a remote git library via `git pull --ff-only`.
/// Refuses to pull if the local working tree has uncommitted changes to avoid conflicts.
#[tauri::command]
pub async fn git_pull_library(id: String, state: State<'_, AppState>) -> Result<GitPullResult, String> {
    let lib = state.library(&id)?;
    if lib.kind != "folder" || lib.remote.is_empty() {
        return Err("这不是远程 Git 知识库".into());
    }
    if state.building.lock().unwrap().contains(&id) {
        return Err("正在建立这个知识库的索引，请稍后再更新".into());
    }
    let config_dir = state.config_dir.clone();
    let source = lib.source.clone();
    let title = lib.title.clone();

    let updated = tauri::async_runtime::spawn_blocking(move || {
        let (git_bin, _) = resolve_git(&config_dir).ok_or("未检测到可用的 git 命令，请在添加远程知识库中重新指定 git 位置")?;
        if !Path::new(&source).is_dir() {
            return Err(format!("本地文件夹不存在：{}", source));
        }
        if is_dirty(&git_bin, &source)? {
            return Err("本地文件夹存在未提交的修改，为避免冲突已中止更新。请打开本地文件夹手动处理后再更新。".into());
        }
        let before = rev_parse(&git_bin, &source, "HEAD");
        let repo_dir = git_path(&source);
        let out = new_cmd(&git_bin)
            .args(["-C", &repo_dir, "pull", "--ff-only"])
            .output()
            .map_err(|e| format!("执行 git pull 失败：{}", e))?;
        if !out.status.success() {
            let detail = git_stderr_or_stdout(&out);
            return Err(if detail.is_empty() {
                "更新失败，请打开本地文件夹手动处理后再试。".into()
            } else {
                format!("更新失败：{}", detail)
            });
        }
        let after = rev_parse(&git_bin, &source, "HEAD");
        Ok::<bool, String>(before != after)
    })
    .await
    .map_err(|e| e.to_string())??;

    if updated {
        state.folders.write().unwrap().remove(&id);
        state.titles.write().unwrap().remove(&id);
    }
    Ok(GitPullResult {
        updated,
        title: title.clone(),
        message: if updated {
            format!("已更新「{}」", title)
        } else {
            format!("「{}」已是最新版本", title)
        },
    })
}
