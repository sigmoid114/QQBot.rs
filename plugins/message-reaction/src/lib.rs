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
    let bot = P::get_runtime_bot();
    let data_path = bot.get_data_path().join("data.json");
    match load_json_data(Data::new(), &data_path) {
        Ok(data) => {
            let data = Arc::new(RwLock::new(data));
            P::on_admin_msg({
                let data = data.clone();
                move |event| on_admin_msg(event, data.clone(), data_path.clone())
            });
            P::on_group_msg({
                let data = data.clone();
                move |event| on_group_msg(event, data.clone(), bot.clone())
            });
        }
        Err(e) => {
            println!("插件 message-reaction 启动失败：{}", e);
        }
    }
}

async fn on_admin_msg(event: Arc<AdminMsgEvent>, data: Arc<RwLock<Data>>, data_path: PathBuf) {
    if let Some(text) = event.borrow_text() {
        let cmd: String = text.chars().filter(|c| !c.is_whitespace()).collect();
        if let Some(caps) = CMD_REG.captures(&cmd) {
            let mut data = data.write().await;
            if &caps[2] == "+" {
                data.groups.insert(String::from(&caps[1]));
            } else {
                data.groups.remove(&caps[1]);
            }
            save_json_data(&*data, data_path).unwrap();
        }
    }
}

async fn on_group_msg(event: Arc<GroupMsgEvent>, data: Arc<RwLock<Data>>, bot: Arc<RuntimeBot>) {
    let group_id = event.data.group.group_id;
    if {
        let data = data.read().await;
        data.groups.contains(&group_id.to_string())
    } {
        let message_seq = event.data.message_seq;
        for segment in event.data.segments.clone() {
            match segment.type_.as_str() {
                "face" => {
                    if let Some(face_id) = segment.data.get("face_id")
                        && let Some(reaction) = face_id.as_str()
                    {
                        bot.send_group_message_reaction(
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
                                bot.send_group_message_reaction(
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
