# Windows 自动更新与发布

自动更新使用 Tauri updater 和 GitHub Releases。应用启动时检查一次；「关于 → 检查更新」可手动检查。发现新版后由用户选择下载，下载并验签成功后再选择安装；安装会关闭并重启应用，应先保存所有窗口中的编辑内容。发布包是 Windows x64 NSIS 安装版，独立复制的 exe 不属于自动更新支持范围。

## 首次配置

1. 将代码推送到公开的 GitHub 仓库 `smart-library`。工作流从实际仓库自动生成更新地址，仓库改名或迁移后，已发布的旧版仍会使用原地址。私有仓库的 Release 下载需要另外设计认证，当前流程不支持。
2. 在安全的本机位置生成 Tauri 更新签名密钥，例如在 `app` 目录运行 `.\node_modules\.bin\tauri.cmd signer generate -w <私钥路径>`。不要把私钥放入仓库或发送给他人，妥善备份私钥和密码；丢失后旧版客户端无法验证新密钥签名的更新。公开密钥也要稳定，不能随意更换。
3. 在 GitHub 仓库 Settings → Secrets and variables → Actions 中设置：变量 `TAURI_UPDATER_PUBLIC_KEY` 为生成的 `.pub` 文件完整内容；密钥 `TAURI_SIGNING_PRIVATE_KEY` 为私钥文件完整内容；密钥 `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` 为生成时使用的密码（无密码时留空）。更新签名与 Windows 代码签名是两回事；未配置 Windows 代码签名时系统可能提示发行者未知。
4. 确认 GitHub Actions 有创建 Release 的权限。工作流会构建并创建草稿 Release，上传安装程序、签名和 `latest.json`。审查安装包、更新说明和附件后手动发布草稿。

## 以后发布

将 `app/src-tauri/tauri.conf.json`、`app/src-tauri/Cargo.toml`、`app/package.json` 中版本改为一致且比上一次更高的 SemVer（例如 `26.11.9`），更新 `Cargo.lock` 和 `app/package-lock.json`，提交后推送对应标签 `v26.11.9`。工作流会校验标签与 Tauri 版本一致。不要发布不含 `latest.json` 的普通正式 Release，否则 GitHub 的 latest 地址可能指向它。

首次安装要从 Release 下载并运行安装程序；已经发布、没有内置该更新功能或使用不同签名公钥的版本不能凭空获得自动更新。当前程序的数据保存在程序目录旁的 `data` 文件夹：请在正式发布前，用测试数据和两个连续版本实际测试升级，确认数据、设置和笔记都保留。更新不会自动把已有便携版的数据迁移到新安装位置，迁移前应先备份。

本地普通开发构建默认未配置更新地址和公钥；通过 GitHub Actions 发布时工作流将其注入构建配置，不会把私钥写进源代码。
