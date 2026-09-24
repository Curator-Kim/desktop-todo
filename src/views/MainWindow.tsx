import { useCallback, useEffect, useMemo, useState } from 'react';
import type { CSSProperties, MouseEvent } from 'react';
import { WebviewWindow } from '@tauri-apps/api/webviewWindow';
import { listen } from '@tauri-apps/api/event';
import { availableMonitors, getCurrentWindow, PhysicalPosition, PhysicalSize } from '@tauri-apps/api/window';
import { invoke } from '@tauri-apps/api/core';
import { isPermissionGranted, requestPermission, sendNotification } from '@tauri-apps/plugin-notification';
import { Check, Clock3, Plus, Settings, X } from 'lucide-react';
import { dueTone, formatDue, loadSettings, loadTasks, loadWindowLayout, saveTasks, saveWindowLayout } from '../store';
import type { AppSettings, Task } from '../types';

function openEditor(taskId?: string, onError?: (message: string) => void) {
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
  void win.once('tauri://error', (error) => {
    console.error('无法打开任务编辑窗口', error.payload);
    onError?.('无法打开任务编辑窗口，请重启程序后重试。');
  });
}

export function MainWindow() {
  const [tasks, setTasks] = useState<Task[]>(loadTasks);
  const [settings, setSettings] = useState<AppSettings>(loadSettings);
  const [editorError, setEditorError] = useState('');
  const [now, setNow] = useState(Date.now());

  const refresh = useCallback(() => setTasks(loadTasks()), []);
  const activeTasks = useMemo(
    () => tasks.filter((task) => !task.completedAt).sort((a, b) => +new Date(a.dueAt) - +new Date(b.dueAt)),
    [tasks],
  );
  useEffect(() => {
    const unlistenPromise = listen('tasks-changed', refresh);
    const settingsChangedPromise = listen('settings-changed', () => setSettings(loadSettings()));
    const resetLayoutPromise = listen('reset-window-layout', () => {
      const currentWindow = getCurrentWindow();
      void currentWindow.setSize(new PhysicalSize(380, 520)).then(() => currentWindow.center())
        .catch((error) => console.error('无法恢复便签默认位置', error));
    });
    const refreshStorage = () => { refresh(); setSettings(loadSettings()); };
    window.addEventListener('storage', refreshStorage);
    window.addEventListener('tasks-local-change', refresh);
    const clock = window.setInterval(() => setNow(Date.now()), 30_000);
    return () => {
      void unlistenPromise.then((fn) => fn());
      void settingsChangedPromise.then((fn) => fn());
      void resetLayoutPromise.then((fn) => fn());
      window.removeEventListener('storage', refreshStorage);
      window.removeEventListener('tasks-local-change', refresh);
      clearInterval(clock);
    };
  }, [refresh]);

  useEffect(() => {
    const root = document.documentElement;
    root.dataset.theme = settings.theme;
  }, [settings.theme]);

  useEffect(() => {
    const layout = loadWindowLayout();
    if (!layout) return;
    const restore = async () => {
      const currentWindow = getCurrentWindow();
      const monitors = await availableMonitors();
      const visible = monitors.some(({ position, size }) =>
        layout.x < position.x + size.width - 80
        && layout.x + layout.width > position.x + 80
        && layout.y < position.y + size.height - 40
        && layout.y + layout.height > position.y + 40,
      );
      await currentWindow.setSize(new PhysicalSize(layout.width, layout.height));
      if (visible) await currentWindow.setPosition(new PhysicalPosition(layout.x, layout.y));
    };
    void restore().catch((error) => console.error('无法恢复便签位置', error));
  }, []);

  useEffect(() => {
    const currentWindow = getCurrentWindow();
    let timer: number | undefined;
    let disposed = false;
    let listeners: Array<() => void> = [];
    const saveLayout = () => {
      window.clearTimeout(timer);
      timer = window.setTimeout(() => {
        void Promise.all([currentWindow.innerPosition(), currentWindow.innerSize()])
          .then(([position, size]) => saveWindowLayout({ x: position.x, y: position.y, width: size.width, height: size.height }))
          .catch((error) => console.error('无法保存便签位置', error));
      }, 250);
    };
    void Promise.all([currentWindow.onMoved(saveLayout), currentWindow.onResized(saveLayout)])
      .then((unlisten) => { if (disposed) unlisten.forEach((stop) => stop()); else listeners = unlisten; })
      .catch((error) => console.error('无法监听便签位置变化', error));
    return () => {
      disposed = true;
      window.clearTimeout(timer);
      listeners.forEach((stop) => stop());
    };
  }, []);

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

  const dragWindow = (event: MouseEvent<HTMLElement>) => {
    if (event.button !== 0) return;
    if (!(event.target instanceof Element)) return;
    const bounds = event.currentTarget.getBoundingClientRect();
    if (event.clientX - bounds.left < 10 || bounds.right - event.clientX < 10
      || event.clientY - bounds.top < 10 || bounds.bottom - event.clientY < 10) return;
    if (event.target.closest('button, a, input, select, textarea, [contenteditable]')) return;
    void getCurrentWindow().startDragging().catch((error) => console.error('无法拖动便签', error));
  };

  const showEditor = (taskId?: string) => {
    setEditorError('');
    openEditor(taskId, setEditorError);
  };

  const noteStyle = {
    '--note-opacity': String(1 - settings.transparency / 100),
    '--note-custom-color': settings.backgroundColor || undefined,
    '--note-background-image': settings.backgroundImage ? `url("${settings.backgroundImage}")` : 'none',
  } as CSSProperties;

  return (
    <main className="sticky-shell" style={noteStyle} onMouseDown={dragWindow}>
      <header className="sticky-header">
        <div className="brand">
          <span className="brand-dot" />
          <span>待办</span>
          <small>{activeTasks.length}</small>
        </div>
        <div className="header-actions">
          <button className="icon-button" title="新建任务" onClick={() => showEditor()}><Plus size={19} /></button>
          <button className="icon-button" title="设置" onClick={() => void invoke('show_settings_command').catch((error) => console.error('无法打开设置窗口', error))}><Settings size={18} /></button>
          <button className="icon-button" title="隐藏到托盘" onClick={() => getCurrentWindow().hide()}><X size={18} /></button>
        </div>
      </header>

      {editorError && <div role="alert" className="editor-error" onClick={() => setEditorError('')}>{editorError}</div>}

      <section className="task-list" aria-label="待办任务">
        {activeTasks.length === 0 ? (
          <div className="empty-state">
            <button className="empty-create" onClick={() => showEditor()}>
              <span className="empty-plus"><Plus size={24} /></span>
              <strong>暂时没有待办</strong>
              <span>点击创建第一项任务</span>
            </button>
          </div>
        ) : activeTasks.map((task) => {
          const tone = dueTone(task.dueAt);
          const overdue = +new Date(task.dueAt) < now;
          return (
            <article className="task-card" key={task.id} style={{ background: tone.background, borderColor: tone.border }}>
              <button className="complete-button" title="完成任务" onClick={() => completeTask(task.id)}><Check size={16} /></button>
              <button className="task-main" onClick={() => showEditor(task.id)}>
                <strong>{task.title}</strong>
                <span style={{ color: tone.accent }}><Clock3 size={13} />{overdue ? '已逾期 · ' : ''}{formatDue(task.dueAt)}</span>
              </button>
            </article>
          );
        })}
      </section>

    </main>
  );
}
