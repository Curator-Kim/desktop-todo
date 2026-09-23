# 桌面待办

一款轻量的 Windows 托盘待办便签。主便签只显示标题和截止时间，任务详情使用即时富文本编辑，支持图片、提醒和完成记录。

主便签可以随时拖动非按钮区域来移动，拖动窗口边缘缩放；下次启动会恢复位置和大小。设置面板支持自选背景颜色或本地图片、调整透明度，以及手动检测 GitHub 新版本并复制下载链接。

## 开发

```powershell
npm install
npm run desktop:dev
```

## 打包

```powershell
npm run desktop:build
```

安装包会生成在 `src-tauri/target/release/bundle`。

发布新版本时同步更新根目录的 `updates.json`；程序在 GitHub API 限流时会用它检测版本。
