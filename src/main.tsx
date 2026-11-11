import React from 'react';
import ReactDOM from 'react-dom/client';
import { EditorWindow } from './views/EditorWindow';
import { MainWindow } from './views/MainWindow';
import { SettingsWindow } from './views/SettingsWindow';
import { HistoryWindow } from './views/HistoryWindow';
import { initializeData } from './store';
import './styles.css';

const params = new URLSearchParams(location.search);
const isEditor = params.get('view') === 'editor';
const isSettings = params.get('view') === 'settings';
const isHistory = params.get('view') === 'history';
document.documentElement.dataset.view = isEditor ? 'editor' : isSettings ? 'settings' : isHistory ? 'history' : 'main';

const root = ReactDOM.createRoot(document.getElementById('root')!);
async function start() {
  root.render(<main className="settings-shell">正在读取待办数据…</main>);
  try {
    await initializeData();
    root.render(<React.StrictMode>{isEditor ? <EditorWindow /> : isSettings ? <SettingsWindow /> : isHistory ? <HistoryWindow /> : <MainWindow />}</React.StrictMode>);
  } catch (error) {
    root.render(<main className="settings-shell">
      <p role="alert">无法读取待办数据。请确认数据目录仍可访问。</p>
      <p>{String(error)}</p>
      <button className="secondary-button" onClick={() => void start()}>重试</button>
    </main>);
  }
}
void start();
