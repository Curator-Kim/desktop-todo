import { useEffect, useState } from 'react';
import { changeData, loadSettings, loadTasks, watchData } from '../store';

export function HistoryWindow() {
  const [tasks, setTasks] = useState(loadTasks);
  const [error, setError] = useState('');
  const completed = tasks.filter((task) => task.completedAt)
    .sort((a, b) => Date.parse(b.completedAt!) - Date.parse(a.completedAt!));
  useEffect(() => watchData(() => {
    setTasks(loadTasks());
    document.documentElement.dataset.theme = loadSettings().theme;
  }), []);

  async function change(kind: 'restore' | 'delete', id: string) {
    setError('');
    try {
      await changeData({ kind, id });
    } catch {
      setError('无法保存更改，请确认当前数据目录可写。');
    }
  }

  return <main className="settings-shell">
    <section className="settings-panel">
      <header><div><h2>已完成任务</h2><p>共 {completed.length} 项</p></div></header>
      {error && <p className="setting-error" role="alert">{error}</p>}
      {completed.length === 0 ? <p className="muted-center">还没有已完成的任务</p> : completed.map((task) => (
        <div className="history-item" key={task.id}>
          <div><strong>{task.title}</strong><span>{new Date(task.completedAt!).toLocaleString('zh-CN')}</span></div>
          <button onClick={() => void change('restore', task.id)}>恢复</button>
          <button className="danger-text" onClick={() => void change('delete', task.id)}>删除</button>
        </div>
      ))}
    </section>
  </main>;
}
