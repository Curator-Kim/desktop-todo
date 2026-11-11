import type { AppSettings, Task } from './types';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

const TASKS_KEY = 'desktop-todo.tasks.v1';
const SETTINGS_KEY = 'desktop-todo.settings.v1';
const WINDOW_LAYOUT_KEY = 'desktop-todo.window-layout.v1';

export interface WindowLayout {
  x: number;
  y: number;
  width: number;
  height: number;
}

interface Data {
  tasks: Task[];
  settings: Partial<AppSettings>;
  layout: WindowLayout | null;
}

interface Snapshot { data: Data; directory: string; revision: number }

type Change =
  | { kind: 'settings'; patch: Partial<AppSettings> }
  | { kind: 'resetSettings'; settings: AppSettings }
  | { kind: 'layout'; layout: WindowLayout }
  | { kind: 'saveTask'; task: Task; existing: boolean }
  | { kind: 'complete'; id: string; at: string }
  | { kind: 'restore' | 'delete'; id: string }
  | { kind: 'notify'; id: string; key: string };

let snapshot: Snapshot;
const subscribers = new Set<() => void>();

function accept(next: Snapshot) {
  if (snapshot && snapshot.revision >= next.revision) return;
  snapshot = next;
  subscribers.forEach((refresh) => refresh());
}

function legacyData(): Data {
  const read = <T,>(key: string, fallback: T): T => JSON.parse(localStorage.getItem(key) ?? JSON.stringify(fallback)) as T;
  return { tasks: read(TASKS_KEY, []), settings: read(SETTINGS_KEY, {}), layout: read(WINDOW_LAYOUT_KEY, null) };
}

export async function initializeData() {
  // Existing disk data takes precedence; read the old browser storage only when seeding.
  const existing = await invoke<Snapshot | null>('load_data');
  if (existing) accept(existing);
  else accept(await invoke<Snapshot>('load_data', { legacy: legacyData() }));
  const stop = await listen('data-changed', () => void invoke<Snapshot>('load_data').then(accept).catch(console.error));
  try {
    accept(await invoke<Snapshot>('load_data'));
  } catch (error) {
    stop();
    throw error;
  }
}

export function watchData(refresh: () => void) {
  subscribers.add(refresh);
  refresh();
  return () => { subscribers.delete(refresh); };
}

export function dataDirectory() { return snapshot.directory; }

export async function migrateData() {
  const next = await invoke<Snapshot | null>('migrate_data');
  if (next) accept(next);
  return Boolean(next);
}

export async function changeData(change: Change) {
  accept(await invoke<Snapshot>('change_data', { change }));
}

export function loadWindowLayout(): WindowLayout | null {
  try {
    const layout = snapshot.data.layout;
    if (layout && [layout.x, layout.y, layout.width, layout.height].every(Number.isFinite)
      && layout.width >= 300 && layout.height >= 300) return layout;
  } catch {
    // Ignore invalid saved geometry and use the configured default window size.
  }
  return null;
}

export function saveWindowLayout(layout: WindowLayout) {
  return changeData({ kind: 'layout', layout });
}

export function loadTasks(): Task[] {
  return snapshot.data.tasks;
}

export function upsertTask(task: Task, existing: boolean) {
  return changeData({ kind: 'saveTask', task, existing });
}

export function defaultSettings(): AppSettings {
  return {
    theme: 'system',
    autostart: false,
    backgroundColor: '',
    backgroundImage: '',
    transparency: 14,
  };
}

export function loadSettings(): AppSettings {
  const defaults = defaultSettings();
  try {
    const saved = { ...defaults, ...snapshot.data.settings };
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

export function saveSettings(patch: Partial<AppSettings>) {
  return changeData({ kind: 'settings', patch });
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
