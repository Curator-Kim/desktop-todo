import { useCallback, useEffect, useMemo, useState } from 'react';
import { emit, listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { disable, enable, isEnabled } from '@tauri-apps/plugin-autostart';
import { History, ImagePlus, Moon, RefreshCw, Sun, X } from 'lucide-react';
import appPackage from '../../package.json';
import { clearWindowLayout, defaultSettings, loadSettings, loadTasks, saveSettings, saveTasks } from '../store';
import type { AppSettings, Task } from '../types';

type UpdateStatus =
  | { kind: 'idle' | 'checking' | 'error' }
  | { kind: 'latest' | 'available'; version: string; url: string };

function compareVersions(left: string, right: string) {
  const parts = (version: string) => version.replace(/^v/, '').split('.').map(Number);
  const a = parts(left);
  const b = parts(right);
  for (let i = 0; i < Math.max(a.length, b.length); i += 1) {
    if ((a[i] ?? 0) !== (b[i] ?? 0)) return (a[i] ?? 0) - (b[i] ?? 0);
  }
  return 0;
}

async function imageAsBackground(file: File) {
  const bitmap = await createImageBitmap(file);
  try {
    const scale = Math.min(1, 1280 / Math.max(bitmap.width, bitmap.height));
    const canvas = document.createElement('canvas');
    canvas.width = Math.max(1, Math.round(bitmap.width * scale));
    canvas.height = Math.max(1, Math.round(bitmap.height * scale));
    const context = canvas.getContext('2d');
    if (!context) throw new Error('Canvas is unavailable');
    context.drawImage(bitmap, 0, 0, canvas.width, canvas.height);
    const dataUrl = canvas.toDataURL('image/jpeg', 0.78);
    if (dataUrl.length > 2_500_000) throw new Error('Background image is too large');
    return dataUrl;
  } finally {
    bitmap.close();
  }
}

export function SettingsWindow() {
  const [tasks, setTasks] = useState<Task[]>(loadTasks);
  const [settings, setSettings] = useState<AppSettings>(loadSettings);
  const [settingsError, setSettingsError] = useState('');
  const [resetMessage, setResetMessage] = useState('');
  const [updateStatus, setUpdateStatus] = useState<UpdateStatus>({ kind: 'idle' });
  const [linkCopied, setLinkCopied] = useState(false);
  const completedTasks = useMemo(() => tasks.filter((task) => task.completedAt).sort((a, b) => +new Date(b.completedAt!) - +new Date(a.completedAt!)), [tasks]);
  const refresh = useCallback(() => setTasks(loadTasks()), []);
  useEffect(() => {
    const unlisten = listen('tasks-changed', refresh);
    window.addEventListener('storage', refresh);
    return () => { void unlisten.then((fn) => fn()); window.removeEventListener('storage', refresh); };
  }, [refresh]);
  useEffect(() => { document.documentElement.dataset.theme = settings.theme; }, [settings.theme]);
  const restoreTask = (id: string) => {
    const next = loadTasks().map((task) => task.id === id ? { ...task, completedAt: undefined } : task);
    saveTasks(next); setTasks(next); void emit('tasks-changed');
  };
  const deleteTask = (id: string) => {
    const next = loadTasks().filter((task) => task.id !== id);
    saveTasks(next); setTasks(next); void emit('tasks-changed');
  };
  const updateSettings = async (patch: Partial<AppSettings>) => {
    const next = { ...settings, ...patch };
    try {
      setSettingsError('');
      saveSettings(next);
      setSettings(next);
      void emit('settings-changed');
      if ('autostart' in patch) {
        if (patch.autostart) await enable(); else await disable();
        next.autostart = await isEnabled();
        setSettings({ ...next });
        saveSettings(next);
        void emit('settings-changed');
      }
    } catch (error) {
      console.error('无法保存设置', error);
      setSettingsError('设置保存失败。背景图片可能占用过多空间，请换一张图片。');
    }
  };

  const chooseBackground = async (file?: File) => {
    if (!file) return;
    try {
      setSettingsError('');
      const backgroundImage = await imageAsBackground(file);
      await updateSettings({ backgroundImage });
    } catch (error) {
      console.error('无法读取背景图片', error);
      setSettingsError('无法使用这张图片，请选择 PNG、JPG 或 WebP 图片。');
    }
  };

  const checkForUpdates = async () => {
    setLinkCopied(false);
    setUpdateStatus({ kind: 'checking' });
    const request = async (url: string, headers?: HeadersInit) => {
      const controller = new AbortController();
      const timeout = window.setTimeout(() => controller.abort(), 8_000);
      try {
        return await fetch(url, { headers, cache: 'no-store', signal: controller.signal });
      } finally {
        window.clearTimeout(timeout);
      }
    };
    try {
      let version: string | undefined;
      try {
        const response = await request('https://api.github.com/repos/Curator-Kim/desktop-todo/releases/latest', { Accept: 'application/vnd.github+json' });
        if (!response.ok) throw new Error(`GitHub returned ${response.status}`);
        const release = await response.json() as { tag_name?: string };
        version = release.tag_name?.replace(/^v/, '');
      } catch {
        const response = await request('https://raw.githubusercontent.com/Curator-Kim/desktop-todo/main/updates.json');
        if (!response.ok) throw new Error(`Version file returned ${response.status}`);
        const fallback = await response.json() as { version?: string };
        version = fallback.version;
      }
      if (!version || !/^\d+\.\d+\.\d+$/.test(version)) throw new Error('Invalid release version');
      const url = `https://github.com/Curator-Kim/desktop-todo/releases/tag/v${version}`;
      setUpdateStatus({ kind: compareVersions(version, appPackage.version) > 0 ? 'available' : 'latest', version, url });
    } catch (error) {
      console.error('无法检查更新', error);
      setUpdateStatus({ kind: 'error' });
    }
  };

  const resetAllSettings = async () => {
    setSettingsError('');
    setResetMessage('');
    try {
      await disable();
      const defaults = defaultSettings();
      defaults.autostart = await isEnabled();
      saveSettings(defaults);
      setSettings(defaults);
      clearWindowLayout();
      await emit('settings-changed');
      await emit('reset-window-layout');
      setResetMessage('已恢复默认设置，任务和完成记录均已保留。');
    } catch (error) {
      console.error('无法恢复默认设置', error);
      setSettingsError('恢复默认设置失败，请重试。');
    }
  };
  return (
    <main className="settings-shell">
      <section className="settings-panel">
            <header><div><h2>设置</h2><p>桌面便签与程序偏好</p></div><button className="icon-button" title="关闭设置" onClick={() => void getCurrentWindow().hide()}><X size={19} /></button></header>
            <div className="setting-row">
              <div><strong>外观</strong><span>选择便签的颜色模式</span></div>
              <div className="segmented">
                <button className={settings.theme === 'light' ? 'active' : ''} onClick={() => updateSettings({ theme: 'light' })}><Sun size={15} />浅色</button>
                <button className={settings.theme === 'dark' ? 'active' : ''} onClick={() => updateSettings({ theme: 'dark' })}><Moon size={15} />深色</button>
                <button className={settings.theme === 'system' ? 'active' : ''} onClick={() => updateSettings({ theme: 'system' })}>系统</button>
              </div>
            </div>
            <div className="setting-row">
              <div><strong>背景颜色</strong><span>选择便签底色</span></div>
              <input className="color-picker" type="color" aria-label="背景颜色" value={settings.backgroundColor || '#fafaf8'} onChange={(event) => void updateSettings({ backgroundColor: event.target.value })} />
            </div>
            <div className="setting-row">
              <div><strong>背景图片</strong><span>{settings.backgroundImage ? '已选择图片' : '可选一张本地图片'}</span></div>
              <div className="setting-actions">
                <label className="secondary-button image-picker"><ImagePlus size={15} />选择图片<input type="file" accept="image/png,image/jpeg,image/webp" onChange={(event) => { void chooseBackground(event.target.files?.[0]); event.target.value = ''; }} /></label>
                {settings.backgroundImage && <button className="secondary-button" onClick={() => void updateSettings({ backgroundImage: '' })}>移除</button>}
              </div>
            </div>
            <div className="setting-row">
              <div><strong>透明度</strong><span>背景越透明，桌面壁纸越明显</span></div>
              <div className="opacity-control"><input type="range" min="0" max="70" step="5" value={settings.transparency} aria-label="透明度" onChange={(event) => void updateSettings({ transparency: Number(event.target.value) })} /><span>{settings.transparency}%</span></div>
            </div>
            {(settings.backgroundColor || settings.backgroundImage) && <button className="reset-background" onClick={() => void updateSettings({ backgroundColor: '', backgroundImage: '' })}>恢复默认背景</button>}
            {settingsError && <p className="setting-error" role="alert">{settingsError}</p>}
            <div className="setting-row">
              <div><strong>开机自动启动</strong><span>启动后安静地驻留在系统托盘</span></div>
              <label className="switch"><input type="checkbox" checked={settings.autostart} onChange={(e) => updateSettings({ autostart: e.target.checked })} /><i /></label>
            </div>
            <div className="setting-row">
              <div><strong>移动与缩放</strong><span>随时拖动空白处移动，拖动边缘调整大小</span></div>
            </div>
            <div className="setting-row">
              <div><strong>检查更新</strong><span>当前版本 v{appPackage.version}</span></div>
              <button className="secondary-button" disabled={updateStatus.kind === 'checking'} onClick={() => void checkForUpdates()}><RefreshCw size={15} />{updateStatus.kind === 'checking' ? '检查中…' : '检测升级'}</button>
            </div>
            {updateStatus.kind === 'available' && <div className="update-result"><span>发现新版本 v{updateStatus.version}</span><input aria-label="新版下载地址" readOnly value={updateStatus.url} onFocus={(event) => event.target.select()} /><button onClick={() => void navigator.clipboard.writeText(updateStatus.url).then(() => setLinkCopied(true)).catch(() => setSettingsError('复制失败，请选中下载地址手动复制。'))}>{linkCopied ? '已复制' : '复制下载链接'}</button></div>}
            {updateStatus.kind === 'latest' && <p className="update-result">已是最新版本 v{updateStatus.version}</p>}
            {updateStatus.kind === 'error' && <p className="setting-error" role="alert">检查失败，请确认网络连接后重试。</p>}
            <div className="setting-row"><div><strong>恢复默认设置</strong><span>重置外观、开机启动及便签位置和大小；保留所有任务</span></div><button className="secondary-button" onClick={() => void resetAllSettings()}>恢复默认设置</button></div>
            {resetMessage && <p className="update-result" role="status">{resetMessage}</p>}
            <div className="history-block">
              <h3><History size={16} />已完成任务 <small>{completedTasks.length}</small></h3>
              {completedTasks.length === 0 ? <p className="muted-center">还没有已完成的任务</p> : completedTasks.map((task) => (
                <div className="history-item" key={task.id}>
                  <div><strong>{task.title}</strong><span>{task.completedAt ? new Date(task.completedAt).toLocaleString('zh-CN') : ''}</span></div>
                  <button onClick={() => restoreTask(task.id)}>恢复</button>
                  <button className="danger-text" onClick={() => deleteTask(task.id)}>删除</button>
                </div>
              ))}
            </div>
      </section>
    </main>
  );
}
