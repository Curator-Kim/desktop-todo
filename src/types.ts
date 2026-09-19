export type ReminderMinutes = 0 | 10 | 60 | 1440;

export interface Task {
  id: string;
  title: string;
  dueAt: string;
  content: string;
  reminderEnabled: boolean;
  reminderMinutes: ReminderMinutes;
  notifiedKey?: string;
  completedAt?: string;
  createdAt: string;
  updatedAt: string;
}

export interface AppSettings {
  theme: 'system' | 'light' | 'dark';
  autostart: boolean;
}
