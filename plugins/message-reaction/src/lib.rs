use emoji_manager::is_emoji;
use kovi::{
    PluginBuilder as P, RuntimeBot,
    tokio::sync::RwLock,
    utils::{load_json_data, save_json_data},
};
use kovi_milky::*;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    path::PathBuf,
    sync::{Arc, LazyLock},
};

static CMD_REG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\$(.*)([+-])=message[-_]reaction$").unwrap());
static BOT: LazyLock<Arc<RuntimeBot>> = LazyLock::new(|| P::get_runtime_bot());
static DATA_PATH: LazyLock<PathBuf> = LazyLock::new(|| BOT.get_data_path().join("data.json"));
static DATA: LazyLock<RwLock<Data>> =
    LazyLock::new(|| match load_json_data(Data::new(), DATA_PATH.clone()) {
        Ok(data) => RwLock::new(data),
        Err(e) => {
            println!("插件 message-reaction 数据格式错误(自动清空)：{}", e);
            save_json_data(&Data::new(), DATA_PATH.clone()).unwrap();
            RwLock::new(Data::new())
        }
    });

#[derive(Serialize, Deserialize)]
struct Data {
    pub groups: HashSet<String>,
}

impl Data {
    fn new() -> Self {
        Self {
            groups: HashSet::new(),
        }
    }
}

#[kovi::plugin]
async fn main() {
    let _ = &*BOT;
    P::on_admin_msg(move |event| on_admin_msg(event));
    P::on_group_msg(move |event| on_group_msg(event));
}

async fn on_admin_msg(event: Arc<AdminMsgEvent>) {
    if let Some(text) = event.borrow_text() {
        let cmd: String = text.chars().filter(|c| !c.is_whitespace()).collect();
        if let Some(caps) = CMD_REG.captures(&cmd) {
            let mut data = DATA.write().await;
            if &caps[2] == "+" {
                data.groups.insert(String::from(&caps[1]));
            } else {
                data.groups.remove(&caps[1]);
            }
            save_json_data(&*data, &*DATA_PATH).unwrap();
        }
    }
}

async fn on_group_msg(event: Arc<GroupMsgEvent>) {
    let group_id = event.data.group.group_id;
    if {
        let data = DATA.read().await;
        data.groups.contains(&group_id.to_string())
    } {
        let message_seq = event.data.message_seq;
        for segment in event.data.segments.clone() {
            match segment.type_.as_str() {
                "face" => {
                    if let Some(face_id) = segment.data.get("face_id")
                        && let Some(reaction) = face_id.as_str()
                    {
                        BOT.send_group_message_reaction(
                            group_id,
                            message_seq,
                            reaction,
                            "face",
                            true,
                        );
                    }
                }
                "text" => {
                    if let Some(text) = segment.data.get("text")
                        && let Some(text) = text.as_str()
                    {
                        for c in text.chars() {
                            if is_emoji(c).await {
                                BOT.send_group_message_reaction(
                                    group_id,
                                    message_seq,
                                    &(c as u32).to_string(),
                                    "emoji",
                                    true,
                                );
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
}
