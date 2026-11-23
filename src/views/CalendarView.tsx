import { useMemo, useState } from 'react';
import { ChevronLeft, ChevronRight } from 'lucide-react';
import { dueTone } from '../store';
import type { Task } from '../types';

const key = (date: Date) => `${date.getFullYear()}-${date.getMonth()}-${date.getDate()}`;

export function CalendarView({ tasks, open, now }: { tasks: Task[]; open: (id: string) => void; now: number }) {
  const [month, setMonth] = useState(() => new Date(new Date().getFullYear(), new Date().getMonth(), 1));
  const groups = useMemo(() => {
    const result = new Map<string, Task[]>();
    tasks.forEach((task) => {
      const day = key(new Date(task.dueAt));
      result.set(day, [...(result.get(day) ?? []), task]);
    });
    return result;
  }, [tasks]);
  const start = new Date(month.getFullYear(), month.getMonth(), 1);
  start.setDate(1 - (start.getDay() + 6) % 7);
  const days = Array.from({ length: 42 }, (_, i) => new Date(start.getFullYear(), start.getMonth(), start.getDate() + i));
  return <section className="calendar-view" aria-label="待办日历">
    <div className="calendar-nav">
      <strong>{month.getFullYear()} 年 {month.getMonth() + 1} 月</strong>
      <div>
        <button className="icon-button" title="上个月" onClick={() => setMonth(new Date(month.getFullYear(), month.getMonth() - 1, 1))}><ChevronLeft size={17} /></button>
        <button className="calendar-today" onClick={() => setMonth(new Date(new Date().getFullYear(), new Date().getMonth(), 1))}>今天</button>
        <button className="icon-button" title="下个月" onClick={() => setMonth(new Date(month.getFullYear(), month.getMonth() + 1, 1))}><ChevronRight size={17} /></button>
      </div>
    </div>
    <div className="calendar-week">{['一', '二', '三', '四', '五', '六', '日'].map((day) => <span key={day}>{day}</span>)}</div>
    <div className="calendar-grid">
      {days.map((day) => <div key={key(day)} className={`calendar-day ${day.getMonth() !== month.getMonth() ? 'outside' : ''}`}>
        <span className={key(day) === key(new Date(now)) ? 'today' : ''}>{day.getDate()}</span>
        <div className="calendar-tasks">{(groups.get(key(day)) ?? []).map((task) => {
          const tone = dueTone(task.dueAt);
          return <button key={task.id} title={`${task.title} · ${new Date(task.dueAt).toLocaleString('zh-CN')}`} onClick={() => open(task.id)}>
            <strong>{task.title}</strong>
            <small style={{ color: tone.accent }}>{new Date(task.dueAt).toLocaleTimeString('zh-CN', { hour: '2-digit', minute: '2-digit', hour12: false })}</small>
          </button>;
        })}</div>
      </div>)}
    </div>
  </section>;
}
