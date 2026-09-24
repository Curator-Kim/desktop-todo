import React from 'react';
import ReactDOM from 'react-dom/client';
import { EditorWindow } from './views/EditorWindow';
import { MainWindow } from './views/MainWindow';
import { SettingsWindow } from './views/SettingsWindow';
import './styles.css';

const params = new URLSearchParams(location.search);
const isEditor = params.get('view') === 'editor';
const isSettings = params.get('view') === 'settings';
document.documentElement.dataset.view = isEditor ? 'editor' : isSettings ? 'settings' : 'main';

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>{isEditor ? <EditorWindow /> : isSettings ? <SettingsWindow /> : <MainWindow />}</React.StrictMode>,
);
