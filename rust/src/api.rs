use flutter_rust_bridge::frb;
use serde_json;
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    str::FromStr,
};
use taskchampion::{
    Operations, Replica, ServerConfig, StorageConfig, Tag,
    chrono::{DateTime, Utc},
};
use uuid::Uuid;

fn parse_datetime(input: &str) -> Option<DateTime<Utc>> {
    if input.trim().is_empty() {
        return None;
    }
    input.parse::<DateTime<Utc>>().ok()
}

#[frb]
pub fn get_all_tasks_json(taskdb_dir_path: String) -> Result<String, taskchampion::Error> {
    let tasks = get_all_tasks(taskdb_dir_path); // your Vec<HashMap<String, String>>
    let json = serde_json::to_string(&tasks)
        .map_err(|e| taskchampion::Error::Other(anyhow::anyhow!(e)))?;
    Ok(json)
}

fn get_all_tasks(taskdb_dir_path: String) -> Vec<HashMap<String, String>> {
    let taskdb_dir = PathBuf::from(taskdb_dir_path);
    let storage = StorageConfig::OnDisk {
        taskdb_dir,
        create_if_missing: true,
        access_mode: taskchampion::storage::AccessMode::ReadWrite,
    }
    .into_storage()
    .unwrap();

    let mut replica = Replica::new(storage);
    let mut vector: Vec<HashMap<String, String>> = Vec::new();

    for (_, value) in replica.all_tasks().unwrap() {
        let mut map: HashMap<String, String> = HashMap::new();
        let mut tags = "".to_string();

        for (k, v) in value.get_taskmap() {
            if k.contains("tag_") {
                if let Some(stripped) = k.strip_prefix("tag_") {
                    tags.push_str(stripped);
                    tags.push(' ');
                }
            } else {
                map.insert(k.into(), v.into());
            }
        }
        map.insert("tags".into(), tags.trim().into());
        map.insert("uuid".into(), value.get_uuid().to_string());
        vector.push(map);
    }
    vector
}

fn task_matches_filter(
    task: &HashMap<String, String>,
    key: &str,
    expected: &Option<String>,
) -> bool {
    match expected {
        Some(value) => task.get(key) == Some(value),
        None => true,
    }
}

fn task_matches_tag_filter(task: &HashMap<String, String>, tag_filter: &str) -> bool {
    let task_tags: HashSet<&str> = task
        .get("tags")
        .map(String::as_str)
        .unwrap_or("")
        .split_whitespace()
        .collect();

    for raw_part in tag_filter.split_whitespace() {
        let (should_exist, tag) = match raw_part.chars().next() {
            Some('+') => (true, &raw_part[1..]),
            Some('-') => (false, &raw_part[1..]),
            _ => (true, raw_part),
        };

        if tag.is_empty() {
            continue;
        }

        if task_tags.contains(tag) != should_exist {
            return false;
        }
    }

    true
}

#[frb]
pub fn query_task(
    taskdb_dir_path: String,
    uuid: Option<String>,
    status: Option<String>,
    tags: Option<String>,
    project: Option<String>,
) -> Result<String, taskchampion::Error> {
    let filtered_tasks: Vec<HashMap<String, String>> = get_all_tasks(taskdb_dir_path)
        .into_iter()
        .filter(|task| task_matches_filter(task, "uuid", &uuid))
        .filter(|task| task_matches_filter(task, "status", &status))
        .filter(|task| task_matches_filter(task, "project", &project))
        .filter(|task| {
            tags.as_deref()
                .map(|tag_filter| task_matches_tag_filter(task, tag_filter))
                .unwrap_or(true)
        })
        .collect();

    let json = serde_json::to_string(&filtered_tasks)
        .map_err(|e| taskchampion::Error::Other(anyhow::anyhow!(e)))?;

    Ok(json)
}

#[frb]
pub fn delete_task(uuid_st: String, taskdb_dir_path: String) -> i8 {
    let taskdb_dir = PathBuf::from(taskdb_dir_path);
    let storage = StorageConfig::OnDisk {
        taskdb_dir,
        create_if_missing: true,
        access_mode: taskchampion::storage::AccessMode::ReadWrite,
    }
    .into_storage()
    .unwrap();

    let mut replica = Replica::new(storage);
    let mut ops = Operations::new();
    let uuid = Uuid::parse_str(&uuid_st).unwrap();

    if let Some(mut t) = replica.get_task_data(uuid).unwrap() {
        t.delete(&mut ops);
    }
    replica.commit_operations(ops).unwrap();
    0
}

#[frb]
pub fn update_task(
	uuid_st: String, 
	taskdb_dir_path: String, 
	map: HashMap<String, String>,
) -> i8 {
    let taskdb_dir = PathBuf::from(taskdb_dir_path);
    let storage = StorageConfig::OnDisk {
        taskdb_dir,
        create_if_missing: true,
        access_mode: taskchampion::storage::AccessMode::ReadWrite,
    }
    .into_storage()
    .unwrap();

    let mut replica = Replica::new(storage);
    let mut ops = Operations::new();
    let uuid = Uuid::parse_str(&uuid_st).unwrap();

    if let Some(mut t) = replica.get_task(uuid).unwrap() {
        let _ = t.set_status(taskchampion::Status::Pending, &mut ops);
        for (key, value) in map {
            match key.as_str() {
                "description" => {
                    let _ = t.set_description(value, &mut ops);
                }
                "due" => {
                    let _ = t.set_due(parse_datetime(&value), &mut ops);
                }
                "start" => {
                    if value == "stop" {
                        let _ = t.stop(&mut ops);
                    } else {
                        let _ = t.start(&mut ops);
                    }
                }
                "wait" => {
                    let _ = t.set_wait(parse_datetime(&value), &mut ops);
                }
                "priority" => {
                    let _ = t.set_priority(value, &mut ops);
                }
                "tags" => {
                    let existing_tags: Vec<String> = t
                        .get_taskmap()
                        .iter()
                        .filter_map(|(k, _)| k.strip_prefix("tag_").map(|s| s.to_string()))
                        .collect();
                    for tag_name in existing_tags {
                        println!("removing tag at rust side {}", tag_name);
                        let mut tag = Tag::from_str(&tag_name).unwrap();
                        let _ = t.remove_tag(&mut tag, &mut ops);
                    }

                    for part in value.split_whitespace() {
                        println!("tag at rust side {}", part);
                        let mut tag = Tag::from_str(part).unwrap();
                        let _ = t.add_tag(&mut tag, &mut ops);
                    }
                }
                "project" => {
                    let _ = t.set_value("project", Some(value), &mut ops);
                }
                "status" => {
                    let status = match value.as_str() {
                        "pending" => taskchampion::Status::Pending,
                        "completed" => taskchampion::Status::Completed,
                        "deleted" => taskchampion::Status::Deleted,
                        _ => taskchampion::Status::Pending,
                    };
                    // print!("status at rust side {}", value);
                    println!("status at rust side {}", value);
                    let _ = t.set_status(status, &mut ops);
                }
                _ => {}
            }
        }
        replica.commit_operations(ops).unwrap();
    }
    0
}

#[frb]
pub fn add_task(taskdb_dir_path: String, map: HashMap<String, String>) -> i8 {
    let taskdb_dir = PathBuf::from(taskdb_dir_path);
    let storage = StorageConfig::OnDisk {
        taskdb_dir,
        create_if_missing: true,
        access_mode: taskchampion::storage::AccessMode::ReadWrite,
    }
    .into_storage()
    .unwrap();

    let mut replica = Replica::new(storage);
    let mut ops = Operations::new();
    if let Some(uuid_str) = map.get("uuid") {
        let uuid = Uuid::parse_str(&uuid_str).unwrap();
        let mut t = replica.create_task(uuid, &mut ops).unwrap();

        let _ = t.set_status(taskchampion::Status::Pending, &mut ops);

        for (key, value) in map {
            match key.as_str() {
                "description" => {
                    let _ = t.set_description(value, &mut ops);
                }
                "due" => {
                    let _ = t.set_due(parse_datetime(&value), &mut ops);
                }
                "start" => {
                    let _ = t.start(&mut ops);
                }
                "wait" => {
                    let _ = t.set_wait(parse_datetime(&value), &mut ops);
                }
                "priority" => {
                    let _ = t.set_priority(value, &mut ops);
                }
                "tags" => {
                    for part in value.split_whitespace() {
                        let mut tag = Tag::from_str(part).unwrap();
                        let _ = t.add_tag(&mut tag, &mut ops);
                    }
                }
                "project" => {
                    let _ = t.set_user_defined_attribute("project", value, &mut ops);
                }
                _ => {}
            }
        }
        replica.commit_operations(ops).unwrap();
        return 0;
    }
    1
}

#[frb]
pub async fn sync(
    taskdb_dir_path: String,
    url: String,
    client_id: String,
    encryption_secret: String,
) -> i8 {
    let taskdb_dir = PathBuf::from(taskdb_dir_path);
    let storage = StorageConfig::OnDisk {
        taskdb_dir,
        create_if_missing: true,
        access_mode: taskchampion::storage::AccessMode::ReadWrite,
    }
    .into_storage()
    .unwrap();

    let mut replica = Replica::new(storage);
    let config = ServerConfig::Remote {
        url: url.into(),
        client_id: Uuid::parse_str(&client_id).unwrap(),
        encryption_secret: encryption_secret.into(),
    };

    let mut server = config.into_server().unwrap();
    replica.sync(&mut server, false).unwrap();
    0
}

#[cfg(test)]
fn create_test_taskdb() -> (std::path::PathBuf, String) {
    use std::{env, fs};

    let tmp = env::temp_dir().join(format!("taskdb_test_{}", Uuid::new_v4()));
    fs::create_dir_all(&tmp).expect("create temp taskdb dir");
    let taskdb_path = tmp.to_string_lossy().into_owned();
    (tmp, taskdb_path)
}

#[test]
fn test_add_task_with_tags() {
    use std::{collections::HashMap, fs};

    let (tmp, taskdb_path) = create_test_taskdb();

    let mut map: HashMap<String, String> = HashMap::new();
    let uuid = Uuid::new_v4().to_string();
    map.insert("uuid".to_string(), uuid.clone());
    map.insert("description".to_string(), "test task".to_string());
    map.insert("tags".to_string(), "tag1 tag2".to_string());

    let res = add_task(taskdb_path.clone(), map);
    assert_eq!(res, 0);

    let json = get_all_tasks_json(taskdb_path.clone()).expect("get_all_tasks_json");
    let tasks: Vec<HashMap<String, String>> = serde_json::from_str(&json).expect("parse json");
    let found = tasks
        .into_iter()
        .find(|m| m.get("uuid").map(|s| s == &uuid).unwrap_or(false));
    assert!(found.is_some(), "task with uuid not found");
    let task = found.unwrap();
    let tags = task.get("tags").map(|s| s.as_str()).unwrap_or("");
    assert!(tags.contains("tag1"), "tag1 missing in tags: {}", tags);
    assert!(tags.contains("tag2"), "tag2 missing in tags: {}", tags);

    fs::remove_dir_all(&tmp).ok();
}

#[test]
fn test_query_task_matches_exact_tags() {
    use std::{collections::HashMap, fs};

    let (tmp, taskdb_path) = create_test_taskdb();

    let task_one_uuid = Uuid::new_v4().to_string();
    let mut task_one: HashMap<String, String> = HashMap::new();
    task_one.insert("uuid".to_string(), task_one_uuid.clone());
    task_one.insert("description".to_string(), "homework task".to_string());
    task_one.insert("tags".to_string(), "homework urgent".to_string());
    task_one.insert("project".to_string(), "school".to_string());
    assert_eq!(add_task(taskdb_path.clone(), task_one), 0);

    let task_two_uuid = Uuid::new_v4().to_string();
    let mut task_two: HashMap<String, String> = HashMap::new();
    task_two.insert("uuid".to_string(), task_two_uuid.clone());
    task_two.insert("description".to_string(), "work task".to_string());
    task_two.insert("tags".to_string(), "work urgent".to_string());
    task_two.insert("project".to_string(), "office".to_string());
    assert_eq!(add_task(taskdb_path.clone(), task_two), 0);

    let json = query_task(
        taskdb_path.clone(),
        None,
        Some("pending".to_string()),
        Some("+work -homework".to_string()),
        None,
    )
    .expect("query_task");
    let tasks: Vec<HashMap<String, String>> = serde_json::from_str(&json).expect("parse json");

    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].get("uuid"), Some(&task_two_uuid));

    fs::remove_dir_all(&tmp).ok();
}

#[test]
fn test_query_task_filters_by_project_and_status() {
    use std::{collections::HashMap, fs};

    let (tmp, taskdb_path) = create_test_taskdb();

    let alpha_uuid = Uuid::new_v4().to_string();
    let mut alpha_task: HashMap<String, String> = HashMap::new();
    alpha_task.insert("uuid".to_string(), alpha_uuid.clone());
    alpha_task.insert("description".to_string(), "alpha task".to_string());
    alpha_task.insert("project".to_string(), "alpha".to_string());
    assert_eq!(add_task(taskdb_path.clone(), alpha_task), 0);

    let beta_uuid = Uuid::new_v4().to_string();
    let mut beta_task: HashMap<String, String> = HashMap::new();
    beta_task.insert("uuid".to_string(), beta_uuid);
    beta_task.insert("description".to_string(), "beta task".to_string());
    beta_task.insert("project".to_string(), "beta".to_string());
    assert_eq!(add_task(taskdb_path.clone(), beta_task), 0);

    let mut update_map: HashMap<String, String> = HashMap::new();
    update_map.insert("status".to_string(), "completed".to_string());
    assert_eq!(
        update_task(alpha_uuid.clone(), taskdb_path.clone(), update_map),
        0
    );

    let json = query_task(
        taskdb_path.clone(),
        None,
        Some("completed".to_string()),
        None,
        Some("alpha".to_string()),
    )
    .expect("query_task");
    let tasks: Vec<HashMap<String, String>> = serde_json::from_str(&json).expect("parse json");

    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].get("uuid"), Some(&alpha_uuid));

    fs::remove_dir_all(&tmp).ok();
}
