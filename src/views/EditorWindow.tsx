import { useEffect, useRef, useState } from 'react';
import { useEditor, EditorContent } from '@tiptap/react';
import StarterKit from '@tiptap/starter-kit';
import Image from '@tiptap/extension-image';
import Link from '@tiptap/extension-link';
import TaskList from '@tiptap/extension-task-list';
import TaskItem from '@tiptap/extension-task-item';
import { emit } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { Bold, Code2, Heading1, Heading2, ImagePlus, Italic, Link2, List, ListChecks, ListOrdered, Quote, Save, Undo2, Redo2 } from 'lucide-react';
import { loadTasks, upsertTask } from '../store';
import type { ReminderMinutes, Task } from '../types';

function localDateTime(iso?: string) {
  const d = iso ? new Date(iso) : new Date(Date.now() + 60 * 60_000);
  const offset = d.getTimezoneOffset() * 60_000;
  return new Date(d.getTime() - offset).toISOString().slice(0, 16);
}

function fileAsDataUrl(file: File) {
  return new Promise<string>((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(String(reader.result));
    reader.onerror = reject;
    reader.readAsDataURL(file);
  });
}

export function EditorWindow() {
  const taskId = new URLSearchParams(location.search).get('taskId');
  const original = taskId ? loadTasks().find((task) => task.id === taskId) : undefined;
  const [title, setTitle] = useState(original?.title ?? '');
  const [dueAt, setDueAt] = useState(localDateTime(original?.dueAt));
  const [reminderEnabled, setReminderEnabled] = useState(original?.reminderEnabled ?? false);
  const [reminderMinutes, setReminderMinutes] = useState<ReminderMinutes>(original?.reminderMinutes ?? 60);
  const [saved, setSaved] = useState(false);
  const imageInput = useRef<HTMLInputElement>(null);

  const editor = useEditor({
    extensions: [
      StarterKit,
      Image.configure({ allowBase64: true, inline: false }),
      Link.configure({ openOnClick: false }),
      TaskList,
      TaskItem.configure({ nested: true }),
    ],
    content: original?.content || '<p></p>',
    editorProps: {
      attributes: { class: 'prose-editor' },
      handlePaste: (_view, event) => {
        const file = [...(event.clipboardData?.files ?? [])].find((item) => item.type.startsWith('image/'));
        if (!file) return false;
        void fileAsDataUrl(file).then((src) => editor?.chain().focus().setImage({ src }).run());
        return true;
      },
      handleDrop: (_view, event) => {
        const file = [...(event.dataTransfer?.files ?? [])].find((item) => item.type.startsWith('image/'));
        if (!file) return false;
        event.preventDefault();
        void fileAsDataUrl(file).then((src) => editor?.chain().focus().setImage({ src }).run());
        return true;
      },
    },
  });

  useEffect(() => {
    const handler = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 's') {
        event.preventDefault();
        save();
      }
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  });

  const insertImage = async (files: FileList | null) => {
    const file = files?.[0];
    if (!file || !editor) return;
    editor.chain().focus().setImage({ src: await fileAsDataUrl(file), alt: file.name }).run();
  };

  const addLink = () => {
    if (!editor) return;
    const href = window.prompt('输入链接地址', editor.getAttributes('link').href ?? 'https://');
    if (!href) return;
    editor.chain().focus().extendMarkRange('link').setLink({ href }).run();
  };

  const save = async () => {
    if (!title.trim() || !dueAt || !editor) return;
    const now = new Date().toISOString();
    const task: Task = {
      id: original?.id ?? crypto.randomUUID(),
      title: title.trim(),
      dueAt: new Date(dueAt).toISOString(),
      content: editor.getHTML(),
      reminderEnabled,
      reminderMinutes,
      notifiedKey: original?.dueAt === new Date(dueAt).toISOString() && original?.reminderMinutes === reminderMinutes ? original.notifiedKey : undefined,
      completedAt: original?.completedAt,
      createdAt: original?.createdAt ?? now,
      updatedAt: now,
    };
    upsertTask(task);
    await emit('tasks-changed');
    setSaved(true);
    setTimeout(() => void getCurrentWindow().close(), 250);
  };

  const tool = (label: string, icon: React.ReactNode, active: boolean, action: () => void) => (
    <button title={label} className={active ? 'active' : ''} onMouseDown={(event) => { event.preventDefault(); action(); }}>{icon}</button>
  );

  return (
    <main className="editor-shell">
      <header className="editor-topbar" data-tauri-drag-region>
        <span data-tauri-drag-region>{original ? '编辑任务' : '新建任务'}</span>
        <button className="primary-button" disabled={!title.trim() || !dueAt} onClick={save}><Save size={16} />{saved ? '已保存' : '保存'}</button>
      </header>
      <section className="editor-meta">
        <input className="title-input" value={title} onChange={(e) => setTitle(e.target.value)} placeholder="任务标题" autoFocus />
        <div className="meta-grid">
          <label><span>截止时间</span><input type="datetime-local" value={dueAt} onChange={(e) => setDueAt(e.target.value)} /></label>
          <label className="reminder-control"><span>到期通知</span><div><input type="checkbox" checked={reminderEnabled} onChange={(e) => setReminderEnabled(e.target.checked)} />开启</div></label>
          {reminderEnabled && <label><span>提醒时间</span><select value={reminderMinutes} onChange={(e) => setReminderMinutes(Number(e.target.value) as ReminderMinutes)}><option value={0}>到期时</option><option value={10}>提前 10 分钟</option><option value={60}>提前 1 小时</option><option value={1440}>提前 1 天</option></select></label>}
        </div>
      </section>
      <nav className="editor-toolbar">
        {tool('撤销', <Undo2 size={17} />, false, () => editor?.chain().focus().undo().run())}
        {tool('重做', <Redo2 size={17} />, false, () => editor?.chain().focus().redo().run())}
        <i />
        {tool('一级标题', <Heading1 size={17} />, !!editor?.isActive('heading', { level: 1 }), () => editor?.chain().focus().toggleHeading({ level: 1 }).run())}
        {tool('二级标题', <Heading2 size={17} />, !!editor?.isActive('heading', { level: 2 }), () => editor?.chain().focus().toggleHeading({ level: 2 }).run())}
        {tool('粗体', <Bold size={17} />, !!editor?.isActive('bold'), () => editor?.chain().focus().toggleBold().run())}
        {tool('斜体', <Italic size={17} />, !!editor?.isActive('italic'), () => editor?.chain().focus().toggleItalic().run())}
        {tool('引用', <Quote size={17} />, !!editor?.isActive('blockquote'), () => editor?.chain().focus().toggleBlockquote().run())}
        {tool('代码块', <Code2 size={17} />, !!editor?.isActive('codeBlock'), () => editor?.chain().focus().toggleCodeBlock().run())}
        <i />
        {tool('无序列表', <List size={17} />, !!editor?.isActive('bulletList'), () => editor?.chain().focus().toggleBulletList().run())}
        {tool('有序列表', <ListOrdered size={17} />, !!editor?.isActive('orderedList'), () => editor?.chain().focus().toggleOrderedList().run())}
        {tool('任务列表', <ListChecks size={17} />, !!editor?.isActive('taskList'), () => editor?.chain().focus().toggleTaskList().run())}
        {tool('链接', <Link2 size={17} />, !!editor?.isActive('link'), addLink)}
        {tool('插入图片', <ImagePlus size={17} />, false, () => imageInput.current?.click())}
        <input ref={imageInput} type="file" accept="image/*" hidden onChange={(e) => void insertImage(e.target.files)} />
      </nav>
      <section className="editor-paper"><EditorContent editor={editor} /></section>
      <footer className="editor-hint">支持粘贴或拖入图片 · Ctrl+S 保存</footer>
    </main>
  );
}
