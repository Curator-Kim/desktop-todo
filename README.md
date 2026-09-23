# 桌面待办

一款轻量的 Windows 托盘待办便签。主便签只显示标题和截止时间，任务详情使用即时富文本编辑，支持图片、提醒和完成记录。

主便签默认固定在桌面位置，并以轻微透明效果融入桌面。需要移动或缩放时，在设置中点击“调整便签位置和大小”，拖动顶部或窗口边缘，最后点击“完成”；下次启动会恢复位置和大小。

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
