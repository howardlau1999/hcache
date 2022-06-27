# 天池 KV 缓存竞赛

竞赛地址：<https://tianchi.aliyun.com/competition/entrance/531982/information>

## 要点

- 计分公式：$\mathrm{score} = \max\\{\frac{10}{T}, 10\\}+10n+(\frac{50k\cdot\mathrm{qps}}{10000})+500n+\frac{50m}{\mathrm{time\\_ delay}}$，其中，$T$ 为加载完成的时间，$n$ 为评测的接口个数（正确性），$k$ 是需要进行评测 QPS 的接口个数（接口性能），$m$ 是需要评测的延时接口个数（接口延迟）。
- 最终只需要提交一个 ECS 镜像，也就是虚拟机镜像，只要能在 8080 端口提供 HTTP 服务就行
- 制作好的镜像需要共享给 1828723137315221，在北京地区
- 允许使用任意系统镜像和启动脚本，也就是内核以及参数也是可以修改的
- 允许数据丢失，但是数据丢失越多分数越低
- 如果错误太多无法通过基本测试则没有分数

## 基本思路

实际上是实现两个不同的系统，一个分布式 KV 和一个有序集合。对于分布式 KV，由于不要求范围查询，可以使用哈希索引，也可以使用基数树，最简单的是 Bitcask，RocksDB 有非常多的优化，所以虽然它是有序的，我们也可以以 RocksDB 为参照。对于有序集合，必须使用 O(log N) 的数据结构，在数据插入删除的时候保证顺序。有序集合需要能根据 value 来查询对应的分数，这样才可以在修改分数的时候快速找到对应的分数集合。设计数据结构的时候，需要考虑如何充分利用 Cache 以及挖掘硬件上的并行度。另外，需要考虑持久化的需求，设计落盘的数据结构的时候，尽可能提高加载速度。

具体工程细节上，计划使用 Thread Per Core 和 io-uring 来实现，使用异步编程的方式提高并发度。更进一步，将不同的 Key 分区到不同的 Core 上去执行。当然也可以全局共享一个数据结构，但这样可能会导致激烈的竞争。另外，还可以使用 DPDK 和 SPDK 来将网络和 IO 操作移到用户态去做。

## API

需要提供以下 HTTP API：

- `GET /init`，返回 200 以及 Body 为 `ok` 这个字符串表示启动完成，返回 400 表示启动失败
- `GET /query/{key}`，获取 Value。返回 200 以及 Body 为 `key` 对应的 `value`，返回 404 表示没有对应的 `key`
- `GET /del/{key}`，删除 KV。返回 200 表示删除成功，不管 `key` 是否存在
- `POST /add`，添加一个 KV。请求体为 JSON，格式是 `{"key": "foo", "value": "bar"}`，返回 200 表示设置成功，**如果之前这个 `key` 是 zset，直接覆盖**，其他情况返回 400 表示设置失败
- `POST /batch`，批量添加 KV。请求体是 JSON 数组，格式是 `[{"key": "foo", "value": "bar"}, {"key": "bar", "value": "foo"}]`，返回 200 表示设置成功，返回 400 表示设置失败
- `POST /list`，批量获取 KV。请求体是 JSON 字符串数组，表示需要获取的 `key`，格式是 `["key1", "key2"]`，返回 200 以及 Body 为 JSON 数组，格式是 `[{"key": "foo", "value": "bar"}, {"key": "bar", "value": "foo"}]`，返回 404 表示获取失败
- `POST /zadd/{key}`，将请求体的 `score` 和 `value` 添加到 `key` 指定的 zset 中，如果 `value` 已存在则更新 `score`。请求体为 JSON，格式是 `{"score": 1, "value": "foo"}`，返回 200 表示设置成功，**如果这个 `key` 已经被 `add` 过了返回 400 表示设置失败**
- `GET /zrmv/{key}/{value}`，将 `value` 从 `key` 指定的 zset 中删除。返回 200 表示删除成功，`key` 或者 `value` 不存在也返回 200
- `POST /zrange/{key}`，获取范围内的所有 `score` 以及对应的所有 `value`。请求体为 JSON，格式是 `{"min_score": 0, "max_score": 10}`，**含两端**，返回 200 以及 Body 为 JSON 数组，格式是 `[{"score": 1, "value": "foo"}, {"score": 1, "value": "bar"}, {"score": 2, value: "baz"}]`，返回 404 表示 zset 不存在

## 开发环境和项目结构

需要 Linux 5.10 以上，用 `rustup` 配置好 Rust 环境之后，直接运行 `cargo build`。在 `target/debug` 下面会编译出两个二进制文件，一个是 `hcache`，是服务器程序，监听 8080 端口，运行需要配置好 `io-uring` 需要的内核参数以及 `sudo` 权限；另一个是 `client` 程序，是客户端程序，用来测试服务器实现是否正确，直接运行就可以了。

`ros` 文件夹是阿里云相关的操作脚本，具体使用方法：

首先是 `lib` 里面写好了两个 Stack，`SubmissionStack` 用来生成提交文件的，`TestStack` 是我们自己部署一台开发机器用来测试和准备镜像的（需要配置好阿里云的 API Key，余额大于 100 人民币）。

运行 `create-test-ecs.sh` 之后会部署开发资源，并且等待资源就绪之后会自动 SSH，使用本地的密钥对，没有的话先 `ssh-keygen` 一个。默认已经装好 Rust 开发环境，但是需要手动克隆代码仓库。

运行 `remote-build.sh` 会自动 SSH 上去拉最新的代码并且编译。在 ECS 编译完成并且测试过之后，可以运行 `pack-image.sh` 关机并打包镜像。因为操作都是异步的，所以有可能操作失败。如果提示实例状态不对的话，等久一点等到实例状态变为 `STOPPED` 后再运行 `pack-image.sh`。

之后会轮询 Image 状态，打包好了会输出 "Image created"，并且最新的镜像信息会保存在 `image.latest.json` 里。这时候可以运行 `destroy-test-ecs.sh` 删掉 ECS，然后可以用 `zip-submission.sh` 打包一个和 Image ID 一样的 zip 文件，提交这个 zip 文件就行了，不需要手动修改 JSON 文件。

等出分之后记得运行 `delete-image.sh` 删掉镜像，不然也是要扣钱的（出分前别删）。所有删除操作都需要确认。

## Change Log

- **2022.06.27(howard)**: 再问了一下工作人员，说是服务 502 了，那应该就是挂了，不知道是不是 panic 了，可能是 RocksDB 不堪重负了，因为已经添加了自启脚本了。添加了一个纯内存实现的 KV，用 Lockfree Cuckoohash 作为 KV 的存储，把 ZSet 相关的哈希表也用 Lockfree Cuckoohash。QPS 终于有分了，score:16966.3373, init_score:1.0000, api_score:630.0000, qps_score:13496.7147, delay_score:2839.6226
- **2022.06.26(howard)**: 添加了 `glommio` 和 `tokio` 运行时，用 io-uring 的运行时好像都会发生卡死的情况，切回默认使用 `tokio` 了，另外调整了一些内核参数，把端口范围和文件数量限制给改大了。但是 QPS 还是 0 分，原因问了工作人员，说是测 QPS 中途 `/query` 接口报了 `Connection refused`，说明程序启动起来了但是中途运行有问题，可能是爆内存，因为 zset 都是存在内存里，和运行时或者内核应该关系不大，本地测 QPS 应该有 5w。修复了 `list` 接口应该返回部分数据的逻辑，只有在全部 key 都查不到才返回 404，API 分有 630 了。 
- **2022.06.25(howard)**: 写了镜像相关的脚本，交了第一版上去，600.05 分，QPS 分为 0，API 分没有拿满（满分应该是 630）。写了客户端 Benchmark，自定义服务器 main。添加逻辑，对普通 KV 进行 zadd 操作的时候报错。
- **2022.06.24(howard)**: 第一版，用 `monoio` 作为异步运行时，使用 RocksDB 作为点查的引擎，用内存的数据结构来保存 zset 也就是有序集合，具体来说，每一个 zset 有两个数据结构，一个是将 value 映射到分数的哈希表，一个是将分数映射到 value 集合的 B 树。写了验证正确性的客户端。

## TODO

- [ ] 持久化 zset  
- [ ] 手动解析 HTTP 请求和构造 HTTP 响应  
- [ ] 使用 DPDK 来实现网络栈  

## 环境配置

建议现在本地调试好了再去云服务器调试。

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
sudo apt install -y jq
curl -o- https://aliyuncli.alicdn.com/aliyun-cli-linux-latest-amd64.tgz | tar zxvf - 
chmod +x aliyun && sudo mv aliyun /usr/bin
curl -o- https://raw.githubusercontent.com/nvm-sh/nvm/v0.39.1/install.sh | bash
nvm install 18
npm config set registry https://npmmirror.com
npm install -g lerna typescript @alicloud/ros-cdk-cli 
cd ros && npm install
```

运行 `aliyun configure` 配置 API 密钥（去阿里云控制台查，右上角头像下拉菜单 AccessKey 管理），区域填 `cn-beijing`。然后在 `ros` 目录下运行 `ros-cdk load-config` 导入 API 配置。 

之后进入 `ros` 文件夹运行 `ros-cdk synth --json`，输出的就是需要提交的 JSON 文件。运行 `ros-cdk deploy TestStack` 就可以部署测试用的 ECS 了。 
