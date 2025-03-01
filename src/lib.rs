mod config;
use crate::config::Config;

use crc32fast::Hasher;

use kovi::{
    MsgEvent, PluginBuilder as plugin, RuntimeBot, bot::AccessControlMode::WhiteList,
    bot::runtimebot::kovi_api::SetAccessControlList, serde_json::Value,
    tokio::sync::Mutex as AMutex,
};

use kovi_plugin_expand_napcat::NapCatApi;

use chrono::Timelike;
use kovi::error::BotError;
use kovi::tokio::time::sleep;
use std::path::PathBuf;
use std::time::Duration;
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
                // println!("{}", self);
                None
            }
        } else {
            None
        }
    }
}

async fn calculate_image_hash(url: &str) -> u32 {
    // 处理QQ图片链接，hash其file_id以确保唯一性
    // we process the image url and hash its file_id to make sure it's unique

    let mut hasher = Hasher::new();

    // let mut file = UNMANAGED_LOG.lock().await;
    // file.write_all(format!("[image] {}\n", url).as_bytes()).unwrap();
    if url.len() > 148 {
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
                    // Follow API, there must is text so unwrap is safe
                    hasher.update(value.get_string("text").unwrap().as_bytes());
                }
                "image" => {
                    hasher.update(
                        &calculate_image_hash(value.get_string("url").unwrap())
                            .await
                            .to_le_bytes(),
                    );
                }
                "video" => {
                    let url = value.get_string("url").unwrap();
                    // hasher.update(&get_image_hash(url).to_le_bytes());
                    if url.as_bytes()[0] == b'/' {
                        hasher.update(&url.as_bytes()[70..]);
                        // 在当前消息中找到了一个本地视频，必重复，立即return
                        return Some(1);
                    } else {
                        hasher.update(&url.as_bytes());
                    }
                }
                "forward" => {
                    // FIXME: 这里获取消息怎么老报错呢？
                    let forward_node = value.get_string("id").unwrap();
                    let forward_pms = bot.get_forward_msg(forward_node).await.unwrap();
                    let mut pms = forward_pms.data.get_vec("messages");
                    if pms.is_none() {
                        pms = value.get_vec("content");
                        if pms.is_none() {
                            println!("获取转发消息失败！消息id: {}", forward_node);
                            let mut file = UNMANAGED_LOG.lock().await;
                            file.write_all(format!("[forward] {}\n", msg).as_bytes())
                                .unwrap();
                            hasher.update(&forward_node.as_bytes());
                            continue;
                        }
                    }
                    let mut pms = pms.unwrap().iter();
                    if pms.len() == 1 {
                        // 只有一条消息的合并转发，哈希以里面的合并转发为准
                        return hash_message_content(bot, pms.next().unwrap(), group_id, t + 1)
                            .await;
                    }
                    for pm in pms.take(5) {
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

fn load_last_duplicate(data_path: &PathBuf) -> HashMap<i64, SystemTime> {
    let file = data_path.join("last_duplicate.json");
    let default = HashMap::<i64, SystemTime>::new();

    kovi::utils::load_json_data(default, &file).unwrap()
}

// type MsgExt = Box<dyn Fn(&RuntimeBot, &Config, &MsgEvent) -> (dyn Future<Output = ()> + Send)>;
// type ExtCleanup = Box<dyn Fn(&RuntimeBot, &Config) -> Box<dyn Future<Output = ()> + Send>>;

const PLUGIN_NAME: &str = "big_bro";

fn init(bot: &RuntimeBot, config: &Config) -> Result<(), BotError> {
    bot.set_plugin_access_control_mode(PLUGIN_NAME, WhiteList)?;
    bot.set_plugin_access_control(PLUGIN_NAME, true)?;
    bot.set_plugin_access_control_list(
        PLUGIN_NAME,
        true,
        SetAccessControlList::Changes(config.manage_groups.clone()),
    )?;
    bot.set_plugin_access_control_list(
        PLUGIN_NAME,
        false,
        SetAccessControlList::Changes(config.admins.clone()),
    )?;
    Ok(())
}

#[kovi::plugin]
async fn big_bro_main() {
    // 主逻辑

    let bot_main = plugin::get_runtime_bot();

    let data_path = bot_main.get_data_path();

    let config = config::load_config(&data_path);

    init(&bot_main, &config).unwrap();

    println!("big_bro 已加载");

    let last_reply_main = Arc::new(AMutex::new(load_last_duplicate(&data_path))); // 每个人上次回复的时间
    let last_duplicate_main = Arc::new(AMutex::new(HashMap::<u32, SystemTime>::new())); // 重复哈希的时间戳

    let bot = bot_main.clone();
    let last_reply = last_reply_main.clone();
    let last_duplicate = last_duplicate_main.clone();
    plugin::on_group_msg(move |msg| {
        let bot = bot.clone();
        let last_reply = last_reply.clone();
        let last_duplicate = last_duplicate.clone();
        async move {
            let group_id = msg.group_id.unwrap();
            let sender_id = msg.user_id;
            if msg.message_type == "private" {
                // 过滤临时会话
                return;
            };
            {
                // 防刷屏部分
                let mut reply = last_reply.lock().await;
                if let Some(time) = reply.get(&sender_id) {
                    if time.elapsed().unwrap().as_secs() < config.freq.min_msg_gap {
                        bot.set_group_ban(group_id, sender_id, config.freq.fast_ban_time);
                    }
                };
                reply.insert(sender_id, SystemTime::now());
            }

            let mut pms = msg.message.iter();

            if pms.len() == 1 {
                let pm = pms.next().unwrap();
                if pm.type_ == "text" {
                    let text = pm.data.get_string("text").unwrap();
                    println!("{}", text.len());
                    if text.len() < 20 || text.len() > 400 {
                        bot.set_group_ban(group_id, sender_id, 3600);
                        return; // 太短或者太长的消息直接ban，懒得跟你玩了
                    }
                    if text.len() < 100 && text.contains("禁言我") {
                        bot.set_group_ban(group_id, sender_id, 3600);
                        msg.reply("满足你😋");
                        return; // 满足你
                    }
                } else if pm.type_ == "video" {
                    let url = pm.data.get_string("url").unwrap();
                    if url.as_bytes()[0] == b'/' {
                        bot.set_msg_emoji_like(msg.message_id as i64, "146")
                            .await
                            .unwrap();
                        return; // 本地视频反馈
                    }
                }
            }

            {
                // 查重部分
                let hash = calculate_msg_hash(&bot, &msg).await.unwrap();

                let mut lock2 = last_duplicate.lock().await;
                if let Some(time) = lock2.get(&hash) {
                    let gap = time.elapsed().unwrap().as_secs();
                    if gap < 30 {
                        // 复读直接撤回
                        bot.delete_msg(msg.message_id);
                        bot.set_group_ban(msg.group_id.unwrap(), msg.sender.user_id, 3600);
                        msg.reply("一天天复读复读，沙了😅");
                        return;
                    }
                    if gap < config.repeat.min_repeat_gap {
                        // TODO: 查到重了怎么办
                        // 目前我们选择回复 angry line 小表情
                        // bot.delete_msg(msg.message_id); // 也可以改成撤回消息，但是crc32 hash有较小概率重复
                        bot.set_msg_emoji_like(msg.message_id as i64, "146")
                            .await
                            .unwrap();
                        // 146: angry line
                        // 162: clock
                        lock2.insert(hash, SystemTime::now()); // 因为现在还没撤回，所以更新最后发现时间
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
            if msg.get_text() == "/save" {
                msg.reply("收到，正在重启插件");
                bot.restart_plugin(PLUGIN_NAME).await.unwrap();
            }
        }
    });

    let bot = bot_main.clone();
    let last_duplicate = last_duplicate_main.clone();
    let data_path = data_path.clone();
    let manage_groups = config.manage_groups.clone();
    plugin::drop(move || {
        let bot = bot.clone();
        let last_duplicate = last_duplicate.clone();
        let data_path = data_path.clone();
        let manage_groups = manage_groups.clone();
        async move {
            let local_time = chrono::Local::now();
            if local_time.hour() == 0 {
                for group_id in manage_groups {
                    bot.send_group_msg(group_id, "宵禁了哈");
                    sleep(Duration::from_secs(1)).await;
                    bot.send_group_msg(group_id, "群公告里有发言规则和项目地址之类的，自己看。");
                    sleep(Duration::from_secs(1)).await;
                    bot.set_group_whole_ban(group_id, true);
                }
            }
            if config.repeat.enable {
                if local_time.hour() == 0 {
                    // 丢弃前一天的查重记录
                    let file = data_path.join("last_duplicate.json");
                    kovi::utils::save_json_data(&HashMap::<u32, SystemTime>::new(), &file).unwrap();
                    println!("已清空查重记录。");
                } else {
                    let mut last = last_duplicate.lock().await;
                    let last = std::mem::take(&mut *last);
                    let file = data_path.join("last_duplicate.json");
                    kovi::utils::save_json_data(&last, &file).unwrap();
                    println!("保存了查重记录，您可以继续调试了。");
                }
            }
            println!("[Done] 清理完毕。");
        }
    });
}
