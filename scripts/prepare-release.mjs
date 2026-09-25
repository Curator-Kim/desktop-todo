import { copyFile, mkdir, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';

const project = process.cwd();
const { version } = JSON.parse(await readFile(path.join(project, 'package.json'), 'utf8'));
const installer = path.join(project, 'src-tauri', 'target', 'release', 'bundle', 'nsis', `桌面待办_${version}_x64-setup.exe`);
const portable = path.join(project, 'src-tauri', 'target', 'release', 'zhuomian-daiban.exe');
const signature = (await readFile(`${installer}.sig`, 'utf8')).trim();
if (!signature) throw new Error('Missing updater signature');

const output = path.join(project, '.private', 'release', `v${version}`);
await mkdir(output, { recursive: true });
const setupName = `desktop-todo-${version}-setup.exe`;
await copyFile(installer, path.join(output, setupName));
await copyFile(portable, path.join(output, `desktop-todo-${version}-portable.exe`));

const manifest = {
  version,
  platforms: {
    'windows-x86_64': {
      signature,
      url: `https://github.com/Curator-Kim/desktop-todo/releases/download/v${version}/${setupName}`,
    },
  },
};
await writeFile(path.join(output, 'latest.json'), `${JSON.stringify(manifest, null, 2)}\n`);
console.log(`Prepared signed release assets in ${output}`);
