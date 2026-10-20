use std::{
    fs,
    path::Path,
};

const LEGACY_DATABASE_FILES: [&str; 3] = ["s-tools.db", "s-tools.db-wal", "s-tools.db-shm"];

pub fn remove_legacy_history(data_dir: &Path) -> Result<(), String> {
    for file_name in LEGACY_DATABASE_FILES {
        remove_if_exists(&data_dir.join(file_name))?;
    }
    remove_if_exists(&data_dir.join("captures").join("history"))
}

fn remove_if_exists(path: &Path) -> Result<(), String> {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return Ok(());
    };
    let result = if metadata.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    };
    result.map_err(|error| format!("删除旧历史数据失败（{}）：{error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_only_legacy_history_data() {
        let root = std::env::temp_dir().join(format!("stools-cleanup-{}", uuid::Uuid::new_v4()));
        let captures = root.join("captures");
        let history = captures.join("history");
        let tmp = captures.join("tmp");
        fs::create_dir_all(&history).expect("create history directory");
        fs::create_dir_all(&tmp).expect("create temp directory");
        fs::write(root.join("s-tools.db"), b"db").expect("write db");
        fs::write(root.join("s-tools.db-wal"), b"wal").expect("write wal");
        fs::write(root.join("s-tools.db-shm"), b"shm").expect("write shm");
        fs::write(history.join("legacy.png"), b"legacy").expect("write legacy capture");
        fs::write(captures.join("keep.png"), b"keep").expect("write kept capture");
        fs::write(tmp.join("active.png"), b"active").expect("write temp capture");

        remove_legacy_history(&root).expect("remove legacy history");

        assert!(!root.join("s-tools.db").exists());
        assert!(!root.join("s-tools.db-wal").exists());
        assert!(!root.join("s-tools.db-shm").exists());
        assert!(!history.exists());
        assert!(captures.join("keep.png").exists());
        assert!(tmp.join("active.png").exists());

        fs::remove_dir_all(root).expect("cleanup fixture");
    }
}
