import React from 'react';
import ReactDOM from 'react-dom/client';
import { EditorWindow } from './views/EditorWindow';
import { MainWindow } from './views/MainWindow';
import './styles.css';

const params = new URLSearchParams(location.search);
const isEditor = params.get('view') === 'editor';

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>{isEditor ? <EditorWindow /> : <MainWindow />}</React.StrictMode>,
);
