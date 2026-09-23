import type { AppSettings, Task } from './types';

const TASKS_KEY = 'desktop-todo.tasks.v1';
const SETTINGS_KEY = 'desktop-todo.settings.v1';
const WINDOW_LAYOUT_KEY = 'desktop-todo.window-layout.v1';

export interface WindowLayout {
  x: number;
  y: number;
  width: number;
  height: number;
}

export function loadWindowLayout(): WindowLayout | null {
  try {
    const layout = JSON.parse(localStorage.getItem(WINDOW_LAYOUT_KEY) ?? 'null') as WindowLayout | null;
    if (layout && [layout.x, layout.y, layout.width, layout.height].every(Number.isFinite)
      && layout.width >= 300 && layout.height >= 300) return layout;
  } catch {
    // Ignore invalid saved geometry and use the configured default window size.
  }
  return null;
}

export function saveWindowLayout(layout: WindowLayout) {
  localStorage.setItem(WINDOW_LAYOUT_KEY, JSON.stringify(layout));
}

export function loadTasks(): Task[] {
  try {
    return JSON.parse(localStorage.getItem(TASKS_KEY) ?? '[]') as Task[];
  } catch {
    return [];
  }
}

export function saveTasks(tasks: Task[]) {
  localStorage.setItem(TASKS_KEY, JSON.stringify(tasks));
  window.dispatchEvent(new Event('tasks-local-change'));
}

export function upsertTask(task: Task) {
  const tasks = loadTasks();
  const index = tasks.findIndex((item) => item.id === task.id);
  if (index >= 0) tasks[index] = task;
  else tasks.push(task);
  saveTasks(tasks);
}

export function loadSettings(): AppSettings {
  const defaults: AppSettings = {
    theme: 'system',
    autostart: false,
    backgroundColor: '',
    backgroundImage: '',
    transparency: 14,
  };
  try {
    const saved = { ...defaults, ...JSON.parse(localStorage.getItem(SETTINGS_KEY) ?? '{}') } as AppSettings;
    return {
      ...saved,
      backgroundColor: /^#[0-9a-f]{6}$/i.test(saved.backgroundColor) ? saved.backgroundColor : '',
      backgroundImage: typeof saved.backgroundImage === 'string' && saved.backgroundImage.startsWith('data:image/jpeg;base64,') ? saved.backgroundImage : '',
      transparency: Number.isFinite(saved.transparency) ? Math.min(70, Math.max(0, saved.transparency)) : 14,
    };
  } catch {
    return defaults;
  }
}

export function saveSettings(settings: AppSettings) {
  localStorage.setItem(SETTINGS_KEY, JSON.stringify(settings));
}

export function formatDue(iso: string) {
  const date = new Date(iso);
  const today = new Date();
  const tomorrow = new Date(today);
  tomorrow.setDate(today.getDate() + 1);
  const sameDay = (a: Date, b: Date) => a.toDateString() === b.toDateString();
  const time = date.toLocaleTimeString('zh-CN', { hour: '2-digit', minute: '2-digit', hour12: false });
  if (sameDay(date, today)) return `今天 ${time}`;
  if (sameDay(date, tomorrow)) return `明天 ${time}`;
  return date.toLocaleString('zh-CN', { month: 'numeric', day: 'numeric', hour: '2-digit', minute: '2-digit', hour12: false });
}

export function dueTone(iso: string) {
  const hours = (new Date(iso).getTime() - Date.now()) / 3_600_000;
  if (hours <= 0) return { background: 'var(--overdue-bg)', border: 'var(--overdue-border)', accent: '#d83a45' };
  if (hours >= 24) return { background: 'var(--card)', border: 'var(--border)', accent: 'var(--muted)' };
  const urgency = 1 - hours / 24;
  const hue = Math.round(47 - urgency * 43);
  return {
    background: `color-mix(in srgb, hsl(${hue} 92% 58%) ${12 + urgency * 12}%, var(--card))`,
    border: `color-mix(in srgb, hsl(${hue} 80% 46%) ${35 + urgency * 35}%, var(--border))`,
    accent: `hsl(${hue} 74% 42%)`,
  };
}
