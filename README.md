# 天池 KV 缓存竞赛

竞赛地址：<https://tianchi.aliyun.com/competition/entrance/531982/information>

## 要点

- 计分公式：$\mathrm{score} = \max\\{\frac{10}{T}, 10\\}+10n+(\frac{50k\cdot\mathrm{qps}}{10000})+500n+\frac{50m}{\mathrm{time\\_ delay}}$，其中，$T$ 为加载完成的时间，$n$ 为评测的接口个数，$k$ 是需要进行评测 QPS 的接口个数，$m$ 是需要评测的延时接口个数。
- 最终只需要提交一个 ECS 镜像，也就是虚拟机镜像，只要能提供 HTTP 服务就行
- 允许使用任意系统镜像和启动脚本，也就是内核以及参数也是可以修改的
- 允许数据丢失，但是数据丢失越多分数越低
- 如果错误太多无法通过基本测试则没有分数

## API

需要提供以下 HTTP API：

- `GET /init`，返回 200 以及 Body 为 `ok` 这个字符串表示启动完成，返回 400 表示启动失败
- `GET /query/{key}`，获取 Value。返回 200 以及 Body 为 `key` 对应的 `value`，返回 404 表示没有对应的 `key`
- `GET /del/{key}`，删除 KV。返回 200 表示删除成功，不管 `key` 是否存在
- `POST /add`，添加一个 KV。请求体为 JSON，格式是 `{"key": "foo", "value": "bar"}`，返回 200 表示设置成功，返回 400 表示设置失败
- `POST /batch`，批量添加 KV。请求体是 JSON 数组，格式是 `[{"key": "foo", "value": "bar"}, {"key": "bar", "value": "foo"}]`，返回 200 表示设置成功，返回 400 表示设置失败
- `POST /list`，批量获取 KV。请求体是 JSON 字符串数组，表示需要获取的 `key`，格式是 `["key1", "key2"]`，返回 200 以及 Body 为 JSON 数组，格式是 `[{"key": "foo", "value": "bar"}, {"key": "bar", "value": "foo"}]`，返回 404 表示获取失败
- `POST /zadd/{key}`，将请求体的 `score` 和 `value` 添加到 `key` 指定的 zset 中，如果 `value` 已存在则更新 `score`。请求体为 JSON，格式是 `{"score": 1, "value": "foo"}`，返回 200 表示设置成功，返回 400 表示设置失败
- `GET /zrmv/{key}/{value}`，将 `value` 从 `key` 指定的 zset 中删除。返回 200 表示删除成功，`key` 或者 `value` 不存在也返回 200
- `POST /zrange/{key}`，获取范围内的所有 `score` 以及对应的所有 `value`。请求体为 JSON，格式是 `{"min_score": 0, "max_score": 10}`，**含两端**，返回 200 以及 Body 为 JSON 数组，格式是 `[{"score": 1, "value": "foo"}, {"score": 1, "value": "bar"}, {"score": 2, value: "baz"}]`，返回 404 表示 zset 不存在

## 开发环境和项目结构

需要 Linux 5.10 以上，用 `rustup` 配置好 Rust 环境之后，直接运行 `cargo build`。在 `target/debug` 下面会编译出两个二进制文件，一个是 `hcache`，是服务器程序，监听 8080 端口，运行需要配置好 `io-uring` 需要的内核参数以及 `sudo` 权限；另一个是 `client` 程序，是客户端程序，用来测试服务器实现是否正确，直接运行就可以了。

`ros` 文件夹是竞赛要求提交的 ROS 配置文件的生成器，我们只需要改里面的 `ecsImageId` 的默认值就可以了。`UserData` 里面的脚本是启动脚本，可以修改。

按照竞赛的要求，我们需要开一个 ECS 实例，把我们的可执行文件上传上去，然后去控制台制作一个镜像得到镜像的 ID，填到配置生成器里面然后运行 `ros-cdk sync --json`，输出的就是需要提交的 JSON 文件，也可以在 `ros/cdk.out/RosStack.template.json` 文件里面看到。

## Change Log

- **2022.06.24(howard)**: 第一版，用 `monoio` 作为异步运行时，使用 RocksDB 作为点查的引擎，用内存的数据结构来保存 zset 也就是有序集合，具体来说，每一个 zset 有两个数据结构，一个是将 value 映射到分数的哈希表，一个是将分数映射到 value 集合的 B 树。

## TODO

- [ ] 持久化 zset  
- [ ] 手动解析 HTTP 请求和构造 HTTP 响应  
- [ ] 使用 DPDK 来实现网络栈  

## 环境配置

### Rust

```bash
# Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

配置清华源，将下面的内容保存到 `~/.cargo/config`

```toml
[source.crates-io]
replace-with = 'tuna'

[source.tuna]
registry = "https://mirrors.tuna.tsinghua.edu.cn/git/crates.io-index.git"
```

配置完成之后，运行 `cargo build` 检查是否能正常编译。

### 阿里云命令行工具

```bash
curl -o- https://raw.githubusercontent.com/nvm-sh/nvm/v0.39.1/install.sh | bash
nvm install 18
npm config set registry https://npmmirror.com
npm install -g lerna typescript @alicloud/ros-cdk-cli 
cd ros && npm install
```

之后进入 `ros` 文件夹运行 `ros-cdk synth --json`，输出的就是需要提交的 JSON 文件。
