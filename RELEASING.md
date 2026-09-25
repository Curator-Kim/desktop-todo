# 发布 Windows 更新

自动更新使用 Tauri 官方更新器。安装包必须由同一私钥签名；`src-tauri/tauri.conf.json` 中的公钥用于客户端校验。私钥位于本机 `.private/updater.key`，已被 Git 忽略。**请单独安全备份私钥；丢失后，已安装的客户端无法验证将来的更新。不要把私钥上传到 GitHub。**

1. 同步修改 `package.json`、`package-lock.json`、`src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json`、`updates.json` 的版本号，然后构建：

   ```powershell
   $env:TAURI_SIGNING_PRIVATE_KEY = (Resolve-Path '.private/updater.key').Path
   npm run tauri -- build --ci
   npm run release:prepare
   ```

2. 把源代码和版本标签推送到 GitHub。创建同名 Release 草稿，上传 `.private/release/vX.Y.Z/` 里的安装版、便携版和 `latest.json`。确认文件全部上传后再发布 Release；`latest.json` 必须与本次安装包的签名匹配。

3. 安装版客户端会从最新 Release 的 `latest.json` 检查更新。便携版应手动替换可执行文件；v0.1.4 及更早版本也需要先手动安装新版本。
