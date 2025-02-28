pub struct Config;

impl Config {

    ///需要管理的群
    pub const MANAGE_GROUPS: [i64; 0] = [];
    /// 最小发消息间隔，低于此间隔禁言
    pub const MIN_MSG_GAP: u64 = 120;
    /// 低于间隔的禁言时间
    pub const OF_BAN_TIME: usize = 300;

    /// 重复消息检测范围
    pub const MIN_REPEAT_GAP: u64 = 3600;

    /// 重复消息池持久化保存（建议每天凌晨清空否则可能过大）
    /// 设为 "" 将关闭保存
    pub const REPEAT_SAVE: &'static str = "./plugins/big_bro/save/repeat.dat";

    /// bot 管理员
    pub const ADMIN: [i64; 0] = [];
}