use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::Mutex,
};
use tauri::{Emitter, Manager};
use tauri_plugin_dialog::DialogExt;

const DATA_FILE: &str = "desktop-todo-data.json";
const LOCATION_FILE: &str = "data-location.json";

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Data {
    tasks: Vec<Value>,
    settings: serde_json::Map<String, Value>,
    layout: Option<Value>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    data: Data,
    directory: String,
    revision: u64,
}

pub struct Store {
    directory: PathBuf,
    location: PathBuf,
    data: Data,
    revision: u64,
    _lock: fs::File,
}

pub type StoreState = Mutex<Option<Store>>;

fn read<T: serde::de::DeserializeOwned>(path: &Path) -> io::Result<T> {
    serde_json::from_slice(&fs::read(path)?).map_err(io::Error::other)
}

fn write(path: &Path, value: &impl Serialize, replace: bool) -> io::Result<()> {
    let folder = path
        .parent()
        .ok_or_else(|| io::Error::other("Missing parent directory"))?;
    fs::create_dir_all(folder)?;
    let mut temp = tempfile::NamedTempFile::new_in(folder)?;
    serde_json::to_writer(temp.as_file_mut(), value).map_err(io::Error::other)?;
    temp.flush()?;
    temp.as_file().sync_all()?;
    if replace {
        temp.persist(path)?;
    } else {
        temp.persist_noclobber(path)?;
    }
    Ok(())
}

impl Store {
    fn open(config: &Path, default_dir: PathBuf, legacy: Option<Data>) -> io::Result<Self> {
        fs::create_dir_all(config)?;
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(config.join("data.lock"))?;
        lock.try_lock().map_err(|e| {
            io::Error::other(format!(
                "无法锁定待办数据，请关闭其他桌面待办实例后重试：{e}"
            ))
        })?;
        let location = config.join(LOCATION_FILE);
        let (directory, custom) = match read::<PathBuf>(&location) {
            Ok(dir) => (dir, true),
            Err(e) if e.kind() == io::ErrorKind::NotFound => (default_dir, false),
            Err(e) => return Err(e),
        };
        let file = directory.join(DATA_FILE);
        let data = match read(&file) {
            Ok(data) => data,
            Err(e) if e.kind() == io::ErrorKind::NotFound && !custom => {
                let legacy = legacy.ok_or(e)?;
                match write(&file, &legacy, false) {
                    Ok(()) => legacy,
                    Err(e) if e.kind() == io::ErrorKind::AlreadyExists => read(&file)?,
                    Err(e) => return Err(e),
                }
            }
            Err(e) => return Err(e),
        };
        Ok(Self {
            directory,
            location,
            data,
            revision: 0,
            _lock: lock,
        })
    }

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            data: self.data.clone(),
            directory: self.directory.to_string_lossy().into_owned(),
            revision: self.revision,
        }
    }

    fn migrate(&mut self, directory: PathBuf) -> io::Result<()> {
        let canonical_target = directory.canonicalize()?;
        if self.directory.canonicalize().ok().as_ref() == Some(&canonical_target) {
            return Ok(());
        }
        let target = directory.join(DATA_FILE);
        write(&target, &self.data, false)?;
        if let Err(err) = write(&self.location, &directory, true) {
            let _ = fs::remove_file(&target);
            return Err(err);
        }
        self.directory = directory;
        self.revision += 1;
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Change {
    Settings {
        patch: serde_json::Map<String, Value>,
    },
    ResetSettings {
        settings: serde_json::Map<String, Value>,
    },
    Layout {
        layout: Option<Value>,
    },
    SaveTask {
        task: serde_json::Map<String, Value>,
        existing: bool,
    },
    Complete {
        id: String,
        at: String,
    },
    Restore {
        id: String,
    },
    Delete {
        id: String,
    },
    Notify {
        id: String,
        key: String,
    },
}

impl Change {
    fn apply(self, data: &mut Data) -> Result<(), String> {
        let tasks = &mut data.tasks;
        let find = |tasks: &[Value], id: &str| {
            tasks
                .iter()
                .position(|task| task["id"].as_str() == Some(id))
        };
        match self {
            Self::Settings { patch } => data.settings.extend(patch),
            Self::ResetSettings { settings } => {
                data.settings = settings;
                data.layout = None;
            }
            Self::Layout { layout } => data.layout = layout,
            Self::SaveTask { mut task, existing } => {
                let id = task
                    .get("id")
                    .and_then(Value::as_str)
                    .ok_or("Missing task ID")?;
                if let Some(i) = find(tasks, id) {
                    let previous = &tasks[i];
                    if let Some(completed) = previous.get("completedAt") {
                        task.insert("completedAt".into(), completed.clone());
                    }
                    if previous.get("dueAt") == task.get("dueAt")
                        && previous.get("reminderMinutes") == task.get("reminderMinutes")
                    {
                        if let Some(key) = previous.get("notifiedKey") {
                            task.insert("notifiedKey".into(), key.clone());
                        }
                    }
                    tasks[i] = Value::Object(task);
                } else if existing {
                    return Err("任务已被删除".into());
                } else {
                    tasks.push(Value::Object(task));
                }
            }
            Self::Complete { id, at } => {
                if let Some(i) = find(tasks, &id) {
                    tasks[i]["completedAt"] = Value::String(at);
                }
            }
            Self::Restore { id } => {
                if let Some(i) = find(tasks, &id) {
                    if let Some(task) = tasks[i].as_object_mut() {
                        task.remove("completedAt");
                    }
                }
            }
            Self::Delete { id } => {
                if let Some(i) = find(tasks, &id) {
                    if tasks[i]["completedAt"].is_string() {
                        tasks.remove(i);
                    }
                }
            }
            Self::Notify { id, key } => {
                if let Some(i) = find(tasks, &id) {
                    let task = &mut tasks[i];
                    let expected = format!(
                        "{}:{}",
                        task["dueAt"].as_str().unwrap_or_default(),
                        task["reminderMinutes"]
                    );
                    if key == expected
                        && task["completedAt"].is_null()
                        && task["reminderEnabled"] == true
                    {
                        task["notifiedKey"] = Value::String(key);
                    }
                }
            }
        }
        Ok(())
    }
}

#[tauri::command]
pub async fn load_data(
    app: tauri::AppHandle,
    legacy: Option<Data>,
) -> Result<Option<Snapshot>, String> {
    let state = app.state::<StoreState>();
    let mut guard = state.lock().map_err(|e| e.to_string())?;
    if guard.is_none() {
        let paths = app.path();
        let config = paths.app_config_dir().map_err(|e| e.to_string())?;
        let default_dir = paths
            .app_local_data_dir()
            .map_err(|e| e.to_string())?
            .join("data");
        if legacy.is_none()
            && !config
                .join(LOCATION_FILE)
                .try_exists()
                .map_err(|e| e.to_string())?
            && !default_dir
                .join(DATA_FILE)
                .try_exists()
                .map_err(|e| e.to_string())?
        {
            return Ok(None);
        }
        *guard = Some(Store::open(&config, default_dir, legacy).map_err(|e| e.to_string())?);
    }
    Ok(guard.as_ref().map(Store::snapshot))
}

#[tauri::command]
pub async fn change_data(app: tauri::AppHandle, change: Change) -> Result<Snapshot, String> {
    let state = app.state::<StoreState>();
    let mut guard = state.lock().map_err(|e| e.to_string())?;
    let store = guard.as_mut().ok_or("Data store is not initialized")?;
    let mut data = store.data.clone();
    change.apply(&mut data)?;
    write(&store.directory.join(DATA_FILE), &data, true).map_err(|e| e.to_string())?;
    store.data = data;
    store.revision += 1;
    let snapshot = store.snapshot();
    let _ = app.emit("data-changed", ());
    Ok(snapshot)
}

#[tauri::command]
pub async fn migrate_data(window: tauri::WebviewWindow) -> Result<Option<Snapshot>, String> {
    if window.label() != "settings" {
        return Err("请从设置窗口迁移数据".into());
    }
    let app = window.app_handle();
    let Some(folder) = app
        .dialog()
        .file()
        .set_title("选择新的数据目录")
        .set_parent(&window)
        .blocking_pick_folder()
    else {
        return Ok(None);
    };
    let directory = folder.into_path().map_err(|e| e.to_string())?;
    let state = app.state::<StoreState>();
    let mut guard = state.lock().map_err(|e| e.to_string())?;
    let store = guard.as_mut().ok_or("Data store is not initialized")?;
    store.migrate(directory).map_err(|e| match e.kind() {
        io::ErrorKind::AlreadyExists => "目标目录已有桌面待办数据，请选择其他目录。".into(),
        _ => e.to_string(),
    })?;
    let _ = app.emit("data-changed", ());
    Ok(Some(store.snapshot()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample() -> Data {
        serde_json::from_value(json!({
            "tasks": [
                {"id": "active", "content": "<img src=\"data:image/png;base64,AA==\">"},
                {"id": "done", "completedAt": "2026-11-11T00:00:00Z"}
            ],
            "settings": {"theme": "dark"},
            "layout": {"x": 30, "y": 50, "width": 380, "height": 520}
        }))
        .unwrap()
    }

    #[test]
    fn migration_preserves_all_data_and_reopens_destination() {
        let root = tempfile::tempdir().unwrap();
        let config = root.path().join("config");
        let original = root.path().join("original");
        let target = root.path().join("target");
        fs::create_dir(&target).unwrap();
        let mut store = Store::open(&config, original.clone(), Some(sample())).unwrap();
        store.migrate(target.clone()).unwrap();
        let before = fs::read(original.join(DATA_FILE)).unwrap();
        assert_eq!(before, fs::read(target.join(DATA_FILE)).unwrap());
        Change::Restore { id: "done".into() }
            .apply(&mut store.data)
            .unwrap();
        write(&target.join(DATA_FILE), &store.data, true).unwrap();
        drop(store);
        let reopened = Store::open(&config, original.clone(), None).unwrap();
        assert_eq!(reopened.directory, target);
        assert!(reopened.data.tasks[1].get("completedAt").is_none());
        assert_eq!(reopened.data.tasks[0], sample().tasks[0]);
        assert_eq!(reopened.data.settings, sample().settings);
        assert_eq!(reopened.data.layout, sample().layout);
        assert_eq!(fs::read(original.join(DATA_FILE)).unwrap(), before);
    }

    #[test]
    fn existing_destination_is_never_overwritten() {
        let root = tempfile::tempdir().unwrap();
        let original = root.path().join("original");
        let target = root.path().join("target");
        let config = root.path().join("config");
        let mut store = Store::open(&config, original.clone(), Some(sample())).unwrap();
        fs::create_dir(&target).unwrap();
        fs::write(target.join(DATA_FILE), b"existing data").unwrap();
        assert_eq!(
            store.migrate(target.clone()).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(store.directory, original);
        assert!(!config.join(LOCATION_FILE).exists());
        assert_eq!(fs::read(target.join(DATA_FILE)).unwrap(), b"existing data");
    }

    #[test]
    fn location_write_failure_keeps_original_directory() {
        let root = tempfile::tempdir().unwrap();
        let original = root.path().join("original");
        let target = root.path().join("target");
        let config = root.path().join("config");
        let mut store = Store::open(&config, original.clone(), Some(sample())).unwrap();
        fs::create_dir(&target).unwrap();
        fs::create_dir(config.join(LOCATION_FILE)).unwrap();
        assert!(store.migrate(target.clone()).is_err());
        assert_eq!(store.directory, original);
        assert!(!target.join(DATA_FILE).exists());
        assert!(original.join(DATA_FILE).is_file());
    }
}
