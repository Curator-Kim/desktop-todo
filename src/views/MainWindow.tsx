import { useCallback, useEffect, useMemo, useState } from 'react';
import { WebviewWindow } from '@tauri-apps/api/webviewWindow';
import { emit, listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { disable, enable, isEnabled } from '@tauri-apps/plugin-autostart';
import { isPermissionGranted, requestPermission, sendNotification } from '@tauri-apps/plugin-notification';
import { Check, Clock3, History, Maximize2, Moon, Plus, Settings, Sun, X } from 'lucide-react';
import { dueTone, formatDue, loadSettings, loadTasks, saveSettings, saveTasks } from '../store';
import type { AppSettings, Task } from '../types';

function openEditor(taskId?: string) {
  const label = taskId ? `task-${taskId}` : `task-new-${Date.now()}`;
  const query = taskId ? `&taskId=${encodeURIComponent(taskId)}` : '';
  const win = new WebviewWindow(label, {
    url: `/?view=editor${query}`,
    title: taskId ? '编辑任务' : '新建任务',
    width: 760,
    height: 720,
    minWidth: 560,
    minHeight: 560,
    center: true,
    resizable: true,
  });
  win.once('tauri://error', (error) => console.error(error));
}

export function MainWindow() {
  const [tasks, setTasks] = useState<Task[]>(loadTasks);
  const [settings, setSettings] = useState<AppSettings>(loadSettings);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [resizeMode, setResizeMode] = useState(false);
  const [now, setNow] = useState(Date.now());

  const refresh = useCallback(() => setTasks(loadTasks()), []);
  const activeTasks = useMemo(
    () => tasks.filter((task) => !task.completedAt).sort((a, b) => +new Date(a.dueAt) - +new Date(b.dueAt)),
    [tasks],
  );
  const completedTasks = useMemo(
    () => tasks.filter((task) => task.completedAt).sort((a, b) => +new Date(b.completedAt!) - +new Date(a.completedAt!)),
    [tasks],
  );

  useEffect(() => {
    const unlistenPromise = listen('tasks-changed', refresh);
    const showSettingsPromise = listen('open-settings', () => setSettingsOpen(true));
    window.addEventListener('storage', refresh);
    window.addEventListener('tasks-local-change', refresh);
    const clock = window.setInterval(() => setNow(Date.now()), 30_000);
    return () => {
      void unlistenPromise.then((fn) => fn());
      void showSettingsPromise.then((fn) => fn());
      window.removeEventListener('storage', refresh);
      window.removeEventListener('tasks-local-change', refresh);
      clearInterval(clock);
    };
  }, [refresh]);

  useEffect(() => {
    const root = document.documentElement;
    root.dataset.theme = settings.theme;
  }, [settings.theme]);

  useEffect(() => {
    const checkReminders = async () => {
      const current = loadTasks();
      let changed = false;
      for (const task of current) {
        if (task.completedAt || !task.reminderEnabled) continue;
        const reminderAt = +new Date(task.dueAt) - task.reminderMinutes * 60_000;
        const key = `${task.dueAt}:${task.reminderMinutes}`;
        if (Date.now() >= reminderAt && task.notifiedKey !== key) {
          let allowed = await isPermissionGranted();
          if (!allowed) allowed = (await requestPermission()) === 'granted';
          if (allowed) {
            sendNotification({ title: task.title, body: `截止时间：${formatDue(task.dueAt)}` });
            task.notifiedKey = key;
            changed = true;
          }
        }
      }
      if (changed) saveTasks(current);
    };
    void checkReminders();
    const timer = window.setInterval(checkReminders, 30_000);
    return () => clearInterval(timer);
  }, []);

  const completeTask = (id: string) => {
    const next = loadTasks().map((task) => task.id === id ? { ...task, completedAt: new Date().toISOString() } : task);
    saveTasks(next);
    setTasks(next);
  };

  const restoreTask = (id: string) => {
    const next = loadTasks().map((task) => task.id === id ? { ...task, completedAt: undefined } : task);
    saveTasks(next);
    setTasks(next);
  };

  const deleteTask = (id: string) => {
    const next = loadTasks().filter((task) => task.id !== id);
    saveTasks(next);
    setTasks(next);
  };

  const updateSettings = async (patch: Partial<AppSettings>) => {
    const next = { ...settings, ...patch };
    setSettings(next);
    saveSettings(next);
    if ('autostart' in patch) {
      if (patch.autostart) await enable(); else await disable();
      next.autostart = await isEnabled();
      setSettings({ ...next });
      saveSettings(next);
    }
  };

  const toggleResize = async () => {
    const next = !resizeMode;
    setResizeMode(next);
    await getCurrentWindow().setResizable(next);
  };

  return (
    <main className="sticky-shell">
      <header className="sticky-header" data-tauri-drag-region>
        <div className="brand" data-tauri-drag-region>
          <span className="brand-dot" />
          <span>待办</span>
          <small>{activeTasks.length}</small>
        </div>
        <div className="header-actions">
          <button className="icon-button" title="新建任务" onClick={() => openEditor()}><Plus size={19} /></button>
          <button className="icon-button" title="设置" onClick={() => setSettingsOpen(true)}><Settings size={18} /></button>
          <button className="icon-button" title="隐藏到托盘" onClick={() => getCurrentWindow().hide()}><X size={18} /></button>
        </div>
      </header>

      <section className="task-list" aria-label="待办任务">
        {activeTasks.length === 0 ? (
          <button className="empty-state" onClick={() => openEditor()}>
            <span className="empty-plus"><Plus size={24} /></span>
            <strong>暂时没有待办</strong>
            <span>点击创建第一项任务</span>
          </button>
        ) : activeTasks.map((task) => {
          const tone = dueTone(task.dueAt);
          const overdue = +new Date(task.dueAt) < now;
          return (
            <article className="task-card" key={task.id} style={{ background: tone.background, borderColor: tone.border }}>
              <button className="complete-button" title="完成任务" onClick={() => completeTask(task.id)}><Check size={16} /></button>
              <button className="task-main" onClick={() => openEditor(task.id)}>
                <strong>{task.title}</strong>
                <span style={{ color: tone.accent }}><Clock3 size={13} />{overdue ? '已逾期 · ' : ''}{formatDue(task.dueAt)}</span>
              </button>
            </article>
          );
        })}
      </section>

      {resizeMode && <div className="resize-banner">拖动窗口边缘调整大小 <button onClick={toggleResize}>完成</button></div>}

      {settingsOpen && (
        <div className="modal-backdrop" onMouseDown={(event) => event.target === event.currentTarget && setSettingsOpen(false)}>
          <section className="settings-panel">
            <header><div><h2>设置</h2><p>桌面便签与程序偏好</p></div><button className="icon-button" onClick={() => setSettingsOpen(false)}><X size={19} /></button></header>
            <div className="setting-row">
              <div><strong>外观</strong><span>选择便签的颜色模式</span></div>
              <div className="segmented">
                <button className={settings.theme === 'light' ? 'active' : ''} onClick={() => updateSettings({ theme: 'light' })}><Sun size={15} />浅色</button>
                <button className={settings.theme === 'dark' ? 'active' : ''} onClick={() => updateSettings({ theme: 'dark' })}><Moon size={15} />深色</button>
                <button className={settings.theme === 'system' ? 'active' : ''} onClick={() => updateSettings({ theme: 'system' })}>系统</button>
              </div>
            </div>
            <div className="setting-row">
              <div><strong>开机自动启动</strong><span>启动后安静地驻留在系统托盘</span></div>
              <label className="switch"><input type="checkbox" checked={settings.autostart} onChange={(e) => updateSettings({ autostart: e.target.checked })} /><i /></label>
            </div>
            <div className="setting-row">
              <div><strong>调整便签大小</strong><span>开启后可拖动主窗口边缘</span></div>
              <button className="secondary-button" onClick={() => { void toggleResize(); setSettingsOpen(false); }}><Maximize2 size={15} />开始调整</button>
            </div>
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
        </div>
      )}
    </main>
  );
}
