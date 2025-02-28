//! Please caution: This file is typically hand-made sh*t code and maybe hell-hard to read and edit.

mod my_config;
use crate::my_config::Config;

use crc32fast::Hasher;

use kovi::{
    MsgEvent, PluginBuilder as plugin, RuntimeBot, bot::AccessControlMode::WhiteList,
    bot::runtimebot::kovi_api::SetAccessControlList, serde_json::Value,
    tokio::sync::Mutex as AMutex,
};

use kovi_plugin_expand_napcat::NapCatApi;

use std::{
    collections::HashMap,
    fs::File,
    fs::OpenOptions,
    io::Write,
    pin::Pin,
    sync::{Arc, LazyLock},
    time::SystemTime,
};

static UNMANAGED_LOG: LazyLock<AMutex<File>, fn() -> AMutex<File>> = LazyLock::new(|| {
    AMutex::new(
        OpenOptions::new()
            .append(true)
            .create(true)
            .open("./plugins/big_bro/logs/unmanaged.log")
            .unwrap(),
    )
});

trait JsonValueExtract {
    // a simple trait to get value from a json object
    fn get_string(&self, key: &str) -> Option<&String>;
    fn get_vec(&self, key: &str) -> Option<&Vec<Value>>;
}

impl JsonValueExtract for Value {
    fn get_string(&self, key: &str) -> Option<&String> {
        if let Value::Object(map) = self {
            if let Some(Value::String(text)) = map.get(key) {
                Some(text)
            } else {
                None
            }
        } else {
            None
        }
    }

    fn get_vec(&self, key: &str) -> Option<&Vec<Value>> {
        if let Value::Object(map) = self {
            if let Some(Value::Array(list)) = map.get(key) {
                Some(list)
            } else {
                println!("{}", self);
                None
            }
        } else {
            println!("{}", self);
            None
        }
    }
}

fn calculate_image_hash(url: &str) -> u32 {
    // 处理QQ图片链接，hash其file_id以确保唯一性
    // we process the image url and hash its file_id to make sure it's unique

    let mut hasher = Hasher::new();

    println!("[image] {}", url);
    if url.len() > 100 && url.as_bytes()[150] == b'&' {
        hasher.update(&url[59..94].as_bytes());
    } else {
        hasher.update(&url.as_bytes());
    }

    let rv = hasher.finalize();
    rv
}

fn hash_message_content<'a>(
    bot: &'a RuntimeBot,
    person_msg: &'a Value,
    group_id: Option<i64>,
    t: i32,
) -> Pin<Box<dyn Future<Output = Option<u32>> + Send + 'a>> {
    // 处理消息列表，并返回crc32 hash
    Box::pin(async move {
        if t > 5 {
            return None; // too deep
        }
        
        let mut hasher = Hasher::new();
        if let Some(pre) = group_id {
            hasher.update(&pre.to_le_bytes());
        }
        
        let msg_list = person_msg.get_vec("message").unwrap();
        for msg in msg_list {
            let type_ = msg.get_string("type").unwrap();
            let value = msg.get("data").unwrap();
            match type_.as_str() {
                "text" => {
                    if let Some(text) = value.get_string("text") {
                        hasher.update(text.as_bytes());
                        // println!("caught text: {}", text);
                    }
                }
                "image" => {
                    if let Some(url) = value.get_string("url") {
                        let hash = calculate_image_hash(url);
                        hasher.update(&hash.to_le_bytes());
                    }
                }
                "video" => {
                    if let Some(url) = value.get_string("url") {
                        // hasher.update(&get_image_hash(url).to_le_bytes());

                        // put to unmanaged log
                        let mut file = UNMANAGED_LOG.lock().await;
                        file.write_all(format!("[video] {}", url).as_bytes())
                            .unwrap();
                    }
                }
                "forward" => {
                    // FIXME: 这里获取消息怎么老报错呢？
                    let forward_node = value.get_string("id").unwrap();
                    let forward_pms = bot.get_forward_msg(forward_node).await;
                    if let Err(e) = forward_pms {
                        println!("获取转发消息失败！API反馈: {}", e);
                        hasher.update(&forward_node.as_bytes());
                        continue;
                    }
                    let forward_pms = forward_pms.unwrap();
                    let pms = forward_pms.data.get_vec("messages");
                    if pms.is_none() {
                        println!("获取转发消息失败！消息id: {}", forward_node);
                        println!("是否没嵌套在node里，看看下面的value: {}", value);
                        hasher.update(&forward_node.as_bytes());
                        continue;
                    }
                    for pm in pms.unwrap().iter().take(5) {
                        let next = hash_message_content(bot, pm, None, t + 1);
                        if let Some(next) = next.await {
                            hasher.update(&next.to_le_bytes());
                        }
                    }
                }
                "face" => {
                    // ignore all emojis
                }
                "json" => {
                    if let Some(json) = value.get_string("data") {
                        println!("caught json: {}", json);
                    }
                }
                _ => {}
            }
        }
        Some(hasher.finalize())
    })
}

async fn calculate_msg_hash(bot: &RuntimeBot, msg_event: &MsgEvent) -> Option<u32> {
    // 拆开MsgEvent并处理crc32 hash
    // 历史遗留问题
    let group_id = msg_event.group_id.unwrap();

    hash_message_content(bot, &msg_event.original_json, Some(group_id), 0).await
}

fn load_last_duplicate() -> HashMap<i64, SystemTime> {
    // 尝试加载防重复文件
    if Config::REPEAT_SAVE.len() > 0 && std::path::Path::new(Config::REPEAT_SAVE).exists() {
        let file = File::open(Config::REPEAT_SAVE).unwrap();
        bincode::deserialize_from(file).unwrap()
    } else {
        HashMap::new()
    }
}

const PLUGIN_NAME: &str = "big_bro";

#[kovi::plugin]
async fn big_bro_main() {
    // 主逻辑
    let access_groups = Config::MANAGE_GROUPS;

    let bot_main = plugin::get_runtime_bot();

    bot_main
        .set_plugin_access_control_mode(PLUGIN_NAME, WhiteList)
        .unwrap();
    bot_main
        .set_plugin_access_control(PLUGIN_NAME, true)
        .unwrap();
    bot_main
        .set_plugin_access_control_list(
            PLUGIN_NAME,
            true,
            SetAccessControlList::Changes(Vec::from(access_groups.clone())),
        )
        .unwrap();
    bot_main
        .set_plugin_access_control_list(
            PLUGIN_NAME,
            false,
            SetAccessControlList::Changes(Vec::from(Config::ADMIN)),
        )
        .unwrap();

    println!("插件配置已加载");
    
    let last_reply_main = Arc::new(AMutex::new(load_last_duplicate()));// 每个人上次回复的时间
    let last_duplicate_main = Arc::new(AMutex::new(HashMap::<u32, SystemTime>::new())); // 重复哈希的时间戳

    let bot = bot_main.clone();
    let last_reply = last_reply_main.clone();
    let last_duplicate = last_duplicate_main.clone();

    plugin::on_group_msg(move |msg| {
        let bot = bot.clone();
        let last_reply = last_reply.clone();
        let last_duplicate = last_duplicate.clone();
        async move {
            if !access_groups.contains(&msg.group_id.unwrap()) {
                // TODO: 我不确定 plugin 的访问控制是否能生效，所以加判断
                return;
            }

            {
                // 防刷屏部分
                let mut reply = last_reply.lock().await;
                if let Some(time) = reply.get(&msg.sender.user_id) {
                    if time.elapsed().unwrap().as_secs() < Config::MIN_MSG_GAP {
                        bot.set_group_ban(
                            msg.group_id.unwrap(),
                            msg.sender.user_id,
                            Config::OF_BAN_TIME,
                        );
                    }
                };
                reply.insert(msg.sender.user_id, SystemTime::now());
            }

            if msg.get_text() == "/help" {
                // 不许玩机器人
                bot.set_group_ban(msg.group_id.unwrap(), msg.sender.user_id, 3600);
            }

            {
                // 查重部分
                let hash = calculate_msg_hash(&bot, &msg).await.unwrap();

                let mut lock2 = last_duplicate.lock().await;
                if let Some(time) = lock2.get(&hash) {
                    if time.elapsed().unwrap().as_secs() < Config::MIN_REPEAT_GAP {
                        // TODO: 查到重了怎么办
                        // 目前我们选择回复 angry line 小表情

                        // bot.delete_msg(msg.message_id); // 也可以改成撤回消息，但是crc32 hash有较小概率重复
                        bot.set_msg_emoji_like(msg.message_id as i64, "146")
                            .await
                            .unwrap();
                        // 146: angry line
                        // 162: clock
                        println!("caught a repeat message with crc32 hash: {}", hash);
                    }
                } else {
                    lock2.insert(hash, SystemTime::now());
                }
            }
        }
    });

    let bot = bot_main.clone();
    plugin::on_admin_msg(move |msg| {
        let bot = bot.clone();
        async move {
            if msg.get_text() == "/stop" {
                msg.reply("收到，正在停止插件");
                bot.disable_plugin(PLUGIN_NAME).unwrap();
            }
            if msg.get_text() == "/save" {
                msg.reply("收到，正在重启插件");
                bot.restart_plugin(PLUGIN_NAME).await.unwrap();
            }
        }
    });

    // let last_reply = last_reply_main.clone();
    let last_duplicate = last_duplicate_main.clone();
    plugin::drop(move || {
        // let last_reply = last_reply.clone();
        let last_duplicate = last_duplicate.clone();
        async move {
            if Config::REPEAT_SAVE.len() > 0 {
                let mut last = last_duplicate.lock().await;
                let last = std::mem::take(&mut *last);
                let mut file = OpenOptions::new()
                    .write(true)
                    .create(true)
                    .open(Config::REPEAT_SAVE)
                    .unwrap();
                bincode::serialize_into(&mut file, &last).unwrap();
                println!("尝试保存了重复记录，您可以继续调试了。");
            }
            println!("[Done] 清理完毕。");
        }
    });
}
