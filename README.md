# big_bro - 频率控制插件

基于Kovi机器人框架的一款频率控制插件。

__Big Bro is watching you all!__

Made by Freude, DeepSeek, Gemini, Copilot (GPT 4o), and You.

感谢无私群友们的帮助。

## 使用

1. 下载项目文件，添加到您Kovi的plugins目录下
2. 将 `./big_bro/src/lib.rs` 第 3~5 行的 my_config 改成 config。(my_config是我本地使用的配置，给您造成不便请谅解)
3. 在主项目的Cargo.toml中添加
```toml
[dependencies]
big_bro = { version = "0.1.0", path = "plugins/big_bro" }


[workspace]
members = ["plugins/big_bro"]
```
4. 修改 `./big_bro/src/config.rs`，添加要管理的群，酌情修改其它配置。
5. 酌情给机器人添加管理员。


## 原理

如您所见，`lib.rs`的书写相当的小作坊，如果您能将其模块化，我将不胜感激。

由于我非常的偷懒，目前频率控制的方式是控制群友两条消息之间的间隔。其实这也非常好改：用个VecDeque就好。

查重的方法是把消息体的前5条都丢crc32里。
crc32的地址空间已经足够大了，在10000条消息的情况下碰撞率在1%左右，对于绝大多数用途来说够了。

## License

This project follows CC-BY-NC 4.0 License.
