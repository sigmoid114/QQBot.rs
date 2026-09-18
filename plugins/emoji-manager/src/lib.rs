use std::{collections::HashSet, path::PathBuf, sync::LazyLock};

use kovi::{
    PluginBuilder as P,
    tokio::sync::RwLock,
    utils::{load_json_data, save_json_data},
};
use kovi_milky::*;
use regex::Regex;

type Data = HashSet<u32>;
static DATA_PATH: LazyLock<PathBuf> = LazyLock::new(|| {
    let bot = P::get_runtime_bot();
    bot.get_data_path().join("data.json")
});
static DATA: LazyLock<RwLock<Data>> =
    LazyLock::new(|| match load_json_data(Data::new(), DATA_PATH.clone()) {
        Ok(data) => RwLock::new(data),
        Err(e) => {
            println!("插件 emoji-manager 数据格式错误(自动清空)：{}", e);
            save_json_data(&Data::new(), DATA_PATH.clone()).unwrap();
            RwLock::new(Data::new())
        }
    });
static CMD_REG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\$emoji([+-])=(.)$").unwrap());

#[kovi::plugin]
async fn main() {
    let _ = &*DATA_PATH;
    P::on_admin_msg(|event| async move {
        if let Some(text) = event.borrow_text() {
            let cmd: String = text.chars().filter(|c| !c.is_whitespace()).collect();
            if let Some(caps) = CMD_REG.captures(&cmd)
                && let Some(code) = caps[2].chars().next()
                && let code = code as u32
            {
                let mut data = DATA.write().await;
                if &caps[1] == "+" {
                    data.insert(code);
                } else {
                    data.remove(&code);
                }
                save_json_data(&*data, DATA_PATH.clone()).unwrap();
            }
        }
    });
}

pub async fn is_emoji(ch: char) -> bool {
    let data = DATA.read().await;
    data.contains(&(ch as u32))
}
