use kovi::utils::load_toml_data;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Serialize, Deserialize)]
pub struct FreqCon {
    pub enable: bool,
    pub min_msg_gap: u64,
    pub fast_ban_time: usize,
}

#[derive(Serialize, Deserialize)]
pub struct RepCon {
    pub enable: bool,
    pub min_repeat_gap: u64,
}

#[derive(Serialize, Deserialize)]
pub struct Config {
    pub admins: Vec<i64>,
    pub manage_groups: Vec<i64>,

    pub freq: FreqCon,
    pub repeat: RepCon,
}

pub fn load_config(data_path: &PathBuf) -> Config {
    let config_path = data_path.join("config.toml");

    let default_config = Config {
        admins: vec![],
        manage_groups: vec![],
        freq: FreqCon {
            enable: true,
            min_msg_gap: 120,
            fast_ban_time: 300,
        },
        repeat: RepCon {
            enable: true,
            min_repeat_gap: 3600,
        },
    };

    load_toml_data(default_config, &config_path).unwrap()
}
